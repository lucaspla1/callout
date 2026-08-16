//! First-run model provisioning: streams the required model files to the app
//! data dir with progress events, resume (HTTP Range) and retries, then hands
//! control to the captions pipeline. Pattern adapted from Handy's downloader
//! (MIT). Hash pinning is a pre-release TODO.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;

pub struct ModelSpec {
    pub id: &'static str,
    pub rel_path: &'static str,
    pub url: &'static str,
    /// Display estimate only; the server's Content-Length is authoritative.
    pub approx_bytes: u64,
}

pub const MODELS: &[ModelSpec] = &[
    ModelSpec {
        id: "whisper-small-q5_1",
        rel_path: "models/whisper/ggml-small-q5_1.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small-q5_1.bin",
        approx_bytes: 190_085_487,
    },
    ModelSpec {
        id: "whisper-large-v3-turbo-q5_0",
        rel_path: "models/whisper/ggml-large-v3-turbo-q5_0.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin",
        approx_bytes: 574_041_195,
    },
    ModelSpec {
        id: "speaker-wespeaker-resnet34",
        rel_path: "models/speaker/wespeaker_en_voxceleb_resnet34_LM.onnx",
        // Upstream release tag really is spelled "recongition".
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/wespeaker_en_voxceleb_resnet34_LM.onnx",
        approx_bytes: 26_000_000,
    },
];

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ModelDl {
    Progress { id: String, got: u64, total: u64 },
    Done { id: String },
    Failed { id: String, message: String },
    AllReady,
}

pub fn missing(data_dir: &Path) -> Vec<&'static ModelSpec> {
    MODELS.iter().filter(|m| !data_dir.join(m.rel_path).is_file()).collect()
}

/// Download every missing model, emitting progress. Returns Err after retries
/// are exhausted for any model.
pub async fn ensure_all(
    data_dir: PathBuf,
    emit: impl Fn(ModelDl) + Send + Sync + 'static,
) -> Result<(), String> {
    for spec in MODELS {
        let dest = data_dir.join(spec.rel_path);
        if dest.is_file() {
            continue;
        }
        download_with_retries(spec, &dest, &emit).await.map_err(|e| {
            emit(ModelDl::Failed { id: spec.id.to_string(), message: e.clone() });
            e
        })?;
        emit(ModelDl::Done { id: spec.id.to_string() });
    }
    emit(ModelDl::AllReady);
    Ok(())
}

async fn download_with_retries(
    spec: &ModelSpec,
    dest: &Path,
    emit: &(impl Fn(ModelDl) + Send + Sync),
) -> Result<(), String> {
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("create {dir:?}: {e}"))?;
    }
    let partial = dest.with_extension("partial");
    let mut last_err = String::new();
    for attempt in 0..3 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_secs(2 * attempt as u64)).await;
        }
        match stream_to_partial(spec, &partial, emit).await {
            Ok(()) => {
                std::fs::rename(&partial, dest).map_err(|e| format!("rename: {e}"))?;
                return Ok(());
            }
            Err(e) => {
                eprintln!("[models] {} attempt {} failed: {e}", spec.id, attempt + 1);
                last_err = e;
            }
        }
    }
    Err(format!("{} download failed after retries: {last_err}", spec.id))
}

async fn stream_to_partial(
    spec: &ModelSpec,
    partial: &Path,
    emit: &(impl Fn(ModelDl) + Send + Sync),
) -> Result<(), String> {
    let mut got = std::fs::metadata(partial).map(|m| m.len()).unwrap_or(0);
    let client = reqwest::Client::new();
    let mut req = client.get(spec.url);
    if got > 0 {
        req = req.header("Range", format!("bytes={got}-"));
    }
    let mut resp = req.send().await.map_err(|e| format!("request: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("http {}", resp.status()));
    }
    // 206 = server honored the resume; anything else restarts from zero.
    let resuming = resp.status() == reqwest::StatusCode::PARTIAL_CONTENT && got > 0;
    if !resuming {
        got = 0;
    }
    let total = got + resp.content_length().unwrap_or(spec.approx_bytes.saturating_sub(got));
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(resuming)
        .write(true)
        .truncate(!resuming)
        .open(partial)
        .map_err(|e| format!("open partial: {e}"))?;

    let mut last_emit = Instant::now();
    emit(ModelDl::Progress { id: spec.id.to_string(), got, total });
    while let Some(chunk) = resp.chunk().await.map_err(|e| format!("stream: {e}"))? {
        file.write_all(&chunk).map_err(|e| format!("write: {e}"))?;
        got += chunk.len() as u64;
        if last_emit.elapsed() >= Duration::from_millis(200) {
            emit(ModelDl::Progress { id: spec.id.to_string(), got, total });
            last_emit = Instant::now();
        }
    }
    emit(ModelDl::Progress { id: spec.id.to_string(), got, total: got.max(total) });
    file.flush().map_err(|e| format!("flush: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_lists_absent_models() {
        let dir = std::env::temp_dir().join(format!("callout-models-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(missing(&dir).len(), MODELS.len());
        // Create one; it disappears from the missing list.
        let p = dir.join(MODELS[0].rel_path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"stub").unwrap();
        assert_eq!(missing(&dir).len(), MODELS.len() - 1);
    }
}
