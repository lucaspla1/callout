//! Discord local RPC integration (Route C's "who is speaking" half).
//! Protocol details and payload shapes: docs/dev/discord-rpc.md.

mod auth;
mod client;
mod proto;
mod transport;
mod wire;

use serde::Serialize;
use tokio::sync::broadcast;

use crate::presence::PresenceEvent;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RpcStatus {
    /// No Discord IPC endpoint found; scanning with backoff.
    WaitingForDiscord,
    Connecting,
    /// The consent modal is (or is about to be) open inside the Discord client.
    AwaitingApproval,
    Ready { username: String },
    AuthError { message: String },
    Disconnected,
}

#[derive(Debug, Clone)]
pub enum RpcOut {
    Presence(PresenceEvent),
    Status(RpcStatus),
}

#[derive(Debug, Clone)]
pub struct RpcConfig {
    pub client_id: String,
}

/// Spawns the connection task; subscribe to the returned sender for events.
/// `now_ms` is the shared capture clock so speaking timestamps line up with
/// audio timestamps downstream.
pub fn spawn(
    cfg: RpcConfig,
    now_ms: impl Fn() -> u64 + Send + Sync + 'static,
) -> broadcast::Sender<RpcOut> {
    let (tx, _) = broadcast::channel(256);
    let tx2 = tx.clone();
    tauri::async_runtime::spawn(async move {
        client::run_loop(cfg, tx2, now_ms).await;
    });
    tx
}
