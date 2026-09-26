use std::collections::HashMap;

use base64::prelude::*;
use godot::prelude::*;
use iroh::SecretKey;
use iroh_mdns_address_lookup::DiscoveryEvent;
use n0_future::StreamExt;
use tokio::sync::mpsc::{Receiver, channel};
use tokio::task::JoinHandle;

use crate::{IrohRuntime, lan_lookup};

enum Event {
    Found(String, String),
    Lost(String),
}

/// Lists the servers (`IrohServer`) running on the local network, without internet access.
///
/// Add it to the scene tree: it listens while in the tree and emits `host_found` with the
/// server's connection string (for `IrohClient.connect`) and the text it advertises
/// (`IrohServer.set_lan_info`), again only when that text changes (not on address updates), and
/// `host_lost` when it disappears.
#[derive(GodotClass)]
#[class(base=Node)]
struct IrohBrowser {
    base: Base<Node>,
    events: Option<Receiver<Event>>,
    task: Option<JoinHandle<()>>,
    /// connection string -> advertised text, of the hosts reported so far
    hosts: HashMap<String, String>,
}

#[godot_api]
impl INode for IrohBrowser {
    fn init(base: Base<Node>) -> Self {
        Self {
            base,
            events: None,
            task: None,
            hosts: HashMap::new(),
        }
    }

    fn enter_tree(&mut self) {
        let (sender, receiver) = channel(32);
        self.events = Some(receiver);
        self.task = Some(IrohRuntime::spawn(async move {
            // a browser never connects: any id will do for listening
            let id = SecretKey::generate().public();
            let lookup = match lan_lookup(false).build(id) {
                Ok(lookup) => lookup,
                Err(error) => {
                    godot_error!("LAN browsing unavailable: {error}");
                    return;
                }
            };
            let mut events = lookup.subscribe().await;
            while let Some(event) = events.next().await {
                let event = match event {
                    DiscoveryEvent::Discovered { endpoint_info, .. } => Event::Found(
                        BASE64_URL_SAFE_NO_PAD.encode(endpoint_info.endpoint_id.as_bytes()),
                        endpoint_info
                            .user_data()
                            .map(|data| data.to_string())
                            .unwrap_or_default(),
                    ),
                    DiscoveryEvent::Expired { endpoint_id } => {
                        Event::Lost(BASE64_URL_SAFE_NO_PAD.encode(endpoint_id.as_bytes()))
                    }
                    _ => continue,
                };
                if sender.send(event).await.is_err() {
                    break;
                }
            }
        }));
    }

    fn exit_tree(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        self.events = None;
        self.hosts.clear();
    }

    fn process(&mut self, _delta: f64) {
        let mut found = Vec::new();
        if let Some(events) = self.events.as_mut() {
            while let Ok(event) = events.try_recv() {
                found.push(event);
            }
        }
        for event in found {
            match event {
                Event::Found(connection_string, info) => {
                    if self.hosts.get(&connection_string) == Some(&info) {
                        continue; // an address update, nothing new for the game
                    }
                    self.hosts.insert(connection_string.clone(), info.clone());
                    self.base_mut().emit_signal(
                        "host_found",
                        &[connection_string.to_variant(), info.to_variant()],
                    );
                }
                Event::Lost(connection_string) => {
                    if self.hosts.remove(&connection_string).is_some() {
                        self.base_mut()
                            .emit_signal("host_lost", &[connection_string.to_variant()]);
                    }
                }
            };
        }
    }
}

#[godot_api]
impl IrohBrowser {
    /// A server was found on the local network, or its advertised text changed.
    #[signal]
    fn host_found(connection_string: GString, info: GString);

    /// A server left the local network (stopped, or unreachable for a while).
    #[signal]
    fn host_lost(connection_string: GString);
}
