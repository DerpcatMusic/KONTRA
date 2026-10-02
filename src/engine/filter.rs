//! Per-voice group insert filters and EQs.
//!
//! Every filter and EQ band is a topology-preserving (zero-delay feedback)
//! state-variable section (Simper's SVF): low/high/band pass and notch from
//! its outputs, EQ bands as bells. Multi-pole filters cascade 2-pole
//! sections. Coefficients follow modulation at control rate ([`CONTROL`]
//! frames); the SVF stays stable under abrupt coefficient changes.
//!
//! Stored values are normalized knobs (0..=1, `$ENGINE_PAR_*` / 1e6); type
//! ids, value laws and their confidence are in `audits/EFFECTS.md`.

use super::{
    MAX_BLOCK,
    params::{Inputs, Mod, ModTable},
    voice::{Ahdsr, Envelope, Phase},
};
use crate::fx::{
    Chain, Kind, Params,
    blocks::{self, Fields, VoiceEffect},
    params::{EqBand, Value},
};
use crate::audio::Frame;
use crate::import::{Group, ModAssignment, ModTarget};

/// Frames between coefficient updates.
pub(crate) const CONTROL: usize = 32;
/// One filter or EQ unit per native group insert slot.
const MAX_UNITS: usize = 8;
/// Eight four-band EQs are the largest supported native insert chain.
const MAX_SECTIONS: usize = MAX_UNITS * 4;
/// Module envelopes and external assignments a voice follows.
const MAX_ENVS: usize = 4;
const MAX_EXT: usize = 8;
/// Knobs per unit: cutoff and resonance, or frequency, bandwidth and gain per band.
const KNOBS: usize = 12;

/// Legacy and state-variable filter cutoff: `43.6 Hz · 2^(8.96 x)`, 43.6 Hz
/// to 21.7 kHz (KSP community law `ep = 111607 · log2(f / 43.6)`).
const CUTOFF_MIN_HZ: f32 = 43.6;
const CUTOFF_OCTAVES: f32 = 8.96;
/// Q at resonance 0 (Butterworth per section) and the growth to resonance 1.
const Q_MIN: f32 = std::f32::consts::FRAC_1_SQRT_2;
const Q_SPAN: f32 = 28.0;
/// EQ knob ranges (Kontakt manual): 20 Hz..20 kHz, 0.3..3 octaves, ±18 dB.
const EQ_MIN_HZ: f32 = 20.0;
const EQ_DECADES: f32 = 3.0;
const BW_MIN: f32 = 0.3;
const BW_SPAN: f32 = 2.7;
const GAIN_DB: f32 = 18.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Response {
    Low,
    High,
    Band,
    Notch,
}

mod models;
pub(crate) use models::Model;

/// Kontakt filter type id → shape and 2-pole section count. Low/high
/// direction of 2..9 and 52..57 is medium confidence, pole counts low; the
/// modelled ids' evidence is in `audits/EFFECTS.md`.
pub(crate) fn filter_type(id: i32) -> Option<(Shape, u8)> {
    use Response::*;
    let ladder = |response, poles, compensate| Model::Ladder { response, poles, compensate };
    let svf = |response, sections| (Shape::Filter(response), sections);
    let model = match id {
        2 => return Some(svf(Low, 1)),
        3 => return Some(svf(High, 1)),
        4 => return Some(svf(Band, 1)),
        5 => return Some(svf(Low, 2)),
        6 => return Some(svf(High, 2)),
        7 => return Some(svf(Band, 2)),
        // 1000 was KONTRA's original scripted SV Notch id; keep old states playable.
        8 | SV_NOTCH4 | 1000 => return Some(svf(Notch, 2)),
        9 => return Some(svf(Low, 3)),
        52 => return Some(svf(Low, 1)),
        53 => return Some(svf(Band, 1)),
        54 => return Some(svf(High, 1)),
        55 => return Some(svf(Low, 2)),
        56 => return Some(svf(Band, 2)),
        57 => return Some(svf(High, 2)),
        13 => Model::Phaser,
        70 | 71 | 90 => Model::Formant,
        100 => ladder(Low, 2, true),
        101 => ladder(Low, 4, true),
        102 => ladder(High, 2, true),
        103 => ladder(High, 4, true),
        104 => ladder(Band, 2, true),
        105 => ladder(Band, 4, true),
        // NI's Daft filters have a 2-pole (12 dB/octave) response.
        // The SVF is a linear proxy; Massive's nonlinear gain is unmodelled.
        106 => return Some(svf(Low, 1)),
        107 => return Some(svf(High, 1)),
        _ => return None,
    };
    Some((Shape::Model(model), model.sections()))
}

/// Native id: factory snapshots pair selected SV Notch 4 groups with type 58.
const SV_NOTCH4: i32 = 58;

/// KSP `$FILTER_TYPE_*` constants: the type ids presets store, so scripts
/// read back what they set. 2..9 follow the reference's order; AR_LP2 is
/// 100 (ANALOG STRINGS sets it on exactly the groups storing 100), the
/// other AR and Daft ids continue that run in reference order and the
/// phaser and formant ids are the remaining ones its presets store (low
/// confidence). SV Notch 4 is 58 in native saved group state.
const KSP_FILTER_TYPES: &[(&str, i32)] = &[
    ("$FILTER_TYPE_LP2POLE", 2),
    ("$FILTER_TYPE_HP2POLE", 3),
    ("$FILTER_TYPE_BP2POLE", 4),
    ("$FILTER_TYPE_LP4POLE", 5),
    ("$FILTER_TYPE_HP4POLE", 6),
    ("$FILTER_TYPE_BP4POLE", 7),
    ("$FILTER_TYPE_BR4POLE", 8),
    ("$FILTER_TYPE_LP6POLE", 9),
    ("$FILTER_TYPE_PHASER", 13),
    ("$FILTER_TYPE_VERSATILE", 19),
    ("$FILTER_TYPE_FORMANT_1", 70),
    ("$FILTER_TYPE_FORMANT_2", 71),
    ("$FILTER_TYPE_AR_LP2", 100),
    ("$FILTER_TYPE_AR_LP4", 101),
    ("$FILTER_TYPE_AR_HP2", 102),
    ("$FILTER_TYPE_AR_HP4", 103),
    ("$FILTER_TYPE_AR_BP2", 104),
    ("$FILTER_TYPE_AR_BP4", 105),
    ("$FILTER_TYPE_DAFT_LP", 106),
    ("$FILTER_TYPE_DAFT_HP", 107),
    ("$FILTER_TYPE_SV_NOTCH4", SV_NOTCH4),
];

/// Value of a KSP `$FILTER_TYPE_*` constant.
pub fn ksp_filter_type(name: &str) -> Option<i32> {
    KSP_FILTER_TYPES.iter().find(|(n, _)| *n == name).map(|&(_, id)| id)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Shape {
    /// A state-variable response, the same on every section.
    Filter(Response),
    Model(Model),
    Eq,
    /// Solid G-EQ: LF shelf or bell, two bells, HF shelf or bell; each
    /// band's knobs `[gain, freq, q | bell]` (normalized).
    Geq,
}

/// Parameter of a filter, EQ or Stereo Modeller slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Knob {
    Cutoff,
    Resonance,
    Freq(u8),
    Bandwidth(u8),
    Gain(u8),
    /// 1.0 bypasses the slot (`$ENGINE_PAR_EFFECT_BYPASS`).
    Bypass,
    /// Linear output gain (`$ENGINE_PAR_INSERT_EFFECT_OUTPUT_GAIN`).
    Output,
    /// Stereo Modeller spread, -1 (mono) ..= 1 (`$ENGINE_PAR_STEREO`).
    Spread,
    /// Stereo Modeller pan, -1 ..= 1 (`$ENGINE_PAR_STEREO_PAN`).
    Pan,
    /// Formant size, the third knob of a formant filter.
    Size,
    /// The filter type id (`$ENGINE_PAR_EFFECT_SUBTYPE`).
    Type,
    /// Value `n` of a `kind` drive stage, normalized ([`blocks`]).
    Field(Kind, u8),
}

impl Knob {
    /// Modulation target names: `filterCutoff`, `eqGain2`...
    pub(crate) fn parse(name: &str) -> Option<Self> {
        let band = |rest: &str| rest.parse::<u8>().ok().filter(|b| (1..=3).contains(b)).map(|b| b - 1);
        Some(match name {
            "filterCutoff" | "formantTalk" => Self::Cutoff,
            "filterResonance" | "filterQ" | "formantSharp" => Self::Resonance,
            "formantSize" => Self::Size,
            _ => {
                if let Some(b) = name.strip_prefix("eqGain") {
                    Self::Gain(band(b)?)
                } else if let Some(b) = name.strip_prefix("eqFreq") {
                    Self::Freq(band(b)?)
                } else {
                    Self::Bandwidth(band(name.strip_prefix("eqBandwidth")?)?)
                }
            }
        })
    }

    /// Position among a unit's knobs; `None` for slot-level parameters.
    fn index(self) -> Option<usize> {
        Some(match self {
            Self::Cutoff => 0,
            Self::Resonance => 1,
            Self::Size => 2,
            Self::Freq(b) => 3 * b as usize,
            Self::Bandwidth(b) => 3 * b as usize + 1,
            Self::Gain(b) => 3 * b as usize + 2,
            Self::Field(_, n) => n as usize,
            Self::Bypass | Self::Output | Self::Spread | Self::Pan | Self::Type => return None,
        })
    }

    /// Whether a unit of `shape` with `sections` holds this knob.
    fn fits(self, shape: Shape, sections: u8) -> bool {
        match (shape, self) {
            (Shape::Filter(_) | Shape::Model(_), Self::Cutoff | Self::Resonance) => true,
            (Shape::Model(m), Self::Size) => m.knobs() == 3,
            (Shape::Geq, Self::Field(Kind::SolidGeq, n)) => (n as usize) < KNOBS,
            (Shape::Eq, Self::Freq(b) | Self::Bandwidth(b) | Self::Gain(b)) => b < sections,
            _ => false,
        }
    }
}

/// One filter or EQ of a group's insert rack.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Unit {
    pub(crate) slot: u8,
    pub(crate) shape: Shape,
    /// 2-pole sections (EQ: bands).
    pub(crate) sections: u8,
    /// Normalized knobs; scripts write them.
    pub(crate) knobs: [f32; KNOBS],
    pub(crate) bypass: bool,
    gain: f32,
    /// Kontakt filter type id (EQs: 22..24).
    kind: i32,
}

impl Unit {
    /// Section `b`'s knobs (clamped) from the unit's `knobs`, and whether
    /// it is a flat EQ band (an identity).
    fn key(&self, knobs: &[f32; KNOBS], b: usize) -> ([f32; 3], bool) {
        let k = knobs.map(|k| k.clamp(0.0, 1.0));
        match self.shape {
            Shape::Filter(_) => ([k[0], k[1], 0.0], false),
            Shape::Model(_) => ([k[0], k[1], k[2]], false),
            Shape::Eq => {
                let key = [k[3 * b], k[3 * b + 1], k[3 * b + 2]];
                (key, (key[2] - 0.5).abs() < FLAT)
            }
            Shape::Geq => {
                let key = [k[3 * b], k[3 * b + 1], k[3 * b + 2]];
                (key, (key[0] - 0.5).abs() * 2.0 * GEQ_DB < 0.01)
            }
        }
    }
}

/// The analog prototype a TPT section discretizes: prewarped `g`, damping
/// `k` and the mix `m` of its high, band and low outputs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Proto {
    g: f32,
    k: f32,
    m: [f32; 3],
}

/// A filter's cutoff (Hz) and Q from its normalized knobs.
pub(crate) fn filter_settings(cutoff: f32, resonance: f32) -> (f32, f32) {
    (CUTOFF_MIN_HZ * (CUTOFF_OCTAVES * cutoff).exp2(), Q_MIN * Q_SPAN.powf(resonance))
}

/// An EQ band's frequency (Hz), bandwidth (octaves) and gain (dB) from its
/// normalized knobs.
pub(crate) fn band_settings(freq: f32, bandwidth: f32, gain: f32) -> (f32, f32, f32) {
    (EQ_MIN_HZ * 10f32.powf(EQ_DECADES * freq), BW_MIN + BW_SPAN * bandwidth, GAIN_DB * (2.0 * gain - 1.0))
}

/// Signed boost/cut of the current Solid G-EQ implementation.
pub(crate) fn geq_gain_db(gain: f32) -> f32 {
    GEQ_DB * (2.0 * gain - 1.0)
}

impl Proto {
    fn filter(response: Response, hz: f32, q: f32, rate: f32) -> Self {
        let g = (std::f32::consts::PI * hz.min(0.49 * rate) / rate).tan();
        let k = 1.0 / q;
        let m = match response {
            Response::Low => [0.0, 0.0, 1.0],
            Response::High => [1.0, -k, -1.0],
            Response::Band => [0.0, 1.0, 0.0],
            Response::Notch => [1.0, -k, 0.0],
        };
        Self { g, k, m }
    }

    /// Peaking EQ band: `gain_db` at `hz`, `bw` octaves wide.
    fn bell(hz: f32, bw: f32, gain_db: f32, rate: f32) -> Self {
        let g = (std::f32::consts::PI * hz.min(0.49 * rate) / rate).tan();
        let a = 10f32.powf(gain_db / 40.0);
        let q = 1.0 / (2.0 * (std::f32::consts::LN_2 * 0.5 * bw).sinh());
        let k = 1.0 / (q * a);
        Self { g, k, m: [1.0, k * (a * a - 1.0), 0.0] }
    }

    /// A unit's section `b` from normalized knobs: `[cutoff, resonance, _]`
    /// (a model's third knob) or `[freq, bandwidth, gain]`.
    fn of(shape: Shape, key: [f32; 3], b: usize, rate: f32) -> Self {
        match shape {
            Shape::Model(model) => model.proto(key, b, rate),
            Shape::Filter(response) => {
                let (hz, q) = filter_settings(key[0], key[1]);
                Self::filter(response, hz, q, rate)
            }
            Shape::Eq => {
                let (hz, bw, db) = band_settings(key[0], key[1], key[2]);
                Self::bell(hz, bw, db, rate)
            }
            Shape::Geq => Self::geq(key, b, rate),
        }
    }

    /// Solid G-EQ band `b`: gain ±15 dB about 0.5, frequency over the
    /// band's range (log), Q 0.4..=4 (log) for the mid bells; the outer
    /// bands are shelves (Q 0.71) unless their bell switch is on.
    fn geq([gain, freq, shape]: [f32; 3], b: usize, rate: f32) -> Self {
        let (lo, hi) = GEQ_RANGES[b.min(3)];
        let hz = lo * (hi / lo).powf(freq);
        let a = 10f32.powf(geq_gain_db(gain) / 40.0);
        let g = (std::f32::consts::PI * hz.min(0.49 * rate) / rate).tan();
        let outer = b == 0 || b == 3;
        if !outer || shape >= 0.5 {
            let q = if outer { std::f32::consts::FRAC_1_SQRT_2 } else { 0.4 * 10f32.powf(shape) };
            let k = 1.0 / (q * a);
            return Self { g, k, m: [1.0, k * (a * a - 1.0), 0.0] };
        }
        // Simper's shelves: the corner moved by √A keeps the slope centred.
        let k = std::f32::consts::SQRT_2;
        if b == 0 {
            Self { g: g / a.sqrt(), k, m: [1.0, k * (a - 1.0), a * a - 1.0] }
        } else {
            Self { g: g * a.sqrt(), k, m: [a * a, k * (1.0 - a) * a, 1.0 - a * a] }
        }
    }

