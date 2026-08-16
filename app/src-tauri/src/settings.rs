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
    /// Caption font size in px, 12–26.
    #[serde(default = "default_font_px")]
    pub caption_font_px: f64,
    /// How speaker identity renders on caption lines: "name" | "avatar" | "both".
    #[serde(default = "default_identity")]
    pub caption_identity: String,
    /// Overlay layout: "captions" (bottom feed) | "roster" (vertical member
    /// list, Discord-overlay style, bubbles under whoever is talking).
    #[serde(default = "default_layout")]
    pub overlay_layout: String,
    /// Saved overlay position (logical px); None = bottom-center default.
    #[serde(default)]
    pub overlay_pos: Option<(f64, f64)>,
}

fn default_opacity() -> f64 {
    0.92
}

fn default_font_px() -> f64 {
    16.0
}

fn default_identity() -> String {
    "both".to_string() // avatar + "Name:" — the chosen principal style
}

fn default_layout() -> String {
    "captions".to_string()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            languages: Vec::new(),
            overlay_opacity: default_opacity(),
            caption_font_px: default_font_px(),
            caption_identity: default_identity(),
            overlay_layout: default_layout(),
            overlay_pos: None,
        }
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

    pub fn caption_font_px(&self) -> f64 {
        self.inner.read().map(|s| s.caption_font_px).unwrap_or(16.0)
    }

    pub fn set_caption_font_px(&self, px: f64) {
        self.mutate(|s| s.caption_font_px = px.clamp(12.0, 26.0));
    }

    pub fn caption_identity(&self) -> String {
        self.inner.read().map(|s| s.caption_identity.clone()).unwrap_or_else(|_| "name".into())
    }

    pub fn set_caption_identity(&self, mode: String) {
        let mode = match mode.as_str() {
            "avatar" | "both" => mode,
            _ => "name".to_string(),
        };
        self.mutate(|s| s.caption_identity = mode);
    }

    pub fn overlay_layout(&self) -> String {
        self.inner.read().map(|s| s.overlay_layout.clone()).unwrap_or_else(|_| "captions".into())
    }

    pub fn set_overlay_layout(&self, layout: String) {
        let layout = if layout == "roster" { layout } else { "captions".to_string() };
        self.mutate(|s| s.overlay_layout = layout);
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
