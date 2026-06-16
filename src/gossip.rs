use godot::prelude::*;
use iroh_gossip::{api::Event, net::Gossip, TopicId};
use tokio::sync::mpsc::{channel, Receiver, Sender};
use crate::IrohRuntime;
use futures_lite::StreamExt;

#[derive(GodotClass)]
#[class(base=Node)]
pub struct IrohGossip {
    base: Base<Node>,
    gossip_engine: Option<Gossip>,
    event_receiver: Option<Receiver<Vec<u8>>>,
    broadcast_sender: Option<Sender<Vec<u8>>>,
}

#[godot_api]
impl INode for IrohGossip {
    fn init(base: Base<Node>) -> Self {
        Self {
            base,
            gossip_engine: None,
            event_receiver: None,
            broadcast_sender: None,
        }
    }

    fn process(&mut self, _delta: f64) {
        let mut messages = Vec::new();
        if let Some(receiver) = &mut self.event_receiver {
            while let Ok(msg) = receiver.try_recv() {
                messages.push(msg);
            }
        }

        for msg in messages {
            let bytes = PackedByteArray::from_iter(msg);
            self.base_mut().emit_signal("message_received", &[bytes.to_variant()]);
        }
    }
}

#[godot_api]
impl IrohGossip {
    #[signal]
    fn message_received(message: PackedByteArray);

    pub fn get_engine(&mut self, endpoint: iroh::Endpoint) -> Gossip {
        let engine = Gossip::builder().spawn(endpoint);
        self.gossip_engine = Some(engine.clone());
        engine
    }

    #[func]
    fn join_topic(&mut self, topic_string: GString) {
        let Some(gossip) = self.gossip_engine.clone() else {
            godot_error!("Gossip engine not initialized. Attach to Router first.");
            return;
        };

        // Deterministically hash the topic string to a 32-byte TopicId
        let hash = blake3::hash(topic_string.to_string().as_bytes());
        let topic_id = TopicId::from_bytes(*hash.as_bytes());

        let (rx_event, tx_event) = channel(100);
        let (tx_broadcast, mut rx_broadcast) = channel::<Vec<u8>>(100);
        
        self.event_receiver = Some(tx_event);
        self.broadcast_sender = Some(tx_broadcast);

        IrohRuntime::spawn(async move {
            let (sender, mut receiver) = gossip.subscribe(topic_id, vec![]).await.unwrap().split();
            
            // Loop 1: Listen for network messages and push to Godot MPSC
            let rx_task = tokio::spawn(async move {
                while let Some(Ok(event)) = receiver.next().await {
                    if let Event::Received(msg) = event {
                        let _ = rx_event.send(msg.content.to_vec()).await;
                    }
                }
            });

            // Loop 2: Listen for Godot requests and broadcast to network
            let tx_task = tokio::spawn(async move {
                while let Some(msg) = rx_broadcast.recv().await {
                    let _ = sender.broadcast(msg.into()).await;
                }
            });

            let _ = tokio::join!(rx_task, tx_task);
        });
    }

    #[func]
    fn broadcast(&self, message: PackedByteArray) {
        if let Some(sender) = &self.broadcast_sender {
            let _ = sender.try_send(message.to_vec());
        }
    }
}