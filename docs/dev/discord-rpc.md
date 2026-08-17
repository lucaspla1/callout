# Discord RPC/IPC Integration Guide

How Unmute learns *who is in the voice channel* and *who is speaking right now* — without a bot,
by talking to the **local Discord client** over its RPC/IPC interface. The protocol is officially
documented but "private beta" gated (§4.4); the command set has been stable for ~8 years and is
what Overlayed, StreamKit, and the Elgato Stream Deck use.

---

## 1. Transport

Use the **IPC transport** (named pipe / unix socket). The alternative WebSocket transport
(`ws://127.0.0.1:6463-6472`) is explicitly deprecated in Discord's docs, requires registering
`rpc_origins` and sending an `Origin` header, and needs a port scan — IPC needs none of that.

### 1.1 Endpoint discovery

Discord (and each running flavor: Stable, PTB, Canary) binds the **first free index** `N` in `0..=9`:

- **Windows**: named pipe `\\.\pipe\discord-ipc-N` (docs write `\\?\pipe\...`; both prefixes work).
- **macOS / Linux**: unix socket `{base}/discord-ipc-N`, where `{base}` is the first set env var of
  `XDG_RUNTIME_DIR`, `TMPDIR`, `TMP`, `TEMP`, falling back to `/tmp`. (On macOS this is normally
  `$TMPDIR` → `/var/folders/.../T/`.)
- **Sandboxed Discord on Linux** additionally: `{base}/snap.discord/discord-ipc-N` (Snap) and
  `{base}/app/com.discordapp.Discord/discord-ipc-N` (Flatpak). Third-party clients use their own
  flatpak id (e.g. `dev.vencord.Vesktop`) — make the path list user-extensible.

**Algorithm**: for N in 0..=9, for each candidate base path, try to connect and complete the
handshake; keep the first success. All-fail ⇒ Discord isn't running (§4.1).

### 1.2 Wire framing

Every message is one frame: an 8-byte header of **two little-endian `u32`s** — `opcode`, then
`payload length` — followed by exactly `length` bytes of UTF-8 JSON.

| Opcode | Name      | Direction | Meaning |
|-------:|-----------|-----------|---------|
| 0      | HANDSHAKE | you → Discord | First frame on the connection |
| 1      | FRAME     | both      | All commands, responses, event dispatches |
| 2      | CLOSE     | Discord → you | JSON body `{"code": int, "message": str}`, then socket closes |
| 3      | PING      | both      | Body echoed back |
| 4      | PONG      | both      | Reply to PING **with the same payload** |

### 1.3 Handshake

Immediately after connecting, send opcode 0:

```json
{ "v": 1, "client_id": "1234567890123456789" }
```

Success ⇒ Discord replies with a FRAME containing the `READY` dispatch:

```json
{ "cmd": "DISPATCH", "evt": "READY",
  "data": {
    "v": 1,
    "config": { "cdn_host": "cdn.discordapp.com", "api_endpoint": "//discord.com/api", "environment": "production" },
    "user": { "id": "53908232506183680", "username": "mason", "discriminator": "0", "global_name": "Mason", "avatar": "8342729096ea3675442027381ff50dfe" }
  } }
```

`data.user` is the logged-in local user (handy: you know "self" before authenticating). Failure
(e.g. unknown `client_id`) ⇒ CLOSE frame, e.g. `{"code": 4000, "message": "Invalid client ID"}`.

Close codes: `4000 INVALID_CLIENTID`, `4001 INVALID_ORIGIN`, `4002 RATELIMITED`,
`4003 TOKEN_REVOKED`, `4004 INVALID_VERSION`, `4005 INVALID_ENCODING`.

---

## 2. Auth: AUTHORIZE → token exchange → AUTHENTICATE

A fresh connection is unauthenticated; most commands fail until you `AUTHENTICATE` with an OAuth2
access token. Full flow (first run only — afterwards a cached token skips straight to AUTHENTICATE):

### 2.1 Scopes

Request **`["rpc", "identify"]`** — the minimum that works: Overlayed ships exactly these two and
calls the same commands/events we need, and Userdoccers lists `GET_SELECTED_VOICE_CHANNEL` as
"`rpc` **or** `rpc.voice.read`". Keep the consent modal minimal; add `rpc.voice.read` only if a
command ever returns `4006 INVALID_PERMISSIONS`.

