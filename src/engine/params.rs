//! Script-controllable engine state: per-group modulation tables and the KSP
//! engine parameters (`set_engine_par`) mapped onto groups, the instrument,
//! buses and effects.
//!
//! Mappings from Kontakt's normalized 0..=1000000 values and the modulation
//! semantics, with their confidence, are in `audits/MODULATION.md`
//! ("Runtime modulation and engine parameters").

use super::{Ahdsr, GroupSettings, filter::Knob, voice::Envelope};
use crate::fx::{DIRECT, FxParam, OUTS, Rack};
use crate::import::{Group, ModAssignment, ModSource, ModTarget};
use crate::ksp::{ENGINE_PAR_BASE, EnginePar};
use std::sync::Arc;

/// Modulation values one voice tracks: its group's first volume and pitch
/// assignments with a modelled source.
pub const VOICE_MODS: usize = 8;

pub(crate) use crate::modulation::PITCH_ENVS;

#[derive(Clone, Debug, PartialEq)]
pub struct PitchEnvelope {
    pub env: Ahdsr,
    pub bypass: bool,
    /// Original Group::envelopes index, preserving script addresses.
    pub index: u8,
    /// Original target index, direction and prepared shaper/depth.
    pub targets: Box<[(u16, f32, Mod)]>,
}

impl PitchEnvelope {
    pub(crate) fn from_group(group: &Group) -> Box<[Self]> {
        group
            .envelopes
            .iter()
            .enumerate()
            .filter_map(|(index, e)| {
                let targets: Box<[_]> = e
                    .targets
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| m.target == ModTarget::Pitch)
                    .map(|(i, m)| (i as u16, if m.invert { -1. } else { 1. }, Mod::from(m)))
                    .collect();
                (!targets.is_empty()).then(|| Self {
                    env: Ahdsr::from(&e.env),
                    bypass: group.modulators.iter().find(|m| m.envelope == Some(index))
                        .is_some_and(|m| m.bypassed),
                    index: index as u8,
                    targets,
                })
            })
            .collect()
    }

    /// Existing voice control rate, with the same AHDSR as amplitude/filter modulation.
    pub(crate) fn pitch(&self, state: &mut Envelope, frames: usize, rate: f32) -> f32 {
        state.skip(frames, None, rate);
        if self.bypass { return 0.; }
        self.targets
            .iter()
            .map(|(_, sign, m)| 12. * sign * m.intensity * m.shape(state.level()))
            .sum()
    }
}

/// Modulation source the engine models.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    Velocity,
    Key,
    Constant,
    Cc(u8),
    Bend,
    Pressure,
    /// Release-trigger counter, fixed when the release voice starts.
    Counter,
}

impl Source {
    fn of(source: ModSource) -> Option<Self> {
        Some(match source {
            ModSource::Velocity => Self::Velocity,
            ModSource::KeyPosition => Self::Key,
            ModSource::Constant => Self::Constant,
            ModSource::MidiCc(cc) if cc < 128 => Self::Cc(cc),
            ModSource::PitchBend => Self::Bend,
            ModSource::MonoAftertouch => Self::Pressure,
            ModSource::ReleaseTriggerCounter => Self::Counter,
            _ => return None,
        })
    }

    /// Changes while a voice plays (otherwise fixed at the note start).
    fn live(self) -> bool {
        matches!(self, Self::Cc(_) | Self::Bend | Self::Pressure)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Volume,
    Pitch,
    Start,
    /// Volume AHDSR attack or release time, fixed at note start.
    Attack,
    Release,
    /// A group filter or EQ knob (see `filter.rs`).
    Fx,
    LoopStart,
    LoopLength,
}

/// One external modulation assignment prepared for playback.
#[derive(Clone, Debug, PartialEq)]
pub struct Mod {
    /// `None` when playback does not model the source or target; the
    /// intensity is still kept for scripts.
    route: Option<(Source, Target)>,
    /// Depth, -1..=1; negative inverts. Scripts change it.
    pub intensity: f32,
    /// Lag time constant in seconds.
    lag: f32,
    /// Shaper sampled at the 128 MIDI steps; `None` is the identity.
    /// Shared between equal shapers by [`share_curves`].
    curve: Option<Arc<[f32; 128]>>,
}

impl From<&ModAssignment> for Mod {
    fn from(m: &ModAssignment) -> Self {
        let target = match m.target {
            ModTarget::Volume => Some(Target::Volume),
            ModTarget::Pitch => Some(Target::Pitch),
            ModTarget::SampleStart => Some(Target::Start),
            ModTarget::Attack => Some(Target::Attack),
            ModTarget::Release => Some(Target::Release),
            ModTarget::Module { .. } => Some(Target::Fx),
            ModTarget::Group(ref p) if m.source == ModSource::Constant && m.lag_ms == 0 && m.shaper.is_none() && !m.invert => match p.as_str() {
                "loopStart" => Some(Target::LoopStart),
                "loopLength" => Some(Target::LoopLength),
                _ => None,
            },
            ModTarget::Group(_) => None,
        };
        Self {
            route: Source::of(m.source).zip(target),
            intensity: m.intensity,
            lag: f32::from(m.lag_ms) / 1000.0,
            curve: m
                .shaper
                .as_ref()
                .map(|_| Arc::new(std::array::from_fn(|i| m.shape(i as f32 / 127.0)))),
        }
    }
}

impl Mod {
    /// Shaped source value; exact at MIDI steps, linear between them.
    pub(crate) fn shape(&self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        let Some(curve) = &self.curve else {
            return x;
        };
        let p = x * 127.0;
        let i = (p as usize).min(126);
        curve[i] + (curve[i + 1] - curve[i]) * (p - i as f32)
    }
}

impl Mod {
    /// Shaped value at note start; 0 for sources playback does not model.
    pub(crate) fn start_value(&self, input: &Inputs) -> f32 {
        self.route.map_or(0.0, |(source, _)| self.shape(input.read(source)))
    }

    /// Advance a live source's lagged `value` over `frames`.
    pub(crate) fn follow(&self, value: &mut f32, input: &Inputs, frames: usize, rate: f32) {
        if let Some((source, _)) = self.route.filter(|(s, _)| s.live()) {
            approach(
                value,
                self.shape(input.read(source)),
                self.lag,
                frames,
                rate,
            );
        }
    }
}

/// Group modulation assignments in import order (the KSP target addresses),
/// plus which of them a voice evaluates.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModTable {
    pub mods: Box<[Mod]>,
    /// Volume and pitch assignments with a modelled source, at most [`VOICE_MODS`].
    voiced: Box<[u16]>,
    /// Sample-start assignments with a modelled source.
    starts: Box<[u16]>,
    /// Envelope-time assignments with a modelled source.
    times: Box<[u16]>,
    /// Constant loop-boundary offsets, read at each native 32-frame control step.
    loops: Box<[u16]>,
}

impl From<&Group> for ModTable {
    fn from(group: &Group) -> Self {
        let mods: Box<[Mod]> = group.mods.iter().map(|m| {
            let mut prepared = Mod::from(m);
            if matches!(prepared.route, Some((_, Target::LoopStart | Target::LoopLength)))
                && !loop_control_supported(group, m) { prepared.route = None; }
            prepared
        }).collect();
        let routed = |want: fn(Target) -> bool| {
            (0..mods.len() as u16)
                .filter(|&i| mods[i as usize].route.is_some_and(|(_, t)| want(t)))
                .collect::<Vec<_>>()
        };
        let mut voiced = routed(|t| matches!(t, Target::Volume | Target::Pitch));
        // ponytail: extra assignments are ignored; no local group has more than 8.
        voiced.truncate(VOICE_MODS);
        Self {
            voiced: voiced.into(),
            starts: routed(|t| t == Target::Start).into(),
            times: routed(|t| matches!(t, Target::Attack | Target::Release)).into(),
            loops: routed(|t| matches!(t, Target::LoopStart | Target::LoopLength)).into(),
            mods,
        }
    }
}

/// Performance state a voice's modulation reads.
#[derive(Clone, Copy)]
pub(crate) struct Inputs<'a> {
    pub cc: &'a [u8; 128],
    /// Combined MPE CC74, preserving a released note's member value.
    pub cc74: Option<u8>,
    /// -1..=1.
    pub bend: f32,
    pub pressure: u8,
    pub note: u8,
    pub velocity: u8,
    /// Release-trigger counter, 0..=1 (see [`release_counter`]).
    pub counter: f32,
}

/// Release-trigger counter as a source value: Kontakt counts down from the
/// group's `T` (ms) while the key is held and stops at the key release, so a
/// short note reads near 1 and a note held `T` or longer reads 0. A group
/// without a counter (`T` = 0) reads 0.
pub(crate) fn release_counter(t_ms: i32, held_ms: f32) -> f32 {
    if t_ms <= 0 {
        return 0.0;
    }
    let t = t_ms as f32;
    ((t - held_ms) / t).clamp(0.0, 1.0)
}

impl Inputs<'_> {
    /// Negotiated MPE master pitch is added independently by the voice;
    /// other bend destinations still read the combined controller normally.
    fn read_mod(&self, source: Source, target: Target, bend_pitch: Option<f32>) -> f32 {
        if source == Source::Bend && target == Target::Pitch {
            if let Some(bend) = bend_pitch { return (bend + 1.) * 0.5; }
        }
        self.read(source)
    }

    /// Unshaped source value, 0..=1.
    fn read(&self, source: Source) -> f32 {
        match source {
            Source::Velocity => f32::from(self.velocity) / 127.0,
            Source::Key => f32::from(self.note) / 127.0,
            Source::Constant => 1.0,
            Source::Cc(cc) => {
                f32::from(if cc == 74 {
                    self.cc74.unwrap_or(self.cc[74])
                } else {
                    self.cc[cc as usize]
                }) / 127.0
            }
            Source::Bend => (self.bend + 1.0) * 0.5,
            Source::Pressure => f32::from(self.pressure) / 127.0,
            Source::Counter => self.counter,
        }
    }
}

/// Native loop consumer is inactive when the sample has a preload-byte budget.
/// Only the independently traced zero-budget mode and Constant source are enabled.
pub(crate) fn loop_control_supported(group: &Group, m: &ModAssignment) -> bool {
    group.source_mode == Some(0) && m.source == ModSource::Constant
        && m.lag_ms == 0 && m.shaper.is_none() && !m.invert
        && matches!(&m.target, ModTarget::Group(p) if p == "loopStart" || p == "loopLength")
}

impl ModTable {
    pub(crate) fn has_loop_controls(&self) -> bool { !self.loops.is_empty() }

    pub(crate) fn loop_offsets(&self) -> [f32; 2] {
        let mut offsets = [0.; 2];
        for &i in &self.loops {
            let m = &self.mods[i as usize];
            match m.route {
                Some((_, Target::LoopStart)) => offsets[0] += m.intensity,
                Some((_, Target::LoopLength)) => offsets[1] += m.intensity,
                _ => {}
            }
        }
        offsets
    }

    /// Initial per-voice values: every source at its current value, unlagged.
    pub(crate) fn start(&self, input: &Inputs, bend_pitch: Option<f32>) -> [f32; VOICE_MODS] {
        let mut values = [0.0; VOICE_MODS];
        for (value, &i) in values.iter_mut().zip(&self.voiced) {
            let m = &self.mods[i as usize];
            if let Some((source, target)) = m.route {
                *value = m.shape(input.read_mod(source, target, bend_pitch));
            }
        }
        values
    }

    /// Advance live sources over `frames` at `rate` and return the volume
    /// factor, the pitch offset in semitones, and whether every live value
    /// has reached its source: settled, the same inputs give the same
    /// result again whatever the frames.
    ///
    /// Volume: each assignment scales amplitude by `1 - |i|·(1 - v)` for
    /// shaped value `v` (inverted, `1 - v`, when `i < 0`). Pitch: `12·i·v`
    /// semitones, with pitch bend mapped back to -1..=1. The flag is true
    /// when every live source has settled on its input: until the inputs
    /// or the table change, another call returns the same.
    pub(crate) fn modulate(
        &self,
        values: &mut [f32; VOICE_MODS],
        input: &Inputs,
        frames: usize,
        rate: f32,
        bend_pitch: Option<f32>,
    ) -> (f32, f32, bool) {
        let (mut gain, mut semitones, mut settled) = (1.0, 0.0, true);
        for (value, &i) in values.iter_mut().zip(&self.voiced) {
            let m = &self.mods[i as usize];
            let Some((source, target)) = m.route else {
                continue;
            };
            if source.live() {
                let x = m.shape(input.read_mod(source, target, bend_pitch));
                approach(value, x, m.lag, frames, rate);
                settled &= *value == x;
            }
            match target {
                Target::Volume => {
                    let v = if m.intensity < 0.0 {
                        1.0 - *value
                    } else {
                        *value
                    };
                    gain *= 1.0 - m.intensity.abs() * (1.0 - v);
                }
                Target::Pitch => {
                    let v = if source == Source::Bend {
                        *value * 2.0 - 1.0
                    } else {
                        *value
                    };
                    semitones += 12.0 * m.intensity * v;
                }
                Target::Start | Target::Attack | Target::Release | Target::Fx | Target::LoopStart | Target::LoopLength => {}
            }
        }
        (gain.max(0.0), semitones, settled)
    }

    /// Sample-start offset as a fraction of the zone's start-mod range.
    pub(crate) fn start_offset(&self, input: &Inputs) -> f32 {
        self.starts
            .iter()
            .map(|&i| &self.mods[i as usize])
            .filter_map(|m| Some(m.intensity.abs() * m.shape(input.read(m.route?.0))))
            .sum::<f32>()
            .min(1.0)
    }

    /// Lowest and highest [`ModTable::start_offset`] of notes in `keys` at
    /// `velocities` with controllers held at `cc`, bend centred and no
    /// pressure: where a zone's voices start until a controller moves.
    pub(crate) fn start_offset_range(
        &self,
        cc: &[u8; 128],
        keys: std::ops::RangeInclusive<u8>,
        velocities: std::ops::RangeInclusive<u8>,
    ) -> (f32, f32) {
        let reads = |source| {
            self.starts.iter().any(|&i| {
                self.mods[i as usize]
                    .route
                    .is_some_and(|(s, _)| s == source)
            })
        };
        // Only sources some start modulation reads need sweeping.
        let one = |r: std::ops::RangeInclusive<u8>, source| {
            if reads(source) {
                r
            } else {
                *r.start()..=*r.start()
            }
        };
        let velocities = one(velocities, Source::Velocity);
        let counters = one(0..=127, Source::Counter);
        let (mut low, mut high) = (f32::MAX, f32::MIN);
        for note in one(keys, Source::Key) {
            for velocity in velocities.clone() {
                for counter in counters.clone() {
                    let counter = f32::from(counter) / 127.0;
                    let input = Inputs {
                        cc74: None,
                        cc,
                        bend: 0.0,
                        pressure: 0,
                        note,
                        velocity,
                        counter,
                    };
                    let x = self.start_offset(&input);
                    (low, high) = (low.min(x), high.max(x));
                }
            }
        }
        (low, high)
    }

    /// Scale the volume AHDSR's attack and release by their note-start
    /// modulation, with the volume law: `1 - |i|·(1 - v)`. Stored shapers
    /// (velocity 0 → 1, 127 → 0.59 on attack) read as time factors.
    pub(crate) fn scale_envelope(&self, env: &mut Ahdsr, input: &Inputs) {
        for m in self.times.iter().map(|&i| &self.mods[i as usize]) {
            let Some((source, target)) = m.route else {
                continue;
            };
            let v = m.shape(input.read(source));
            let v = if m.intensity < 0.0 { 1.0 - v } else { v };
            let factor = (1.0 - m.intensity.abs() * (1.0 - v)).max(0.0);
            match target {
                Target::Attack => env.attack *= factor,
                _ => env.release *= factor,
            }
        }
    }
}