    /// `|H|` at `hz`. Trapezoidal integration is the bilinear transform, so
    /// the section's response is its prototype's,
    /// `(m0 (s² + k s + 1) + m1 s + m2) / (s² + k s + 1)`, at `s = jΩ` with
    /// `Ω = tan(π f / rate) / g`.
    pub(crate) fn gain(&self, hz: f32, rate: f32) -> f32 {
        let w = (std::f32::consts::PI * hz.min(0.499 * rate) / rate).tan() / self.g;
        let (re, im) = (1.0 - w * w, self.k * w);
        let [m0, m1, m2] = self.m;
        let (nr, ni) = (m0 * re + m2, m0 * im + m1 * w);
        ((nr * nr + ni * ni) / (re * re + im * im).max(1e-30)).sqrt()
    }
}

/// A channel-mixing slot: Stereo Modeller (`[spread, pan]`) or Inverter (`None`).
#[derive(Clone, Debug, PartialEq)]
struct Mixer {
    slot: u8,
    stereo: Option<[f32; 2]>,
    bypass: bool,
    gain: f32,
}

/// A per-voice nonlinear slot ([`Drive`]): Saturation, Distortion, Lo-Fi,
/// Skreamer, Tape Saturator.
#[derive(Clone, Debug, PartialEq)]
struct Stage {
    slot: u8,
    kind: Kind,
    fields: Fields,
    bypass: bool,
    gain: f32,
}

/// Drive stages a voice runs; later ones pass through.
// Kontakt group insert chains have eight slots, including bypassed modules
// that scripts may enable after import.
const MAX_STAGES: usize = 8;
/// Knob rows modulation reaches: the units', then the drive stages'.
const ROWS: usize = MAX_UNITS + MAX_STAGES;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Operation {
    Unit { index: u8, section: u8 },
    Stage(u8),
    Mixer(u8),
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Insert { slot: u8, operation: Operation }

/// A drive stage's value a modulation target names.
pub(crate) fn stage_knob(param: &str) -> Option<(Kind, u8)> {
    Some(match param {
        "shaper" => (Kind::SurroundPanner, 0),
        "distortionIntensity" => (Kind::Distortion, 1),
        "bitdepth" => (Kind::LoFi, 0),
        "downsample" => (Kind::LoFi, 1),
        _ => return None,
    })
}

/// A modulation route onto a unit's knob.
#[derive(Clone, Debug, PartialEq)]
struct Route {
    unit: u8,
    knob: u8,
    /// Invert button, read as a negative direction.
    sign: f32,
    /// Original internal target index (external routes do not use it).
    target: u16,
}

/// `[ll, lr, rl, rr]`: `l' = ll·l + lr·r`, `r' = rl·l + rr·r`.
type Matrix = [f32; 4];
const IDENTITY: Matrix = [1.0, 0.0, 0.0, 1.0];

#[derive(Clone, Copy)]
struct Amplifier<'a> { amp: &'a [f32], gains: [f32; 2], delta: [f32; 2] }
impl Amplifier<'_> {
    fn apply(self, start: usize, left: &mut [f32], right: &mut [f32]) {
        for (i, (l, r)) in left.iter_mut().zip(right).enumerate() {
            let frame = start + i;
            let a = self.amp[frame];
            *l *= a * (self.gains[0] + self.delta[0] * frame as f32);
            *r *= a * (self.gains[1] + self.delta[1] * frame as f32);
        }
    }
}


/// A group's filters, EQs and channel mixers with their modulation, shared
/// by its voices.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupFilter {
    units: Box<[Unit]>,
    mixers: Box<[Mixer]>,
    /// In slot order; the first [`MAX_STAGES`] play.
    stages: Box<[Stage]>,
    inserts: Box<[Insert]>,
    amp_split: Option<u8>,
    slot_matrices: [Matrix; 8],
    type_revision: u32,
    pre_active: bool,
    matrix_interleaved: bool,
    /// Stereo Modellers and every active slot's output gain as one matrix:
    /// they are linear and the filters treat both channels alike, so the
    /// order does not matter.
    matrix: Matrix,
    /// Module envelopes and what they drive (intensity and shaper in `Mod`),
    /// with their index in `Group::envelopes`.
    envs: Box<[(Ahdsr, Box<[(Route, Mod)]>, u8, bool)]>,
    /// External assignments: index into the group's `ModTable`.
    ext: Box<[(Route, u16)]>,
}

fn normalized(band: &EqBand) -> [f32; 3] {
    [
        (band.freq_hz / EQ_MIN_HZ).log10() / EQ_DECADES,
        (band.bandwidth_oct - BW_MIN) / BW_SPAN,
        (band.gain_db + GAIN_DB) / (2.0 * GAIN_DB),
    ]
}

/// Filters and EQs of known type, bypassed ones included (scripts switch
/// them on), in slot order.
fn units(chain: &Chain) -> impl Iterator<Item = Unit> + '_ {
    chain.slots.iter().filter_map(|fx| {
        let mut knobs = [0.0; KNOBS];
        let (shape, sections) = match &fx.params {
            Params::Filter(f) => {
                knobs[..5].copy_from_slice(&[f.cutoff, f.resonance, f.extra[0], f.extra[1], f.extra[2]]);
                filter_type(f.filter_type)?
            }
            Params::Eq(eq) => {
                for (k, band) in knobs.chunks_mut(3).zip(&eq.bands) {
                    k.copy_from_slice(&normalized(band));
                }
                (Shape::Eq, eq.bands.len() as u8)
            }
            p if fx.kind == Kind::SolidGeq => {
                knobs = blocks::fields(p)?;
                (Shape::Geq, 4)
            }
            _ => return None,
        };
        let kind = match &fx.params {
            Params::Filter(f) => f.filter_type,
            _ => 21 + i32::from(sections),
        };
        Some(Unit {
            slot: fx.slot as u8,
            shape,
            sections,
            knobs,
            bypass: fx.bypass,
            gain: fx.output_gain,
            kind,
        })
    })
}

/// Inverter phase/swap buttons (two flags, meaning unverified) that are set.
fn inverter_flags(params: &Params) -> bool {
    matches!(params, Params::Fields(f) if f.iter().any(|f| matches!(f.value, Value::Flag(true))))
}

/// Import warnings for group effects that do not play.
pub fn unsupported(chain: &Chain) -> Vec<String> { unsupported_at(chain, None) }

pub fn unsupported_at(chain: &Chain, amp_split: Option<u8>) -> Vec<String> {
    let mut out = Vec::new();
    for fx in chain.slots.iter().filter(|fx| !fx.bypass) {
        match &fx.params {
            Params::Filter(f) if filter_type(f.filter_type).is_none() => {
                out.push(format!("Group filter type {} is not implemented; audio passes through", f.filter_type));
            }
            Params::Filter(_) | Params::Eq(_) => {}
            _ if VoiceEffect::supports_at(fx.kind, amp_split) && blocks::fields(&fx.params).is_some() => {}
            _ if fx.kind == Kind::SolidGeq => {}
            Params::StereoModeller(s) if s.pseudo_stereo => {
                out.push("Group Stereo Modeller: pseudo stereo is not applied".into());
            }
            Params::StereoModeller(_) => {}
            p if fx.kind == Kind::Inverter => {
                if inverter_flags(p) {
                    out.push("Group Inverter: only its output gain is applied".into());
                }
            }
            _ => out.push(format!("Group insert effect {} is not applied", fx.kind.name())),
        }
    }
    if amp_split.is_none() && !chain.slots.is_empty() {
        out.push("Group insert Amplifier placement is unknown; existing inserts use post-Amplifier routing".into());
    }
    let (units, sections) = units(chain).fold((0, 0), |(u, s), unit| (u + 1, s + unit.sections as usize));
    if units > MAX_UNITS || sections > MAX_SECTIONS {
        out.push(format!("Only the first {MAX_UNITS} group filters ({MAX_SECTIONS} poles pairs) play"));
    }
    out
}

impl GroupFilter {
    /// `None` when the group has no filter, EQ, Stereo Modeller or Inverter.
    pub(crate) fn new(group: &Group) -> Option<Box<Self>> {
        let mut units_: Vec<Unit> = Vec::new();
        let mut sections = 0;
        for unit in units(&group.fx) {
            sections += unit.sections as usize;
            if units_.len() == MAX_UNITS || sections > MAX_SECTIONS {
                break;
            }
            units_.push(unit);
        }
        units_.sort_unstable_by_key(|unit| unit.slot);
        let units = units_;
        let mut mixers: Box<[Mixer]> = group
            .fx
            .slots
            .iter()
            .filter_map(|fx| {
                let stereo = match &fx.params {
                    Params::StereoModeller(s) => Some([s.spread, s.pan]),
                    _ if fx.kind == Kind::Inverter => None,
                    _ => return None,
                };
                Some(Mixer {
                    slot: fx.slot as u8,
                    stereo,
                    bypass: fx.bypass,
                    gain: fx.output_gain,
                })
            })
            .collect();
        mixers.sort_unstable_by_key(|mixer| mixer.slot);
        let mut stages: Box<[Stage]> = (group.fx.slots.iter())
            .filter(|fx| VoiceEffect::supports_at(fx.kind, group.amp_split_slot))
            .filter_map(|fx| {
                Some(Stage {
                    slot: fx.slot as u8,
                    kind: fx.kind,
                    fields: blocks::fields(&fx.params)?,
                    bypass: fx.bypass,
                    gain: fx.output_gain,
                })
            })
            .take(MAX_STAGES)
            .collect();
        stages.sort_unstable_by_key(|stage| stage.slot);
        if units.is_empty() && mixers.is_empty() && stages.is_empty() {
            return None;
        }
        // Drive stages are routed as rows after the units'.
        let route = |m: &ModAssignment, target| {
            let ModTarget::Module { param, slot } = &m.target else {
                return None;
            };
            let sign = if m.invert { -1.0 } else { 1.0 };
            if let Some((kind, n)) = stage_knob(param) {
                let i = stages.iter().position(|s| s.slot == *slot && s.kind == kind)?;
                return Some(Route { unit: (units.len() + i) as u8, knob: n, sign, target });
            }
            let knob = Knob::parse(param)?;
            let unit = units.iter().position(|u| u.slot == *slot && knob.fits(u.shape, u.sections))?;
            Some(Route { unit: unit as u8, knob: knob.index()? as u8, sign, target })
        };
        let envs = (group.envelopes.iter().enumerate())
            .filter_map(|(i, e)| {
                let routes: Box<[_]> = e.targets.iter().enumerate().filter_map(|(t, m)| Some((route(m, t as u16)?, Mod::from(m)))).collect();
                (!routes.is_empty()).then(|| (Ahdsr::from(&e.env), routes, i as u8, false))
            })
            .take(MAX_ENVS)
            .collect();
        let ext = group
            .mods
            .iter()
            .enumerate()
            .filter_map(|(i, m)| Some((route(m, 0)?, i as u16)))
            .take(MAX_EXT)
            .collect();
        let mut out = Box::new(Self {
            units: units.into(),
            mixers,
            stages,
            inserts: [].into(),
            amp_split: group.amp_split_slot,
            slot_matrices: [IDENTITY; 8],
            type_revision: 0,
            pre_active: false,
            matrix_interleaved: false,
            matrix: IDENTITY,
            envs,
            ext,
        });
        out.compile_inserts();
        Some(out)
    }

    fn compile_inserts(&mut self) {
        let mut inserts = Vec::with_capacity(8);
        let mut section = 0;
        for (index, unit) in self.units.iter().enumerate() {
            if unit.slot < 8 { inserts.push(Insert { slot: unit.slot, operation: Operation::Unit { index: index as u8, section } }); }
            section += unit.sections;
        }
        for (index, stage) in self.stages.iter().enumerate() {
            if stage.slot < 8 { inserts.push(Insert { slot: stage.slot, operation: Operation::Stage(index as u8) }); }
        }
        for (index, mixer) in self.mixers.iter().enumerate() {
            if mixer.slot < 8 { inserts.push(Insert { slot: mixer.slot, operation: Operation::Mixer(index as u8) }); }
        }
        inserts.sort_unstable_by_key(|op| op.slot);
        self.inserts = inserts.into();
        self.refresh_matrices();
    }

    fn refresh_matrices(&mut self) {
        self.matrix = self.mix();
        self.slot_matrices.fill(IDENTITY);
        self.pre_active = false;
        self.matrix_interleaved = false;
        let mut matrix_before = false;
        for insert in &self.inserts {
            let (bypass, gain, stereo) = match insert.operation {
                Operation::Unit { index, .. } => { let u = &self.units[index as usize]; (u.bypass, u.gain, None) }
                Operation::Stage(index) => { let s = &self.stages[index as usize]; (s.bypass, s.gain, None) }
                Operation::Mixer(index) => { let m = &self.mixers[index as usize]; (m.bypass, m.gain, m.stereo) }
            };
            if bypass { continue; }
            self.pre_active |= self.amp_split.is_some_and(|split| insert.slot < split);
            self.matrix_interleaved |= matrix_before && matches!(insert.operation, Operation::Unit { .. });
            let mut matrix = IDENTITY;
            if let Some([spread, pan]) = stereo {
                let w = (1.0 + spread).clamp(0.0, 2.0);
                let (same, other) = (0.5 * (1.0 + w), 0.5 * (1.0 - w));
                let (bl, br) = ((1.0 - pan).clamp(0.0, 1.0), (1.0 + pan).clamp(0.0, 1.0));
                matrix = [bl * same, bl * other, br * other, br * same];
            }
            self.slot_matrices[insert.slot as usize] = matrix.map(|x| x * gain);
            matrix_before |= self.slot_matrices[insert.slot as usize] != IDENTITY;
        }
    }

    /// The stereo matrix of the active mixers and output gains. The Stereo
    /// Modeller scales the side signal by `1 + spread`, then balances with
    /// the rack's law (the far side attenuates linearly).
    fn mix(&self) -> Matrix {
        let stages = self.stages.iter().filter(|s| !s.bypass).map(|s| s.gain);
        let gains = self.units.iter().filter(|u| !u.bypass).map(|u| u.gain).chain(stages);
        let active = self.mixers.iter().filter(|m| !m.bypass);
        let gain: f32 = gains.chain(active.clone().map(|m| m.gain)).product();
        let mut m = IDENTITY;
        for [spread, pan] in active.filter_map(|m| m.stereo) {
            let w = (1.0 + spread).clamp(0.0, 2.0);
            let (same, other) = (0.5 * (1.0 + w), 0.5 * (1.0 - w));
            let [bl, br] = [(1.0 - pan).clamp(0.0, 1.0), (1.0 + pan).clamp(0.0, 1.0)];
            let [ll, lr, rl, rr] = m;
            m = [
                bl * (same * ll + other * rl),
                bl * (same * lr + other * rr),
                br * (other * ll + same * rl),
                br * (other * lr + same * rr),
            ];
        }
        m.map(|x| x * gain)
    }

    /// Filters and EQs, in slot order.
    pub(crate) fn units(&self) -> &[Unit] {
        &self.units
    }

    /// Magnitude of the whole chain at `hz` with its knobs as set (no
    /// modulation), bypassed units and flat bands left out as `process` does.
    pub(crate) fn magnitude(&self, hz: f32, rate: f32) -> f32 {
        let mut gain = 1.0;
        for unit in self.units.iter().filter(|u| !u.bypass) {
            for b in 0..unit.sections as usize {
                let (key, flat) = unit.key(&unit.knobs, b);
                if !flat {
                    gain *= Proto::of(unit.shape, key, b, rate).gain(hz, rate);
                }
            }
        }
        gain
    }

