use std::collections::{HashMap, VecDeque};

use bytes::Bytes;
use godot::classes::multiplayer_peer::{ConnectionStatus, TransferMode};
use godot::classes::{IMultiplayerPeerExtension, MultiplayerPeerExtension};
use godot::global::Error;
use godot::prelude::*;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::{Receiver, Sender, channel};

use crate::IrohRuntime;
use crate::connection::{IrohConnection, IrohListener};

#[derive(GodotClass)]
#[class(tool, no_init, base=MultiplayerPeerExtension)]
struct IrohServer {
    base: Base<MultiplayerPeerExtension>,
    listener: IrohListener,
    accepted_peer_sender: Sender<(i32, IrohConnection)>,
    accepted_peer_receiver: Receiver<(i32, IrohConnection)>,
    refuse_new_connections: bool,
    peers: HashMap<i32, IrohConnection>,
    last_peer_id: i32,
    received_packets: VecDeque<(i32, i32, TransferMode, Bytes)>,
    target_peer_id: i32,
    transfer_channel: i32,
    transfer_mode: TransferMode,
}

#[godot_api]
impl IrohServer {
    /// Starts a server that is listening for incoming connections.
    ///
    /// Other clients can connect to this server by calling the connect function on `IrohClient`
    /// using the connection string returned by the [Self::connection_string] function.
    #[func]
    fn start() -> Gd<Self> {
        let listener = match IrohRuntime::block_on(IrohListener::new()) {
            Ok(listener) => listener,
            Err(error) => panic!("failed to start listening: {error}"),
        };
        let (accepted_peer_sender, accepted_peer_receiver) = channel(32);
        Gd::from_init_fn(|base| Self {
            base,
            listener,
            accepted_peer_sender,
            accepted_peer_receiver,
            refuse_new_connections: false,
            peers: HashMap::new(),
            last_peer_id: 1,
            received_packets: VecDeque::new(),
            transfer_channel: 0,
            transfer_mode: TransferMode::RELIABLE,
            target_peer_id: 0,
        })
    }

    /// Returns the connection string that can be used to connect to this server.
    #[func]
    fn connection_string(&self) -> GString {
        GString::from(self.listener.connection_string())
    }

    /// Connect to an other server using the connection string.
    #[func]
    fn connect(&mut self, connection_string: GString) {
        let node_id = connection_string.to_string();
        let peer_id = {
            self.last_peer_id = (self.last_peer_id + 1) % i32::MAX;
            if self.last_peer_id < 2 {
                self.last_peer_id = 2;
            }
            self.last_peer_id
        };
        let endpoint = self.listener.endpoint.clone();
        let accepted_peer_sender = self.accepted_peer_sender.clone();
        IrohRuntime::spawn(async move {
            let (_, connection) = IrohConnection::connect(endpoint, node_id).await?;
            accepted_peer_sender.send((peer_id, connection)).await?;
            Ok::<(), anyhow::Error>(())
        });
    }
}