/// Point equal shaper curves at one copy. Groups mostly repeat the same
/// assignments, and voices spread over hundreds of groups otherwise read as
/// many copies every block, each a cache miss (Mega Brass: 1128 curves, 33
/// distinct).
pub(crate) fn share_curves<'a>(tables: impl Iterator<Item = &'a mut ModTable>) {
    let mut seen = std::collections::HashMap::new();
    for curve in tables
        .flat_map(|t| t.mods.iter_mut())
        .filter_map(|m| m.curve.as_mut())
    {
        *curve = Arc::clone(
            seen.entry(curve.map(f32::to_bits))
                .or_insert_with(|| Arc::clone(curve)),
        );
    }
}

/// Move a lagged `value` toward `x` over `frames`. Settled (as a held
/// controller soon is) it costs no exp: within 1e-6 it lands on `x`, which
/// steps smaller than half a float's spacing would never reach.
fn approach(value: &mut f32, x: f32, lag: f32, frames: usize, rate: f32) {
    if (x - *value).abs() <= 1e-6 {
        *value = x;
    } else {
        *value += (x - *value) * lag_factor(lag, frames, rate);
    }
}

/// One-pole smoothing step over `frames`: the share of the distance covered.
fn lag_factor(lag: f32, frames: usize, rate: f32) -> f32 {
    if lag <= 0.0 {
        1.0
    } else {
        1.0 - (-(frames as f32) / (lag * rate)).exp()
    }
}

// ---- Engine parameters ------------------------------------------------------------

/// `$ENGINE_PAR_*` ids, as positions in the runtime's engine parameter table.
pub mod id {
    use super::ENGINE_PAR_BASE as B;
    pub const VOLUME: i32 = B;
    pub const PAN: i32 = B + 1;
    pub const TUNE: i32 = B + 2;
    pub const OUTPUT_CHANNEL: i32 = B + 3;
    pub const CUTOFF: i32 = B + 4;
    pub const RESONANCE: i32 = B + 5;
    pub const ATTACK: i32 = B + 6;
    pub const DECAY: i32 = B + 7;
    pub const SUSTAIN: i32 = B + 8;
    pub const RELEASE: i32 = B + 9;
    pub const HOLD: i32 = B + 10;
    pub const ATK_CURVE: i32 = B + 11;
    // Appended runtime symbol index; existing serialized parameter IDs stay stable.
    pub const ENV_AHD: i32 = B + 183;
    pub const MOD_TARGET_INTENSITY: i32 = B + 16;
    pub const MOD_TARGET_MP_INTENSITY: i32 = B + 17;
    pub const INTMOD_INTENSITY: i32 = B + 18;
    pub const INTMOD_BYPASS: i32 = B + 19;
    pub const EFFECT_BYPASS: i32 = B + 22;
    pub const EFFECT_TYPE: i32 = B + 23;
    pub const EFFECT_SUBTYPE: i32 = B + 24;
    pub const SEND_EFFECT_TYPE: i32 = B + 25;
    pub const SEND_EFFECT_BYPASS: i32 = B + 26;
    pub const SEND_EFFECT_DRY_LEVEL: i32 = B + 27;
    pub const SEND_EFFECT_OUTPUT_GAIN: i32 = B + 28;
    pub const INSERT_EFFECT_OUTPUT_GAIN: i32 = B + 29;
    pub const SENDLEVEL_0: i32 = B + 30;
    pub const SENDLEVEL_7: i32 = B + 37;
    pub const RV2_PREDELAY: i32 = B + 101;
    pub const RV2_TIME: i32 = B + 102;
    pub const RV2_TYPE: i32 = B + 103;
    pub const RV2_SIZE: i32 = B + 104;
    pub const RV2_DAMPING: i32 = B + 105;
    pub const RV2_DIFF: i32 = B + 106;
    pub const RV2_MOD: i32 = B + 107;
    pub const RV2_STEREO: i32 = B + 108;
    pub const RV2_FREEZE: i32 = B + 109;
    #[cfg(test)]
    pub const RV2_EQ_LOW_FREQ: i32 = B + 110;
    pub const RV2_EQ_LOW_GAIN: i32 = B + 111;
    #[cfg(test)]
    pub const RV2_EQ_HIGH_FREQ: i32 = B + 112;
    pub const RV2_EQ_HIGH_GAIN: i32 = B + 113;
    pub const STEREO: i32 = B + 138;
    pub const STEREO_PAN: i32 = B + 139;
    pub const FREQ1: i32 = B + 157;
    pub const FREQ3: i32 = B + 159;
    pub const BW1: i32 = B + 160;
    pub const BW3: i32 = B + 162;
    pub const GAIN1: i32 = B + 163;
    pub const GAIN3: i32 = B + 165;
}

/// KSP `$NI_BUS_OFFSET`: generic values from here address instrument buses.
const BUS_OFFSET: i32 = 1000;
/// Instrument buses.
const BUSES: u8 = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum GroupPar {
    Volume,
    Pan,
    Tune,
    Output,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum WavePar { Position, Phase, Form, Form2, FormMode, Form2Mode }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Stage {
    Attack,
    /// Attack curve, -1..=1.
    Curve,
    Hold,
    Decay,
    Sustain,
    Release,
    AhdOnly,
}

/// Independently corroborated signed depth laws; only pitch allows >12 st.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum CubicDepth { Pitch, Cutoff, Loop }

impl CubicDepth {
    fn target(target: &ModTarget) -> Option<Self> {
        match target {
            ModTarget::Pitch => Some(Self::Pitch),
            ModTarget::Group(p) if p == "loopStart" || p == "loopLength" => Some(Self::Loop),
            ModTarget::Module { param, .. } if param == "filterCutoff" => Some(Self::Cutoff),
            _ => None,
        }
    }
}

/// A modelled engine parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Address {
    Group(u16, GroupPar),
    Wavetable(u16, WavePar),
    /// Volume, pan or tune of the whole instrument.
    Instrument(GroupPar),
    /// Volume envelope of a group.
    Envelope(u16, Stage),
    /// A group's module envelope (filter, EQ), by `Group::envelopes` index.
    ModEnvelope(u16, u8, Stage),
    /// A modulation assignment's intensity; `bipolar` for either signed alias.
    Intensity {
        group: u16,
        index: u16,
        bipolar: bool,
        /// Verified pitch or cutoff cubic law, separate from other MP targets.
        cubic: Option<CubicDepth>,
    },
    /// An internal pitch/filter/EQ-envelope depth, in original target order.
    InternalIntensity {
        group: u16,
        envelope: u8,
        target: u16,
        bipolar: bool,
        /// Verified pitch or cutoff cubic law, separate from other MP targets.
        cubic: Option<CubicDepth>,
    },
    /// An admitted pitch LFO target; source and target use native slot indices.
    PitchLfoIntensity { group: u16, slot: u8, target: u32, bipolar: bool },
    /// Explicit KSP bypass, separate from the undecoded preset flags.
    InternalBypass(u16, u8),
    Fx(Rack, u8, FxParam),
    /// A group insert slot's filter/EQ knob (normalized), bypass, output
    /// gain or Stereo Modeller setting.
    Filter(u16, u8, Knob),
    /// The `$EFFECT_TYPE_*` of a group insert slot; read only.
    GroupType(u16, u8),
}

/// The effect rack a KSP `generic` argument names.
pub(crate) fn rack(generic: i32) -> Option<Rack> {
    Some(match generic {
        0 => Rack::Send,
        1 => Rack::Insert,
        2 => Rack::Main,
        g => Rack::Bus(u8::try_from(g - BUS_OFFSET).ok().filter(|&b| b < BUSES)?),
    })
}

impl Address {
    /// Map a KSP address onto the engine, or `None` when it is not modelled.
    pub(crate) fn resolve(par: EnginePar, groups: &[Group]) -> Option<Self> {
        let group = || {
            u16::try_from(par.group)
                .ok()
                .filter(|&g| (g as usize) < groups.len())
        };
        let wave = || {
            let g = group()?;
            let source = groups[g as usize].wavetable.as_ref()?;
            super::wavetable::supported(source, groups[g as usize].key_tracking).then_some(g)
        };
        let modulator = |g: u16| {
            let m = usize::try_from(par.slot).ok()?;
            groups[g as usize].modulators.get(m)
        };
        let rack = || {
            (par.group == -1).then_some(())?;
            rack(par.generic)
        };
        let fx = |param| Some(Self::Fx(rack()?, u8::try_from(par.slot).ok()?, param));
        let slot = |knob| Some(Self::Filter(group()?, u8::try_from(par.slot).ok()?, knob));
        // Group inserts are addressed by group; the racks by `group == -1`.
        let insert = |knob, param| {
            if par.group >= 0 {
                slot(knob)
            } else {
                fx(param)
            }
        };
        let filter = |knob| insert(knob, FxParam::Filter(knob));
        Some(match par.id {
            _ if crate::ksp::engine_par_name(par.id) == Some("$ENGINE_PAR_WT_POSITION") => Self::Wavetable(wave()?, WavePar::Position),
            _ if crate::ksp::engine_par_name(par.id) == Some("$ENGINE_PAR_WT_PHASE") => Self::Wavetable(wave()?, WavePar::Phase),
            _ if crate::ksp::engine_par_name(par.id) == Some("$ENGINE_PAR_WT_FORM") => Self::Wavetable(wave()?, WavePar::Form),
            _ if crate::ksp::engine_par_name(par.id) == Some("$ENGINE_PAR_WT_FORM2") => Self::Wavetable(wave()?, WavePar::Form2),
            _ if crate::ksp::engine_par_name(par.id) == Some("$ENGINE_PAR_WT_FORM_MODE") => Self::Wavetable(wave()?, WavePar::FormMode),
            _ if crate::ksp::engine_par_name(par.id) == Some("$ENGINE_PAR_WT_FORM2_MODE") => Self::Wavetable(wave()?, WavePar::Form2Mode),
            id::VOLUME | id::PAN | id::TUNE | id::OUTPUT_CHANNEL => {
                let p = match par.id {
                    id::VOLUME => GroupPar::Volume,
                    id::PAN => GroupPar::Pan,
                    id::TUNE => GroupPar::Tune,
                    _ => GroupPar::Output,
                };
                let bus = u8::try_from(par.generic - BUS_OFFSET)
                    .ok()
                    .filter(|&b| b < BUSES);
                match (par.group, bus, p) {
                    (0.., _, _) => Self::Group(group()?, p),
                    (_, Some(b), GroupPar::Volume) => Self::Fx(Rack::Bus(b), 0, FxParam::Volume),
                    (_, Some(b), GroupPar::Pan) => Self::Fx(Rack::Bus(b), 0, FxParam::Pan),
                    (_, Some(b), GroupPar::Output) => Self::Fx(Rack::Bus(b), 0, FxParam::Output),
                    (_, None, GroupPar::Volume | GroupPar::Pan | GroupPar::Tune) => {
                        Self::Instrument(p)
                    }
                    _ => return None,
                }
            }
            id::ATTACK | id::ATK_CURVE | id::DECAY | id::SUSTAIN | id::RELEASE | id::HOLD | id::ENV_AHD => {
                let g = group()?;
                let m = modulator(g)?;
                let stage = match par.id {
                    id::ATTACK => Stage::Attack,
                    id::ATK_CURVE => Stage::Curve,
                    id::HOLD => Stage::Hold,
                    id::DECAY => Stage::Decay,
                    id::SUSTAIN => Stage::Sustain,
                    id::ENV_AHD => Stage::AhdOnly,
                    _ => Stage::Release,
                };
                match (m.volume_env, m.envelope) {
                    (true, _) => Self::Envelope(g, stage),
                    (_, Some(e)) => Self::ModEnvelope(g, u8::try_from(e).ok()?, stage),
                    _ => return None,
                }
            }
            id::INTMOD_BYPASS => {
                let g = group()?;
                let e = modulator(g)?.envelope?;
                groups[g as usize].envelopes.get(e)?.targets.iter()
                    .any(|t| match &t.target {
                        ModTarget::Pitch => true,
                        ModTarget::Module { param, .. } => Knob::parse(param).is_some() || super::filter::stage_knob(param).is_some(),
                        _ => false,
                    }).then_some(())?;
                Self::InternalBypass(g, u8::try_from(e).ok()?)
            }
            id::MOD_TARGET_INTENSITY | id::MOD_TARGET_MP_INTENSITY | id::INTMOD_INTENSITY => {
                let g = group()?;
                let m = modulator(g)?;
                let target = usize::try_from(par.generic).unwrap_or(0);
                (target < m.targets.len()).then_some(())?;
                // Kontakt registers both signed names as parameter 449. Restrict
                // newly accepted legacy targets to the independently proved laws.
                let bipolar = par.id != id::MOD_TARGET_INTENSITY;
                match m.assignments {
                    Some(index) => {
                        let cubic = CubicDepth::target(&groups[g as usize].mods.get(index + target)?.target);
                        if par.id == id::INTMOD_INTENSITY && cubic.is_none() { return None; }
                        Self::Intensity {
                            group: g,
                            index: u16::try_from(index + target).ok()?,
                            bipolar,
                            cubic: if bipolar { cubic } else { None },
                        }
                    },
                    None => {
                        if m.kind == "lfo" {
                            // Prepared LFO playback currently exists only on
                            // ordinary sampler groups, not wavetable sources.
                            groups[g as usize].wavetable.is_none().then_some(())?;
                            let slot = u8::try_from(par.slot).ok()?;
                            let target = u32::try_from(target).ok()?;
                            groups[g as usize].pitch_lfos.iter().find(|l| l.slot == slot)?
                                .targets.iter().find(|t| t.0 == target)?;
                            return Some(Self::PitchLfoIntensity { group: g, slot, target, bipolar });
                        }
                        let envelope = m.envelope?;
                        let routed = &groups[g as usize]
                            .envelopes
                            .get(envelope)?
                            .targets
                            .get(target)?.target;
                        match routed {
                            ModTarget::Pitch => {},
                            ModTarget::Module { param, .. } if param == "filterCutoff"
                                || (par.id != id::INTMOD_INTENSITY
                                    && (Knob::parse(param).is_some() || super::filter::stage_knob(param).is_some())) => {},
                            _ => return None,
                        }
                        let envelope = u8::try_from(envelope).ok()?;
                        let target = u16::try_from(target).ok()?;
                        Self::InternalIntensity { group: g, envelope, target, bipolar, cubic: if bipolar { CubicDepth::target(routed) } else { None } }
                    }
                }
            }
            id::CUTOFF => filter(Knob::Cutoff)?,
            id::RESONANCE => filter(Knob::Resonance)?,
            id::FREQ1..=id::FREQ3 => filter(Knob::Freq((par.id - id::FREQ1) as u8))?,
            id::BW1..=id::BW3 => filter(Knob::Bandwidth((par.id - id::BW1) as u8))?,
            id::GAIN1..=id::GAIN3 => filter(Knob::Gain((par.id - id::GAIN1) as u8))?,
            id::STEREO => filter(Knob::Spread)?,
            id::STEREO_PAN => filter(Knob::Pan)?,
            id::EFFECT_SUBTYPE => filter(Knob::Type)?,
            id::EFFECT_BYPASS => insert(Knob::Bypass, FxParam::Bypass)?,
            id::INSERT_EFFECT_OUTPUT_GAIN => insert(Knob::Output, FxParam::Wet)?,
            id::SEND_EFFECT_BYPASS => fx(FxParam::Bypass)?,
            id::EFFECT_TYPE if par.group >= 0 => {
                Self::GroupType(group()?, u8::try_from(par.slot).ok().filter(|&s| s < 8)?)
            }
            id::EFFECT_TYPE | id::SEND_EFFECT_TYPE => fx(FxParam::Type)?,
            // Reverb (`$EFFECT_TYPE_REVERB2`), as its stored values, which
            // are the KSP values / 1e6 as for every other knob. Presets store
            // two EQ values, 0 in every local preset: the high and low cut
            // amounts (gains, 0 flat), not frequencies, which would not
            // default to 0. The band frequencies are not stored and stay
            // unmapped; freeze is not stored either.
            id::RV2_TYPE => fx(FxParam::Reverb(0))?,
            id::RV2_TIME => fx(FxParam::Reverb(1))?,
            id::RV2_SIZE => fx(FxParam::Reverb(2))?,
            id::RV2_DAMPING => fx(FxParam::Reverb(3))?,
            id::RV2_MOD => fx(FxParam::Reverb(4))?,
            id::RV2_DIFF => fx(FxParam::Reverb(5))?,
            id::RV2_PREDELAY => fx(FxParam::Reverb(6))?,
            id::RV2_STEREO => fx(FxParam::Reverb(9))?,
            id::RV2_EQ_HIGH_GAIN => fx(FxParam::Reverb(7))?,
            id::RV2_EQ_LOW_GAIN => fx(FxParam::Reverb(8))?,
            id::RV2_FREEZE => fx(FxParam::Reverb(10))?,
            id::SEND_EFFECT_DRY_LEVEL => fx(FxParam::Dry)?,
            id::SEND_EFFECT_OUTPUT_GAIN => fx(FxParam::Wet)?,
            id::SENDLEVEL_0..=id::SENDLEVEL_7 => {
                let n = (par.id - id::SENDLEVEL_0) as u8;
                if par.group >= 0 {
                    Self::Filter(group()?, u8::try_from(par.slot).ok()?, Knob::SendLevel(n))
                } else {
                    let rack = if par.generic == 0 { Rack::Insert } else { rack()? };
                    (par.group == -1).then_some(())?;
                    Self::Fx(rack, u8::try_from(par.slot).ok()?, FxParam::SendLevel(n))
                }
            }
            _ => match crate::ksp::engine_par_name(par.id)? {
                "$ENGINE_PAR_IRC_PREDELAY" => fx(FxParam::Convolution(0))?,
                "$ENGINE_PAR_IRC_LENGTH_RATIO_ER" => fx(FxParam::Convolution(1))?,
                "$ENGINE_PAR_IRC_LENGTH_RATIO_LR" => fx(FxParam::Convolution(2))?,
                "$ENGINE_PAR_IRC_REVERSE" => fx(FxParam::Convolution(3))?,
                "$ENGINE_PAR_IRC_AUTO_GAIN" => fx(FxParam::Convolution(4))?,
                // The formant filter's knobs: talk, sharp, size.
                "$ENGINE_PAR_FORMANT_TALK" => filter(Knob::Cutoff)?,
                "$ENGINE_PAR_FORMANT_SHARP" => filter(Knob::Resonance)?,
                "$ENGINE_PAR_FORMANT_SIZE" => filter(Knob::Size)?,
                name => {
                    let (kind, n) = crate::fx::blocks::engine_par(name)?;
                    insert(Knob::Field(kind, n), FxParam::Field(kind, n))?
                }
            },
        })
    }

