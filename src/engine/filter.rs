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
    voice::{Ahdsr, Envelope},
};
use crate::fx::{
    Chain, Kind, Params,
    params::{EqBand, Value},
};
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
enum Shape {
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
    slot: u8,
    shape: Shape,
    /// 2-pole sections (EQ: bands).
    sections: u8,
    /// Normalized knobs; scripts write them.
    knobs: [f32; KNOBS],
    bypass: bool,
    gain: f32,
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
    fn set(&mut self, response: Response, hz: f32, q: f32, rate: f32) {
        let g = (std::f32::consts::PI * hz.min(0.49 * rate) / rate).tan();
        let k = 1.0 / q;
        let (m0, m1, m2) = match response {
            Response::Low => (0.0, 0.0, 1.0),
            Response::High => (1.0, -k, -1.0),
            Response::Band => (0.0, 1.0, 0.0),
            Response::Notch => (1.0, -k, 0.0),
        };
        self.coefficients(g, k, [m0, m1, m2]);
    }

    /// Peaking EQ band: `gain_db` at `hz`, `bw` octaves wide.
    fn set_bell(&mut self, hz: f32, bw: f32, gain_db: f32, rate: f32) {
        let g = (std::f32::consts::PI * hz.min(0.49 * rate) / rate).tan();
        let a = 10f32.powf(gain_db / 40.0);
        let q = 1.0 / (2.0 * (std::f32::consts::LN_2 * 0.5 * bw).sinh());
        let k = 1.0 / (q * a);
        self.coefficients(g, k, [1.0, k * (a * a - 1.0), 0.0]);
    }

    fn coefficients(&mut self, g: f32, k: f32, m: [f32; 3]) {
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        self.c = [a1, a2, g * a2, m[0], m[1], m[2]];
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let [a1, a2, a3, m0, m1, m2] = self.c;
        let [mut l1, mut l2, mut r1, mut r2] = self.s;
        // The loop is latency-bound. The state update `s' = 2v - s` is
        // expanded so each sample's dependency chain is a subtract, a
        // multiply and an add, and both channels share one SSE register:
        // 2.4 instead of 4.6 ns per stereo frame.
        let (b1, b2, b3) = (2.0 * a1 - 1.0, 2.0 * a2, 2.0 * a3);
        #[cfg(target_arch = "x86_64")]
        {
            // Left and right in the two low lanes of one SSE register.
            use std::arch::x86_64::*;
            // SAFETY: SSE2 is baseline on x86_64; loads and stores stay
            // within the zipped slices.
            unsafe {
                let v = _mm_set1_ps;
                let (mut s1, mut s2) = (_mm_setr_ps(l1, r1, 0.0, 0.0), _mm_setr_ps(l2, r2, 0.0, 0.0));
                for (l, r) in left.iter_mut().zip(right.iter_mut()) {
                    let x = _mm_unpacklo_ps(_mm_load_ss(l), _mm_load_ss(r));
                    let v3 = _mm_sub_ps(x, s2);
                    let v1 = _mm_add_ps(_mm_mul_ps(v(a1), s1), _mm_mul_ps(v(a2), v3));
                    let v2 = _mm_add_ps(_mm_add_ps(s2, _mm_mul_ps(v(a2), s1)), _mm_mul_ps(v(a3), v3));
                    let next1 = _mm_add_ps(_mm_mul_ps(v(b1), s1), _mm_mul_ps(v(b2), v3));
                    s2 = _mm_add_ps(_mm_add_ps(s2, _mm_mul_ps(v(b2), s1)), _mm_mul_ps(v(b3), v3));
                    s1 = next1;
                    let y = _mm_add_ps(
                        _mm_add_ps(_mm_mul_ps(v(m0), x), _mm_mul_ps(v(m1), v1)),
                        _mm_mul_ps(v(m2), v2),
                    );
                    _mm_store_ss(l, y);
                    _mm_store_ss(r, _mm_shuffle_ps(y, y, 1));
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
}

impl VoiceFilter {
    pub fn new(filter: Option<&GroupFilter>, table: &ModTable, input: &Inputs, rate: f32) -> Self {
        let mut out = Self {
            envs: [Envelope::new(&Ahdsr::UNITY, rate); MAX_ENVS],
            ext: [0.0; MAX_EXT],
            sections: [Section::default(); MAX_SECTIONS],
            tuned: [[f32::NAN; 3]; MAX_SECTIONS],
            matrix: IDENTITY,
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

    pub fn release(&mut self) {
        self.envs.iter_mut().for_each(|e| e.release(None));
    }

    /// Filter one block (at most [`MAX_BLOCK`] frames) in place; `ctl` is scratch.
    #[allow(clippy::too_many_arguments)]
    pub fn process(
        &mut self,
        f: &GroupFilter,
        table: &ModTable,
        input: &Inputs,
        ctl: &mut [f32; MAX_BLOCK],
        left: &mut [f32],
        right: &mut [f32],
        rate: f32,
    ) {
        let n = left.len();
        for (value, (_, i)) in self.ext.iter_mut().zip(&f.ext) {
            table.mods[*i as usize].follow(value, input, n, rate);
        }
        // Envelope levels at each control tick.
        let mut levels = [[0.0; MAX_BLOCK / CONTROL]; MAX_ENVS];
        for (env, ticks) in self.envs.iter_mut().zip(&mut levels).take(f.envs.len()) {
            env.render(&mut ctl[..n], None, rate);
            for (t, level) in ticks.iter_mut().enumerate().take(n.div_ceil(CONTROL)) {
                *level = ctl[t * CONTROL];
            }
        }
        let modulated = !f.envs.is_empty() || !f.ext.is_empty();
        let step = if modulated { CONTROL } else { n };
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
                let k = knobs.map(|k| k.clamp(0.0, 1.0));
                for b in 0..unit.sections as usize {
                    let (section, tuned) = (&mut self.sections[s + b], &mut self.tuned[s + b]);
                    let key = match unit.shape {
                        Shape::Filter(_) => [k[0], k[1], 0.0],
                        Shape::Eq => [k[3 * b], k[3 * b + 1], k[3 * b + 2]],
                    };
                    // Bypassed units and flat EQ bands are identities: skip
                    // them, restarting from rest when they return.
                    if unit.bypass || (unit.shape == Shape::Eq && (key[2] - 0.5).abs() < FLAT) {
                        section.s = [0.0; 4];
                        continue;
                    }
                    if key != *tuned {
                        *tuned = key;
                        match unit.shape {
                            Shape::Filter(response) => section.set(
                                response,
                                CUTOFF_MIN_HZ * (CUTOFF_OCTAVES * key[0]).exp2(),
                                Q_MIN * Q_SPAN.powf(key[1]),
                                rate,
                            ),
                            Shape::Eq => section.set_bell(
                                EQ_MIN_HZ * 10f32.powf(EQ_DECADES * key[0]),
                                BW_MIN + BW_SPAN * key[1],
                                GAIN_DB * (2.0 * key[2] - 1.0),
                                rate,
                            ),
                        }
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
        s.process(&mut left[..100], &mut right[..100]);
        s.process(&mut left[100..], &mut right[100..]);
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
                voice.process(&filter, &table, &input, &mut ctl, &mut l, &mut r, RATE);
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
                voice.process(f, &table, &input, &mut ctl, &mut l, &mut r, RATE);
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
