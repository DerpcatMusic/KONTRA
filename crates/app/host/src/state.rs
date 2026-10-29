//! What a project saves of the rack (07-APP.md §4): one versioned serde
//! struct. A reload loads each part's instrument again and puts back how it
//! sat in the rack. No v1 or v2 compatibility.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use kontra_engine::ir::RackAt;

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
    /// Its attack delay as auto-align measured it, so a reload need not
    /// measure again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delay_ms: Option<f32>,
    /// Effect types its scripts loaded into slots after `on init`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slot_types: Vec<SlotType>,
}

/// One slot a script loaded with an effect type: the rack, the slot and
/// the type, as the plan numbers them.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SlotType {
    pub rack: SavedRack,
    pub slot: u8,
    pub ty: u16,
}

/// [`RackAt`] as a project saves it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SavedRack {
    Group(u32),
    Inserts,
    Sends,
    Main,
    Bus(u8),
}

impl From<RackAt> for SavedRack {
    fn from(at: RackAt) -> Self {
        match at {
            RackAt::Group(g) => Self::Group(g),
            RackAt::Inserts => Self::Inserts,
            RackAt::Sends => Self::Sends,
            RackAt::Main => Self::Main,
            RackAt::Bus(b) => Self::Bus(b),
        }
    }
}

impl From<SavedRack> for RackAt {
    fn from(at: SavedRack) -> Self {
        match at {
            SavedRack::Group(g) => Self::Group(g),
            SavedRack::Inserts => Self::Inserts,
            SavedRack::Sends => Self::Sends,
            SavedRack::Main => Self::Main,
            SavedRack::Bus(b) => Self::Bus(b),
        }
    }
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
                    delay_ms: p.delay_ms,
                    slot_types: (p.slot_types.iter())
                        .map(|&(at, slot, ty)| SlotType {
                            rack: at.into(),
                            slot,
                            ty,
                        })
                        .collect(),
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
            let types: Vec<_> = (p.slot_types.iter())
                .map(|t| (t.rack.into(), t.slot, t.ty))
                .collect();
            match self.load_saved(p.part, &p.path, &p.scripts, &types) {
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
                    if let Some(ms) = p.delay_ms {
                        self.set_delay(p.part, ms);
                    }
                }
                Err(e) => failed.push((p.part, e.to_string())),
            }
        }
        failed
    }
}
