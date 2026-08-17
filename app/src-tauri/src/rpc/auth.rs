//! OAuth2 over RPC: AUTHORIZE (PKCE, no client_secret) → token exchange →
//! AUTHENTICATE, with tokens cached in the OS keychain.
//! Requires the Discord application to have "Public Client" enabled and
//! `http://127.0.0.1` registered as a redirect URI. See docs/dev/discord-rpc.md §2.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncRead, AsyncWrite};

use super::client::{CallError, Session};
use super::{RpcConfig, RpcStatus};

const TOKEN_URL: &str = "https://discord.com/api/oauth2/token";
#[cfg_attr(debug_assertions, allow(dead_code))]
const KEYRING_SERVICE: &str = "app.callout.desktop";
#[cfg_attr(debug_assertions, allow(dead_code))]
const KEYRING_ACCOUNT: &str = "discord-oauth";

#[derive(Serialize, Deserialize)]
struct CachedTokens {
    access_token: String,
    refresh_token: Option<String>,
    expires_at: u64,
}

pub async fn ensure_token<S: AsyncRead + AsyncWrite + Unpin>(
    sess: &mut Session<'_, S>,
    cfg: &RpcConfig,
) -> Result<String, CallError> {
    // Fast path: cached (possibly refreshed) token → AUTHENTICATE.
    if let Some(mut tokens) = load_cached() {
        if tokens.expires_at <= now_epoch() + 60 {
            match &tokens.refresh_token {
                Some(refresh) => {
                    match exchange(
                        &cfg.client_id,
                        &[("grant_type", "refresh_token"), ("refresh_token", refresh)],
                    )
                    .await
                    {
                        Ok(fresh) => {
                            save_cached(&fresh);
                            tokens = fresh;
                        }
                        Err(_) => {
                            clear_cached();
                            tokens.expires_at = 0;
                        }
                    }
                }
                None => clear_cached(),
            }
        }
        if tokens.expires_at > now_epoch() + 60 {
            match authenticate(sess, &tokens.access_token).await {
                Ok(username) => return Ok(username),
                // Token revoked or invalid (e.g. 4009): fall through to a full authorize.
                Err(CallError::Rpc { .. }) => clear_cached(),
                Err(e) => return Err(e),
            }
        }
    }

    // Full flow: consent modal inside Discord (human in the loop — generous timeout).
    sess.emit_status(RpcStatus::AwaitingApproval);
    let mut verifier_bytes = [0u8; 64];
    rand::rng().fill_bytes(&mut verifier_bytes);
    let verifier = URL_SAFE_NO_PAD.encode(verifier_bytes);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));

    let authorize = sess
        .call_with_timeout(
            json!({
                "cmd": "AUTHORIZE",
                // No redirect_uri: Discord rejects it in the RPC flow ("Redirect URI
                // cannot be used in the RPC OAuth2 Authorization flow") — the code is
                // handed back over the socket, so none is needed at exchange either.
                "args": {
                    "client_id": cfg.client_id,
                    "scopes": ["rpc", "identify"],
                    "code_challenge": challenge,
                    "code_challenge_method": "S256",
                }
            }),
            Duration::from_secs(300),
        )
        .await
        .map_err(|e| {
            sess.emit_status(RpcStatus::AuthError {
                message: e.to_string(),
            });
            e
        })?;
    let code = authorize
        .get("code")
        .and_then(|v| v.as_str())
        .ok_or_else(|| CallError::Protocol("AUTHORIZE returned no code".into()))?;

    let tokens = exchange(
        &cfg.client_id,
        &[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("code_verifier", &verifier),
        ],
    )
    .await
    .map_err(|e| {
        sess.emit_status(RpcStatus::AuthError {
            message: e.to_string(),
        });
        e
    })?;
    save_cached(&tokens);
    authenticate(sess, &tokens.access_token).await
}

async fn authenticate<S: AsyncRead + AsyncWrite + Unpin>(
    sess: &mut Session<'_, S>,
    access_token: &str,
) -> Result<String, CallError> {
    let data = sess
        .call(json!({ "cmd": "AUTHENTICATE", "args": { "access_token": access_token } }))
        .await?;
    Ok(data
        .pointer("/user/username")
        .and_then(|v| v.as_str())
        .unwrap_or("connected")
        .to_string())
}