    /// Whether Kontakt itself ignores `par`, which [`resolve`](Self::resolve)
    /// cannot map: AHDSR stages addressed to a flex envelope or to a
    /// modulator slot the group does not have.
    pub(crate) fn inert(par: EnginePar, groups: &[Group]) -> bool {
        let stage = matches!(
            par.id,
            id::ATTACK | id::ATK_CURVE | id::DECAY | id::SUSTAIN | id::RELEASE | id::HOLD
        );
        let group = usize::try_from(par.group).ok().and_then(|g| groups.get(g));
        let slot = usize::try_from(par.slot).ok();
        stage
            && group.zip(slot).is_some_and(|(g, m)| g.modulators.get(m).is_none_or(|m| m.flex))
    }

    /// Held by the bank's group settings.
    pub(crate) fn is_group(&self) -> bool {
        matches!(
            self,
            Self::Group(..)
                | Self::Wavetable(..)
                | Self::Envelope(..)
                | Self::ModEnvelope(..)
                | Self::InternalIntensity { .. }
                | Self::PitchLfoIntensity { .. }
                | Self::InternalBypass(..)
                | Self::Intensity { .. }
                | Self::Filter(..)
        )
    }

    /// Physical value of a KSP value: linear gain, pan -1..=1, semitones,
    /// seconds, intensity, bus index (-1 = instrument output) or bypass 0/1.
    pub(crate) fn decode(self, value: i32) -> f32 {
        let x = (value as f32 / UNIT).clamp(0.0, 1.0);
        match self {
            Self::Wavetable(_, WavePar::FormMode | WavePar::Form2Mode) => value as f32,
            Self::Wavetable(..) => x,
            // An instrument bus, or past the instrument output to output
            // channel `c` as bus `DIRECT + c` (a mic mixer's "Out 2").
            Self::Group(_, GroupPar::Output) => match (value, value - BUS_OFFSET) {
                (_, b @ 0..16) => b as f32,
                (c, _) if (0..OUTS as i32).contains(&c) => f32::from(DIRECT) + c as f32,
                _ => -1.0,
            },
            Self::Fx(_, _, FxParam::Output) if (0..OUTS as i32).contains(&value) => value as f32,
            Self::Fx(_, _, FxParam::Output) => -1.0,
            Self::Fx(_, _, FxParam::Type | FxParam::Filter(Knob::Type)) | Self::GroupType(..) => {
                value as f32
            }
            // `$NI_REVERB2_TYPE_ROOM` (0) or `_HALL` (1).
            Self::Fx(_, _, FxParam::Reverb(0 | 10) | FxParam::Convolution(3 | 4)) => f32::from(value != 0),
            Self::Fx(_, _, FxParam::Reverb(_) | FxParam::Convolution(_) | FxParam::Field(..)) => x,
            Self::Group(_, p) | Self::Instrument(p) => match p {
                GroupPar::Volume => volume(x),
                GroupPar::Pan => 2.0 * x - 1.0,
                _ => (2.0 * x - 1.0) * TUNE_RANGE,
            },
            Self::Envelope(_, stage) | Self::ModEnvelope(_, _, stage) => match stage {
                Stage::Sustain => x,
                Stage::AhdOnly => f32::from(value != 0),
                // Solo sets 1000000, 750000 and 333333 where its presets store 1, 0.5, -0.33.
                Stage::Curve => 2.0 * x - 1.0,
                Stage::Attack | Stage::Hold => time(x, SHORT),
                Stage::Decay | Stage::Release => time(x, LONG),
            },
            // Conflux's saved 2 st pitch target and raw507160 cutoff target
            // independently corroborate the cubic law. Cutoff stays normalized.
            Self::Intensity { cubic: Some(CubicDepth::Pitch), .. }
            | Self::InternalIntensity { cubic: Some(CubicDepth::Pitch), .. }
            | Self::PitchLfoIntensity { bipolar: true, .. } => (2.0 * value as f32 / UNIT - 1.0).powi(3),
            Self::Intensity { cubic: Some(CubicDepth::Cutoff | CubicDepth::Loop), .. }
            | Self::InternalIntensity { cubic: Some(CubicDepth::Cutoff | CubicDepth::Loop), .. } => (2.0 * x - 1.0).powi(3),
            Self::Intensity { bipolar: true, .. }
            | Self::InternalIntensity { bipolar: true, .. } => 2.0 * x - 1.0,
            Self::Filter(_, _, Knob::Bypass) | Self::InternalBypass(..) => f32::from(value != 0),
            Self::Filter(_, _, Knob::Type) => value as f32,
            Self::Filter(_, _, Knob::Output) => effect_gain(x),
            Self::Filter(_, _, Knob::SendLevel(_)) => volume(x),
            // Afflatus sets 434210 where it stores spread -0.1316, Solo 500000 for 0.
            Self::Filter(_, _, Knob::Spread | Knob::Pan)
            | Self::Fx(_, _, FxParam::Filter(Knob::Spread | Knob::Pan)) => 2.0 * x - 1.0,
            // Stored knobs are the KSP value / 1e6: Solo sets 1000000 and 0 where it stores 1 and 0.
            Self::Filter(..) | Self::Fx(_, _, FxParam::Filter(_)) => x,
            // Square law: Areia sets 704316 where its presets store 0.4961.
            Self::Intensity { .. } | Self::InternalIntensity { .. }
            | Self::PitchLfoIntensity { .. } => x * x,
            Self::Fx(_, _, FxParam::Bypass) => f32::from(value != 0),
            Self::Fx(_, _, FxParam::Pan) => 2.0 * x - 1.0,
            Self::Fx(_, _, FxParam::Wet | FxParam::Dry) => effect_gain(x),
            Self::Fx(..) => volume(x),
        }
    }

    /// Inverse of [`decode`](Self::decode), rounded.
    pub(crate) fn encode(self, v: f32) -> i32 {
        let x = match self {
            Self::Wavetable(_, WavePar::FormMode | WavePar::Form2Mode) => return v as i32,
            Self::Wavetable(..) => v,
            Self::Group(_, GroupPar::Output) => {
                return match v {
                    v if v >= f32::from(DIRECT) => v as i32 - i32::from(DIRECT),
                    v if v >= 0.0 => BUS_OFFSET + v as i32,
                    _ => -1,
                };
            }
            Self::Fx(_, _, FxParam::Output) => return if v >= 0.0 { v as i32 } else { -1 },
            Self::Fx(_, _, FxParam::Type | FxParam::Filter(Knob::Type)) | Self::GroupType(..) => {
                return v as i32;
            }
            Self::Fx(_, _, FxParam::Reverb(0 | 10) | FxParam::Convolution(3 | 4)) => return i32::from(v >= 0.5),
            Self::Fx(_, _, FxParam::Reverb(_) | FxParam::Convolution(_) | FxParam::Field(..)) => v,
            Self::Filter(_, _, Knob::Type) => return v as i32,
            Self::Fx(_, _, FxParam::Bypass) | Self::Filter(_, _, Knob::Bypass) | Self::InternalBypass(..) => {
                return i32::from(v != 0.0);
            }
            Self::Group(_, p) | Self::Instrument(p) => match p {
                GroupPar::Volume => volume_value(v),
                GroupPar::Pan => (v + 1.0) * 0.5,
                _ => (v / TUNE_RANGE + 1.0) * 0.5,
            },
            Self::Envelope(_, stage) | Self::ModEnvelope(_, _, stage) => match stage {
                Stage::Sustain => v,
                Stage::AhdOnly => return i32::from(v != 0.),
                Stage::Curve => (v + 1.0) * 0.5,
                Stage::Attack | Stage::Hold => time_value(v, SHORT),
                Stage::Decay | Stage::Release => time_value(v, LONG),
            },
            Self::Intensity { cubic: Some(CubicDepth::Pitch), .. }
            | Self::InternalIntensity { cubic: Some(CubicDepth::Pitch), .. }
            | Self::PitchLfoIntensity { bipolar: true, .. } => return ((v.cbrt() + 1.0) * 0.5 * UNIT).round() as i32,
            Self::Intensity { cubic: Some(CubicDepth::Cutoff | CubicDepth::Loop), .. }
            | Self::InternalIntensity { cubic: Some(CubicDepth::Cutoff | CubicDepth::Loop), .. } => (v.cbrt() + 1.0) * 0.5,
            Self::Intensity { bipolar: true, .. }
            | Self::InternalIntensity { bipolar: true, .. }
            | Self::Fx(_, _, FxParam::Pan)
            | Self::Filter(_, _, Knob::Spread | Knob::Pan)
            | Self::Fx(_, _, FxParam::Filter(Knob::Spread | Knob::Pan)) => (v + 1.0) * 0.5,
            Self::Intensity { .. } | Self::InternalIntensity { .. }
            | Self::PitchLfoIntensity { .. } => v.abs().sqrt(),
            Self::Filter(_, _, Knob::Output) | Self::Fx(_, _, FxParam::Wet | FxParam::Dry) => {
                (v.max(0.0) / EFFECT_MAX_GAIN).cbrt()
            }
            Self::Filter(_, _, Knob::SendLevel(_)) => volume_value(v),
            Self::Filter(..) | Self::Fx(_, _, FxParam::Filter(_)) => v,
            Self::Fx(..) => volume_value(v),
        };
        (x.clamp(0.0, 1.0) * UNIT).round() as i32
    }
}

pub(super) const UNIT: f32 = 1_000_000.0;
/// +12 dB, Kontakt's volume maximum.
const MAX_GAIN: f32 = 3.981_071_7;
/// Group and instrument tune span ±36 semitones.
const TUNE_RANGE: f32 = 36.0;
/// Attack and hold maximum, decay and release maximum (seconds).
const SHORT: f32 = 15.000_02;
const LONG: f32 = 25.000_04;
/// Envelope time curve offset (seconds).
const TIME_BASE: f32 = 0.002;

/// Kontakt's volume law: 630859 is 0 dB, 1000000 is +12 dB (cubic in amplitude).
fn volume(x: f32) -> f32 {
    MAX_GAIN * x * x * x
}

/// +24 dB, the effect output gain and dry level maximum.
const EFFECT_MAX_GAIN: f32 = 16.0;

/// Effect output gain and dry level, cubic: Afflatus sets 396851 where it
/// stores 1.0000056, and 125919 where it stores 0.0319443.
fn effect_gain(x: f32) -> f32 {
    EFFECT_MAX_GAIN * x * x * x
}

fn volume_value(gain: f32) -> f32 {
    (gain.max(0.0) / MAX_GAIN).cbrt()
}

/// Envelope stage time: `2 ms · ((1 + max / 2 ms)^x − 1)`.
fn time(x: f32, max: f32) -> f32 {
    TIME_BASE * ((1.0 + max / TIME_BASE).powf(x) - 1.0)
}

fn time_value(seconds: f32, max: f32) -> f32 {
    (seconds.max(0.0) / TIME_BASE + 1.0).ln() / (1.0 + max / TIME_BASE).ln()
}

/// What `get_engine_par_disp` shows: the value in the unit scripts append
/// themselves (`& " dB"`, `" ms"`, `" Hz"`, `" %"`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Disp {
    /// Linear gain, shown in dB with one decimal, `-inf` at silence.
    Gain(f32),
    /// A number with this many decimals.
    Num(f32, u8),
    /// -1..=1, shown as `C`, `L 50`, `R 50`.
    Pan(f32),
}

impl std::fmt::Display for Disp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::Gain(g) if g <= 1e-6 => f.write_str("-inf"),
            // 630000 is -0.01 dB: Kontakt shows 0.0, not -0.0.
            Self::Gain(g) => write!(f, "{:.1}", (20.0 * g.log10() * 10.0).round() / 10.0 + 0.0),
            Self::Num(v, d) => write!(f, "{:.*}", d as usize, v),
            Self::Pan(p) => match (p.abs() * 100.0).round() {
                n if n < 1.0 => f.write_str("C"),
                n => write!(f, "{} {n:.0}", if p < 0.0 { "L" } else { "R" }),
            },
        }
    }
}

/// Display of an engine parameter value by Kontakt's laws, the same the
/// engine decodes with; `None` for parameters whose law is not modelled.
pub fn display(id: i32, value: i32) -> Option<Disp> {
    let x = (value as f32 / UNIT).clamp(0.0, 1.0);
    Some(match id {
        id::VOLUME | id::SENDLEVEL_0..=id::SENDLEVEL_7 => Disp::Gain(volume(x)),
        id::INSERT_EFFECT_OUTPUT_GAIN | id::SEND_EFFECT_DRY_LEVEL | id::SEND_EFFECT_OUTPUT_GAIN => {
            Disp::Gain(effect_gain(x))
        }
        id::SUSTAIN => Disp::Gain(x),
        id::PAN => Disp::Pan(2.0 * x - 1.0),
        id::TUNE => Disp::Num((2.0 * x - 1.0) * TUNE_RANGE, 2),
        id::ATTACK | id::HOLD => Disp::Num(time(x, SHORT) * 1000.0, 1),
        id::DECAY | id::RELEASE => Disp::Num(time(x, LONG) * 1000.0, 1),
        // The filter's cutoff law (`filter.rs`): 43.6 Hz · 2^(8.96 x).
        id::CUTOFF => Disp::Num(43.6 * (8.96 * x).exp2(), 1),
        // Stereo Modeller spread: 0 % mono, 100 % as recorded, 200 % widest.
        id::STEREO => Disp::Num(x * 200.0, 1),
        // NI's Time is reverb duration. Display this implementation's decay
        // in milliseconds, so scripted labels match the effective DSP.
        id::RV2_TIME => Disp::Num(crate::fx::params::Reverb::time_seconds(x) * 1000., 1),
        _ => match crate::ksp::engine_par_name(id)? {
            "$ENGINE_PAR_SEQ_LF_GAIN" | "$ENGINE_PAR_SEQ_LMF_GAIN"
                | "$ENGINE_PAR_SEQ_HMF_GAIN" | "$ENGINE_PAR_SEQ_HF_GAIN" => {
                Disp::Num(super::filter::geq_gain_db(x), 1)
            }
            name @ ("$ENGINE_PAR_COMP_ATTACK" | "$ENGINE_PAR_COMP_DECAY"
                | "$ENGINE_PAR_LIM_RELEASE" | "$ENGINE_PAR_DL_TIME") => {
                // These fields are stored in milliseconds. Reuse the DSP's
                // conversion, including the limiter's current approximation.
                let (kind, field) = crate::fx::blocks::engine_par(name)?;
                Disp::Num(crate::fx::blocks::stored(kind, field, x), 1)
            }
            "$ENGINE_PAR_IRC_PREDELAY" => {
                Disp::Num(crate::fx::params::IrSettings::predelay_ms(x), 2)
            }
            "$ENGINE_PAR_IRC_LENGTH_RATIO_ER" | "$ENGINE_PAR_IRC_LENGTH_RATIO_LR" => {
                Disp::Num(50. + 100. * x, 1)
            }
            _ => return None,
        },
    })
}

