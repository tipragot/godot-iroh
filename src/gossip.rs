use godot::prelude::*;
use iroh_gossip::{api::Event, net::Gossip, TopicId};
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::task::JoinHandle;
use std::collections::HashMap;
use crate::IrohRuntime;
use futures_lite::StreamExt;
use iroh::PublicKey;
use std::str::FromStr;

pub enum GossipEvent {
    Joined(String),
    Message { topic: String, data: Vec<u8> },
    BroadcastSuccess(String),
    Error { topic: String, message: String },
    Log { topic: String, message: String },
}

#[derive(GodotClass)]
#[class(base=Node)]
pub struct IrohGossip {
    base: Base<Node>,
    gossip_engine: Option<Gossip>,
    event_receiver: Option<Receiver<GossipEvent>>,
    event_sender: Option<Sender<GossipEvent>>, 
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

    fn ready(&mut self) {
        self.base_mut().set_process(true);
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
                    self.base_mut().emit_signal("topic_joined", &[GString::from(&topic).to_variant()]);
                }
                Some(GossipEvent::BroadcastSuccess(topic)) => {
                    self.base_mut().emit_signal("broadcast_sent", &[GString::from(&topic).to_variant()]);
                }
                Some(GossipEvent::Message { topic, data }) => {
                    let bytes = PackedByteArray::from_iter(data);
                    self.base_mut().emit_signal("message_received", &[GString::from(&topic).to_variant(), bytes.to_variant()]);
                }
                Some(GossipEvent::Error { topic, message }) => {
                    self.base_mut().emit_signal("gossip_error", &[GString::from(&topic).to_variant(), GString::from(&message).to_variant()]);
                }
                Some(GossipEvent::Log { topic, message }) => {
                    self.base_mut().emit_signal("gossip_log", &[GString::from(&topic).to_variant(), GString::from(&message).to_variant()]);
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

    #[signal]
    fn gossip_error(topic: GString, error: GString);

    #[signal]
    fn gossip_log(topic: GString, message: GString);

    pub fn get_engine(&mut self, endpoint: iroh::Endpoint) -> Gossip {
        let engine = Gossip::builder().spawn(endpoint);
        self.gossip_engine = Some(engine.clone());
        
        let (tx_event, rx_event) = channel(1000);
        self.event_sender = Some(tx_event);
        self.event_receiver = Some(rx_event);
        
        engine
    }

    #[func]
    fn join_topic(&mut self, topic_string: GString, bootstrap_peers: PackedStringArray) {
        let Some(gossip) = self.gossip_engine.clone() else { return; };
        let topic_str = topic_string.to_string();

        if self.active_topics.contains_key(&topic_str) { return; }
        let Some(tx_event) = self.event_sender.clone() else { return; };

        let hash = blake3::hash(topic_str.as_bytes());
        let topic_id = TopicId::from_bytes(*hash.as_bytes());

        let (tx_broadcast, mut rx_broadcast) = channel::<Vec<u8>>(100);
        self.broadcast_senders.insert(topic_str.clone(), tx_broadcast);

        let topic_name_for_task = topic_str.clone();

        let mut peers = vec![];
        for peer_gstr in bootstrap_peers.as_slice() {
            if let Ok(node_id) = PublicKey::from_str(&peer_gstr.to_string()) {
                peers.push(node_id);
            }
        }

        let task = IrohRuntime::spawn(async move {
            match gossip.subscribe(topic_id, peers).await {
                Ok(sub) => {
                    let _ = tx_event.send(GossipEvent::Joined(topic_name_for_task.clone())).await;
                    let (sender, mut receiver) = sub.split();
                    
                    let tx_event_clone = tx_event.clone();
                    let topic_for_rx = topic_name_for_task.clone();
                    
                    let rx_task = tokio::spawn(async move {
                        while let Some(result) = receiver.next().await {
                            match result {
                                Ok(Event::Received(msg)) => {
                                    let _ = tx_event_clone.send(GossipEvent::Message {
                                        topic: topic_for_rx.clone(),
                                        data: msg.content.to_vec(),
                                    }).await;
                                }
                                Ok(other_event) => {
                                    // Pushes NeighborUp/NeighborDown to Godot
                                    let _ = tx_event_clone.send(GossipEvent::Log {
                                        topic: topic_for_rx.clone(),
                                        message: format!("{:?}", other_event),
                                    }).await;
                                }
                                Err(e) => {
                                    let _ = tx_event_clone.send(GossipEvent::Error {
                                        topic: topic_for_rx.clone(),
                                        message: e.to_string(),
                                    }).await;
                                }
                            }
                        }
                    });

                    let topic_for_tx = topic_name_for_task;
                    let tx_task = tokio::spawn(async move {
                        while let Some(msg) = rx_broadcast.recv().await {
                            match sender.broadcast(msg.into()).await {
                                Ok(_) => {
                                    let _ = tx_event.send(GossipEvent::BroadcastSuccess(topic_for_tx.clone())).await;
                                }
                                Err(e) => {
                                    let _ = tx_event.send(GossipEvent::Error {
                                        topic: topic_for_tx.clone(),
                                        message: format!("Broadcast failed: {}", e),
                                    }).await;
                                }
                            }
                        }
                    });

                    let _ = tokio::join!(rx_task, tx_task);
                }
                Err(e) => {
                    let _ = tx_event.send(GossipEvent::Error {
                        topic: topic_name_for_task,
                        message: format!("Subscribe failed: {}", e),
                    }).await;
                }
            }
        });

        self.active_topics.insert(topic_str, task);
    }

    #[func]
    fn broadcast(&self, topic_string: GString, message: PackedByteArray) {
        let topic_str = topic_string.to_string();
        if let Some(sender) = self.broadcast_senders.get(&topic_str) {
            if let Err(e) = sender.try_send(message.to_vec()) {
                godot_error!("MPSC queue full or closed: {}", e);
            }
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