use godot::prelude::*;
use iroh::Endpoint;
use iroh_docs::{protocol::Docs, DocTicket, NamespaceId, AuthorId};
use iroh_blobs::BlobsProtocol;
use tokio::sync::mpsc::{channel, Receiver};
use futures_lite::StreamExt;
use std::path::PathBuf;
use std::str::FromStr;

use crate::IrohRuntime;

#[derive(GodotClass)]
#[class(base=Node)]
pub struct IrohDocs {
    base: Base<Node>,
    docs_engine: Option<Docs>,
    blobs_engine: Option<BlobsProtocol>,
    author: Option<AuthorId>,
    namespace: Option<NamespaceId>,
    event_receiver: Option<Receiver<(String, Vec<u8>)>>,
    process_queue: Vec<(String, Vec<u8>)>, 
}

#[godot_api]
impl INode for IrohDocs {
    fn init(base: Base<Node>) -> Self {
        Self {
            base,
            docs_engine: None,
            blobs_engine: None,
            author: None,
            namespace: None,
            event_receiver: None,
            process_queue: Vec::new(),
        }
    }

    fn ready(&mut self) {   
        self.base_mut().set_process(true);
    }
    
    fn process(&mut self, _delta: f64) {
        // Step 1: Safely drain the receiver into the reusable queue
        if let Some(receiver) = &mut self.event_receiver {
            while let Ok(msg) = receiver.try_recv() {
                self.process_queue.push(msg);
            }
        }

        let events = std::mem::take(&mut self.process_queue);

        // 2. Iterate over the local variable `events`, not `self`
        for (key, value) in events {
            let k = GString::from(key.as_str());
            let bytes = PackedByteArray::from_iter(value);
            self.base_mut().emit_signal("entry_synced", &[k.to_variant(), bytes.to_variant()]);
        }
    }
}

#[godot_api]
impl IrohDocs {
    #[signal]
    fn entry_synced(key: GString, value: PackedByteArray);

    pub async fn get_engine(&mut self, endpoint: Endpoint, blobs: BlobsProtocol, gossip: iroh_gossip::net::Gossip, cache_dir: String) -> Docs {
        let path = PathBuf::from(cache_dir).join("docs");
        tokio::fs::create_dir_all(&path).await.unwrap();
        
        let engine = Docs::persistent(path).spawn(endpoint, (*blobs).clone(), gossip).await.unwrap();
        
        self.blobs_engine = Some(blobs);
        self.docs_engine = Some(engine.clone());
        engine
    }

    #[func]
    fn setup_author(&mut self, saved_author_str: GString) -> GString {
        let docs = self.docs_engine.as_ref().expect("Docs engine uninitialized").clone();
        
        let author_string = IrohRuntime::block_on(async move {
            if saved_author_str.is_empty() {
                // First boot: create a new author in the persistent DB
                let new_author = docs.author_create().await.unwrap();
                new_author.to_string()
            } else {
                // Subsequent boots: verify the saved author exists in the DB
                let parsed_author = AuthorId::from_str(&saved_author_str.to_string()).unwrap();
                
                // If the user wiped their cache but kept the config, recreate it
                let authors = docs.author_list().await.unwrap().collect::<Vec<_>>().await;
                let exists = authors.into_iter().filter_map(Result::ok).any(|a| a == parsed_author);
                
                if exists {
                    parsed_author.to_string()
                } else {
                    let new_author = docs.author_create().await.unwrap();
                    new_author.to_string()
                }
            }
        });
        
        // Save it to memory for when we call set_entry()
        self.author = Some(AuthorId::from_str(&author_string).unwrap());
        
        GString::from(author_string.to_string().as_str())
    }