/// POST to Discord's token endpoint. Public-client (PKCE) — no client_secret anywhere.
async fn exchange(client_id: &str, params: &[(&str, &str)]) -> Result<CachedTokens, CallError> {
    let mut form: Vec<(&str, &str)> = vec![("client_id", client_id)];
    form.extend_from_slice(params);
    let resp = reqwest::Client::new()
        .post(TOKEN_URL)
        .form(&form)
        .send()
        .await
        .map_err(|e| CallError::Protocol(format!("token endpoint unreachable: {e}")))?;
    let status = resp.status();
    let body: Value = resp
        .json()
        .await
        .map_err(|e| CallError::Protocol(format!("token endpoint bad body: {e}")))?;
    if !status.is_success() {
        return Err(CallError::Protocol(format!(
            "token exchange failed ({status}): {body}"
        )));
    }
    let access_token = body
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| CallError::Protocol(format!("no access_token in response: {body}")))?
        .to_string();
    Ok(CachedTokens {
        access_token,
        refresh_token: body
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .map(String::from),
        expires_at: now_epoch()
            + body
                .get("expires_in")
                .and_then(|v| v.as_u64())
                .unwrap_or(3600),
    })
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// Token cache is a plain file for ALL builds until the app ships with a real
// signing identity: ad-hoc-signed builds change identity on every rebuild, so
// the keychain silently denies access to items the previous build created —
// which forced a fresh Discord authorization on every launch. Restore the
// keychain store (below, cfg'd off) once Developer ID signing lands.

mod store {
    use super::CachedTokens;

    /// Same directory the rest of the app uses (Tauri's app_data_dir for
    /// identifier app.callout.desktop) — recreated here because the RPC layer
    /// has no AppHandle. HOME is unset on Windows, which silently disabled the
    /// cache there (re-auth on every launch).
    fn data_dir() -> Option<std::path::PathBuf> {
        #[cfg(windows)]
        {
            let roaming = std::env::var("APPDATA").ok()?;
            Some(std::path::PathBuf::from(roaming).join("app.callout.desktop"))
        }
        #[cfg(not(windows))]
        {
            let home = std::env::var("HOME").ok()?;
            Some(
                std::path::PathBuf::from(home)
                    .join("Library/Application Support/app.callout.desktop"),
            )
        }
    }

    fn path() -> Option<std::path::PathBuf> {
        Some(data_dir()?.join("tokens.json"))
    }

    fn legacy_path() -> Option<std::path::PathBuf> {
        Some(data_dir()?.join("dev-tokens.json"))
    }

    pub fn load() -> Option<CachedTokens> {
        let read = |p: std::path::PathBuf| {
            std::fs::read_to_string(p)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
        };
        path()
            .and_then(read)
            .or_else(|| legacy_path().and_then(read))
    }

    pub fn save(tokens: &CachedTokens) {
        let (Some(p), Ok(json)) = (path(), serde_json::to_string(tokens)) else {
            return;
        };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, json);
    }

    pub fn clear() {
        if let Some(p) = path() {
            let _ = std::fs::remove_file(p);
        }
    }
}

// Disabled until the app has a stable signing identity (see note above).
#[cfg(any())]
mod keychain_store {
    use super::{CachedTokens, KEYRING_ACCOUNT, KEYRING_SERVICE};

    pub fn load() -> Option<CachedTokens> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT).ok()?;
        entry
            .get_password()
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
    }

    pub fn save(tokens: &CachedTokens) {
        if let (Ok(entry), Ok(serialized)) = (
            keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT),
            serde_json::to_string(tokens),
        ) {
            let _ = entry.set_password(&serialized);
        }
    }

    pub fn clear() {
        if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT) {
            let _ = entry.delete_credential();
        }
    }
}

fn load_cached() -> Option<CachedTokens> {
    store::load()
}

fn save_cached(tokens: &CachedTokens) {
    store::save(tokens);
}

fn clear_cached() {
    store::clear();
}