/// Set a group-level parameter (group, envelope or intensity); false for
/// other addresses and missing groups.
pub(crate) fn write(settings: &mut [GroupSettings], address: Address, value: f32) -> bool {
    match address {
        Address::Wavetable(g, p) => {
            let Some(source) = settings.get_mut(g as usize).and_then(|g| g.wavetable.as_mut()) else { return false; };
            match p {
                WavePar::Position => source.position = value.clamp(0., 1.),
                WavePar::Phase => source.phase = value.clamp(0., 1.),
                WavePar::Form => source.form1 = value.clamp(0., 1.),
                WavePar::Form2 => source.form2 = value.clamp(0., 1.),
                WavePar::FormMode | WavePar::Form2Mode => {
                    if !super::wavetable::form_supported(value as i32) { return false; }
                    if matches!(p, WavePar::FormMode) { source.form1_type = value as i32; }
                    else { source.form2_type = value as i32; }
                }
            }
        }
        Address::Group(g, p) => {
            let Some(group) = settings.get_mut(g as usize) else {
                return false;
            };
            match p {
                GroupPar::Volume => group.gain = value.max(0.0),
                GroupPar::Pan => group.pan = value.clamp(-1.0, 1.0),
                GroupPar::Tune => group.tune = value,
                GroupPar::Output => group.bus = (value >= 0.0).then_some(value as u8),
            }
        }
        Address::Envelope(..) | Address::ModEnvelope(..) => {
            let Some((env, stage)) = envelope(settings, address) else {
                return false;
            };
            match stage {
                Stage::Curve => env.curve = value.clamp(-1.0, 1.0),
                Stage::Attack => env.attack = value.max(0.0),
                Stage::Hold => env.hold = value.max(0.0),
                Stage::Decay => env.decay = value.max(0.0),
                Stage::Sustain => env.sustain = value.clamp(0.0, 1.0),
                Stage::Release => env.release = value.max(0.0),
                Stage::AhdOnly => env.ahd_only = value != 0.,
            }
            // One modulator may drive pitch and a filter; both copies share its knobs.
            let updated = *env;
            if let Address::ModEnvelope(g, e, _) = address {
                if let Some(env) = settings[g as usize]
                    .filter
                    .as_mut()
                    .and_then(|f| f.envelope(e))
                {
                    *env = updated;
                }
            }
        }
        Address::Intensity { group, index, cubic, .. } => {
            if cubic.is_some() && !value.is_finite() { return false; }
            let m = settings
                .get_mut(group as usize)
                .and_then(|s| s.mods.mods.get_mut(index as usize));
            let Some(m) = m else {
                return false;
            };
            m.intensity = if cubic == Some(CubicDepth::Pitch) { value } else { value.clamp(-1.0, 1.0) };
        }
        Address::InternalIntensity { group, envelope, target, .. } => {
            if !value.is_finite() { return false; }
            let value = if matches!(address, Address::InternalIntensity { cubic: Some(CubicDepth::Pitch), .. }) {
                value
            } else { value.clamp(-1., 1.) };
            let Some(settings) = settings.get_mut(group as usize) else { return false; };
            let mut applied = false;
            if let Some(m) = settings.pitch_envelopes.iter_mut().find(|e| e.index == envelope)
                .and_then(|e| e.targets.iter_mut().find(|t| t.0 == target)) {
                m.2.intensity = value;
                applied = true;
            }
            if let Some(m) = settings.filter.as_mut().and_then(|f| f.envelope_mod(envelope, target)) {
                m.intensity = value;
                applied = true;
            }
            return applied;
        }
        Address::PitchLfoIntensity { group, slot, target, .. } => {
            if !value.is_finite() { return false; }
            let Some(lfo) = settings.get_mut(group as usize)
                .and_then(|g| g.pitch_lfos.iter_mut().find(|l| l.slot == slot)) else { return false; };
            let Some(index) = lfo.targets.iter().position(|t| t.0 == target) else { return false; };
            let depth: f32 = lfo.targets.iter().enumerate()
                .map(|(i, t)| if i == index { value } else { t.1 }).sum();
            if !depth.is_finite() { return false; }
            lfo.targets[index].1 = value;
            lfo.depth = depth;
        }
        Address::InternalBypass(g, index) => {
            let Some(settings) = settings.get_mut(g as usize) else { return false; };
            let mut applied = false;
            if let Some(envelope) = settings.pitch_envelopes.iter_mut().find(|e| e.index == index) {
                envelope.bypass = value != 0.;
                applied = true;
            }
            if let Some(bypass) = settings.filter.as_mut().and_then(|f| f.envelope_bypass(index)) {
                *bypass = value != 0.;
                applied = true;
            }
            return applied;
        }
        Address::Filter(g, slot, knob) => {
            let filter = settings.get_mut(g as usize).and_then(|s| s.filter.as_mut());
            return filter.is_some_and(|f| f.set_knob(slot, knob, value));
        }
        Address::Instrument(_) | Address::Fx(..) | Address::GroupType(..) => return false,
    }
    true
}

/// The envelope `address` names and the stage.
fn envelope(settings: &mut [GroupSettings], address: Address) -> Option<(&mut Ahdsr, Stage)> {
    match address {
        Address::Envelope(g, stage) => {
            Some((settings.get_mut(g as usize)?.envelope.as_mut()?, stage))
        }
        Address::ModEnvelope(g, e, stage) => {
            let settings = settings.get_mut(g as usize)?;
            let env = if let Some(env) = settings.pitch_envelopes.iter_mut().find(|p| p.index == e)
            {
                &mut env.env
            } else {
                settings.filter.as_mut()?.envelope(e)?
            };
            Some((env, stage))
        }
        _ => None,
    }
}

/// Current value of a group-level parameter.
pub(crate) fn read(settings: &[GroupSettings], address: Address) -> Option<f32> {
    match address {
        Address::Wavetable(g, p) => {
            let source = settings.get(g as usize)?.wavetable.as_ref()?;
            Some(match p { WavePar::Position => source.position, WavePar::Phase => source.phase,
                WavePar::Form => source.form1, WavePar::Form2 => source.form2,
                WavePar::FormMode => source.form1_type as f32, WavePar::Form2Mode => source.form2_type as f32 })
        }
        Address::Group(g, p) => {
            let group = settings.get(g as usize)?;
            Some(match p {
                GroupPar::Volume => group.gain,
                GroupPar::Pan => group.pan,
                GroupPar::Tune => group.tune,
                GroupPar::Output => group.bus.map_or(-1.0, f32::from),
            })
        }
        Address::Envelope(g, stage) | Address::ModEnvelope(g, _, stage) => {
            let env = match address {
                Address::ModEnvelope(_, e, _) => {
                    let settings = settings.get(g as usize)?;
                    settings
                        .pitch_envelopes
                        .iter()
                        .find(|p| p.index == e)
                        .map(|p| p.env)
                        .or_else(|| settings.filter.as_ref()?.envelope_at(e).copied())?
                }
                _ => settings.get(g as usize)?.envelope?,
            };
            Some(match stage {
                Stage::Attack => env.attack,
                Stage::Curve => env.curve,
                Stage::Hold => env.hold,
                Stage::Decay => env.decay,
                Stage::Sustain => env.sustain,
                Stage::Release => env.release,
                Stage::AhdOnly => f32::from(env.ahd_only),
            })
        }
        Address::Intensity { group, index, .. } => settings
            .get(group as usize)?
            .mods
            .mods
            .get(index as usize)
            .map(|m| m.intensity),
        Address::InternalIntensity { group, envelope, target, .. } => {
            let settings = settings.get(group as usize)?;
            settings.pitch_envelopes.iter().find(|e| e.index == envelope)
                .and_then(|e| e.targets.iter().find(|t| t.0 == target)).map(|t| t.2.intensity)
                .or_else(|| settings.filter.as_ref()?.envelope_mod_at(envelope, target).map(|m| m.intensity))
        },
        Address::PitchLfoIntensity { group, slot, target, .. } => settings.get(group as usize)?
            .pitch_lfos.iter().find(|l| l.slot == slot)?.targets.iter().find(|t| t.0 == target).map(|t| t.1),
        Address::InternalBypass(g, index) => {
            let settings = settings.get(g as usize)?;
            settings.pitch_envelopes.iter().find(|e| e.index == index).map(|e| f32::from(e.bypass))
                .or_else(|| settings.filter.as_ref()?.envelope_bypass_at(index).map(f32::from))
        },
        Address::Filter(g, slot, knob) => {
            settings.get(g as usize)?.filter.as_ref()?.knob(slot, knob)
        }
        _ => None,
    }
}

/// One engine parameter change, applied at frame `at` of the next render.
#[derive(Clone, Copy, Debug)]
pub(super) struct Write {
    pub par: EnginePar,
    pub native: i32,
    pub at: u32,
    pub address: Address,
    pub value: f32,
}

/// Engine parameter changes one render can hold; later ones are dropped and counted.
pub const MAX_WRITES: usize = 4096;

/// `$EFFECT_TYPE_*` of group `g`'s insert `slot`, 0 when it is empty.
pub(crate) fn group_type(groups: &[Group], g: u16, slot: u8) -> Option<f32> {
    let fx = &groups.get(g as usize)?.fx;
    let kind = fx.slots.iter().find(|fx| fx.slot == slot as usize).map(|fx| fx.kind.ser_id());
    Some(f32::from(kind.unwrap_or(0)))
}

/// `find_mod`: position in `Group::modulators` of the first name `is` accepts.
pub(crate) fn find_mod(groups: &[Group], group: usize, is: &dyn Fn(&str) -> bool) -> Option<usize> {
    groups.get(group)?.modulators.iter().position(|m| m.kind != "undecoded" && is(&m.name))
}

/// `find_target`: position among the modulator's targets.
pub(crate) fn find_target(
    groups: &[Group],
    group: usize,
    modulator: usize,
    is: &dyn Fn(&str) -> bool,
) -> Option<usize> {
    groups
        .get(group)?
        .modulators
        .get(modulator)?
        .targets
        .iter()
        .position(|t| is(t))
}

#[cfg(test)]
mod tests {
    #[test]
    fn solid_geq_callback_captions_show_signed_dsp_gain() {
        use crate::ksp::{KspEngine, LogEngine, Runtime, Value};
        for name in ["$ENGINE_PAR_SEQ_LF_GAIN", "$ENGINE_PAR_SEQ_LMF_GAIN",
            "$ENGINE_PAR_SEQ_HMF_GAIN", "$ENGINE_PAR_SEQ_HF_GAIN"] {
            let source = format!("on init\nmake_perfview\ndeclare ui_slider $gain(0,1000000)\ndeclare ui_label $caption(1,1)\nend on\non ui_control($gain)\nset_engine_par({name},$gain,-1,2,$NI_BUS_OFFSET)\nset_control_par_str(get_ui_id($gain),$CONTROL_PAR_LABEL,get_engine_par_disp({name},-1,2,$NI_BUS_OFFSET) & \" dB\")\nset_text($caption,get_engine_par_disp_ext({name},$gain,-1,2,$NI_BUS_OFFSET) & \" dB\")\nend on");
            let mut host = LogEngine::new(Vec::new(), 48000.);
            let (mut rt, errors) = Runtime::with_scripts(&[&source], &mut host, 8, Vec::new());
            assert!(errors.iter().all(Option::is_none), "{errors:?}");
            let id = (ENGINE_PAR_BASE..ENGINE_PAR_BASE + 512).find(|&id| crate::ksp::engine_par_name(id) == Some(name)).unwrap();
            for (value, caption) in [(0, "-15.0 dB"), (500000, "0.0 dB"),
                (750000, "7.5 dB"), (1000000, "15.0 dB")] {
                rt.ui_control(&mut host, 0, 0, value);
                let ui = rt.interface(0);
                assert_eq!(ui.controls[0].properties["$CONTROL_PAR_LABEL"], Value::Text(caption.into()), "{name}");
                assert_eq!(ui.controls[1].properties["$CONTROL_PAR_TEXT"], Value::Text(caption.into()), "{name}");
                assert_eq!(host.engine_par(EnginePar { id, group: -1, slot: 2, generic: 1000 }), Some(value));
            }
        }
    }

    #[test]
    fn reverb_time_callback_displays_the_effective_decay_in_milliseconds() {
        let ui = crate::ksp::initialize("on init\nmake_perfview\ndeclare ui_slider $time(0,127)\ndeclare ui_label $caption(1,1)\nset_engine_par($ENGINE_PAR_RV2_TIME,370078,-1,0,0)\nset_control_par_str(get_ui_id($time),$CONTROL_PAR_LABEL,get_engine_par_disp($ENGINE_PAR_RV2_TIME,-1,0,0) & \" ms\")\nset_text($caption,get_engine_par_disp($ENGINE_PAR_RV2_TIME,-1,0,0) & \" ms\")\nend on", 0, 8).unwrap();
        assert_eq!(ui.controls[0].properties["$CONTROL_PAR_LABEL"], crate::ksp::Value::Text("1099.5 ms".into()));
        assert_eq!(ui.controls[1].properties["$CONTROL_PAR_TEXT"], crate::ksp::Value::Text("1099.5 ms".into()));
        for (value, milliseconds) in [(0,200.), (500000,2000.), (1000000,20000.)] {
            let Some(Disp::Num(shown, _)) = display(id::RV2_TIME, value) else { panic!("RV2_TIME display absent") };
            assert_eq!(shown, milliseconds);
        }
    }

    #[test]
    fn effect_time_callback_labels_use_dsp_milliseconds() {
        let ui = crate::ksp::initialize("on init\nmake_perfview\ndeclare ui_slider $release(0,1000000)\n$release := 230000\nset_control_par_str(get_ui_id($release),$CONTROL_PAR_LABEL,get_engine_par_disp_ext($ENGINE_PAR_LIM_RELEASE,$release,-1,4,0) & \" ms\")\nend on", 0, 0).unwrap();
        assert_eq!(ui.controls[0].properties["$CONTROL_PAR_LABEL"], crate::ksp::Value::Text("28.8 ms".into()));
        for name in ["$ENGINE_PAR_COMP_ATTACK", "$ENGINE_PAR_COMP_DECAY", "$ENGINE_PAR_LIM_RELEASE", "$ENGINE_PAR_DL_TIME"] {
            let id = (ENGINE_PAR_BASE..ENGINE_PAR_BASE + 512).find(|&id| crate::ksp::engine_par_name(id) == Some(name)).unwrap();
            let (kind, field) = crate::fx::blocks::engine_par(name).unwrap();
            for value in [0, 230000, 1000000] {
                let Some(Disp::Num(shown, _)) = display(id, value) else { panic!("{name}") };
                assert_eq!(shown, crate::fx::blocks::stored(kind, field, value as f32 / 1e6));
            }
        }
    }

    use super::*;
    use crate::import::{Modulator, ShaperCurve};

