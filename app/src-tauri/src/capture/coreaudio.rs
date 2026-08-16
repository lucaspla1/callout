//! macOS Core Audio process-tap capture of Discord's audio output.
//! Pattern follows cidre's `core-audio-record` example (MIT) and AudioCap
//! (BSD-2-Clause): tap → private aggregate device → IOProc.
//! See docs/dev/audio-capture.md §2.

use std::time::Duration;

use cidre::{arc, cat, cf, core_audio as ca, ns, os};

use ca::aggregate_device_keys as agg_keys;
use ca::sub_device_keys as sub_keys;

use super::pipeline::Conditioner;
use super::PcmChunk;
use crate::CaptionsStatus;

/// Raw mono blocks from the IOProc (native rate) to the conditioner thread.
struct IoCtx {
    tx: crossbeam_channel::Sender<Vec<f32>>,
    channels: usize,
    scratch: Vec<f32>,
}

extern "C" fn io_proc(
    _device: ca::Device,
    _now: &cat::AudioTimeStamp,
    input_data: &cat::AudioBufList<2>,
    _input_time: &cat::AudioTimeStamp,
    _output_data: &mut cat::AudioBufList<2>,
    _output_time: &cat::AudioTimeStamp,
    ctx: Option<&mut IoCtx>,
) -> os::Status {
    let Some(ctx) = ctx else { return Default::default() };
    let nb = (input_data.number_buffers as usize).min(2);
    ctx.scratch.clear();
    if nb == 0 {
        return Default::default();
    }
    unsafe {
        if nb == 1 {
            let buf = &input_data.buffers[0];
            let n = (buf.data_bytes_size as usize) / 4;
            let samples = std::slice::from_raw_parts(buf.data as *const f32, n);
            let ch = (buf.number_channels as usize).max(ctx.channels).max(1);
            if ch == 1 {
                ctx.scratch.extend_from_slice(samples);
            } else {
                let frames = n / ch;
                let gain = 1.0 / ch as f32;
                for f in 0..frames {
                    let mut acc = 0.0f32;
                    for c in 0..ch {
                        acc += samples[f * ch + c];
                    }
                    ctx.scratch.push(acc * gain);
                }
            }
        } else {
            // Non-interleaved: one buffer per channel; average them.
            let b0 = &input_data.buffers[0];
            let b1 = &input_data.buffers[1];
            let n = ((b0.data_bytes_size.min(b1.data_bytes_size)) as usize) / 4;
            let s0 = std::slice::from_raw_parts(b0.data as *const f32, n);
            let s1 = std::slice::from_raw_parts(b1.data as *const f32, n);
            for i in 0..n {
                ctx.scratch.push(0.5 * (s0[i] + s1[i]));
            }
        }
    }
    // Real-time thread: never block. Drop the block if the consumer is behind.
    let _ = ctx.tx.try_send(std::mem::take(&mut ctx.scratch));
    Default::default()
}

/// Built-in output if present (rock-steady clock), else the default output.
fn stable_clock_uid() -> Result<(cidre::arc::R<cf::String>, String), String> {
    if let Ok(devices) = ca::System::devices() {
        for d in devices {
            let built_in = d
                .transport_type()
                .map(|t| t == ca::DeviceTransportType::BUILT_IN)
                .unwrap_or(false);
            let has_output = d.asbd(ca::PropScope::OUTPUT).is_ok();
            if built_in && has_output {
                if let Ok(uid) = d.uid() {
                    return Ok((uid, "built-in output".into()));
                }
            }
        }
    }
    let dev = ca::System::default_output_device().map_err(|e| format!("default output: {e:?}"))?;
    let uid = dev.uid().map_err(|e| format!("output uid: {e:?}"))?;
    Ok((uid, "default output (no built-in found)".into()))
}

fn discord_process_ids(bundle_prefix: &str, verbose: bool) -> Vec<u32> {
    let Ok(procs) = ca::System::processes() else { return Vec::new() };
    let mut ids: Vec<u32> = procs
        .iter()
        .filter(|p| {
            let matched = p
                .bundle_id()
                .ok()
                .map_or(false, |b| b.to_string().starts_with(bundle_prefix));
            if matched && verbose {
                eprintln!(
                    "[capture] tapping {:?} pid={:?} running_output={:?}",
                    p.bundle_id().ok().map(|b| b.to_string()),
                    p.pid().ok(),
                    p.is_running_output().ok()
                );
            }
            matched
        })
        .map(|p| {
            let obj: &ca::Obj = p;
            obj.0
        })
        .collect();
    ids.sort_unstable();
    ids
}

fn ns_number_array(ids: &[u32]) -> arc::R<ns::Array<ns::Number>> {
    let nums: Vec<arc::R<ns::Number>> = ids.iter().map(|i| ns::Number::with_u32(*i)).collect();
    let refs: Vec<&ns::Number> = nums.iter().map(|n| n.as_ref()).collect();
    ns::Array::from_slice(&refs)
}

