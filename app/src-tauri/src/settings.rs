//! Tiny persisted settings (app_data_dir/settings.json).

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Whisper language codes the user allows. Empty = full auto-detect.
    /// One entry = hard pin. Several = auto-detect restricted to this set.
    #[serde(default)]
    pub languages: Vec<String>,
    /// Caption-box background opacity, 0.2–1.0.
    #[serde(default = "default_opacity")]
    pub overlay_opacity: f64,
    /// Saved overlay position (logical px); None = bottom-center default.
    #[serde(default)]
    pub overlay_pos: Option<(f64, f64)>,
}

fn default_opacity() -> f64 {
    0.92
}

impl Default for Settings {
    fn default() -> Self {
        Self { languages: Vec::new(), overlay_opacity: default_opacity(), overlay_pos: None }
    }
}

#[derive(Clone)]
pub struct SettingsHandle {
    path: PathBuf,
    pub inner: Arc<RwLock<Settings>>,
}

impl SettingsHandle {
    pub fn load(app_data_dir: PathBuf) -> Self {
        let path = app_data_dir.join("settings.json");
        let settings = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self { path, inner: Arc::new(RwLock::new(settings)) }
    }

    pub fn languages(&self) -> Vec<String> {
        self.inner.read().map(|s| s.languages.clone()).unwrap_or_default()
    }

    pub fn set_languages(&self, languages: Vec<String>) {
        self.mutate(|s| s.languages = languages);
    }

    pub fn overlay_opacity(&self) -> f64 {
        self.inner.read().map(|s| s.overlay_opacity).unwrap_or(0.92)
    }

    pub fn set_overlay_opacity(&self, opacity: f64) {
        self.mutate(|s| s.overlay_opacity = opacity.clamp(0.2, 1.0));
    }

    pub fn overlay_pos(&self) -> Option<(f64, f64)> {
        self.inner.read().ok().and_then(|s| s.overlay_pos)
    }

    pub fn set_overlay_pos(&self, x: f64, y: f64) {
        self.mutate(|s| s.overlay_pos = Some((x, y)));
    }

    fn mutate(&self, f: impl FnOnce(&mut Settings)) {
        if let Ok(mut s) = self.inner.write() {
            f(&mut s);
            let _ = std::fs::create_dir_all(self.path.parent().unwrap_or(&self.path));
            if let Ok(json) = serde_json::to_string_pretty(&*s) {
                let _ = std::fs::write(&self.path, json);
            }
        }
    }
}