    #[test]
    fn equal_curves_share_one_copy() {
        let mut other = group();
        other.mods[0].shaper = Some(ShaperCurve::Table(vec![0.5; 128]));
        let mut tables = [&group(), &group(), &other].map(ModTable::from);
        share_curves(tables.iter_mut());
        let curve = |t: &ModTable| t.mods[0].curve.clone().unwrap();
        assert!(Arc::ptr_eq(&curve(&tables[0]), &curve(&tables[1])));
        assert!(!Arc::ptr_eq(&curve(&tables[0]), &curve(&tables[2])));
        assert_eq!(tables[0], ModTable::from(&group()), "sharing changes no value");
    }

    #[test]
    fn release_counter_moves_the_release_sample_start() {
        // Pacific's RTC_PITCH: counter -> sample start at 0.5, shaped from 1
        // (counter 0) down to 0.42 (counter 1). Dropping the route plays the
        // release's loud onset, about 4 dB over the whole audit render.
        let group = Group {
            mods: vec![ModAssignment {
                name: "RTC_PITCH".into(),
                source: ModSource::ReleaseTriggerCounter,
                target: ModTarget::SampleStart,
                intensity: 0.5,
                invert: false,
                lag_ms: 0,
                shaper: Some(ShaperCurve::Table(
                    (0..128).map(|i| 1.0 - 0.58 * i as f32 / 127.0).collect(),
                )),
            }],
            ..Group::default()
        };
        let table = ModTable::from(&group);
        let cc = [0u8; 128];
        let at = |counter| {
            table.start_offset(&Inputs {
                cc74: None,
                cc: &cc,
                bend: 0.0,
                pressure: 0,
                note: 60,
                velocity: 100,
                counter,
            })
        };
        // A 700 ms note under T = 1500 ms leaves 0.53 of the counter.
        let x = release_counter(1500, 700.0);
        assert!((x - 0.533).abs() < 1e-3);
        assert!((at(x) - 0.5 * (1.0 - 0.58 * x)).abs() < 1e-2, "{}", at(x));
        let (low, high) = table.start_offset_range(&cc, 60..=60, 100..=100);
        assert!(
            (low - 0.21).abs() < 1e-2 && (high - 0.5).abs() < 1e-3,
            "{low}..{high}"
        );
    }

    #[test]
    fn lagged_values_settle_exactly() {
        // A long lag over short blocks steps less than half the float
        // spacing near the target: without the snap it never arrives.
        let mut value = 0.2f32;
        for _ in 0..2000 {
            approach(&mut value, 0.7, 0.01, 128, 48000.0);
        }
        assert_eq!(value, 0.7);
    }

    #[test]
    fn ids_match_the_runtime_table() {
        use crate::ksp::engine_par_name as name;
        for (id, expected) in [
            (id::VOLUME, "VOLUME"),
            (id::PAN, "PAN"),
            (id::TUNE, "TUNE"),
            (id::OUTPUT_CHANNEL, "OUTPUT_CHANNEL"),
            (id::ATTACK, "ATTACK"),
            (id::DECAY, "DECAY"),
            (id::SUSTAIN, "SUSTAIN"),
            (id::RELEASE, "RELEASE"),
            (id::HOLD, "HOLD"),
            (id::ATK_CURVE, "ATK_CURVE"),
            (id::MOD_TARGET_INTENSITY, "MOD_TARGET_INTENSITY"),
            (id::MOD_TARGET_MP_INTENSITY, "MOD_TARGET_MP_INTENSITY"),
            (id::INTMOD_INTENSITY, "INTMOD_INTENSITY"),
            (id::INTMOD_BYPASS, "INTMOD_BYPASS"),
            (id::EFFECT_BYPASS, "EFFECT_BYPASS"),
            (id::EFFECT_TYPE, "EFFECT_TYPE"),
            (id::EFFECT_SUBTYPE, "EFFECT_SUBTYPE"),
            (id::SEND_EFFECT_TYPE, "SEND_EFFECT_TYPE"),
            (id::RV2_PREDELAY, "RV2_PREDELAY"),
            (id::RV2_TIME, "RV2_TIME"),
            (id::RV2_TYPE, "RV2_TYPE"),
            (id::RV2_SIZE, "RV2_SIZE"),
            (id::RV2_DAMPING, "RV2_DAMPING"),
            (id::RV2_DIFF, "RV2_DIFF"),
            (id::RV2_MOD, "RV2_MOD"),
            (id::RV2_STEREO, "RV2_STEREO"),
            (id::RV2_FREEZE, "RV2_FREEZE"),
            (id::RV2_EQ_LOW_FREQ, "RV2_EQ_LOW_FREQ"),
            (id::RV2_EQ_LOW_GAIN, "RV2_EQ_LOW_GAIN"),
            (id::RV2_EQ_HIGH_FREQ, "RV2_EQ_HIGH_FREQ"),
            (id::RV2_EQ_HIGH_GAIN, "RV2_EQ_HIGH_GAIN"),
            (id::SEND_EFFECT_BYPASS, "SEND_EFFECT_BYPASS"),
            (id::SEND_EFFECT_DRY_LEVEL, "SEND_EFFECT_DRY_LEVEL"),
            (id::SEND_EFFECT_OUTPUT_GAIN, "SEND_EFFECT_OUTPUT_GAIN"),
            (id::INSERT_EFFECT_OUTPUT_GAIN, "INSERT_EFFECT_OUTPUT_GAIN"),
            (id::SENDLEVEL_0, "SENDLEVEL_0"),
            (id::SENDLEVEL_7, "SENDLEVEL_7"),
            (id::STEREO, "STEREO"),
            (id::STEREO_PAN, "STEREO_PAN"),
            (id::FREQ1, "FREQ1"),
            (id::FREQ3, "FREQ3"),
            (id::BW1, "BW1"),
            (id::BW3, "BW3"),
            (id::GAIN1, "GAIN1"),
            (id::GAIN3, "GAIN3"),
        ] {
            assert_eq!(name(id), Some(format!("$ENGINE_PAR_{expected}").as_str()));
        }
    }

    #[test]
    fn value_laws_match_reference_points() {
        let volume = Address::Instrument(GroupPar::Volume);
        assert!((volume.decode(630_859) - 1.0).abs() < 1e-3, "0 dB");
        // The cube law is Kontakt's to within 0.005 dB, not bit-exact.
        assert!((volume.encode(1.0) - 630_859).abs() < 200);
        // Areia sets 704316 where its presets store intensity 0.4961.
        let intensity = Address::Intensity {
            group: 0,
            index: 0,
            bipolar: false,
            cubic: None,
        };
        assert!((intensity.decode(704_316) - 0.4961).abs() < 1e-3);
        // Areia sets these at init; its saved envelopes hold the same times.
        let attack = Address::Envelope(0, Stage::Attack);
        let release = Address::Envelope(0, Stage::Release);
        assert!((attack.decode(465_229) - 0.125_013).abs() < 1e-4);
        assert!((release.decode(512_668) - 0.250_001).abs() < 1e-4);
        // Afflatus sets these on a group insert and its send; the presets store the gains.
        let output = Address::Filter(0, 1, Knob::Output);
        assert!((output.decode(396_851) - 1.000_005_6).abs() < 1e-4);
        assert!((Address::Fx(Rack::Send, 0, FxParam::Wet).decode(125_919) - 0.031_944).abs() < 1e-5);
        assert!((output.encode(1.0) - 396_851).abs() < 2);
        let spread = Address::Filter(0, 0, Knob::Spread);
        assert!((spread.decode(434_210) + 0.131_58).abs() < 1e-4);
        assert!((release.decode(1_000_000) - 25.000_04).abs() < 1e-3);
        // Solo sets these where its presets store attack curves 0.5 and -0.33.
        let curve = Address::Envelope(0, Stage::Curve);
        assert_eq!(curve.decode(750_000), 0.5);
        assert!((curve.decode(333_333) + 0.333_33).abs() < 1e-5);
        assert_eq!(curve.encode(1.0), 1_000_000);
        assert_eq!(release.encode(0.250_001), 512_668);
        // Areia's send level, stored as 0.25 (−12 dB).
        let send = Address::Fx(Rack::Insert, 7, FxParam::SendLevel(0));
        assert!((send.decode(396_820) - 0.25).abs() < 2e-3);
        let output = Address::Group(0, GroupPar::Output);
        assert_eq!(output.decode(1003), 3.0);
        assert_eq!(output.encode(output.decode(-1)), -1);
        // Past the instrument output: channel 1 ("Out 2"), both ways.
        assert_eq!(output.decode(1), f32::from(DIRECT) + 1.0);
        assert_eq!(output.encode(output.decode(1)), 1);
        let bus_out = Address::Fx(Rack::Bus(2), 0, FxParam::Output);
        assert_eq!((bus_out.decode(3), bus_out.decode(-1), bus_out.encode(3.0)), (3.0, -1.0, 3));
    }

    fn group() -> Group {
        let assignment = |source, lag_ms, shaper| ModAssignment {
            name: String::new(),
            source,
            target: ModTarget::Volume,
            intensity: 1.0,
            invert: false,
            lag_ms,
            shaper,
        };
        Group {
            mods: vec![
                // Velocity squared by a table shaper.
                assignment(
                    ModSource::Velocity,
                    0,
                    Some(ShaperCurve::Table(
                        (0..128).map(|i| (i as f32 / 127.0).powi(2)).collect(),
                    )),
                ),
                assignment(ModSource::MidiCc(11), 100, None),
            ],
            modulators: vec![
                Modulator {
                    name: "ENV_AHDSR".into(),
                    targets: vec!["ENV_AHDSR_VOLUME".into()],
                    assignments: None,
                    volume_env: true,
                    bypassed: false,
                    flex: false,
                    envelope: None,
                    kind: String::new(),
                },
                Modulator {
                    name: "VEL_VOLUME".into(),
                    targets: vec![String::new()],
                    assignments: Some(0),
                    volume_env: false,
                    bypassed: false,
                    flex: false,
                    envelope: None,
                    kind: String::new(),
                },
                Modulator {
                    name: "CC_VOLUME".into(),
                    targets: vec![String::new()],
                    assignments: Some(1),
                    volume_env: false,
                    bypassed: false,
                    flex: false,
                    envelope: None,
                    kind: String::new(),
                },
            ],
            ..Group::default()
        }
    }

    #[test]
    fn group_insert_types_and_reverb_eq_freeze_resolve() {
        use crate::fx::{Effect, Kind, Params};
        let mut g = group();
        g.fx.slots.push(Effect {
            slot: 2,
            kind: Kind::StereoModeller,
            version: 0,
            bypass: false,
            output_gain: 1.0,
            dry_level: 0.0,
            params: Params::Opaque { bytes: 0 },
        });
        let groups = [g];
        let par = |id, group, slot, generic| EnginePar {
            id,
            group,
            slot,
            generic,
        };
        let ty = |slot| Address::resolve(par(id::EFFECT_TYPE, 0, slot, -1), &groups);
        assert_eq!((ty(2), ty(8)), (Some(Address::GroupType(0, 2)), None));
        assert_eq!(
            (group_type(&groups, 0, 2), group_type(&groups, 0, 1)),
            (Some(31.0), Some(0.0))
        );
        // A filter's type is its subtype; it decodes as the raw type id.
        let sub = Address::resolve(par(id::EFFECT_SUBTYPE, 0, 1, -1), &groups);
        assert_eq!(sub, Some(Address::Filter(0, 1, Knob::Type)));
        assert_eq!(
            (
                Address::Filter(0, 1, Knob::Type).decode(106),
                Address::Filter(0, 1, Knob::Type).encode(106.0)
            ),
            (106.0, 106)
        );
        // Reverb: the stored EQ values are the cut amounts; freeze is a switch.
        let rv = |id| {
            Address::resolve(par(id, -1, 0, 0), &[]).map(|a| match a {
                Address::Fx(_, _, FxParam::Reverb(n)) => n,
                _ => u8::MAX,
            })
        };
        let unresolved = |id| Address::resolve(par(id, -1, 0, 0), &[]).is_none();
        assert_eq!(
            [id::RV2_EQ_HIGH_GAIN, id::RV2_EQ_LOW_GAIN, id::RV2_FREEZE].map(rv),
            [Some(7), Some(8), Some(10)]
        );
        assert!(unresolved(id::RV2_EQ_LOW_FREQ) && unresolved(id::RV2_EQ_HIGH_FREQ));
        let freeze = Address::Fx(Rack::Send, 0, FxParam::Reverb(10));
        assert_eq!(
            (
                freeze.decode(1_000_000),
                freeze.decode(0),
                freeze.encode(1.0)
            ),
            (1.0, 0.0, 1)
        );
    }

    #[test]
    fn scripted_ahd_only_ignores_early_release_and_finishes_without_heap() {
        use crate::{engine::ScriptSetup, import::Instrument, ksp::{KspEngine, Runtime, Value}};
        let imported = crate::import::Ahdsr { attack_curve: 0., attack_ms: 7., hold_ms: 3.,
            decay_ms: 40., sustain: 0.8, release_ms: 1., unknown_flag: 0, unknown_tail: Vec::new() };
        let mut volume = group();
        volume.volume_env = Some(imported.clone());
        let mut pitch = volume.clone();
        pitch.volume_env = None;
        pitch.modulators[0].volume_env = false;
        pitch.modulators[0].envelope = Some(0);
        pitch.envelopes = vec![crate::modulation::ModEnvelope { env: imported, targets: vec![ModAssignment {
            name: "Envelope".into(), source: ModSource::Unassigned, target: ModTarget::Pitch,
            intensity: 1., invert: false, lag_ms: 0, shaper: None,
        }] }];
        let i = Instrument { groups: vec![volume, pitch], ..Default::default() };
        assert_eq!(crate::ksp::engine_par_name(id::ENV_AHD), Some("$ENGINE_PAR_ENV_AHD"));
        let mut setup = ScriptSetup::new(&i, 1000.);
        let source = "on init\nset_engine_par($ENGINE_PAR_ENV_AHD,1,0,0,-1)\nset_engine_par($ENGINE_PAR_ENV_AHD,1,1,0,-1)\ndeclare $saved := get_engine_par($ENGINE_PAR_ENV_AHD,0,0,-1)\nmake_persistent($saved)\nend on";
        let (rt, errors) = Runtime::with_scripts(&[source], &mut setup, 0, Vec::new());
        assert!(errors.iter().all(Option::is_none));
        assert_eq!(rt.persistence()[0]["$saved"], Value::Int(1));
        let pars: [_; 2] = std::array::from_fn(|g| EnginePar { id: id::ENV_AHD, group: g as i32, slot: 0, generic: -1 });
        for par in pars { assert_eq!(setup.engine_par(par), Some(1)); }
        let mut settings: Vec<_> = i.groups.iter().map(GroupSettings::from).collect();
        let addresses: [_; 2] = pars.map(|p| Address::resolve(p, &i.groups).unwrap());
        assert!(!settings[0].envelope.unwrap().ahd_only, "preset flags are not inferred");
        assert_eq!(crate::plugin::tests::allocations(|| {
            for address in addresses {
                assert!(write(&mut settings, address, address.decode(1)));
                assert_eq!(address.encode(read(&settings, address).unwrap()), 1);
            }
            assert!(settings[0].envelope.unwrap().ahd_only);
            assert!(settings[1].pitch_envelopes[0].env.ahd_only);
            for curve in [-0.8, 0., 0.8] {
                for zero_times in [false, true] {
                    let mut p = settings[0].envelope.unwrap();
                    p.curve = curve;
                    if zero_times { p.attack = 0.; p.hold = 0.; p.decay = 0.; }
                    let mut held = Envelope::new(&p, 1000.);
                    let mut released = held;
                    let mut skipped = held;
                    let mut scalar = held;
                    released.release(None);
                    skipped.release(None);
                    scalar.release(None);
                    for block in 0..32 {
                        let (mut h, mut r, mut q) = ([0.; 13], [0.; 13], [0.; 13]);
                        held.render(&mut h, None, 1000.);
                        released.render(&mut r, None, 1000.);
                        for x in &mut q { scalar.render(std::slice::from_mut(x), None, 1000.); }
                        skipped.skip(13, None, 1000.);
                        assert_eq!(h, r, "AHD is one-shot even when released during attack");
                        assert!(h.iter().all(|v| v.is_finite()));
                        for (a,b) in h.into_iter().zip(q) { assert!((a-b).abs() < 0.00002); }
                        assert!((held.level() - skipped.level()).abs() < 0.00002);
                        assert_eq!(held.phase(), skipped.phase());
                        if block == 1 { released.release(None); skipped.release(None); scalar.release(None); }
                    }
                    assert!(held.done() && released.done() && skipped.done() && scalar.done());
                    assert_eq!(held.level(), 0.);
                }
            }
            // Turning it off restores the stored sustain/release behavior.
            let address = addresses[0];
            assert!(write(&mut settings, address, address.decode(0)));
            assert_eq!(address.encode(read(&settings, address).unwrap()), 0);
            let p = settings[0].envelope.unwrap();
            assert_eq!(p.sustain, 0.8);
            let mut ordinary = Envelope::new(&p, 1000.);
            ordinary.skip(300, None, 1000.);
            assert!(!ordinary.done());
            assert_eq!(ordinary.level(), 0.8);
            ordinary.release(None);
            ordinary.skip(20, None, 1000.);
            assert!(ordinary.done());
        }), 0);
    }

