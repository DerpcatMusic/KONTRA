//! The interface's preferences, kept between sessions.
//!
//! One small JSON file in the user's config directory, shared by every
//! instance, as the reference's is. A missing or unreadable file is the
//! defaults; a write is atomic, so a crash never leaves half a file.

use serde::{Deserialize, Serialize};
use smooth_ui::menus::Prefs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct File {
    tooltips: bool,
    frequency_near_cursor: bool,
    always_ultra: bool,
    low_latency_all: bool,
    listen_on_drag: bool,
    listen_delta: bool,
}

impl Default for File {
    fn default() -> Self {
        Self::from(Prefs::default())
    }
}

impl From<Prefs> for File {
    fn from(p: Prefs) -> Self {
        Self {
            tooltips: p.tooltips,
            frequency_near_cursor: p.frequency_near_cursor,
            always_ultra: p.always_ultra,
            low_latency_all: p.low_latency_all,
            listen_on_drag: p.listen_on_drag,
            listen_delta: p.listen_delta,
        }
    }
}

impl From<File> for Prefs {
    fn from(f: File) -> Self {
        Self {
            tooltips: f.tooltips,
            frequency_near_cursor: f.frequency_near_cursor,
            always_ultra: f.always_ultra,
            low_latency_all: f.low_latency_all,
            listen_on_drag: f.listen_on_drag,
            listen_delta: f.listen_delta,
        }
    }
}

fn path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("smooth").join("config.json"))
}

/// The saved preferences, or the defaults.
pub fn load() -> Prefs {
    path()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|bytes| serde_json::from_slice::<File>(&bytes).ok())
        .map_or_else(Prefs::default, Prefs::from)
}

/// Keep the preferences. Failure is silent: they are a convenience.
pub fn save(prefs: Prefs) {
    let Some(path) = path() else { return };
    let Some(dir) = path.parent() else { return };
    let Ok(json) = serde_json::to_vec_pretty(&File::from(prefs)) else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let temp = path.with_extension("json.tmp");
    if std::fs::write(&temp, json).is_ok() {
        let _ = std::fs::rename(temp, path);
    }
}
