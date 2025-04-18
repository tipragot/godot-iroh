use std::collections::{HashMap, hash_map::Entry};

use anyhow::{Context, bail};
use base64::prelude::*;
use bytes::{Buf, Bytes};
use godot::classes::multiplayer_peer::TransferMode;
use iroh::{
    Endpoint, NodeId,
    endpoint::{Connection, VarInt},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::mpsc::{Receiver, UnboundedSender, channel, error::TryRecvError, unbounded_channel},
};

use crate::{ALPN, IrohRuntime, MAX_PACKET_SIZE};

pub struct IrohListener {
    endpoint: Endpoint,
    connection_receiver: Receiver<Connection>,
    closed: bool,
}

impl IrohListener {
    pub async fn new() -> anyhow::Result<Self> {
        let endpoint = Endpoint::builder()
            .alpns(vec![ALPN.to_vec()])
            .discovery_n0()
            .bind()
            .await?;

        // Accept connection loop
        let endpoint_clone = endpoint.clone();
        let (connection_sender, connection_receiver) = channel(32);
        tokio::spawn(async move {
            while let Some(incoming) = endpoint_clone.accept().await {
                let Ok(connection) = incoming.await else {
                    continue;
                };
                if connection_sender.send(connection).await.is_err() {
                    break;
                }
            }
        });

        // Return the listener
        Ok(Self {
            endpoint,
            connection_receiver,
            closed: false,
        })
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    pub fn close(&mut self) {
        let endpoint = self.endpoint.clone();
        IrohRuntime::spawn(async move { endpoint.close().await });
        self.closed = true;
    }

    pub fn connection_string(&self) -> String {
        BASE64_URL_SAFE_NO_PAD.encode(self.endpoint.node_id().as_bytes())
    }

    pub fn receive_connection(&mut self) -> Result<Connection, TryRecvError> {
        match self.connection_receiver.try_recv() {
            Ok(result) => Ok(result),
            Err(TryRecvError::Disconnected) => {
                self.close();
                Err(TryRecvError::Disconnected)
            }
            Err(TryRecvError::Empty) => Err(TryRecvError::Empty),
        }
    }
}

pub struct IrohConnection {
    connection: Connection,
    reliable_channels: HashMap<i32, UnboundedSender<Bytes>>,
    unreliable_sender: UnboundedSender<(i32, bool, Vec<u8>)>,
    packet_receiver: Receiver<(i32, TransferMode, Bytes)>,
}

impl IrohConnection {
    async fn new(connection: Connection) -> Self {
        let (unreliable_sender, mut unreliable_receiver) =
            unbounded_channel::<(i32, bool, Vec<u8>)>();
        let (packet_sender, packet_receiver) = channel(32);

        // Unreliable packet send loop
        let connection_clone = connection.clone();
        tokio::spawn(async move {
            let mut last_counts = HashMap::new();
            while let Some((channel, ordered, mut buffer)) = unreliable_receiver.recv().await {
                buffer.extend_from_slice(&channel.to_be_bytes());
                if ordered {
                    let count = last_counts.entry(channel).or_insert(0u32);
                    *count = count.wrapping_add(1);
                    if *count == 0 {
                        *count += 1;
                    }
                    buffer.extend_from_slice(&count.to_be_bytes());
                } else {
                    buffer.extend_from_slice(&0u32.to_be_bytes());
                }
                if connection_clone.send_datagram(buffer.into()).is_err() {
                    break;
                }
            }
        });

        // Unreliable packet receive loop
        let connection_clone = connection.clone();
        let packet_sender_clone = packet_sender.clone();
        tokio::spawn(async move {
            let mut last_counts = HashMap::new();
            while let Ok(mut packet) = connection_clone.read_datagram().await {
                if packet.len() < 8 {
                    break;
                }
                let count = packet.split_off(packet.len() - 4).get_u32();
                let channel = packet.split_off(packet.len() - 4).get_i32();
                let mode: TransferMode;

                // Ignore packets from the past if in ordered mode
                if count != 0 {
                    mode = TransferMode::UNRELIABLE_ORDERED;
                    let last_count = last_counts.entry(channel).or_insert(0u32);
                    if count < *last_count && *last_count - count < (u32::MAX / 4) {
                        continue;
                    }
                    *last_count = count;
                } else {
                    mode = TransferMode::UNRELIABLE;
                }

                // Send the packet to the main thread
                if packet_sender_clone
                    .send((channel, mode, packet))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });

        // Reliable channel receive loop
        let connection_clone = connection.clone();
        tokio::spawn(async move {
            while let Ok(mut stream) = connection_clone.accept_uni().await {
                let packet_sender = packet_sender.clone();
                tokio::spawn(async move {
                    let channel = stream.read_i32().await?;
                    loop {
                        let packet_len = stream.read_u16().await?;
                        anyhow::ensure!(packet_len as usize <= MAX_PACKET_SIZE);
                        let mut packet = vec![0u8; packet_len as usize];
                        stream.read_exact(&mut packet).await?;
                        if packet_sender
                            .send((channel, TransferMode::RELIABLE, packet.into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Ok::<(), anyhow::Error>(())
                });
            }
        });

        // Return the connection
        Self {
            connection,
            reliable_channels: HashMap::new(),
            unreliable_sender,
            packet_receiver,
        }
    }

    pub async fn accept(connection: Connection, peer_id: i32) -> anyhow::Result<Self> {
        connection.open_uni().await?.write_i32(peer_id).await?;
        Ok(Self::new(connection).await)
    }

    pub async fn connect(
        endpoint: Endpoint,
        connection_string: String,
    ) -> anyhow::Result<(i32, Self)> {
        let node_id_bytes = BASE64_URL_SAFE_NO_PAD
            .decode(connection_string)
            .context("invalid connection string")?;
        let node_id_bytes: [u8; 32] = match node_id_bytes.try_into() {
            Ok(bytes) => bytes,
            Err(_) => bail!("invalid connection string"),
        };
        let node_id = NodeId::from_bytes(&node_id_bytes).context("invalid connection string")?;
        let connection = endpoint.connect(node_id, ALPN).await?;
        let peer_id = connection.accept_uni().await?.read_i32().await?;
        Ok((peer_id, Self::new(connection).await))
    }

    pub fn close(&self) {
        self.connection.close(VarInt::from_u32(0), b"");
    }

    pub fn send_packet(&mut self, channel: i32, mode: TransferMode, packet: Vec<u8>) {
        if mode == TransferMode::RELIABLE {
            let sender = match self.reliable_channels.entry(channel) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => {
                    let connection = self.connection.clone();
                    let (sender, mut receiver) = unbounded_channel::<Bytes>();
                    IrohRuntime::spawn(async move {
                        let mut stream = connection.open_uni().await?;
                        stream.write_i32(channel).await?;
                        while let Some(packet) = receiver.recv().await {
                            stream.write_u16(packet.len().try_into()?).await?;
                            stream.write_all(&packet).await?;
                        }
                        Ok::<(), anyhow::Error>(())
                    });
                    entry.insert(sender)
                }
            };
            let _ = sender.send(packet.into());
        } else {
            let _ = self.unreliable_sender.send((
                channel,
                mode == TransferMode::UNRELIABLE_ORDERED,
                packet,
            ));
        }
    }

    pub fn receive_packet(&mut self) -> Result<(i32, TransferMode, Bytes), TryRecvError> {
        self.packet_receiver.try_recv()
    }
}

impl Drop for IrohConnection {
    fn drop(&mut self) {
        self.close();
    }
}
