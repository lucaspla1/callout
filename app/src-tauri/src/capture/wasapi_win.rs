//! Windows per-process loopback capture of Discord's audio (Win10 2004+).
//! WASAPI process loopback via the `wasapi` crate (0.24): include-process-tree
//! on the root Discord.exe catches the Electron audio-service child.
//! API shapes follow the crate's MIT `record_application` example; quirks per
//! docs/dev/audio-capture.md §1 (no mix format / device period / padding on a
//! process-loopback client — declare 48k stereo f32 + autoconvert, event-driven).

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use wasapi::{AudioClient, Direction, SampleType, StreamMode, WaveFormat};

use super::pipeline::Conditioner;
use super::PcmChunk;
use crate::CaptionsStatus;

const DISCORD_EXES: &[&str] = &["Discord.exe", "DiscordPTB.exe", "DiscordCanary.exe"];
const NATIVE_RATE: u32 = 48_000;

/// The root Discord.exe: a Discord process whose parent is not itself Discord
/// (its real parent is Squirrel's Update.exe, usually already gone).
fn find_discord_root() -> Option<u32> {
    use std::ffi::OsStr;
    use sysinfo::{ProcessRefreshKind, RefreshKind, System};
    let system = System::new_with_specifics(
        RefreshKind::nothing().with_processes(ProcessRefreshKind::nothing()),
    );
    let is_discord =
        |name: &OsStr| DISCORD_EXES.iter().any(|n| name == OsStr::new(n));
    system
        .processes()
        .values()
        .find(|p| {
            is_discord(p.name())
                && p.parent()
                    .and_then(|pp| system.process(pp))
                    .map(|pp| !is_discord(pp.name()))
                    .unwrap_or(true)
        })
        .map(|p| p.pid().as_u32())
}

pub fn spawn_supervisor(
    now_ms: impl Fn() -> u64 + Send + Sync + Clone + 'static,
    status_tx: tokio::sync::mpsc::UnboundedSender<CaptionsStatus>,
    mut sink: impl FnMut(PcmChunk) + Send + 'static,
) {
    std::thread::Builder::new()
        .name("callout-capture".into())
        .spawn(move || {
            // COM for this thread; HRESULT, not Result.
            if wasapi::initialize_mta().ok().is_err() {
                let _ = status_tx.send(CaptionsStatus::CaptureError {
                    message: "COM initialization failed".into(),
                });
                return;
            }
            loop {
                let Some(pid) = find_discord_root() else {
                    let _ = status_tx.send(CaptionsStatus::WaitingForDiscordAudio);
                    std::thread::sleep(Duration::from_secs(2));
                    continue;
                };
                match run_session(pid, &now_ms, &status_tx, &mut sink) {
                    Ok(()) => { /* Discord restarted — re-discover immediately */ }
                    Err(message) => {
                        eprintln!("[capture] {message}");
                        let _ = status_tx.send(CaptionsStatus::CaptureError { message });
                        std::thread::sleep(Duration::from_secs(5));
                    }
                }
            }
        })
        .expect("spawn capture thread");
}

