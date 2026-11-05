//! Persisted settings, stored as JSON in the user's config directory.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Reduce memory automatically.
    pub autoreduce: bool,
    /// Autoreduce triggers when usage is above this percent.
    pub threshold_hi: u32,
    /// Periodic cleaning, minutes; 0 = off.
    pub interval_min: u32,

    pub clean_working_sets: bool,
    pub clean_system_working_sets: bool,
    pub clean_standby: bool,

    pub show_tray: bool,
    pub close_to_tray: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            autoreduce: false,
            threshold_hi: 79,
            interval_min: 0,
            clean_working_sets: true,
            clean_system_working_sets: false,
            clean_standby: false,
            show_tray: true,
            close_to_tray: false,
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        std::fs::read_to_string(config_file())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let path = config_file();
        if let Some(parent) = std::path::Path::new(&path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }
}

pub fn config_file() -> String {
    #[cfg(windows)]
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    #[cfg(not(windows))]
    let base = std::env::var("XDG_CONFIG_HOME")
        .or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            Ok::<String, ()>(format!("{home}/.config"))
        })
        .unwrap_or_else(|_| ".".into());

    format!("{base}/memreduct-rs/settings.json")
}
