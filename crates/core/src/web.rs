//! Placeholder; the browser client server is filled in below.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use tokio::net::TcpStream;

use crate::node::Shared;

#[derive(Default)]
pub struct WebState {}

pub async fn serve_http(_shared: Arc<Shared>, _sock: TcpStream, _from: SocketAddr) -> Result<()> {
    Ok(())
}

pub fn offer_to_browser(_shared: &Shared, _peer: &str, _sources: Vec<crate::storage::SourceItem>, _text: Option<String>) -> Result<String> {
    anyhow::bail!("browser client not built yet")
}

pub fn info(_node: &crate::node::Node) -> serde_json::Value {
    serde_json::json!({})
}