/// One capture session against a fixed root PID; returns Ok(()) when the
/// Discord process set changes (caller re-discovers and restarts).
fn run_session(
    pid: u32,
    now_ms: &(impl Fn() -> u64 + Send + Sync + Clone + 'static),
    status_tx: &tokio::sync::mpsc::UnboundedSender<CaptionsStatus>,
    sink: &mut (impl FnMut(PcmChunk) + Send),
) -> Result<(), String> {
    // Declared format (GetMixFormat is unavailable on process-loopback clients);
    // autoconvert makes the engine deliver it regardless of the device mix.
    let format = WaveFormat::new(32, 32, &SampleType::Float, NATIVE_RATE as usize, 2, None);
    let blockalign = format.get_blockalign() as usize; // 8 bytes: 2ch × f32

    let mut client = AudioClient::new_application_loopback_client(pid, true)
        .map_err(|e| format!("loopback client (pid {pid}): {e}"))?;
    let mode = StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: 0 };
    client
        .initialize_client(&format, &Direction::Capture, &mode)
        .map_err(|e| format!("initialize: {e}"))?;
    let h_event = client.set_get_eventhandle().map_err(|e| format!("event handle: {e}"))?;
    let capture = client.get_audiocaptureclient().map_err(|e| format!("capture client: {e}"))?;
    client.start_stream().map_err(|e| format!("start: {e}"))?;

    let _ = status_tx.send(CaptionsStatus::Capturing { native_rate: NATIVE_RATE });
    let mut conditioner = Conditioner::new(NATIVE_RATE, now_ms.clone())?;

    let mut queue: VecDeque<u8> = VecDeque::new();
    let mut mono: Vec<f32> = Vec::with_capacity(4096);
    let mut zeros: Vec<f32> = Vec::new();
    let mut last_pid_check = Instant::now();

    // Wall-clock accounting: process loopback goes quiet whenever Discord
    // renders nothing, but the VAD needs silence *chunks* to endpoint an
    // utterance — without them, speech only finalizes at the max-utterance cap,
    // chopped mid-word. Backfill droughts with zeros to keep the clock ticking.
    let session_t0 = Instant::now();
    let mut fed_frames: u64 = 0; // native frames handed to the conditioner
    const GAP_FILL_THRESHOLD: u64 = NATIVE_RATE as u64 * 150 / 1000; // 150 ms
    const GAP_FILL_MAX: u64 = NATIVE_RATE as u64; // ≤1 s per iteration

    // Capture health, reported every 5 s via diag.
    let (mut st_packets, mut st_frames, mut st_fills, mut st_fill_ms) = (0u64, 0u64, 0u64, 0u64);
    let mut st_last = Instant::now();

    loop {
        // Silence = no packets and no events; a timeout here means idle, not failure.
        let _ = h_event.wait_for_event(500);
        // Drain EVERYTHING available — one packet per wakeup falls behind
        // realtime and the backlog becomes ever-growing caption delay.
        while let Ok(Some(frames)) = capture.get_next_packet_size() {
            if frames == 0 {
                break;
            }
            capture
                .read_from_device_to_deque(&mut queue)
                .map_err(|e| format!("read: {e}"))?;
            st_packets += 1;
        }
        let whole_frames = queue.len() / blockalign;
        if whole_frames > 0 {
            mono.clear();
            mono.reserve(whole_frames);
            for _ in 0..whole_frames {
                let mut b = [0u8; 4];
                for slot in b.iter_mut() {
                    *slot = queue.pop_front().unwrap_or(0);
                }
                let left = f32::from_le_bytes(b);
                for slot in b.iter_mut() {
                    *slot = queue.pop_front().unwrap_or(0);
                }
                let right = f32::from_le_bytes(b);
                mono.push(0.5 * (left + right));
            }
            fed_frames += mono.len() as u64;
            st_frames += mono.len() as u64;
            for chunk in conditioner.feed(&mono) {
                sink(chunk);
            }
        }
        // Backfill a packet drought with silence (after real data is drained,
        // so zeros never splice into speech that merely arrived late).
        let expected = session_t0.elapsed().as_millis() as u64 * (NATIVE_RATE as u64) / 1000;
        if expected > fed_frames + GAP_FILL_THRESHOLD {
            let missing = (expected - fed_frames).min(GAP_FILL_MAX) as usize;
            zeros.clear();
            zeros.resize(missing, 0.0);
            fed_frames += missing as u64;
            st_fills += 1;
            st_fill_ms += missing as u64 * 1000 / NATIVE_RATE as u64;
            for chunk in conditioner.feed(&zeros) {
                sink(chunk);
            }
        }
        if st_last.elapsed() >= Duration::from_secs(5) {
            st_last = Instant::now();
            crate::diag::log(&format!(
                "capture 5s: packets={st_packets} frames={st_frames} fills={st_fills} fill_ms={st_fill_ms} backlog_frames={}",
                queue.len() / blockalign
            ));
            (st_packets, st_frames, st_fills, st_fill_ms) = (0, 0, 0, 0);
        }
        if last_pid_check.elapsed() >= Duration::from_secs(2) {
            last_pid_check = Instant::now();
            if find_discord_root() != Some(pid) {
                let _ = client.stop_stream();
                return Ok(());
            }
        }
    }
}
