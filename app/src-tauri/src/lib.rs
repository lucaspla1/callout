mod align;
mod capture;
mod diag;
mod models;
mod presence;
mod rpc;
mod settings;
mod stt;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::broadcast;

use crate::align::SpeakingLog;
use crate::presence::PresenceEvent;
use crate::stt::{MockStt, SttEvent};

/// Callout's registered Discord application. RPC for unapproved apps only
/// authorizes the app owner/testers, so users can override this with their own
/// app id via CALLOUT_CLIENT_ID (settings UI comes in M4).
const DEFAULT_DISCORD_CLIENT_ID: &str = "1538241556560085065";

const MODEL_FILE: &str = "models/whisper/ggml-small-q5_1.bin";
const TURBO_MODEL_FILE: &str = "models/whisper/ggml-large-v3-turbo-q5_0.bin";
const SPEAKER_MODEL_FILE: &str = "models/speaker/wespeaker_en_voxceleb_resnet34_LM.onnx";

/// Ten distinct speaker colors, assigned in arrival order; the 11th person
/// starts reusing them. Readable on the dark caption pills.
const SPEAKER_PALETTE: [&str; 10] = [
    "#57F287", "#FEE75C", "#EB459E", "#5865F2", "#1ABC9C", "#E67E22", "#3498DB", "#ED4245",
    "#B57EDC", "#F4A261",
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
fn get_caption_identity(state: tauri::State<settings::SettingsHandle>) -> String {
    state.caption_identity()
}

#[tauri::command]
fn get_overlay_layout(state: tauri::State<settings::SettingsHandle>) -> String {
    state.overlay_layout()
}

#[tauri::command]
fn set_overlay_layout(
    app: AppHandle,
    state: tauri::State<settings::SettingsHandle>,
    layout: String,
) {
    state.set_overlay_layout(layout);
    let layout = state.overlay_layout();
    apply_overlay_size(&app, &layout);
    let _ = app.emit("overlay_layout", layout);
}

/// "Forget learned voices": set by the command, honored by the pipeline loop,
/// which drops its in-memory prints before any enroll can rewrite the file.
static CLEAR_VOICEPRINTS: AtomicBool = AtomicBool::new(false);

#[tauri::command]
fn clear_voiceprints(app: AppHandle) {
    CLEAR_VOICEPRINTS.store(true, Ordering::SeqCst);
    // Delete right away too, so the wipe holds even while no call is active.
    if let Ok(dir) = app.path().app_data_dir() {
        let _ = std::fs::remove_file(dir.join("voiceprints.json"));
        let _ = std::fs::remove_file(dir.join("voiceprints.tmp"));
    }
}

/// Feed mode is a tall column; captions mode is a wide bottom band.
fn apply_overlay_size(app: &AppHandle, layout: &str) {
    if let Some(overlay) = app.get_webview_window("overlay") {
        let (w, h) = if layout == "feed" {
            (520.0, 640.0)
        } else {
            (760.0, 240.0)
        };
        let _ = overlay.set_size(tauri::LogicalSize::new(w, h));
        // Growing the window (captions → feed) can push it past the screen
        // edge — most of the feed column was rendering offscreen.
        clamp_overlay_into_screen(&overlay);
    }
}

/// Keep the overlay fully inside its monitor's bounds (a saved position from
/// another monitor/layout, or a resize, can strand it offscreen).
fn clamp_overlay_into_screen(overlay: &tauri::WebviewWindow) {
    let monitor = overlay
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| overlay.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else { return };
    let (Ok(pos), Ok(size)) = (overlay.outer_position(), overlay.outer_size()) else {
        return;
    };
    let m_pos = monitor.position();
    let m_size = monitor.size();
    let max_x = m_pos.x + (m_size.width as i32 - size.width as i32).max(0);
    let max_y = m_pos.y + (m_size.height as i32 - size.height as i32).max(0);
    let x = pos.x.clamp(m_pos.x, max_x);
    let y = pos.y.clamp(m_pos.y, max_y);
    if x != pos.x || y != pos.y {
        let _ = overlay.set_position(tauri::PhysicalPosition::new(x, y));
    }
}

#[tauri::command]
fn set_caption_identity(
    app: AppHandle,
    state: tauri::State<settings::SettingsHandle>,
    mode: String,
) {
    state.set_caption_identity(mode);
    let _ = app.emit("caption_identity", state.caption_identity());
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
    let Some(overlay) = app.get_webview_window("overlay") else {
        return;
    };
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
    use std::str::FromStr;
    use tauri_plugin_global_shortcut::{Shortcut, ShortcutState};

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
            get_caption_identity,
            set_caption_identity,
            get_overlay_layout,
            set_overlay_layout,
            toggle_move_overlay,
            clear_voiceprints
        ])
        .setup(|app| {
            let data_dir = app.path().app_data_dir().unwrap_or_default();
            let _ = std::fs::create_dir_all(&data_dir);
            diag::init(&data_dir);
            let settings = settings::SettingsHandle::load(data_dir);
            app.manage(settings.clone());
            setup_tray(app.handle())?;
            setup_overlay_window(app.handle(), &settings);
            spawn_pipeline(app.handle().clone(), settings);
            Ok(())
        })
        // Closing the settings window quits nothing: captions must survive it.
        // The app lives in the tray; Quit is a deliberate act there.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|_app, event| {
            if let tauri::RunEvent::Exit = event {
                // whisper.cpp's Metal device has a static destructor that
                // ggml_aborts during normal exit ("Callout quit unexpectedly"
                // on every close). Skip atexit destructors — the OS reclaims
                // everything, and all our state is already persisted.
                #[cfg(unix)]
                unsafe {
                    libc::_exit(0)
                };
            }
        });
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
                    let name = roster
                        .get(one)
                        .map(|m| m.display_name.as_str())
                        .unwrap_or(one);
                    eprintln!(
                        "[voice] learned {} (sample #{}, {}ms)",
                        name,
                        store.samples(one),
                        dur
                    );
                }
            }
            many if many.len() > 1 && dur >= MIN_MATCH_MS => {
                let Some(emb) = vid.embed(seg) else { continue };
                let scored: Vec<(String, f32)> = many
                    .iter()
                    .filter_map(|id| store.similarity(id, &emb).map(|s| (id.clone(), s)))
                    .collect();
                let dump: Vec<String> = scored
                    .iter()
                    .map(|(id, s)| format!("{id}:{s:.2}"))
                    .collect();
                if let Some(winner) = voiceid::pick_by_similarity(scored) {
                    if let Some(m) = roster.get(&winner) {
                        eprintln!(
                            "[voice] '{}' resolved → {} ({})",
                            line.speaker_label,
                            m.display_name,
                            dump.join(" ")
                        );
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
/// Tray icon (Windows: bottom-right notification area; macOS: menu bar). The
/// app is find-and-controllable here even with every window hidden.
fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::TrayIconBuilder;

    let open = MenuItem::with_id(app, "open", "Open Unmute", true, None::<&str>)?;
    let overlay = MenuItem::with_id(app, "overlay", "Show / hide overlay", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Unmute", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &overlay, &quit])?;

    let mut tray = TrayIconBuilder::with_id("unmute")
        .tooltip("Unmute — Discord live captions")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.unminimize();
                    let _ = w.set_focus();
                }
            }
            "overlay" => toggle_overlay(app),
            "quit" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

fn setup_overlay_window(app: &AppHandle, settings: &settings::SettingsHandle) {
    let Some(overlay) = app.get_webview_window("overlay") else {
        return;
    };
    let _ = overlay.set_ignore_cursor_events(true);
    apply_overlay_size(app, &settings.overlay_layout());
    if let Some((x, y)) = settings.overlay_pos() {
        let _ = overlay.set_position(tauri::LogicalPosition::new(x, y));
        // A position saved on another monitor/resolution must not strand the
        // overlay outside the visible screen.
        clamp_overlay_into_screen(&overlay);
    } else if let Ok(Some(monitor)) = overlay.primary_monitor() {
        let scale = monitor.scale_factor();
        let screen_w = monitor.size().width as f64 / scale;
        let screen_h = monitor.size().height as f64 / scale;
        let (w, h) = (760.0, 240.0);
        let _ = overlay.set_position(tauri::LogicalPosition::new(
            (screen_w - w) / 2.0,
            screen_h - h - 60.0,
        ));
    }
    // Some games/apps steal the topmost slot on Windows and the overlay quietly
    // drops behind the game ("the overlay vanished"). Re-assert every few
    // seconds while visible — a no-op when nothing changed. (Exclusive
    // fullscreen bypasses the compositor entirely; docs recommend borderless.)
    #[cfg(windows)]
    {
        let overlay2 = overlay.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                if overlay2.is_visible().unwrap_or(false) {
                    let _ = overlay2.set_always_on_top(true);
                }
            }
        });
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
    // The main subscription (rx) is the Receiver returned by channel creation,
    // so it exists before any source task can send — a send with zero
    // receivers is silently dropped, and the mock's instant ChannelJoined
    // actually lost that race in CI once.
    let (rpc_tx, mut rx) = broadcast::channel::<rpc::RpcOut>(256);
    if mock {
        let (ptx, mut source_rx) = broadcast::channel(64);
        let tx2 = rpc_tx.clone();
        tauri::async_runtime::spawn(async move {
            while let Ok(ev) = source_rx.recv().await {
                let _ = tx2.send(rpc::RpcOut::Presence(ev));
            }
        });
        presence::start_mock_presence(ptx, now_ms);
    } else {
        let client_id = std::env::var("CALLOUT_CLIENT_ID")
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| DEFAULT_DISCORD_CLIENT_ID.to_string());
        rpc::spawn_into(rpc_tx.clone(), rpc::RpcConfig { client_id }, now_ms);
    }

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
            // Turbo is an optional adaptive Finals model. On Windows the worker
            // selects it only for short, naturally-ended utterances with no
            // backlog; live Partials and long/capped Finals stay on Small.
            let turbo_path = Some(data_dir2.join(TURBO_MODEL_FILE));
            let (feed, mut rx) = stt::spawn_whisper(
                data_dir2.join(MODEL_FILE),
                turbo_path,
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
        // The local user's own voice is never in the captured audio (Discord
        // doesn't play your mic back) — exclude them from attribution.
        let mut self_id: Option<String> = None;
        if mock {
            let _ = app.emit("status", &serde_json::json!({ "state": "mock" }));
        }
        loop {
            if CLEAR_VOICEPRINTS.swap(false, Ordering::SeqCst) {
                voice_store.clear();
                eprintln!("[callout] voiceprints cleared");
            }
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
                            PresenceEvent::SelfIdentified { user_id } => {
                                eprintln!("[callout] local user: {user_id} (excluded from attribution)");
                                self_id = Some(user_id.clone());
                            }
                            PresenceEvent::SpeakingStart { user_id, at_ms } => {
                                if self_id.as_deref() != Some(user_id.as_str()) {
                                    log.speaking_start(user_id, *at_ms);
                                }
                            }
                            PresenceEvent::SpeakingStop { user_id, at_ms } => {
                                if self_id.as_deref() != Some(user_id.as_str()) {
                                    log.speaking_stop(user_id, *at_ms);
                                }
                            }
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
                        SttEvent::Partial { text, t_start_ms, t_end_ms } => {
                            let line = align::attribute(&log, &roster, &text, false, t_start_ms, t_end_ms);
                            let _ = app.emit("caption", &line);
                        }
                        SttEvent::Final { text, words, pcm, t_start_ms, t_end_ms } => {
                            // Transcript text is private user content; keep only
                            // a structural marker for smoke tests/diagnostics.
                            eprintln!("[callout] final: received");
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
