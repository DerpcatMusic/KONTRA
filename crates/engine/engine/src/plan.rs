//! The immutable instrument the engine plays, built off the audio thread by a
//! format translator (02-ENGINE.md §6). Sampler behaviour is data here
//! (13-FRESH-CORE.md §2.1): scripts are optional, not where round robin lives.

use crate::ScriptPlan;
use std::ops::RangeInclusive;

use kontra_ir as ir;
use kontra_voice::{Ahdsr, Region};

/// When a zone sounds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Trigger {
    /// On note-on.
    #[default]
    Attack,
    /// On note release (key up, or the pedal lifting), from the original velocity.
    Release,
    /// On note-on with no other key down.
    First,
    /// On note-on while another key is down.
    Legato,
}

/// Which counter a round robin steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RrCounter {
    /// One counter for the instrument.
    Global,
    /// One counter per key.
    Key,
    /// One counter per voice group ([`Zone::group`]).
    Group,
}

/// The zone plays on step `position` (0-based) of a cycle of `length`.
/// Every zone of one note reads the same step, so mic layers always agree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoundRobin {
    pub counter: RrCounter,
    pub position: u8,
    pub length: u8,
    /// Random step, never the same twice in a row; else sequential.
    pub random: bool,
}

/// One start condition of a zone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Atom {
    /// The latch of [`Plan::switches`] at this index is set.
    Switch(u16),
    /// The controller's value is in range.
    Cc(u8, RangeInclusive<u8>),
    RoundRobin(RoundRobin),
}

/// What a crossfade or a range reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Velocity,
    Key,
    /// A MIDI CC, live: moving it changes sounding voices.
    Cc(u8),
}

/// A crossfade over 0..=127 values of `control`: a linear amplitude ramp,
/// Kontakt's measured zone fade (`(v − L + 1)/(F + 1)` with `fade_in` =
/// (L − 1, L + F), KONTAKT_REFERENCE.md §11 and §15).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Crossfade {
    pub control: Control,
    /// Silent at or below `.0`, full at or above `.1`.
    pub fade_in: Option<(u8, u8)>,
    /// Full at or below `.0`, silent at or above `.1`.
    pub fade_out: Option<(u8, u8)>,
}

impl Crossfade {
    #[must_use]
    pub fn gain(&self, value: u8) -> f32 {
        let ramp = |(lo, hi): (u8, u8)| -> f32 {
            if value <= lo {
                0.0
            } else if value >= hi {
                1.0
            } else {
                f32::from(value - lo) / f32::from(hi - lo)
            }
        };
        let fade_in = self.fade_in.map_or(1.0, ramp);
        let fade_out = self.fade_out.map_or(1.0, |r| 1.0 - ramp(r));
        fade_in * fade_out
    }
}

/// Level, pan and pitch a group or the instrument adds to its voices; a
/// script may change them while they play.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Level {
    /// Linear.
    pub gain: f32,
    /// -1..=1, added to the zone's pan.
    pub pan: f32,
    /// Semitones.
    pub tune: f64,
}

impl Level {
    pub const UNITY: Self = Self {
        gain: 1.0,
        pan: 0.0,
        tune: 0.0,
    };
}

/// What a group's voices share and a script may change while they play.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    /// What scripts find it by (`find_group`, `group_name`).
    pub name: String,
    pub level: Level,
    /// Its samples are unloaded (KSP `purge_group`): its zones do not play.
    pub purged: bool,
    /// Effects on its voices, by slot.
    pub inserts: Vec<Option<ir::Effect>>,
    pub mods: Vec<ir::Mod>,
    pub envelope: Ahdsr,
    /// Read by `Source::Envelope(n)` and `Source::Lfo(n)`.
    pub envelopes: Vec<ir::Breakpoints>,
    pub lfos: Vec<ir::Lfo>,
    pub routes: Vec<ir::Routes>,
}

impl Group {
    /// Velocity 24 dB down at zero, a gate envelope.
    #[must_use]
    pub fn standard() -> Self {
        Self {
            name: String::new(),
            level: Level::UNITY,
            purged: false,
            inserts: Vec::new(),
            mods: vec![ir::Mod {
                name: String::new(),
                slot: 0,
                target: 0,
                bypass: false,
                kind: ir::ModKind::Volume {
                    source: ir::Source::Velocity,
                    intensity: 1.0,
                    curve: ir::Curve::Decibels(24.0),
                },
            }],
            envelope: Ahdsr::GATE,
            envelopes: Vec::new(),
            lfos: Vec::new(),
            routes: Vec::new(),
        }
    }

    /// The route a script addresses by modulator `slot` and `target`; a
    /// negative target is the modulator's first route.
    pub fn mod_mut(&mut self, slot: i32, target: i32) -> Option<&mut ir::Mod> {
        self.mods.iter_mut().find(|m| addressed(m, slot, target))
    }
}