#[godot_api]
impl IMultiplayerPeerExtension for IrohServer {
    fn poll(&mut self) {
        // Accept new connections
        while let Ok(connection) = self.listener.receive_connection() {
            let peer_id = {
                self.last_peer_id = (self.last_peer_id + 1) % i32::MAX;
                if self.last_peer_id < 2 {
                    self.last_peer_id = 2;
                }
                self.last_peer_id
            };
            let accepted_peer_sender = self.accepted_peer_sender.clone();
            IrohRuntime::spawn(async move {
                let connection = IrohConnection::accept(connection, peer_id).await?;
                accepted_peer_sender.send((peer_id, connection)).await?;
                Ok::<(), anyhow::Error>(())
            });
        }

        // Register new peers
        while let Ok((peer_id, connection)) = self.accepted_peer_receiver.try_recv() {
            self.peers.insert(peer_id, connection);
            self.base_mut()
                .emit_signal("peer_connected", &[peer_id.to_variant()]);
        }

        // Receive packets from peers
        let mut disconnected_peers = Vec::new();
        for (peer_id, connection) in &mut self.peers {
            loop {
                match connection.receive_packet() {
                    Ok((channel, mode, packet)) => self
                        .received_packets
                        .push_back((*peer_id, channel, mode, packet)),
                    Err(TryRecvError::Disconnected) => {
                        disconnected_peers.push(*peer_id);
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                }
            }
        }

        // Remove disconnected peers
        for peer_id in disconnected_peers {
            self.peers.remove(&peer_id);
            self.base_mut()
                .emit_signal("peer_disconnected", &[peer_id.to_variant()]);
        }
    }

    fn get_connection_status(&self) -> ConnectionStatus {
        match self.listener.is_closed() {
            true => ConnectionStatus::DISCONNECTED,
            false => ConnectionStatus::CONNECTED,
        }
    }

    fn close(&mut self) {
        self.listener.close();
    }

    fn disconnect_peer(&mut self, peer_id: i32, force: bool) {
        if let Some(connection) = self.peers.remove(&peer_id) {
            connection.close();
            if !force {
                self.base_mut()
                    .emit_signal("peer_disconnected", &[peer_id.to_variant()]);
            }
        }
    }

    fn get_unique_id(&self) -> i32 {
        1
    }

    fn get_max_packet_size(&self) -> i32 {
        if self.transfer_mode == TransferMode::RELIABLE {
            u16::MAX as i32
        } else {
            1024
        }
    }

    fn get_available_packet_count(&self) -> i32 {
        self.received_packets.len() as i32
    }

    fn get_packet_channel(&self) -> i32 {
        match self.received_packets.front() {
            Some((_, channel, _, _)) => *channel,
            None => 0,
        }
    }

    fn get_packet_mode(&self) -> TransferMode {
        match self.received_packets.front() {
            Some((_, _, mode, _)) => *mode,
            None => TransferMode::RELIABLE,
        }
    }

    fn get_packet_peer(&self) -> i32 {
        match self.received_packets.front() {
            Some((peer_id, _, _, _)) => *peer_id,
            None => -1,
        }
    }

    fn get_packet_script(&mut self) -> PackedByteArray {
        match self.received_packets.pop_front() {
            Some((_, _, _, packet)) => packet.to_vec().into(),
            _ => PackedByteArray::new(),
        }
    }

    fn get_transfer_channel(&self) -> i32 {
        self.transfer_channel
    }

    fn set_transfer_channel(&mut self, channel: i32) {
        self.transfer_channel = channel;
    }

    fn get_transfer_mode(&self) -> TransferMode {
        self.transfer_mode
    }

    fn set_transfer_mode(&mut self, mode: TransferMode) {
        self.transfer_mode = mode;
    }

    fn set_target_peer(&mut self, peer_id: i32) {
        self.target_peer_id = peer_id;
    }

    fn put_packet_script(&mut self, buffer: PackedByteArray) -> Error {
        match self.target_peer_id {
            0 => {
                for connection in self.peers.values_mut() {
                    connection.send_packet(
                        self.transfer_channel,
                        self.transfer_mode,
                        buffer.to_vec(),
                    );
                }
            }
            peer_id if peer_id < 0 => {
                for (other_id, connection) in &mut self.peers {
                    if *other_id == peer_id {
                        continue;
                    }
                    connection.send_packet(
                        self.transfer_channel,
                        self.transfer_mode,
                        buffer.to_vec(),
                    );
                }
            }
            peer_id => {
                if let Some(connection) = self.peers.get_mut(&peer_id) {
                    connection.send_packet(
                        self.transfer_channel,
                        self.transfer_mode,
                        buffer.to_vec(),
                    );
                }
            }
        }
        Error::OK
    }

    fn is_server(&self) -> bool {
        true
    }

    fn is_server_relay_supported(&self) -> bool {
        true
    }

    fn is_refusing_new_connections(&self) -> bool {
        self.refuse_new_connections
    }

    fn set_refuse_new_connections(&mut self, enable: bool) {
        self.refuse_new_connections = enable;
    }
}

impl Drop for IrohServer {
    fn drop(&mut self) {
        self.close();
    }
}
