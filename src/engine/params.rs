//! Script-controllable engine state: per-group modulation tables and the KSP
//! engine parameters (`set_engine_par`) mapped onto groups, the instrument,
//! buses and effects.
//!
//! Mappings from Kontakt's normalized 0..=1000000 values and the modulation
//! semantics, with their confidence, are in `audits/MODULATION.md`
//! ("Runtime modulation and engine parameters").

use super::{Ahdsr, GroupSettings};
use crate::fx::{FxParam, Rack};
use crate::import::{Group, ModAssignment, ModSource, ModTarget};
use crate::ksp::{ENGINE_PAR_BASE, EnginePar};

/// Modulation values one voice tracks: its group's first volume and pitch
/// assignments with a modelled source.
pub const VOICE_MODS: usize = 8;

/// Modulation source the engine models.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    Velocity,
    Key,
    Constant,
    Cc(u8),
    Bend,
    Pressure,
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
    curve: Option<Box<[f32; 128]>>,
}

impl From<&ModAssignment> for Mod {
    fn from(m: &ModAssignment) -> Self {
        let target = match m.target {
            ModTarget::Volume => Some(Target::Volume),
            ModTarget::Pitch => Some(Target::Pitch),
            ModTarget::SampleStart => Some(Target::Start),
            ModTarget::Attack => Some(Target::Attack),
            ModTarget::Release => Some(Target::Release),
            ModTarget::Module { .. } => None,
        };
        Self {
            route: Source::of(m.source).zip(target),
            intensity: m.intensity,
            lag: f32::from(m.lag_ms) / 1000.0,
            curve: m
                .shaper
                .as_ref()
                .map(|_| Box::new(std::array::from_fn(|i| m.shape(i as f32 / 127.0)))),
        }
    }
}

impl Mod {
    /// Shaped source value; exact at MIDI steps, linear between them.
    fn shape(&self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        let Some(curve) = &self.curve else {
            return x;
        };
        let p = x * 127.0;
        let i = (p as usize).min(126);
        curve[i] + (curve[i + 1] - curve[i]) * (p - i as f32)
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
}

impl From<&Group> for ModTable {
    fn from(group: &Group) -> Self {
        let mods: Box<[Mod]> = group.mods.iter().map(Mod::from).collect();
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
            mods,
        }
    }
}

/// Performance state a voice's modulation reads.
#[derive(Clone, Copy)]
pub(crate) struct Inputs<'a> {
    pub cc: &'a [u8; 128],
    /// -1..=1.
    pub bend: f32,
    pub pressure: u8,
    pub note: u8,
    pub velocity: u8,
}

impl Inputs<'_> {
    /// Unshaped source value, 0..=1.
    fn read(&self, source: Source) -> f32 {
        match source {
            Source::Velocity => f32::from(self.velocity) / 127.0,
            Source::Key => f32::from(self.note) / 127.0,
            Source::Constant => 1.0,
            Source::Cc(cc) => f32::from(self.cc[cc as usize]) / 127.0,
            Source::Bend => (self.bend + 1.0) * 0.5,
            Source::Pressure => f32::from(self.pressure) / 127.0,
        }
    }
}

impl ModTable {
    /// Initial per-voice values: every source at its current value, unlagged.
    pub(crate) fn start(&self, input: &Inputs) -> [f32; VOICE_MODS] {
        let mut values = [0.0; VOICE_MODS];
        for (value, &i) in values.iter_mut().zip(&self.voiced) {
            let m = &self.mods[i as usize];
            if let Some((source, _)) = m.route {
                *value = m.shape(input.read(source));
            }
        }
        values
    }

