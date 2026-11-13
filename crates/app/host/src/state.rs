//! What a project saves of the rack (07-APP.md §4): one versioned serde
//! struct. A reload loads each part's instrument again and puts back how it
//! sat in the rack. No v1 or v2 compatibility.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use kontra_engine::ir::RackAt;

use kontra_protocol::MAX_PARTS;

use crate::rack::{Delays, Mix, Rack};

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
    /// The instrument file it loads: for a user preset, its base, so the
    /// project never needs the preset's file.
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
    /// Its attack delays as auto-align measured them, so a reload need not
    /// measure again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delays: Option<Delays>,
    /// Effect types its scripts loaded into slots after `on init`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slot_types: Vec<SlotType>,
    /// Library responses its scripts loaded into convolutions after
    /// `on init`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slot_responses: Vec<SlotResponse>,
    /// The user preset it plays, only to name it: a file that is gone
    /// leaves the part its base instrument, changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<PathBuf>,
    /// The player changed its preset after it loaded ([`crate::rack::Part::modified`]).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub modified: bool,
}

pub use kontra_library::presets::{SavedRack, SlotResponse, SlotType};

/// Effect types and responses scripts loaded after `on init`, by rack.
pub(crate) type LiveSlots = (Vec<(RackAt, u8, u16)>, Vec<(RackAt, u8, String)>);

/// `types` and `responses` as a part keeps them, as a file saves them.
pub(crate) fn saved_slots(
    types: &[(RackAt, u8, u16)],
    responses: &[(RackAt, u8, String)],
) -> (Vec<SlotType>, Vec<SlotResponse>) {
    let types = (types.iter())
        .map(|&(at, slot, ty)| SlotType {
            rack: saved(at),
            slot,
            ty,
        })
        .collect();
    let responses = (responses.iter())
        .map(|(at, slot, name)| SlotResponse {
            rack: saved(*at),
            slot: *slot,
            name: name.clone(),
        })
        .collect();
    (types, responses)
}

/// [`saved_slots`] back as a part keeps them.
pub(crate) fn live_slots(types: &[SlotType], responses: &[SlotResponse]) -> LiveSlots {
    let types = (types.iter())
        .map(|t| (live(t.rack), t.slot, t.ty))
        .collect();
    let responses = (responses.iter())
        .map(|r| (live(r.rack), r.slot, r.name.clone()))
        .collect();
    (types, responses)
}

/// `at` as a file names it.
fn saved(at: RackAt) -> SavedRack {
    match at {
        RackAt::Group(g) => SavedRack::Group(g),
        RackAt::Inserts => SavedRack::Inserts,
        RackAt::Sends => SavedRack::Sends,
        RackAt::Main => SavedRack::Main,
        RackAt::Bus(b) => SavedRack::Bus(b),
    }
}

/// [`saved`] back as the engine names it.
fn live(at: SavedRack) -> RackAt {
    match at {
        SavedRack::Group(g) => RackAt::Group(g),
        SavedRack::Inserts => RackAt::Inserts,
        SavedRack::Sends => RackAt::Sends,
        SavedRack::Main => RackAt::Main,
        SavedRack::Bus(b) => RackAt::Bus(b),
    }
}

/// The editor's view as a project keeps it: its tab, panels and sizes,
/// and how each part sits. The numbers index the editor's own lists.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedView {
    pub tab: u8,
    pub browser_hidden: bool,
    pub keyboard_hidden: bool,
    pub keys_hidden: bool,
    pub mixer_wide: bool,
    pub octave: i8,
    pub scale: u8,
    pub face_scale: u8,
    pub format: u8,
    pub shelf: u8,
    pub descending: bool,
    /// The browser's library list's share of its height; 0 is the default.
    pub split: f32,
    /// How a library's view is drawn: Original, Vector, KONTRA.
    pub look: u8,
    pub parts: Vec<SavedPartView>,
}

/// One rack part in the editor: unfolded, and its tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedPartView {
    pub part: usize,
    pub open: bool,
    pub tab: u8,
    /// Its own look, 1 + the view's index; 0 follows the view's.
    #[serde(default)]
    pub look: u8,
}

impl SavedView {
    /// The view in `json`; anything unreadable is a fresh editor.
    #[must_use]
    pub fn read(json: &str) -> Self {
        serde_json::from_str(json).unwrap_or_default()
    }

