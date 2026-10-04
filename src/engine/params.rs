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
                    env: { let mut p = Ahdsr::from(&e.env); p.ahd_only = e.native_ahd_only; p },
                    bypass: group
                        .modulators
                        .iter()
                        .find(|m| m.envelope == Some(index))
                        .is_some_and(|m| m.bypassed),
                    index: index as u8,
                    targets,
                })
            })
            .collect()
    }

    fn level_pitch(&self, level: f32) -> f32 {
        if self.bypass {
            return 0.;
        }
        self.targets
            .iter()
            .map(|(_, sign, m)| 12. * sign * m.intensity * m.shape(level))
            .sum()
    }

    /// Add pitch while the source is enabled; bypass preserves its clock.
    pub(crate) fn render_add(&self, state: &mut Envelope, out: &mut [f32], rate: f32) {
        if self.bypass { return; }
        for pitch in out {
            *pitch += self.level_pitch(state.pitch_frame(rate));
        }
    }

    #[cfg(test)]
    pub(crate) fn pitch(&self, state: &mut Envelope, frames: usize, rate: f32) -> f32 {
        if !self.bypass { state.skip(frames, None, rate); }
        self.level_pitch(state.level())
    }
}

/// Modulation source the engine models.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    Velocity,
    ReleaseVelocity,
    Key,
    Constant,
    Cc(u8),
    Script(u16),
    Bend,
    Pressure,
    /// Release-trigger counter, fixed when the release voice starts.
    Counter,
}

impl Source {
    fn of(source: ModSource) -> Option<Self> {
        Some(match source {
            ModSource::Velocity => Self::Velocity,
            ModSource::ReleaseVelocity => Self::ReleaseVelocity,
            ModSource::KeyPosition => Self::Key,
            ModSource::Constant => Self::Constant,
            ModSource::MidiCc(cc) if cc < 128 => Self::Cc(cc),
            ModSource::PitchBend => Self::Bend,
            ModSource::MonoAftertouch => Self::Pressure,
            ModSource::ReleaseTriggerCounter => Self::Counter,
            ModSource::Script(n) if n < super::SCRIPT_MOD_SOURCES as u32 => Self::Script(n as u16),
            _ => return None,
        })
    }

    /// Changes while a voice plays (otherwise fixed at the note start).
    fn live(self) -> bool {
        matches!(
            self,
            Self::Cc(_) | Self::Bend | Self::Pressure | Self::ReleaseVelocity | Self::Script(_)
        )
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
    cc_default: Option<i16>,
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
            ModTarget::Group(ref p)
                if m.source == ModSource::Constant
                    && m.lag_ms == 0
                    && m.shaper.is_none()
                    && !m.invert =>
            {
                match p.as_str() {
                    "loopStart" => Some(Target::LoopStart),
                    "loopLength" => Some(Target::LoopLength),
                    _ => None,
                }
            }
            ModTarget::Group(_) => None,
        };
        Self {
            route: Source::of(m.source).zip(target),
            cc_default: m.cc_default,
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
        // From-script values are signed; the EX variant is deliberately unbounded.
        // A missing shaper must preserve that value rather than clip it to a CC.
        if self.curve.is_none() && self.route.is_some_and(|(source,_)|matches!(source,Source::Script(_))) {
            return x;
        }
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
    /// Availability precedes shaping, lag and destination binding.
    fn read_value(&self, input: &Inputs, bend_pitch: Option<f32>) -> Option<f32> {
        let (source, target) = self.route?;
        if let Source::Cc(cc) = source
            && let Some(default) = self.cc_default
            && !input.controller_received(cc) {
            return (default >= 0).then(|| f32::from(default) / 127.);
        }
        Some(input.read_mod(source, target, bend_pitch))
    }

    pub(crate) fn start_value(&self, input: &Inputs) -> Option<f32> {
        self.read_value(input, None).map(|x| self.shape(x))
    }

    /// Static sources retain their note-start capture across planning fragments.
    pub(crate) fn live_value(&self, input: &Inputs) -> Option<Option<f32>> {
        self.route.filter(|(source, _)| source.live())
            .map(|_| self.start_value(input))
    }

    pub(crate) fn lag_step(&self, rate: f32) -> f64 {
        lag_factor64(self.lag, 1, rate)
    }

}

/// Group modulation assignments in import order (the KSP target addresses),
/// plus which of them a voice evaluates.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModTable {
    pub mods: Box<[Mod]>,
    /// Volume and pitch assignments with a modelled source. Voice storage
    /// is prepared from this roster; script setup retains every address.
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
        let mods: Box<[Mod]> = group
            .mods
            .iter()
            .map(|m| {
                let mut prepared = Mod::from(m);
                if matches!(
                    prepared.route,
                    Some((_, Target::LoopStart | Target::LoopLength))
                ) && !loop_control_supported(group, m)
                {
                    prepared.route = None;
                }
                prepared
            })
            .collect();
        let routed = |want: fn(Target) -> bool| {
            (0..mods.len() as u16)
                .filter(|&i| mods[i as usize].route.is_some_and(|(_, t)| want(t)))
                .collect::<Vec<_>>()
        };
        let voiced = routed(|t| matches!(t, Target::Volume | Target::Pitch));
        Self {
            voiced: voiced.into(),
            starts: routed(|t| t == Target::Start).into(),
            times: routed(|t| matches!(t, Target::Attack | Target::Release)).into(),
            loops: routed(|t| matches!(t, Target::LoopStart | Target::LoopLength)).into(),
            mods,
        }
    }
}

/// Validate the fixed voice state before preparing or installing a bank.
pub(crate) fn validate_modulation_capacity(group: &Group) -> anyhow::Result<()> {
    anyhow::ensure!(group.volume_env.iter().chain(group.envelopes.iter().map(|source| &source.env))
        .all(|source| [source.attack_curve, source.attack_ms, source.hold_ms, source.decay_ms,
            source.sustain, source.release_ms].into_iter().all(f32::is_finite)),
        "Authored AHDSR envelope source contains a nonfinite value");
    anyhow::ensure!(
        group.mods.len() <= u16::MAX as usize,
        "modulation assignment count {} exceeds the 65535-address engine limit",
        group.mods.len()
    );
    for (index, envelope) in group.envelopes.iter().enumerate() {
        anyhow::ensure!(
            envelope.targets.len() <= u16::MAX as usize,
            "envelope {index} has {} targets, exceeding the 65535-address engine limit",
            envelope.targets.len()
        );
    }
    anyhow::ensure!(group.mods.iter().all(|m| m.cc_default.is_none_or(|value| value <= 127)),
        "CC modulation defaults above127 are unsupported");
    super::lfo::validate_sources(&group.pitch_lfos, &group.volume_lfos)?;
    super::glide::validate_sources(&group.pitch_glides, &group.pitch_lfos, &group.volume_lfos)
}

