mod align;
mod capture;
mod presence;
mod rpc;
mod settings;
mod stt;

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::broadcast;

use crate::align::SpeakingLog;
use crate::presence::{MockPresence, PresenceEvent, PresenceSource};
use crate::stt::{MockStt, SttEvent};

/// Callout's registered Discord application. RPC for unapproved apps only
/// authorizes the app owner/testers, so users can override this with their own
/// app id via CALLOUT_CLIENT_ID (settings UI comes in M4).
const DEFAULT_DISCORD_CLIENT_ID: &str = "1538241556560085065";

const MODEL_FILE: &str = "models/whisper/ggml-small-q5_1.bin";

/// Status of the captions half (capture + STT), independent of the RPC status.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CaptionsStatus {
    ModelMissing { path: String },
    LoadingModel,
    SttReady,
    WaitingForDiscordAudio,
    Capturing { native_rate: u32 },
    CaptureError { message: String },
    SttError { message: String },
}

#[tauri::command]
fn get_languages(state: tauri::State<settings::SettingsHandle>) -> Vec<String> {
    state.languages()
}

#[tauri::command]
fn set_languages(state: tauri::State<settings::SettingsHandle>, languages: Vec<String>) {
    state.set_languages(languages);
}

#[tauri::command]
fn get_overlay_opacity(state: tauri::State<settings::SettingsHandle>) -> f64 {
    state.overlay_opacity()
}

/// UI fallback for the move hotkey (hotkeys can lose conflicts to other apps).
#[tauri::command]
fn toggle_move_overlay(app: AppHandle) {
    toggle_move_mode(&app);
}

#[tauri::command]
fn set_overlay_opacity(
    app: AppHandle,
    state: tauri::State<settings::SettingsHandle>,
    opacity: f64,
) {
    state.set_overlay_opacity(opacity);
    let _ = app.emit("overlay_opacity", state.overlay_opacity());
}

/// Toggle hotkey for the caption overlay; move hotkey unlocks dragging.
/// (Move is ⌘⇧O, not ⌘⇧M — Discord's own global mute hotkey owns ⌘⇧M.)
const TOGGLE_SHORTCUT: &str = "CmdOrCtrl+Shift+C";
const MOVE_SHORTCUT: &str = "CmdOrCtrl+Shift+O";

static MOVE_MODE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn toggle_overlay(app: &AppHandle) {
    if let Some(overlay) = app.get_webview_window("overlay") {
        if overlay.is_visible().unwrap_or(false) {
            let _ = overlay.hide();
        } else {
            let _ = overlay.show();
        }
    }
}

