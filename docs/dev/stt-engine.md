# STT Engine — Implementation Guide

**Module scope:** consume 16 kHz mono PCM from the capture module; emit caption events — a rolling *partial* line plus stabilized *final* lines with capture-clock timestamps. Must run well on CPU/iGPU while a game owns the dGPU. English first, pt-BR close second; multilingual-capable from day one. Research current as of **2026-08-15**.

**TL;DR recommendation:** v0.1 ships **VAD-gated chunked transcription with whisper.cpp `small-q5_1` (multilingual) via `whisper-rs`**, with Silero VAD for endpointing, behind an `SttEngine` trait. **NVIDIA Nemotron-3.5-ASR-streaming-0.6b** (true streaming, EN + pt-BR, punctuation built in) is the v0.2 engine: it became genuinely reachable from Rust in mid-2026 via ggml runtimes (`transcribe.cpp`, `parakeet.cpp`) but those bindings are weeks old — spike them, don't bet v0.1 on them.

---

## 1. Engine landscape from Rust (as of 2026)

| Option | Streaming? | pt? | Rust path | Maturity | Verdict |
|---|---|---|---|---|---|
| whisper.cpp small (multilingual) | pseudo (VAD-chunked) | yes (good) | `whisper-rs` 0.16 | very high | **v0.1 default** |
| Nemotron-3.5-ASR-streaming-0.6b | true (80 ms–1.12 s) | yes (pt-BR, WER ≈5.5%) | `transcribe-cpp` 0.1.x / FFI to `parakeet.cpp` | runtimes young (Jun–Jul 2026) | **v0.2 primary** |
| Parakeet-TDT 0.6b v3 (offline) | chunked only | yes (25 EU langs) | `transcribe-rs` (ort) / sherpa-onnx | high | accuracy-tier option |
| sherpa-onnx streaming zipformer | true | **no pt model exists** | `sherpa-rs` 0.6.8 | medium | EN-only streaming, skip |
| Moonshine streaming (v2) | true | EN-only | `transcribe-rs` StreamingModel | medium | low-end EN option |
| Vosk small-pt | true | yes (dated) | `vosk-rs` | stale | last-resort fallback |

### 1.1 whisper-rs (whisper.cpp bindings) — the boring, proven choice

