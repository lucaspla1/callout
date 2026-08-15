//! Tiny persisted settings (app_data_dir/settings.json).

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    /// Whisper language codes the user allows. Empty = full auto-detect.
    /// One entry = hard pin. Several = auto-detect restricted to this set.
    #[serde(default)]
    pub languages: Vec<String>,
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
        if let Ok(mut s) = self.inner.write() {
            s.languages = languages;
            let _ = std::fs::create_dir_all(self.path.parent().unwrap_or(&self.path));
            if let Ok(json) = serde_json::to_string_pretty(&*s) {
                let _ = std::fs::write(&self.path, json);
            }
        }
    }
}
