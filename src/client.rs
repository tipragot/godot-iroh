use std::collections::VecDeque;
use std::mem::replace;

use bytes::Bytes;
use godot::classes::multiplayer_peer::{ConnectionStatus, TransferMode};
use godot::classes::{IMultiplayerPeerExtension, MultiplayerPeerExtension};
use godot::global::Error;
use godot::prelude::*;
use iroh::Endpoint;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::task::JoinHandle;

use crate::connection::IrohConnection;
use crate::{ALPN, IrohRuntime};

enum ClientStatus {
    Connecting(JoinHandle<anyhow::Result<(Endpoint, i32, IrohConnection)>>),
    Connected {
        endpoint: Endpoint,
        peer_id: i32,
        connection: IrohConnection,
    },
    Failed(anyhow::Error),
    Disconnected,
}

#[derive(GodotClass)]
#[class(tool, no_init, base=MultiplayerPeerExtension)]
struct IrohClient {
    base: Base<MultiplayerPeerExtension>,
    status: ClientStatus,
    received_packets: VecDeque<(i32, TransferMode, Bytes)>,
    transfer_channel: i32,
    transfer_mode: TransferMode,
}

#[godot_api]
impl IrohClient {
    /// Connect to an existing server using the connection string.
    ///
    /// If there is an error connecting to the server, the
    /// `multiplayer.connection_failed` signal will be emitted
    /// and the error message will be returned by the
    /// [Self::connection_error] function.
    #[func]
    fn connect(node_id: GString) -> Gd<Self> {
        let node_id = node_id.to_string();
        let handle = IrohRuntime::spawn(async {
            let endpoint = Endpoint::builder()
                .alpns(vec![ALPN.to_vec()])
                .discovery_n0()
                .bind()
                .await?;
            let (peer_id, connection) = IrohConnection::connect(endpoint.clone(), node_id).await?;
            Ok((endpoint, peer_id, connection))
        });
        Gd::from_init_fn(|base| Self {
            base,
            status: ClientStatus::Connecting(handle),
            received_packets: VecDeque::new(),
            transfer_channel: 0,
            transfer_mode: TransferMode::RELIABLE,
        })
    }

    /// Returns the error message that occurred when connecting to the server.
    ///
    /// This function should be called after receiving the
    /// `multiplayer.connection_failed` signal.
    #[func]
    fn connection_error(&self) -> GString {
        if let ClientStatus::Failed(error) = &self.status {
            return error.to_string().into();
        }
        GString::new()
    }
}

#[godot_api]
impl IMultiplayerPeerExtension for IrohClient {
    fn poll(&mut self) {
        let mut notify_connection = false;
        let mut notify_disconnection = false;
        self.status = match replace(&mut self.status, ClientStatus::Disconnected) {
            ClientStatus::Connecting(handle) => {
                if handle.is_finished() {
                    match IrohRuntime::block_on(handle) {
                        Ok(Ok((endpoint, peer_id, connection))) => {
                            notify_connection = true;
                            ClientStatus::Connected {
                                endpoint,
                                peer_id,
                                connection,
                            }
                        }
                        Ok(Err(error)) => ClientStatus::Failed(error),
                        Err(error) => ClientStatus::Failed(error.into()),
                    }
                } else {
                    ClientStatus::Connecting(handle)
                }
            }
            ClientStatus::Connected {
                endpoint,
                peer_id,
                mut connection,
            } => loop {
                match connection.receive_packet() {
                    Ok(packet) => self.received_packets.push_back(packet),
                    Err(TryRecvError::Disconnected) => {
                        notify_disconnection = true;
                        break ClientStatus::Disconnected;
                    }
                    Err(TryRecvError::Empty) => {
                        break ClientStatus::Connected {
                            endpoint,
                            peer_id,
                            connection,
                        };
                    }
                }
            },
            status => status,
        };
        if notify_connection {
            self.base_mut()
                .emit_signal("peer_connected", &[1i32.to_variant()]);
        }
        if notify_disconnection {
            self.base_mut()
                .emit_signal("peer_disconnected", &[1i32.to_variant()]);
        }
    }

    fn get_connection_status(&self) -> ConnectionStatus {
        match self.status {
            ClientStatus::Connecting(_) => ConnectionStatus::CONNECTING,
            ClientStatus::Connected { .. } => ConnectionStatus::CONNECTED,
            ClientStatus::Failed(_) => ConnectionStatus::DISCONNECTED,
            ClientStatus::Disconnected => ConnectionStatus::DISCONNECTED,
        }
    }

    fn close(&mut self) {
        self.disconnect_peer(1, true);
    }

    fn disconnect_peer(&mut self, peer_id: i32, force: bool) {
        if peer_id != 1 {
            return;
        }
        if let ClientStatus::Connected { endpoint, .. } = &self.status {
            let endpoint_clone = endpoint.clone();
            IrohRuntime::spawn(async move { endpoint_clone.close().await });
            self.status = ClientStatus::Disconnected;
            if !force {
                self.base_mut()
                    .emit_signal("peer_disconnected", &[1i32.to_variant()]);
            }
        }
    }

    fn get_unique_id(&self) -> i32 {
        match &self.status {
            ClientStatus::Connected { peer_id, .. } => *peer_id,
            _ => -1,
        }
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
            Some((channel, _, _)) => *channel,
            _ => 0,
        }
    }

    fn get_packet_mode(&self) -> TransferMode {
        match self.received_packets.front() {
            Some((_, mode, _)) => *mode,
            _ => TransferMode::RELIABLE,
        }
    }

    fn get_packet_peer(&self) -> i32 {
        1
    }

    fn get_packet_script(&mut self) -> PackedByteArray {
        match self.received_packets.pop_front() {
            Some((_, _, packet)) => packet.to_vec().into(),
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

    fn set_target_peer(&mut self, _peer_id: i32) {}

    fn put_packet_script(&mut self, buffer: PackedByteArray) -> Error {
        if let ClientStatus::Connected { connection, .. } = &mut self.status {
            connection.send_packet(self.transfer_channel, self.transfer_mode, buffer.to_vec());
        }
        Error::OK
    }

    fn is_server(&self) -> bool {
        false
    }

    fn is_server_relay_supported(&self) -> bool {
        true
    }

    fn is_refusing_new_connections(&self) -> bool {
        true
    }

    fn set_refuse_new_connections(&mut self, _enable: bool) {}
}

impl Drop for IrohClient {
    fn drop(&mut self) {
        self.close();
    }
}