    #[test]
    fn pitch_envelope_uses_ahdsr_and_script_addresses() {
        use crate::modulation::{Ahdsr as ImportedAhdsr, ModEnvelope};
        let env = ImportedAhdsr {
            attack_curve: 0.,
            attack_ms: 10.,
            hold_ms: 0.,
            decay_ms: 10.,
            sustain: 0.25,
            release_ms: 10.,
            unknown_flag: 0,
            unknown_tail: Vec::new(),
        };
        let mut pitch = group().mods.remove(0);
        pitch.source = ModSource::Unassigned;
        pitch.target = ModTarget::Pitch;
        pitch.intensity = 0.5;
        pitch.shaper = None;
        let g = Group {
            envelopes: vec![ModEnvelope {
                env,
                targets: vec![pitch],
            }],
            modulators: vec![Modulator {
                name: "Pitch".into(),
                targets: vec!["Depth".into()],
                assignments: None,
                volume_env: false,
                bypassed: false,
                flex: false,
                envelope: Some(0),
                kind: "ahdsr".into(),
            }],
            ..Group::default()
        };
        let mut settings = vec![GroupSettings::from(&g)];
        let p = &settings[0].pitch_envelopes[0];
        let mut state = Envelope::new(&p.env, 48_000.);
        assert!((p.pitch(&mut state, 480, 48_000.) - 6.).abs() < 1e-4);
        let peak = p.pitch(&mut state, 1, 48_000.);
        assert!((peak - 6.).abs() < 1e-4);
        let sustain = p.pitch(&mut state, 20_000, 48_000.);
        assert!((sustain - 1.5).abs() < 1e-4);
        state.release(None);
        assert!(p.pitch(&mut state, 480, 48_000.) < sustain);
        assert_eq!(p.pitch(&mut state, 20_000, 48_000.), 0.);
        let groups = [g];
        let address = Address::resolve(
            EnginePar {
                id: id::MOD_TARGET_MP_INTENSITY,
                group: 0,
                slot: 0,
                generic: 0,
            },
            &groups,
        )
        .unwrap();
        assert!(write(&mut settings, address, address.decode(250_000)));
        assert_eq!(read(&settings, address), Some(-0.125));
        assert_eq!(address.encode(read(&settings, address).unwrap()), 250_000);
        let attack = Address::resolve(
            EnginePar {
                id: id::ATTACK,
                group: 0,
                slot: 0,
                generic: -1,
            },
            &groups,
        )
        .unwrap();
        assert!(write(&mut settings, attack, 0.02));
        assert_eq!(read(&settings, attack), Some(0.02));
        let p = &settings[0].pitch_envelopes[0];
        let mut state = Envelope::new(&p.env, 48_000.);
        assert!((p.pitch(&mut state, 480, 48_000.) + 0.75).abs() < 1e-4);

        // A held +6 semitone pitch envelope must reach the actual resampler.
        let legacy = Address::resolve(EnginePar { id: id::INTMOD_INTENSITY, group: 0, slot: 0, generic: 0 }, &groups).unwrap();
        assert_eq!(legacy, address, "internal pitch aliases share the same law and target");
        for (value, semitones) in [(0, -12.), (250_000, -1.5), (500_000, 0.), (750_000, 1.5),
            (1_000_000, 12.), (1_129_961, 24.), (1_221_125, 36.), (1_293_701, 48.), (2_000_000, 324.)] {
            let depth = legacy.decode(value);
            assert!((12. * depth - semitones).abs() < 1e-3);
            assert!((legacy.encode(depth) - value).abs() <= 1);
            assert!(write(&mut settings, legacy, depth));
            assert_eq!(read(&settings, legacy), Some(depth));
        }
        assert_eq!(address.decode(750_000), 0.125); // Both signed pitch APIs use the cubic law.
        assert!(!write(&mut settings, legacy, f32::NAN));
        assert!(write(&mut settings, legacy, -0.5));
        let bypass = Address::resolve(EnginePar { id: id::INTMOD_BYPASS, group: 0, slot: 0, generic: -1 }, &groups).unwrap();
        assert!(write(&mut settings, bypass, bypass.decode(1)));
        assert_eq!(bypass.encode(read(&settings, bypass).unwrap()), 1);
        let p = &settings[0].pitch_envelopes[0];
        let mut bypassed = Envelope::new(&p.env, 48_000.);
        assert_eq!(p.pitch(&mut bypassed, 480, 48_000.), 0.);
        assert!((bypassed.level() - 0.5).abs() < 1e-4);
        assert!(write(&mut settings, bypass, bypass.decode(0)));
        assert!((settings[0].pitch_envelopes[0].pitch(&mut bypassed, 0, 48_000.) + 3.).abs() < 1e-4);
        let mut full = groups[0].clone();
        full.envelopes = vec![full.envelopes[0].clone(); PITCH_ENVS];
        let prepared = PitchEnvelope::from_group(&full);
        assert_eq!(prepared.len(), 16);
        assert_eq!(prepared.last().unwrap().index, 15);

        let mut pitched = groups[0].clone();
        let env = &mut pitched.envelopes[0];
        (env.env.attack_ms, env.env.sustain) = (0., 1.);
        let reference = Group { tune: 2f64.powf(0.5), ..Group::default() };
        let render = |group| {
            let sample = crate::audio::Sample { rate: 48_000, frames: (0..4096)
                .map(|i| [(i as f32 * 0.08).sin() * 0.25; 2]).collect() };
            let bank = super::super::Bank::from_samples(vec![group], vec![crate::import::Zone::default()],
                vec![(std::path::PathBuf::new(), sample)]).unwrap();
            let mut engine = super::super::Engine::default();
            engine.set_bank(Some(Box::new(bank)));
            engine.note_on(0, 60, 100);
            let (mut l, mut r) = ([0.; 1024], [0.; 1024]);
            engine.render(&mut l, &mut r);
            l
        };
        for e in &mut full.envelopes {
            (e.env.attack_ms, e.env.sustain) = (0., 1.);
            e.targets[0].intensity = 0.5 / PITCH_ENVS as f32;
        }
        let all_slots = render(full.clone());
        let (pitched, reference) = (render(pitched), render(reference));
        assert!(reference.iter().any(|x| x.abs() > 0.01));
        assert!(pitched.iter().zip(reference).all(|(a, b)| (a - b).abs() < 1e-6));
        assert!(all_slots.iter().zip(reference).all(|(a, b)| (a - b).abs() < 1e-6));
        for (raw, st) in [(1_129_961, 24.), (1_221_125, 36.)] {
            let mut pitched = groups[0].clone();
            let env = &mut pitched.envelopes[0];
            (env.env.attack_ms, env.env.sustain) = (0., 1.);
            env.targets[0].intensity = address.decode(raw);
            let pitched = render(pitched);
            let ratio = 2f64.powf(f64::from(address.decode(raw)));
            assert!((ratio - 2f64.powf(f64::from(st) / 12.)).abs() < 1e-4);
            let reference = render(Group { tune: ratio, ..Group::default() });
            assert!(pitched.iter().zip(reference).all(|(a,b)| (a-b).abs() < 1e-4),
                "modern signed depth must reach the actual resampler at {st} semitones");
        }
        full.envelopes.push(full.envelopes[0].clone());
        assert!(super::super::Bank::from_samples(vec![full], Vec::new(), Vec::new()).is_err());
    }

    #[test]
    #[ignore = "requires the user's installed library; only aggregate modulation facts are printed"]
    fn analog_strings_pitch_envelopes_are_routed() {
        let path = std::env::var_os("KONTRA_ANALOG_NKI").expect("set KONTRA_ANALOG_NKI");
        let i = crate::import::read(std::path::Path::new(&path)).unwrap();
        let mut count = 0;
        for g in &i.groups {
            let settings = GroupSettings::from(g);
            count += settings.pitch_envelopes.len();
            for m in &g.modulators {
                if m.name == "Pitch_Envelope" {
                    let e = &g.envelopes[m.envelope.unwrap()];
                    assert!(e.targets.iter().any(|t| t.target == ModTarget::Pitch));
                    assert!(!settings.pitch_envelopes.is_empty());
                }
            }
        }
        println!("groups={} pitch_envelopes={count}", i.groups.len());
        assert_eq!(count, 483);
    }

    #[test]
    fn velocity_shaper_and_lagged_cc_volume() {
        let table = ModTable::from(&group());
        let mut cc = [0; 128];
        cc[11] = 127;
        let mut input = Inputs {
            cc: &cc,
            cc74: None,
            bend: 0.0,
            pressure: 0,
            note: 60,
            velocity: 64,
            counter: 0.0,
        };
        let mut values = table.start(&input, None);
        let (gain, ..) = table.modulate(&mut values, &input, 128, 48000.0, None);
        let expected = (64.0f32 / 127.0).powi(2);
        assert!((gain - expected).abs() < 1e-5, "{gain} vs {expected}");

        // CC11 drops to 0: after one lag time constant 63% of the way down.
        let quiet = [0; 128];
        input.cc = &quiet;
        let (gain, ..) = table.modulate(&mut values, &input, 4800, 48000.0, None);
        assert!((gain / expected - (-1.0f32).exp()).abs() < 1e-4, "{gain}");
    }

    #[test]
    fn velocity_scales_envelope_attack_at_note_start() {
        let group = Group {
            mods: vec![ModAssignment {
                name: "VEL_ATTACK".into(),
                source: ModSource::Velocity,
                target: ModTarget::Attack,
                intensity: 1.0,
                invert: false,
                lag_ms: 0,
                // Pacific's stored shaper: soft notes keep the attack, hard ones shorten it.
                shaper: Some(ShaperCurve::Table(vec![1.0, 0.59])),
            }],
            ..Group::default()
        };
        let table = ModTable::from(&group);
        let cc = [0; 128];
        let attack = |velocity| {
            let mut env = Ahdsr {
                attack: 0.9,
                ..Ahdsr::UNITY
            };
            let input = Inputs {
                cc: &cc,
                cc74: None,
                bend: 0.0,
                pressure: 0,
                note: 60,
                velocity,
                counter: 0.0,
            };
            table.scale_envelope(&mut env, &input);
            (env.attack, env.release)
        };
        assert_eq!(attack(0), (0.9, f32::INFINITY));
        assert!((attack(127).0 - 0.9 * 0.59).abs() < 1e-5);
        // Envelope times are not per-block voice modulation.
        assert!(table.voiced.is_empty());
    }

    #[test]
    #[cfg(feature = "plugin")]
    fn modern_signed_pitch_depth_matches_native_writer_without_heap() {
        let mut g = group();
        g.mods[1].target = ModTarget::Pitch;
        g.modulators[1].targets = vec!["Volume".into(), "Pitch".into()];
        g.modulators.truncate(2);
        let groups = [g];
        let mut settings = [GroupSettings::from(&groups[0])];
        let par = |id, generic| EnginePar { id, group: 0, slot: 1, generic };
        let pitch = Address::resolve(par(id::MOD_TARGET_MP_INTENSITY, 1), &groups).unwrap();
        let legacy = Address::resolve(par(id::INTMOD_INTENSITY, 1), &groups).unwrap();
        assert_eq!(legacy, pitch, "native parameter449 aliases resolve to the same exact target");
        assert!(Address::resolve(par(id::INTMOD_INTENSITY, 0), &groups).is_none(),
            "unverified external volume law remains unsupported for the legacy alias");
        let volume = Address::resolve(par(id::MOD_TARGET_MP_INTENSITY, 0), &groups).unwrap();
        let unipolar = Address::resolve(par(id::MOD_TARGET_INTENSITY, 1), &groups).unwrap();
        assert_eq!(volume.decode(750_000), 0.5, "unverified non-pitch law remains unchanged");
        assert_eq!(Address::resolve(par(id::MOD_TARGET_MP_INTENSITY, 2), &groups), None);
        // Actual Conflux PB->PITCH UP target: raw775160 and saved depth .16666558385.
        assert!((pitch.decode(775_160) - 0.16666558385).abs() < 1e-7);
        let mut native = super::super::native_state::NativeState::default();
        native.prepare(pitch, par(id::MOD_TARGET_MP_INTENSITY, 1), pitch.encode(1.));
        native.prepare(unipolar, par(id::MOD_TARGET_INTENSITY, 1), unipolar.encode(1.));
        native.prepare(legacy, par(id::INTMOD_INTENSITY, 1), legacy.encode(1.));
        assert_eq!(native.capacity().0, 1, "aliases share the physical target");
        let mut saved = native.snapshot();
        assert_eq!(crate::plugin::tests::allocations(|| {
            for (raw, st) in [(0, -12.), (250_000, -1.5), (500_000, 0.),
                (750_000, 1.5), (775_160, 2.), (1_000_000, 12.), (1_129_961, 24.), (1_221_125, 36.)] {
                let physical = pitch.decode(raw);
                assert_eq!(legacy.decode(raw), physical);
                assert_eq!(legacy.encode(physical), pitch.encode(physical));
                assert!((physical * 12. - st).abs() < 1e-3);
                assert!((pitch.encode(physical) - raw).abs() <= 1);
                assert!(write(&mut settings, pitch, physical));
                assert_eq!(read(&settings, pitch), Some(physical));
                assert_eq!(read(&settings, unipolar), Some(physical));
                assert_eq!(settings[0].mods.mods[0].intensity, 1., "adjacent volume target is unchanged");
                native.capture(pitch, par(id::MOD_TARGET_MP_INTENSITY, 1), raw, physical);
            }
            assert!(!write(&mut settings, pitch, f32::NAN));
            assert!(!write(&mut settings, pitch, f32::INFINITY));
            while !native.refresh(&mut saved, 1) {}
        }), 0);
        let edits = saved.saved();
        assert_eq!(edits.len(), 1);
        assert_eq!((edits[0].par, edits[0].value), (par(id::MOD_TARGET_MP_INTENSITY, 1), 1_221_125));
    }