    /// Advance live sources over `frames` at `rate` and return the volume
    /// factor and pitch offset in semitones.
    ///
    /// Volume: each assignment scales amplitude by `1 - |i|·(1 - v)` for
    /// shaped value `v` (inverted, `1 - v`, when `i < 0`). Pitch: `12·i·v`
    /// semitones, with pitch bend mapped back to -1..=1.
    pub(crate) fn modulate(
        &self,
        values: &mut [f32; VOICE_MODS],
        input: &Inputs,
        frames: usize,
        rate: f32,
    ) -> (f32, f32) {
        let (mut gain, mut semitones) = (1.0, 0.0);
        for (value, &i) in values.iter_mut().zip(&self.voiced) {
            let m = &self.mods[i as usize];
            let Some((source, target)) = m.route else {
                continue;
            };
            if source.live() {
                let x = m.shape(input.read(source));
                *value += (x - *value) * lag_factor(m.lag, frames, rate);
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
                Target::Start | Target::Attack | Target::Release => {}
            }
        }
        (gain.max(0.0), semitones)
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
mod id {
    use super::ENGINE_PAR_BASE as B;
    pub const VOLUME: i32 = B;
    pub const PAN: i32 = B + 1;
    pub const TUNE: i32 = B + 2;
    pub const OUTPUT_CHANNEL: i32 = B + 3;
    pub const ATTACK: i32 = B + 6;
    pub const DECAY: i32 = B + 7;
    pub const SUSTAIN: i32 = B + 8;
    pub const RELEASE: i32 = B + 9;
    pub const HOLD: i32 = B + 10;
    pub const ATK_CURVE: i32 = B + 11;
    pub const MOD_TARGET_INTENSITY: i32 = B + 16;
    pub const MOD_TARGET_MP_INTENSITY: i32 = B + 17;
    pub const EFFECT_BYPASS: i32 = B + 22;
    pub const SEND_EFFECT_BYPASS: i32 = B + 26;
    pub const SEND_EFFECT_DRY_LEVEL: i32 = B + 27;
    pub const SEND_EFFECT_OUTPUT_GAIN: i32 = B + 28;
    pub const INSERT_EFFECT_OUTPUT_GAIN: i32 = B + 29;
    pub const SENDLEVEL_0: i32 = B + 30;
    pub const SENDLEVEL_7: i32 = B + 37;
}

/// KSP `$NI_BUS_OFFSET`: generic values from here address instrument buses.
const BUS_OFFSET: i32 = 1000;
/// Instrument buses.
const BUSES: u8 = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GroupPar {
    Volume,
    Pan,
    Tune,
    Output,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stage {
    Attack,
    /// Attack curve, -1..=1.
    Curve,
    Hold,
    Decay,
    Sustain,
    Release,
}

/// A modelled engine parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Address {
    Group(u16, GroupPar),
    /// Volume, pan or tune of the whole instrument.
    Instrument(GroupPar),
    /// Volume envelope of a group.
    Envelope(u16, Stage),
    /// A modulation assignment's intensity; `bipolar` for the MP variant.
    Intensity {
        group: u16,
        index: u16,
        bipolar: bool,
    },
    Fx(Rack, u8, FxParam),
}

impl Address {
    /// Map a KSP address onto the engine, or `None` when it is not modelled.
    pub(crate) fn resolve(par: EnginePar, groups: &[Group]) -> Option<Self> {
        let group = || {
            u16::try_from(par.group)
                .ok()
                .filter(|&g| (g as usize) < groups.len())
        };
        let modulator = |g: u16| {
            let m = usize::try_from(par.slot).ok()?;
            groups[g as usize].modulators.get(m)
        };
        let rack = || {
            (par.group == -1).then_some(())?;
            Some(match par.generic {
                0 => Rack::Send,
                1 => Rack::Insert,
                2 => Rack::Main,
                g => Rack::Bus(u8::try_from(g - BUS_OFFSET).ok().filter(|&b| b < BUSES)?),
            })
        };
        let fx = |param| Some(Self::Fx(rack()?, u8::try_from(par.slot).ok()?, param));
        Some(match par.id {
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
                    (_, None, GroupPar::Volume | GroupPar::Pan | GroupPar::Tune) => {
                        Self::Instrument(p)
                    }
                    _ => return None,
                }
            }
            id::ATTACK | id::ATK_CURVE | id::DECAY | id::SUSTAIN | id::RELEASE | id::HOLD => {
                let g = group()?;
                modulator(g)?.volume_env.then_some(())?;
                let stage = match par.id {
                    id::ATTACK => Stage::Attack,
                    id::ATK_CURVE => Stage::Curve,
                    id::HOLD => Stage::Hold,
                    id::DECAY => Stage::Decay,
                    id::SUSTAIN => Stage::Sustain,
                    _ => Stage::Release,
                };
                Self::Envelope(g, stage)
            }
            id::MOD_TARGET_INTENSITY | id::MOD_TARGET_MP_INTENSITY => {
                let g = group()?;
                let m = modulator(g)?;
                let target = usize::try_from(par.generic).unwrap_or(0);
                (target < m.targets.len()).then_some(())?;
                Self::Intensity {
                    group: g,
                    index: u16::try_from(m.assignments? + target).ok()?,
                    bipolar: par.id == id::MOD_TARGET_MP_INTENSITY,
                }
            }
            id::EFFECT_BYPASS | id::SEND_EFFECT_BYPASS => fx(FxParam::Bypass)?,
            id::SEND_EFFECT_DRY_LEVEL => fx(FxParam::Dry)?,
            id::SEND_EFFECT_OUTPUT_GAIN | id::INSERT_EFFECT_OUTPUT_GAIN => fx(FxParam::Wet)?,
            id::SENDLEVEL_0..=id::SENDLEVEL_7 => {
                fx(FxParam::SendLevel((par.id - id::SENDLEVEL_0) as u8))?
            }
            _ => return None,
        })
    }

