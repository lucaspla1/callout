//! Serde types for the slices of the RPC protocol we consume.
//! Tolerant by design: `#[serde(default)]` everywhere, never `deny_unknown_fields` —
//! Discord adds fields without notice. Payload shapes: docs/dev/discord-rpc.md §3.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct User {
    pub id: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub global_name: Option<String>,
    #[serde(default)]
    pub avatar: Option<String>,
    #[serde(default)]
    pub bot: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct InnerVoiceState {
    #[serde(default)]
    pub mute: bool,
    #[serde(default)]
    pub deaf: bool,
    #[serde(default)]
    pub self_mute: bool,
    #[serde(default)]
    pub self_deaf: bool,
    #[serde(default)]
    pub suppress: bool,
}

/// One entry of `voice_states[]` — also the payload of VOICE_STATE_CREATE/UPDATE/DELETE.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct VoiceStateEntry {
    #[serde(default)]
    pub nick: Option<String>,
    /// The *local user's* per-member mute toggle (not the member's own state).
    #[serde(default)]
    pub mute: bool,
    #[serde(default)]
    pub voice_state: InnerVoiceState,
    #[serde(default)]
    pub user: User,
}

/// Response data of GET_SELECTED_VOICE_CHANNEL (when not null).
#[derive(Debug, Clone, Deserialize)]
pub struct SelectedChannel {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub guild_id: Option<String>,
    #[serde(default)]
    pub voice_states: Vec<VoiceStateEntry>,
}

impl VoiceStateEntry {
    /// Display-name precedence per the guide §3.5: nick → global_name → username.
    pub fn display_name(&self) -> String {
        self.nick
            .as_deref()
            .filter(|s| !s.is_empty())
            .or(self.user.global_name.as_deref().filter(|s| !s.is_empty()))
            .unwrap_or(&self.user.username)
            .to_string()
    }

    pub fn avatar_url(&self, cdn_host: &str) -> String {
        match &self.user.avatar {
            Some(hash) if !hash.is_empty() => {
                let ext = if hash.starts_with("a_") { "gif" } else { "png" };
                format!(
                    "https://{cdn_host}/avatars/{}/{hash}.{ext}?size=64",
                    self.user.id
                )
            }
            _ => {
                let idx = self
                    .user
                    .id
                    .parse::<u64>()
                    .map(|v| (v >> 22) % 6)
                    .unwrap_or(0);
                format!("https://{cdn_host}/embed/avatars/{idx}.png")
            }
        }
    }
}

/// Deterministic per-user caption color (role colors aren't exposed over RPC).
pub fn color_for(user_id: &str) -> String {
    const PALETTE: &[&str] = &[
        "#57F287", "#FEE75C", "#EB459E", "#5865F2", "#1ABC9C", "#E67E22", "#3498DB", "#ED4245",
    ];
    let hash: u64 = user_id.bytes().fold(1469598103934665603, |h, b| {
        (h ^ b as u64).wrapping_mul(1099511628211)
    });
    PALETTE[(hash % PALETTE.len() as u64) as usize].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_selected_channel_fixture() {
        // Trimmed real-shape fixture from the guide §3.2.
        let json = serde_json::json!({
            "id": "199737254929760257",
            "guild_id": "199737254929760256",
            "name": "General",
            "type": 2,
            "voice_states": [{
                "nick": "cool nickname",
                "mute": false,
                "volume": 100,
                "voice_state": { "mute": false, "deaf": false, "self_mute": true, "self_deaf": false, "suppress": false },
                "user": { "id": "190320984123768832", "username": "beetroot", "discriminator": "0",
                          "global_name": "Beet Root", "avatar": "b004ec1740a63ca06ae2e14c5cee11f3", "bot": false }
            }]
        });
        let ch: SelectedChannel = serde_json::from_value(json).unwrap();
        assert_eq!(ch.name, "General");
        assert_eq!(ch.voice_states.len(), 1);
        let entry = &ch.voice_states[0];
        assert_eq!(entry.display_name(), "cool nickname");
        assert!(entry.voice_state.self_mute);
        assert!(entry
            .avatar_url("cdn.discordapp.com")
            .contains("/avatars/190320984123768832/"));
    }

    #[test]
    fn display_name_precedence_falls_through() {
        let mut e = VoiceStateEntry::default();
        e.user.username = "beetroot".into();
        assert_eq!(e.display_name(), "beetroot");
        e.user.global_name = Some("Beet Root".into());
        assert_eq!(e.display_name(), "Beet Root");
        e.nick = Some("cool".into());
        assert_eq!(e.display_name(), "cool");
        e.nick = Some(String::new()); // empty nick is ignored
        assert_eq!(e.display_name(), "Beet Root");
    }

    #[test]
    fn default_avatar_when_hash_missing() {
        let mut e = VoiceStateEntry::default();
        e.user.id = "190320984123768832".into();
        assert!(e
            .avatar_url("cdn.discordapp.com")
            .contains("/embed/avatars/"));
    }

    #[test]
    fn color_is_deterministic() {
        assert_eq!(color_for("123"), color_for("123"));
    }
}