/// Move mode: makes the overlay clickable + draggable; leaving it saves the
/// position and restores click-through.
fn toggle_move_mode(app: &AppHandle) {
    use std::sync::atomic::Ordering;
    let Some(overlay) = app.get_webview_window("overlay") else { return };
    let entering = !MOVE_MODE.load(Ordering::Relaxed);
    MOVE_MODE.store(entering, Ordering::Relaxed);
    if entering {
        let _ = overlay.show();
        let _ = overlay.set_ignore_cursor_events(false);
        let _ = overlay.set_focus();
    } else {
        let _ = overlay.set_ignore_cursor_events(true);
        if let (Ok(pos), Ok(scale)) = (overlay.outer_position(), overlay.scale_factor()) {
            let logical = pos.to_logical::<f64>(scale);
            if let Some(settings) = app.try_state::<settings::SettingsHandle>() {
                settings.set_overlay_pos(logical.x, logical.y);
            }
        }
    }
    let _ = app.emit("overlay_move_mode", entering);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    use tauri_plugin_global_shortcut::{Shortcut, ShortcutState};
    use std::str::FromStr;

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_shortcuts([TOGGLE_SHORTCUT, MOVE_SHORTCUT])
                .expect("valid shortcuts")
                .with_handler(|app, shortcut, event| {
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }
                    eprintln!("[callout] hotkey: {shortcut}");
                    let move_sc = Shortcut::from_str(MOVE_SHORTCUT).expect("valid");
                    if shortcut == &move_sc {
                        toggle_move_mode(app);
                    } else {
                        toggle_overlay(app);
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            get_languages,
            set_languages,
            get_overlay_opacity,
            set_overlay_opacity,
            toggle_move_overlay
        ])
        .setup(|app| {
            let data_dir = app.path().app_data_dir().unwrap_or_default();
            let settings = settings::SettingsHandle::load(data_dir);
            app.manage(settings.clone());
            setup_overlay_window(app.handle(), &settings);
            spawn_pipeline(app.handle().clone(), settings);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Overlay: click-through, at the saved position or parked bottom-center.
fn setup_overlay_window(app: &AppHandle, settings: &settings::SettingsHandle) {
    let Some(overlay) = app.get_webview_window("overlay") else { return };
    let _ = overlay.set_ignore_cursor_events(true);
    if let Some((x, y)) = settings.overlay_pos() {
        let _ = overlay.set_position(tauri::LogicalPosition::new(x, y));
        return;
    }
    if let Ok(Some(monitor)) = overlay.primary_monitor() {
        let scale = monitor.scale_factor();
        let screen_w = monitor.size().width as f64 / scale;
        let screen_h = monitor.size().height as f64 / scale;
        let (w, h) = (760.0, 240.0);
        let _ = overlay.set_position(tauri::LogicalPosition::new(
            (screen_w - w) / 2.0,
            screen_h - h - 60.0,
        ));
    }
}

/// Wires the whole thing together:
///   presence (Discord RPC, or mock) ─┐
///                                    ├─► attribution ─► "presence"/"caption"/"status" events
///   capture ─► VAD ─► whisper ───────┘
fn spawn_pipeline(app: AppHandle, settings: settings::SettingsHandle) {
    let start = Instant::now();
    let now_ms = move || start.elapsed().as_millis() as u64;
    let mock = std::env::var("CALLOUT_MOCK").ok().as_deref() == Some("1");

    // ── Presence source ────────────────────────────────────────────────────
    let rpc_tx: broadcast::Sender<rpc::RpcOut> = if mock {
        let (tx, _) = broadcast::channel(256);
        let source = MockPresence::start(now_ms);
        let mut source_rx = source.subscribe();
        let tx2 = tx.clone();
        tauri::async_runtime::spawn(async move {
            while let Ok(ev) = source_rx.recv().await {
                let _ = tx2.send(rpc::RpcOut::Presence(ev));
            }
        });
        tx
    } else {
        let client_id = std::env::var("CALLOUT_CLIENT_ID")
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| DEFAULT_DISCORD_CLIENT_ID.to_string());
        rpc::spawn(rpc::RpcConfig { client_id }, now_ms)
    };
    let mut rx = rpc_tx.subscribe();

    // ── Captions source (capture → VAD → whisper), or mock ────────────────
    let (cstatus_tx, mut cstatus_rx) = tokio::sync::mpsc::unbounded_channel::<CaptionsStatus>();
    let mut stt_rx: tokio::sync::mpsc::UnboundedReceiver<SttEvent> = if mock {
        MockStt::start(now_ms)
    } else {
        let model_path = app
            .path()
            .app_data_dir()
            .map(|d| d.join(MODEL_FILE))
            .unwrap_or_default();
        if model_path.is_file() {
            let (feed, rx) = stt::spawn_whisper(model_path, settings.inner.clone(), cstatus_tx.clone());
            let feed = Mutex::new(feed);
            capture::spawn(now_ms, cstatus_tx.clone(), move |chunk| {
                if let Ok(mut f) = feed.lock() {
                    f.feed(chunk);
                }
            });
            rx
        } else {
            let _ = cstatus_tx.send(CaptionsStatus::ModelMissing {
                path: model_path.to_string_lossy().to_string(),
            });
            let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
            rx
        }
    };

    // ── Join + fan out to the UI ───────────────────────────────────────────
    tauri::async_runtime::spawn(async move {
        let mut roster: HashMap<String, presence::Member> = HashMap::new();
        let mut log = SpeakingLog::new();
        if mock {
            let _ = app.emit("status", &serde_json::json!({ "state": "mock" }));
        }
        loop {
            tokio::select! {
                Ok(out) = rx.recv() => match out {
                    rpc::RpcOut::Presence(ev) => {
                        match &ev {
                            PresenceEvent::ChannelJoined { channel_name, members } => {
                                eprintln!("[callout] joined '{channel_name}' with {} member(s)", members.len());
                                roster = members.iter().cloned().map(|m| (m.id.clone(), m)).collect();
                            }
                            PresenceEvent::ChannelLeft => roster.clear(),
                            PresenceEvent::MemberJoined { member } | PresenceEvent::MemberUpdated { member } => {
                                roster.insert(member.id.clone(), member.clone());
                            }
                            PresenceEvent::MemberLeft { user_id } => {
                                roster.remove(user_id);
                            }
                            PresenceEvent::SpeakingStart { user_id, at_ms } => log.speaking_start(user_id, *at_ms),
                            PresenceEvent::SpeakingStop { user_id, at_ms } => log.speaking_stop(user_id, *at_ms),
                        }
                        let _ = app.emit("presence", &ev);
                    }
                    rpc::RpcOut::Status(s) => {
                        eprintln!("[callout] status: {s:?}");
                        let _ = app.emit("status", &s);
                    }
                },
                Some(ev) = stt_rx.recv() => {
                    match ev {
                        SttEvent::Partial { text, t_start_ms } => {
                            let line = align::attribute(&log, &roster, &text, false, t_start_ms, now_ms());
                            let _ = app.emit("caption", &line);
                        }
                        SttEvent::Final { text, words, t_start_ms, t_end_ms } => {
                            eprintln!("[callout] final: {text:?}");
                            let lines = align::attribute_final(&log, &roster, &text, &words, t_start_ms, t_end_ms);
                            // Diagnostic while attribution 2.0 settles: when a joint
                            // line survives, dump word timings to judge whisper's spans.
                            if lines.iter().any(|l| l.speaker_ids.len() > 1) {
                                let spans: Vec<String> = words
                                    .iter()
                                    .map(|w| format!("{}@{}-{}", w.text, w.t0_ms, w.t1_ms))
                                    .collect();
                                eprintln!("[align] joint survived; words: {}", spans.join(" "));
                            }
                            for line in lines {
                                let _ = app.emit("caption", &line);
                            }
                        }
                    }
                }
                Some(cs) = cstatus_rx.recv() => {
                    eprintln!("[callout] captions: {cs:?}");
                    let _ = app.emit("captions_status", &cs);
                }
                else => break,
            }
        }
    });
}
