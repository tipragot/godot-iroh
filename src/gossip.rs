use godot::prelude::*;
use iroh_gossip::{api::Event, net::Gossip, TopicId};
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::task::JoinHandle;
use std::collections::HashMap;
use crate::IrohRuntime;
use futures_lite::StreamExt;

pub enum GossipEvent {
    Joined(String),
    Message { topic: String, data: Vec<u8> },
    BroadcastSuccess(String),
}

#[derive(GodotClass)]
#[class(base=Node)]
pub struct IrohGossip {
    base: Base<Node>,
    gossip_engine: Option<Gossip>,
    
    // Central event channel for all network tasks to send data back to Godot
    event_receiver: Option<Receiver<GossipEvent>>,
    event_sender: Option<Sender<GossipEvent>>, 
    
    // Maps a topic string to its specific network sender and Tokio task
    broadcast_senders: HashMap<String, Sender<Vec<u8>>>,
    active_topics: HashMap<String, JoinHandle<()>>,
}

#[godot_api]
impl INode for IrohGossip {
    fn init(base: Base<Node>) -> Self {
        Self {
            base,
            gossip_engine: None,
            event_receiver: None,
            event_sender: None,
            broadcast_senders: HashMap::new(),
            active_topics: HashMap::new(),
        }
    }

    fn process(&mut self, _delta: f64) {
        loop {
            let event_opt = if let Some(receiver) = &mut self.event_receiver {
                receiver.try_recv().ok()
            } else {
                None
            };

            match event_opt {
                Some(GossipEvent::Joined(topic)) => {
                    let topic_gstr = GString::from(topic.as_str());
                    self.base_mut().emit_signal("topic_joined", &[topic_gstr.to_variant()]);
                }
                Some(GossipEvent::BroadcastSuccess(topic)) => {
                    let topic_gstr = GString::from(topic.as_str());
                    self.base_mut().emit_signal("broadcast_sent", &[topic_gstr.to_variant()]);
                }
                Some(GossipEvent::Message { topic, data }) => {
                    let topic_gstr = GString::from(topic.as_str());
                    let bytes = PackedByteArray::from_iter(data);
                    self.base_mut().emit_signal("message_received", &[topic_gstr.to_variant(), bytes.to_variant()]);
                }
                None => break,
            }
        }
    }
}

#[godot_api]
impl IrohGossip {
    #[signal]
    fn message_received(topic: GString, message: PackedByteArray);

    #[signal]
    fn topic_joined(topic: GString);

    #[signal]
    fn broadcast_sent(topic: GString);

    pub fn get_engine(&mut self, endpoint: iroh::Endpoint) -> Gossip {
        let engine = Gossip::builder().spawn(endpoint);
        self.gossip_engine = Some(engine.clone());
        
        // Initialize the central event channel once
        let (tx_event, rx_event) = channel(1000);
        self.event_sender = Some(tx_event);
        self.event_receiver = Some(rx_event);
        
        engine
    }

    #[func]
    fn join_topic(&mut self, topic_string: GString) {
        let Some(gossip) = self.gossip_engine.clone() else {
            godot_error!("Gossip engine not initialized. Attach to Router first.");
            return;
        };

        let topic_str = topic_string.to_string();

        if self.active_topics.contains_key(&topic_str) {
            return; // Already joined
        }

        let Some(tx_event) = self.event_sender.clone() else {
            return; // Engine not initialized
        };

        let hash = blake3::hash(topic_str.as_bytes());
        let topic_id = TopicId::from_bytes(*hash.as_bytes());

        let (tx_broadcast, mut rx_broadcast) = channel::<Vec<u8>>(100);
        self.broadcast_senders.insert(topic_str.clone(), tx_broadcast);

        let topic_name_for_task = topic_str.clone();

        let task = IrohRuntime::spawn(async move {
            if let Ok(sub) = gossip.subscribe(topic_id, vec![]).await {
                let _ = tx_event.send(GossipEvent::Joined(topic_name_for_task.clone())).await;
                let (sender, mut receiver) = sub.split();
                
                let tx_event_clone = tx_event.clone();
                let topic_for_rx = topic_name_for_task.clone();
                let rx_task = tokio::spawn(async move {
                    while let Some(Ok(event)) = receiver.next().await {
                        if let Event::Received(msg) = event {
                            let _ = tx_event_clone.send(GossipEvent::Message {
                                topic: topic_for_rx.clone(),
                                data: msg.content.to_vec(),
                            }).await;
                        }
                    }
                });

                let topic_for_tx = topic_name_for_task;
                let tx_task = tokio::spawn(async move {
                    while let Some(msg) = rx_broadcast.recv().await {
                        if sender.broadcast(msg.into()).await.is_ok() {
                            let _ = tx_event.send(GossipEvent::BroadcastSuccess(topic_for_tx.clone())).await;
                        }
                    }
                });

                let _ = tokio::join!(rx_task, tx_task);
            }
        });

        self.active_topics.insert(topic_str, task);
    }

    #[func]
    fn broadcast(&self, topic_string: GString, message: PackedByteArray) {
        let topic_str = topic_string.to_string();
        if let Some(sender) = self.broadcast_senders.get(&topic_str) {
            let _ = sender.try_send(message.to_vec());
        } else {
            godot_error!("Cannot broadcast to {}. Not joined.", topic_str);
        }
    }

    #[func]
    fn leave_topic(&mut self, topic_string: GString) {
        let topic_str = topic_string.to_string();
        
        if let Some(task) = self.active_topics.remove(&topic_str) {
            task.abort();
        }
        self.broadcast_senders.remove(&topic_str);
    }
}