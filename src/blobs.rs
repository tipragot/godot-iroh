use godot::prelude::*;
use iroh_blobs::{
    api::{
        blobs::{AddPathOptions, AddProgressItem, ExportMode, ExportOptions, ExportProgressItem, ImportMode},
        TempTag,
    },
    store::fs::FsStore,
    ticket::BlobTicket,
    BlobFormat, BlobsProtocol,
};
use tokio::sync::mpsc::{channel, Receiver};
use futures_lite::StreamExt;
use crate::IrohRuntime;
use std::path::PathBuf;

#[derive(GodotClass)]
#[class(base=Node)]
pub struct IrohBlobs {
    base: Base<Node>,
    endpoint: Option<iroh::Endpoint>,
    store: Option<FsStore>,
    transfer_receiver: Option<Receiver<String>>,
    hosted_tags: Vec<TempTag>, 
}

#[godot_api]
impl INode for IrohBlobs {
    fn init(base: Base<Node>) -> Self {
        Self {
            base,
            endpoint: None,
            store: None,
            transfer_receiver: None,
            hosted_tags: Vec::new(),
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

    pub async fn get_engine(&mut self, endpoint: iroh::Endpoint, cache_dir: String) -> BlobsProtocol {
        let path = PathBuf::from(cache_dir);
        tokio::fs::create_dir_all(&path).await.unwrap();
        
        let store = FsStore::load(&path).await.unwrap();
        let engine = BlobsProtocol::new(&store, None);
        
        self.store = Some(store.clone());
        self.endpoint = Some(endpoint);
        engine
    }

    /// Streams the file to Iroh's cache without loading it into RAM
    #[func]
    fn host_file(&mut self, absolute_path: GString) -> GString {
        let Some(store) = &self.store else { return GString::new(); };
        let Some(endpoint) = &self.endpoint else { return GString::new(); };
        
        let path = PathBuf::from(absolute_path.to_string());
        let store_clone = store.clone();
        
        let ep_id = endpoint.id();

        let (ticket, tag) = IrohRuntime::block_on(async move {
            let import = store_clone.add_path_with_opts(AddPathOptions {
                path,
                mode: ImportMode::TryReference, // Zero-copy stream directly from disk
                format: BlobFormat::Raw,
            });
            
            let mut stream = import.stream().await;
            let mut final_tag = None;
            
            while let Some(item) = stream.next().await {
                if let AddProgressItem::Done(tt) = item {
                    final_tag = Some(tt);
                    break;
                }
            }
            
            let tag = final_tag.unwrap();
            let ticket = BlobTicket::new(ep_id.into(), tag.hash(), BlobFormat::Raw);
            (ticket, tag)
        });

        self.hosted_tags.push(tag);
        GString::from(ticket.to_string().as_str())
    }

    /// Zero-RAM stream from Network -> FsStore -> Final Database File
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
            
            // 1. Download to FsStore cache using your exact downloader setup
            let downloader = iroh_blobs::api::downloader::Downloader::new(&store_clone, &endpoint);
            let hash = ticket.hash();
            
            let node_id = ticket.addr().id; 

            let req = iroh_blobs::protocol::GetRequest::all(hash);
            let _ = downloader.download(req, vec![node_id]).await; // Ignore err if already complete
            
            // 2. Export from the FsStore Cache directly to the final file location
            let target = PathBuf::from(save_path_string.clone());
            let export = store_clone.export_with_opts(ExportOptions {
                hash,
                target,
                mode: ExportMode::Copy,
            });
            
            let mut stream = export.stream().await;
            while let Some(item) = stream.next().await {
                if let ExportProgressItem::Done = item { break; }
            }
            
            let _ = tx.send(save_path_string).await;
        });
    }
}