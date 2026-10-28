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
    /// Read by `Source::Shaped(n)`.
    pub shapers: Vec<ir::Transfer>,
    /// Read by `Source::Blend(n)`.
    pub blends: Vec<ir::Blend>,
    pub routes: Vec<ir::Routes>,
    /// It plays on release (scripts read this).
    pub release_trigger: bool,
    /// The bus its voices play into, an index into [`Plan::buses`]; a
    /// script may move it while they play.
    pub bus: usize,
}

/// The bus `zone`'s group plays into now.
pub(crate) fn bus(groups: &[Group], zone: &Zone) -> usize {
    groups.get(zone.group as usize).map_or(0, |g| g.bus)
}

impl Group {
    /// Whether `source` moves with controller `cc`, through its shaper or
    /// any input of its blend.
    pub(crate) fn reads(&self, source: ir::Source, cc: u8) -> bool {
        let blend = |s: ir::Source| match s {
            ir::Source::Blend(n) => self.blends.get(usize::from(n)).is_some_and(|b| {
                b.terms.iter().any(|t| {
                    t.input.source.controller() == Some(cc)
                        || t.factors
                            .iter()
                            .any(|f| f.input.source.controller() == Some(cc))
                })
            }),
            s => s.controller() == Some(cc),
        };
        match source {
            ir::Source::Shaped(n) => self
                .shapers
                .get(usize::from(n))
                .is_some_and(|s| blend(s.source)),
            s => blend(s),
        }
    }

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
            shapers: Vec::new(),
            blends: Vec::new(),
            routes: Vec::new(),
            release_trigger: false,
            bus: 0,
        }
    }

    /// The route a script addresses by modulator `slot` and `target`; a
    /// negative target is the modulator's first route.
    pub fn mod_mut(&mut self, slot: i32, target: i32) -> Option<&mut ir::Mod> {
        self.mods.iter_mut().find(|m| addressed(m, slot, target))
    }

    /// The LFO of modulator `slot`.
    pub fn lfo(&self, slot: i32) -> Option<&ir::Lfo> {
        self.lfos.get(self.lfo_index(slot)?)
    }

    pub fn lfo_mut(&mut self, slot: i32) -> Option<&mut ir::Lfo> {
        let n = self.lfo_index(slot)?;
        self.lfos.get_mut(n)
    }

    fn lfo_index(&self, slot: i32) -> Option<usize> {
        self.mods
            .iter()
            .filter(|m| i32::from(m.slot) == slot)
            .find_map(|m| match source(&m.kind) {
                Some(ir::Source::Lfo(n)) => Some(usize::from(n)),
                _ => None,
            })
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
    /// Its index among its group's zones ([`ir::ModKind::Mix`] names it).
    pub member: u16,
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
            member: 0,
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
    /// Plays on the main output ([`ir::Bus::to_main`]).
    pub to_main: bool,
    /// A disabled bus starts no voices (purged mic): it costs no CPU.
    pub enabled: bool,
    /// Routes moving its inserts ([`ir::Bus::mods`]) and the blends they read.
    pub mods: Vec<ir::ModKind>,
    pub blends: Vec<ir::Blend>,
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
    /// How [`ir::KnobTarget::Slot`] knobs move the effects they address.
    pub slot_laws: Vec<ir::SlotLaw>,
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
                to_main: true,
                enabled: true,
                mods: Vec::new(),
                blends: Vec::new(),
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
            slot_laws: Vec::new(),
        }
    }
}

/// What drives a route; the amplitude envelope has no source.
pub(crate) fn source(kind: &ir::ModKind) -> Option<ir::Source> {
    use ir::ModKind as K;
    match *kind {
        K::Volume { source, .. }
        | K::Pitch { source, .. }
        | K::Pan { source, .. }
        | K::Cutoff { source, .. }
        | K::Resonance { source, .. }
        | K::EqGain { source, .. }
        | K::EffectParam { source, .. }
        | K::Mix { source, .. }
        | K::SampleStart { source, .. }
        | K::Attack { source, .. }
        | K::Release { source, .. }
        | K::LfoPhase { source, .. }
        | K::LfoRate { source, .. }
        | K::Intensity { source, .. }
        | K::LoopShift { source, .. } => Some(source),
        K::Envelope => None,
    }
}

/// Whether scripts address `m` by modulator `slot` and `target`. A negative
/// target is the first route (measured, probes/mod_par_laws.lua: generic
/// -1 moves route 0 alone).
pub(crate) fn addressed(m: &ir::Mod, slot: i32, target: i32) -> bool {
    i32::from(m.slot) == slot && i32::from(m.target) == target.max(0)
}