### 2.2 AUTHORIZE (over the socket)

```json
{ "cmd": "AUTHORIZE", "nonce": "e0d9…-uuid",
  "args": { "client_id": "YOUR_APP_ID", "scopes": ["rpc", "identify"],
            "code_challenge": "BASE64URL(SHA256(verifier))", "code_challenge_method": "S256" } }
```

Discord pops a consent modal in the client; on approval you get
`{"cmd":"AUTHORIZE","data":{"code":"o6a…"},"nonce":"e0d9…"}`. This can take arbitrarily long (human
in the loop) — use a generous timeout (~60 s) and treat `5000 OAUTH2_ERROR` as "user declined".

### 2.3 Code → token exchange **without a client_secret** (the OSS problem)

The code must be exchanged at `POST https://discord.com/api/oauth2/token`
(`application/x-www-form-urlencoded`). Classically this needs `client_secret` — which an MIT desktop
app cannot ship. Options, in order of preference:

1. **PKCE public client (recommended).** In the Developer Portal → OAuth2, enable the **Public
   Client** toggle (`PUBLIC_OAUTH2_CLIENT` flag). Then the exchange needs no secret:
   `grant_type=authorization_code&code=…&code_verifier=…&client_id=…&redirect_uri=…`.
   The `code_challenge`/`code_challenge_method` args on RPC `AUTHORIZE` are documented by
   Userdoccers (reverse-engineered from the live client), **not** in the official RPC docs — so
   spike-test this first; if the client drops the challenge, fall back to option 2.
   Verifier: 43–128 chars of URL-safe random; challenge: unpadded base64url(SHA-256).
2. **Tiny hosted exchange — what Overlayed does.** Its desktop app sends AUTHORIZE with
   `["identify","rpc"]`, then POSTs the code to `https://api.overlayed.dev/token` — a Cloudflare
   Worker (Hono) whose env holds `CLIENT_ID`/`CLIENT_SECRET` — which calls Discord's token endpoint
   and relays the token JSON back verbatim (approach described from their AGPL source; no code
   reused). Works regardless of PKCE support, but you must run infrastructure and sign-in dies
   with your domain.
3. **Embed the secret in the binary/repo — never.** Anyone could mint tokens as "Unmute", and it
   would torpedo a later Discord review.
4. *(Known in the wild, avoid)*: some overlays hijack **StreamKit's** approved `client_id`
   (`207646673902501888`) + an `rpc_token` from StreamKit's endpoint to skip consent and approval —
   impersonation, fragile, and a ToS risk an accessibility app shouldn't take.

**Verified live 2026-08-15 (M1 spike):** do **NOT** pass `redirect_uri` — the RPC flow rejects it
with `5000: "Redirect URI cannot be used in the RPC OAuth2 Authorization flow"`. The code comes
back over the socket, so no redirect exists; omit `redirect_uri` from both the AUTHORIZE args and
the token exchange. (A registered redirect in the portal is harmless but unused.) With the app's
**Public Client** toggle on, the PKCE exchange (`code_verifier`, no secret) succeeded against the
live token endpoint — option 1 is confirmed as the shipping flow; option 2 (hosted exchange) is
not needed.

### 2.4 AUTHENTICATE + token lifecycle

```json
{ "cmd": "AUTHENTICATE", "nonce": "…", "args": { "access_token": "CZhtkLDpNYXgPH9Ml6shqh2OwykChw" } }
```

Response `data` includes `user`, granted `scopes`, `expires` (ISO timestamp), and `application`.
Tokens: `expires_in` is 604800 s (7 days); the exchange also returns a `refresh_token`.

- **Cache** both tokens in the OS keychain (`keyring` crate), never in a plain config file.
- On startup: cached token → AUTHENTICATE. On `4009 INVALID_TOKEN` (or expiry) → refresh grant
  (`grant_type=refresh_token`, no secret for public clients) → retry. Refresh fails → full
  AUTHORIZE again. (Overlayed keeps it simpler: token in localStorage, re-prompt on failure.)

---

## 3. Commands and events

### 3.1 Envelope

Everything on the wire is `{ cmd, nonce?, evt?, args?, data? }`. Rules:

- Every **command** carries a fresh `nonce` (UUID v4); the response echoes it — correlate with a
  `HashMap<Nonce, oneshot::Sender>`. Errors arrive as `"evt": "ERROR"` **with your nonce**.