    /// A slot's stored parameter (`get_engine_par`).
    pub(crate) fn knob(&self, slot: u8, knob: Knob) -> Option<f32> {
        if let Some(st) = self.stages.iter().find(|s| s.slot == slot) {
            return match knob {
                Knob::Bypass => Some(f32::from(st.bypass)),
                Knob::Output => Some(st.gain),
                Knob::Field(kind, n) if kind == st.kind => {
                    Some(blocks::normalized(kind, n, *st.fields.get(n as usize)?))
                }
                _ => None,
            };
        }
        let unit = self.units.iter().find(|u| u.slot == slot);
        let mixer = self.mixers.iter().find(|m| m.slot == slot);
        match knob {
            Knob::Bypass => unit.map(|u| u.bypass).or(mixer.map(|m| m.bypass)).map(f32::from),
            Knob::Output => unit.map(|u| u.gain).or(mixer.map(|m| m.gain)),
            Knob::Spread | Knob::Pan => Some(mixer?.stereo?[usize::from(knob == Knob::Pan)]),
            Knob::Type => unit.map(|u| u.kind as f32),
            _ => {
                let unit = unit.filter(|u| knob.fits(u.shape, u.sections))?;
                Some(unit.knobs[knob.index()?])
            }
        }
    }

    /// Module envelope `i` of the group (`Group::envelopes`), if it drives
    /// anything here; voices starting later follow changes to it.
    pub(crate) fn envelope(&mut self, i: u8) -> Option<&mut Ahdsr> {
        self.envs.iter_mut().find(|e| e.2 == i).map(|e| &mut e.0)
    }

    pub(crate) fn envelope_at(&self, i: u8) -> Option<&Ahdsr> {
        self.envs.iter().find(|e| e.2 == i).map(|e| &e.0)
    }

    pub(crate) fn envelope_bypass(&mut self, i: u8) -> Option<&mut bool> {
        self.envs.iter_mut().find(|e| e.2 == i).map(|e| &mut e.3)
    }

    pub(crate) fn envelope_bypass_at(&self, i: u8) -> Option<bool> {
        self.envs.iter().find(|e| e.2 == i).map(|e| e.3)
    }

    pub(crate) fn envelope_mod(&mut self, i: u8, target: u16) -> Option<&mut Mod> {
        self.envs.iter_mut().find(|e| e.2 == i)?.1.iter_mut()
            .find(|(r, _)| r.target == target).map(|(_, m)| m)
    }

    pub(crate) fn envelope_mod_at(&self, i: u8, target: u16) -> Option<&Mod> {
        self.envs.iter().find(|e| e.2 == i)?.1.iter()
            .find(|(r, _)| r.target == target).map(|(_, m)| m)
    }

    /// Switch a filter slot to another filter type. The voices' section
    /// states stay where they are, so a switch that changes the section
    /// count rearranges them for the notes playing.
    fn set_type(&mut self, slot: u8, kind: i32) -> bool {
        let total: usize = self.units.iter().map(|u| u.sections as usize).sum();
        let Some(u) = self.units.iter_mut().find(|u| u.slot == slot && !matches!(u.shape, Shape::Eq | Shape::Geq)) else {
            return false;
        };
        match filter_type(kind) {
            Some((shape, sections)) if total - u.sections as usize + sections as usize <= MAX_SECTIONS => {
                (u.shape, u.sections, u.kind) = (shape, sections, kind);
                self.type_revision = self.type_revision.wrapping_add(1);
                let mut section = 0;
                for insert in &mut self.inserts {
                    if let Operation::Unit { index, section: start } = &mut insert.operation {
                        *start = section;
                        section += self.units[*index as usize].sections;
                    }
                }
                true
            }
            _ => false,
        }
    }

    /// Set a slot's parameter (`set_engine_par`); false if the slot lacks it.
    pub(crate) fn set_knob(&mut self, slot: u8, knob: Knob, value: f32) -> bool {
        if knob == Knob::Type {
            return self.set_type(slot, value as i32);
        }
        if let Some(st) = self.stages.iter_mut().find(|s| s.slot == slot) {
            match knob {
                Knob::Bypass => st.bypass = value != 0.0,
                Knob::Output => st.gain = value.max(0.0),
                Knob::Field(kind, n) if kind == st.kind && (n as usize) < st.fields.len() => {
                    st.fields[n as usize] = blocks::stored(kind, n, value);
                    return true;
                }
                _ => return false,
            }
            self.refresh_matrices();
            return true;
        }
        let unit = self.units.iter_mut().find(|u| u.slot == slot);
        let mixer = self.mixers.iter_mut().find(|m| m.slot == slot);
        match (knob, unit, mixer) {
            (Knob::Bypass, Some(u), _) => u.bypass = value != 0.0,
            (Knob::Bypass, None, Some(m)) => m.bypass = value != 0.0,
            (Knob::Output, Some(u), _) => u.gain = value.max(0.0),
            (Knob::Output, None, Some(m)) => m.gain = value.max(0.0),
            (Knob::Spread | Knob::Pan, _, Some(Mixer { stereo: Some(s), .. })) => {
                s[usize::from(knob == Knob::Pan)] = value.clamp(-1.0, 1.0);
            }
            (_, Some(u), _) if knob.fits(u.shape, u.sections) => {
                u.knobs[knob.index().unwrap_or(0)] = value.clamp(0.0, 1.0);
                return true;
            }
            _ => return false,
        }
        self.refresh_matrices();
        true
    }
}

/// One 2-pole TPT state-variable section, stereo.
#[derive(Clone, Copy, Debug, Default)]
struct Section {
    /// `a1, a2, a3, m0, m1, m2` (Simper's notation).
    c: [f32; 6],
    /// `ic1eq, ic2eq` for left, then right.
    s: [f32; 4],
}

impl Section {
    #[cfg(test)]
    fn set(&mut self, response: Response, hz: f32, q: f32, rate: f32) {
        self.coefficients(Proto::filter(response, hz, q, rate));
    }

    #[cfg(test)]
    fn set_bell(&mut self, hz: f32, bw: f32, gain_db: f32, rate: f32) {
        self.coefficients(Proto::bell(hz, bw, gain_db, rate));
    }

    fn coefficients(&mut self, Proto { g, k, m }: Proto) {
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        self.c = [a1, a2, g * a2, m[0], m[1], m[2]];
    }

    /// Uses an AVX build when the CPU has it: the same arithmetic, in
    /// three-operand instructions that drop the SSE register copies.
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        #[cfg(target_arch = "x86_64")]
        if std::arch::is_x86_feature_detected!("avx") {
            // SAFETY: the running CPU supports AVX.
            return unsafe { self.process_avx(left, right) };
        }
        self.process_body(left, right);
    }

    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx")]
    fn process_avx(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.process_body(left, right);
    }

    #[inline(always)]
    fn process_body(&mut self, left: &mut [f32], right: &mut [f32]) {
        let [a1, a2, a3, m0, m1, m2] = self.c;
        let [mut l1, mut l2, mut r1, mut r2] = self.s;
        // The loop is latency-bound. The state update `s' = 2v - s` is
        // expanded so each sample's dependency chain is a subtract, a
        // multiply and an add.
        let (b1, b2, b3) = (2.0 * a1 - 1.0, 2.0 * a2, 2.0 * a3);
        #[cfg(target_arch = "x86_64")]
        {
            // As a state-space system per channel, `s' = s + E s + B x` and
            // `y = C s + D x`, which steps two frames as
            // `s'' = s + (E² + 2E) s + (B + E B) x + B x'`. The two low lanes
            // of an SSE register step left and right one frame, the high two
            // step them two frames from the same state, so the loop-carried
            // chain (a multiply and two adds) runs once per two frames.
            // Increments on `s` rather than `A = I + E` keep low cutoffs,
            // where `E` is tiny, as exact as the one-frame update.
            use std::arch::x86_64::*;
            let (e11, e12, e21, e22) = (b1 - 1.0, -b2, b2, -b3);
            let f11 = e11 * e11 + e12 * e21 + 2.0 * e11;
            let f12 = e11 * e12 + e12 * e22 + 2.0 * e12;
            let f21 = e21 * e11 + e22 * e21 + 2.0 * e21;
            let f22 = e21 * e12 + e22 * e22 + 2.0 * e22;
            let (g1, g2) = (b2 + e11 * b2 + e12 * b3, b3 + e21 * b2 + e22 * b3);
            let (c1, c2, d) = (m1 * a1 + m2 * a2, m2 * (1.0 - a3) - m1 * a2, m0 + m1 * a2 + m2 * a3);
            // SAFETY: SSE2 is baseline on x86_64; the 64-bit loads and
            // stores are unaligned-safe and cover two frames of a chunk.
            unsafe {
                let pair = |one: f32, two: f32| _mm_setr_ps(one, one, two, two);
                let (p11, p12, p21, p22) = (pair(e11, f11), pair(e12, f12), pair(e21, f21), pair(e22, f22));
                let (q1, q2, r1_, r2_) = (pair(b2, g1), pair(b3, g2), pair(0.0, b2), pair(0.0, b3));
                let (c1, c2, d) = (_mm_set1_ps(c1), _mm_set1_ps(c2), _mm_set1_ps(d));
                let (mut s1, mut s2) = (_mm_setr_ps(l1, r1, l1, r1), _mm_setr_ps(l2, r2, l2, r2));
                // Both states from `s` and the inputs' terms `u`.
                let step = |s1: __m128, s2: __m128, u1: __m128, u2: __m128| {
                    let t1 = _mm_add_ps(_mm_add_ps(_mm_mul_ps(p11, s1), u1), _mm_add_ps(_mm_mul_ps(p12, s2), s1));
                    let t2 = _mm_add_ps(_mm_add_ps(_mm_mul_ps(p21, s1), u2), _mm_add_ps(_mm_mul_ps(p22, s2), s2));
                    (t1, t2)
                };
                let out = |s1: __m128, s2: __m128, x: __m128| {
                    _mm_add_ps(_mm_add_ps(_mm_mul_ps(c1, s1), _mm_mul_ps(c2, s2)), _mm_mul_ps(d, x))
                };
                let n = left.len().min(right.len());
                let (ls, l_rest) = left[..n].as_chunks_mut::<2>();
                let (rs, r_rest) = right[..n].as_chunks_mut::<2>();
                for (l, r) in ls.iter_mut().zip(rs) {
                    let load = |p: &[f32; 2]| _mm_castsi128_ps(_mm_loadl_epi64(p.as_ptr().cast()));
                    // [l0, r0, l1, r1]
                    let x = _mm_unpacklo_ps(load(l), load(r));
                    let (x0, x1) = (_mm_movelh_ps(x, x), _mm_movehl_ps(x, x));
                    let u1 = _mm_add_ps(_mm_mul_ps(q1, x0), _mm_mul_ps(r1_, x1));
                    let u2 = _mm_add_ps(_mm_mul_ps(q2, x0), _mm_mul_ps(r2_, x1));
                    let (t1, t2) = step(s1, s2, u1, u2);
                    // States at both frames, then the outputs [l0, l1, r0, r1].
                    let y = out(_mm_movelh_ps(s1, t1), _mm_movelh_ps(s2, t2), x);
                    let y = _mm_shuffle_ps(y, y, 0b11_01_10_00);
                    _mm_storel_epi64(l.as_mut_ptr().cast(), _mm_castps_si128(y));
                    _mm_storel_epi64(r.as_mut_ptr().cast(), _mm_castps_si128(_mm_movehl_ps(y, y)));
                    (s1, s2) = (_mm_movehl_ps(t1, t1), _mm_movehl_ps(t2, t2));
                }
                // An odd last frame steps once, on the low lanes.
                if let (Some(l), Some(r)) = (l_rest.first_mut(), r_rest.first_mut()) {
                    let x = _mm_unpacklo_ps(_mm_load_ss(l), _mm_load_ss(r));
                    let x0 = _mm_movelh_ps(x, x);
                    let (t1, t2) = step(s1, s2, _mm_mul_ps(q1, x0), _mm_mul_ps(q2, x0));
                    let y = out(s1, s2, x);
                    _mm_store_ss(l, y);
                    _mm_store_ss(r, _mm_shuffle_ps(y, y, 1));
                    (s1, s2) = (_mm_movelh_ps(t1, t1), _mm_movelh_ps(t2, t2));
                }
                let (mut a, mut b) = ([0.0; 4], [0.0; 4]);
                _mm_storeu_ps(a.as_mut_ptr(), s1);
                _mm_storeu_ps(b.as_mut_ptr(), s2);
                (l1, r1, l2, r2) = (a[0], a[1], b[0], b[1]);
            }
        }
        #[cfg(not(target_arch = "x86_64"))]
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let v3 = *l - l2;
            let v1 = a1 * l1 + a2 * v3;
            let v2 = l2 + a2 * l1 + a3 * v3;
            (l1, l2) = (b1 * l1 + b2 * v3, l2 + b2 * l1 + b3 * v3);
            *l = m0 * *l + m1 * v1 + m2 * v2;
            let v3 = *r - r2;
            let v1 = a1 * r1 + a2 * v3;
            let v2 = r2 + a2 * r1 + a3 * v3;
            (r1, r2) = (b1 * r1 + b2 * v3, r2 + b2 * r1 + b3 * v3);
            *r = m0 * *r + m1 * v1 + m2 * v2;
        }
        // Flush denormals once per call: decaying states would otherwise slow down.
        self.s = [l1, l2, r1, r2].map(|v| if v.abs() < 1e-20 { 0.0 } else { v });
    }
}

/// Saved rack knobs use the same units as the group filter.
pub(crate) fn effect_knob(fx: &crate::fx::Effect, knob: Knob) -> Option<f32> {
    match (&fx.params, knob) {
        (Params::StereoModeller(s), Knob::Spread) => Some(s.spread),
        (Params::StereoModeller(s), Knob::Pan) => Some(s.pan),
        (Params::Filter(f), Knob::Type) => Some(f.filter_type as f32),
        (Params::Filter(f), k) if k.fits(filter_type(f.filter_type)?.0, 0) => {
            [f.cutoff, f.resonance, f.extra[0]].get(k.index()?).copied()
        }
        (Params::Eq(eq), k) if k.fits(Shape::Eq, eq.bands.len() as u8) => {
            let i = k.index()?;
            Some(normalized(eq.bands.get(i / 3)?)[i % 3])
        }
        _ => None,
    }
}

/// Offline convolution IR shaping with the same non-resonant SVF as the racks.
/// Pad the decay before filtering so a short IR does not truncate the poles.
pub(crate) fn filter_ir(ir: &mut [Vec<f32>; 2], low: f32, high: f32, rate: f32) {
    let highpass = low > 20.5;
    let lowpass = high < 19_990.0;
    if !highpass && !lowpass { return }
    let cutoff = match (highpass, lowpass) { (true, true) => low.min(high), (true, false) => low, _ => high }.clamp(20.0, rate * 0.49);
    // A Butterworth pole's envelope falls as exp(-2*pi*cutoff*t/sqrt(2)).
    let tail = (rate * 16.0 / (2.0 * std::f32::consts::PI * cutoff * Q_MIN)).ceil() as usize;
    for channel in ir.iter_mut() { channel.resize(channel.len() + tail, 0.0); }
    let [left, right] = ir;
    for (response, hz, enabled) in [(Response::High, low, highpass), (Response::Low, high, lowpass)] {
        if enabled {
            let mut section = Section::default();
            section.coefficients(Proto::filter(response, hz.clamp(20.0, rate * 0.49), Q_MIN, rate));
            section.process(left, right);
        }
    }
}