    #[func]
    fn create_document(&mut self) -> GString {
        let docs = self.docs_engine.as_ref().expect("Docs engine uninitialized").clone();
        let blobs = self.blobs_engine.as_ref().expect("Blobs engine uninitialized").clone();
        
        let (tx, rx) = channel(100);
        self.event_receiver = Some(rx);

        let ticket_str = IrohRuntime::block_on(async move {
            let author = docs.author_create().await.unwrap();
            let replica = docs.create().await.unwrap();
           
            let ticket = replica.share(iroh_docs::api::protocol::ShareMode::Write, Default::default()).await.unwrap();
            let mut events = replica.subscribe().await.unwrap();
            let replica_id = replica.id();
            
            tokio::spawn(async move {
                let _keep_alive = replica; // PREVENTS RPC DISCONNECT

                while let Some(Ok(event)) = events.next().await {
                    match event {
                        iroh_docs::engine::LiveEvent::InsertLocal { entry, .. } |
                        iroh_docs::engine::LiveEvent::InsertRemote { entry, .. } => {
                            let key = String::from_utf8_lossy(entry.key()).to_string();
                            
                            match (*blobs).blobs().get_bytes(entry.content_hash()).await {
                                Ok(bytes) => {
                                    let _ = tx.send((key, bytes.to_vec())).await;
                                }
                                Err(e) => {
                                    godot_error!("Blob not ready/failed for key {}: {}", key, e);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            });
            self.author = Some(author); 
            self.namespace = Some(replica_id);
            ticket.to_string()
        });

        GString::from(&ticket_str)
    }

    #[func]
    fn join_document(&mut self, ticket_string: GString) {
        let docs = self.docs_engine.as_ref().expect("Docs engine uninitialized").clone();
        let blobs = self.blobs_engine.as_ref().expect("Blobs engine uninitialized").clone();
        
        let (tx, rx) = channel(100);
        self.event_receiver = Some(rx);

        let namespace_id = IrohRuntime::block_on(async move {
            let ticket = DocTicket::from_str(&ticket_string.to_string()).expect("Invalid ticket string");
            let replica = docs.import(ticket.clone()).await.unwrap();
            let replica_id = replica.id();

            let mut events = replica.subscribe().await.unwrap();
            
            tokio::spawn(async move {
                let _keep_alive = replica; // PREVENTS RPC DISCONNECT
                
                while let Some(Ok(event)) = events.next().await {
                    match event {
                        iroh_docs::engine::LiveEvent::InsertLocal { entry, .. } |
                        iroh_docs::engine::LiveEvent::InsertRemote { entry, .. } => {
                            let key = String::from_utf8_lossy(entry.key()).to_string();
                            
                            match (*blobs).blobs().get_bytes(entry.content_hash()).await {
                                Ok(bytes) => {
                                    let _ = tx.send((key, bytes.to_vec())).await;
                                }
                                Err(e) => {
                                    godot_error!("Blob not ready/failed for key {}: {}", key, e);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            });
            
            replica_id
        });

        self.namespace = Some(namespace_id);
    }
    
    #[func]
    fn set_entry(&self, key: GString, value: PackedByteArray) {
        let docs = self.docs_engine.as_ref().unwrap().clone();
        let namespace = *self.namespace.as_ref().unwrap();
        let author = *self.author.as_ref().unwrap();

        let k = key.to_string().into_bytes();
        let v = value.to_vec();

        IrohRuntime::spawn(async move {
            let replica = docs.open(namespace).await.unwrap().unwrap();
            replica.set_bytes(author, k, v).await.unwrap();
        });
    }

    #[func]
    fn set_entries_batched(&self, entries: VarDictionary) {
        let docs = self.docs_engine.as_ref().unwrap().clone();
        let namespace = *self.namespace.as_ref().unwrap();
        let author = *self.author.as_ref().unwrap();

        let mut batch = Vec::new();
        for (key, value) in entries.iter_shared() {
            let k = key.to_string().into_bytes();
            let v = value.try_to::<PackedByteArray>().unwrap().to_vec();
            batch.push((k, v));
        }

        IrohRuntime::spawn(async move {
            if let Ok(Some(replica)) = docs.open(namespace).await {
                for (k, v) in batch {
                    let _ = replica.set_bytes(author, k, v).await;
                }
            }
        });
    }
}