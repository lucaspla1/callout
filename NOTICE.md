# Third-party notices

Unmute is MIT-licensed ([LICENSE](LICENSE)) and stands on the shoulders of these projects. Models are downloaded at first run directly from their publishers and remain under their own licenses.

## Models

| Component | License | Attribution |
|---|---|---|
| [Whisper](https://github.com/openai/whisper) speech-recognition models | MIT | © OpenAI. GGML-format conversions from [ggerganov/whisper.cpp on Hugging Face](https://huggingface.co/ggerganov/whisper.cpp). |
| [Silero VAD v5](https://github.com/snakers4/silero-vad) voice-activity model | MIT | © Silero Team. Embedded via the `voice_activity_detector` crate. |
| [WeSpeaker](https://github.com/wenet-e2e/wespeaker) ResNet34 speaker-embedding model (`wespeaker_en_voxceleb_resnet34_LM`) | [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) | © The WeSpeaker project. Trained on VoxCeleb2. ONNX export distributed by [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) (Apache-2.0). Used unmodified for local speaker identification. |

## Libraries

| Component | License | Attribution |
|---|---|---|
| [whisper.cpp](https://github.com/ggml-org/whisper.cpp) | MIT | © Georgi Gerganov and the ggml authors |
| [whisper-rs](https://github.com/tazz4843/whisper-rs) | Unlicense | Rust bindings to whisper.cpp |
| [voice_activity_detector](https://github.com/nkeenan38/voice_activity_detector) | MIT | © Nicholas Keenan |
| [ONNX Runtime](https://github.com/microsoft/onnxruntime) | MIT | © Microsoft Corporation |
| [ort](https://github.com/pykeio/ort) | MIT / Apache-2.0 | Rust bindings to ONNX Runtime |
| [Tauri](https://github.com/tauri-apps/tauri) | MIT / Apache-2.0 | © Tauri Programme within The Commons Conservancy |
| [React](https://github.com/facebook/react) | MIT | © Meta Platforms, Inc. |
| [Vite](https://github.com/vitejs/vite) | MIT | © VoidZero Inc. and Vite contributors |
| [webpki-roots](https://github.com/rustls/webpki-roots) (Mozilla CA certificate bundle) | CDLA-Permissive-2.0 | Root-certificate data from Mozilla's CA program, used for TLS verification |

## Acknowledgments

- The model-downloader pattern (streaming, resume, progress) is adapted from [Handy](https://github.com/cjpais/Handy) (MIT).

The full dependency trees (all under permissive licenses) are declared in [`app/src-tauri/Cargo.toml`](app/src-tauri/Cargo.toml) and [`app/package.json`](app/package.json).