/// A Filter/EQ effect in an instrument rack or bus: the group filter's
/// sections at the stored knobs.
pub(crate) struct RackFilter {
    unit: Unit,
    rate: f32,
    sections: [Section; 4],
    active: usize,
}

impl RackFilter {
    /// `None` for an unknown filter type.
    pub(crate) fn new(fx: &crate::fx::Effect, rate: f32) -> Option<Self> {
        let chain = Chain { slots: vec![fx.clone()] };
        let unit = units(&chain).next()?;
        let mut out = Self { unit, rate, sections: [Section::default(); 4], active: 0 };
        out.tune();
        Some(out)
    }

    fn tune(&mut self) {
        let unit = &self.unit;
        self.active = 0;
        for b in 0..(unit.sections as usize).min(4) {
            let (key, flat) = unit.key(&unit.knobs, b);
            if !flat {
                self.sections[self.active].coefficients(Proto::of(unit.shape, key, b, self.rate));
                self.active += 1;
            }
        }
    }

    pub(crate) fn set_knob(&mut self, knob: Knob, value: f32) -> bool {
        if knob == Knob::Type && !matches!(self.unit.shape, Shape::Eq | Shape::Geq) {
            if self.unit.kind == value as i32 { return true }
            let Some((shape, sections)) = filter_type(value as i32) else { return false };
            (self.unit.shape, self.unit.sections, self.unit.kind) = (shape, sections, value as i32);
            self.clear();
        } else if knob.fits(self.unit.shape, self.unit.sections) {
            self.unit.knobs[knob.index().unwrap()] = value.clamp(0.0, 1.0);
        } else {
            return false;
        }
        self.tune();
        true
    }

    pub(crate) fn knob(&self, knob: Knob) -> Option<f32> {
        if knob == Knob::Type && !matches!(self.unit.shape, Shape::Eq | Shape::Geq) {
            return Some(self.unit.kind as f32);
        }
        knob.fits(self.unit.shape, self.unit.sections).then(|| self.unit.knobs[knob.index().unwrap()])
    }

    /// Set knob `n` (normalized), as [`GroupFilter::set_knob`] does with
    /// [`Knob::Field`]; false if the unit has no such knob.
    pub(crate) fn set(&mut self, kind: Kind, n: u8, value: f32) -> bool {
        self.set_knob(Knob::Field(kind, n), value)
    }

    pub(crate) fn get(&self, kind: Kind, n: u8) -> Option<f32> {
        self.knob(Knob::Field(kind, n))
    }

    pub(crate) fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for s in &mut self.sections[..self.active] {
            s.process(left, right);
        }
    }

    pub(crate) fn clear(&mut self) {
        self.sections.iter_mut().for_each(|s| s.s = [0.0; 4]);
    }
}

/// Solid G-EQ gain span (±dB) and band ranges (Hz).
const GEQ_DB: f32 = 15.0;
const GEQ_RANGES: [(f32, f32); 4] = [(30.0, 450.0), (200.0, 2500.0), (600.0, 7000.0), (1500.0, 16_000.0)];

/// EQ gain knobs this close to 0.5 (0 dB, ±0.01 dB) make a band an identity.
const FLAT: f32 = 0.01 / (2.0 * GAIN_DB);

/// Apply `m` to one block, ramping linearly from `from` when it differs.
fn apply(from: Matrix, m: Matrix, left: &mut [f32], right: &mut [f32]) {
    if from == m {
        if m[1] == 0.0 && m[2] == 0.0 {
            left.iter_mut().for_each(|l| *l *= m[0]);
            right.iter_mut().for_each(|r| *r *= m[3]);
        } else {
            for (l, r) in left.iter_mut().zip(right.iter_mut()) {
                (*l, *r) = (m[0] * *l + m[1] * *r, m[2] * *l + m[3] * *r);
            }
        }
        return;
    }
    let n = left.len() as f32;
    let step: Matrix = std::array::from_fn(|i| (m[i] - from[i]) / n);
    let mut c = from;
    for (l, r) in left.iter_mut().zip(right.iter_mut()) {
        c = std::array::from_fn(|i| c[i] + step[i]);
        (*l, *r) = (c[0] * *l + c[1] * *r, c[2] * *l + c[3] * *r);
    }
}

/// A voice's filter state: its module envelopes, lagged external sources,
/// sections, the knobs each section is tuned for and the stereo matrix
/// reached. Empty (no work) for voices of groups without filters.
#[derive(Clone, Copy, Debug)]
pub(crate) struct VoiceFilter {
    envs: [Envelope; MAX_ENVS],
    ext: [f32; MAX_EXT],
    sections: [Section; MAX_SECTIONS],
    /// Knobs of each section's last coefficients (NaN: none yet), so static
    /// settings cost no coefficient math.
    tuned: [[f32; 3]; MAX_SECTIONS],
    drives: [VoiceEffect; MAX_STAGES],
    slot_matrices: [Matrix; 8],
    type_revision: u32,
    /// The values each drive is tuned for (NaN: none yet).
    drive_fields: [Fields; MAX_STAGES],
    /// The last [`VoiceFilter::hold`]: the filter, and its active sections
    /// in order.
    pub held: FilterKey,
    slots: [u8; LANE_SECTIONS],
}

impl VoiceFilter {
    pub fn new(filter: Option<&GroupFilter>, table: &ModTable, input: &Inputs, rate: f32) -> Self {
        let mut out = Self {
            envs: [Envelope::new(&Ahdsr::UNITY, rate); MAX_ENVS],
            ext: [0.0; MAX_EXT],
            sections: [Section::default(); MAX_SECTIONS],
            tuned: [[f32::NAN; 3]; MAX_SECTIONS],
            drives: [VoiceEffect::default(); MAX_STAGES],
            slot_matrices: [IDENTITY; 8],
            type_revision: 0,
            drive_fields: [[f32::NAN; blocks::FIELDS]; MAX_STAGES],
            held: FilterKey::default(),
            slots: [0; LANE_SECTIONS],
        };
        if let Some(f) = filter {
            for (env, (params, ..)) in out.envs.iter_mut().zip(&f.envs) {
                *env = Envelope::new(params, rate);
            }
            for (value, (_, i)) in out.ext.iter_mut().zip(&f.ext) {
                *value = table.mods[*i as usize].start_value(input);
            }
            out.slot_matrices = f.slot_matrices;
            out.type_revision = f.type_revision;
        }
        out
    }

    fn check_type_revision(&mut self, f: &GroupFilter) {
        if self.type_revision != f.type_revision {
            self.tuned.fill([f32::NAN; 3]);
            self.type_revision = f.type_revision;
        }
    }

    /// Move external sources over `n` frames before [`process_amplified`](Self::process_amplified).
    #[inline(always)]
    pub fn follow(&mut self, f: &GroupFilter, table: &ModTable, input: &Inputs, n: usize, rate: f32) {
        for (value, (_, i)) in self.ext.iter_mut().zip(&f.ext) {
            table.mods[*i as usize].follow(value, input, n, rate);
        }
    }

    /// Work out this block's [`FilterKey`] into `held` when the filter is
    /// held: no module envelopes and the matrix reached, with at most four
    /// active sections. Tunes the sections it keeps. Returns the key's hash.
    #[inline(always)]
    pub fn hold(&mut self, f: &GroupFilter, table: &ModTable, rate: f32) -> Option<u64> {
        self.check_type_revision(f);
        // Drives are nonlinear: voices cannot share them.
        if f.pre_active || f.matrix_interleaved
            || !f.envs.is_empty() || self.slot_matrices != f.slot_matrices || f.stages.iter().any(|s| !s.bypass) {
            return None;
        }
        let mut knobs: [[f32; KNOBS]; ROWS] =
            std::array::from_fn(|u| f.units.get(u).map_or([0.0; KNOBS], |unit| unit.knobs));
        for ((r, i), value) in f.ext.iter().zip(&self.ext) {
            knobs[r.unit as usize][r.knob as usize] += r.sign * table.mods[*i as usize].intensity * value;
        }
        let key = &mut self.held;
        key.matrix = f.matrix;
        let (mut s, mut active) = (0, 0);
        for (unit, knobs) in f.units.iter().zip(&knobs) {
            for b in 0..unit.sections as usize {
                let (k, flat) = unit.key(knobs, b);
                if unit.bypass || flat {
                    continue;
                }
                if active == LANE_SECTIONS {
                    return None;
                }
                if k != self.tuned[s + b] {
                    self.tuned[s + b] = k;
                    self.sections[s + b].coefficients(Proto::of(unit.shape, k, b, rate));
                }
                key.c[active] = self.sections[s + b].c;
                self.slots[active] = (s + b) as u8;
                active += 1;
            }
            s += unit.sections as usize;
        }
        key.active = active as u8;
        // Sections past the active ones do not count.
        key.c[active..].fill([0.0; 6]);
        Some(key.hash())
    }

    pub fn release(&mut self) {
        self.envs.iter_mut().for_each(|e| e.release(None));
    }

    /// Clear the sections' state, as silent input leaves it.
    pub fn rest(&mut self) {
        self.sections.iter_mut().for_each(|s| s.s = [0.0; 4]);
        self.drives.iter_mut().for_each(VoiceEffect::clear);
    }

    /// Run drive stage `i` over one stretch of frames; `m` is the
    /// modulation of each of its values (normalized).
    #[inline]
    fn drive(&mut self, f: &GroupFilter, i: usize, m: &[f32; KNOBS], left: &mut [f32], right: &mut [f32], rate: f32) {
        let stage = &f.stages[i];
        if stage.bypass {
            self.drives[i].clear();
            return;
        }
        let mut fields = stage.fields;
        for (n, (v, d)) in fields.iter_mut().zip(m).enumerate().filter(|(_, (_, d))| **d != 0.0) {
            let n = n as u8;
            *v = blocks::stored(stage.kind, n, blocks::normalized(stage.kind, n, *v) + d);
        }
        // Bitwise, so the NaN start compares unequal once.
        if fields.map(f32::to_bits) != self.drive_fields[i].map(f32::to_bits) {
            self.drive_fields[i] = fields;
            self.drives[i].tune(stage.kind, &fields, rate);
        }
        self.drives[i].process(left, right);
    }

    /// Process inserts without an Amplifier (reference tests).
    #[cfg(test)]
    pub fn process(&mut self, f: &GroupFilter, table: &ModTable, ctl: &mut [f32; MAX_BLOCK], left: &mut [f32], right: &mut [f32], rate: f32) {
        self.process_chain(f, table, ctl, left, right, rate, None);
    }

    /// Process native insert slots with the Amplifier at its stored split.
    #[allow(clippy::too_many_arguments)]
    pub fn process_amplified(&mut self, f: &GroupFilter, table: &ModTable, ctl: &mut [f32; MAX_BLOCK], left: &mut [f32], right: &mut [f32], rate: f32, amp: &[f32], gains: [f32; 2], delta: [f32; 2]) {
        self.process_chain(f, table, ctl, left, right, rate, Some(Amplifier { amp, gains, delta }));
    }

    /// Filter one block (at most [`MAX_BLOCK`] frames) in place; `ctl` is scratch.
    #[allow(clippy::too_many_arguments)]
    fn process_chain(
        &mut self,
        f: &GroupFilter,
        table: &ModTable,
        ctl: &mut [f32; MAX_BLOCK],
        left: &mut [f32],
        right: &mut [f32],
        rate: f32,
        amplifier: Option<Amplifier<'_>>,
    ) {
        let n = left.len();
        if n == 0 { return; }
        self.check_type_revision(f);
        // Envelope levels at each control tick. External sources move once
        // a block, so knobs change within one only while an envelope moves.
        let mut levels = [[0.0; MAX_BLOCK / CONTROL]; MAX_ENVS];
        let mut moving = false;
        for (env, ticks) in self.envs.iter_mut().zip(&mut levels).take(f.envs.len()) {
            if matches!(env.phase(), Phase::Sustain | Phase::Done) {
                // Held: one level all block, and no pass over its frames.
                env.skip(n, None, rate);
                ticks.fill(env.level());
                continue;
            }
            moving = true;
            env.render(&mut ctl[..n], None, rate);
            for (t, level) in ticks.iter_mut().enumerate().take(n.div_ceil(CONTROL)) {
                *level = ctl[t * CONTROL];
            }
        }
        // Held knobs filter the block in one pass: the same frames pair up
        // (CONTROL is even), so the output is the same as tick by tick.
        let step = if moving { CONTROL } else { n };
        for (t, start) in (0..n).step_by(step).enumerate() {
            let end = (start + step).min(n);
            // Units' knobs, then each drive stage's modulation (normalized).
            let mut rows: [[f32; KNOBS]; ROWS] = std::array::from_fn(|u| {
                f.units.get(u).map_or([0.0; KNOBS], |unit| unit.knobs)
            });
            for (e, (_, routes, _, bypass)) in f.envs.iter().enumerate() {
                // Bypass disconnects every target, including nonzero shaper
                // intercepts. The envelope above still advances while bypassed.
                if *bypass { continue; }
                for (r, m) in routes.iter() {
                    rows[r.unit as usize][r.knob as usize] += r.sign * m.intensity * m.shape(levels[e][t]);
                }
            }
            for ((r, i), value) in f.ext.iter().zip(&self.ext) {
                rows[r.unit as usize][r.knob as usize] += r.sign * table.mods[*i as usize].intensity * value;
            }
            let (l, r) = (&mut left[start..end], &mut right[start..end]);
            let mut amplified = amplifier.is_none();
            let split = f.amp_split.unwrap_or(0);
            for insert in &f.inserts {
                if !amplified && insert.slot >= split {
                    amplifier.unwrap().apply(start, l, r);
                    amplified = true;
                }
                match insert.operation {
                    Operation::Unit { index, section } => {
                        let unit = &f.units[index as usize];
                        let knobs = &rows[index as usize];
                        for band in 0..unit.sections as usize {
                            let index = section as usize + band;
                            let (section, tuned) = (&mut self.sections[index], &mut self.tuned[index]);
                            let (key, flat) = unit.key(knobs, band);
                            if unit.bypass || flat { section.s = [0.0; 4]; continue; }
                            if key != *tuned {
                                *tuned = key;
                                section.coefficients(Proto::of(unit.shape, key, band, rate));
                            }
                            section.process(l, r);
                        }
                    }
                    Operation::Stage(index) => {
                        let index = index as usize;
                        self.drive(f, index, &rows[f.units.len() + index], l, r, rate);
                    }
                    Operation::Mixer(_) => {}
                }
                let slot = insert.slot as usize;
                let (from, to) = (self.slot_matrices[slot], f.slot_matrices[slot]);
                if from != IDENTITY || to != IDENTITY {
                    let at = |frame: usize| std::array::from_fn(|i| from[i] + (to[i] - from[i]) * (frame as f32 / n as f32));
                    apply(at(start), at(end), l, r);
                }
            }
            if !amplified { amplifier.unwrap().apply(start, l, r); }
        }
        self.slot_matrices = f.slot_matrices;
    }
}

/// States per channel a [`LaneFilter`] tracks: two per active section.
const LANE_STATES: usize = 8;
const LANE_SECTIONS: usize = LANE_STATES / 2;
type States = [f32; LANE_STATES];
type Square = [[f64; LANE_STATES]; LANE_STATES];
/// Filters a [`LaneFilter`] keeps worked out, most recent first to go.
const TUNINGS: usize = 32;