    /// Held by the bank's group settings.
    pub(crate) fn is_group(&self) -> bool {
        matches!(
            self,
            Self::Group(..) | Self::Envelope(..) | Self::Intensity { .. }
        )
    }

    /// Physical value of a KSP value: linear gain, pan -1..=1, semitones,
    /// seconds, intensity, bus index (-1 = instrument output) or bypass 0/1.
    pub(crate) fn decode(self, value: i32) -> f32 {
        let x = (value as f32 / UNIT).clamp(0.0, 1.0);
        match self {
            Self::Group(_, GroupPar::Output) => match value - BUS_OFFSET {
                b @ 0..16 => b as f32,
                _ => -1.0,
            },
            Self::Group(_, p) | Self::Instrument(p) => match p {
                GroupPar::Volume => volume(x),
                GroupPar::Pan => 2.0 * x - 1.0,
                _ => (2.0 * x - 1.0) * TUNE_RANGE,
            },
            Self::Envelope(_, stage) => match stage {
                Stage::Sustain => x,
                // Solo sets 1000000, 750000 and 333333 where its presets store 1, 0.5, -0.33.
                Stage::Curve => 2.0 * x - 1.0,
                Stage::Attack | Stage::Hold => time(x, SHORT),
                Stage::Decay | Stage::Release => time(x, LONG),
            },
            Self::Intensity { bipolar: true, .. } => 2.0 * x - 1.0,
            // Square law: Areia sets 704316 where its presets store 0.4961.
            Self::Intensity { .. } => x * x,
            Self::Fx(_, _, FxParam::Bypass) => f32::from(value != 0),
            Self::Fx(_, _, FxParam::Pan) => 2.0 * x - 1.0,
            Self::Fx(..) => volume(x),
        }
    }

    /// Inverse of [`decode`](Self::decode), rounded.
    pub(crate) fn encode(self, v: f32) -> i32 {
        let x = match self {
            Self::Group(_, GroupPar::Output) => {
                return if v >= 0.0 { BUS_OFFSET + v as i32 } else { -1 };
            }
            Self::Fx(_, _, FxParam::Bypass) => return i32::from(v != 0.0),
            Self::Group(_, p) | Self::Instrument(p) => match p {
                GroupPar::Volume => volume_value(v),
                GroupPar::Pan => (v + 1.0) * 0.5,
                _ => (v / TUNE_RANGE + 1.0) * 0.5,
            },
            Self::Envelope(_, stage) => match stage {
                Stage::Sustain => v,
                Stage::Curve => (v + 1.0) * 0.5,
                Stage::Attack | Stage::Hold => time_value(v, SHORT),
                Stage::Decay | Stage::Release => time_value(v, LONG),
            },
            Self::Intensity { bipolar: true, .. } | Self::Fx(_, _, FxParam::Pan) => (v + 1.0) * 0.5,
            Self::Intensity { .. } => v.abs().sqrt(),
            Self::Fx(..) => volume_value(v),
        };
        (x.clamp(0.0, 1.0) * UNIT).round() as i32
    }
}

const UNIT: f32 = 1_000_000.0;
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

/// Set a group-level parameter (group, envelope or intensity); false for
/// other addresses and missing groups.
pub(crate) fn write(settings: &mut [GroupSettings], address: Address, value: f32) -> bool {
    match address {
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
        Address::Envelope(g, stage) => {
            let Some(env) = settings
                .get_mut(g as usize)
                .and_then(|s| s.envelope.as_mut())
            else {
                return false;
            };
            match stage {
                Stage::Curve => env.curve = value.clamp(-1.0, 1.0),
                Stage::Attack => env.attack = value.max(0.0),
                Stage::Hold => env.hold = value.max(0.0),
                Stage::Decay => env.decay = value.max(0.0),
                Stage::Sustain => env.sustain = value.clamp(0.0, 1.0),
                Stage::Release => env.release = value.max(0.0),
            }
        }
        Address::Intensity { group, index, .. } => {
            let m = settings
                .get_mut(group as usize)
                .and_then(|s| s.mods.mods.get_mut(index as usize));
            let Some(m) = m else {
                return false;
            };
            m.intensity = value.clamp(-1.0, 1.0);
        }
        Address::Instrument(_) | Address::Fx(..) => return false,
    }
    true
}

