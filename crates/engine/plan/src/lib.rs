//! The immutable instrument the engine plays, built off the audio thread by a
//! format translator (02-ENGINE.md §6). Sampler behaviour is data here
//! (13-FRESH-CORE.md §2.1): scripts are optional, not where round robin lives.

mod engine_pars;
pub mod files;
pub mod midi_file;
pub mod modulation;
pub mod par_laws;
pub mod par_values;
mod route;
pub mod route_depth;
mod script;
mod slots;
pub mod soa;
mod zones;

pub use engine_pars::{EnginePar, EnginePars};
pub use route::held_time;
pub use script::{
    Callbacks, CcLink, Controls, Holder, KnobModPlan, LookPars, ParLaw, ParRead, ParWrite, Recall,
    Saved, ScriptPlan,
};
pub use soa::{SavedZoneDesc, ZoneMatch, ZonePlay, ZoneTable};
pub use zones::{LoopPar, UserZone, ZoneRules, user_loops, zone_name};

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

/// Who starts [`Trigger::Release`] zones.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReleaseZones {
    /// The engine, when a note is released.
    #[default]
    AtRelease,
    /// A script, by playing a note into their groups: they start and end
    /// with that note, as other zones do.
    WithNote,
}

/// What the sustain pedal (CC 64) does besides reaching the scripts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum SustainPedal {
    /// Down, it holds released notes until it lifts.
    #[default]
    Holds,
    /// Nothing: the scripts hold notes themselves.
    ToScripts,
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
    /// Random step; else sequential.
    pub random: bool,
    /// When `random`, exclude the previous winner so consecutive notes do not repeat.
    pub no_repeat: bool,
    /// When `random`, redraw on release triggers instead of reusing the attack's draw.
    pub release_redraws: bool,
}

/// One start condition of a zone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Atom {
    /// The latch of [`Plan::switches`] at this index is set.
    Switch(u16),
    /// The controller's value is in range.
    Cc(u8, RangeInclusive<u8>),
    RoundRobin(RoundRobin),
    /// Random probability window on `[0, 10_000)` (`lorand`/`hirand`).
    Random {
        lo: u16,
        hi: u16,
    },
}

/// What a crossfade or a range reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Velocity,
    Key,
    /// A MIDI CC, live: moving it changes sounding voices.
    Cc(u8),
}

/// A crossfade over 0..=127 values of `control`: linear amplitude ramp
/// (Kontakt's measured zone fade: `(v − L + 1)/(F + 1)` with `fade_in` =
/// (L − 1, L + F), KONTAKT_REFERENCE.md §11 and §15) or equal-power sine/cosine
/// (SFZ default).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Crossfade {
    pub control: Control,
    /// Silent at or below `.0`, full at or above `.1`.
    pub fade_in: Option<(i16, i16)>,
    /// Full at or below `.0`, silent at or above `.1`.
    pub fade_out: Option<(i16, i16)>,
    pub law: ir::CrossfadeLaw,
}