    #[test]
    fn constant_loop_controls_preserve_physical_cursor_stream_seams_and_clock_without_heap() {
        use crate::{audio::Sample, engine::{Bank, Engine}, import::{Instrument, Loop, Zone}};
        use super::super::map::{FOREVER, LoopMap, PlayMap};
        let assignment = |param: &str, intensity| ModAssignment { name: "Constant".into(),
            source: ModSource::Constant, target: ModTarget::Group(param.into()), intensity,
            invert: false, lag_ms: 0, shaper: None };
        let group = Group { mods: vec![assignment("pan", 0.25),
            assignment("loopStart", 0.1266360729932785),
            assignment("loopLength", -0.9170099496841431)],
            modulators: vec![Modulator { name: "Constant".into(), targets: vec!["Pan".into(),
                "Loop_Start".into(), "Loop_Length".into()], assignments: Some(0), volume_env: false,
                bypassed: false, flex: false, envelope: None, kind: "external".into() }],
            ..Group::default() };
        let par = |id, index| EnginePar { id, group: 0, slot: 0, generic: index };
        let start = Address::resolve(par(id::INTMOD_INTENSITY, 1), std::slice::from_ref(&group)).unwrap();
        let length = Address::resolve(par(id::MOD_TARGET_MP_INTENSITY, 2), std::slice::from_ref(&group)).unwrap();
        assert_eq!(start, Address::resolve(par(id::MOD_TARGET_MP_INTENSITY, 1), std::slice::from_ref(&group)).unwrap());
        assert_eq!(length, Address::resolve(par(id::INTMOD_INTENSITY, 2), std::slice::from_ref(&group)).unwrap());
        // Independent actual stored UI/timing + original inverse lookup produces
        // these normalized writes, not values fitted from the saved depths.
        assert!((start.decode(751_086) - 0.1266360729932785).abs() < 1e-7);
        assert!((length.decode(14_233) + 0.9170099496841431).abs() < 1e-7);
        assert!((start.encode(0.1266360729932785) - 751_086).abs() <= 1);
        assert!((length.encode(-0.9170099496841431) - 14_233).abs() <= 1);
        let saved = PlayMap { start: 0, end: 4096, reverse: false,
            looped: Some(LoopMap { start: 0, end: 4096, xfade: 32, until_release: true, alternating: false }) };
        let initial = saved.controlled_loop([0.1266360729932785, -0.9170099496841431], 4096).unwrap();
        assert_eq!(initial.looped.unwrap(), LoopMap { start: 518, end: 858, xfade: 32,
            until_release: true, alternating: false });
        for (offsets, bounds) in [([-0.125, -0.125], (0, 3584)), ([0.125, 0.125], (0, 4096)),
            ([0., -1.], (0, 4)), ([0.5, -1.], (2048, 2052))] {
            let l = saved.controlled_loop(offsets, 4096).unwrap().looped.unwrap();
            assert_eq!((l.start, l.end), bounds);
            assert!(l.xfade <= l.start && l.xfade < l.end - l.start);
        }
        assert!((PlayMap { reverse: true, ..saved }).controlled_loop([0.; 2], 4096).is_none());
        assert!((PlayMap { start: 1, ..saved }).controlled_loop([0.; 2], 4096).is_none());
        let alternate = PlayMap { looped: Some(LoopMap { alternating: true, ..saved.looped.unwrap() }), ..saved };
        assert!(alternate.controlled_loop([0.; 2], 4096).is_none());
        assert!(saved.controlled_loop([f32::MAX, 0.], 4096).is_none());
        assert!(saved.controlled_loop([f32::NAN, 0.], 4096).is_none());
        let mut unsupported = group.clone(); unsupported.source_mode = Some(3);
        assert!(!ModTable::from(&unsupported).has_loop_controls());
        unsupported.source_mode = None;
        assert!(!ModTable::from(&unsupported).has_loop_controls(), "old cache must not invent a source mode");
        let frames: Vec<_> = (0..4096).map(|n| [(n as f32 * 0.031).sin() * 0.2; 2]).collect();
        let path = std::env::temp_dir().join(format!("kontakto-loop-controls-{}.wav", std::process::id()));
        let mut wav = b"RIFF".to_vec(); wav.extend((36u32 + 4096 * 8).to_le_bytes());
        wav.extend(b"WAVEfmt "); wav.extend(16u32.to_le_bytes()); wav.extend(3u16.to_le_bytes());
        wav.extend(2u16.to_le_bytes()); wav.extend(48_000u32.to_le_bytes()); wav.extend(384_000u32.to_le_bytes());
        wav.extend(8u16.to_le_bytes()); wav.extend(32u16.to_le_bytes()); wav.extend(b"data");
        wav.extend((4096u32 * 8).to_le_bytes());
        for frame in &frames { for channel in frame { wav.extend(channel.to_le_bytes()); } }
        std::fs::write(&path, wav).unwrap();
        let zone = Zone { sample: path.clone(), tune: 1.37,
            loop_range: Some(Loop { start: 0, end: 4096, crossfade: 32, until_release: true, alternating: false }),
            ..Zone::default() };
        let instrument = Instrument { groups: vec![group.clone()], zones: vec![zone.clone()], ..Default::default() };
        let bare = Bank::load_bare(&instrument).unwrap();
        assert_eq!(bare.samples[0].frames, 4096);
        assert_eq!(bare.samples[0].spans[0].data.len(), 0);
        let ram = Bank::from_samples(vec![group], vec![zone], vec![(path.clone(), Sample { rate: 48_000, frames })]).unwrap();
        let create = |bank| {
            let mut e = Engine::default(); e.attack = 0.; e.release = 1.; e.blocking_streams = true;
            e.set_bank(Some(Box::new(bank))); e
        };
        let (mut a, mut b) = (create(ram), create(bare));
        let (mut al, mut ar, mut bl, mut br) = ([0.; 128], [0.; 128], [0.; 128], [0.; 128]);
        let render = |a: &mut Engine, b: &mut Engine, al: &mut [f32], ar: &mut [f32], bl: &mut [f32], br: &mut [f32]| {
            a.render(al, ar); b.render(bl, br);
            assert_eq!(al, bl, "stream configuration shares the resident physical path");
            assert_eq!(ar, br);
            assert!(al.iter().chain(ar.iter()).all(|x| x.is_finite()));
        };
        assert_eq!(crate::plugin::tests::allocations(|| {
            a.note_on(0, 60, 100); b.note_on(0, 60, 100);
            assert_eq!(a.player.voices[0].map, initial, "saved crossfade survives start modulation");
            assert_eq!(b.player.voices[0].map, initial);
            for _ in 0..10 { render(&mut a, &mut b, &mut al, &mut ar, &mut bl, &mut br); }
            render(&mut a, &mut b, &mut al[..13], &mut ar[..13], &mut bl[..13], &mut br[..13]);
            for e in [&mut a, &mut b] {
                let settings = &mut e.bank.as_mut().unwrap().settings;
                assert!(write(settings, start, 0.125)); assert!(write(settings, length, -0.875));
                assert_eq!(read(settings, length), Some(-0.875));
                assert_eq!(settings[0].mods.mods[0].intensity, 0.25, "adjacent target stays unchanged");
                e.player.touch();
            }
            render(&mut a, &mut b, &mut al[..19], &mut ar[..19], &mut bl[..19], &mut br[..19]);
            assert_eq!(a.player.voices[0].map, initial, "fragment beginning at tick13 must finish the old step");
            let v = &a.player.voices[0];
            let physical = v.map.run(v.pos as u64, v.wraps).unwrap().frame as f64 + v.pos.fract();
            render(&mut a, &mut b, &mut al, &mut ar, &mut bl, &mut br);
            let v = &a.player.voices[0];
            assert_eq!(v.map.looped.unwrap().start, 512);
            assert_eq!(v.map.looped.unwrap().end, 1024);
            assert!((v.pos - physical - 128. * v.plan.step as f64 / 4294967296.).abs() < 1e-8,
                "a boundary edit preserves physical position and its fraction");
            // The shortened-behind-cursor seam is explicitly deferred once,
            // then retried when the old loop returns to an eligible position.
            for e in [&mut a, &mut b] { assert!(write(&mut e.bank.as_mut().unwrap().settings, length, -0.9)); }
            render(&mut a, &mut b, &mut al, &mut ar, &mut bl, &mut br);
            for _ in 0..8 { render(&mut a, &mut b, &mut al, &mut ar, &mut bl, &mut br); }
            assert_eq!(a.player.voices[0].loop_deferred_edits, 1);
            assert!(a.player.voices[0].loop_deferred.is_none(), "eligible control tick retries without a new write");
            let map = a.player.voices[0].map;
            a.note_off(0, 60); b.note_off(0, 60);
            assert_ne!(a.player.voices[0].wraps, FOREVER);
            for e in [&mut a, &mut b] { assert!(write(&mut e.bank.as_mut().unwrap().settings, start, -0.5)); }
            render(&mut a, &mut b, &mut al, &mut ar, &mut bl, &mut br);
            assert_eq!(a.player.voices[0].map, map, "release keeps the final seam and tail");
            for _ in 0..30 { render(&mut a, &mut b, &mut al, &mut ar, &mut bl, &mut br); }
            assert_eq!(a.active_voices(), 0); assert_eq!(b.active_voices(), 0);
        }), 0);
        assert_eq!(b.underruns(), 0);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn saved_pitch_lfo_and_constant_loop_are_partition_invariant_in_ram_and_stream() {
        use crate::{audio::Sample, engine::{Bank, Engine}, import::{Instrument, Loop, PitchLfo, Zone}};
        let mut group = Group { pitch_lfos: vec![PitchLfo { slot: 7, count: 1.,
            note_value: 1. / 24., sine: 0.5, fade_ms: 0., depth: 0.5, targets: vec![], bypassed: false }],
            mods: ["loopStart", "loopLength"].into_iter().map(|param| ModAssignment {
                name: "Constant".into(), source: ModSource::Constant,
                target: ModTarget::Group(param.into()), intensity: 0., invert: false,
                lag_ms: 0, shaper: None }).collect(),
            modulators: vec![Modulator { name: "Constant".into(),
                targets: vec!["Loop_Start".into(), "Loop_Length".into()], assignments: Some(0),
                volume_env: false, bypassed: false, flex: false, envelope: None,
                kind: "external".into() }], ..Group::default() };
        let length = Address::resolve(EnginePar { id: id::INTMOD_INTENSITY,
            group: 0, slot: 0, generic: 1 }, std::slice::from_ref(&group)).unwrap();
        // The saved oscillator is audible, but live LFO target writes remain
        // unsupported. Do not accept a write into metadata while leaving its
        // prepared depth unchanged; both Host and ScriptSetup use this gate.
        group.modulators.resize_with(8, || Modulator { name: String::new(), targets: vec![],
            assignments: None, volume_env: false, bypassed: false, flex: false,
            envelope: None, kind: "unsupported".into() });
        group.modulators[7] = Modulator { name: "LFO_Pitch_Retrigger".into(),
            targets: vec!["Pitch".into()], assignments: None, volume_env: false,
            bypassed: false, flex: false, envelope: None, kind: "lfo".into() };
        for id in [id::INTMOD_INTENSITY, id::MOD_TARGET_MP_INTENSITY, id::MOD_TARGET_INTENSITY] {
            let par = EnginePar { id, group: 0, slot: 7, generic: 0 };
            assert!(Address::resolve(par, std::slice::from_ref(&group)).is_none());
            assert!(!Address::inert(par, std::slice::from_ref(&group)));
        }
        let frames: Vec<_> = (0..4096).map(|n| [(n as f32 * 0.031).sin() * 0.2; 2]).collect();
        let path = std::env::temp_dir().join(format!("kontakto-loop-lfo-{}.wav", std::process::id()));
        let mut wav = b"RIFF".to_vec(); wav.extend((36u32 + 4096 * 8).to_le_bytes());
        wav.extend(b"WAVEfmt "); wav.extend(16u32.to_le_bytes()); wav.extend(3u16.to_le_bytes());
        wav.extend(2u16.to_le_bytes()); wav.extend(48_000u32.to_le_bytes()); wav.extend(384_000u32.to_le_bytes());
        wav.extend(8u16.to_le_bytes()); wav.extend(32u16.to_le_bytes()); wav.extend(b"data");
        wav.extend((4096u32 * 8).to_le_bytes());
        for frame in &frames { for channel in frame { wav.extend(channel.to_le_bytes()); } }
        std::fs::write(&path, wav).unwrap();
        let zone = Zone { sample: path.clone(), tune: 1.37,
            loop_range: Some(Loop { start: 0, end: 4096, crossfade: 32,
                until_release: true, alternating: false }), ..Zone::default() };
        let create = |bank| {
            let mut e = Engine::default(); e.attack = 0.; e.release = 0.001;
            e.blocking_streams = true; e.set_transport(false, 120., 0., (4, 4));
            e.set_bank(Some(Box::new(bank))); e
        };
        let ram = |group: Group| Bank::from_samples(vec![group], vec![zone.clone()],
            vec![(path.clone(), Sample { rate: 48_000, frames: frames.clone() })]).unwrap();
        let mut full = create(ram(group.clone()));
        let mut split = create(ram(group.clone()));
        let mut stream = create(Bank::load_bare(&Instrument { groups: vec![group.clone()],
            zones: vec![zone.clone()], ..Default::default() }).unwrap());
        group.mods.clear(); group.modulators.clear();
        let mut no_controls = create(ram(group));
        assert!(full.bank().unwrap().has_native_controls());
        assert!(!no_controls.bank().unwrap().has_native_controls());
        let (mut a, mut ar, mut b, mut br, mut c, mut cr, mut d, mut dr) =
            ([0.; 128], [0.; 128], [0.; 128], [0.; 128], [0.; 128], [0.; 128], [0.; 128], [0.; 128]);
        let equal = |a: &[f32], b: &[f32]| assert!(a.iter().zip(b)
            .all(|(a, b)| (a - b).abs() < 1e-7), "planner partition changed PCM");
        assert_eq!(crate::plugin::tests::allocations(|| {
            for e in [&mut full, &mut split, &mut stream, &mut no_controls] { e.note_on(0, 60, 127); }
            full.render(&mut a, &mut ar);
            no_controls.render(&mut d, &mut dr);
            for (from, to) in [(0, 17), (17, 128)] {
                split.render(&mut b[from..to], &mut br[from..to]);
                stream.render(&mut c[from..to], &mut cr[from..to]);
            }
            equal(&a, &b); equal(&ar, &br); equal(&a, &c); equal(&ar, &cr);
            equal(&a, &d); equal(&ar, &dr);
            assert!(full.player.voices[0].pos < 4096., "loop has not been reached");
            // Same command at note frame141; only fragment boundaries differ.
            for (e, l, r) in [(&mut full, &mut a, &mut ar), (&mut split, &mut b, &mut br),
                (&mut stream, &mut c, &mut cr)] { e.render(&mut l[..13], &mut r[..13]); }
            let old = full.player.voices[0].map;
            for e in [&mut full, &mut split, &mut stream] {
                assert!(write(&mut e.bank.as_mut().unwrap().settings, length, -0.5));
                assert_eq!(read(&e.bank().unwrap().settings, length), Some(-0.5));
                e.player.touch();
            }
            full.render(&mut a[13..], &mut ar[13..]);
            for (from, to) in [(13, 32), (32, 128)] {
                split.render(&mut b[from..to], &mut br[from..to]);
                stream.render(&mut c[from..to], &mut cr[from..to]);
                if to == 32 {
                    assert_eq!(split.player.voices[0].map, old, "edit waits for native tick160");
                }
            }
            equal(&a, &b); equal(&ar, &br); equal(&a, &c); equal(&ar, &cr);
            for e in [&mut full, &mut split, &mut stream] {
                assert_eq!(e.player.voices[0].map.looped.unwrap().end, 2048);
                e.note_off(0, 60);
            }
            for _ in 0..16 {
                full.render(&mut a, &mut ar); split.render(&mut b, &mut br); stream.render(&mut c, &mut cr);
                equal(&a, &b); equal(&ar, &br); equal(&a, &c); equal(&ar, &cr);
            }
            assert_eq!(full.active_voices(), 0); assert_eq!(split.active_voices(), 0);
            assert_eq!(stream.active_voices(), 0, "source clock cannot extend release lifetime");
        }), 0);
        assert_eq!(stream.underruns(), 0);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn live_saved_pitch_lfo_targets_preserve_native_identity_phase_and_persistence_without_heap() {
        use crate::{audio::Sample, engine::{Bank, Engine, load_scripts, load_scripts_with_state},
            import::{Instrument, PitchLfo, Zone}, ksp::Value};
        let mut group = Group { pitch_lfos: vec![
            PitchLfo { slot: 7, count: 1., note_value: 1. / 24., sine: 0.5, fade_ms: 0.,
                depth: 0.2, targets: vec![(1, 0.25), (3, -0.05)], bypassed: false },
            PitchLfo { slot: 3, count: 2., note_value: 1. / 24., sine: 0.1, fade_ms: 0.,
                depth: 0.1, targets: vec![(1, 0.1)], bypassed: false }], ..Group::default() };
        group.modulators.resize_with(8, || Modulator { name: String::new(), targets: vec![],
            assignments: None, volume_env: false, bypassed: false, flex: false,
            envelope: None, kind: "unsupported".into() });
        for slot in [3, 7] { group.modulators[slot] = Modulator { name: format!("LFO{slot}"),
            targets: vec!["Cutoff".into(), "PitchA".into(), "Volume".into(), "PitchB".into()],
            assignments: None, volume_env: false, bypassed: false, flex: false,
            envelope: None, kind: "lfo".into() }; }
        let par = |id, slot, target| EnginePar { id, group: 0, slot, generic: target };
        let address = |id, slot, target| Address::resolve(par(id, slot, target), std::slice::from_ref(&group)).unwrap();
        for id in [id::INTMOD_INTENSITY, id::MOD_TARGET_MP_INTENSITY, id::MOD_TARGET_INTENSITY] {
            assert!(Address::resolve(par(id, 7, 0), std::slice::from_ref(&group)).is_none(), "adjacent cutoff remains unsupported");
            assert!(Address::resolve(par(id, 3, 3), std::slice::from_ref(&group)).is_none(), "target indices belong to their own source");
        }
        let source = "on init
            declare $initial := get_engine_par($ENGINE_PAR_INTMOD_INTENSITY,0,7,1)
            make_persistent($initial)
            declare $after := 0
            make_persistent($after)
            end on
            on controller
            if (%CC[1] > 0)
                set_engine_par($ENGINE_PAR_MOD_TARGET_INTENSITY,250000,0,7,1)
                set_engine_par($ENGINE_PAR_INTMOD_INTENSITY,750000,0,7,1)
                set_engine_par($ENGINE_PAR_MOD_TARGET_MP_INTENSITY,775160,0,7,1)
                set_engine_par($ENGINE_PAR_INTMOD_INTENSITY,250000,0,7,3)
                set_engine_par($ENGINE_PAR_MOD_TARGET_INTENSITY,250000,0,3,1)
                $after := get_engine_par($ENGINE_PAR_INTMOD_INTENSITY,0,7,1)
            end if
            end on";
        let instrument = Instrument { groups: vec![group.clone()], scripts: vec![source.into()], ..Default::default() };
        let create = || {
            let sample = Sample { rate: 48_000,
                frames: (0..4096).map(|n| [(n as f32 * 0.031).sin() * 0.2; 2]).collect() };
            let mut e = Engine::default(); e.attack = 0.; e.release = 0.001;
            e.set_transport(false, 120., 0., (4, 4));
            e.set_bank(Some(Box::new(Bank::from_samples(vec![group.clone()], vec![Zone::default()],
                vec![(std::path::PathBuf::new(), sample)]).unwrap()))); e
        };
        let (runtime, errors) = load_scripts(&instrument, vec![], 48_000.);
        assert!(errors.is_empty(), "{errors:?}");
        let (mut live, mut reference) = (create(), create()); live.set_script(runtime);
        let mut snapshot = live.script().unwrap().native_state.snapshot();
        let (mut a, mut ar, mut b, mut br) = ([0.; 128], [0.; 128], [0.; 128], [0.; 128]);
        let equal = |a: &[f32], b: &[f32]| assert!(a.iter().zip(b)
            .all(|(a, b)| (a - b).abs() < 1e-7), "live depth write must preserve elapsed source phase");
        assert_eq!(crate::plugin::tests::allocations(|| {
            live.note_on(0, 60, 127); reference.note_on(0, 60, 127);
            live.render(&mut a[..13], &mut ar[..13]); reference.render(&mut b[..13], &mut br[..13]);
            equal(&a[..13], &b[..13]);
            live.cc(0, 1, 127);
            // Independent physical-depth reference, without setters or source reset.
            let lfos = &mut reference.bank.as_mut().unwrap().settings[0].pitch_lfos;
            lfos[0].targets[0].1 = 0.16666558385; lfos[0].targets[1].1 = -0.125;
            lfos[0].depth = 0.16666558385 - 0.125;
            lfos[1].targets[0].1 = 0.0625; lfos[1].depth = 0.0625;
            for _ in 0..8 {
                live.render(&mut a, &mut ar); reference.render(&mut b, &mut br);
                equal(&a, &b); equal(&ar, &br);
            }
            let settings = &live.bank().unwrap().settings;
            assert_eq!(read(settings, address(id::INTMOD_INTENSITY, 7, 3)), Some(-0.125));
            assert_eq!(read(settings, address(id::MOD_TARGET_INTENSITY, 3, 1)), Some(0.0625));
            assert!((settings[0].pitch_lfos[0].depth - (0.16666558385 - 0.125)).abs() < 1e-7);
            assert!(!write(&mut live.bank.as_mut().unwrap().settings, address(id::INTMOD_INTENSITY, 7, 1), f32::NAN));
            while !live.script().unwrap().native_state.refresh(&mut snapshot, 1) {}
            live.note_off(0, 60); reference.note_off(0, 60);
            for _ in 0..16 { live.render(&mut a, &mut ar); reference.render(&mut b, &mut br); }
            assert_eq!(live.active_voices(), 0); assert_eq!(reference.active_voices(), 0);
        }), 0);
        let rt = live.script().unwrap();
        for (id, slot, target) in [(id::MOD_TARGET_INTENSITY, 7, 1),
            (id::INTMOD_INTENSITY, 7, 1), (id::MOD_TARGET_MP_INTENSITY, 7, 1),
            (id::INTMOD_INTENSITY, 7, 3), (id::MOD_TARGET_INTENSITY, 3, 1)] {
            assert!(rt.env.engine_par(par(id, slot, target)).is_none(),
                "accepted aliases must not use the unsupported stored-value fallback");
        }
        let Value::Int(after) = rt.persistence()[0]["$after"] else { panic!("missing readback"); };
        assert!((after - 775160).abs() <= 1);
        let edits = snapshot.saved();
        assert_eq!(edits.len(), 3, "aliases share a physical source/target record");
        assert!(edits.iter().any(|e| e.par == par(id::MOD_TARGET_MP_INTENSITY, 7, 1) && e.value == 775160));
        assert!(edits.iter().any(|e| e.par == par(id::INTMOD_INTENSITY, 7, 3) && e.value == 250000));
        assert!(edits.iter().any(|e| e.par == par(id::MOD_TARGET_INTENSITY, 3, 1) && e.value == 250000));
        let (restored, errors) = load_scripts_with_state(&instrument, vec![], 48_000., &[], &edits);
        assert!(errors.is_empty(), "{errors:?}");
        let mut replay = create(); replay.set_script(restored);
        assert_eq!(replay.bank().unwrap().settings[0].pitch_lfos[0].targets, live.bank().unwrap().settings[0].pitch_lfos[0].targets);
        assert_eq!(replay.bank().unwrap().settings[0].pitch_lfos[1].targets, live.bank().unwrap().settings[0].pitch_lfos[1].targets);
        let Value::Int(initial) = replay.script().unwrap().persistence()[0]["$initial"] else { panic!("missing seeded readback"); };
        assert!((initial - 775160).abs() <= 1, "restoration seeds init getters before callbacks");
    }

    #[test]
    fn signed_intensity_aliases_reach_external_pitch_and_cutoff_pcm_without_heap() {
        use crate::{audio::Sample, engine::{Bank, Engine, ScriptSetup}, import::{Instrument, Zone},
            ksp::{Runtime, Value}};
        let assignment = |target, intensity| ModAssignment { name: "Constant".into(),
            source: ModSource::Constant, target, intensity, invert: false, lag_ms: 0, shaper: None };
        let group = Group {
            mods: vec![assignment(ModTarget::Volume, 1.), assignment(ModTarget::Pitch, 0.),
                assignment(ModTarget::Module { param: "filterCutoff".into(), slot: 0 }, 0.)],
            modulators: vec![Modulator { name: "Constant".into(), targets: vec!["Volume".into(),
                "Pitch".into(), "Cutoff".into()], assignments: Some(0), volume_env: false,
                bypassed: false, flex: false, envelope: None, kind: "external".into() }],
            fx: crate::fx::Chain { slots: vec![crate::fx::Effect { slot: 0, kind: crate::fx::Kind::Filter,
                version: 0, bypass: false, output_gain: 1., dry_level: 1.,
                params: crate::fx::params::Params::Filter(crate::fx::params::Filter {
                    filter_type: 2, cutoff: 0.3, resonance: 0., extra: [0.; 3] }) }] },
            ..Group::default()
        };
        let render = |group: Group, source: &str, pitch_raw: i32, cutoff_raw: i32| {
            let instrument = Instrument { groups: vec![group.clone()], ..Default::default() };
            let mut setup = ScriptSetup::new(&instrument, 48_000.);
            let (rt, errors) = Runtime::with_scripts(&[source], &mut setup, 0, Vec::new());
            assert!(errors.iter().all(Option::is_none), "{errors:?}");
            for (name, expected) in [("$p", pitch_raw), ("$c", cutoff_raw)] {
                let Value::Int(raw) = rt.persistence()[0][name] else { panic!("missing readback"); };
                assert!((raw - expected).abs() <= 1, "{name}: {raw} != {expected}");
            }
            let sample = Sample { rate: 48_000, frames: (0..8192).map(|n|
                [(std::f32::consts::TAU * 2000. * n as f32 / 48_000.).sin() * 0.25; 2]).collect() };
            let bank = Bank::from_samples(vec![group], vec![Zone::default()],
                vec![(std::path::PathBuf::new(), sample)]).unwrap();
            let mut engine = Engine::default();
            engine.set_bank(Some(Box::new(bank)));
            engine.set_script(Some(Box::new(rt)));
            let (mut l, mut r) = ([0.; 2048], [0.; 2048]);
            assert_eq!(crate::plugin::tests::allocations(|| {
                engine.note_on(0, 60, 100);
                engine.render(&mut l, &mut r);
            }), 0);
            assert!(l.iter().chain(&r).all(|x| x.is_finite()));
            l
        };
        let mut power = [0.; 4];
        for (i, (pitch_raw, cutoff_raw)) in [(224_840, 250_000), (224_840, 750_000),
            (775_160, 250_000), (775_160, 750_000)].into_iter().enumerate() {
            // Independently saved Conflux 2 st target; signed midpoint probes
            // also exercise an audible cutoff change rather than readback alone.
            let readback = "declare $p := get_engine_par($ENGINE_PAR_MOD_TARGET_MP_INTENSITY,0,0,1)\nmake_persistent($p)\ndeclare $c := get_engine_par($ENGINE_PAR_INTMOD_INTENSITY,0,0,2)\nmake_persistent($c)\nend on";
            let source = |name| format!("on init\nset_engine_par({name},{pitch_raw},0,0,1)\nset_engine_par({name},{cutoff_raw},0,0,2)\n{readback}\non note\nset_engine_par({name},{pitch_raw},0,0,1)\nset_engine_par({name},{cutoff_raw},0,0,2)\nend on");
            let legacy = render(group.clone(), &source("$ENGINE_PAR_INTMOD_INTENSITY"), pitch_raw, cutoff_raw);
            let modern = render(group.clone(), &source("$ENGINE_PAR_MOD_TARGET_MP_INTENSITY"), pitch_raw, cutoff_raw);
            let mut reference = group.clone();
            reference.mods[1].intensity = if pitch_raw < 500_000 { -0.16666558385 } else { 0.16666558385 };
            reference.mods[2].intensity = if cutoff_raw < 500_000 { -0.125 } else { 0.125 };
            let reference = render(reference, &format!("on init\n{readback}"), pitch_raw, cutoff_raw);
            // Authored serialized target records exercise the import path too:
            // magnitudes are positive; the negative cutoff is target flag0x02,
            // independently of the serialized invert button (left false).
            use ni_file::kontakt::{Chunk, StructuredObject};
            let name = |out: &mut Vec<u8>, text: &str| {
                out.extend((text.len() as u32).to_le_bytes()); out.extend(text.as_bytes());
            };
            let object = |id, version: u16, private: &[u8], public: &[u8]| {
                let mut data = vec![1]; data.extend(version.to_le_bytes());
                for part in [private, public, &[]] {
                    data.extend((part.len() as u32).to_le_bytes()); data.extend(part);
                }
                Chunk { id, data }
            };
            let mut private = 3u32.to_le_bytes().to_vec();
            for (param, label, slot, depth, flags) in [
                ("volume", "Volume", None, 1.0f32, 0x10),
                ("pitch", "Pitch", None, 0.16666558385, if pitch_raw < 500_000 { 0x12 } else { 0x10 }),
                ("filterCutoff", "Cutoff", Some(0), 0.125, if cutoff_raw < 500_000 { 0x12 } else { 0x10 })] {
                name(&mut private, param); private.extend(depth.to_le_bytes());
                private.extend((-1i16).to_le_bytes()); private.push(flags); private.extend(0u16.to_le_bytes());
                name(&mut private, label); private.extend(slot); private.push(0);
            }
            private.extend([0; 3]); name(&mut private, "Constant");
            private.extend(1u32.to_le_bytes()); private.extend(9u32.to_le_bytes());
            private.extend([0; 4]); private.extend(0u32.to_le_bytes());
            let mut public = vec![1]; object(0x0c, 0x102, &private, &[]).write(&mut public).unwrap();
            public.extend([0; 31]);
            let raw = ni_file::kontakt::objects::Group(StructuredObject { version: 0x95,
                public_data: vec![], private_data: vec![], children: vec![object(0x3c, 0x10, &[], &public)] });
            let imported = crate::modulation::read_group(&raw).unwrap();
            let mut saved = group.clone(); saved.mods = imported.mods; saved.modulators = imported.modulators;
            let saved = render(saved, &format!("on init\n{readback}"), pitch_raw, cutoff_raw);
            assert_eq!(legacy, modern, "aliases must produce identical PCM");
            assert_eq!(saved, reference, "saved signed targets must match the physical-depth reference");
            assert!(legacy.iter().zip(reference).all(|(a,b)| (a-b).abs() < 1e-6),
                "signed setters must match independently supplied physical depths");
            power[i] = legacy.iter().map(|&x| f64::from(x).powi(2)).sum::<f64>();
        }
        assert!(power[1] > power[0] * 4. && power[3] > power[2] * 4.,
            "cutoff depth changes the real filter response at both pitch directions: {power:?}");
    }

    #[test]
    fn addresses_resolve_by_decoded_names() {
        let groups = [group()];
        assert_eq!(find_mod(&groups, 0, &|n| n == "CC_VOLUME"), Some(2));
        assert_eq!(
            find_target(&groups, 0, 0, &|n| n == "ENV_AHDSR_VOLUME"),
            Some(0)
        );
        let par = |id, slot, generic| EnginePar {
            id,
            group: 0,
            slot,
            generic,
        };
        assert_eq!(
            Address::resolve(par(id::MOD_TARGET_INTENSITY, 2, -1), &groups),
            Some(Address::Intensity {
                group: 0,
                index: 1,
                bipolar: false,
                cubic: None,
            })
        );
        assert_eq!(
            Address::resolve(par(id::ATTACK, 0, -1), &groups),
            Some(Address::Envelope(0, Stage::Attack))
        );
        assert_eq!(Address::resolve(par(id::ATTACK, 1, -1), &groups), None);
        // A flex envelope (slot 1 here) has no AHDSR stages, nor has a
        // missing slot: Kontakt ignores those; an external modulator's are
        // left unmapped.
        let mut flex = group();
        flex.modulators.insert(
            1,
            Modulator {
                name: "ENV_FLEX".into(),
                targets: Vec::new(),
                assignments: None,
                volume_env: false,
                bypassed: false,
                flex: true,
                envelope: None,
                kind: "flex".into(),
            },
        );
        let flex = [flex];
        assert!(Address::inert(par(id::ATTACK, 1, -1), &flex));
        assert!(Address::inert(par(id::RELEASE, 9, -1), &flex));
        assert!(!Address::inert(par(id::ATTACK, 2, -1), &flex));
        assert!(!Address::inert(par(id::VOLUME, 1, -1), &flex));
        // A filter envelope is addressed by its `Group::envelopes` index.
        let mut filter = group();
        filter.modulators[1].envelope = Some(3);
        assert_eq!(
            Address::resolve(par(id::DECAY, 1, -1), &[filter]),
            Some(Address::ModEnvelope(0, 3, Stage::Decay))
        );
        let bus = EnginePar {
            id: id::VOLUME,
            group: -1,
            slot: -1,
            generic: 1002,
        };
        assert_eq!(
            Address::resolve(bus, &groups),
            Some(Address::Fx(Rack::Bus(2), 0, FxParam::Volume))
        );
    }
}