/// A voice's group filter for one block, when it is held (no module
/// envelopes, the matrix reached): its active sections' coefficients in
/// order and its matrix. Voices of any group whose filters are equal this
/// block can share a [`LaneFilter`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct FilterKey {
    c: [[f32; 6]; LANE_SECTIONS],
    matrix: Matrix,
    active: u8,
}

impl FilterKey {
    /// Never 0 (a lane without a filter).
    pub fn hash(&self) -> u64 {
        let bits = self.c.iter().flatten().chain(&self.matrix).map(|x| x.to_bits());
        let h = bits.fold(u64::from(self.active), |h, b| (h ^ u64::from(b)).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        h | 1 << 63
    }
}

/// A filter worked out for blocks of `n` frames: the block's state
/// transition `m = A^n`, and `k[i] = A^(n-1-i) B`, how an input at frame
/// `i` reaches the end state.
struct Tuning {
    key: FilterKey,
    n: usize,
    m: [States; LANE_STATES],
    k: Box<[States; MAX_BLOCK]>,
}

/// The group filter of voices with equal [`FilterKey`]s. Held, it is
/// linear and time-invariant over the block, so it filters the voices' sum
/// once, from the sum of their states. Each voice's own state moves on as
/// filtering it alone would have moved it: by `M = A^n`, plus its input
/// dotted with how each frame reaches the end state. For a lane of voices
/// the input is its source frames, through the lane's interpolation and
/// envelope curve (the kernel `g`). So a voice leaves its lane, or is
/// muted, from its own state; only rounding differs from filtering voice
/// by voice.
pub(crate) struct LaneFilter {
    /// Active sections in order, holding the summed state.
    sections: [Section; LANE_SECTIONS],
    active: usize,
    matrix: Matrix,
    tunings: Box<[Tuning]>,
    /// The tuning in use, and the next to replace.
    tuning: usize,
    victim: usize,
    /// The lane's kernel: each source frame's share of the end state.
    g: Box<[States]>,
    /// A voice's input dotted with how it reaches the end state, per channel.
    d: [States; 2],
}

impl LaneFilter {
    pub fn new(window: usize) -> Self {
        let tuning = || Tuning {
            key: FilterKey::default(),
            n: 0,
            m: [[0.0; LANE_STATES]; LANE_STATES],
            k: Box::new([[0.0; LANE_STATES]; MAX_BLOCK]),
        };
        Self {
            sections: [Section::default(); LANE_SECTIONS],
            active: 0,
            matrix: IDENTITY,
            tunings: (0..TUNINGS).map(|_| tuning()).collect(),
            tuning: 0,
            victim: 0,
            g: vec![[0.0; LANE_STATES]; window].into_boxed_slice(),
            d: [[0.0; LANE_STATES]; 2],
        }
    }

    /// Tune to `key` for a block of `n` frames, with no state yet.
    pub fn prepare(&mut self, key: &FilterKey, n: usize) {
        self.active = key.active as usize;
        self.matrix = key.matrix;
        for (section, c) in self.sections.iter_mut().zip(&key.c).take(self.active) {
            (section.c, section.s) = (*c, [0.0; 4]);
        }
        match self.tunings.iter().position(|t| t.n == n && t.key == *key) {
            Some(i) => self.tuning = i,
            None => {
                self.tuning = self.victim;
                self.victim = (self.victim + 1) % TUNINGS;
                self.tune(key, n);
            }
        }
    }

    /// Work out the tuning for `key` and `n` into the current slot.
    fn tune(&mut self, key: &FilterKey, n: usize) {
        // The cascade as one state-space system per channel, in f64. The
        // input to section j is `p · s + q x`.
        let mut a: Square = [[0.0; LANE_STATES]; LANE_STATES];
        let mut b = [0f64; LANE_STATES];
        let (mut p, mut q) = ([0f64; LANE_STATES], 1f64);
        for (j, c) in key.c.iter().enumerate().take(key.active as usize) {
            let [a1, a2, a3, m0, m1, m2] = c.map(f64::from);
            let (b1, b2, b3) = (2.0 * a1 - 1.0, 2.0 * a2, 2.0 * a3);
            let (own, input) = ([[b1, -b2], [b2, 1.0 - b3]], [b2, b3]);
            for r in 0..2 {
                let row = &mut a[2 * j + r];
                for (x, p) in row.iter_mut().zip(&p).take(2 * j) {
                    *x = input[r] * p;
                }
                row[2 * j..2 * j + 2].copy_from_slice(&own[r]);
                b[2 * j + r] = input[r] * q;
            }
            let d = m0 + m1 * a2 + m2 * a3;
            p.iter_mut().for_each(|p| *p *= d);
            p[2 * j] += m1 * a1 + m2 * a2;
            p[2 * j + 1] += m2 * (1.0 - a3) - m1 * a2;
            q *= d;
        }
        let times = |x: &Square, y: &Square| -> Square {
            std::array::from_fn(|r| std::array::from_fn(|c| (0..LANE_STATES).map(|i| x[r][i] * y[i][c]).sum()))
        };
        let t = &mut self.tunings[self.tuning];
        let mut v = b;
        for k in (0..n).rev() {
            t.k[k] = v.map(|x| x as f32);
            v = std::array::from_fn(|r| (0..LANE_STATES).map(|c| a[r][c] * v[c]).sum());
        }
        let mut m: Square = std::array::from_fn(|r| std::array::from_fn(|c| f64::from(u8::from(r == c))));
        let (mut power, mut e) = (a, n);
        while e > 0 {
            if e & 1 == 1 {
                m = times(&m, &power);
            }
            power = times(&power, &power);
            e >>= 1;
        }
        (t.key, t.n, t.m) = (*key, n, m.map(|row| row.map(|x| x as f32)));
    }

    /// The kernel for a lane reading `count` source frames from `base` by
    /// `step` (32.32) with envelope curve `amp`: each source frame's share
    /// of the end state, through the cubic's taps as `mix` weighs them.
    pub fn kernel(&mut self, amp: &[f32], base: u64, step: u64, count: usize) {
        #[cfg(target_arch = "x86_64")]
        if std::arch::is_x86_feature_detected!("avx2") {
            // SAFETY: the running CPU supports AVX2.
            return unsafe { self.kernel_avx2(amp, base, step, count) };
        }
        self.kernel_body(amp, base, step, count);
    }

    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2")]
    fn kernel_avx2(&mut self, amp: &[f32], base: u64, step: u64, count: usize) {
        self.kernel_body(amp, base, step, count);
    }

    #[inline(always)]
    fn kernel_body(&mut self, amp: &[f32], base: u64, step: u64, count: usize) {
        let g = &mut self.g[..count];
        g.fill([0.0; LANE_STATES]);
        let k = &self.tunings[self.tuning].k;
        let add = |g: &mut States, c: f32, k: &States| {
            for q in 0..LANE_STATES {
                g[q] += c * k[q];
            }
        };
        const FRACTION: u64 = (1 << 32) - 1;
        let whole = step & FRACTION == 0 && base & FRACTION == 0;
        for (i, (&a, k)) in amp.iter().zip(k.iter()).enumerate() {
            let p = base + step * i as u64;
            let j = (p >> 32) as usize;
            // Frames read `j - 1..=j + 2`, all inside the window.
            let Some(taps) = g.get_mut(j - 1..j + 3) else {
                break;
            };
            if whole {
                add(&mut taps[1], a, k);
                continue;
            }
            let t = ((p as u32) >> 8) as f32 * (1.0 / (1 << 24) as f32);
            let (t2, t3) = (t * t, t * t * t);
            let w = [
                -0.5 * t + t2 - 0.5 * t3,
                1.0 - 2.5 * t2 + 1.5 * t3,
                0.5 * t + 2.0 * t2 - 1.5 * t3,
                -0.5 * t2 + 0.5 * t3,
            ];
            for (g, w) in taps.iter_mut().zip(w) {
                add(g, a * w, k);
            }
        }
    }

    /// Start on a voice: its state joins the sum.
    pub fn begin(&mut self, voice: &VoiceFilter) {
        for (section, slot) in self.sections.iter_mut().zip(&voice.slots).take(self.active) {
            let s = voice.sections[*slot as usize].s;
            section.s = std::array::from_fn(|i| section.s[i] + s[i]);
        }
        self.d = [[0.0; LANE_STATES]; 2];
    }

    /// Dot the voice's source frames `src`, from frame `at` of the lane's
    /// window, with the kernel.
    pub fn dots(&mut self, src: &[Frame], at: usize) {
        #[cfg(target_arch = "x86_64")]
        if std::arch::is_x86_feature_detected!("avx2") {
            // SAFETY: the running CPU supports AVX2.
            return unsafe { self.dots_avx2(src, at) };
        }
        self.dots_body(src, at);
    }

    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2")]
    fn dots_avx2(&mut self, src: &[Frame], at: usize) {
        self.dots_body(src, at);
    }

    #[inline(always)]
    fn dots_body(&mut self, src: &[Frame], at: usize) {
        let [mut dl, mut dr] = self.d;
        for (x, g) in src.iter().zip(&self.g[at..]) {
            for q in 0..LANE_STATES {
                dl[q] += g[q] * x[0];
                dr[q] += g[q] * x[1];
            }
        }
        self.d = [dl, dr];
    }

    /// Dot a voice's own filter input, `left`/`right`, with how each frame
    /// reaches the end state, and add it to the run's input `sum`.
    pub fn dots_out(&mut self, left: &[f32], right: &[f32], sum: [&mut [f32]; 2]) {
        #[cfg(target_arch = "x86_64")]
        if std::arch::is_x86_feature_detected!("avx2") {
            // SAFETY: the running CPU supports AVX2.
            return unsafe { self.dots_out_avx2(left, right, sum) };
        }
        self.dots_out_body(left, right, sum);
    }

    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2")]
    fn dots_out_avx2(&mut self, left: &[f32], right: &[f32], sum: [&mut [f32]; 2]) {
        self.dots_out_body(left, right, sum);
    }

    #[inline(always)]
    fn dots_out_body(&mut self, left: &[f32], right: &[f32], [sl, sr]: [&mut [f32]; 2]) {
        sl.iter_mut().zip(left).for_each(|(o, x)| *o += x);
        sr.iter_mut().zip(right).for_each(|(o, x)| *o += x);
        // Four sums a channel, of every fourth frame, so the adds overlap
        // rather than wait on each other; the end state moves by the
        // rounding of the regrouped sum (well under -120 dBFS).
        let mut d = [[[0f32; LANE_STATES]; 4]; 2];
        (d[0][0], d[1][0]) = (self.d[0], self.d[1]);
        let k = &self.tunings[self.tuning].k;
        let n = left.len().min(right.len()).min(k.len());
        let at = |d: &mut [[f32; LANE_STATES]; 4], j: usize, x: f32, k: &States| {
            for q in 0..LANE_STATES {
                d[j][q] += k[q] * x;
            }
        };
        let whole = n / 4 * 4;
        for ((k, l), r) in k[..whole].chunks_exact(4).zip(left.chunks_exact(4)).zip(right.chunks_exact(4)) {
            for j in 0..4 {
                at(&mut d[0], j, l[j], &k[j]);
                at(&mut d[1], j, r[j], &k[j]);
            }
        }
        for i in whole..n {
            at(&mut d[0], i % 4, left[i], &k[i]);
            at(&mut d[1], i % 4, right[i], &k[i]);
        }
        for (d, [a, b, c, e]) in self.d.iter_mut().zip(&d) {
            for q in 0..LANE_STATES {
                d[q] = (a[q] + b[q]) + (c[q] + e[q]);
            }
        }
    }

    /// Move the voice's state on by the block, its dotted input weighted
    /// `weights`.
    pub fn end(&mut self, voice: &mut VoiceFilter, weights: [f32; 2]) {
        let dim = 2 * self.active;
        let m = &self.tunings[self.tuning].m;
        let slots = voice.slots;
        let at = |q: usize, c: usize| (slots[q / 2] as usize, 2 * c + q % 2);
        let mut next = [[0f32; LANE_STATES]; 2];
        for (c, w) in weights.iter().enumerate() {
            let s: States = std::array::from_fn(|q| match q < dim {
                true => {
                    let (slot, i) = at(q, c);
                    voice.sections[slot].s[i]
                }
                false => 0.0,
            });
            for q in 0..dim {
                let x = (0..dim).map(|i| m[q][i] * s[i]).sum::<f32>() + w * self.d[c][q];
                next[c][q] = if x.abs() < 1e-20 { 0.0 } else { x };
            }
        }
        // Sections the lane skips are identities, at rest.
        for s in &mut voice.sections {
            s.s = [0.0; 4];
        }
        for (c, next) in next.iter().enumerate() {
            for (q, x) in next.iter().enumerate().take(dim) {
                let (slot, i) = at(q, c);
                voice.sections[slot].s[i] = *x;
            }
        }
    }

