//! What a project saves of the rack (07-APP.md §4): one versioned serde
//! struct. A reload loads each part's instrument again and puts back how it
//! sat in the rack. No v1 or v2 compatibility.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::rack::{Mix, Rack};

/// Bumped when a field changes meaning; a state of another version loads
/// nothing rather than something wrong.
pub const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    pub version: u32,
    pub parts: Vec<SavedPart>,
}

/// One part: where it sits, the instrument it plays and how it sounds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedPart {
    pub part: usize,
    pub path: PathBuf,
    pub channel: u8,
    pub output: u8,
    pub gain: f32,
    pub pan: f32,
    pub solo: bool,
    pub mute: bool,
    /// By instrument script, its saved values (Kontakt's `"<name> <value…>"`);
    /// empty for a part saved before scripts were.
    #[serde(default)]
    pub scripts: Vec<Vec<String>>,
}

impl Saved {
    /// The state of a rack holding `parts`.
    #[must_use]
    pub fn new(parts: Vec<SavedPart>) -> Self {
        Self {
            version: VERSION,
            parts,
        }
    }

    /// The state in `json`; an empty or foreign state is an empty rack.
    #[must_use]
    pub fn read(json: &str) -> Self {
        serde_json::from_str::<Self>(json)
            .ok()
            .filter(|s| s.version == VERSION)
            .unwrap_or_else(|| Self::new(Vec::new()))
    }

    #[must_use]
    pub fn write(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

impl Rack {
    /// The rack as a project saves it. Parts built in memory (no file) are
    /// left out: there is nothing to load them from.
    #[must_use]
    pub fn save(&self) -> Saved {
        let parts = (self.parts().iter().enumerate())
            .filter_map(|(part, p)| {
                let p = p.as_ref()?;
                Some(SavedPart {
                    part,
                    path: p.path.clone()?,
                    channel: p.mix.channel,
                    output: p.mix.output,
                    gain: p.mix.gain,
                    pan: p.mix.pan,
                    solo: p.solo,
                    mute: p.mute,
                    scripts: self.saved_scripts(part),
                })
            })
            .collect();
        Saved::new(parts)
    }

    /// Empties the rack and loads `saved` into it, part by part. A part
    /// that fails to load is left empty; its error is returned.
    pub fn restore(&mut self, saved: &Saved) -> Vec<(usize, String)> {
        let loaded: Vec<usize> = (self.parts().iter().enumerate())
            .filter_map(|(n, p)| p.as_ref().map(|_| n))
            .collect();
        for part in loaded {
            self.remove(part);
        }
        let mut failed = Vec::new();
        for p in &saved.parts {
            match self.load_saved(p.part, &p.path, &p.scripts) {
                Ok(()) => {
                    self.set_mix(
                        p.part,
                        Mix {
                            channel: p.channel,
                            output: p.output,
                            gain: p.gain,
                            pan: p.pan,
                            silent: false,
                        },
                    );
                    self.set_solo_mute(p.part, p.solo, p.mute);
                }
                Err(e) => failed.push((p.part, e.to_string())),
            }
        }
        failed
    }
}