- Server-pushed events are `"cmd": "DISPATCH"` + `"evt": …` + `data`, no nonce.
- `evt` on a command is only used with `SUBSCRIBE` / `UNSUBSCRIBE`.

### 3.2 Roster snapshot: GET_SELECTED_VOICE_CHANNEL

Request `{ "cmd": "GET_SELECTED_VOICE_CHANNEL", "nonce": "…", "args": {} }`.
`data` is `null` when not in voice, else a channel object:

```json
{ "cmd": "GET_SELECTED_VOICE_CHANNEL", "nonce": "…",
  "data": {
    "id": "199737254929760257", "guild_id": "199737254929760256",
    "name": "General", "type": 2, "bitrate": 64000, "user_limit": 0,
    "voice_states": [
      { "nick": "cool nickname",
        "mute": false, "volume": 100, "pan": { "left": 1.0, "right": 1.0 },
        "voice_state": { "mute": false, "deaf": false, "self_mute": false, "self_deaf": false, "suppress": false },
        "user": { "id": "190320984123768832", "username": "beetroot", "discriminator": "0",
                  "global_name": "Beet Root", "avatar": "b004ec1740a63ca06ae2e14c5cee11f3", "bot": false } }
    ] } }
```

Per entry: outer `mute`/`volume`/`pan` are the *local user's* per-member settings; inner
`voice_state` is the member's real state. `type` 2 = guild voice, 13 = stage; DM calls have no `guild_id`.

### 3.3 Subscriptions

`SUBSCRIBE`/`UNSUBSCRIBE` take the event name in `evt` plus that event's `args`. Response `data` is
just `{ "evt": "…" }` (confirmation). Two kinds matter to us:

**Global (subscribe once, no args) — channel tracking:**

```json
{ "cmd": "SUBSCRIBE", "evt": "VOICE_CHANNEL_SELECT", "nonce": "…", "args": {} }
```

Dispatch: `{ "cmd": "DISPATCH", "evt": "VOICE_CHANNEL_SELECT", "data": { "channel_id": "…"|null, "guild_id": "…"|null } }`
— `null` means the user left voice.

**Per-channel (require `"args": {"channel_id": …}`) — the five we need:**
`SPEAKING_START`, `SPEAKING_STOP`, `VOICE_STATE_CREATE`, `VOICE_STATE_UPDATE`, `VOICE_STATE_DELETE`.

```json
{ "cmd": "SUBSCRIBE", "evt": "SPEAKING_START", "nonce": "…", "args": { "channel_id": "199737254929760257" } }
```

Dispatches:

```json
{ "cmd": "DISPATCH", "evt": "SPEAKING_START", "data": { "channel_id": "199737254929760257", "user_id": "190320984123768832" } }
{ "cmd": "DISPATCH", "evt": "VOICE_STATE_CREATE", "data": { /* one voice_states[] entry, §3.2 */ } }
```

`VOICE_STATE_CREATE`/`DELETE` = member joined/left your channel; `UPDATE` = mute/deafen/nick/volume
change. **SPEAKING events fire for the local user too** — Overlayed's and StreamKit's self-speaking
indicators are driven by them (official docs are silent on this; confirm in the spike). The
`user_id` in `SPEAKING_START` is the key the caption pipeline joins on against the roster.

### 3.4 The resubscribe dance (channel switching)

1. On AUTHENTICATE success: `SUBSCRIBE VOICE_CHANNEL_SELECT` (once per connection), then
   `GET_SELECTED_VOICE_CHANNEL`.
2. If a channel came back: subscribe the five per-channel events for `channel_id`; build the roster
   from `voice_states`.
3. On `VOICE_CHANNEL_SELECT`: UNSUBSCRIBE the five for the old channel (stale-unsubscribe errors
   are harmless — swallow them) and clear roster + speaking set; if `channel_id` is non-null,
   re-run step 2 for it. Always re-snapshot via `GET_SELECTED_VOICE_CHANNEL` after subscribing —
   events alone won't backfill members who were already in the channel.
4. Track live subscriptions in the client so reconnects can replay them (they die with the
   connection).

### 3.5 Identity data → labels and avatars

Display name precedence: **`nick`** (server/channel nickname) → **`user.global_name`** →
**`user.username`** (`discriminator` is `"0"` for migrated accounts; ignore it). Avatars:

- `https://cdn.discordapp.com/avatars/{user_id}/{avatar}.png?size=64` (hash starting `a_` → `.gif`)
- `avatar == null` → `https://cdn.discordapp.com/embed/avatars/{(user_id >> 22) % 6}.png`
- Prefer the `config.cdn_host` from READY over a hard-coded host.

---

## 4. Operational realities

### 4.1 Presence detection, reconnect, backoff

- **Not running**: the full path scan (§1.1) fails ⇒ show "waiting for Discord" and retry the scan
  with backoff: 2 s doubling to a 30 s cap, ±20 % jitter, reset on success. Userdoccers reports a
  **2 connections/minute per-client rate limit** on the RPC server — never hot-loop the handshake.
- **Discord quit/crash**: read returns EOF (or a write fails) ⇒ tear down, emit `Disconnected`,
  re-enter the scan loop. A restarted Discord may land on a *different* pipe index; the updater
  also restarts the client, so expect several EOF/refused cycles in a row.
- Answer PINGs promptly (echo payload as PONG); send your own PING for idle health checks if wanted.

### 4.2 Multiple instances (Stable / PTB / Canary)

Each flavor takes the first free `discord-ipc-N`, so the index carries no meaning — after a reboot
Stable might be `-1` and Canary `-0`; scanning order decides which you get. If the wrong one
answers (user in voice on the other), expose an instance picker, or probe each live socket and
prefer the one whose `GET_SELECTED_VOICE_CHANNEL` is non-null.

### 4.3 Error handling

RPC errors (as `evt: "ERROR"`, `data: {code, message}`): `1000 UNKNOWN_ERROR`,
`4000 INVALID_PAYLOAD`, `4002 INVALID_COMMAND`, `4004 INVALID_EVENT`, `4005 INVALID_CHANNEL`,
`4006 INVALID_PERMISSIONS`, `4009 INVALID_TOKEN`, `4010 INVALID_USER`, `5000 OAUTH2_ERROR`,
`5011 RATE_LIMITED`. Treat `4006`/`4009` as auth-repair triggers (§2.4); log the rest with the
offending nonce'd command.

### 4.4 The approval gate (plan for this early)