impl Crossfade {
    #[must_use]
    pub fn gain(&self, value: u8) -> f32 {
        let value = i16::from(value);
        let ramp = |(lo, hi): (i16, i16)| -> f32 {
            if value <= lo {
                0.0
            } else if value >= hi {
                1.0
            } else {
                f32::from(value - lo) / f32::from(hi - lo)
            }
        };
        let shape_in = |t: f32| match self.law {
            ir::CrossfadeLaw::Linear => t,
            ir::CrossfadeLaw::EqualPower => (t * std::f32::consts::FRAC_PI_2).sin(),
        };
        let shape_out = |t: f32| match self.law {
            ir::CrossfadeLaw::Linear => 1.0 - t,
            ir::CrossfadeLaw::EqualPower => (t * std::f32::consts::FRAC_PI_2).cos(),
        };
        let fade_in = self.fade_in.map_or(1.0, |r| shape_in(ramp(r)));
        let fade_out = self.fade_out.map_or(1.0, |r| shape_out(ramp(r)));
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

    /// `gain` and `pan`, untuned.
    #[must_use]
    pub const fn panned(gain: f32, pan: f32) -> Self {
        Self {
            gain,
            pan,
            tune: 0.0,
        }
    }
}

/// What a group's voices share and a script may change while they play.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    /// What scripts find it by (`find_group`, `group_name`).
    pub name: String,
    pub level: Level,
    /// Its samples are unloaded (KSP `purge_group`): its zones do not play.
    pub purged: bool,
    /// Muted (or silenced by another group's solo): zones stay loaded, start no voices.
    pub muted: bool,
    /// Effects on its voices, by slot.
    pub inserts: Vec<Option<ir::Effect>>,
    /// First insert slot that runs after the voice's amplitude envelope,
    /// volume and pan (`0..amp_split` are pre-amp; `amp_split..` are
    /// post-amp). Values `>= inserts.len()` place every insert pre-amp.
    pub amp_split: u8,
    pub mods: Vec<ir::Mod>,
    pub envelope: Ahdsr,
    /// Read by `Source::Envelope(n)` and `Source::Lfo(n)`.
    pub envelopes: Vec<ir::Breakpoints>,
    pub lfos: Vec<ir::Lfo>,
    /// Read by `Source::Shaped(n)`.
    pub shapers: Vec<ir::Transfer>,
    /// Read by `Source::Blend(n)`.
    pub blends: Vec<ir::Blend>,
    /// Audio-rate envelope followers on this group, by index.
    pub followers: Vec<ir::EnvelopeFollower>,
    pub routes: Vec<ir::Routes>,
    /// It plays on release (scripts read this).
    pub release_trigger: bool,
    /// Its held controls ([`kontra_ir::Group::held`]).
    pub held: Vec<(i8, u32, i32)>,
    /// The bus its voices play into, an index into [`Plan::buses`]; a
    /// script may move it while they play.
    pub bus: usize,
    /// Its zones play as wavetables, read so. A zone `set_zone_par` moves
    /// in gets its table built off the audio thread.
    pub wavetable: Option<kontra_voice::Read>,
    /// Its zones synthesize audio via a real-time polyBLEP oscillator
    /// ([`kontra_ir::Generator::Oscillator`]).
    pub oscillator: Option<kontra_voice::Oscillator>,
    /// Its zones synthesize audio via a real-time granular cloud engine
    /// ([`kontra_ir::Generator::Granular`]).
    pub granular: Option<kontra_voice::GranularSpec>,
    /// The speed its voices start at when its source stretches time
    /// ([`kontra_ir::Group::speed`]).
    pub speed: Option<f32>,
    /// Its notes' portamento ([`kontra_ir::Group::glide`]).
    pub glide: Option<ir::Glide>,
    /// What it gives each of its zones.
    pub zones: ZoneRules,
}

/// The bus `zone`'s group plays into now.
pub fn bus(groups: &[Group], zone: &Zone) -> usize {
    groups.get(zone.group as usize).map_or(0, |g| g.bus)
}

impl Group {
    /// Slot index where the amplitude stage sits, clamped to `self.inserts.len()`.
    #[must_use]
    pub fn amp_split_index(&self) -> usize {
        usize::from(self.amp_split).min(self.inserts.len())
    }

