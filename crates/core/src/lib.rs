//! SweaxDrop core: discovery, pairing, encrypted multi-stream transfer,
//! resume, history and the browser client, shared by every platform.

pub mod api;
pub mod discovery;
pub mod history;
pub mod identity;
pub mod link;
pub mod node;
pub mod pairing;
pub mod peers;
pub mod proto;
pub mod recv;
pub mod send;
pub mod settings;
pub mod storage;
pub mod tls;
pub mod transfer;
pub mod util;
pub mod web;
pub mod wire;

pub use node::{Config, Node};

/// Owns a tokio runtime and a node; what the FFI layers hold on to.
pub struct Host {
    pub rt: tokio::runtime::Runtime,
    pub node: Node,
}

impl Host {
    pub fn start(cfg: Config) -> anyhow::Result<Self> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .thread_name("swx-core")
            .enable_all()
            .build()?;
        let node = rt.block_on(Node::start(cfg))?;
        Ok(Self { rt, node })
    }

    pub fn call_blocking(&self, req: &str) -> String {
        self.rt.block_on(api::call(&self.node, req))
    }

    /// Runs a command and hands the JSON answer to `done` from a core thread.
    pub fn call_async(&self, req: String, done: Box<dyn FnOnce(String) + Send>) {
        let node = self.node.clone();
        self.rt.spawn(async move {
            let out = api::call(&node, &req).await;
            done(out);
        });
    }
}
