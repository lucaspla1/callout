mod align;
mod capture;
mod models;
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
const SPEAKER_MODEL_FILE: &str = "models/speaker/wespeaker_en_voxceleb_resnet34_LM.onnx";

/// Ten distinct speaker colors, assigned in arrival order; the 11th person
/// starts reusing them. Readable on the dark caption pills.
const SPEAKER_PALETTE: [&str; 10] = [
    "#57F287", "#FEE75C", "#EB459E", "#5865F2", "#1ABC9C",
    "#E67E22", "#3498DB", "#ED4245", "#B57EDC", "#F4A261",
];

#[derive(Default)]
struct ColorAssigner {
    assigned: HashMap<String, usize>,
    next: usize,
}

impl ColorAssigner {
    fn color(&mut self, user_id: &str) -> String {
        let idx = *self.assigned.entry(user_id.to_string()).or_insert_with(|| {
            let n = self.next;
            self.next += 1;
            n
        });
        SPEAKER_PALETTE[idx % SPEAKER_PALETTE.len()].to_string()
    }
}

/// Status of the captions half (capture + STT), independent of the RPC status.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CaptionsStatus {
    DownloadingModels,
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
fn get_caption_font(state: tauri::State<settings::SettingsHandle>) -> f64 {
    state.caption_font_px()
}