/// One playable zone, resolved: no format knowledge is left here.
#[derive(Clone)]
pub struct Zone {
    // Selection.
    pub keys: RangeInclusive<u8>,
    /// MIDI velocities 1..=127 the zone answers.
    pub velocities: RangeInclusive<u8>,
    /// See [`ir::Zone::slot`].
    pub slot: u16,
    /// Start conditions folded left to right by their joins; empty always starts.
    pub start: Vec<(ir::Join, Atom)>,
    pub trigger: Trigger,
    pub crossfades: Vec<Crossfade>,
    // Voice group.
    /// Voice group, for polyphony limits, choke and group round robins.
    pub group: u32,
    /// A voice of this zone fades when a voice of group `off_by` starts (hi-hat choke).
    pub off_by: Option<u32>,
    /// Index into [`Plan::voice_groups`].
    pub voice_group: Option<u32>,
    // Playback.
    /// The key at which the sample plays at its recorded pitch.
    pub root: u8,
    /// Semitones of pitch per key away from `root`: 1 tracks the keyboard, 0 is fixed.
    pub keytrack: f64,
    /// Fine tune in semitones.
    pub tune: f64,
    /// The sample's own rate, for the step against the engine rate.
    pub sample_rate: u32,
    pub region: Region,
    /// Frames a [`ir::ModKind::SampleStart`] may move the start either way.
    pub start_mod: u64,
    /// Linear gain at full velocity.
    pub gain: f32,
    /// −1 (left) ..= 1 (right).
    pub pan: f32,
    /// Release triggers: dB lost per second the key was held.
    pub release_decay_db: f32,
    /// Ignores note-off and plays to the end (drums, one-shots).
    pub one_shot: bool,
    /// Output bus (mic position) index into [`Plan::buses`].
    pub bus: usize,
}

impl Zone {
    /// A plain attack zone over the full key and velocity range on bus 0.
    #[must_use]
    pub fn new(region: Region, root: u8, sample_rate: u32) -> Self {
        Self {
            keys: 0..=127,
            velocities: 1..=127,
            slot: 0,
            start: Vec::new(),
            trigger: Trigger::Attack,
            crossfades: Vec::new(),
            group: 0,
            off_by: None,
            voice_group: None,
            root,
            keytrack: 1.0,
            tune: 0.0,
            sample_rate,
            region,
            start_mod: 0,
            gain: 1.0,
            pan: 0.0,
            release_decay_db: 0.0,
            one_shot: false,
            bus: 0,
        }
    }
}

/// A mixer channel: one mic position, or one output of the instrument.
#[derive(Clone, Debug, PartialEq)]
pub struct Bus {
    pub name: String,
    pub gain: f32,
    /// -1..=1 balance.
    pub pan: f32,
    /// Effects on what it carries, by slot.
    pub inserts: Vec<Option<ir::Effect>>,
    /// Later buses it feeds ([`ir::Bus::sends`]).
    pub sends: Vec<ir::BusSend>,
    /// A disabled bus starts no voices (purged mic): it costs no CPU.
    pub enabled: bool,
}

/// A prepared impulse response.
#[derive(Clone, Debug, PartialEq)]
pub struct Response {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
}

/// Pool sizes, zones, groups and buses. Growing a pool is a new plan, not a resize.
#[derive(Clone)]
pub struct Plan {
    pub zones: Vec<Zone>,
    /// Indexed by [`Zone::group`].
    pub groups: Vec<Group>,
    pub buses: Vec<Bus>,
    /// Voice limits per group: (group, polyphony). The oldest voice in the group fades.
    pub polyphony: Vec<(u32, u16)>,
    /// Voice limits shared across groups.
    pub voice_groups: Vec<ir::VoiceGroup>,
    /// Key ranges [`Atom::Switch`] latches; distinct.
    pub switches: Vec<RangeInclusive<u8>>,
    /// The key the latch starts as if pressed.
    pub default_keyswitch: Option<u8>,
    pub voices: usize,
    pub notes: usize,
    /// Pending timed work; also bounds one block's input.
    pub queue: usize,
    /// The instrument's scripts in slot order: an event passes through each
    /// in turn, and a note one plays only through those after it.
    pub scripts: Vec<ScriptPlan>,
    /// Effects on everything, by slot.
    pub inserts: Vec<Option<ir::Effect>>,
    /// Send effects, by send slot, and the main rack after them.
    pub sends: Vec<Option<ir::Effect>>,
    pub main: Vec<Option<ir::Effect>>,
    /// Convolution responses at the engine's rate, by [`ir::Instrument::impulses`]
    /// index; `None` when unreadable.
    pub impulses: Vec<Option<Response>>,
}

impl Plan {
    /// One bus, no behaviour beyond the zones.
    #[must_use]
    pub fn new(zones: Vec<Zone>, voices: usize) -> Self {
        let groups = zones
            .iter()
            .map(|z| z.group as usize + 1)
            .max()
            .unwrap_or(0);
        Self {
            groups: vec![Group::standard(); groups],
            zones,
            buses: vec![Bus {
                name: "Main".into(),
                gain: 1.0,
                pan: 0.0,
                inserts: Vec::new(),
                sends: Vec::new(),
                enabled: true,
            }],
            polyphony: Vec::new(),
            voice_groups: Vec::new(),
            switches: Vec::new(),
            default_keyswitch: None,
            voices,
            notes: 128,
            queue: 1024,
            scripts: Vec::new(),
            inserts: Vec::new(),
            sends: Vec::new(),
            main: Vec::new(),
            impulses: Vec::new(),
        }
    }
}

/// Whether scripts address `m` by modulator `slot` and `target` (any when negative).
pub(crate) fn addressed(m: &ir::Mod, slot: i32, target: i32) -> bool {
    i32::from(m.slot) == slot && (target < 0 || i32::from(m.target) == target)
}