/// Performance state a voice's modulation reads.
#[derive(Clone, Copy)]
pub(crate) struct Inputs<'a> {
    pub cc: &'a [crate::midi::ControlValue; 128],
    pub cc_received: Option<&'a [u64; 2]>,
    /// Native registered controllers retained by this note's physical owner.
    pub per_note_cc: Option<&'a [Option<crate::midi::ControlValue>; 128]>,
    pub script_mods: &'a [i32],
    /// Combined MPE CC74, preserving a released note's member value.
    pub cc74: Option<f32>,
    /// The composed MPE source may have a frozen, unreceived member value.
    pub cc74_received: Option<bool>,
    /// -1..=1.
    pub bend: f32,
    /// Normalized pressure selected from physical and exact-note controls.
    pub pressure: f32,
    pub note: u8,
    pub velocity: crate::midi::Velocity,
    /// Captured key-up value; None remains an unreceived source.
    pub release_velocity: Option<crate::midi::Velocity>,
    /// Release-trigger counter. Compatibility is 0..=1; the raw Native
    /// admission scalar can exceed unity after a negative span-offset reset.
    /// Destination shaping still applies its established normalized range.
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

pub(crate) fn controller_received(received: &[u64; 2], cc: u8) -> bool {
    received[cc as usize / 64] & (1 << (cc & 63)) != 0
}

impl Inputs<'_> {
    fn controller_received(&self, cc: u8) -> bool {
        if cc == 74 {
            if self.cc74.is_some() && self.cc74_received.unwrap_or(true) { return true; }
            if self.per_note_cc.and_then(|values| values[74]).is_some() { return true; }
            if let Some(received) = self.cc74_received { return received; }
        } else if self.per_note_cc.and_then(|values| values[cc as usize]).is_some() { return true; }
        self.cc_received.is_none_or(|received| controller_received(received, cc))
    }

    /// Negotiated MPE master pitch is added independently by the voice;
    /// other bend destinations still read the combined controller normally.
    fn read_mod(&self, source: Source, target: Target, bend_pitch: Option<f32>) -> f32 {
        if source == Source::Bend && target == Target::Pitch {
            if let Some(bend) = bend_pitch {
                return (bend + 1.) * 0.5;
            }
        }
        self.read(source)
    }

    /// Unshaped source value; Native RTC and script sources can exceed unity.
    fn read(&self, source: Source) -> f32 {
        match source {
            Source::Velocity => self.velocity.dsp(),
            Source::ReleaseVelocity => self.release_velocity.map_or(0., |v| v.dsp()),
            Source::Key => f32::from(self.note) / 127.0,
            Source::Constant => 1.0,
            Source::Cc(cc) => (if cc == 74 { self.cc74 } else { None })
                .or_else(|| self.per_note_cc.and_then(|values| values[cc as usize]).map(crate::midi::ControlValue::dsp))
                .unwrap_or_else(|| self.cc[cc as usize].dsp()),
            Source::Bend => (self.bend + 1.0) * 0.5,
            Source::Pressure => self.pressure,
            Source::Counter => self.counter,
            Source::Script(n) => self.script_mods.get(n as usize).copied().unwrap_or(0) as f32 / 1_000_000.,
        }
    }
}

/// Native loop consumer is inactive when the sample has a preload-byte budget.
/// Only the independently traced zero-budget mode and Constant source are enabled.
pub(crate) fn loop_control_supported(group: &Group, m: &ModAssignment) -> bool {
    group.source_mode == Some(0)
        && m.source == ModSource::Constant
        && m.lag_ms == 0
        && m.shaper.is_none()
        && !m.invert
        && matches!(&m.target, ModTarget::Group(p) if p == "loopStart" || p == "loopLength")
}

impl ModTable {
    pub(crate) fn has_loop_controls(&self) -> bool {
        !self.loops.is_empty()
    }

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
    pub(crate) fn voiced_count(&self) -> usize { self.voiced.len() }

    pub(super) fn same_routes(&self, other: &Self) -> bool {
        self.voiced == other.voiced && self.starts == other.starts
            && self.times == other.times && self.loops == other.loops
            && self.mods.len() == other.mods.len()
            && self.mods.iter().zip(&other.mods).all(|(a, b)| a.route == b.route && a.cc_default == b.cc_default)
    }