Official docs: *"We currently do not allow access to RPC for unapproved apps without being on the
[app]'s list of testers."* Until Discord approves the application, **only its owner/team members
and up to 50 invited App Testers can pass AUTHORIZE**; everyone else gets an OAuth error. There is
**no self-serve approval form** — approved RPC apps are rare (StreamKit, Overlayed, Reactive
Images, Elgato Stream Deck; see Hacksore's tracking gist). Overlayed proves approval is attainable
for an overlay-style app (theirs carries the full `rpc.*` scope family) via direct contact with
Discord developer support — budget months, not days.

**Developer Portal setup (dev + tester phase):** discord.com/developers → New Application (no bot
needed) → OAuth2 page: add redirect `http://127.0.0.1`, enable **Public Client** → App Testers
page: invite by email (≤ 50; testers must accept the emailed invite before they can authorize).

**Distribution constraint:** use the application's invited-tester list while pursuing official
approval. The `CALLOUT_CLIENT_ID` override is useful for controlled development with an application
the operator is authorized to use, but do not ship or promote bring-your-own client IDs as a public
way around the tester ceiling without written confirmation from Discord. The Developer Policy
prohibits attempts to circumvent API limits. Discord now points new projects at the **Social SDK**,
but it targets games embedding Discord features and cannot read the local client's current voice
channel — RPC remains the only currently implemented mechanism for this use case.

---

## 5. Rust implementation plan

### 5.1 Crate survey (checked 2026-08)

| Crate | Version / status | License | Verdict for us |
|---|---|---|---|
| `discord-presence` | 3.2.0, active (Jun 2026) | MIT | Rich Presence + subscribe, but its `Event` enum is only `Ready/Connected/Disconnected/Error/ActivityJoin/ActivitySpectate/ActivityJoinRequest` — no voice events, no arbitrary commands. Sync (threads + crossbeam). **No.** |
| `discord-rich-presence` | 1.1.0 (Jan 2026), 450 k dl | MIT | Deliberately minimal Activity-only IPC; no subscriptions, sync. **No** — but a clean MIT reference for pipe-path discovery/framing (adapt with attribution). |
| `discord-sdk` (Embark) | 0.4.0, **archived May 2026** | MIT/Apache-2.0 | Game-SDK reimplementation (activities, overlay, relationships, users); no voice-channel events. **No.** |
| Overlayed | app, TS over deprecated WebSocket | AGPL-3.0 | Behavioral reference only — **do not copy code** (license-incompatible with MIT). |

**Conclusion (verified):** nothing on crates.io does arbitrary commands + event subscriptions over
IPC. Hand-roll a small client — the protocol is tiny; expect 300–500 lines plus types.

### 5.2 Transport: plain tokio, no extra crate

`tokio` already covers both platforms — `interprocess` is an option but adds nothing we need:

```rust
// rpc/transport.rs
#[cfg(unix)]   pub type Conn = tokio::net::UnixStream;
#[cfg(windows)]pub type Conn = tokio::net::windows::named_pipe::NamedPipeClient;

pub async fn connect_any() -> Option<(Conn, String)> {
    for n in 0..10 {
        for path in candidate_paths(n) {           // §1.1 rules incl. snap/flatpak
            #[cfg(unix)]   let c = tokio::net::UnixStream::connect(&path).await;
            #[cfg(windows)]let c = tokio::net::windows::named_pipe::ClientOptions::new().open(&path);
            if let Ok(conn) = c { return Some((conn, path)); }
        }
    }
    None
}
```

Frame codec (unit-test against byte fixtures):

```rust
// rpc/wire.rs
pub struct Frame { pub op: u32, pub json: serde_json::Value }

pub async fn write_frame<W: AsyncWriteExt + Unpin>(w: &mut W, f: &Frame) -> io::Result<()> {
    let body = serde_json::to_vec(&f.json)?;
    let mut buf = Vec::with_capacity(8 + body.len());
    buf.extend(f.op.to_le_bytes());
    buf.extend((body.len() as u32).to_le_bytes());
    buf.extend(body);
    w.write_all(&buf).await                       // single write per frame
}
pub async fn read_frame<R: AsyncReadExt + Unpin>(r: &mut R) -> io::Result<Frame> {
    let mut hdr = [0u8; 8];
    r.read_exact(&mut hdr).await?;                // EOF here ⇒ Discord went away
    let op  = u32::from_le_bytes(hdr[0..4].try_into().unwrap());
    let len = u32::from_le_bytes(hdr[4..8].try_into().unwrap()) as usize;
    let mut body = vec![0u8; len];                // sanity-cap len (e.g. 8 MiB) before allocating
    r.read_exact(&mut body).await?;
    Ok(Frame { op, json: serde_json::from_slice(&body)? })
}
```

### 5.3 Module layout

```
src-tauri/src/rpc/
  mod.rs        pub API: RpcHandle { events(), snapshot(), state() }
  transport.rs  path candidates + connect_any()
  wire.rs       Frame, opcodes, read/write (+ codec tests)
  proto.rs      serde types: envelope, commands, events, models
  client.rs     the connection task / state machine
  auth.rs       PKCE, token exchange (reqwest), keyring cache, refresh
  mock.rs       replay transport for tests & demo mode
```

Typed protocol core (serde does the heavy lifting; keep unknown fields tolerated —
`#[serde(default)]` everywhere, never `deny_unknown_fields`):

```rust
// rpc/proto.rs
#[derive(Serialize)] #[serde(tag = "cmd", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Command {
    Authorize      { nonce: String, args: AuthorizeArgs },
    Authenticate   { nonce: String, args: AuthenticateArgs },
    GetSelectedVoiceChannel { nonce: String, args: EmptyArgs },
    Subscribe      { nonce: String, evt: EventKind, args: SubscribeArgs },
    Unsubscribe    { nonce: String, evt: EventKind, args: SubscribeArgs },
}

#[derive(Clone, Debug)]                     // what the rest of Unmute consumes
pub enum RpcEvent {
    Connected { self_user: User },
    ChannelJoined { channel: VoiceChannel },   // includes full roster
    ChannelLeft,
    MemberJoined(VoiceStateEntry), MemberUpdated(VoiceStateEntry), MemberLeft { user_id: UserId },
    SpeakingStart { user_id: UserId }, SpeakingStop { user_id: UserId },
    Disconnected { reason: String },
}
```

### 5.4 Connection state machine (`client.rs`)

One spawned task owns the socket; the app talks to it via a command mpsc and listens on a
`tokio::sync::broadcast::Sender<RpcEvent>` (broadcast fits us: overlay UI, caption engine, and
debug log all tap the same stream; slow receivers just lag-drop).

```
Discovering ─connect ok→ Handshaking ─READY→ Authenticating ─ok→ Ready
     ↑  backoff 2s→30s+jitter   │ CLOSE/err        │ 4009→refresh→retry │
     └───────────────────────────┴──────────────────┴── EOF/err ─────────┘
Ready: SUBSCRIBE VOICE_CHANNEL_SELECT → GET_SELECTED_VOICE_CHANNEL → per-channel subs (§3.4)
```

Skeleton of the Ready-state select loop:

```rust
loop {
    tokio::select! {
        frame = wire::read_frame(&mut conn) => match frame {
            Err(_) => break Reconnect,                          // EOF → Discovering
            Ok(f) if f.op == op::PING => wire::write_frame(&mut conn, &f.pong()).await?,
            Ok(f) if f.op == op::CLOSE => break Reconnect,
            Ok(f) => match parse_payload(f.json)? {
                Payload::Reply { nonce, result } =>             // command response or ERROR
                    { if let Some(tx) = pending.remove(&nonce) { let _ = tx.send(result); } }
                Payload::Dispatch(evt) => self.handle_dispatch(evt).await?, // §3.4 dance + fanout
            },
        },
        Some(req) = cmd_rx.recv() => {                          // app-side requests
            pending.insert(req.nonce.clone(), req.reply_tx);
            wire::write_frame(&mut conn, &req.frame).await?;
        },
    }
}
```

`handle_dispatch` implements the resubscribe dance and maintains the canonical roster
(`HashMap<UserId, VoiceStateEntry>` + `HashSet<UserId>` speaking) so late subscribers can request a
`snapshot()` instead of replaying history.

### 5.5 Mock/replay mode (test without Discord)

Make `client.rs` generic over `AsyncRead + AsyncWrite` (it already is, via `wire.rs`). Then:

- **Record**: a `--record-rpc` flag tees every frame to JSONL: `{"ts_ms":…,"dir":"rx"|"tx","op":…,"json":…}`.
- **Replay**: `mock.rs` hands the client one end of a `tokio::io::duplex` pair; a task feeds
  recorded `rx` frames at original (or accelerated) timing and asserts the client's `tx` frames.
- Ship a scrubbed fixture (connect → READY → auth → join → two speakers alternate → leave) so CI
  and a UI demo mode run with zero Discord and no real IDs.
- Unit tests: codec round-trip, nonce correlation, resubscribe-on-switch, EOF → backoff schedule.

### 5.6 Suggested dependencies

`tokio` (net, io-util, sync, time) · `serde`/`serde_json` · `uuid` (nonces) · `reqwest`
(token exchange only) · `sha2` + `base64` (PKCE) · `keyring` (token cache) · `rand` (verifier, jitter).

---

## Sources

- Official RPC topic (payload examples, tester gate, WS-deprecated note): https://docs.discord.com/developers/topics/rpc · OAuth2: https://docs.discord.com/developers/topics/oauth2
- Userdoccers, unofficial but tracks the live client — RPC (IPC paths, opcodes, close/error codes, AUTHORIZE `code_challenge`): https://docs.discord.food/topics/rpc · OAuth2 (PKCE, `PUBLIC_OAUTH2_CLIENT`): https://docs.discord.food/topics/oauth2
- Overlayed, AGPL-3.0 — approach studied, no code reused: https://github.com/overlayeddev/overlayed (`apps/desktop/src/rpc/manager.ts`, CF-Worker exchange `apps/api/src/handlers/token.ts`)
- Approved-RPC-apps tracking gist (Hacksore/Overlayed): https://gist.github.com/Hacksore/24bf9f8a950b740cd914d62975accff0
- Legacy protocol notes ("hard mode"): https://github.com/discord/discord-rpc/blob/master/documentation/hard-mode.md · sandboxed paths: https://github.com/xhayper/discord-rpc
- Crates surveyed: https://crates.io/crates/discord-presence (3.2.0) · https://crates.io/crates/discord-rich-presence (1.1.0, MIT reference impl) · https://github.com/EmbarkStudios/discord-sdk (archived 2026-05)
