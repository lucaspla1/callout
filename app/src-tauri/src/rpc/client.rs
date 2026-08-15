//! The RPC connection state machine:
//! Discovering → Handshaking → Authenticating → Ready (subscribe dance + event loop).
//! Single task owns the socket; commands are sequential request/response with
//! dispatches queued in between (docs/dev/discord-rpc.md §5.4, simplified).

use std::collections::VecDeque;
use std::time::Duration;

use rand::Rng;
use serde_json::{json, Value};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::broadcast;

use super::wire::{self, op};
use super::{auth, proto, transport, RpcConfig, RpcOut, RpcStatus};
use crate::presence::{Member, PresenceEvent};

#[derive(Debug, thiserror::Error)]
pub enum CallError {
    #[error("rpc error {code}: {message}")]
    Rpc { code: i64, message: String },
    #[error("transport: {0}")]
    Transport(#[from] std::io::Error),
    #[error("{0}")]
    Protocol(String),
}

const PER_CHANNEL_EVENTS: &[&str] = &[
    "SPEAKING_START",
    "SPEAKING_STOP",
    "VOICE_STATE_CREATE",
    "VOICE_STATE_UPDATE",
    "VOICE_STATE_DELETE",
];

pub async fn run_loop(
    cfg: RpcConfig,
    tx: broadcast::Sender<RpcOut>,
    now_ms: impl Fn() -> u64 + Send + Sync + 'static,
) {
    let mut backoff_s = 2u64;
    let mut fail_streak = 0u32;
    loop {
        let Some(mut conn) = transport::connect_any().await else {
            let _ = tx.send(RpcOut::Status(RpcStatus::WaitingForDiscord));
            let jitter = rand::rng().random_range(0..500);
            tokio::time::sleep(Duration::from_millis(backoff_s * 1000 + jitter)).await;
            backoff_s = (backoff_s * 2).min(30);
            continue;
        };
        backoff_s = 2;
        let _ = tx.send(RpcOut::Status(RpcStatus::Connecting));
        let session_start = std::time::Instant::now();
        if let Err(e) = session(&mut conn, &cfg, &tx, &now_ms).await {
            eprintln!("[rpc] session ended: {e}");
        }
        // A session that lived a while was healthy (Discord quit, channel churn);
        // a short one is a repeating failure — back off harder each time.
        if session_start.elapsed() > Duration::from_secs(60) {
            fail_streak = 0;
        } else {
            fail_streak = fail_streak.saturating_add(1);
        }
        let _ = tx.send(RpcOut::Presence(PresenceEvent::ChannelLeft));
        let _ = tx.send(RpcOut::Status(RpcStatus::Disconnected));
        // The RPC server rate-limits handshakes (~2/min); never hot-loop.
        let cooldown = (10u64 << fail_streak.min(3)).min(60);
        tokio::time::sleep(Duration::from_secs(cooldown)).await;
    }
}

async fn session<S: AsyncRead + AsyncWrite + Unpin>(
    conn: &mut S,
    cfg: &RpcConfig,
    tx: &broadcast::Sender<RpcOut>,
    now_ms: &(impl Fn() -> u64 + Send + Sync),
) -> Result<(), CallError> {
    wire::write_frame(conn, op::HANDSHAKE, &json!({ "v": 1, "client_id": cfg.client_id })).await?;
    let ready = wait_ready(conn).await?;
    let cdn_host = ready
        .pointer("/data/config/cdn_host")
        .and_then(|v| v.as_str())
        .unwrap_or("cdn.discordapp.com")
        .to_string();

    let mut sess = Session { conn, tx, dispatch_q: VecDeque::new(), cdn_host };

    let username = auth::ensure_token(&mut sess, cfg).await?;
    sess.emit_status(RpcStatus::Ready { username });

    // Subscribe dance (§3.4): channel tracking first, then the current channel if any.
    sess.call(json!({ "cmd": "SUBSCRIBE", "evt": "VOICE_CHANNEL_SELECT", "args": {} })).await?;
    let mut current: Option<String> = None;
    let snap = sess.call(json!({ "cmd": "GET_SELECTED_VOICE_CHANNEL", "args": {} })).await?;
    if let Some(id) = snap.get("id").and_then(|v| v.as_str()).map(String::from) {
        current = sess.enter_channel(&id).await?;
    }

    loop {
        while let Some(d) = sess.dispatch_q.pop_front() {
            sess.handle_dispatch(d, &mut current, now_ms).await?;
        }
        let f = wire::read_frame(sess.conn).await?;
        match f.op {
            op::PING => wire::write_frame(sess.conn, op::PONG, &f.json).await?,
            op::CLOSE => return Err(CallError::Protocol(format!("closed: {}", f.json))),
            op::FRAME => {
                if f.json.get("cmd").and_then(|v| v.as_str()) == Some("DISPATCH") {
                    sess.handle_dispatch(f.json, &mut current, now_ms).await?;
                }
            }
            _ => {}
        }
    }
}

async fn wait_ready<S: AsyncRead + AsyncWrite + Unpin>(conn: &mut S) -> Result<Value, CallError> {
    let fut = async {
        loop {
            let f = wire::read_frame(conn).await?;
            match f.op {
                op::PING => wire::write_frame(conn, op::PONG, &f.json).await?,
                op::CLOSE => {
                    return Err(CallError::Protocol(format!("handshake rejected: {}", f.json)))
                }
                op::FRAME
                    if f.json.get("evt").and_then(|v| v.as_str()) == Some("READY") =>
                {
                    return Ok(f.json)
                }
                _ => {}
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(15), fut)
        .await
        .map_err(|_| CallError::Protocol("timeout waiting for READY".into()))?
}

pub struct Session<'a, S> {
    conn: &'a mut S,
    tx: &'a broadcast::Sender<RpcOut>,
    dispatch_q: VecDeque<Value>,
    cdn_host: String,
}

impl<'a, S: AsyncRead + AsyncWrite + Unpin> Session<'a, S> {
    pub(super) async fn call(&mut self, cmd: Value) -> Result<Value, CallError> {
        self.call_with_timeout(cmd, Duration::from_secs(15)).await
    }

    /// Send one command and wait for its nonce-matched reply, answering PINGs and
    /// queueing any dispatches that arrive in between.
    pub(super) async fn call_with_timeout(
        &mut self,
        mut cmd: Value,
        dur: Duration,
    ) -> Result<Value, CallError> {
        let nonce = uuid::Uuid::new_v4().to_string();
        cmd["nonce"] = json!(nonce);
        wire::write_frame(self.conn, op::FRAME, &cmd).await?;
        let conn = &mut *self.conn;
        let dispatch_q = &mut self.dispatch_q;
        let fut = async move {
            loop {
                let f = wire::read_frame(conn).await?;
                match f.op {
                    op::PING => wire::write_frame(conn, op::PONG, &f.json).await?,
                    op::CLOSE => {
                        return Err(CallError::Protocol(format!("closed: {}", f.json)))
                    }
                    op::FRAME => {
                        let ours =
                            f.json.get("nonce").and_then(|v| v.as_str()) == Some(nonce.as_str());
                        if ours {
                            if f.json.get("evt").and_then(|v| v.as_str()) == Some("ERROR") {
                                let code = f
                                    .json
                                    .pointer("/data/code")
                                    .and_then(|v| v.as_i64())
                                    .unwrap_or(0);
                                let message = f
                                    .json
                                    .pointer("/data/message")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown")
                                    .to_string();
                                return Err(CallError::Rpc { code, message });
                            }
                            return Ok(f.json.get("data").cloned().unwrap_or(Value::Null));
                        } else if f.json.get("cmd").and_then(|v| v.as_str()) == Some("DISPATCH") {
                            dispatch_q.push_back(f.json);
                        }
                    }
                    _ => {}
                }
            }
        };
        tokio::time::timeout(dur, fut)
            .await
            .map_err(|_| CallError::Protocol("command timed out".into()))?
    }

    pub(super) fn emit(&self, ev: PresenceEvent) {
        let _ = self.tx.send(RpcOut::Presence(ev));
    }

    pub(super) fn emit_status(&self, s: RpcStatus) {
        let _ = self.tx.send(RpcOut::Status(s));
    }

    /// Subscribe the five per-channel events, then snapshot the roster.
    /// Returns the channel id we ended up in (None if the user already left).
    async fn enter_channel(&mut self, channel_id: &str) -> Result<Option<String>, CallError> {
        for evt in PER_CHANNEL_EVENTS {
            self.call(json!({ "cmd": "SUBSCRIBE", "evt": evt, "args": { "channel_id": channel_id } }))
                .await?;
        }
        let snap = self.call(json!({ "cmd": "GET_SELECTED_VOICE_CHANNEL", "args": {} })).await?;
        match serde_json::from_value::<proto::SelectedChannel>(snap) {
            Ok(ch) => {
                let members: Vec<Member> =
                    ch.voice_states.iter().map(|e| self.member_from(e)).collect();
                let channel_name =
                    if ch.name.is_empty() { "Voice call".to_string() } else { ch.name.clone() };
                self.emit(PresenceEvent::ChannelJoined { channel_name, members });
                Ok(Some(ch.id))
            }
            Err(_) => {
                // Snapshot came back null: user left while we were subscribing.
                self.leave_channel(channel_id).await;
                Ok(None)
            }
        }
    }

    /// Best-effort unsubscribe; stale-unsubscribe errors are harmless.
    async fn leave_channel(&mut self, channel_id: &str) {
        for evt in PER_CHANNEL_EVENTS {
            let _ = self
                .call(json!({ "cmd": "UNSUBSCRIBE", "evt": evt, "args": { "channel_id": channel_id } }))
                .await;
        }
    }

    async fn handle_dispatch(
        &mut self,
        frame: Value,
        current: &mut Option<String>,
        now_ms: &(impl Fn() -> u64 + Send + Sync),
    ) -> Result<(), CallError> {
        let evt = frame.get("evt").and_then(|v| v.as_str()).unwrap_or("");
        let data = frame.get("data").cloned().unwrap_or(Value::Null);
        match evt {
            "VOICE_CHANNEL_SELECT" => {
                if let Some(old) = current.take() {
                    self.leave_channel(&old).await;
                    self.emit(PresenceEvent::ChannelLeft);
                }
                if let Some(id) = data.get("channel_id").and_then(|v| v.as_str()) {
                    *current = self.enter_channel(id).await?;
                }
            }
            "SPEAKING_START" => {
                if let Some(uid) = data.get("user_id").and_then(|v| v.as_str()) {
                    self.emit(PresenceEvent::SpeakingStart { user_id: uid.to_string(), at_ms: now_ms() });
                }
            }
            "SPEAKING_STOP" => {
                if let Some(uid) = data.get("user_id").and_then(|v| v.as_str()) {
                    self.emit(PresenceEvent::SpeakingStop { user_id: uid.to_string(), at_ms: now_ms() });
                }
            }
            "VOICE_STATE_CREATE" | "VOICE_STATE_UPDATE" => {
                if let Ok(entry) = serde_json::from_value::<proto::VoiceStateEntry>(data) {
                    let member = self.member_from(&entry);
                    if evt == "VOICE_STATE_CREATE" {
                        self.emit(PresenceEvent::MemberJoined { member });
                    } else {
                        self.emit(PresenceEvent::MemberUpdated { member });
                    }
                }
            }
            "VOICE_STATE_DELETE" => {
                if let Ok(entry) = serde_json::from_value::<proto::VoiceStateEntry>(data) {
                    self.emit(PresenceEvent::MemberLeft { user_id: entry.user.id });
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn member_from(&self, entry: &proto::VoiceStateEntry) -> Member {
        Member {
            id: entry.user.id.clone(),
            display_name: entry.display_name(),
            color: proto::color_for(&entry.user.id),
            avatar_url: Some(entry.avatar_url(&self.cdn_host)),
            muted: entry.voice_state.self_mute || entry.voice_state.mute,
        }
    }
}