    pub(crate) fn start(&self, values: &mut [Option<f64>], input: &Inputs, bend_pitch: Option<f32>) {
        assert!(values.len() >= self.voiced.len(), "prepared voiced modulation storage");
        values.fill(None);
        for (value, &i) in values.iter_mut().zip(&self.voiced) {
            let m = &self.mods[i as usize];
            *value = m.read_value(input, bend_pitch).map(|x| f64::from(m.shape(x)));
        }
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
        values: &mut [Option<f64>],
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
                let Some(x) = m.read_value(input, bend_pitch).map(|x| m.shape(x)) else {
                    *value = None;
                    continue;
                };
                let value = value.get_or_insert(0.);
                approach64(value, f64::from(x), m.lag, frames, rate);
                settled &= *value == f64::from(x);
            }
            let Some(value) = *value else { continue; };
            let value = value as f32;
            match target {
                Target::Volume => {
                    let v = if m.intensity < 0.0 {
                        1.0 - value
                    } else {
                        value
                    };
                    gain *= 1.0 - m.intensity.abs() * (1.0 - v);
                }
                Target::Pitch => {
                    let v = if source == Source::Bend {
                        value * 2.0 - 1.0
                    } else {
                        value
                    };
                    semitones += 12.0 * m.intensity * v;
                }
                Target::Start
                | Target::Attack
                | Target::Release
                | Target::Fx
                | Target::LoopStart
                | Target::LoopLength => {}
            }
        }
        (gain.max(0.0), semitones, settled)
    }

    /// Shape targets at the control boundary; DSP retains only values.
    pub(crate) fn targets(&self, targets: &mut [Option<f32>], input: &Inputs, bend_pitch: Option<f32>) {
        assert!(targets.len() >= self.voiced.len(), "prepared voiced modulation targets");
        for (value, &index) in targets.iter_mut().zip(&self.voiced) {
            let modulation = &self.mods[index as usize];
            *value = modulation.read_value(input, bend_pitch).map(|x| modulation.shape(x));
        }
    }

    /// Unlagged edits retain the destination's existing declick law. Positive
    /// lag routes advance only when their source samples are consumed.
    pub(crate) fn prepare(&self, values: &mut [Option<f64>], targets: &[Option<f32>]) {
        for ((value, target), &index) in values.iter_mut().zip(targets).zip(&self.voiced) {
            let modulation = &self.mods[index as usize];
            if modulation.route.is_some_and(|(source, _)| source.live()) {
                if target.is_none() { *value = None; }
                else if modulation.lag <= 0. { *value = target.map(f64::from); }
                else if value.is_none() { *value = Some(0.); }
            }
        }
    }

    pub(crate) fn lagging(&self, values: &[Option<f64>], targets: &[Option<f32>]) -> bool {
        self.voiced.iter().enumerate().any(|(voice, &index)| {
            let modulation = &self.mods[index as usize];
            modulation.lag > 0. && modulation.route.is_some_and(|(source, _)| source.live())
                && values[voice] != targets[voice].map(f64::from)
        })
    }

    /// Fixed routes feed the existing amplifier declick. Lagged live routes
    /// are multiplied separately so they do not receive a second fade.
    pub(crate) fn fixed(&self, values: &[Option<f64>]) -> (f32, f32) {
        self.factors(values, false)
    }

    pub(crate) fn lagged(&self, values: &[Option<f64>]) -> (f32, f32) {
        self.factors(values, true)
    }

    fn factors(&self, values: &[Option<f64>], lagged: bool) -> (f32, f32) {
        let (mut gain, mut pitch) = (1., 0.);
        for (&value, &index) in values.iter().zip(&self.voiced) {
            let Some(value) = value else { continue; };
            let modulation = &self.mods[index as usize];
            let Some((source, target)) = modulation.route else { continue; };
            if (modulation.lag > 0. && source.live()) != lagged { continue; }
            let value = value as f32;
            match target {
                Target::Volume => {
                    let shaped = if modulation.intensity < 0. { 1. - value } else { value };
                    gain *= 1. - modulation.intensity.abs() * (1. - shaped);
                }
                Target::Pitch => {
                    let shaped = if source == Source::Bend { value * 2. - 1. } else { value };
                    pitch += 12. * modulation.intensity * shaped;
                }
                _ => {}
            }
        }
        (gain, pitch)
    }

    /// Render only live positive-lag contributions into reused fixed scratch.
    /// Preview uses a copy; only actual rendering advances saved values.
    pub(crate) fn render(
        &self, values: &mut [Option<f64>], targets: &[Option<f32>],
        rate: f32, gain: &mut [f32], pitch: &mut [f32],
    ) -> bool {
        assert_eq!(gain.len(), pitch.len());
        gain.fill(1.); pitch.fill(0.);
        let mut settled = true;
        for ((value, target), &index) in values.iter_mut().zip(targets).zip(&self.voiced) {
            let modulation = &self.mods[index as usize];
            let Some((source, destination)) = modulation.route else { continue; };
            if modulation.lag <= 0. || !source.live() { continue; }
            let Some(target) = *target else { *value = None; continue; };
            // Keep the existing core first-event lag seed. Native first-event
            // lag initialization is separate from source availability evidence.
            let value = value.get_or_insert(0.);
            let coefficient = modulation.lag_step(rate);
            let target = f64::from(target);
            for (gain, pitch) in gain.iter_mut().zip(pitch.iter_mut()) {
                if *value != target { *value += (target - *value) * coefficient; }
                let value = *value as f32;
                match destination {
                    Target::Volume => {
                        let shaped = if modulation.intensity < 0. { 1. - value } else { value };
                        *gain *= 1. - modulation.intensity.abs() * (1. - shaped);
                    }
                    Target::Pitch => {
                        let shaped = if source == Source::Bend { value * 2. - 1. } else { value };
                        *pitch += 12. * modulation.intensity * shaped;
                    }
                    _ => {}
                }
            }
            settled &= *value == target;
        }
        settled
    }

    /// Sample-start offset as a fraction of the zone's start-mod range.
    pub(crate) fn start_offset(&self, input: &Inputs) -> f32 {
        self.starts
            .iter()
            .map(|&i| &self.mods[i as usize])
            .filter_map(|m| m.start_value(input).map(|value| m.intensity.abs() * value))
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
        // Script values can change at the note callback after preparation.
        // Keep the entire legal start range resident for these zones.
        if self.starts.iter().any(|&i|self.mods[i as usize].route.is_some_and(|(source,_)|matches!(source,Source::Script(_)))) {
            return (0.,1.);
        }
        // Offline legacy preparation seeds are bytes; the DSP view has one
        // canonical normalized representation.
        let controls = cc.map(crate::midi::ControlValue::from_midi1);
        let cc = &controls;
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
        // Independent release velocity can arrive after zone preparation.
        // Its additive start contribution needs its own range, without
        // multiplying the key/attack/counter preparation sweep by 128.
        let release = |x| {
            self.starts
                .iter()
                .map(|&i| &self.mods[i as usize])
                .filter(|m| m.route.is_some_and(|(s, _)| s == Source::ReleaseVelocity))
                .map(|m| m.intensity.abs() * m.shape(x))
                .sum::<f32>()
        };
        let (release_low, release_high) = (0..=127)
            .map(|v| release(v as f32 / 127.))
            .fold((f32::MAX, f32::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)));
        let (mut low, mut high) = (f32::MAX, f32::MIN);
        for note in one(keys, Source::Key) {
            for velocity in velocities.clone() {
                for counter in counters.clone() {
                    let counter = f32::from(counter) / 127.0;
                    let input = Inputs { cc_received: None, cc74_received: None, per_note_cc: None, script_mods: &[],
                        cc74: None,
                        cc,
                        bend: 0.0,
                        pressure: 0.,
                        note,
                        velocity: crate::midi::Velocity::from_midi1(velocity),
                        release_velocity: None,
                        counter,
                    };
                    let base = self
                        .starts
                        .iter()
                        .map(|&i| &self.mods[i as usize])
                        .filter_map(|m| {
                            let (source, _) = m.route?;
                            (source != Source::ReleaseVelocity)
                                .then(|| m.intensity.abs() * m.shape(input.read(source)))
                        })
                        .sum::<f32>();
                    // A new instrument may start from its saved substitute
                    // rather than this preparation seed. Cover both starts.
                    let initial = Inputs { cc_received: Some(&[0; 2]), ..input };
                    let initial_base = self.starts.iter().map(|&i| &self.mods[i as usize])
                        .filter(|m| m.route.is_some_and(|(source, _)| source != Source::ReleaseVelocity))
                        .filter_map(|m| m.start_value(&initial).map(|value| m.intensity.abs() * value))
                        .sum::<f32>();
                    (low, high) = (
                        low.min((base.min(initial_base) + release_low).min(1.)),
                        high.max((base.max(initial_base) + release_high).min(1.)),
                    );
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
            let Some((_, target)) = m.route else {
                continue;
            };
            let Some(v) = m.start_value(input) else { continue; };
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


/// One-pole smoothing step over `frames`: the share of the distance covered.

fn lag_factor64(lag: f32, frames: usize, rate: f32) -> f64 {
    if lag <= 0. { 1. }
    else { (-(frames as f64) / (f64::from(lag) * f64::from(rate))).exp_m1() * -1. }
}

fn approach64(value: &mut f64, target: f64, lag: f32, frames: usize, rate: f32) {
    if lag <= 0. { *value = target; }
    else if *value != target { *value += (target - *value) * lag_factor64(lag, frames, rate); }
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
    pub const INTMOD_FREQUENCY: i32 = B + 168;
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
pub(crate) enum WavePar {
    Position,
    Phase,
    Form,
    Form2,
    FormMode,
    Form2Mode,
}

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
pub(crate) enum CubicDepth {
    Pitch,
    Cutoff,
    Loop,
}

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

/// Raw-address proof plus the narrow ordinary sampler/source baseline.
pub(super) fn primary_bypass_slot(group: &Group) -> Option<u8> {
    let slot = group.native_primary_bypass_slot?;
    let source = group.modulators.get(usize::from(slot))?;
    (group.source_mode == Some(0) && group.wavetable.is_none() && !group.reverse
        && group.mods.is_empty() && group.volume_lfos.is_empty() && !source.bypassed
        && ((group.native_volume_env && group.flex_env.is_none() && source.volume_env)
            || ((group.native_flex_oneshot || group.native_flex_loop)
                && group.volume_env.is_none() && source.primary_flex))).then_some(slot)
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
    PitchSourceIntensity {
        group: u16,
        slot: u8,
        target: u32,
        bipolar: bool,
    },
    /// An admitted volume LFO target: magnitude and source polarity are separate.
    VolumeSourceIntensity {
        group: u16,
        slot: u8,
        target: u32,
        bipolar: bool,
    },
    /// A prepared saved pitch LFO, addressed by its original source slot.
    PitchSourceBypass(u16, u8),
    /// Original Digital source slot; value is its authored Hz or sync count.
    LfoFrequency { group: u16, slot: u8, synchronized: bool },
    /// Explicit KSP bypass for an admitted pitch/filter envelope.
    InternalBypass(u16, u8),
    /// Sole primary amplitude source, with separately validated sample lifetime.
    PrimaryBypass(u16, u8),
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
            _ if crate::ksp::engine_par_name(par.id) == Some("$ENGINE_PAR_WT_POSITION") => {
                Self::Wavetable(wave()?, WavePar::Position)
            }
            _ if crate::ksp::engine_par_name(par.id) == Some("$ENGINE_PAR_WT_PHASE") => {
                Self::Wavetable(wave()?, WavePar::Phase)
            }
            _ if crate::ksp::engine_par_name(par.id) == Some("$ENGINE_PAR_WT_FORM") => {
                Self::Wavetable(wave()?, WavePar::Form)
            }
            _ if crate::ksp::engine_par_name(par.id) == Some("$ENGINE_PAR_WT_FORM2") => {
                Self::Wavetable(wave()?, WavePar::Form2)
            }
            _ if crate::ksp::engine_par_name(par.id) == Some("$ENGINE_PAR_WT_FORM_MODE") => {
                Self::Wavetable(wave()?, WavePar::FormMode)
            }
            _ if crate::ksp::engine_par_name(par.id) == Some("$ENGINE_PAR_WT_FORM2_MODE") => {
                Self::Wavetable(wave()?, WavePar::Form2Mode)
            }
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
            id::ATTACK
            | id::ATK_CURVE
            | id::DECAY
            | id::SUSTAIN
            | id::RELEASE
            | id::HOLD
            | id::ENV_AHD => {
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
            id::INTMOD_FREQUENCY => {
                let g = group()?;
                let group = &groups[g as usize];
                (par.generic == -1 && group.wavetable.is_none()
                    && modulator(g)?.kind == "lfo").then_some(())?;
                let slot = u8::try_from(par.slot).ok().filter(|&s| s < 16)?;
                let source = group.pitch_lfos.iter()
                    .chain(group.volume_lfos.iter().map(|l| &l.source))
                    .find(|l| l.slot == slot)?;
                super::lfo::validate_sources(&group.pitch_lfos, &group.volume_lfos).ok()?;
                (source.digital.is_some() && (!source.shared
                    || !source.extended_fade)).then_some(())?;
                Self::LfoFrequency { group: g, slot, synchronized: source.note_value > 1e-9 }
            }
            id::INTMOD_BYPASS => {
                let g = group()?;
                let m = modulator(g)?;
                if matches!(m.kind.as_str(), "lfo" | "glide") {
                    let slot = u8::try_from(par.slot).ok()?;
                    groups[g as usize].wavetable.is_none().then_some(())?;
                    let group = &groups[g as usize];
                    (group.pitch_lfos.iter().any(|l| l.slot == slot)
                        || group.volume_lfos.iter().any(|l| l.source.slot == slot)
                        || group.pitch_glides.iter().any(|l| l.slot == slot))
                    .then_some(())?;
                    return Some(Self::PitchSourceBypass(g, slot));
                }
                if primary_bypass_slot(&groups[g as usize]) == u8::try_from(par.slot).ok() {
                    return Some(Self::PrimaryBypass(g, u8::try_from(par.slot).ok()?));
                }
                let e = m.envelope?;
                groups[g as usize]
                    .envelopes
                    .get(e)?
                    .targets
                    .iter()
                    .any(|t| match &t.target {
                        ModTarget::Pitch => true,
                        ModTarget::Module { param, .. } => {
                            Knob::parse(param).is_some()
                                || super::filter::stage_knob(param).is_some()
                        }
                        _ => false,
                    })
                    .then_some(())?;
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
                        let cubic = CubicDepth::target(
                            &groups[g as usize].mods.get(index + target)?.target,
                        );
                        if par.id == id::INTMOD_INTENSITY && cubic.is_none() {
                            return None;
                        }
                        Self::Intensity {
                            group: g,
                            index: u16::try_from(index + target).ok()?,
                            bipolar,
                            cubic: if bipolar { cubic } else { None },
                        }
                    }
                    None => {
                        if matches!(m.kind.as_str(), "lfo" | "glide") {
                            // Prepared pitch-source playback currently exists only on
                            // ordinary sampler groups, not wavetable sources.
                            groups[g as usize].wavetable.is_none().then_some(())?;
                            let slot = u8::try_from(par.slot).ok()?;
                            let target = u32::try_from(target).ok()?;
                            let group = &groups[g as usize];
                            if m.kind == "lfo" && group.volume_lfos.iter()
                                .any(|l| l.source.slot == slot && l.target == target) {
                                return Some(Self::VolumeSourceIntensity { group: g, slot, target, bipolar });
                            }
                            group.pitch_lfos.iter().map(|l| (l.slot, &l.targets))
                                .chain(group.pitch_glides.iter().map(|l| (l.slot, &l.targets)))
                                .find(|l| l.0 == slot)?.1.iter().find(|t| t.0 == target)?;
                            return Some(Self::PitchSourceIntensity {
                                group: g,
                                slot,
                                target,
                                bipolar,
                            });
                        }
                        let envelope = m.envelope?;
                        let routed = &groups[g as usize]
                            .envelopes
                            .get(envelope)?
                            .targets
                            .get(target)?
                            .target;
                        match routed {
                            ModTarget::Pitch => {}
                            ModTarget::Module { param, .. }
                                if param == "filterCutoff"
                                    || (par.id != id::INTMOD_INTENSITY
                                        && (Knob::parse(param).is_some()
                                            || super::filter::stage_knob(param).is_some())) => {}
                            _ => return None,
                        }
                        let envelope = u8::try_from(envelope).ok()?;
                        let target = u16::try_from(target).ok()?;
                        Self::InternalIntensity {
                            group: g,
                            envelope,
                            target,
                            bipolar,
                            cubic: if bipolar {
                                CubicDepth::target(routed)
                            } else {
                                None
                            },
                        }
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
                    let rack = if par.generic == 0 {
                        Rack::Insert
                    } else {
                        rack()?
                    };
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
                "$ENGINE_PAR_GAIN" => filter(Knob::FilterGain)?,
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
            && group
                .zip(slot)
                .is_some_and(|(g, m)| g.modulators.get(m).is_none_or(|m| m.flex))
    }

    pub(crate) fn group_index(self) -> Option<usize> {
        match self {
            Self::Group(g, _) | Self::Wavetable(g, _) | Self::Envelope(g, _)
            | Self::ModEnvelope(g, _, _) | Self::PitchSourceBypass(g, _)
            | Self::InternalBypass(g, _) | Self::PrimaryBypass(g, _) | Self::Filter(g, _, _)
            | Self::GroupType(g, _) | Self::Intensity { group: g, .. }
            | Self::InternalIntensity { group: g, .. } | Self::PitchSourceIntensity { group: g, .. }
            | Self::VolumeSourceIntensity { group: g, .. }
            | Self::LfoFrequency { group: g, .. } => Some(usize::from(g)),
            Self::Instrument(_) | Self::Fx(..) => None,
        }
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
                | Self::PitchSourceIntensity { .. }
                | Self::VolumeSourceIntensity { .. }
                | Self::PitchSourceBypass(..)
                | Self::LfoFrequency { .. }
                | Self::InternalBypass(..)
                | Self::PrimaryBypass(..)
                | Self::Intensity { .. }
                | Self::Filter(..)
        )
    }

    /// Physical value of a KSP value: linear gain, pan -1..=1, semitones,
    /// seconds, intensity, bus index (-1 = instrument output) or bypass 0/1.
    pub(crate) fn decode(self, value: i32) -> f32 {
        let x = (value as f32 / UNIT).clamp(0.0, 1.0);
        match self {
            Self::LfoFrequency { synchronized, .. } => {
                // Only the documented KSP slider range is admitted. The native
                // synced adapter itself does not prove out-of-range clamping.
                if !(0..=1_000_000).contains(&value) { return f32::NAN; }
                // KSP first multiplies in double, then narrows the slider to f32.
                let x = (f64::from(value) * 1e-6) as f32;
                if synchronized { (12. - 11. * x).round() }
                else { lfo_frequency(x) }
            }
            Self::Filter(_, _, Knob::FilterGain)
            | Self::Fx(_, _, FxParam::Filter(Knob::FilterGain)) => value as f32 / UNIT,
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
            Self::Fx(_, _, FxParam::Reverb(0 | 10) | FxParam::Convolution(3 | 4)) => {
                f32::from(value != 0)
            }
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
            Self::Intensity {
                cubic: Some(CubicDepth::Pitch),
                ..
            }
            | Self::InternalIntensity {
                cubic: Some(CubicDepth::Pitch),
                ..
            }
            | Self::PitchSourceIntensity { bipolar: true, .. }
            | Self::VolumeSourceIntensity { bipolar: true, .. } => {
                (2.0 * value as f32 / UNIT - 1.0).powi(3)
            }
            Self::Intensity {
                cubic: Some(CubicDepth::Cutoff | CubicDepth::Loop),
                ..
            }
            | Self::InternalIntensity {
                cubic: Some(CubicDepth::Cutoff | CubicDepth::Loop),
                ..
            } => (2.0 * x - 1.0).powi(3),
            Self::Intensity { bipolar: true, .. }
            | Self::InternalIntensity { bipolar: true, .. } => 2.0 * x - 1.0,
            Self::Filter(_, _, Knob::Bypass)
            | Self::PitchSourceBypass(..)
            | Self::InternalBypass(..)
            | Self::PrimaryBypass(..) => f32::from(value != 0),
            Self::Filter(_, _, Knob::Type) => value as f32,
            Self::Filter(_, _, Knob::Output) => effect_gain(x),
            Self::Filter(_, _, Knob::SendLevel(_)) => volume(x),
            // Afflatus sets 434210 where it stores spread -0.1316, Solo 500000 for 0.
            Self::Filter(_, _, Knob::Spread | Knob::Pan)
            | Self::Fx(_, _, FxParam::Filter(Knob::Spread | Knob::Pan)) => 2.0 * x - 1.0,
            // Stored knobs are the KSP value / 1e6: Solo sets 1000000 and 0 where it stores 1 and 0.
            Self::Filter(..) | Self::Fx(_, _, FxParam::Filter(_)) => x,
            // Square law: Areia sets 704316 where its presets store 0.4961.
            Self::Intensity { .. }
            | Self::InternalIntensity { .. }
            | Self::PitchSourceIntensity { .. }
            | Self::VolumeSourceIntensity { .. } => x * x,
            Self::Fx(_, _, FxParam::Bypass) => f32::from(value != 0),
            Self::Fx(_, _, FxParam::Pan) => 2.0 * x - 1.0,
            Self::Fx(_, _, FxParam::Wet | FxParam::Dry) => effect_gain(x),
            Self::Fx(..) => volume(x),
        }
    }

    /// Inverse of [`decode`](Self::decode), rounded.
    pub(crate) fn encode(self, v: f32) -> i32 {
        let x = match self {
            Self::LfoFrequency { synchronized, .. } => {
                let x = if synchronized { (12. - v) * f32::from_bits(0x3dba2e8c) }
                    else { lfo_frequency_value(v) };
                return (x.clamp(0., 1.) * UNIT + 0.5) as i32;
            }
            Self::Filter(_, _, Knob::FilterGain)
            | Self::Fx(_, _, FxParam::Filter(Knob::FilterGain)) => {
                return (v * UNIT).round() as i32;
            }
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
            Self::Fx(_, _, FxParam::Reverb(0 | 10) | FxParam::Convolution(3 | 4)) => {
                return i32::from(v >= 0.5);
            }
            Self::Fx(_, _, FxParam::Reverb(_) | FxParam::Convolution(_) | FxParam::Field(..)) => v,
            Self::Filter(_, _, Knob::Type) => return v as i32,
            Self::Fx(_, _, FxParam::Bypass)
            | Self::Filter(_, _, Knob::Bypass)
            | Self::PitchSourceBypass(..)
            | Self::InternalBypass(..)
            | Self::PrimaryBypass(..) => {
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
            Self::Intensity {
                cubic: Some(CubicDepth::Pitch),
                ..
            }
            | Self::InternalIntensity {
                cubic: Some(CubicDepth::Pitch),
                ..
            }
            | Self::PitchSourceIntensity { bipolar: true, .. }
            | Self::VolumeSourceIntensity { bipolar: true, .. } => {
                return ((v.cbrt() + 1.0) * 0.5 * UNIT).round() as i32;
            }
            Self::Intensity {
                cubic: Some(CubicDepth::Cutoff | CubicDepth::Loop),
                ..
            }
            | Self::InternalIntensity {
                cubic: Some(CubicDepth::Cutoff | CubicDepth::Loop),
                ..
            } => (v.cbrt() + 1.0) * 0.5,
            Self::Intensity { bipolar: true, .. }
            | Self::InternalIntensity { bipolar: true, .. }
            | Self::Fx(_, _, FxParam::Pan)
            | Self::Filter(_, _, Knob::Spread | Knob::Pan)
            | Self::Fx(_, _, FxParam::Filter(Knob::Spread | Knob::Pan)) => (v + 1.0) * 0.5,
            Self::Intensity { .. }
            | Self::InternalIntensity { .. }
            | Self::PitchSourceIntensity { .. }
            | Self::VolumeSourceIntensity { .. } => v.abs().sqrt(),
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

// Native parameter713's legacy exponential approximation. Keep every f32
// boundary; its slider endpoints differ from the saved frequency bounds.
fn lfo_frequency(x: f32) -> f32 {
    let y = x.clamp(0., 1.) * f32::from_bits(0x41650ef6) - f32::from_bits(0x40d2b2b7);
    let fraction = y - y.floor();
    let correction = (fraction - fraction * fraction) * f32::from_bits(0x3eadee78);
    f32::from_bits(((y + 127. - correction) * 8_388_608.) as u32)
}

fn lfo_frequency_value(hz: f32) -> f32 {
    let y = hz.to_bits() as i32 as f32 * f32::from_bits(0x34000000) - 127.;
    let fraction = y - y.floor();
    let correction = (fraction - fraction * fraction) * f32::from_bits(0x3eb17679);
    (((y + correction) * f32::from_bits(0x3f317218) - f32::from_bits(0xc0922ab7))
        * f32::from_bits(0x3dce2772)).clamp(0., 1.)
}
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

/// Native `_set_engine_par(VOLUME, value, ...)`: double normalization,
/// one float conversion, then the verified float cubic converter (8.13.1).
pub(super) fn native_volume(value: f64) -> f32 {
    let x = (value * 0.000_001) as f32;
    (x * x * x).clamp(0.0, 1.0) * 4.0
}

/// Native getter inverse: float clamp(gain/4), CRT powf with float 1/3,
/// then float million-unit rounding. Host/Windows CRT bit parity is unproved.
pub(super) fn native_volume_value(gain: f32) -> i32 {
    let x = (gain * 0.25).clamp(0.0, 1.0).powf(1.0 / 3.0);
    (x * UNIT + 0.5) as i32
}

/// The native standard CC7 script multiplies `%CC[7]` by these integers.
/// MIDI1 follows that byte law. Higher resolutions extend it continuously;
/// this precision policy is not a claim about Kontakt's MIDI2 implementation.
pub(super) fn native_cc7(value: crate::midi::ControlValue, maximum_db: i32) -> f32 {
    let multiplier = match maximum_db {
        -12 => 3113,
        -6 => 3937,
        6 => 6243,
        12 => 7874,
        _ => 4956,
    };
    native_volume(value.normalized() * f64::from(127 * multiplier))
}

/// Native instrument PAN: double normalization, float bipolar conversion,
/// clamp and the row converter's float unipolar/bipolar round trip.
pub(super) fn native_pan(value: f64) -> f32 {
    let x = (value * 0.000_001) as f32;
    let z = (x * 2.0 - 1.0).clamp(-1.0, 1.0);
    (z * 0.5 + 0.5) * 2.0 - 1.0
}

pub(super) fn native_pan_value(pan: f32) -> i32 {
    let x = (pan.clamp(-1.0, 1.0) + 1.0) * 0.5;
    (x * UNIT + 0.5) as i32
}

/// MIDI1 coefficient recovered from the system script; higher resolutions
/// extend that mapping continuously without claiming Kontakt MIDI2 parity.
pub(super) fn native_cc10(value: crate::midi::ControlValue) -> f32 {
    native_pan(value.normalized() * f64::from(127 * 7874))
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
            "$ENGINE_PAR_GAIN" => Disp::Num(12.0 * value as f32 / UNIT, 1),
            "$ENGINE_PAR_SEQ_LF_GAIN"
            | "$ENGINE_PAR_SEQ_LMF_GAIN"
            | "$ENGINE_PAR_SEQ_HMF_GAIN"
            | "$ENGINE_PAR_SEQ_HF_GAIN" => Disp::Num(super::filter::geq_gain_db(x), 1),
            name @ ("$ENGINE_PAR_COMP_ATTACK"
            | "$ENGINE_PAR_COMP_DECAY"
            | "$ENGINE_PAR_LIM_RELEASE"
            | "$ENGINE_PAR_DL_TIME") => {
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
            let Some(source) = settings
                .get_mut(g as usize)
                .and_then(|g| g.wavetable.as_mut())
            else {
                return false;
            };
            match p {
                WavePar::Position => source.position = value.clamp(0., 1.),
                WavePar::Phase => source.phase = value.clamp(0., 1.),
                WavePar::Form => source.form1 = value.clamp(0., 1.),
                WavePar::Form2 => source.form2 = value.clamp(0., 1.),
                WavePar::FormMode | WavePar::Form2Mode => {
                    if !super::wavetable::form_supported(value as i32) {
                        return false;
                    }
                    if matches!(p, WavePar::FormMode) {
                        source.form1_type = value as i32;
                    } else {
                        source.form2_type = value as i32;
                    }
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
        Address::Intensity {
            group,
            index,
            cubic,
            ..
        } => {
            if cubic.is_some() && !value.is_finite() {
                return false;
            }
            let m = settings
                .get_mut(group as usize)
                .and_then(|s| s.mods.mods.get_mut(index as usize));
            let Some(m) = m else {
                return false;
            };
            m.intensity = if cubic == Some(CubicDepth::Pitch) {
                value
            } else {
                value.clamp(-1.0, 1.0)
            };
        }
        Address::InternalIntensity {
            group,
            envelope,
            target,
            ..
        } => {
            if !value.is_finite() {
                return false;
            }
            let value = if matches!(
                address,
                Address::InternalIntensity {
                    cubic: Some(CubicDepth::Pitch),
                    ..
                }
            ) {
                value
            } else {
                value.clamp(-1., 1.)
            };
            let Some(settings) = settings.get_mut(group as usize) else {
                return false;
            };
            let mut applied = false;
            if let Some(m) = settings
                .pitch_envelopes
                .iter_mut()
                .find(|e| e.index == envelope)
                .and_then(|e| e.targets.iter_mut().find(|t| t.0 == target))
            {
                m.2.intensity = value;
                applied = true;
            }
            if let Some(m) = settings
                .filter
                .as_mut()
                .and_then(|f| f.envelope_mod(envelope, target))
            {
                m.intensity = value;
                applied = true;
            }
            return applied;
        }
        Address::PitchSourceIntensity {
            group,
            slot,
            target,
            ..
        } => {
            if !value.is_finite() {
                return false;
            }
            let Some(group) = settings.get_mut(group as usize) else { return false; };
            let Some((_, depth, targets)) = group.pitch_lfos.iter_mut()
                .map(|l| (l.slot, &mut l.depth, &mut l.targets))
                .chain(group.pitch_glides.iter_mut().map(|l| (l.slot, &mut l.depth, &mut l.targets)))
                .find(|l| l.0 == slot) else { return false; };
            let Some(index) = targets.iter().position(|t| t.0 == target) else {
                return false;
            };
            let next: f32 = targets.iter().enumerate()
                .map(|(i, t)| if i == index { value } else { t.1 }).sum();
            if !next.is_finite() { return false; }
            targets[index].1 = value;
            *depth = next;
        }
        Address::VolumeSourceIntensity { group, slot, target, bipolar } => {
            if !value.is_finite() { return false; }
            let Some(lfo) = settings.get_mut(group as usize)
                .and_then(|g| g.volume_lfos.iter_mut()
                    .find(|l| l.source.slot == slot && l.target == target)) else { return false; };
            lfo.intensity = value.abs();
            if bipolar { lfo.negative = value < 0.; }
        }
        Address::LfoFrequency { group: g, slot, synchronized } => {
            if !value.is_finite() || (!synchronized && value <= 0.) { return false; }
            let Some(group) = settings.get_mut(g as usize) else { return false; };
            if super::lfo::validate_sources(&group.pitch_lfos, &group.volume_lfos).is_err() { return false; }
            let mut sources = group.pitch_lfos.iter()
                .chain(group.volume_lfos.iter().map(|l| &l.source)).filter(|l| l.slot == slot);
            let supported = |l: &crate::import::PitchLfo| l.digital.is_some()
                && (l.note_value > 1e-9) == synchronized
                && (!l.shared || (!l.extended_fade && (!synchronized || (1. ..=12.).contains(&value))));
            if !sources.next().is_some_and(supported) || !sources.all(supported) { return false; }
            for source in group.pitch_lfos.iter_mut()
                .chain(group.volume_lfos.iter_mut().map(|l| &mut l.source)).filter(|l| l.slot == slot) {
                source.count = value;
            }
        }
        Address::PitchSourceBypass(g, slot) => {
            let Some(group) = settings.get_mut(g as usize) else {
                return false;
            };
            let mut applied = false;
            for lfo in group.pitch_lfos.iter_mut().filter(|l| l.slot == slot) {
                lfo.bypassed = value != 0.;
                applied = true;
            }
            for lfo in group
                .volume_lfos
                .iter_mut()
                .filter(|l| l.source.slot == slot)
            {
                lfo.source.bypassed = value != 0.;
                applied = true;
            }
            for glide in group.pitch_glides.iter_mut().filter(|l| l.slot == slot) {
                glide.bypassed = value != 0.;
                applied = true;
            }
            return applied;
        }
        Address::PrimaryBypass(g, slot) => {
            let Some(control) = settings.get_mut(g as usize).and_then(|g| g.primary_bypass.as_mut())
                .filter(|control| control.0 == slot) else { return false; };
            control.1 = value != 0.;
        }
        Address::InternalBypass(g, index) => {
            let Some(settings) = settings.get_mut(g as usize) else {
                return false;
            };
            let mut applied = false;
            if let Some(envelope) = settings
                .pitch_envelopes
                .iter_mut()
                .find(|e| e.index == index)
            {
                envelope.bypass = value != 0.;
                applied = true;
            }
            if let Some(bypass) = settings
                .filter
                .as_mut()
                .and_then(|f| f.envelope_bypass(index))
            {
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
            Some(match p {
                WavePar::Position => source.position,
                WavePar::Phase => source.phase,
                WavePar::Form => source.form1,
                WavePar::Form2 => source.form2,
                WavePar::FormMode => source.form1_type as f32,
                WavePar::Form2Mode => source.form2_type as f32,
            })
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
        Address::InternalIntensity {
            group,
            envelope,
            target,
            ..
        } => {
            let settings = settings.get(group as usize)?;
            settings
                .pitch_envelopes
                .iter()
                .find(|e| e.index == envelope)
                .and_then(|e| e.targets.iter().find(|t| t.0 == target))
                .map(|t| t.2.intensity)
                .or_else(|| {
                    settings
                        .filter
                        .as_ref()?
                        .envelope_mod_at(envelope, target)
                        .map(|m| m.intensity)
                })
        }
        Address::PitchSourceIntensity {
            group,
            slot,
            target,
            ..
        } => {
            let group = settings.get(group as usize)?;
            group.pitch_lfos.iter().map(|l| (l.slot, &l.targets))
                .chain(group.pitch_glides.iter().map(|l| (l.slot, &l.targets)))
                .find(|l| l.0 == slot)?.1.iter().find(|t| t.0 == target).map(|t| t.1)
        },
        Address::VolumeSourceIntensity { group, slot, target, bipolar } => {
            let lfo = settings.get(group as usize)?.volume_lfos.iter()
                .find(|l| l.source.slot == slot && l.target == target)?;
            Some(if bipolar && lfo.negative { -lfo.intensity } else { lfo.intensity })
        }
        Address::LfoFrequency { group: g, slot, .. } => {
            let group = settings.get(g as usize)?;
            group.pitch_lfos.iter().chain(group.volume_lfos.iter().map(|l| &l.source))
                .find(|l| l.slot == slot).map(|l| l.count)
        }
        Address::PitchSourceBypass(g, slot) => {
            let group = settings.get(g as usize)?;
            group
                .pitch_lfos
                .iter()
                .find(|l| l.slot == slot)
                .map(|l| f32::from(l.bypassed))
                .or_else(|| {
                    group
                        .volume_lfos
                        .iter()
                        .find(|l| l.source.slot == slot)
                        .map(|l| f32::from(l.source.bypassed))
                }).or_else(|| group.pitch_glides.iter().find(|l| l.slot == slot)
                    .map(|l| f32::from(l.bypassed)))
        }
        Address::PrimaryBypass(g, slot) => {
            let control = settings.get(g as usize)?.primary_bypass.filter(|p| p.0 == slot)?;
            Some(f32::from(control.1))
        }
        Address::InternalBypass(g, index) => {
            let settings = settings.get(g as usize)?;
            settings
                .pitch_envelopes
                .iter()
                .find(|e| e.index == index)
                .map(|e| f32::from(e.bypass))
                .or_else(|| {
                    settings
                        .filter
                        .as_ref()?
                        .envelope_bypass_at(index)
                        .map(f32::from)
                })
        }
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
    /// Same-sample standard commands preceding this native scalar write.
    pub standard_controls_before: usize,
    /// A physical native scalar action, retaining the canonical wire precision.
    pub standard_physical: Option<crate::midi::ControlValue>,
    /// A coalesced physical action may also retain an earlier authored baseline.
    pub standard_authored: bool,
}

/// Engine parameter changes one render can hold; later ones are dropped and counted.
pub const MAX_WRITES: usize = 4096;

/// `$EFFECT_TYPE_*` of group `g`'s insert `slot`, 0 when it is empty.
pub(crate) fn group_type(groups: &[Group], g: u16, slot: u8) -> Option<f32> {
    let fx = &groups.get(g as usize)?.fx;
    let kind = fx
        .slots
        .iter()
        .find(|fx| fx.slot == slot as usize)
        .map(|fx| fx.kind.ser_id());
    Some(f32::from(kind.unwrap_or(0)))
}

/// `find_mod`: position in `Group::modulators` of the first name `is` accepts.
pub(crate) fn find_mod(groups: &[Group], group: usize, is: &dyn Fn(&str) -> bool) -> Option<usize> {
    groups
        .get(group)?
        .modulators
        .iter()
        .position(|m| m.kind != "undecoded" && is(&m.name))
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
mod tests;
