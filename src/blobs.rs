use godot::prelude::*;
use iroh_blobs::{BlobsProtocol, store::mem::MemStore, ticket::BlobTicket};
use tokio::sync::mpsc::{channel, Receiver};
use crate::IrohRuntime;
use std::path::PathBuf;

#[derive(GodotClass)]
#[class(base=Node)]
pub struct IrohBlobs {
    base: Base<Node>,
    endpoint: Option<iroh::Endpoint>,
    store: Option<MemStore>,
    transfer_receiver: Option<Receiver<String>>,
}

#[godot_api]
impl INode for IrohBlobs {
    fn init(base: Base<Node>) -> Self {
        Self {
            base,
            endpoint: None,
            store: None,
            transfer_receiver: None,
        }
    }

    fn process(&mut self, _delta: f64) {
        let mut paths = Vec::new();
        if let Some(receiver) = &mut self.transfer_receiver {
            while let Ok(path) = receiver.try_recv() {
                paths.push(path);
            }
        }
        for path in paths {
            self.base_mut().emit_signal("download_complete", &[GString::from(&path).to_variant()]);
        }
    }
}

#[godot_api]
impl IrohBlobs {
    #[signal]
    fn download_complete(save_path: GString);

    pub fn get_engine(&mut self, endpoint: iroh::Endpoint) -> BlobsProtocol {
        let store = MemStore::default();
        let engine = BlobsProtocol::new(&store, None);
        self.store = Some(store);
        self.endpoint = Some(endpoint);
        engine
    }

    /// Hashes the file and returns a ticket for peers to download
    #[func]
    fn host_file(&self, absolute_path: GString) -> GString {
        let Some(store) = &self.store else { return GString::new(); };
        let Some(endpoint) = &self.endpoint else { return GString::new(); };
        
        let path = absolute_path.to_string();
        let store_clone = store.clone();
        let ep_id = endpoint.id();

        let ticket = IrohRuntime::block_on(async move {
            let path_buf = PathBuf::from(path);
            let bytes = std::fs::read(&path_buf).unwrap();
            let tag = store_clone.add_bytes(bytes).await.unwrap();
            BlobTicket::new(ep_id.into(), tag.hash, tag.format)
        });

        GString::from(ticket.to_string().as_str())
    }

    /// Connects to a Queen node and streams the file directly to disk
    #[func]
    fn fetch_file(&mut self, ticket_str: GString, save_path: GString) {
        let Some(store) = &self.store else { return; };
        let Some(endpoint) = self.endpoint.clone() else { return; };
        
        let (tx, rx) = channel(2);
        self.transfer_receiver = Some(rx);
        let save_path_string = save_path.to_string();
        let store_clone = store.clone();

        IrohRuntime::spawn(async move {
            let ticket = ticket_str.to_string().parse::<BlobTicket>().unwrap();
            let downloader = iroh_blobs::api::downloader::Downloader::new(&store_clone, &endpoint);
            
            let hash = ticket.hash();
            let node_id = ticket.addr().id;

            let req = iroh_blobs::protocol::GetRequest::all(hash);
            downloader.download(req, vec![node_id]).await.unwrap();
            
            let _ = tx.send(save_path_string).await;
        });
    }
}