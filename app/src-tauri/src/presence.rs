//! Who is in the voice channel and who is speaking, right now.
//!
//! Real implementation (M1): Discord local RPC/IPC client — see docs/dev/discord-rpc.md.
//! `MockPresence` fakes a channel with a few members and rotating speakers so the
//! overlay UI can be built and demoed without Discord running.

use serde::Serialize;
use tokio::sync::broadcast;

#[derive(Debug, Clone, Serialize)]
pub struct Member {
    pub id: String,
    pub display_name: String,
    /// CSS color used for this member's caption lines (from Discord role color when available).
    pub color: String,
    pub avatar_url: Option<String>,
    pub muted: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PresenceEvent {
    /// The local user's Discord id (from the RPC READY handshake). The local
    /// user's own voice never appears in the captured audio (Discord doesn't
    /// play your mic back), so attribution must exclude them.
    SelfIdentified { user_id: String },
    ChannelJoined { channel_name: String, members: Vec<Member> },
    ChannelLeft,
    MemberJoined { member: Member },
    MemberUpdated { member: Member },
    MemberLeft { user_id: String },
    SpeakingStart { user_id: String, at_ms: u64 },
    SpeakingStop { user_id: String, at_ms: u64 },
}

/// A source of presence events. `subscribe` may be called any number of times.
pub trait PresenceSource: Send + Sync {
    fn subscribe(&self) -> broadcast::Receiver<PresenceEvent>;
}

/// Fake presence feed for development: 3 members, speakers rotate every few seconds.
pub struct MockPresence {
    tx: broadcast::Sender<PresenceEvent>,
}

impl MockPresence {
    pub fn start(now_ms: impl Fn() -> u64 + Send + 'static) -> Self {
        let (tx, _) = broadcast::channel(64);
        let tx2 = tx.clone();
        tauri::async_runtime::spawn(async move {
            let members = vec![
                Member { id: "1".into(), display_name: "Marina".into(), color: "#FEE75C".into(), avatar_url: None, muted: false },
                Member { id: "2".into(), display_name: "Lucas".into(), color: "#57F287".into(), avatar_url: None, muted: false },
                Member { id: "3".into(), display_name: "Rafa".into(), color: "#EB459E".into(), avatar_url: None, muted: true },
            ];
            let _ = tx2.send(PresenceEvent::ChannelJoined {
                channel_name: "Duo Q".into(),
                members: members.clone(),
            });
            let mut i = 0usize;
            loop {
                let speaker = &members[i % 2]; // Rafa is muted, never speaks
                let _ = tx2.send(PresenceEvent::SpeakingStart { user_id: speaker.id.clone(), at_ms: now_ms() });
                tokio::time::sleep(std::time::Duration::from_millis(2600)).await;
                let _ = tx2.send(PresenceEvent::SpeakingStop { user_id: speaker.id.clone(), at_ms: now_ms() });
                tokio::time::sleep(std::time::Duration::from_millis(900)).await;
                i += 1;
            }
        });
        Self { tx }
    }
}

impl PresenceSource for MockPresence {
    fn subscribe(&self) -> broadcast::Receiver<PresenceEvent> {
        self.tx.subscribe()
    }
}