/// Current value of a group-level parameter.
pub(crate) fn read(settings: &[GroupSettings], address: Address) -> Option<f32> {
    match address {
        Address::Group(g, p) => {
            let group = settings.get(g as usize)?;
            Some(match p {
                GroupPar::Volume => group.gain,
                GroupPar::Pan => group.pan,
                GroupPar::Tune => group.tune,
                GroupPar::Output => group.bus.map_or(-1.0, f32::from),
            })
        }
        Address::Envelope(g, stage) => {
            let env = settings.get(g as usize)?.envelope?;
            Some(match stage {
                Stage::Attack => env.attack,
                Stage::Curve => env.curve,
                Stage::Hold => env.hold,
                Stage::Decay => env.decay,
                Stage::Sustain => env.sustain,
                Stage::Release => env.release,
            })
        }
        Address::Intensity { group, index, .. } => settings
            .get(group as usize)?
            .mods
            .mods
            .get(index as usize)
            .map(|m| m.intensity),
        _ => None,
    }
}

/// One engine parameter change, applied at frame `at` of the next render.
#[derive(Clone, Copy, Debug)]
pub(super) struct Write {
    pub at: u32,
    pub address: Address,
    pub value: f32,
}

/// Engine parameter changes one render can hold; later ones are dropped and counted.
pub const MAX_WRITES: usize = 4096;

/// `find_mod`: position in `Group::modulators`.
pub(crate) fn find_mod(groups: &[Group], group: usize, name: &str) -> Option<usize> {
    groups
        .get(group)?
        .modulators
        .iter()
        .position(|m| m.name == name)
}

/// `find_target`: position among the modulator's targets.
pub(crate) fn find_target(
    groups: &[Group],
    group: usize,
    modulator: usize,
    name: &str,
) -> Option<usize> {
    groups
        .get(group)?
        .modulators
        .get(modulator)?
        .targets
        .iter()
        .position(|t| t == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::{Modulator, ShaperCurve};

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
            (id::EFFECT_BYPASS, "EFFECT_BYPASS"),
            (id::SEND_EFFECT_BYPASS, "SEND_EFFECT_BYPASS"),
            (id::SEND_EFFECT_DRY_LEVEL, "SEND_EFFECT_DRY_LEVEL"),
            (id::SEND_EFFECT_OUTPUT_GAIN, "SEND_EFFECT_OUTPUT_GAIN"),
            (id::INSERT_EFFECT_OUTPUT_GAIN, "INSERT_EFFECT_OUTPUT_GAIN"),
            (id::SENDLEVEL_0, "SENDLEVEL_0"),
            (id::SENDLEVEL_7, "SENDLEVEL_7"),
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
        };
        assert!((intensity.decode(704_316) - 0.4961).abs() < 1e-3);
        // Areia sets these at init; its saved envelopes hold the same times.
        let attack = Address::Envelope(0, Stage::Attack);
        let release = Address::Envelope(0, Stage::Release);
        assert!((attack.decode(465_229) - 0.125_013).abs() < 1e-4);
        assert!((release.decode(512_668) - 0.250_001).abs() < 1e-4);
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
                },
                Modulator {
                    name: "VEL_VOLUME".into(),
                    targets: vec![String::new()],
                    assignments: Some(0),
                    volume_env: false,
                },
                Modulator {
                    name: "CC_VOLUME".into(),
                    targets: vec![String::new()],
                    assignments: Some(1),
                    volume_env: false,
                },
            ],
            ..Group::default()
        }
    }

    #[test]
    fn velocity_shaper_and_lagged_cc_volume() {
        let table = ModTable::from(&group());
        let mut cc = [0; 128];
        cc[11] = 127;
        let mut input = Inputs {
            cc: &cc,
            bend: 0.0,
            pressure: 0,
            note: 60,
            velocity: 64,
        };
        let mut values = table.start(&input);
        let (gain, _) = table.modulate(&mut values, &input, 128, 48000.0);
        let expected = (64.0f32 / 127.0).powi(2);
        assert!((gain - expected).abs() < 1e-5, "{gain} vs {expected}");

        // CC11 drops to 0: after one lag time constant 63% of the way down.
        let quiet = [0; 128];
        input.cc = &quiet;
        let (gain, _) = table.modulate(&mut values, &input, 4800, 48000.0);
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
                bend: 0.0,
                pressure: 0,
                note: 60,
                velocity,
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
    fn addresses_resolve_by_decoded_names() {
        let groups = [group()];
        assert_eq!(find_mod(&groups, 0, "CC_VOLUME"), Some(2));
        assert_eq!(find_target(&groups, 0, 0, "ENV_AHDSR_VOLUME"), Some(0));
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
                bipolar: false
            })
        );
        assert_eq!(
            Address::resolve(par(id::ATTACK, 0, -1), &groups),
            Some(Address::Envelope(0, Stage::Attack))
        );
        assert_eq!(Address::resolve(par(id::ATTACK, 1, -1), &groups), None);
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
