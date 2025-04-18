use std::time::Duration;

use godot::{classes::Engine, prelude::*};
use tokio::{
    runtime::{self, Runtime},
    task::JoinHandle,
};

const ALPN: &[u8] = b"godot-iroh/0.1";
const MAX_PACKET_SIZE: usize = 1024;

mod client;
mod connection;
mod server;

struct MyExtension;

#[gdextension]
unsafe impl ExtensionLibrary for MyExtension {
    fn on_level_init(level: InitLevel) {
        if level == InitLevel::Scene {
            Engine::singleton().register_singleton("IrohRuntime", &IrohRuntime::new_alloc());
        }
    }

    fn on_level_deinit(level: InitLevel) {
        if level == InitLevel::Scene {
            let mut engine = Engine::singleton();
            let singleton = engine
                .get_singleton("IrohRuntime")
                .expect("singleton not found");
            engine.unregister_singleton("IrohRuntime");
            singleton.free();
        }
    }
}

#[derive(GodotClass)]
#[class(base=Object)]
pub struct IrohRuntime {
    base: Base<Object>,
    runtime: Option<Runtime>,
}

#[godot_api]
impl IObject for IrohRuntime {
    fn init(base: Base<Object>) -> Self {
        let runtime = Some(
            runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap(),
        );
        Self { base, runtime }
    }
}

#[godot_api]
impl IrohRuntime {
    pub fn spawn<F>(future: F) -> JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        Engine::singleton()
            .get_singleton("IrohRuntime")
            .expect("singleton not found")
            .cast::<Self>()
            .bind()
            .runtime
            .as_ref()
            .expect("invalid singleton")
            .spawn(future)
    }

    pub fn block_on<F: Future>(future: F) -> F::Output {
        Engine::singleton()
            .get_singleton("IrohRuntime")
            .expect("singleton not found")
            .cast::<Self>()
            .bind()
            .runtime
            .as_ref()
            .expect("invalid singleton")
            .block_on(future)
    }
}

impl Drop for IrohRuntime {
    fn drop(&mut self) {
        if let Some(runtime) = std::mem::take(&mut self.runtime) {
            runtime.shutdown_timeout(Duration::from_secs(5));
        }
    }
}