    /// Whether `source` moves with controller `cc`, through its shaper or
    /// any input of its blend.
    pub fn reads(&self, source: ir::Source, cc: u8) -> bool {
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

    /// Whether a timed source can also move through a controller-driven
    /// intensity, phase or rate route. The direct source uses [`Self::reads`].
    #[must_use]
    pub fn reads_modulated(&self, source: ir::Source, cc: u8) -> bool {
        if self.reads(source, cc) {
            return true;
        }
        let target_source = match source {
            ir::Source::Shaped(n) => self
                .shapers
                .get(usize::from(n))
                .map_or(source, |s| s.source),
            other => other,
        };
        self.mods
            .iter()
            .filter(|m| !m.bypass)
            .any(|m| match m.route.target {
                ir::ParamAddr::RouteIntensity { of }
                    if of == target_source
                        && matches!(
                            target_source,
                            ir::Source::Lfo(_) | ir::Source::Envelope(_)
                        ) =>
                {
                    self.reads(m.route.source, cc)
                }
                ir::ParamAddr::LfoParam { lfo, .. } if target_source == ir::Source::Lfo(lfo) => {
                    self.reads(m.route.source, cc)
                }
                _ => false,
            })
    }

    /// Velocity 24 dB down at zero, a gate envelope.
    #[must_use]
    pub fn standard() -> Self {
        Self {
            name: String::new(),
            level: Level::UNITY,
            purged: false,
            muted: false,
            inserts: Vec::new(),
            amp_split: u8::MAX,
            mods: vec![ir::Mod {
                name: String::new(),
                slot: 0,
                target: 0,
                bypass: false,
                route: ir::ModRoute::new(ir::Source::Velocity, ir::ParamAddr::Volume, 1.0)
                    .law(ir::RouteLaw::Gain(ir::Curve::Decibels(24.0))),
            }],
            envelope: Ahdsr::GATE,
            envelopes: Vec::new(),
            lfos: Vec::new(),
            shapers: Vec::new(),
            blends: Vec::new(),
            followers: Vec::new(),
            routes: Vec::new(),
            release_trigger: false,
            held: Vec::new(),
            bus: 0,
            wavetable: None,
            oscillator: None,
            granular: None,
            speed: None,
            glide: None,
            zones: ZoneRules::PLAIN,
        }
    }

    /// The route a script addresses by modulator `slot` and `target`; a
    /// negative target is the modulator's first route.
    pub fn mod_mut(&mut self, slot: i32, target: i32) -> Option<&mut ir::Mod> {
        self.mods.iter_mut().find(|m| addressed(m, slot, target))
    }

    /// Whether it lacks the LFO, route or modulator `holder` names at
    /// `slot` (and, for a route, `target`). A modulator the engine does
    /// not carry still exists: its [`ir::Routes`] keep it.
    #[must_use]
    pub fn lacks(&self, holder: Holder, slot: i32, target: i32) -> bool {
        let kept = |r: &&ir::Routes| i32::from(r.slot) == slot;
        let routed = || self.routes.iter().find(kept);
        match holder {
            Holder::Lfo => self.lfo(slot).is_none() && routed().is_none(),
            Holder::Route => {
                !self.mods.iter().any(|m| addressed(m, slot, target))
                    && routed().is_none_or(|r| {
                        usize::try_from(target.max(0)).is_ok_and(|t| t >= r.names.len())
                    })
            }
            Holder::Mod => {
                !self.mods.iter().any(|m| i32::from(m.slot) == slot) && routed().is_none()
            }
            _ => false,
        }
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
            .find_map(|m| match m.route.driver() {
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
    /// See [`ir::Zone::id`].
    pub id: u32,
    /// Start conditions: `and`/`and not` bind tighter than `or`, as in
    /// one unparenthesized condition; empty always starts.
    pub start: Vec<(ir::Join, Atom)>,
    pub trigger: Trigger,
    /// Velocity and key crossfades, then any on controllers. Those on
    /// velocity and key follow [`Self::fades`] and the current ranges
    /// ([`Self::range_crossfades`]).
    pub crossfades: Vec<Crossfade>,
    /// Velocity and key fade lengths, each (in, out), as the zone saves them.
    pub fades: [(u8, u8); 2],
    // Voice group.
    /// Voice group, for polyphony limits, choke and group round robins.
    pub group: u32,
    /// Its index among its group's zones ([`ir::ParamAddr::ZoneMix`] names it).
    pub member: u16,
    /// A voice of this zone stops when a voice of voice group `off_by`
    /// starts (hi-hat choke), as its own voice group stops voices.
    pub off_by: Option<u32>,
    /// Its voices of a key stop when that key is played again.
    pub repeat_cuts: bool,
    /// A script offset past [`Self::start_mod`] plays from the start.
    pub offset_limited: bool,
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
    /// Frames a [`ir::ParamAddr::SampleStart`] route may move the start either way.
    pub start_mod: u64,
    /// Linear gain at full velocity.
    pub gain: f32,
    /// −1 (left) ..= 1 (right).
    pub pan: f32,
    /// Release triggers: dB lost per second the key was held.
    pub release_decay_db: f32,
    /// Ignores note-off and plays to the end (drums, one-shots).
    pub one_shot: bool,
    /// Per-zone sample-position envelopes read by [`ir::Source::ZoneEnvelope`].
    pub envelopes: Vec<ir::ZoneEnvelope>,
}

impl Zone {
    /// Sets the velocity and key crossfades from [`Self::fades`] at the
    /// current ranges, as Kontakt measures them: from `low` a fade `F`
    /// rises over (low − 1, low + F), to `high` it falls over
    /// (high − F, high + 1). The loader and a range edit both call it.
    pub fn range_crossfades(&mut self) {
        let ranged = [
            (Control::Velocity, &self.velocities, self.fades[0]),
            (Control::Key, &self.keys, self.fades[1]),
        ];
        let ranged: Vec<_> = (ranged.into_iter())
            .filter(|&(_, _, (fade_in, fade_out))| fade_in > 0 || fade_out > 0)
            .map(|(control, range, (fade_in, fade_out))| {
                let (low, high) = (i16::from(*range.start()), i16::from(*range.end()));
                Crossfade {
                    control,
                    fade_in: (fade_in > 0).then(|| (low - 1, low + i16::from(fade_in))),
                    fade_out: (fade_out > 0).then(|| (high - i16::from(fade_out), high + 1)),
                    law: ir::CrossfadeLaw::Linear,
                }
            })
            .collect();
        self.crossfades
            .retain(|x| matches!(x.control, Control::Cc(_)));
        self.crossfades.splice(0..0, ranged);
    }

    /// A plain attack zone over the full key and velocity range on bus 0.
    #[must_use]
    pub fn new(region: Region, root: u8, sample_rate: u32) -> Self {
        Self {
            keys: 0..=127,
            velocities: 1..=127,
            slot: 0,
            id: 0,
            start: Vec::new(),
            trigger: Trigger::Attack,
            crossfades: Vec::new(),
            fades: [(0, 0); 2],
            group: 0,
            member: 0,
            off_by: None,
            repeat_cuts: false,
            offset_limited: false,
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
            envelopes: Vec::new(),
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
    pub mods: Vec<ir::ModRoute>,
    pub blends: Vec<ir::Blend>,
    /// Read by `Source::Lfo(n)` in those routes ([`ir::Bus::lfos`]).
    pub lfos: Vec<ir::Lfo>,
}

pub use kontra_effects::Response;

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
    /// How a voice makes room at [`Self::voices`] (its steal, released
    /// first and fade; the instrument's).
    pub steal: ir::VoiceGroup,
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
    /// The fader and pan after every effect.
    pub output: ir::Output,
    pub route_pitch: ir::Exp2,
    /// [`ir::Instrument::steal_whole_groups`].
    pub steal_whole_groups: bool,
    /// [`ir::Instrument::recalled`].
    pub recalled: bool,
    /// Who starts the release-trigger zones; [`ReleaseZones::AtRelease`]
    /// (the engine, at a note's release) unless a script takes them over.
    pub release_zones: ReleaseZones,
    /// What the sustain pedal does; [`SustainPedal::Holds`] unless a script takes it over.
    pub sustain_pedal: SustainPedal,
    /// Convolution responses at the engine's rate, by [`ir::Instrument::impulses`]
    /// index; `None` when unreadable.
    pub impulses: Vec<Option<Response>>,
    /// Each of [`Self::impulses`]' file (as [`Self::responses`] names it)
    /// and how it was prepared: a script loading another file into its
    /// slot keeps that.
    pub impulse_laws: Vec<(String, ir::Impulse)>,
    /// Response files a script may load by name ([`ir::Instrument::responses`]).
    pub responses: Vec<(String, ir::Sample)>,
    /// How [`ir::KnobTarget::Slot`] knobs move the effects they address.
    pub slot_laws: Vec<ir::SlotLaw>,
    /// Slots whose knobs [`ir::KnobModel`]s rebuild together.
    pub slot_models: Vec<(ir::RackAt, u8, ir::KnobModel)>,
    /// The filter shapes [`EnginePar::EffectSubtype`] switches slots to.
    pub slot_shapes: ir::SlotShapes,
    /// Controllers that move foreign scripts' controls.
    pub knob_mods: Vec<KnobModPlan>,
    /// What [`KnobModPlan::source`] reads ([`ir::Instrument::knob_blends`]).
    pub knob_blends: Vec<ir::Blend>,
    pub knob_lfos: Vec<ir::Lfo>,
    /// Slots loaded with a [`ir::SlotShapes::types`] entry ([`Plan::load_type`]).
    pub loaded: Vec<(ir::RackAt, u8, u16)>,
    /// Compact SoA table of every saved zone (active, purged, or oscillator)
    /// read by scripts (`find_zone`, `get_zone_par`, `get_loop_par`) with zero
    /// per-zone heap allocations and O(1) clone via `Arc`.
    pub zone_table: std::sync::Arc<soa::ZoneTable>,
    /// Sparse script-created user zones (`set_num_user_zones` / `set_sample` /
    /// `set_loop_par`); empty (0 bytes) for instruments without user zones.
    pub user_zones: Vec<UserZone>,
    /// Saved-purged groups, nothing of them loaded: what a script's
    /// `purge_group(g, 1)` builds ([`Dormant`]).
    pub dormant: std::sync::Arc<Vec<Dormant>>,
    /// The sample each user zone's playing [`Zone`] was built from; a
    /// zone whose [`UserZone::source`] differs is built again
    /// (`kontra_prepare::user_zones`).
    pub user_loaded: Vec<(u32, std::sync::Arc<ir::Sample>)>,
    /// Optional dynamic multi-channel audio routing DAG ([`ir::Instrument::graph`]).
    pub graph: Option<ir::AudioGraph>,
}

/// A purged group as saved: its zones and the samples they play, kept as the
/// instrument described them (sample indices local to `samples`) until the
/// host loads them, off the audio thread, and appends the zones to the plan.
#[derive(Clone, Debug)]
pub struct Dormant {
    /// Its index in [`Plan::groups`].
    pub group: u32,
    pub source: ir::Group,
    pub samples: Vec<ir::Sample>,
}

impl Plan {
    /// Note-table entries for a voice limit: a held or stolen note keeps its
    /// entry, so at least one per voice, 128 at least. The queue holds four per note.
    // ponytail: capped at 4096 (per-note group masks); raise if a library plays more.
    fn notes_for(voices: usize) -> usize {
        voices.clamp(128, 4096)
    }

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
                lfos: Vec::new(),
            }],
            polyphony: Vec::new(),
            voice_groups: Vec::new(),
            steal: ir::VoiceGroup::DEFAULT,
            switches: Vec::new(),
            default_keyswitch: None,
            voices,
            notes: Self::notes_for(voices),
            // Room for every task a script may park ([`kontra_vm::TASKS`]).
            queue: (4 * Self::notes_for(voices)).max(1024) + kontra_vm::TASKS,
            scripts: Vec::new(),
            inserts: Vec::new(),
            sends: Vec::new(),
            main: Vec::new(),
            output: ir::Output::UNITY,
            route_pitch: ir::Exp2::Exact,
            steal_whole_groups: false,
            recalled: false,
            release_zones: ReleaseZones::AtRelease,
            sustain_pedal: SustainPedal::Holds,
            impulses: Vec::new(),
            impulse_laws: Vec::new(),
            responses: Vec::new(),
            slot_laws: Vec::new(),
            slot_models: Vec::new(),
            slot_shapes: ir::SlotShapes::NONE,
            knob_mods: Vec::new(),
            knob_blends: Vec::new(),
            knob_lfos: Vec::new(),
            loaded: Vec::new(),
            zone_table: std::sync::Arc::default(),
            user_zones: Vec::new(),
            dormant: std::sync::Arc::default(),
            user_loaded: Vec::new(),
            graph: None,
        }
    }
}

/// Whether scripts address `m` by modulator `slot` and `target`. A negative
/// target is the first route (measured, probes/mod_par_laws.lua: generic
/// -1 moves route 0 alone).
pub fn addressed(m: &ir::Mod, slot: i32, target: i32) -> bool {
    i32::from(m.slot) == slot && i32::from(m.target) == target.max(0)
}

#[cfg(test)]
mod route_dependency_tests {
    use super::*;
    #[test]
    fn timed_source_dependencies_include_intensity_and_lfo_rate() {
        let mut group = Group::standard();
        group.mods = vec![
            ir::Mod {
                name: "intensity".into(),
                slot: 1,
                target: 0,
                bypass: false,
                route: ir::ModRoute::new(
                    ir::Source::Controller(1),
                    ir::ParamAddr::RouteIntensity {
                        of: ir::Source::Lfo(0),
                    },
                    1.0,
                ),
            },
            ir::Mod {
                name: "rate".into(),
                slot: 2,
                target: 0,
                bypass: false,
                route: ir::ModRoute::new(
                    ir::Source::Controller(11),
                    ir::ParamAddr::LfoParam {
                        lfo: 0,
                        param: ir::LfoParam::Hz,
                    },
                    1.0,
                )
                .law(ir::RouteLaw::Range([0.0, 4.0])),
            },
        ];
        assert!(group.reads_modulated(ir::Source::Lfo(0), 1));
        assert!(group.reads_modulated(ir::Source::Lfo(0), 11));
        assert!(!group.reads_modulated(ir::Source::Lfo(1), 1));
        assert!(!group.reads_modulated(ir::Source::Controller(2), 1));
        group.mods[0].bypass = true;
        assert!(!group.reads_modulated(ir::Source::Lfo(0), 1));
    }

