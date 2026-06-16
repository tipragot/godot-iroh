use godot::prelude::*;
use iroh_docs::{protocol::Docs, NamespaceId, AuthorId};
use iroh_blobs::store::mem::MemStore;
use tokio::sync::mpsc::{channel, Receiver};
use futures_lite::StreamExt;
use crate::IrohRuntime;

#[derive(GodotClass)]
#[class(base=Node)]
pub struct IrohDocs {
    base: Base<Node>,
    docs_engine: Option<Docs>,
    store: Option<MemStore>,
    author: Option<AuthorId>,
    namespace: Option<NamespaceId>,
    event_receiver: Option<Receiver<(String, Vec<u8>)>>,
}

#[godot_api]
impl INode for IrohDocs {
    fn init(base: Base<Node>) -> Self {
        Self {
            base,
            docs_engine: None,
            store: None,
            author: None,
            namespace: None,
            event_receiver: None,
        }
    }

    fn process(&mut self, _delta: f64) {
        let mut events = Vec::new();
        if let Some(receiver) = &mut self.event_receiver {
            while let Ok((key, value)) = receiver.try_recv() {
                events.push((key, value));
            }
        }
        for (key, value) in events {
            let k = GString::from(key.as_str());
            let value_string = String::from_utf8_lossy(&value).into_owned();
            let v = GString::from(value_string.as_str());
            self.base_mut().emit_signal("entry_synced", &[k.to_variant(), v.to_variant()]);
        }
    }
}

#[godot_api]
impl IrohDocs {
    #[signal]
    fn entry_synced(key: GString, value: PackedByteArray);

    pub async fn get_engine(&mut self, endpoint: iroh::Endpoint, gossip: iroh_gossip::net::Gossip) -> Docs {
        let store = MemStore::default();
        let engine = Docs::memory().spawn(endpoint, store.clone().into(), gossip).await.unwrap();
        self.store = Some(store);
        self.docs_engine = Some(engine.clone());
        engine
    }

    #[func]
    fn create_staging_replica(&mut self) {
        let Some(docs) = self.docs_engine.clone() else { return; };
        
        IrohRuntime::block_on(async {
            let author = docs.author_create().await.unwrap();
            let replica = docs.create().await.unwrap();
            
            self.author = Some(author);
            self.namespace = Some(replica.id());
            
            let (tx, rx) = channel(100);
            self.event_receiver = Some(rx);

            // Subscribe to remote changes
            tokio::spawn(async move {
                let mut events = replica.subscribe().await.unwrap();
                while let Some(Ok(event)) = events.next().await {
                    if let iroh_docs::engine::LiveEvent::InsertRemote { entry, .. } = event {
                        let key = entry.key().to_vec();
                        let _ = tx.send((String::from_utf8_lossy(&key).to_string(), vec![])).await;
                    }
                }
            });
        });
    }

    #[func]
    fn set_entry(&self, key: GString, value: PackedByteArray) {
        // 1. Extract and clone the variables out of `self` FIRST
        let docs = self.docs_engine.as_ref().expect("Docs engine not initialized").clone();
        let namespace = *self.namespace.as_ref().unwrap(); // Or however you safely copy the ID
        let author = *self.author.as_ref().unwrap();

        // Keep your existing k and v parsing here...
        let k = key.to_string().into_bytes();
        let v = value.to_vec();

        // 2. Now spawn the static block
        IrohRuntime::spawn(async move {
            let replica = docs.open(namespace).await.unwrap().unwrap();
            replica.set_bytes(author, k, v).await.unwrap();
        });
    }
}