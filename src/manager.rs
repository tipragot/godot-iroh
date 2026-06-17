use godot::prelude::*;
use iroh::{Endpoint, endpoint::presets, protocol::Router, endpoint::Connection, SecretKey};
use tokio::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

use crate::connection::GodotRpcHandler;
use crate::IrohRuntime;
use crate::docs::IrohDocs;
use crate::gossip::IrohGossip;
use crate::blobs::IrohBlobs;

#[derive(GodotClass)]
#[class(base=Node)]
pub struct IrohManager {
    base: Base<Node>,
    pub endpoint: Option<Endpoint>,
    router: Option<Router>,
    pub rpc_receiver: Option<Receiver<Connection>>,
}

#[godot_api]
impl INode for IrohManager {
    fn init(base: Base<Node>) -> Self {
        Self {
            base,
            endpoint: None,
            router: None,
            rpc_receiver: None,
        }
    }
}

#[godot_api]
impl IrohManager {
    #[signal]
    fn network_started(id: GString);

    #[signal]
    fn network_start_failed(error: GString);

    #[func]
    fn start_network(&mut self, docs_path: NodePath, gossip_path: NodePath, blobs_path: NodePath, cache_dir: GString, secret_key_bytes: PackedByteArray) {
        let mut docs_node = self.base().get_node_as::<IrohDocs>(&docs_path);
        let mut gossip_node = self.base().get_node_as::<IrohGossip>(&gossip_path);
        let mut blobs_node = self.base().get_node_as::<IrohBlobs>(&blobs_path);

        let (connection_sender, connection_receiver) = channel(32);
        let rpc_handler = GodotRpcHandler { connection_sender };
        let cache_dir_str = cache_dir.to_string();

        let key = if secret_key_bytes.is_empty() {
            SecretKey::generate()
        } else {
            let bytes: [u8; 32] = secret_key_bytes.as_slice().try_into().expect("Invalid secret key byte length");
            SecretKey::from_bytes(&bytes)
        };

        let runtime_result: Result<_, String> = IrohRuntime::block_on(async {
            let endpoint = Endpoint::builder(presets::N0)
                .secret_key(key)
                .alpns(vec![
                    crate::ALPN.to_vec(),
                    iroh_gossip::ALPN.to_vec(),
                    iroh_docs::ALPN.to_vec(),
                    iroh_blobs::ALPN.to_vec(),
                ])
                .bind()
                .await
                .map_err(|e| format!("Failed to bind endpoint: {}", e))?;
            
            let gossip_engine = gossip_node.bind_mut().get_engine(endpoint.clone());
            let blobs_engine = blobs_node.bind_mut().get_engine(endpoint.clone(), cache_dir_str.clone()).await;
            let docs_engine = docs_node.bind_mut().get_engine(endpoint.clone(), blobs_engine.clone(), gossip_engine.clone(), cache_dir_str).await;

            let router = iroh::protocol::Router::builder(endpoint.clone())
                .accept(crate::ALPN, Arc::new(rpc_handler))
                .accept(iroh_gossip::ALPN, Arc::new(gossip_engine))
                .accept(iroh_docs::ALPN, Arc::new(docs_engine))
                .accept(iroh_blobs::ALPN, Arc::new(blobs_engine))
                .spawn();

            Ok((endpoint, router))
        });

        match runtime_result {
            Ok((endpoint, router)) => {
                let id_string = endpoint.id().to_string();
                let id = GString::from(id_string.to_string().as_str());
                
                self.endpoint = Some(endpoint);
                self.router = Some(router);
                self.rpc_receiver = Some(connection_receiver);
                
                self.base_mut().emit_signal("network_started", &[id.to_variant()]);
            }
            Err(e) => {
                self.base_mut().emit_signal("network_start_failed", &[GString::from(e.to_string().as_str()).to_variant()]);
            }
        }
    }

    #[func]
    fn generate_secret_key() -> PackedByteArray {
        let key = SecretKey::generate();
        PackedByteArray::from_iter(key.to_bytes())
    }

    #[func]
    fn get_node_id(&mut self) -> GString {
        match &self.endpoint {
            Some(ep) => {
                let id_string = ep.id().to_string();
                GString::from(id_string.to_string().as_str())
            },
            None => GString::new(),
        }
    }

}