/// Runs one capture session: builds tap + aggregate, pumps audio until Discord's
/// audio-process set changes or an error occurs. Returns Ok(true) to rebuild
/// immediately (membership change), Ok(false)/Err to back off.
fn run_session(
    bundle_prefix: &str,
    ids: &[u32],
    now_ms: &(impl Fn() -> u64 + Send + Sync + Clone + 'static),
    status_tx: &tokio::sync::mpsc::UnboundedSender<CaptionsStatus>,
    sink: &mut (impl FnMut(PcmChunk) + Send),
) -> Result<bool, String> {
    // Mono mixdown: voice content, half the resampler work.
    let mut tap_desc = ca::TapDesc::with_mono_mixdown_of_processes(&ns_number_array(ids));
    tap_desc.set_private(true);
    let tap = tap_desc
        .create_process_tap()
        .map_err(|e| format!("create_process_tap failed ({e:?}) — if this is a permission problem, grant System Audio Recording in System Settings → Privacy & Security"))?;
    let asbd = tap.asbd().map_err(|e| format!("tap format: {e:?}"))?;
    let native_rate = asbd.sample_rate as u32;
    let channels = (asbd.channels_per_frame as usize).max(1);
    eprintln!(
        "[capture] tap format: rate={} ch={} format_id={:?} flags={:?} bits={}",
        native_rate, channels, asbd.format, asbd.format_flags, asbd.bits_per_channel
    );

    // Clock the aggregate off a STABLE device. Bluetooth default outputs make
    // aggregate IO erratic → the tap delivers speech shredded with silence
    // holes → whisper confabulates fluent nonsense (the evidence: debug-audio
    // WAVs with 30–75% silent blocks while the default output was a BT headset).
    let (output_uid, clock_label) = stable_clock_uid()?;
    eprintln!("[capture] aggregate clock: {clock_label}");
    let sub_device =
        cf::DictionaryOf::with_keys_values(&[sub_keys::uid()], &[output_uid.as_type_ref()]);
    let tap_uid = tap.uid().map_err(|e| format!("tap uid: {e:?}"))?;
    let sub_tap = cf::DictionaryOf::with_keys_values(&[sub_keys::uid()], &[tap_uid.as_type_ref()]);

    let desc = cf::DictionaryOf::with_keys_values(
        &[
            agg_keys::is_private(),
            agg_keys::is_stacked(),
            agg_keys::tap_auto_start(),
            agg_keys::name(),
            agg_keys::main_sub_device(),
            agg_keys::uid(),
            agg_keys::sub_device_list(),
            agg_keys::tap_list(),
        ],
        &[
            cf::Boolean::value_true().as_type_ref(),
            cf::Boolean::value_false(),
            cf::Boolean::value_true(),
            cf::str!(c"Callout Discord Tap"),
            &output_uid,
            &cf::Uuid::new().to_cf_string(),
            &cf::ArrayOf::from_slice(&[sub_device.as_ref()]),
            &cf::ArrayOf::from_slice(&[sub_tap.as_ref()]),
        ],
    );
    let agg = ca::AggregateDevice::with_desc(&desc).map_err(|e| format!("aggregate: {e:?}"))?;

    let (raw_tx, raw_rx) = crossbeam_channel::bounded::<Vec<f32>>(64);
    let mut ctx = Box::new(IoCtx { tx: raw_tx, channels, scratch: Vec::with_capacity(4096) });
    let proc_id = agg
        .create_io_proc_id(io_proc, Some(&mut *ctx))
        .map_err(|e| format!("io proc: {e:?}"))?;
    let started = ca::device_start(agg, Some(proc_id)).map_err(|e| format!("start: {e:?}"))?;

    let _ = status_tx.send(CaptionsStatus::Capturing { native_rate });
    let mut conditioner = Conditioner::new(native_rate, now_ms.clone())?;

    // Pump until the Discord audio-process set changes (restart, new helper).
    let mut last_check = std::time::Instant::now();
    let mut stats_at = std::time::Instant::now();
    let (mut blocks, mut samples, mut peak) = (0u64, 0u64, 0f32);
    let mut had_audio = false;
    loop {
        if stats_at.elapsed() >= Duration::from_secs(5) {
            // Log only on silence↔audio transitions to keep the log readable.
            let has_audio = peak > 0.001;
            if has_audio != had_audio {
                eprintln!("[capture] audio {}: blocks={blocks} samples={samples} peak={peak:.4}",
                    if has_audio { "FLOWING" } else { "went silent" });
                had_audio = has_audio;
            }
            stats_at = std::time::Instant::now();
            (blocks, samples, peak) = (0, 0, 0.0);
        }
        match raw_rx.recv_timeout(Duration::from_millis(500)) {
            Ok(block) => {
                blocks += 1;
                samples += block.len() as u64;
                for s in &block {
                    peak = peak.max(s.abs());
                }
                for chunk in conditioner.feed(&block) {
                    sink(chunk);
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                drop(started);
                return Err("io proc channel closed".into());
            }
        }
        if last_check.elapsed() >= Duration::from_secs(2) {
            last_check = std::time::Instant::now();
            let current = discord_process_ids(bundle_prefix, false);
            if current != ids {
                drop(started);
                return Ok(!current.is_empty());
            }
        }
    }
}

pub fn spawn_supervisor(
    bundle_prefix: &'static str,
    now_ms: impl Fn() -> u64 + Send + Sync + Clone + 'static,
    status_tx: tokio::sync::mpsc::UnboundedSender<CaptionsStatus>,
    mut sink: impl FnMut(PcmChunk) + Send + 'static,
) {
    std::thread::Builder::new()
        .name("callout-capture".into())
        .spawn(move || loop {
            let ids = discord_process_ids(bundle_prefix, true);
            if ids.is_empty() {
                let _ = status_tx.send(CaptionsStatus::WaitingForDiscordAudio);
                std::thread::sleep(Duration::from_secs(2));
                continue;
            }
            match run_session(bundle_prefix, &ids, &now_ms, &status_tx, &mut sink) {
                Ok(_membership_changed) => { /* rebuild immediately */ }
                Err(message) => {
                    eprintln!("[capture] {message}");
                    let _ = status_tx.send(CaptionsStatus::CaptureError { message });
                    std::thread::sleep(Duration::from_secs(5));
                }
            }
        })
        .expect("spawn capture thread");
}