#[tauri::command]
fn set_caption_font(app: AppHandle, state: tauri::State<settings::SettingsHandle>, px: f64) {
    state.set_caption_font_px(px);
    let _ = app.emit("caption_font", state.caption_font_px());
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
            get_caption_font,
            set_caption_font,
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

/// Voiceprint pass over a final's attributed lines:
///  - a line owned by exactly one (indicator-clean) speaker teaches that
///    person's voiceprint (auto-enrollment — solo speech is labeled data);
///  - an ambiguous "N speaking" line is re-embedded and reassigned to the
///    candidate whose voiceprint clearly matches (threshold + margin), which
///    is what separates a talking voice from an open-mic hammering noise.
fn refine_with_voice(
    lines: &mut [align::CaptionLine],
    pcm: &[f32],
    utt_start: u64,
    utt_end: u64,
    voice: &mut Option<stt::voiceid::VoiceId>,
    store: &mut stt::voiceid::VoiceStore,
    roster: &HashMap<String, presence::Member>,
) {
    use stt::voiceid::{self, MIN_ENROLL_MS, MIN_MATCH_MS};
    let Some(vid) = voice.as_mut() else { return };
    if pcm.is_empty() {
        return;
    }
    let ends: Vec<u64> = (0..lines.len())
        .map(|i| lines.get(i + 1).map(|n| n.t_start_ms).unwrap_or(utt_end))
        .collect();
    for (i, line) in lines.iter_mut().enumerate() {
        let t0 = line.t_start_ms.max(utt_start);
        let t1 = ends[i].max(t0);
        let dur = t1 - t0;
        let s0 = (((t0 - utt_start) as usize) * 16).min(pcm.len());
        let s1 = (((t1 - utt_start) as usize) * 16).min(pcm.len());
        let seg = &pcm[s0..s1];
        match line.speaker_ids.clone().as_slice() {
            [one] if dur >= MIN_ENROLL_MS => {
                if let Some(emb) = vid.embed(seg) {
                    store.enroll(one, &emb);
                    let name = roster.get(one).map(|m| m.display_name.as_str()).unwrap_or(one);
                    eprintln!("[voice] learned {} (sample #{}, {}ms)", name, store.samples(one), dur);
                }
            }
            many if many.len() > 1 && dur >= MIN_MATCH_MS => {
                let Some(emb) = vid.embed(seg) else { continue };
                let scored: Vec<(String, f32)> = many
                    .iter()
                    .filter_map(|id| store.similarity(id, &emb).map(|s| (id.clone(), s)))
                    .collect();
                let dump: Vec<String> =
                    scored.iter().map(|(id, s)| format!("{id}:{s:.2}")).collect();
                if let Some(winner) = voiceid::pick_by_similarity(scored) {
                    if let Some(m) = roster.get(&winner) {
                        eprintln!("[voice] '{}' resolved → {} ({})", line.speaker_label, m.display_name, dump.join(" "));
                        line.speaker_ids = vec![winner];
                        line.speaker_label = m.display_name.clone();
                        line.color = m.color.clone();
                    }
                } else if !dump.is_empty() {
                    eprintln!("[voice] kept '{}' ({})", line.speaker_label, dump.join(" "));
                }
            }
            _ => {}
        }
    }
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
    // Models are provisioned first (first run downloads them with progress);
    // caption events flow through a forwarding channel once the pipeline is up.
    let (cstatus_tx, mut cstatus_rx) = tokio::sync::mpsc::unbounded_channel::<CaptionsStatus>();
    let data_dir = app.path().app_data_dir().unwrap_or_default();
    let (stt_fwd_tx, mut stt_rx) = tokio::sync::mpsc::unbounded_channel::<SttEvent>();
    if mock {
        let mut mock_rx = MockStt::start(now_ms);
        let tx = stt_fwd_tx.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(ev) = mock_rx.recv().await {
                if tx.send(ev).is_err() {
                    break;
                }
            }
        });
    } else {
        let app2 = app.clone();
        let settings2 = settings.clone();
        let cstatus2 = cstatus_tx.clone();
        let data_dir2 = data_dir.clone();
        let tx = stt_fwd_tx.clone();
        tauri::async_runtime::spawn(async move {
            if !models::missing(&data_dir2).is_empty() {
                let _ = cstatus2.send(CaptionsStatus::DownloadingModels);
                let app3 = app2.clone();
                if let Err(message) = models::ensure_all(data_dir2.clone(), move |ev| {
                    let _ = app3.emit("model_dl", &ev);
                })
                .await
                {
                    let _ = cstatus2.send(CaptionsStatus::SttError { message });
                    return;
                }
            }
            let (feed, mut rx) = stt::spawn_whisper(
                data_dir2.join(MODEL_FILE),
                settings2.inner.clone(),
                cstatus2.clone(),
            );
            let feed = Mutex::new(feed);
            capture::spawn(now_ms, cstatus2.clone(), move |chunk| {
                if let Ok(mut f) = feed.lock() {
                    f.feed(chunk);
                }
            });
            while let Some(ev) = rx.recv().await {
                if tx.send(ev).is_err() {
                    break;
                }
            }
        });
    }

    // ── Join + fan out to the UI ───────────────────────────────────────────
    // Voiceprint layer: loaded lazily — on first run the speaker model may
    // still be downloading when this task starts.
    let speaker_model_path = data_dir.join(SPEAKER_MODEL_FILE);
    let mut voice: Option<stt::voiceid::VoiceId> = None;
    let mut voice_load_failed = false;
    let mut voice_store = stt::voiceid::VoiceStore::load(data_dir.join("voiceprints.json"));

    tauri::async_runtime::spawn(async move {
        let mut roster: HashMap<String, presence::Member> = HashMap::new();
        let mut log = SpeakingLog::new();
        let mut colors = ColorAssigner::default();
        if mock {
            let _ = app.emit("status", &serde_json::json!({ "state": "mock" }));
        }
        loop {
            tokio::select! {
                Ok(out) = rx.recv() => match out {
                    rpc::RpcOut::Presence(mut ev) => {
                        // Session-stable per-person colors, assigned on first sight.
                        match &mut ev {
                            PresenceEvent::ChannelJoined { members, .. } => {
                                for m in members.iter_mut() {
                                    m.color = colors.color(&m.id);
                                }
                            }
                            PresenceEvent::MemberJoined { member }
                            | PresenceEvent::MemberUpdated { member } => {
                                member.color = colors.color(&member.id);
                            }
                            _ => {}
                        }
                        match &ev {
                            PresenceEvent::ChannelJoined { channel_name, members } => {
                                eprintln!("[callout] joined '{channel_name}' with {} member(s)", members.len());
                                roster = members.iter().cloned().map(|m| (m.id.clone(), m)).collect();
                                log = SpeakingLog::new(); // stale spans must not cross channels
                            }
                            PresenceEvent::ChannelLeft => {
                                roster.clear();
                                log = SpeakingLog::new();
                            }
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
                        SttEvent::Final { text, words, pcm, t_start_ms, t_end_ms } => {
                            eprintln!("[callout] final: {text:?}");
                            if voice.is_none() && !voice_load_failed && speaker_model_path.is_file() {
                                match stt::voiceid::VoiceId::load(&speaker_model_path) {
                                    Ok(v) => {
                                        eprintln!("[voice] speaker model loaded");
                                        voice = Some(v);
                                    }
                                    Err(e) => {
                                        eprintln!("[voice] {e}");
                                        voice_load_failed = true;
                                    }
                                }
                            }
                            let mut lines = align::attribute_final(&log, &roster, &text, &words, t_start_ms, t_end_ms);
                            refine_with_voice(
                                &mut lines,
                                &pcm,
                                t_start_ms,
                                t_end_ms,
                                &mut voice,
                                &mut voice_store,
                                &roster,
                            );
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
