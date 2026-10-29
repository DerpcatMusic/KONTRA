//! What the player keeps between sessions about their libraries, whatever
//! the project: the roots, favourites and what they loaded lately.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::Root;

/// How many recent instruments are kept.
const RECENT: usize = 32;

/// The player's library preferences; the host stores them where it keeps
/// its settings. Fields this version does not know are dropped on save.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub roots: Vec<Root>,
    /// Starred libraries, by folder.
    pub starred: Vec<PathBuf>,
    /// Instruments loaded lately, newest first.
    pub recent: Vec<PathBuf>,
    /// Where a part the browser adds listens and plays: the lowest MIDI
    /// channel no other part has (Kontakt's auto-increment), else every
    /// channel; its own output pair, else the main one.
    pub next_channel: bool,
    pub own_output: bool,
}

impl Prefs {
    /// Stars or unstars the library at `dir`.
    pub fn star(&mut self, dir: &Path, on: bool) {
        self.starred.retain(|d| d != dir);
        if on {
            self.starred.push(dir.to_path_buf());
        }
    }

    #[must_use]
    pub fn starred(&self, dir: &Path) -> bool {
        self.starred.iter().any(|d| d == dir)
    }

    /// Puts `preset` first among the recent instruments.
    pub fn loaded(&mut self, preset: &Path) {
        self.recent.retain(|p| p != preset);
        self.recent.insert(0, preset.to_path_buf());
        self.recent.truncate(RECENT);
    }

    /// Adds a root once.
    pub fn add_root(&mut self, root: Root) {
        if !self.roots.iter().any(|r| r.path == root.path) {
            self.roots.push(root);
        }
    }
}
