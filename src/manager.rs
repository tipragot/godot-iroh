use godot::prelude::*;
use iroh::{Endpoint, endpoint::presets, protocol::Router, endpoint::Connection};
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
    #[func]
    fn start_network(&mut self, docs_path: NodePath, gossip_path: NodePath, blobs_path: NodePath, cache_dir: GString) {
        let mut docs_node = self.base().get_node_as::<IrohDocs>(&docs_path);
        let mut gossip_node = self.base().get_node_as::<IrohGossip>(&gossip_path);
        let mut blobs_node = self.base().get_node_as::<IrohBlobs>(&blobs_path);

        let (connection_sender, connection_receiver) = channel(32);
        let rpc_handler = GodotRpcHandler { connection_sender };
        let cache_dir_str = cache_dir.to_string();

        let (endpoint, router) = IrohRuntime::block_on(async {
            let endpoint = Endpoint::builder(presets::N0)
                .alpns(vec![
                    crate::ALPN.to_vec(),
                    iroh_gossip::ALPN.to_vec(),
                    iroh_docs::ALPN.to_vec(),
                    iroh_blobs::ALPN.to_vec(),
                ])
                .bind()
                .await
                .expect("Failed to bind endpoint");

            let gossip_engine = gossip_node.bind_mut().get_engine(endpoint.clone());
            let docs_engine = docs_node.bind_mut().get_engine(endpoint.clone(), gossip_engine.clone()).await;
            
            let blobs_engine = blobs_node.bind_mut().get_engine(endpoint.clone(), cache_dir_str).await;

            let router = iroh::protocol::Router::builder(endpoint.clone())
                .accept(crate::ALPN, Arc::new(rpc_handler))
                .accept(iroh_gossip::ALPN, Arc::new(gossip_engine))
                .accept(iroh_docs::ALPN, Arc::new(docs_engine))
                .accept(iroh_blobs::ALPN, Arc::new(blobs_engine))
                .spawn();

            (endpoint, router)
        });

        self.endpoint = Some(endpoint);
        self.router = Some(router);
        self.rpc_receiver = Some(connection_receiver);
    }
}