- Crate: <https://crates.io/crates/whisper-rs> — **v0.16.0 (2026-03-12)**, actively maintained (tazz4843), bindings to ggml-org/whisper.cpp. Repo: <https://github.com/tazz4843/whisper-rs>.
- Backends via cargo features: **Metal** (our dev Mac — small runs at RTF ≈0.1 on a base M1), **Vulkan** (Windows iGPU — pin the adapter, see §5), CUDA, OpenBLAS, plain CPU. Whisper quantized models (q5_1/q8_0) are first-class.
- No true streaming: whisper is a 30 s-window batch model. **Pseudo-streaming is feasible and well-trodden** (whisper.cpp's own `stream` example, Handy, WhisperLive): VAD segments speech, you re-decode the growing utterance every ~0.5–0.7 s for partials, and decode once more at the VAD endpoint for the final. Two implementation details make or break latency:
  1. **`audio_ctx` trimming.** whisper pads every input to 30 s; a 2 s chunk costs almost as much as 30 s unless you shrink the encoder context: `audio_ctx ≈ (len_s / 30.0 * 1500) as i32 + 128` (clamp ≥ 512). `whisper-rs` exposes `FullParams::set_audio_ctx`. This is the difference between ~3 s and ~0.4 s per partial on CPU.
  2. **`no_context = true`, `single_segment = true`, greedy sampling, temperature 0.0** — prevents cross-utterance hallucination loops and repeated-token spirals that plague naive chunked whisper.
- whisper.cpp also ships a **built-in Silero-v5.1.2 VAD** (issue [#3003](https://github.com/ggml-org/whisper.cpp/issues/3003)), exposed in whisper-rs as `WhisperVadContext` — usable standalone, no extra runtime.
- Model quality/size (multilingual unless `.en`): see §4 table. `small` is the sweet spot for pt — `base` is noticeably worse on Portuguese; `small-q5_1` (181 MiB) loses ~nothing vs f16 `small` (466 MiB).

### 1.2 Handy's `transcribe-rs` + `transcribe-cpp` (MIT — cjpais / handy-computer)

Handy (<https://github.com/cjpais/Handy>, MIT, Tauri 2.11) is the closest existing product to our pipeline (push-to-talk dictation, not captions). Its STT stack is now **two crates**, both MIT and on crates.io:

- **`transcribe-rs`** (<https://crates.io/crates/transcribe-rs>, **v0.3.11, 2026-04-07**, ~29k downloads/mo): batch, multi-engine — Whisper (via whisper-rs 0.16 / GGML), and ONNX engines via `ort =2.0.0-rc.12`: **Parakeet, Canary, Moonshine, SenseVoice, GigaAM**, plus remote OpenAI. API: `SpeechModel::transcribe_with(&mut self, samples: &[f32], &TranscribeOptions) -> TranscriptionResult` with per-segment timestamps; a `StreamingModel` trait exists for **Moonshine-streaming** variants only. Built for batch push-to-talk: no VAD/endpointing inside, one full-buffer call per utterance.
- **`transcribe-cpp`** (<https://lib.rs/crates/transcribe-cpp>, **v0.1.3, 2026-07-12**, MIT): safe Rust binding to **`handy-computer/transcribe.cpp`** (<https://github.com/handy-computer/transcribe.cpp>) — a new ggml STT runtime covering "16+ model families, 60+ variants" as GGUF, **batch AND streaming sessions**, Metal (default on macOS)/Vulkan/CUDA + tinyBLAS CPU, `dynamic-backends` packaging. Its docs explicitly recommend **`nemotron-3.5-asr-streaming-0.6b` (multilingual) and `nemotron-speech-streaming-en-0.6b`** for low-latency streaming. Handy itself ships it (per-platform Metal/Vulkan features) alongside a "Unified EN 0.6B" 731 MB GGUF.

**Engineering fit:** depend on the crates, don't vendor. `transcribe-rs` is a fine batch layer but adds little over `whisper-rs` for our chunked-whisper v0.1 (we need VAD + partial/final logic either way, and its `ort` pin is a heavy transitive dep). Its real value: a ready **Parakeet-TDT v3 (ONNX)** engine if we want the accuracy tier cheaply. `transcribe-cpp` is the exciting one — a streaming API over Nemotron from Rust, maintained by the Handy author — but at 0.1.x/one month old it needs a spike before we depend on it. Handy's **model-downloader and VAD wiring are the parts to adapt** (MIT, attribute in NOTICE): see §2, §4.

### 1.3 sherpa-onnx / sherpa-rs — true streaming, wrong languages

- sherpa-onnx (<https://github.com/k2-fsa/sherpa-onnx>) has real streaming (online) transducer support. But enumerate the actual **streaming zipformer models** ([pretrained list](https://k2-fsa.github.io/sherpa/onnx/pretrained_models/online-transducer/zipformer-transducer-models.html)): English (3 variants + 20M tiny), Chinese (several), bilingual zh-en, Korean, French (~60 MB, 2023), Bengali (2026). **No Portuguese — no streaming model covering pt exists in the k2-fsa zoo as of Aug 2026.** Streaming-first via sherpa would make pt-BR a second-class citizen, the opposite of our goal.
- Offline models incl. **Parakeet-TDT-0.6b-v3** (25 European languages incl. pt, <https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3>) run chunked — same pseudo-streaming pattern as whisper, better English WER, int8 ≈ 478 MB.
- Bindings: `sherpa-rs` (<https://github.com/thewh1teagle/sherpa-rs>, v0.6.8, ~Mar 2026) — works, statically links prebuilt sherpa-onnx; maintenance is one-person with forks (`chobits-sherpa-rs`) shipping ahead of it. Acceptable but not a foundation I'd pick without a language reason. Verdict: **skip for v0.1**; revisit only if k2-fsa publishes a multilingual streaming model.

### 1.4 NVIDIA Nemotron-3.5-ASR-streaming-0.6b — the v0.2 engine

Model card: <https://huggingface.co/nvidia/nemotron-3.5-asr-streaming-0.6b> (released 2026-06-04, **OpenMDW-1.1** license — permissive, redistribution OK). 600 M cache-aware FastConformer-RNNT with language-ID prompt; **40 language-locales incl. pt-BR/pt-PT**; runtime-configurable chunk = 80/160/320/560/1120 ms; punctuation + capitalization native. WER @1.12 s chunk, LangID mode: pt 5.48%, en 7.91% (multi-domain sets). This is exactly the caption-shaped model: partials every ~100–500 ms with no re-decoding, stable text, punctuation for free.

Integration paths from Rust/Tauri **today**, honestly assessed:

1. **`transcribe.cpp` via `transcribe-cpp` crate (most promising).** GGUF (q8_0 ≈ 731 MB for the EN 0.6b; q4_k ≈ 458 MB / f16 1.3 GB conversions exist, e.g. <https://huggingface.co/cstr/nemotron-3.5-asr-streaming-GGUF>), streaming sessions, Metal/Vulkan/CPU. Risk: crate is v0.1.3 (July 2026); multilingual-3.5 prompt conditioning through the Rust API is unverified. **Spike it.**
2. **`mudler/parakeet.cpp` via hand-rolled FFI** (<https://github.com/mudler/parakeet.cpp>, MIT, 762★, LocalAI's ASR backend): C++17/ggml, validated "WER 0 vs NeMo", supports nemotron-3.5-streaming with prompt conditioning, cache-aware streaming with per-word timestamps through a flat C API (`parakeet_capi.h`) designed for FFI; prebuilt Metal/Vulkan/CPU binaries; GGUFs at <https://huggingface.co/mudler/parakeet-cpp-gguf>. No Rust crate yet — we'd write a small `-sys` binding (~a day, the API is flat).
3. **ONNX route (DIY, documented but heavy):** community exports exist — <https://github.com/codavidgarcia/nemotron-3.5-asr-streaming-onnx> (Apache-2.0; cache-aware encoder/decoder/joiner with **96 cache tensors** — 56 left-context K/V frames × 24 layers + conv caches — RTF 0.26 CPU fp32) and int4 <https://huggingface.co/onnx-community/nemotron-3.5-asr-streaming-0.6b-onnx-int4>. Reimplementing mel frontend + RNNT greedy loop + cache plumbing in Rust/`ort` is 2–4 weeks of careful work. Only if (1) and (2) fail.
4. NVIDIA's own **NeMo-Speech.cpp** C++ runtime exists (`nemo-speech transcribe … .q8_0.gguf`) but is lab-grade with no Rust story yet.

**Maturity call: not v0.1.** Every path is < 3 months old. Put it behind the `SttEngine` trait as the designed-for v0.2 upgrade, and run the spike (§5) in parallel with v0.1 hardening.

### 1.5 Briefly: Moonshine streaming, Vosk

- **Moonshine v2 streaming** (UsefulSensors, tiny/small/medium: <https://huggingface.co/UsefulSensors/moonshine-streaming-tiny>, arXiv [2602.12241](https://arxiv.org/abs/2602.12241)): genuine low-latency streaming encoder, very light, in `transcribe-rs` as `StreamingModel` — but **English-only**. Candidate for a "potato mode" EN preset, nothing more.
- **Vosk** (`vosk-rs`, <https://alphacephei.com/vosk/models>): streaming Kaldi models; `vosk-model-small-pt-0.3` ≈ 31 MB. Real streaming and tiny, but 2020-era accuracy (no punctuation, weak on gamer slang/callouts). Keep as an accessibility-of-last-resort footnote for machines that can't run whisper base; do not build for it.

---

## 2. VAD in Rust

| Crate | Model | Deps | Notes |
|---|---|---|---|
| `voice_activity_detector` (<https://crates.io/crates/voice_activity_detector>) | Silero v5 | `ort` | Clean per-frame API (512 samples @16 kHz → speech probability), model embedded in the crate, maintained. **Pick.** |
| `vad-rs` (<https://github.com/thewh1teagle/vad-rs>) | Silero | `ort` | What Handy uses (cjpais git fork). Same model, less polished API, fork-of-a-fork churn. |
| `earshot` (<https://github.com/pykeio/earshot>) | own 40 KiB NN | none (pure Rust) | RTF ~0.0007, ~95 KiB total; claims parity with Silero v6 on its own benchmarks, but Silero still shows better recall/segment-IoU under noise in independent tests. Great zero-dep fallback behind a feature flag. |
| whisper.cpp built-in (`WhisperVadContext`) | Silero v5.1.2 ggml | whisper-rs (already present) | Segment-oriented API — fine for batch trimming, clumsy for frame-by-frame live endpointing. |

**Decision: `voice_activity_detector` (Silero v5).** Best accuracy-per-effort for noisy gaming audio (music, keyboard, hype screaming); the `ort` dependency is amortized the moment we add any ONNX engine (Parakeet v3). `earshot` stays as a `--no-default-features` lightweight alternative.

**Parameters for gaming speech** (callouts are short, clipped, and dense; we want aggressive endpointing but must not chop word tails):

```rust
pub struct VadConfig {
    pub frame: usize,           // 512 samples = 32 ms @ 16 kHz (Silero v5 native)
    pub enter_threshold: f32,   // 0.55  — prob to enter SPEECH (hysteresis high)
    pub exit_threshold: f32,    // 0.35  — prob to stay in SPEECH (hysteresis low)
    pub min_speech_ms: u32,     // 128   — ≥4 frames before we accept an utterance (kills clicks)
    pub endpoint_silence_ms: u32,// 400  — silence to finalize; 300 = twitchy, 700 = laggy captions
    pub pre_roll_ms: u32,       // 240   — ring buffer prepended so onsets aren't clipped
    pub max_utterance_s: f32,   // 12.0  — force-finalize and continue (whisper window safety)
}
```

Hysteresis (enter 0.55 / exit 0.35) plus the 400 ms hangover is what prevents mid-word chops on plosives and breath pauses; pre-roll rescues the "B-site!" first syllable that VAD always misses. All four latency-relevant numbers become user settings later (Deaf/HH users have told other caption projects they prefer twitchier partials).

---

## 3. Recommended v0.1 pipeline

**Why chunked whisper `small-q5_1` multilingual:** (a) only proven engine that is simultaneously good at EN *and* pt-BR at 181 MiB; (b) `whisper-rs` is mature on all three backends we need (Metal dev Mac, Vulkan/CPU Windows); (c) the VAD/partial/final scaffolding we build is engine-agnostic and survives the Nemotron swap; (d) whisper punctuates and capitalizes, which matters for caption readability. Cost: partials arrive every ~0.6 s instead of every ~0.1 s, and each partial re-decodes the utterance so far — acceptable at callout lengths (2–6 s), and the trait hides it.

```
capture (16 kHz mono f32, absolute sample index)
  └─► VAD gate (32 ms frames, hysteresis, pre-roll)          [audio thread]
        └─► utterance buffer + job queue (crossbeam channel)
              └─► STT worker thread (whisper ctx, 1 instance)
                    ├─ every ≥600 ms new audio → decode(so-far) → Partial
                    └─ on endpoint/force-cut  → decode(full)   → Final
                          └─► caption controller → Tauri event → overlay
```

### 3.1 The `SttEngine` trait

```rust
/// All times are seconds on the capture clock: absolute_sample_index / 16_000.0.
#[derive(Clone, Debug)]
pub enum SttEvent {
    /// Provisional text for the utterance in progress. Replaces the previous Partial.
    Partial { text: String, t_start: f64, t_audio_end: f64 },
    /// Utterance finalized (VAD endpoint or force-cut). Immutable afterwards.
    Final   { text: String, t_start: f64, t_end: f64, lang: Option<String> },
    Error   { msg: String },
}

pub trait SttEngine: Send {
    /// Feed PCM with the absolute capture-clock index of pcm[0].
    /// Non-blocking: copies into internal buffers, never decodes inline.
    fn feed(&mut self, pcm: &[f32], first_sample_index: u64);
    /// Force-finalize the current utterance (mute, stream end, engine swap).
    fn flush(&mut self);
    fn reset(&mut self);
}

/// Construction: engine spawns its worker and hands back the event stream.
pub fn spawn(cfg: EngineConfig) -> (Box<dyn SttEngine>, crossbeam_channel::Receiver<SttEvent>);
```

**Timestamps are capture-clock based — keep them that way.** The capture module owns a monotonically increasing sample counter starting at stream open; `t = samples / 16_000.0`. VAD marks `t_start` as (first speech frame − pre-roll) in absolute samples; whisper's per-segment timestamps are *relative to the fed buffer*, so add the buffer's base index before emitting. Convert to wall clock only at the alignment layer, via one anchor captured once: `(Instant_at_stream_open, unix_time_at_stream_open)`. Never stamp events with `SystemTime::now()` at emission — decode latency would smear them and downstream alignment with Discord speaking events (which we also timestamp on arrival against the same anchor) would drift.

### 3.2 VAD gate + worker loop sketch

The audio thread runs the VAD state machine and only ever copies samples; the worker owns the whisper context. Backpressure rule: **partial jobs are droppable, final jobs are not** — if the worker is still decoding when the next partial tick arrives, skip it; if an endpoint fires mid-decode, queue exactly one final job.

```rust
enum VadState { Silence, Speech { started_at: u64, silence_frames: u32 } }

struct Gate {
    state: VadState,
    pre_roll: VecDeque<f32>,          // pre_roll_ms worth of samples, always fed
    utterance: Vec<f32>,              // grows while in Speech
    utt_start_sample: u64,            // absolute capture-clock index incl. pre-roll
    since_last_decode: usize,         // samples accumulated since last partial job
}

impl Gate {
    fn on_frame(&mut self, frame: &[f32], idx: u64, p: f32, cfg: &VadConfig, tx: &Sender<Job>) {
        match &mut self.state {
            VadState::Silence if p > cfg.enter_threshold => {
                self.utt_start_sample = idx.saturating_sub(self.pre_roll.len() as u64);
                self.utterance.clear();
                self.utterance.extend(self.pre_roll.iter());
                self.utterance.extend_from_slice(frame);
                self.state = VadState::Speech { started_at: idx, silence_frames: 0 };
            }
            VadState::Speech { silence_frames, .. } => {
                self.utterance.extend_from_slice(frame);
                *silence_frames = if p < cfg.exit_threshold { *silence_frames + 1 } else { 0 };
                let endpoint = *silence_frames as u32 * 32 >= cfg.endpoint_silence_ms
                    || self.utterance.len() as f32 / 16_000.0 >= cfg.max_utterance_s;
                if endpoint {
                    tx.send(Job::Final { pcm: take(&mut self.utterance),
                                         base: self.utt_start_sample }).ok();
                    self.state = VadState::Silence;
                } else if self.since_last_decode >= 600 * 16 {       // ≥600 ms new audio
                    tx.try_send(Job::Partial { pcm: self.utterance.clone(),
                                               base: self.utt_start_sample }).ok(); // droppable
                    self.since_last_decode = 0;
                }
            }
            VadState::Silence => { /* keep filling pre_roll ring */ }
        }
    }
}
```

Worker side: `Final` decodes with full beam-free params below; `Partial` may additionally cap `audio_ctx` harder and skip timestamp emission for speed. One whisper context, created once (model load ≈ 0.5–2 s — do it at app start, emit a `ready` event). `flush()` injects a synthetic endpoint; `reset()` clears the gate and the last-partial cache.

### 3.3 Whisper decode parameters

```rust
let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
params.set_language(Some(cfg.lang.as_deref().unwrap_or("auto"))); // "en"/"pt" pin = faster + fewer flips
params.set_no_context(true);          // no cross-utterance carryover (hallucination guard)
params.set_single_segment(true);      // one segment per VAD utterance; t_start/t_end come from the capture clock
params.set_suppress_blank(true);
params.set_suppress_nst(true);        // non-speech tokens
params.set_temperature(0.0);
params.set_no_timestamps(false);
let audio_ctx = ((buf_len_s / 30.0) * 1500.0) as i32 + 128;  // the latency trick, §1.1
params.set_audio_ctx(audio_ctx.clamp(512, 1500));
params.set_n_threads(n_physical_cores.saturating_sub(2).clamp(2, 6) as i32);
```

Hallucination guards for the caption path (all standard in chunked-whisper deployments): drop a decode result if avg token logprob < −1.0, if `no_speech_prob` > 0.6 while VAD said speech < 300 ms, or if it exactly repeats the previous final (whisper's classic "Thank you." on noise). Maintain a small ban-list of known noise hallucinations per language ("Legendas pela comunidade…" shows up on pt silence).

### 3.4 Partial/final caption UX pattern

- **Partial cadence:** decode when ≥600 ms of new audio has accumulated since the last decode (and previous decode finished — never queue two). Typical partial rhythm: 0.6–1.0 s.
- **UI throttle:** overlay renders partials at most every 100 ms (10 Hz); no animation on replace.
- **Stability trick:** captions must not visibly "un-say" words. Since each partial re-decodes the whole utterance, early words can mutate. Split the line into `stable + provisional` at word granularity:

```rust
/// Words agreeing across the last two partials are promoted to stable.
fn stabilize(prev: &str, curr: &str) -> (String, String) {
    let (p, c): (Vec<_>, Vec<_>) = (prev.split_whitespace().collect(),
                                    curr.split_whitespace().collect());
    let n = p.iter().zip(&c).take_while(|(a, b)| a == b).count();
    (c[..n].join(" "), c[n..].join(" "))   // (stable, provisional)
}
```

  Render stable at full opacity, provisional dimmed/italic. On `Final`, replace the whole line, promote it to history, cap history at 2–3 lines with per-line fade-out after ~6 s.
- **Language handling:** default `auto` detection but expose a pin (EN / pt-BR) in settings. Whisper's per-chunk auto-detect can flip languages between partials of one utterance; mitigate by detecting once per utterance (first partial) and pinning for its remainder — or trust the user's pin, which also cuts ~10–20 ms per decode.
- **Endpoint feel:** with `endpoint_silence_ms = 400` and final decode ≈ 0.3–0.9 s, the finalized line lands ~0.7–1.3 s after the speaker stops — inside target. Partials mean the user has been reading the gist for the whole utterance already; perceived latency is far lower than the final-line number.

### 3.5 Crate layout

Isolate the engine behind cargo features so the Nemotron swap is additive:

```
app/src-tauri/
  crates/stt/           # this module: no Tauri deps, pure lib + tests on wav fixtures
    src/{lib.rs, vad.rs, whisper.rs, events.rs, stabilize.rs}
    Cargo.toml          # [features] default=["whisper"]; whisper=["dep:whisper-rs"]
                        #            nemotron=["dep:transcribe-cpp"]  (v0.2)
```

Whisper backend features flow through: `whisper-rs/metal` on macOS, `whisper-rs/vulkan` + CPU fallback on Windows (runtime adapter check, §5.2). Keeping `stt` Tauri-free lets the spike harness (§5.3) drive it from a plain CLI.

---

## 4. Model management

**v0.1 model set** (whisper ggml, from `https://huggingface.co/ggerganov/whisper.cpp` — URL pattern `…/resolve/main/<file>`; sizes from the repo):

| File | Size | Role |
|---|---|---|
| `ggml-small-q5_1.bin` | 181 MiB | **Default.** Multilingual (EN + pt-BR), best quality/latency balance |
| `ggml-base-q5_1.bin` | 57 MiB | "Fast/low-end" preset; acceptable EN, weak pt |
| `ggml-large-v3-turbo-q5_0.bin` | 547 MiB | Opt-in "accuracy" preset (beats `medium-q5_0` at similar size); only sensible on Metal/strong CPU |

Silero VAD ships embedded in `voice_activity_detector` — nothing to download. Nemotron GGUF (~458 MB q4_k / 731 MB q8_0) joins this table in v0.2. Do **not** bundle models in the installer (keeps it < 10 MB); download on first run with a bundled-model escape hatch documented for offline installs (Handy does exactly this).

**Layout** (`app_data_dir` via `tauri::Manager::path()`):

```
{app_data_dir}/
  models/
    whisper/ggml-small-q5_1.bin
    manifest.json        # per-file: url, sha256, bytes, engine, revision
  settings.json
```

**Download-with-progress** (adapt Handy's `models.rs` pattern — MIT, attribute in NOTICE): reqwest streamed GET → write to `{name}.partial` → verify **SHA-256** against a hash pinned in our manifest (HF ETags are weak refs; pin real hashes at release time) → atomic rename. Resume with `Range: bytes={n}-` when `.partial` exists. Emit Tauri events `model-dl-progress { id, got, total }` throttled to 10 Hz; UI shows per-model progress + cancel. Retry ×3 with backoff; on hash mismatch delete and re-download once, then surface an error with the manual-install path (user drops the file in `models/whisper/`, we re-hash on scan).

---

## 5. Performance plan

### 5.1 Latency budget — chunked whisper, speech-end → final caption visible

| Stage | Budget (typical / p95) | Notes |
|---|---|---|
| Capture + resample buffering | 20 / 60 ms | capture module's frame size |
| VAD endpoint wait | 400 / 400 ms | by design (`endpoint_silence_ms`) — dominant fixed cost |
| Final decode, 3–5 s utterance, `small-q5_1` | Metal M-series: 150–400 ms · mid CPU 4–6 threads + `audio_ctx` trim: 300–900 ms | without `audio_ctx` trim: 2–8 s on CPU — non-negotiable optimization |
| Event → IPC → overlay paint | 10 / 30 ms | Tauri emit + WebView frame |
| **Total (speech end → final)** | **~0.6–0.9 s Metal · ~0.75–1.4 s mid CPU** | target < 2 s: comfortable · stretch < 1.2 s: met on Metal, borderline CPU |
| First partial after speech onset | ~0.9–1.4 s | 600 ms accumulation + one decode |

Reference points: whisper.cpp `small` decodes 30 s in ~3 s on a base M1 (RTF ≈ 0.1); Apple-Silicon and CPU thread-scaling benchmarks: <https://justvoice.ai/blog/whisper-benchmark-apple-silicon-m3-m4>, <https://openbenchmarking.org/test/pts/whisper-cpp>.

### 5.2 Expected CPU load (mid-range gaming CPU, e.g. Ryzen 5 5600 class)

Decode bursts of 0.3–0.9 s at 60–80% of 4 worker threads, idle between; with typical voice-chat duty cycle (speech ~30% of the time) expect **~8–15% average of a 6-core/12-thread CPU**, spiking to ~35% momentarily. VAD is negligible (< 0.5% of one core). Run the worker at *below-normal* process priority is wrong — keep normal thread priority but cap `n_threads` at physical-cores − 2 so the game keeps its headroom. On Windows+Vulkan, whisper.cpp must be pinned to the **iGPU** (enumerate adapters; select non-dGPU via `GGML_VK_VISIBLE_DEVICES`) or fall back to CPU — never compete with the game on the dGPU.

### 5.3 First spike — what to measure, and pass/fail

Harness: feed prerecorded 16 kHz fixtures (LibriSpeech test-clean subset; Common Voice pt-BR subset; 10 min of real Discord gaming audio with crosstalk/music) through the full VAD→engine path, log every event with capture-clock + wall-clock stamps.

1. **Dev Mac (Metal):** `small-q5_1` RTF at 2/5/10 s chunk lengths; endpoint→final p50/p95; partial cadence achieved. **Pass: p95 endpoint→final < 1.2 s.**
2. **Windows mid CPU (and Vulkan iGPU if present):** same suite, `n_threads` ∈ {4, 6}; with/without `audio_ctx` trim (prove the trick). **Pass: p95 < 2.0 s, avg CPU < 15% of machine during continuous speech, no dGPU utilization.**
3. **Quality gate:** WER en < 12%, pt-BR < 18% on the gaming fixture (informal — hallucination rate matters more: < 1 bogus final per 10 min of non-speech).
4. **Nemotron spike (parallel, timeboxed 3 days):** `transcribe-cpp` 0.1.x streaming session with `nemotron-3.5-asr-streaming-0.6b` q4_k on the Mac — does multilingual + prompt conditioning work through the Rust API? Partial latency at 560 ms chunks? If yes on both: schedule as v0.2 engine; if not, retry via `parakeet.cpp` C API before falling back to waiting.

### Key sources

whisper-rs <https://github.com/tazz4843/whisper-rs> · whisper.cpp models <https://huggingface.co/ggerganov/whisper.cpp> · Handy <https://github.com/cjpais/Handy> · transcribe-rs <https://github.com/cjpais/transcribe-rs> · transcribe.cpp <https://github.com/handy-computer/transcribe.cpp> · sherpa-onnx streaming models <https://k2-fsa.github.io/sherpa/onnx/pretrained_models/online-transducer/zipformer-transducer-models.html> · sherpa-rs <https://github.com/thewh1teagle/sherpa-rs> · Nemotron 3.5 <https://huggingface.co/nvidia/nemotron-3.5-asr-streaming-0.6b> · parakeet.cpp <https://github.com/mudler/parakeet.cpp> · Nemotron ONNX export <https://github.com/codavidgarcia/nemotron-3.5-asr-streaming-onnx> · Parakeet-TDT v3 <https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3> · Moonshine streaming <https://huggingface.co/UsefulSensors/moonshine-streaming-tiny> · VAD: <https://crates.io/crates/voice_activity_detector>, <https://github.com/pykeio/earshot>, <https://github.com/thewh1teagle/vad-rs> · Vosk models <https://alphacephei.com/vosk/models>