    #[must_use]
    pub fn write(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
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
        Self::parse(json).unwrap_or_else(|_| Self::new(Vec::new()))
    }

    /// The state in `json`, made safe to load: parts past the rack, repeated
    /// parts and values that are not numbers are dropped or reset. A blank
    /// state is a new instance's: an empty rack, no error.
    ///
    /// # Errors
    /// A message for the player when the state is corrupt or of another
    /// version; the rack then starts empty.
    pub fn parse(json: &str) -> Result<Self, String> {
        if json.trim().is_empty() {
            return Ok(Self::new(Vec::new()));
        }
        let mut saved: Self = serde_json::from_str(json).map_err(|e| {
            format!("the project's saved state is damaged ({e}); the rack starts empty")
        })?;
        if saved.version != VERSION {
            return Err(format!(
                "the project's saved state is version {}, this KONTRA reads version {VERSION}; the rack starts empty",
                saved.version
            ));
        }
        let mut seen = Vec::new();
        saved.parts.retain(|p| {
            p.part < MAX_PARTS && !seen.contains(&p.part) && {
                seen.push(p.part);
                true
            }
        });
        for p in &mut saved.parts {
            p.gain = if p.gain.is_finite() {
                p.gain.clamp(0.0, 16.0)
            } else {
                1.0
            };
            p.pan = if p.pan.is_finite() {
                p.pan.clamp(-1.0, 1.0)
            } else {
                0.0
            };
            if p.channel > 16 {
                p.channel = 0;
            }
            p.output = p.output.min(MAX_PARTS as u8 - 1);
        }
        Ok(saved)
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
                let user = (p.path.as_deref()).filter(|f| crate::presets::ours(f));
                Some(SavedPart {
                    part,
                    path: if user.is_some() { &p.source } else { &p.path }.clone()?,
                    preset: user.map(std::path::Path::to_owned),
                    channel: p.mix.channel,
                    output: p.mix.output,
                    gain: p.mix.gain,
                    pan: p.mix.pan,
                    solo: p.solo,
                    mute: p.mute,
                    scripts: self.saved_scripts(part),
                    delays: p.delays.clone(),
                    slot_types: saved_slots(&p.slot_types, &p.slot_responses).0,
                    slot_responses: saved_slots(&p.slot_types, &p.slot_responses).1,
                    modified: p.modified,
                })
            })
            .collect();
        Saved::new(parts)
    }

    /// Makes the rack `saved`, part by part. A part already as saved stays
    /// as it is, so a host that hands over the same state again (at
    /// creation, then again on activation) loads nothing twice. A part that
    /// fails to load is left empty; its error is returned.
    pub fn restore(&mut self, saved: &Saved) -> Vec<(usize, String)> {
        let now = self.save().parts;
        // A state saved before auto-align measured a part still matches it.
        let same = |p: &SavedPart| {
            now.iter().any(|n| {
                n.part == p.part
                    && *n
                        == SavedPart {
                            delays: p.delays.clone().or_else(|| n.delays.clone()),
                            ..p.clone()
                        }
            })
        };
        let kept: Vec<usize> = (saved.parts.iter())
            .filter(|p| same(p))
            .map(|p| p.part)
            .collect();
        let loaded: Vec<usize> = (self.parts().iter().enumerate())
            .filter_map(|(n, p)| p.as_ref().map(|_| n))
            .filter(|n| !kept.contains(n))
            .collect();
        for part in loaded {
            self.remove(part);
        }
        let mut failed = Vec::new();
        for p in saved.parts.iter().filter(|p| !kept.contains(&p.part)) {
            let (types, responses) = live_slots(&p.slot_types, &p.slot_responses);
            match self.load_saved(p.part, &p.path, &p.scripts, &types, &responses) {
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
                    if let Some(delays) = p.delays.clone() {
                        self.set_delays(p.part, delays);
                    }
                    match &p.preset {
                        Some(preset) if preset.is_file() => self.set_preset(p.part, preset),
                        Some(_) => self.set_modified(p.part),
                        None => {}
                    }
                    if p.modified {
                        self.set_modified(p.part);
                    }
                }
                Err(e) => failed.push((p.part, e.to_string())),
            }
        }
        failed
    }
}
