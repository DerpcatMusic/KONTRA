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
    params::{EqBand, Value},
};
use crate::audio::Frame;
use crate::import::{Group, ModAssignment, ModTarget};

/// Frames between coefficient updates.
pub(crate) const CONTROL: usize = 32;
/// Filter and EQ units per group.
const MAX_UNITS: usize = 4;
/// 2-pole sections per voice, all units together.
const MAX_SECTIONS: usize = 8;
/// Module envelopes and external assignments a voice follows.
const MAX_ENVS: usize = 4;
const MAX_EXT: usize = 8;
/// Knobs per unit: cutoff and resonance, or frequency, bandwidth and gain per band.
const KNOBS: usize = 9;

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

/// Kontakt filter type id → response and 2-pole section count. Low/high
/// direction is medium confidence, pole counts low (see `audits/EFFECTS.md`).
fn filter_type(id: i32) -> Option<(Response, u8)> {
    use Response::*;
    Some(match id {
        2 => (Low, 1),
        3 => (High, 1),
        4 => (Band, 1),
        5 => (Low, 2),
        6 => (High, 2),
        7 => (Band, 2),
        8 => (Notch, 2),
        9 => (Low, 3),
        52 => (Low, 1),
        53 => (Band, 1),
        54 => (High, 1),
        55 => (Low, 2),
        56 => (Band, 2),
        57 => (High, 2),
        _ => return None,
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Shape {
    Filter(Response),
    Eq,
}

/// Parameter of a group insert slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Knob {
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
}

impl Knob {
    /// Modulation target names: `filterCutoff`, `eqGain2`...
    fn parse(name: &str) -> Option<Self> {
        let band = |rest: &str| rest.parse::<u8>().ok().filter(|b| (1..=3).contains(b)).map(|b| b - 1);
        Some(match name {
            "filterCutoff" => Self::Cutoff,
            "filterResonance" => Self::Resonance,
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
            Self::Freq(b) => 3 * b as usize,
            Self::Bandwidth(b) => 3 * b as usize + 1,
            Self::Gain(b) => 3 * b as usize + 2,
            Self::Bypass | Self::Output | Self::Spread | Self::Pan => return None,
        })
    }

    /// Whether a unit of `shape` with `sections` holds this knob.
    fn fits(self, shape: Shape, sections: u8) -> bool {
        match (shape, self) {
            (Shape::Filter(_), Self::Cutoff | Self::Resonance) => true,
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
}

impl Unit {
    /// Section `b`'s knobs (clamped) from the unit's `knobs`, and whether
    /// it is a flat EQ band (an identity).
    fn key(&self, knobs: &[f32; KNOBS], b: usize) -> ([f32; 3], bool) {
        let k = knobs.map(|k| k.clamp(0.0, 1.0));
        match self.shape {
            Shape::Filter(_) => ([k[0], k[1], 0.0], false),
            Shape::Eq => {
                let key = [k[3 * b], k[3 * b + 1], k[3 * b + 2]];
                (key, (key[2] - 0.5).abs() < FLAT)
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

    /// A unit's section from normalized knobs: `[cutoff, resonance, _]` or
    /// `[freq, bandwidth, gain]`.
    fn of(shape: Shape, key: [f32; 3], rate: f32) -> Self {
        match shape {
            Shape::Filter(response) => {
                let (hz, q) = filter_settings(key[0], key[1]);
                Self::filter(response, hz, q, rate)
            }
            Shape::Eq => {
                let (hz, bw, db) = band_settings(key[0], key[1], key[2]);
                Self::bell(hz, bw, db, rate)
            }
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

/// A modulation route onto a unit's knob.
#[derive(Clone, Debug, PartialEq)]
struct Route {
    unit: u8,
    knob: u8,
    /// Invert button, read as a negative direction.
    sign: f32,
}

/// `[ll, lr, rl, rr]`: `l' = ll·l + lr·r`, `r' = rl·l + rr·r`.
type Matrix = [f32; 4];
const IDENTITY: Matrix = [1.0, 0.0, 0.0, 1.0];

/// A group's filters, EQs and channel mixers with their modulation, shared
/// by its voices.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupFilter {
    units: Box<[Unit]>,
    mixers: Box<[Mixer]>,
    /// Stereo Modellers and every active slot's output gain as one matrix:
    /// they are linear and the filters treat both channels alike, so the
    /// order does not matter.
    matrix: Matrix,
    /// Module envelopes and what they drive (intensity and shaper in `Mod`).
    envs: Box<[(Ahdsr, Box<[(Route, Mod)]>)]>,
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
                let (response, sections) = filter_type(f.filter_type)?;
                knobs[..2].copy_from_slice(&[f.cutoff, f.resonance]);
                (Shape::Filter(response), sections)
            }
            Params::Eq(eq) => {
                for (k, band) in knobs.chunks_mut(3).zip(&eq.bands) {
                    k.copy_from_slice(&normalized(band));
                }
                (Shape::Eq, eq.bands.len() as u8)
            }
            _ => return None,
        };
        Some(Unit {
            slot: fx.slot as u8,
            shape,
            sections,
            knobs,
            bypass: fx.bypass,
            gain: fx.output_gain,
        })
    })
}

/// Inverter phase/swap buttons (two flags, meaning unverified) that are set.
fn inverter_flags(params: &Params) -> bool {
    matches!(params, Params::Fields(f) if f.iter().any(|f| matches!(f.value, Value::Flag(true))))
}

/// Import warnings for group effects that do not play.
pub fn unsupported(chain: &Chain) -> Vec<String> {
    let mut out = Vec::new();
    for fx in chain.slots.iter().filter(|fx| !fx.bypass) {
        match &fx.params {
            Params::Filter(f) if filter_type(f.filter_type).is_none() => {
                out.push(format!("Group filter type {} is not implemented; audio passes through", f.filter_type));
            }
            Params::Filter(_) | Params::Eq(_) => {}
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
        let units = units_;
        let mixers: Box<[Mixer]> = group
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
        if units.is_empty() && mixers.is_empty() {
            return None;
        }
        let route = |m: &ModAssignment| {
            let ModTarget::Module { param, slot } = &m.target else {
                return None;
            };
            let knob = Knob::parse(param)?;
            let unit = units.iter().position(|u| u.slot == *slot && knob.fits(u.shape, u.sections))?;
            Some(Route {
                unit: unit as u8,
                knob: knob.index()? as u8,
                sign: if m.invert { -1.0 } else { 1.0 },
            })
        };
        let envs = group
            .envelopes
            .iter()
            .filter_map(|e| {
                let routes: Box<[_]> = e.targets.iter().filter_map(|m| Some((route(m)?, Mod::from(m)))).collect();
                (!routes.is_empty()).then(|| (Ahdsr::from(&e.env), routes))
            })
            .take(MAX_ENVS)
            .collect();
        let ext = group
            .mods
            .iter()
            .enumerate()
            .filter_map(|(i, m)| Some((route(m)?, i as u16)))
            .take(MAX_EXT)
            .collect();
        let mut out = Box::new(Self {
            units: units.into(),
            mixers,
            matrix: IDENTITY,
            envs,
            ext,
        });
        out.matrix = out.mix();
        Some(out)
    }

    /// The stereo matrix of the active mixers and output gains. The Stereo
    /// Modeller scales the side signal by `1 + spread`, then balances with
    /// the rack's law (the far side attenuates linearly).
    fn mix(&self) -> Matrix {
        let gains = self.units.iter().filter(|u| !u.bypass).map(|u| u.gain);
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
                    gain *= Proto::of(unit.shape, key, rate).gain(hz, rate);
                }
            }
        }
        gain
    }

    /// A slot's stored parameter (`get_engine_par`).
    pub(crate) fn knob(&self, slot: u8, knob: Knob) -> Option<f32> {
        let unit = self.units.iter().find(|u| u.slot == slot);
        let mixer = self.mixers.iter().find(|m| m.slot == slot);
        match knob {
            Knob::Bypass => unit.map(|u| u.bypass).or(mixer.map(|m| m.bypass)).map(f32::from),
            Knob::Output => unit.map(|u| u.gain).or(mixer.map(|m| m.gain)),
            Knob::Spread | Knob::Pan => Some(mixer?.stereo?[usize::from(knob == Knob::Pan)]),
            _ => {
                let unit = unit.filter(|u| knob.fits(u.shape, u.sections))?;
                Some(unit.knobs[knob.index()?])
            }
        }
    }

    /// Set a slot's parameter (`set_engine_par`); false if the slot lacks it.
    pub(crate) fn set_knob(&mut self, slot: u8, knob: Knob, value: f32) -> bool {
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
        self.matrix = self.mix();
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
    matrix: Matrix,
    /// The active sections of the last [`VoiceFilter::key`], in order.
    slots: [u8; LANE_SECTIONS],
}

impl VoiceFilter {
    pub fn new(filter: Option<&GroupFilter>, table: &ModTable, input: &Inputs, rate: f32) -> Self {
        let mut out = Self {
            envs: [Envelope::new(&Ahdsr::UNITY, rate); MAX_ENVS],
            ext: [0.0; MAX_EXT],
            sections: [Section::default(); MAX_SECTIONS],
            tuned: [[f32::NAN; 3]; MAX_SECTIONS],
            matrix: IDENTITY,
            slots: [0; LANE_SECTIONS],
        };
        if let Some(f) = filter {
            for (env, (params, _)) in out.envs.iter_mut().zip(&f.envs) {
                *env = Envelope::new(params, rate);
            }
            for (value, (_, i)) in out.ext.iter_mut().zip(&f.ext) {
                *value = table.mods[*i as usize].start_value(input);
            }
            out.matrix = f.matrix;
        }
        out
    }

    /// Move the external sources on over `n` frames, before [`process`](Self::process).
    pub fn follow(&mut self, f: &GroupFilter, table: &ModTable, input: &Inputs, n: usize, rate: f32) {
        for (value, (_, i)) in self.ext.iter_mut().zip(&f.ext) {
            table.mods[*i as usize].follow(value, input, n, rate);
        }
    }

    /// This block's [`FilterKey`], when the filter is held: no module
    /// envelopes and the matrix reached, with at most four active
    /// sections. Tunes the sections it keeps.
    pub fn key(&mut self, f: &GroupFilter, table: &ModTable, rate: f32) -> Option<FilterKey> {
        if !f.envs.is_empty() || self.matrix != f.matrix {
            return None;
        }
        let mut knobs: [[f32; KNOBS]; MAX_UNITS] =
            std::array::from_fn(|u| f.units.get(u).map_or([0.0; KNOBS], |unit| unit.knobs));
        for ((r, i), value) in f.ext.iter().zip(&self.ext) {
            knobs[r.unit as usize][r.knob as usize] += r.sign * table.mods[*i as usize].intensity * value;
        }
        let mut key = FilterKey { matrix: f.matrix, ..FilterKey::default() };
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
                    self.sections[s + b].coefficients(Proto::of(unit.shape, k, rate));
                }
                key.c[active] = self.sections[s + b].c;
                self.slots[active] = (s + b) as u8;
                active += 1;
            }
            s += unit.sections as usize;
        }
        key.active = active as u8;
        Some(key)
    }

    pub fn release(&mut self) {
        self.envs.iter_mut().for_each(|e| e.release(None));
    }

    /// Clear the sections' state, as silent input leaves it.
    pub fn rest(&mut self) {
        self.sections.iter_mut().for_each(|s| s.s = [0.0; 4]);
    }

    /// Filter one block (at most [`MAX_BLOCK`] frames) in place; `ctl` is scratch.
    #[allow(clippy::too_many_arguments)]
    pub fn process(
        &mut self,
        f: &GroupFilter,
        table: &ModTable,
        ctl: &mut [f32; MAX_BLOCK],
        left: &mut [f32],
        right: &mut [f32],
        rate: f32,
    ) {
        let n = left.len();
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
        for (t, start) in (0..n).step_by(step).enumerate().take_while(|_| !f.units.is_empty()) {
            let end = (start + step).min(n);
            let mut knobs: [[f32; KNOBS]; MAX_UNITS] = std::array::from_fn(|u| {
                f.units.get(u).map_or([0.0; KNOBS], |unit| unit.knobs)
            });
            for (e, (_, routes)) in f.envs.iter().enumerate() {
                for (r, m) in routes.iter() {
                    knobs[r.unit as usize][r.knob as usize] += r.sign * m.intensity * m.shape(levels[e][t]);
                }
            }
            for ((r, i), value) in f.ext.iter().zip(&self.ext) {
                knobs[r.unit as usize][r.knob as usize] += r.sign * table.mods[*i as usize].intensity * value;
            }
            let mut s = 0;
            for (unit, knobs) in f.units.iter().zip(&knobs) {
                for b in 0..unit.sections as usize {
                    let (section, tuned) = (&mut self.sections[s + b], &mut self.tuned[s + b]);
                    let (key, flat) = unit.key(knobs, b);
                    // Bypassed units and flat EQ bands are identities: skip
                    // them, restarting from rest when they return.
                    if unit.bypass || flat {
                        section.s = [0.0; 4];
                        continue;
                    }
                    if key != *tuned {
                        *tuned = key;
                        section.coefficients(Proto::of(unit.shape, key, rate));
                    }
                    section.process(&mut left[start..end], &mut right[start..end]);
                }
                s += unit.sections as usize;
            }
        }
        if f.matrix != IDENTITY || self.matrix != IDENTITY {
            apply(self.matrix, f.matrix, left, right);
            self.matrix = f.matrix;
        }
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
    /// reaches the end state.
    pub fn dots_out(&mut self, left: &[f32], right: &[f32]) {
        #[cfg(target_arch = "x86_64")]
        if std::arch::is_x86_feature_detected!("avx2") {
            // SAFETY: the running CPU supports AVX2.
            return unsafe { self.dots_out_avx2(left, right) };
        }
        self.dots_out_body(left, right);
    }

    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2")]
    fn dots_out_avx2(&mut self, left: &[f32], right: &[f32]) {
        self.dots_out_body(left, right);
    }

    #[inline(always)]
    fn dots_out_body(&mut self, left: &[f32], right: &[f32]) {
        let [mut dl, mut dr] = self.d;
        let k = &self.tunings[self.tuning].k;
        for ((l, r), k) in left.iter().zip(right).zip(k.iter()) {
            for q in 0..LANE_STATES {
                dl[q] += k[q] * l;
                dr[q] += k[q] * r;
            }
        }
        self.d = [dl, dr];
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

    const RATE: f32 = 48_000.0;

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
        let input = Inputs { cc: &cc, bend: 0.0, pressure: 0, note: 60, velocity: 100, counter: 0.0 };
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
        let unit = |slot, shape, bypass, gain| Unit { slot, shape, sections: 1, knobs: [0.5; KNOBS], bypass, gain };
        let mut f = GroupFilter {
            units: [
                Unit { knobs: [0.0; KNOBS], ..unit(0, Shape::Filter(Response::Low), true, 0.5) },
                unit(1, Shape::Eq, false, 2.0),
            ]
            .into(),
            // Stereo Modeller: mono, panned half right.
            mixers: [Mixer { slot: 2, stereo: Some([-1.0, 0.5]), bypass: false, gain: 1.0 }].into(),
            matrix: IDENTITY,
            envs: [].into(),
            ext: [].into(),
        };
        f.matrix = f.mix();
        let table = ModTable::default();
        let cc = [0u8; 128];
        let input = Inputs { cc: &cc, bend: 0.0, pressure: 0, note: 60, velocity: 100, counter: 0.0 };
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