    #[test]
    fn crossfade_at_key_zero_uses_low_minus_one_bound() {
        let xf = Crossfade {
            control: Control::Key,
            fade_in: Some((-1, 3)),
            fade_out: None,
            law: ir::CrossfadeLaw::Linear,
        };
        assert!(
            (xf.gain(0) - 0.25).abs() < 1e-6,
            "key 0 gain should be 1/(F+1)=0.25, got {}",
            xf.gain(0)
        );
        assert!((xf.gain(3) - 1.0).abs() < 1e-6);
        let xf_hi = Crossfade {
            control: Control::Key,
            fade_in: None,
            fade_out: Some((124, 128)),
            law: ir::CrossfadeLaw::Linear,
        };
        assert!(
            (xf_hi.gain(127) - 0.25).abs() < 1e-6,
            "key 127 fade-out gain should be 1/(F+1)=0.25, got {}",
            xf_hi.gain(127)
        );
    }

    #[test]
    fn equal_power_crossfade_preserves_unity_power_at_midpoint() {
        let fade_in = Crossfade {
            control: Control::Cc(1),
            fade_in: Some((40, 80)),
            fade_out: None,
            law: ir::CrossfadeLaw::EqualPower,
        };
        let fade_out = Crossfade {
            control: Control::Cc(1),
            fade_in: None,
            fade_out: Some((40, 80)),
            law: ir::CrossfadeLaw::EqualPower,
        };
        let g_in = fade_in.gain(60);
        let g_out = fade_out.gain(60);
        assert!((g_in * g_in + g_out * g_out - 1.0).abs() < 1e-6);
        assert!((g_in - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
        assert_eq!(fade_in.gain(40), 0.0);
        assert!((fade_in.gain(80) - 1.0).abs() < 1e-6);
        assert!((fade_out.gain(40) - 1.0).abs() < 1e-6);
        assert!(fade_out.gain(80).abs() < 1e-6);
    }
}