    /// Filter the voices' summed output, from the summed state, and apply
    /// the matrix.
    pub fn finish(&mut self, left: &mut [f32], right: &mut [f32]) {
        for section in &mut self.sections[..self.active] {
            section.process(left, right);
        }
        if self.matrix != IDENTITY {
            apply(self.matrix, self.matrix, left, right);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    const RATE: f32 = 48_000.0;

    #[test]
    fn native_insert_order_and_amplifier_split_match_rack_dsp_without_heap() {
        use crate::{fx::{Effect, params::{Field, Filter}}, engine::{GroupSettings, params::{Address, self}}, ksp::EnginePar};
        eprintln!("group voice state: Drive={} VoiceEffect={} VoiceFilter={} bytes",
            std::mem::size_of::<blocks::Drive>(), std::mem::size_of::<VoiceEffect>(), std::mem::size_of::<VoiceFilter>());
        let effect = |slot, kind, values: &[f32], output_gain| Effect {
            slot, kind, version: 0, bypass: false, output_gain, dry_level: 0.0,
            params: Params::Fields(crate::fx::params::layout_names(kind).unwrap().iter().zip(values)
                .map(|(&name, &value)| Field { name, value: Value::Number(value) }).collect()),
        };
        let chain = Chain { slots: vec![
            Effect { slot: 0, kind: Kind::Filter, version: 0, bypass: false, output_gain: 1.5,
                dry_level: 0.0, params: Params::Filter(Filter { filter_type: 2, cutoff: 1.0, resonance: 0.0, extra: [0.0; 3] }) },
            effect(2, Kind::Compressor, &[0.0, -12.0, 0.5, 1.0, 50.0, 1.0], 0.8),
            effect(7, Kind::SurroundPanner, &[1.0, 0.0], 0.7),
        ] };
        let table = ModTable::default();
        let cc = [0; 128];
        let input = Inputs { cc: &cc, cc74: None, bend: 0.0, pressure: 0, note: 60, velocity: 100, counter: 0.0 };
        let mut energies = [0.0f64; 3];
        for (case, split) in [0, 6, 8].into_iter().enumerate() {
            let groups = [Group { fx: chain.clone(), amp_split_slot: Some(split), ..Group::default() }];
            let mut settings = [GroupSettings::from(&groups[0])];
            let threshold_id = (crate::ksp::ENGINE_PAR_BASE..crate::ksp::ENGINE_PAR_BASE + 512)
                .find(|&id| crate::ksp::engine_par_name(id) == Some("$ENGINE_PAR_THRESHOLD")).unwrap();
            let threshold = Address::resolve(EnginePar { id: threshold_id,
                group: 0, slot: 2, generic: -1 }, &groups).unwrap();
            assert_eq!(crate::plugin::tests::allocations(|| {
                assert!(params::write(&mut settings, threshold, threshold.decode(500_000)));
                assert_eq!(threshold.encode(params::read(&settings, threshold).unwrap()), 500_000);
            }), 0);
            let f = settings[0].filter.as_ref().unwrap();
            assert!(unsupported_at(&groups[0].fx, Some(split)).is_empty());
            let mut voice = VoiceFilter::new(Some(f), &table, &input, RATE);
            assert_eq!(voice.hold(f, &table, RATE), None, "compressors remain per voice");
            let mut reference: Vec<_> = chain.slots.iter().map(|fx| blocks::Block::new(fx, RATE).unwrap()).collect();
            assert!(reference[1].set(Kind::Compressor, 1, 0.5));
            assert_eq!(crate::plugin::tests::allocations(|| {
                for block in 0..32 {
                    let source: [f32; 128] = std::array::from_fn(|i| 0.8 * (TAU * 1000.0 * (block * 128 + i) as f32 / RATE).sin());
                    let (mut l, mut r, mut expected_l, mut expected_r) = (source, source, source, source);
                    let amp: [f32; 128] = std::array::from_fn(|i| 0.6 + 0.4 * i as f32 / 128.0);
                    let amplifier = Amplifier { amp: &amp, gains: [0.1, 0.2], delta: [-0.02 / 128.0, 0.01 / 128.0] };
                    let mut applied = false;
                    for (fx, dsp) in chain.slots.iter().zip(&mut reference) {
                        if !applied && fx.slot >= split as usize {
                            amplifier.apply(0, &mut expected_l, &mut expected_r);
                            applied = true;
                        }
                        dsp.process(&mut expected_l, &mut expected_r);
                        expected_l.iter_mut().chain(&mut expected_r).for_each(|x| *x *= fx.output_gain);
                    }
                    if !applied { amplifier.apply(0, &mut expected_l, &mut expected_r); }
                    voice.process_amplified(f, &table, &mut [0.0; MAX_BLOCK], &mut l, &mut r, RATE,
                        &amp, amplifier.gains, amplifier.delta);
                    assert!(l.iter().chain(&r).all(|x| x.is_finite()));
                    assert!(l.iter().zip(&expected_l).chain(r.iter().zip(&expected_r)).all(|(a,b)| (a-b).abs() < 2e-6));
                    energies[case] += l.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>();
                }
            }), 0);
        }
        assert!(energies[0] > energies[2] * 2.0, "pre-Amplifier detection must not use attenuated input: {energies:?}");
        let unknown = Group { fx: Chain { slots: vec![chain.slots[1].clone()] }, ..Group::default() };
        assert!(GroupFilter::new(&unknown).is_none(), "missing native placement does not invent a compressor route");
        assert!(unsupported(&unknown.fx).iter().any(|w| w.contains("Compressor is not applied")));
    }

    #[test]
    fn group_dynamics_families_match_rack_and_native_edits_without_heap() {
        check_group_dynamics(&[
            (Kind::FeedbackCompressor, "$ENGINE_PAR_FCOMP_INPUT", 900_000),
            (Kind::Limiter, "$ENGINE_PAR_LIM_IN_GAIN", 900_000),
            (Kind::SolidBusComp, "$ENGINE_PAR_SCOMP_THRESHOLD", 100_000),
        ]);
    }

    #[test]
    fn eight_group_eq_inserts_match_rack_and_native_edits_without_heap() {
        use crate::{fx::{Effect, params::Field}, engine::{GroupSettings, params::{self, Address}}, ksp::EnginePar};
        eprintln!("eight-slot filter state: Section={} VoiceFilter={} bytes",
            std::mem::size_of::<Section>(), std::mem::size_of::<VoiceFilter>());
        let mut values = blocks::defaults(Kind::SolidGeq).unwrap().to_vec();
        for band in [0, 3, 6, 9] { values[band] = 0.52; }
        let chain = Chain { slots: (0..8).map(|slot| Effect {
            slot, kind: Kind::SolidGeq, version: 0, bypass: false, output_gain: 1.0, dry_level: 0.0,
            params: Params::Fields(crate::fx::params::layout_names(Kind::SolidGeq).unwrap().iter()
                .zip(&values).map(|(&name, &value)| Field { name, value: Value::Number(value) }).collect()),
        }).collect() };
        let table = ModTable::default();
        let cc = [0; 128];
        let input = Inputs { cc: &cc, cc74: None, bend: 0.0, pressure: 0, note: 60, velocity: 100, counter: 0.0 };
        let name = "$ENGINE_PAR_SEQ_HF_GAIN";
        let id = (crate::ksp::ENGINE_PAR_BASE..crate::ksp::ENGINE_PAR_BASE + 512)
            .find(|&id| crate::ksp::engine_par_name(id) == Some(name)).unwrap();
        let (_, field) = blocks::engine_par(name).unwrap();
        for split in [0, 4, 8] {
            let groups = [Group { fx: chain.clone(), amp_split_slot: Some(split), ..Group::default() }];
            let address = Address::resolve(EnginePar { id, group: 0, slot: 7, generic: -1 }, &groups).unwrap();
            let mut outputs = [[0.0f32; 4096]; 2];
            for (phase, output) in outputs.iter_mut().enumerate() {
                let mut settings = [GroupSettings::from(&groups[0])];
                let mut reference: Vec<_> = chain.slots.iter().map(|fx| blocks::Block::new(fx, RATE).unwrap()).collect();
                if phase == 1 {
                    assert_eq!(crate::plugin::tests::allocations(|| {
                        assert!(params::write(&mut settings, address, address.decode(900_000)));
                        assert_eq!(address.encode(params::read(&settings, address).unwrap()), 900_000);
                        assert!(reference[7].set(Kind::SolidGeq, field, 0.9));
                    }), 0);
                }
                let f = settings[0].filter.as_ref().unwrap();
                assert_eq!(f.units.len(), 8);
                assert_eq!(f.units.iter().map(|u| usize::from(u.sections)).sum::<usize>(), 32);
                assert!(unsupported_at(&groups[0].fx, Some(split)).is_empty());
                let mut voice = VoiceFilter::new(Some(f), &table, &input, RATE);
                assert!(voice.hold(f, &table, RATE).is_none(), "32 active sections use the per-voice path");
                assert_eq!(crate::plugin::tests::allocations(|| {
                    for block in 0..32 {
                        let mut l: [f32; 128] = std::array::from_fn(|i| {
                            let t = (block * 128 + i) as f32 / RATE;
                            [110.0, 1000.0, 8000.0, 16000.0].iter()
                                .map(|hz| 0.025 * (TAU * hz * t).sin()).sum()
                        });
                        let (mut r, mut expected_l, mut expected_r) = (l, l, l);
                        let amp = [0.7; 128];
                        let amplifier = Amplifier { amp: &amp, gains: [0.4, 0.6], delta: [0.0; 2] };
                        for (slot, dsp) in reference.iter_mut().enumerate() {
                            if slot == split as usize { amplifier.apply(0, &mut expected_l, &mut expected_r); }
                            dsp.process(&mut expected_l, &mut expected_r);
                        }
                        if split == 8 { amplifier.apply(0, &mut expected_l, &mut expected_r); }
                        voice.process_amplified(f, &table, &mut [0.0; MAX_BLOCK], &mut l, &mut r, RATE,
                            &amp, amplifier.gains, amplifier.delta);
                        assert!(l.iter().chain(&r).all(|x| x.is_finite()));
                        assert!(l.iter().zip(&expected_l).chain(r.iter().zip(&expected_r))
                            .all(|(a,b)| (a-b).abs() < 2e-6), "split{split}, phase{phase}");
                        output[block * 128..(block + 1) * 128].copy_from_slice(&l);
                    }
                }), 0);
            }
            let difference: f64 = outputs[0].iter().zip(&outputs[1]).map(|(a,b)| f64::from(a-b).powi(2)).sum();
            assert!(difference > 1e-6, "split{split}: the eighth insert's native edit must affect PCM");
        }
    }

    #[test]
    fn group_transient_master_matches_rack_and_native_edits_without_heap() {
        eprintln!("group transient state: Transient={} Drive={} VoiceEffect={} VoiceFilter={} bytes",
            std::mem::size_of::<blocks::Transient>(), std::mem::size_of::<blocks::Drive>(),
            std::mem::size_of::<VoiceEffect>(), std::mem::size_of::<VoiceFilter>());
        check_group_dynamics(&[
            (Kind::TransientMaster, "$ENGINE_PAR_TR_ATTACK", 900_000),
            (Kind::TransientMaster, "$ENGINE_PAR_TR_SUSTAIN", 900_000),
        ]);
    }

    fn check_group_dynamics(cases: &[(Kind, &str, i32)]) {
        use crate::{fx::{Effect, params::Field}, engine::{GroupSettings, params::{self, Address}}, ksp::EnginePar};
        let table = ModTable::default();
        let cc = [0; 128];
        let input = Inputs { cc: &cc, cc74: None, bend: 0.0, pressure: 0, note: 60, velocity: 100, counter: 0.0 };
        for &(kind, name, native) in cases {
            let fx = Effect { slot: 3, kind, version: 0, bypass: false, output_gain: 0.8, dry_level: 0.0,
                params: Params::Fields(crate::fx::params::layout_names(kind).unwrap().iter()
                    .zip(blocks::defaults(kind).unwrap())
                    .map(|(&name, &value)| Field { name, value: Value::Number(value) }).collect()),
            };
            let groups = [Group { fx: Chain { slots: vec![fx.clone()] }, amp_split_slot: Some(8), ..Group::default() }];
            let id = (crate::ksp::ENGINE_PAR_BASE..crate::ksp::ENGINE_PAR_BASE + 512)
                .find(|&id| crate::ksp::engine_par_name(id) == Some(name)).unwrap();
            let address = Address::resolve(EnginePar { id, group: 0, slot: 3, generic: -1 }, &groups).unwrap();
            let (mapped, field) = blocks::engine_par(name).unwrap();
            assert_eq!(mapped, kind);
            let unknown = Group { amp_split_slot: None, ..groups[0].clone() };
            assert!(GroupFilter::new(&unknown).is_none());
            assert!(unsupported(&unknown.fx).iter().any(|w| w.contains(&kind.name())));
            for split in [0, 8] {
                let mut outputs = [[0.0f32; 8192]; 2];
                for (phase, output) in outputs.iter_mut().enumerate() {
                    let group = Group { amp_split_slot: Some(split), ..groups[0].clone() };
                    let mut settings = [GroupSettings::from(&group)];
                    let mut reference = blocks::Block::new(&fx, RATE).unwrap();
                    if phase == 1 {
                        assert_eq!(crate::plugin::tests::allocations(|| {
                            assert!(params::write(&mut settings, address, address.decode(native)));
                            assert_eq!(address.encode(params::read(&settings, address).unwrap()), native);
                            assert!(reference.set(kind, field, native as f32 / 1_000_000.0));
                        }), 0);
                    }
                    let f = settings[0].filter.as_ref().unwrap();
                    assert!(unsupported_at(&group.fx, Some(split)).is_empty());
                    let mut voice = VoiceFilter::new(Some(f), &table, &input, RATE);
                    assert!(voice.hold(f, &table, RATE).is_none());
                    assert_eq!(crate::plugin::tests::allocations(|| {
                        for block in 0..64 {
                            let level = if kind == Kind::TransientMaster {
                                match block % 16 { 0..=3 => 0.3, 4..=7 => 0.075, _ => 0.0 }
                            // After split0's amplifier, the linked peak must
                            // cross the edited Solid Bus threshold (-12 dB).
                            } else if kind == Kind::SolidBusComp { 1.0 } else { 0.3 };
                            let mut l: [f32; 128] = std::array::from_fn(|i| level * (TAU * 1000.0 * (block * 128 + i) as f32 / RATE).sin());
                            let (mut r, mut expected_l, mut expected_r) = (l, l, l);
                            let amp = [0.7; 128];
                            let amplifier = Amplifier { amp: &amp, gains: [0.4, 0.6], delta: [0.0; 2] };
                            if split == 0 { amplifier.apply(0, &mut expected_l, &mut expected_r); }
                            reference.process(&mut expected_l, &mut expected_r);
                            expected_l.iter_mut().chain(&mut expected_r).for_each(|x| *x *= fx.output_gain);
                            if split == 8 { amplifier.apply(0, &mut expected_l, &mut expected_r); }
                            voice.process_amplified(f, &table, &mut [0.0; MAX_BLOCK], &mut l, &mut r, RATE,
                                &amp, amplifier.gains, amplifier.delta);
                            assert!(l.iter().chain(&r).all(|x| x.is_finite()));
                            assert!(l.iter().zip(&expected_l).chain(r.iter().zip(&expected_r))
                                .all(|(a,b)| (a-b).abs() < 2e-6), "{kind:?}, split{split}, phase{phase}");
                            output[block * 128..(block + 1) * 128].copy_from_slice(&l);
                        }
                    }), 0);
                }
                let difference: f64 = outputs[0].iter().zip(&outputs[1]).map(|(a,b)| f64::from(a-b).powi(2)).sum();
                assert!(difference > 1e-6, "{kind:?}, split{split}: native edit must affect PCM");
            }
        }
    }

    #[test]
    fn interleaved_slot_gains_do_not_share_canonical_filter_states() {
        let mut group = Group { amp_split_slot: Some(0), ..Group::default() };
        for slot in [0, 1] {
            group.fx.slots.push(crate::fx::Effect {
                slot, kind: Kind::Filter, version: 0, bypass: false, output_gain: 1.0, dry_level: 0.0,
                params: Params::Filter(crate::fx::params::Filter { filter_type: 2, cutoff: 0.5, resonance: 0.0, extra: [0.0; 3] }),
            });
        }
        let mut f = GroupFilter::new(&group).unwrap();
        let table = ModTable::default();
        let cc = [0; 128];
        let input = Inputs { cc: &cc, cc74: None, bend: 0.0, pressure: 0, note: 60, velocity: 100, counter: 0.0 };
        let mut voice = VoiceFilter::new(Some(&f), &table, &input, RATE);
        assert!(voice.hold(&f, &table, RATE).is_some());
        assert_eq!(crate::plugin::tests::allocations(|| {
            assert!(f.set_knob(0, Knob::Output, 2.0));
            assert!(f.set_knob(1, Knob::Output, 0.5));
            assert_eq!(f.matrix, IDENTITY, "the collapsed final gain hides the interleaving");
            for block in 0..8 {
                let mut l: [f32; 128] = std::array::from_fn(|i| (TAU * 1000.0 * (block * 128 + i) as f32 / RATE).sin());
                let mut r = l;
                voice.process(&f, &table, &mut [0.0; MAX_BLOCK], &mut l, &mut r, RATE);
            }
            // Unit 1 has processed twice the canonical lane input. Summing
            // these states into a lane followed by one final gain is invalid.
            assert!(voice.hold(&f, &table, RATE).is_none());
            assert!(f.set_knob(0, Knob::Output, 1.0));
            assert!(f.set_knob(1, Knob::Output, 1.0));
            assert!(voice.hold(&f, &table, RATE).is_none(), "pending inline ramps cannot collapse to a final matrix");
            let (mut l, mut r) = ([0.0; 128], [0.0; 128]);
            voice.process(&f, &table, &mut [0.0; MAX_BLOCK], &mut l, &mut r, RATE);
            assert!(voice.hold(&f, &table, RATE).is_some(), "canonical routing returns after the inline ramp");
        }), 0);
    }

    #[test]
    fn live_filter_type_change_retunes_identical_knobs_without_heap() {
        let group = Group { amp_split_slot: Some(8), fx: Chain { slots: vec![crate::fx::Effect {
            slot: 0, kind: Kind::Filter, version: 0, bypass: false, output_gain: 1.0, dry_level: 0.0,
            params: Params::Filter(crate::fx::params::Filter { filter_type: 2, cutoff: 0.3, resonance: 0.0, extra: [0.0; 3] }),
        }] }, ..Group::default() };
        let mut f = GroupFilter::new(&group).unwrap();
        let table = ModTable::default();
        let cc = [0; 128];
        let input = Inputs { cc: &cc, cc74: None, bend: 0.0, pressure: 0, note: 60, velocity: 100, counter: 0.0 };
        let mut voice = VoiceFilter::new(Some(&f), &table, &input, RATE);
        let mut energy = [0.0f64; 2];
        assert_eq!(crate::plugin::tests::allocations(|| {
            for (case, kind) in [2, 3].into_iter().enumerate() {
                assert!(f.set_knob(0, Knob::Type, kind as f32));
                for block in 0..32 {
                    let mut l: [f32; 128] = std::array::from_fn(|i| (TAU * 5000.0 * (block * 128 + i) as f32 / RATE).sin());
                    let mut r = l;
                    voice.process(&f, &table, &mut [0.0; MAX_BLOCK], &mut l, &mut r, RATE);
                    if block >= 16 { energy[case] += l.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>(); }
                }
            }
        }), 0);
        assert!(energy[1] > energy[0] * 100.0, "same cutoff/resonance must not retain the low-pass coefficients: {energy:?}");
    }

    #[test]
    fn module_envelope_controls_preserve_targets_and_elapsed_clock_without_heap() {
        use crate::{engine::{GroupSettings, params::{self, Address, id}}, import::{ModSource, Modulator},
            modulation::{Ahdsr as ImportedAhdsr, ModEnvelope}, ksp::EnginePar};
        let target = |target, shaper| ModAssignment { name: "Envelope".into(), source: ModSource::Unassigned,
            target, intensity: 0.5, invert: false, lag_ms: 0, shaper };
        let env = ImportedAhdsr { attack_curve: 0., attack_ms: 100., hold_ms: 0., decay_ms: 0.,
            sustain: 1., release_ms: 100., unknown_flag: 0, unknown_tail: Vec::new() };
        let group = Group {
            envelopes: vec![ModEnvelope { env: env.clone(), targets: vec![] }, ModEnvelope { env, targets: vec![
                target(ModTarget::Group("loopLength".into()), None),
                target(ModTarget::Pitch, None),
                target(ModTarget::Module { param: "filterCutoff".into(), slot: 0 },
                    Some(crate::import::ShaperCurve::Table(vec![0.3, 1.]))),
                target(ModTarget::Module { param: "filterQ".into(), slot: 0 }, None),
            ] }],
            modulators: vec![Modulator { name: "Envelope".into(), targets: vec![String::new(); 4],
                assignments: None, volume_env: false, flex: false, envelope: Some(1), kind: "ahdsr".into() }],
            fx: Chain { slots: vec![crate::fx::Effect { slot: 0, kind: Kind::Filter, version: 0,
                bypass: false, output_gain: 1., dry_level: 1., params: Params::Filter(crate::fx::params::Filter {
                    filter_type: 2, cutoff: 0.3, resonance: 0., extra: [0.; 3] }) }] },
            ..Group::default()
        };
        let groups = [group];
        let mut settings = [GroupSettings::from(&groups[0])];
        let address = |id, generic| Address::resolve(EnginePar { id, group: 0, slot: 0, generic }, &groups);
        let depth = address(id::MOD_TARGET_MP_INTENSITY, 2).unwrap();
        let unipolar = address(id::MOD_TARGET_INTENSITY, 2).unwrap();
        let bypass = address(id::INTMOD_BYPASS, -1).unwrap();
        assert!(address(id::INTMOD_INTENSITY, 2).is_none(), "unknown legacy filter law stays explicit");
        assert!(address(id::MOD_TARGET_MP_INTENSITY, 0).is_none(), "unsupported target does not alias a routed one");
        let table = ModTable::default();
        let cc = [0; 128];
        let input = Inputs { cc: &cc, cc74: None, bend: 0., pressure: 0, note: 60, velocity: 100, counter: 0. };
        let f = settings[0].filter.as_ref().unwrap();
        let mut dry = f.clone();
        dry.envs = [].into();
        let mut voice = VoiceFilter::new(Some(f), &table, &input, RATE);
        let mut reference = VoiceFilter::new(Some(&dry), &table, &input, RATE);
        let mut clock = Envelope::new(&Ahdsr::from(&groups[0].envelopes[1].env), RATE);
        let mut ctl = [0.; MAX_BLOCK];
        let mut difference = 0f32;
        assert_eq!(crate::plugin::tests::allocations(|| {
            assert!(params::write(&mut settings, unipolar, unipolar.decode(500_000)));
            assert_eq!(params::read(&settings, unipolar), Some(0.25));
            assert!(params::write(&mut settings, depth, depth.decode(750_000)));
            assert_eq!(params::read(&settings, depth), Some(0.5));
            assert_eq!(depth.encode(params::read(&settings, depth).unwrap()), 750_000);
            assert!(params::write(&mut settings, bypass, bypass.decode(1)));
            assert!(settings[0].pitch_envelopes[0].bypass, "all copies of a mixed envelope share bypass");
            for block in 0..48 {
                let source = std::array::from_fn::<_, 128, _>(|n| (TAU * 2000. * (block * 128 + n) as f32 / RATE).sin());
                let (mut l, mut r) = (source, source);
                let (mut dl, mut dr) = (source, source);
                if block == 24 { assert!(params::write(&mut settings, bypass, 0.)); }
                voice.process(settings[0].filter.as_ref().unwrap(), &table, &mut ctl, &mut l, &mut r, RATE);
                reference.process(&dry, &table, &mut ctl, &mut dl, &mut dr, RATE);
                clock.skip(128, None, RATE);
                assert_eq!(voice.envs[0].level(), clock.level(), "bypass does not restart or freeze the clock");
                assert!(l.iter().chain(&r).all(|x| x.is_finite()));
                if block < 24 { assert_eq!(l, dl, "bypass removes even the shaper intercept"); }
                else { difference += l.iter().zip(&dl).map(|(a,b)| (a-b).abs()).sum::<f32>(); }
            }
            voice.release();
            clock.release(None);
            let (mut l, mut r) = ([0.; 128], [0.; 128]);
            voice.process(settings[0].filter.as_ref().unwrap(), &table, &mut ctl, &mut l, &mut r, RATE);
            clock.skip(128, None, RATE);
            assert_eq!(voice.envs[0].level(), clock.level());
        }), 0);
        assert!(difference > 1., "resuming the elapsed envelope changes actual PCM: {difference}");
    }

    /// The rearranged SSE section matches Simper's textbook update.
    #[test]
    fn section_matches_the_reference_svf() {
        let mut s = Section::default();
        s.set(Response::High, 3000.0, 2.0, RATE);
        let (mut a1, mut a2, mut b1, mut b2) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        let [c1, c2, c3, m0, m1, m2] = s.c;
        let (mut left, mut right): (Vec<f32>, Vec<f32>) = (0..300)
            .map(|i| (((i * 7919) % 200) as f32 / 100.0 - 1.0, (i as f32 * 0.3).sin()))
            .unzip();
        let reference = |x: f32, s1: &mut f32, s2: &mut f32| {
            let v3 = x - *s2;
            let v1 = c1 * *s1 + c2 * v3;
            let v2 = *s2 + c2 * *s1 + c3 * v3;
            (*s1, *s2) = (2.0 * v1 - *s1, 2.0 * v2 - *s2);
            m0 * x + m1 * v1 + m2 * v2
        };
        let expected: Vec<(f32, f32)> = left
            .iter()
            .zip(&right)
            .map(|(&l, &r)| (reference(l, &mut a1, &mut a2), reference(r, &mut b1, &mut b2)))
            .collect();
        // Odd chunks exercise the single-frame step.
        s.process(&mut left[..101], &mut right[..101]);
        s.process(&mut left[101..102], &mut right[101..102]);
        s.process(&mut left[102..], &mut right[102..]);
        for (i, (l, r)) in left.iter().zip(&right).enumerate() {
            assert!((l - expected[i].0).abs() < 1e-5 && (r - expected[i].1).abs() < 1e-5, "frame {i}");
        }
    }

    /// Steady-state gain (dB) of `section` for a sine at `hz`.
    fn gain_db(mut section: Section, hz: f32) -> f32 {
        let frames = (RATE as usize) / 2;
        let mut peak = 0.0f32;
        let mut block = [0.0; 256];
        for start in (0..frames).step_by(256) {
            let mut right = [0.0; 256];
            for (i, x) in block.iter_mut().enumerate() {
                *x = (2.0 * std::f32::consts::PI * hz * (start + i) as f32 / RATE).sin();
            }
            section.process(&mut block, &mut right);
            if start > frames / 2 {
                peak = block.iter().fold(peak, |p, x| p.max(x.abs()));
            }
        }
        20.0 * peak.log10()
    }

    #[test]
    fn daft_two_pole_proxy_preserves_passbands_and_resonates_at_cutoff() {
        let c = (1000.0 / CUTOFF_MIN_HZ).log2() / CUTOFF_OCTAVES;
        for (id, response) in [(106, Response::Low), (107, Response::High)] {
            let (shape, sections) = filter_type(id).unwrap();
            assert_eq!(sections, 1, "Daft is a two-pole filter");
            assert!(matches!(shape, Shape::Filter(r) if r == response));
            // Actual saved ANALOG STRINGS resonance; the shared SVF keeps
            // unity in the pass band instead of adding ladder feedback loss.
            let proto = Proto::of(shape, [c, 0.594595, 0.0], 0, RATE);
            let pass = if response == Response::Low { 10.0 } else { 20_000.0 };
            assert!((proto.gain(pass, RATE) - 1.0).abs() < 0.01);
            let mut s = Section::default();
            s.coefficients(proto);
            let expected = 20.0 * proto.gain(1000.0, RATE).log10();
            assert!(expected > 10.0 && expected < 20.0);
            assert!((gain_db(s, 1000.0) - expected).abs() < 0.1);
            let proto = Proto::of(shape, [c, 0.0, 0.0], 0, RATE);
            let (near, far) = if response == Response::Low { (4000.0, 8000.0) } else { (125.0, 62.5) };
            let slope = 20.0 * (proto.gain(near, RATE) / proto.gain(far, RATE)).log10();
            assert!((slope - 12.0).abs() < 2.0, "type {id}: {slope} dB/octave");
            let mut s = Section::default();
            for n in 0..200 {
                let key = [(n % 20) as f32 / 19.0, (n % 11) as f32 / 10.0, 0.0];
                s.coefficients(Proto::of(shape, key, 0, RATE));
                let (mut left, mut right) = ([0.01; 64], [-0.01; 64]);
                s.process(&mut left, &mut right);
                assert!(left.iter().chain(&right).all(|v| v.is_finite() && v.abs() < 10.0));
            }
        }
    }

    #[test]
    fn native_sv_notch_id_runs_four_pole_notch_and_matches_script_constant() {
        assert_eq!(ksp_filter_type("$FILTER_TYPE_SV_NOTCH4"), Some(58));
        let fx = crate::fx::Effect {
            slot: 0, kind: Kind::Filter, version: 146, bypass: false,
            output_gain: 1.0, dry_level: 1.0,
            params: Params::Filter(crate::fx::params::Filter {
                filter_type: 58,
                cutoff: (1000.0 / CUTOFF_MIN_HZ).log2() / CUTOFF_OCTAVES,
                resonance: 0.0, extra: [0.0; 3],
            }),
        };
        assert!(unsupported_at(&Chain { slots: vec![fx.clone()] }, Some(8)).is_empty());
        for hz in [10.0, 1000.0, 20_000.0] {
            let mut filter = RackFilter::new(&fx, RATE).expect("native notch must not pass through");
            assert_eq!(filter.active, 2, "four poles require two SVF sections");
            assert_eq!(filter.knob(Knob::Type), Some(58.0));
            let (mut input_power, mut output_power) = (0.0f64, 0.0f64);
            for start in (0..24_000).step_by(128) {
                let (mut left, mut right) = ([0.0; 128], [0.0; 128]);
                for (n, x) in left.iter_mut().enumerate() {
                    *x = (2.0 * std::f32::consts::PI * hz * (start + n) as f32 / RATE).sin();
                }
                if start > 12_000 {
                    input_power += left.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>();
                }
                filter.process(&mut left, &mut right);
                assert!(left.iter().all(|x| x.is_finite()));
                if start > 12_000 {
                    output_power += left.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>();
                }
            }
            let gain = (output_power / input_power).sqrt();
            if hz == 1000.0 {
                assert!(gain < 0.001, "cutoff must be rejected: {gain}");
            } else {
                assert!((gain - 1.0).abs() < 0.01, "pass band at {hz} Hz: {gain}");
            }
        }
    }

    #[test]
    fn lowpass_highpass_and_resonance_match_analytic_responses() {
        let mut s = Section::default();
        s.set(Response::Low, 1000.0, Q_MIN, RATE);
        assert!((gain_db(s, 1000.0) + 3.01).abs() < 0.1, "LP -3 dB at cutoff");
        assert!(gain_db(s, 100.0).abs() < 0.05);
        assert!((gain_db(s, 4000.0) + 24.1).abs() < 0.5, "12 dB/octave");
        s.set(Response::High, 1000.0, Q_MIN, RATE);
        assert!((gain_db(s, 1000.0) + 3.01).abs() < 0.1, "HP -3 dB at cutoff");
        assert!(gain_db(s, 10_000.0).abs() < 0.1);
        // Resonance peak: |H(fc)| = Q for the 2-pole low pass.
        s.set(Response::Low, 1000.0, 8.0, RATE);
        assert!((gain_db(s, 1000.0) - 20.0 * 8f32.log10()).abs() < 0.1);
        s.set(Response::Notch, 1000.0, Q_MIN, RATE);
        assert!(gain_db(s, 1000.0) < -40.0);
        // Low cutoffs, where the state increments are tiny, keep unity gain.
        s.set(Response::Low, 25.0, Q_MIN, RATE);
        assert!(gain_db(s, 4.0).abs() < 0.05, "LP 25 Hz passes 4 Hz");
    }

    /// The editor's response curve is the running section's: the analytic
    /// `Proto::gain` matches a sine through the SVF.
    #[test]
    fn analytic_response_matches_the_running_section() {
        for proto in [
            Proto::filter(Response::Low, 1000.0, 4.0, RATE),
            Proto::filter(Response::High, 300.0, Q_MIN, RATE),
            Proto::filter(Response::Band, 2000.0, 2.0, RATE),
            Proto::filter(Response::Notch, 800.0, 1.0, RATE),
            Proto::bell(2500.0, 0.7, -9.0, RATE),
        ] {
            let mut s = Section::default();
            s.coefficients(proto);
            for hz in [60.0, 400.0, 1000.0, 2500.0, 9000.0] {
                let analytic = 20.0 * proto.gain(hz, RATE).log10();
                // Deep in a notch the measurement reads its own noise floor.
                if analytic > -40.0 {
                    let measured = gain_db(s, hz);
                    assert!((analytic - measured).abs() < 0.1, "{proto:?} at {hz}: {analytic} vs {measured}");
                }
            }
        }
    }

    #[test]
    fn eq_bell_reaches_its_gain_at_the_center() {

        let mut s = Section::default();
        for db in [-12.0, 6.0, 18.0] {
            s.set_bell(2000.0, 1.0, db, RATE);
            assert!((gain_db(s, 2000.0) - db).abs() < 0.05, "{db} dB");
            assert!(gain_db(s, 30.0).abs() < 0.1);
        }
    }

    /// Per-voice cost of real group racks (Vista, Solo), 128-frame blocks:
    /// `cargo test --release --no-default-features --lib engine::filter::tests::bench -- --ignored --nocapture`.
    #[test]
    #[ignore = "benchmark; needs the library corpus"]
    fn bench() {
        use crate::import::{LIBRARY_ROOT, read};
        const BLOCK: usize = 128;
        const BLOCKS: usize = 20_000;
        let cases = [
            ("Vista Cellos legato (EQ3, HP, 2 envelopes)", "Performance Samples Vista/Instruments/Vista - 3 Cellos.nki", "cl legatodyn2"),
            ("Vista Cellos sustain (stereo modeller)", "Performance Samples Vista/Instruments/Vista - 3 Cellos.nki", "cl susdyn2"),
            ("Vista Harp (LP, envelope)", "Performance Samples Vista/Instruments/Bonus/Vista - Harp.nki", ""),
            ("Solo Pads (2 filters, stereo modeller)", "Solo/Instruments/03 Sound Design/Solo - 01 Pads.nki", ""),
        ];
        let cc = [64u8; 128];
        let input = Inputs { cc74: None, cc: &cc, bend: 0.0, pressure: 0, note: 60, velocity: 100, counter: 0.0 };
        for (name, path, group) in cases {
            let instrument = read(&std::path::Path::new(LIBRARY_ROOT).join(path)).unwrap();
            let g = instrument
                .groups
                .iter()
                .find(|g| (group.is_empty() || g.name == group) && GroupFilter::new(g).is_some_and(|f| !f.units.is_empty()))
                .or_else(|| instrument.groups.iter().find(|g| g.name == group));
            let Some(g) = g else { continue };
            let Some(filter) = GroupFilter::new(g) else {
                println!("{name}: no per-voice work");
                continue;
            };
            let table = ModTable::from(g);
            let mut voice = VoiceFilter::new(Some(&filter), &table, &input, RATE);
            let (mut l, mut r, mut ctl) = ([0.0; BLOCK], [0.0; BLOCK], [0.0; MAX_BLOCK]);
            let mut seed = 1u32;
            let mut noise = || {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 8) as f32 / (1 << 24) as f32 - 0.5
            };
            let (mut best, mut total) = (u128::MAX, 0);
            for _ in 0..BLOCKS {
                l.iter_mut().chain(r.iter_mut()).for_each(|x| *x = noise());
                let t = std::time::Instant::now();
                voice.follow(&filter, &table, &input, BLOCK, RATE);
                voice.process(&filter, &table, &mut ctl, &mut l, &mut r, RATE);
                let ns = t.elapsed().as_nanos();
                (best, total) = (best.min(ns), total + ns);
            }
            let mean = total / BLOCKS as u128;
            // Share of a 128-frame block at 48 kHz (2.67 ms) for 100 such voices.
            let share = mean as f64 * 100.0 / (BLOCK as f64 / RATE as f64 * 1e9) * 100.0;
            println!("{name}: mean {mean} ns/block, best {best} ns/block, 100 voices {share:.1}% of the block");
        }
    }

    #[test]
    fn mixers_gains_and_bypass_follow_the_rack() {
        let unit = |slot, shape, bypass, gain| Unit { slot, shape, sections: 1, knobs: [0.5; KNOBS], bypass, gain, kind: 2 };
        let mut f = GroupFilter {
            units: [
                Unit { knobs: [0.0; KNOBS], ..unit(0, Shape::Filter(Response::Low), true, 0.5) },
                unit(1, Shape::Eq, false, 2.0),
            ]
            .into(),
            // Stereo Modeller: mono, panned half right.
            mixers: [Mixer { slot: 2, stereo: Some([-1.0, 0.5]), bypass: false, gain: 1.0 }].into(),
            stages: [].into(),
            inserts: [].into(), amp_split: None, slot_matrices: [IDENTITY; 8], type_revision: 0, pre_active: false, matrix_interleaved: false,
            matrix: IDENTITY,
            envs: [].into(),
            ext: [].into(),
        };
        f.compile_inserts();
        let table = ModTable::default();
        let cc = [0u8; 128];
        let input = Inputs { cc74: None, cc: &cc, bend: 0.0, pressure: 0, note: 60, velocity: 100, counter: 0.0 };
        let mut voice = VoiceFilter::new(Some(&f), &table, &input, RATE);
        let mut ctl = [0.0; MAX_BLOCK];
        let mut run = |f: &GroupFilter| {
            let (mut l, mut r) = ([0.0; 128], [0.0; 128]);
            for _ in 0..100 {
                (l, r) = ([1.0; 128], [0.0; 128]);
                voice.process(f, &table, &mut ctl, &mut l, &mut r, RATE);
            }
            (l[127], r[127])
        };
        // Bypassed low pass and flat EQ play as identities; the EQ's gain doubles.
        let (l, r) = run(&f);
        assert!((l - 0.5).abs() < 1e-5 && (r - 1.0).abs() < 1e-5, "{l} {r}");
        // Scripts switch the filter in: its output gain applies and DC passes.
        assert!(f.set_knob(0, Knob::Bypass, 0.0));
        assert_eq!(f.knob(0, Knob::Bypass), Some(0.0));
        let (l, r) = run(&f);
        assert!((l - 0.25).abs() < 1e-3 && (r - 0.5).abs() < 1e-3, "{l} {r}");
        assert!(!f.set_knob(1, Knob::Cutoff, 0.3), "an EQ has no cutoff");
    }
    /// `$ENGINE_PAR_EFFECT_SUBTYPE` switches a slot among filter types,
    /// and every modelled type plays finite audio.
    #[test]
    fn filter_types_switch_and_play() {
        let unit = Unit { slot: 0, shape: Shape::Filter(Response::Low), sections: 1, knobs: [0.6, 0.7, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], bypass: false, gain: 1.0, kind: 2 };
        let mut f = GroupFilter { units: [unit].into(), mixers: [].into(), stages: [].into(), inserts: [].into(), amp_split: None, slot_matrices: [IDENTITY; 8], type_revision: 0, pre_active: false, matrix_interleaved: false, matrix: IDENTITY, envs: [].into(), ext: [].into() };
        f.compile_inserts();
        let table = ModTable::default();
        let cc = [0u8; 128];
        let input = Inputs { cc74: None, cc: &cc, bend: 0.0, pressure: 0, note: 60, velocity: 100, counter: 0.0 };
        let mut ctl = [0.0; MAX_BLOCK];
        for kind in [13, 70, 90, 100, 101, 102, 103, 104, 105, 106, 107, SV_NOTCH4] {
            assert!(f.set_knob(0, Knob::Type, kind as f32), "{kind}");
            assert_eq!(f.knob(0, Knob::Type), Some(kind as f32));
            let mut voice = VoiceFilter::new(Some(&f), &table, &input, RATE);
            let mut seed = 1u32;
            for _ in 0..50 {
                let mut noise = [0.0f32; 128];
                for x in &mut noise {
                    seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    *x = (seed >> 9) as f32 / (1u32 << 22) as f32 - 1.0;
                }
                let (mut l, mut r) = (noise, noise);
                voice.process(&f, &table, &mut ctl, &mut l, &mut r, RATE);
                assert!(l.iter().chain(&r).all(|x| x.is_finite() && x.abs() < 100.0), "{kind}");
            }
        }
        // A daft low pass passes the bass, its high pass does not.
        f.set_knob(0, Knob::Type, 106.0);
        let low = f.magnitude(60.0, RATE);
        f.set_knob(0, Knob::Type, 107.0);
        assert!(low > 10.0 * f.magnitude(60.0, RATE));
        assert!(!f.set_knob(0, Knob::Type, 4242.0), "unknown types are refused");
    }

    #[test]
    fn drive_stages_bend_per_voice_in_slot_order() {
        // A low pass at slot 0, Saturation (shape 1) at slot 2.
        let unit = Unit { slot: 0, shape: Shape::Filter(Response::Low), sections: 1, knobs: [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], bypass: false, gain: 1.0, kind: 2 };
        let mut fields = [0.0; blocks::FIELDS];
        fields[0] = 1.0;
        let stage = Stage { slot: 2, kind: Kind::SurroundPanner, fields, bypass: false, gain: 1.0 };
        let mut f = GroupFilter { units: [unit].into(), mixers: [].into(), stages: [stage].into(), inserts: [].into(), amp_split: None, slot_matrices: [IDENTITY; 8], type_revision: 0, pre_active: false, matrix_interleaved: false, matrix: IDENTITY, envs: [].into(), ext: [].into() };
        f.compile_inserts();
        let table = ModTable::default();
        let cc = [0u8; 128];
        let input = Inputs { cc74: None, cc: &cc, bend: 0.0, pressure: 0, note: 60, velocity: 100, counter: 0.0 };
        let mut ctl = [0.0; MAX_BLOCK];
        let mut voice = VoiceFilter::new(Some(&f), &table, &input, RATE);
        assert_eq!(voice.hold(&f, &table, RATE), None, "nonlinear voices are not shared");
        let mut peak = |voice: &mut VoiceFilter, f: &GroupFilter| {
            let mut out = 0f32;
            for i in 0..20 {
                let mut l: [f32; 128] = std::array::from_fn(|j| (TAU * 100.0 * (i * 128 + j) as f32 / RATE).sin());
                let mut r = l;
                voice.process(f, &table, &mut ctl, &mut l, &mut r, RATE);
                out = out.max(l.iter().fold(0f32, |m, x| m.max(x.abs())));
            }
            out
        };
        let bent = peak(&mut voice, &f);
        assert!(bent < 0.6, "{bent}");
        // Scripts set the shape (normalized: 0.5 is 0) and bypass.
        assert!(f.set_knob(2, Knob::Field(Kind::SurroundPanner, 0), 0.5));
        assert_eq!(f.knob(2, Knob::Field(Kind::SurroundPanner, 0)), Some(0.5));
        assert!((peak(&mut voice, &f) - 1.0).abs() < 0.05);
        assert!(f.set_knob(2, Knob::Field(Kind::SurroundPanner, 0), 1.0));
        assert!(f.set_knob(2, Knob::Bypass, 1.0));
        assert!(voice.hold(&f, &table, RATE).is_some());
        assert!(!f.set_knob(2, Knob::Field(Kind::LoFi, 0), 0.5), "another effect's value");
        // Modulation ("shaper") moves the shape in normalized units: -0.5
        // takes shape 1 back to 0, a straight wire.
        assert!(f.set_knob(2, Knob::Bypass, 0.0));
        assert_eq!(stage_knob("shaper"), Some((Kind::SurroundPanner, 0)));
        let mut m = [0.0; KNOBS];
        m[0] = -0.5;
        let mut l: [f32; 128] = std::array::from_fn(|j| (TAU * 100.0 * j as f32 / RATE).sin());
        let mut r = l;
        let dry = l;
        voice.drive(&f, 0, &m, &mut l, &mut r, RATE);
        assert!(l.iter().zip(&dry).all(|(a, b)| (a - b).abs() < 0.05));
    }

    #[test]
    fn third_drive_insert_remains_addressable_and_processes_audio() {
        use crate::fx::{Effect, params::{Field, Value}};
        let mut group = Group::default();
        // Analog Strings uses these three native slots. Bypassed earlier
        // modules must not consume the capacity needed by Saturation at 7.
        for (slot, kind) in [(5, Kind::LoFi), (6, Kind::Distortion), (7, Kind::SurroundPanner)] {
            let params = Params::Fields(crate::fx::params::layout_names(kind).unwrap().iter()
                .zip(blocks::defaults(kind).unwrap())
                .map(|(&name, &value)| Field { name, value: Value::Number(value) })
                .collect());
            group.fx.slots.push(Effect { slot, kind, version: 0, bypass: true,
                output_gain: 1.0, dry_level: 0.0, params });
        }
        let mut filter = GroupFilter::new(&group).unwrap();
        assert_eq!(filter.stages.iter().map(|s| s.slot).collect::<Vec<_>>(), [5, 6, 7]);
        assert!(filter.set_knob(7, Knob::Field(Kind::SurroundPanner, 0), 1.0));
        assert!(filter.set_knob(7, Knob::Bypass, 0.0));
        assert_eq!(filter.knob(7, Knob::Field(Kind::SurroundPanner, 0)), Some(1.0));
        let table = ModTable::default();
        let cc = [0; 128];
        let input = Inputs { cc74: None, cc: &cc, bend: 0.0, pressure: 0,
            note: 60, velocity: 100, counter: 0.0 };
        let mut voice = VoiceFilter::new(Some(&filter), &table, &input, RATE);
        let dry: [f32; 128] = std::array::from_fn(|i| (TAU * 1000.0 * i as f32 / RATE).sin());
        let (mut left, mut right) = (dry, dry);
        voice.process(&filter, &table, &mut [0.0; MAX_BLOCK], &mut left, &mut right, RATE);
        assert!(left.iter().chain(&right).all(|x| x.is_finite() && x.abs() < 0.6));
        assert!(left.iter().zip(dry).any(|(a, b)| (a - b).abs() > 0.4));
        assert!(filter.set_knob(7, Knob::Bypass, 1.0));
        let (mut left, mut right) = (dry, dry);
        voice.process(&filter, &table, &mut [0.0; MAX_BLOCK], &mut left, &mut right, RATE);
        assert_eq!(left, dry);
        assert_eq!(right, dry);
    }

    #[test]
    fn geq_shelves_and_bells() {
        let db = |p: Proto, hz: f32| 20.0 * p.gain(hz, RATE).log10();
        // LF shelf +15 dB at 116 Hz; HF shelf -15 dB at 4.9 kHz.
        let lf = Proto::geq([1.0, 0.5, 0.0], 0, RATE);
        assert!((db(lf, 20.0) - 15.0).abs() < 1.5 && db(lf, 5000.0).abs() < 0.5, "{} {}", db(lf, 20.0), db(lf, 5000.0));
        let hf = Proto::geq([0.0, 0.5, 0.0], 3, RATE);
        assert!((db(hf, 20_000.0) + 15.0).abs() < 2.0 && db(hf, 200.0).abs() < 0.5, "{} {}", db(hf, 20_000.0), db(hf, 200.0));
        // Bell switch on: a peak at the frequency, flat away from it.
        let bell = Proto::geq([1.0, 0.5, 1.0], 0, RATE);
        assert!((db(bell, 116.2) - 15.0).abs() < 0.3 && db(bell, 5000.0).abs() < 0.5);
    }

    #[test]
    fn stays_finite_under_fast_modulation() {
        let mut s = Section::default();
        let (mut l, mut r) = ([1.0; CONTROL], [-1.0; CONTROL]);
        for i in 0..20_000 {
            let hz = if i % 2 == 0 { 30.0 } else { 20_000.0 };
            s.set(Response::Low, hz, Q_MIN * Q_SPAN, RATE);
            s.process(&mut l, &mut r);
            l.fill(if i % 3 == 0 { 1.0 } else { -1.0 });
            r.fill(0.5);
        }
        assert!(s.s.iter().all(|v| v.is_finite() && v.abs() < 1e3));
    }
}
