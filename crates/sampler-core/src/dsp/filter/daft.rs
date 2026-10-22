//! Kontakt's Daft filter: a two-stage zero-delay loop run at twice the rate through the shared
//! Kontakt x2 resampler ([`kaiser`]), with two knees ([`shaper::Knee`]) inside the loop.
//!
//! Native (Kontakt 8.13.1): the per-2x-sample loop is `FUN_140affa30`; the controls are
//! `FUN_140b05900` (gain `FUN_140b05290`, cutoff and feedback `FUN_140b04c80`, resonance and
//! response in the same dispatcher); the ramp object is `FUN_140acb6c0` (four lanes at `0x48e30`,
//! targets at `0x48e50`); the table is `FUN_1404d14d0`; the prepare is `FUN_140afd210`. The
//! constants are read from the image. Stage A runs at the quarter angle (tan of `pi f / 4 rate`),
//! stage B at the full angle; both use the tan series of `FUN_140affa30`.
//! Ponytail: the lanes begin at zero in the native object and ramp in; here they snap at the first
//! control tick, and the ramp clock follows the 32-frame control quantum.
use super::super::control::{ControlRamp, ControlRange, Parameter, PreparedParameter};
use super::super::kaiser::{self, Kaiser};
use super::super::{Planar, ProcessorState};
use super::shaper::{Knee, Slope};
use super::table::{exponential, prepare as prepare_table, Past};

/// Frames between control updates (the ramp object's clock).
const QUANTUM: u64 = 32;

/// Constants as stored in `Kontakt8.exe`; the hex is the f32 bit pattern at the named address.
mod native {
    /// Stage A tan series: pi^4/120 (0x144fcfd40), pi^2/12 (0x144fcfd44), pi/2 (0x144714960).
    pub(super) const A1: f32 = f32::from_bits(0x3f4f_ce5c);
    pub(super) const A2: f32 = f32::from_bits(0x3f52_8d34);
    pub(super) const KA: f32 = f32::from_bits(0x3fc9_0fdb);
    /// Stage B tan series: 2 pi^4/15 (0x144fcfe08), pi^2/3 (0x144fcfdb8), pi (0x1447149d4).
    pub(super) const B1: f32 = f32::from_bits(0x414f_ce5c);
    pub(super) const B2: f32 = f32::from_bits(0x4052_8d34);
    pub(super) const KB: f32 = f32::from_bits(0x4049_0fdb);
    /// Input drive (0x144725080).
    pub(super) const DRIVE: f32 = 0.6;
    /// Resonance damping scale (0x144f38820).
    pub(super) const DAMP: f32 = 0.8;
    /// Output resonance trim (0x144fcfd68) and output gain (0x144714980).
    pub(super) const TRIM: f32 = f32::from_bits(0x3f99_9999);
    pub(super) const OUT: f32 = f32::from_bits(0x3fe6_6666);
    /// Feedback `1.6 - min(hz / 2, 14000) * 5.714285725844093e-5` (0x144714964, 0x144fcfc8c).
    pub(super) const FEEDBACK: f32 = f32::from_bits(0x3fcc_cccd);
    pub(super) const FEEDBACK_SCALE: f32 = f32::from_bits(0x386f_acad);
}

/// First knee: clamp to +-1, then `(1 - 0.5 |c|) c` (native: `0.25 |c|` added twice).
const KNEE_A: Knee = Knee { limit: 1.0, slope: Slope::Fused(0.5) };
/// Second knee: clamp to +-4, then `(1 - 0.125 |c|) c` (native constant 0x144f39df4).
const KNEE_B: Knee = Knee { limit: 4.0, slope: Slope::Fused(0.125) };

/// Normalized controls (0..=1): leading gain (12 dB at 1), cutoff, resonance and response (below
/// 0.5 low pass, from 0.5 high pass).
#[derive(Clone, Copy, Debug)]
pub struct DaftSettings {
    pub gain: Parameter,
    pub cutoff: Parameter,
    pub resonance: Parameter,
    pub response: Parameter,
}

impl DaftSettings {
    fn parameters(&self) -> [Parameter; 4] {
        [self.gain, self.cutoff, self.resonance, self.response]
    }

    pub(crate) fn valid(&self) -> bool {
        self.parameters()
            .iter()
            .all(|p| p.valid() && !matches!(p, Parameter::Expression { .. }))
    }

    pub(crate) fn compile(self, rate: u32, bindings: &mut Vec<ControlRange>) -> Daft {
        prepare_table();
        let rate = rate as f32;
        // Native: trunc((2 rate * 0.002) / 64 + 0.5), at least one quantum.
        let ramp_quanta = ((rate + rate) * 0.002 / 64. + 0.5).max(1.) as u32;
        Daft {
            rate,
            lanes: self.parameters().map(|p| p.compile(bindings)),
            ramp_quanta,
            oversampler: Kaiser::new(2),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Daft {
    /// The base sample rate; the loop runs at twice it.
    rate: f32,
    lanes: [PreparedParameter; 4],
    ramp_quanta: u32,
    oversampler: Kaiser,
}

/// Per-instance Daft state: the loop's three states per channel and the x2 resampler per channel.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct State {
    /// `[band high, band, low]` per channel (native `pfVar13[-1]`, `pfVar13[0]`, `local_1c8`).
    stage: [[f32; 3]; 2],
    resample: [kaiser::State; 2],
}

// Layout of `ProcessorState::aux`: four ramped lanes (current, delta, target), the ramp clock and
// the response mode, which the native setter writes immediately rather than ramping.
const CURRENT: usize = 0;
const DELTA: usize = 4;
const TARGET: usize = 8;
const REMAINING: usize = 12;
const STARTED: usize = 13;
const HIGH: usize = 14;

/// One tan series in the native order: `((c1 (w f)^2 + c2)... )`: the Taylor terms in `f w`, times
/// `f k w` (`FUN_140affa30`).
#[inline(always)]
fn tan_series(f: f32, w: f32, c1: f32, c2: f32, k: f32) -> f32 {
    let p = (w * w * f * f * c1 + c2) * f * f * w * w + 1.0;
    p * f * k * w
}

/// Feedback of a cutoff in Hz (`FUN_140b04c80`).
fn feedback_for(hz: f32) -> f32 {
    native::FEEDBACK - (hz * 0.5).min(14_000.) * native::FEEDBACK_SCALE
}

impl Daft {
    pub(crate) fn trace_parameters(&self) -> [(&'static str, PreparedParameter); 4] {
        std::array::from_fn(|i| {
            (
                ["gain", "cutoff", "resonance", "response"][i],
                self.lanes[i],
            )
        })
    }

    /// The four ramped lanes `[gain amplitude, cutoff Hz, feedback, resonance]` from the four
    /// normalized controls `[gain, cutoff, resonance, response]`. Feedback follows the cutoff.
    fn targets(x: [f64; 4]) -> [f32; 4] {
        // The native gain setter does not clamp its (signed) leading value.
        let gain = if x[0].is_nan() { 0. } else { x[0] as f32 };
        let x = x.map(|v| if v.is_nan() { 0. } else { v.clamp(0., 1.) as f32 });
        let hz = exponential(x[1] * 625. + 1_481.881_6, Past::Tail).clamp(1., 30_000.);
        let resonance = 1. - (1. - x[2]) * (1. - x[2]);
        [exponential(gain * 119.589_41 + 1_200., Past::Tail), hz, feedback_for(hz), resonance]
    }

    /// One 2x-rate sample of the loop: `lanes` as in [`Self::targets`], `w = 1 / (2 rate)`,
    /// `high` selects the high-pass output and `s` is the channel state (updated).
    #[inline(always)]
    fn tick(lanes: [f32; 4], w: f32, high: bool, s: &mut [f32; 3], x: f32) -> f32 {
        let [gain, f, feedback, r] = lanes;
        let ga = tan_series(f, w, native::A1, native::A2, native::KA);
        let gb = tan_series(f, w, native::B1, native::B2, native::KB);
        let [sa, sb, sc] = *s;
        let damp_term = r * native::DAMP;
        let sa2 = sa + sa;
        let two_damp = (1.0 - damp_term) + (1.0 - damp_term);
        let m = two_damp * gb + gb * gb;
        let input = gain * x * native::DRIVE;
        let f19 = sb + sb + sa2 * gb;
        let v22 = sa2 * two_damp + f19;
        let d = 1.0 / (m + 1.0);
        let dg2 = d * gb * gb;
        let e = 1.0 / (gb + 1.0);
        let num = (input - e * (sc + sc)) * dg2 + (f19 - dg2 * v22);
        let den = 1.0 - (e * damp_term) * dg2;
        let mut y = (num / den) * damp_term;
        y -= ((y * ga + sc) + sc) * e;
        let sc = y * ga + sc;
        let input = KNEE_A.apply(y) * feedback + input;
        let shaped = KNEE_B.apply(input);
        let u = (shaped - (shaped * m + v22) * d) * gb + sa;
        let band = (sa + u) * gb + sb;
        let low = sb + band;
        *s = [u, band, sc];
        let (hp, lp) = if high { (1.0, 0.0) } else { (0.0, 1.0) };
        ((shaped - low) * hp + low * lp) * (native::OUT - r * r * native::TRIM)
    }

    fn control(&self, state: &mut ProcessorState, parameters: &[ControlRamp], at: u64, modulation: [f64; 4]) {
        let mut x = self.lanes.map(|lane| lane.value(parameters, at, None));
        x[0] += modulation[2];
        // Native FUN_1408fa270 clamps every enabled route to [0, 1] before setParam, even at zero
        // delta; bit 4 of modulation[3] marks a Gain route (as the Ladder does).
        if (modulation[3] as u8) & 4 != 0 {
            x[0] = x[0].clamp(0., 1.);
        }
        x[1] += modulation[0];
        x[2] += modulation[1];
        let target = Self::targets(x);
        let a = &mut state.aux;
        a[HIGH] = if (2. * x[3].clamp(0., 1.)).trunc() >= 1. { 1. } else { 0. };
        if a[STARTED] == 0. {
            a[CURRENT..CURRENT + 4].copy_from_slice(&target.map(f64::from));
            a[TARGET..TARGET + 4].copy_from_slice(&target.map(f64::from));
            a[STARTED] = 1.;
            return;
        }
        if a[REMAINING] > 0. {
            a[REMAINING] -= 1.;
            if a[REMAINING] == 0. {
                a.copy_within(TARGET..TARGET + 4, CURRENT);
                a[DELTA..DELTA + 4].fill(0.);
            }
        }
        // Native: one dirty flag for all four lanes, set by any setter. A routed setter writes on
        // every control chunk (`modulation[3]` bits 1, 2, 4: cutoff, resonance, gain), so its ramp
        // restarts each chunk even when the value holds. The tick then re-derives every lane's delta.
        if modulation[3] != 0. || a[TARGET..TARGET + 4] != target.map(f64::from) {
            // Native: delta = (target - current) * (1 / (quanta * 32 * 2)) in f32, snapped below 1e-15.
            let inverse = 1. / ((self.ramp_quanta * QUANTUM as u32 * 2) as f32);
            for k in 0..4 {
                let delta = (target[k] - a[CURRENT + k] as f32) * inverse;
                if f64::from(delta * delta) < 1e-15 {
                    a[CURRENT + k] = f64::from(target[k]);
                    a[DELTA + k] = 0.;
                } else {
                    a[DELTA + k] = f64::from(delta);
                }
            }
            a[TARGET..TARGET + 4].copy_from_slice(&target.map(f64::from));
            a[REMAINING] = f64::from(self.ramp_quanta);
        }
    }

    /// Filter `len` planar frames in place from absolute frame `at`.
    pub(crate) fn process(
        &self,
        state: &mut ProcessorState,
        parameters: &[ControlRamp],
        block: &mut Planar,
        len: usize,
        at: u64,
        modulation: [f64; 4],
    ) {
        let w = 1. / (self.rate + self.rate);
        let mut i = 0;
        while i < len {
            let t = at + i as u64;
            if t % QUANTUM == 0 || state.aux[STARTED] == 0. {
                self.control(state, parameters, t, modulation);
            }
            let run = ((QUANTUM - t % QUANTUM) as usize).min(len - i);
            let high = state.aux[HIGH] != 0.;
            for n in i..i + run {
                let mut up = [[0f32; 2]; 2];
                for c in 0..2 {
                    self.oversampler.up(&mut state.daft.resample[c], block[c][n] as f32, &mut up[c]);
                }
                let mut frame = [[0f32; 2]; 2];
                for half in 0..2 {
                    if state.aux[DELTA..DELTA + 4].iter().any(|d| *d != 0.) {
                        for k in 0..4 {
                            state.aux[CURRENT + k] = f64::from(state.aux[CURRENT + k] as f32 + state.aux[DELTA + k] as f32);
                        }
                    }
                    let lanes: [f32; 4] = std::array::from_fn(|k| state.aux[CURRENT + k] as f32);
                    for c in 0..2 {
                        frame[c][half] = Self::tick(lanes, w, high, &mut state.daft.stage[c], up[c][half]);
                    }
                }
                for c in 0..2 {
                    block[c][n] = f64::from(self.oversampler.down(&mut state.daft.resample[c], &frame[c]));
                }
            }
            i += run;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FNV-1a over the output bits for both responses. Any change to the loop's arithmetic or its
    /// order shows here.
    #[test]
    fn output_is_bit_stable() {
        let golden = [0x7d97_d228_ea8b_9b25u64, 0x774a_1a28_ea8b_9b25u64];
        for (response, expected) in [0., 1.].into_iter().zip(golden) {
            let daft = DaftSettings {
                gain: Parameter::Constant(0.3),
                cutoff: Parameter::Constant(0.6),
                resonance: Parameter::Constant(0.7),
                response: Parameter::Constant(response),
            }
            .compile(48_000, &mut Vec::new());
            let mut state = ProcessorState::default();
            let mut hash = 0xcbf2_9ce4_8422_2325u64;
            for n in 0..24 {
                let mut block = [[0.; crate::dsp::BLOCK]; 2];
                for (i, v) in block[0].iter_mut().enumerate() {
                    let t = (n * crate::dsp::BLOCK + i) as f64 / 48_000.;
                    *v = 0.9 * (std::f64::consts::TAU * 1_000. * t).sin() + 0.2 * (std::f64::consts::TAU * 61. * t).sin();
                }
                block[1] = block[0];
                daft.process(&mut state, &[], &mut block, crate::dsp::BLOCK, (n * crate::dsp::BLOCK) as u64, [0.; 4]);
                for v in block[0] {
                    hash = (hash ^ v.to_bits()).wrapping_mul(0x0000_0100_0000_01b3);
                }
            }
            println!("response {response}: {hash:#018x}");
            assert_eq!(hash, expected);
        }
    }

    #[test]
    fn parameter_laws_match_the_native_table_points() {
        // DSP_SYSTEM_INVENTORY: normalized input, gain amplitude, cutoff Hz.
        for (x, amplitude, hz) in [
            (0., 1., 25.956_726),
            (0.25, 1.412_546, 157.827_438),
            (0.5, 1.995_283, 959.662_048),
            (0.75, 2.818_422, 5_835.129_395),
            (1., 3.981_133, 30_000.),
        ] {
            let t = Daft::targets([x, x, 0., 0.]);
            assert!((f64::from(t[0]) / amplitude - 1.).abs() < 1e-4, "{x}: {}", t[0]);
            assert!((f64::from(t[1]) / hz - 1.).abs() < 1e-4, "{x}: {}", t[1]);
        }
        // Feedback at the top cutoff: 1.6 - 14000 * 5.714e-5.
        assert!((f64::from(Daft::targets([0., 1., 0., 0.])[2]) - (1.6 - 14_000. * 5.714_285_725_844_093e-5)).abs() < 1e-6);
        assert!((f64::from(Daft::targets([0., 0., 0.5, 0.])[3]) - 0.75).abs() < 1e-6);
    }

    #[test]
    fn ramp_length_follows_the_native_rounding() {
        let quanta = |rate| {
            DaftSettings {
                gain: Parameter::Constant(0.),
                cutoff: Parameter::Constant(0.),
                resonance: Parameter::Constant(0.),
                response: Parameter::Constant(0.),
            }
            .compile(rate, &mut Vec::new())
            .ramp_quanta
        };
        assert_eq!(quanta(48_000), 3);
        assert_eq!(quanta(22_050), 1);
    }
}

#[cfg(test)]
mod kemu_vectors {
    use super::*;

    /// Native Daft through kemu (`tools/re/daft_kemu_vectors.py`): constructor, prepare, setters, the
    /// 32-frame tick and the loop, stereo. A modulation route is written on every block; a disabled
    /// control is fixed at its first value.
    /// Replays each case through the production kernel; returns the cases that miss the native audio
    /// (worst error above 2e-6 of peak).
    fn replay(json: &str) -> Vec<String> {
        let cases: Vec<serde_json::Value> = serde_json::from_str(json).unwrap();
        let num = |v: &serde_json::Value| v.as_f64().unwrap();
        let mut failures = Vec::new();
        for case in &cases {
            let name = case["name"].as_str().unwrap();
            let schedule: Vec<[f64; 3]> = case["schedule"]
                .as_array()
                .unwrap()
                .iter()
                .map(|k| [num(&k[0]), num(&k[1]), num(&k[2])])
                .collect();
            let enabled: Vec<bool> = case["enabled"].as_array().unwrap().iter().map(|v| v.as_bool().unwrap()).collect();
            let fixed = schedule[0];
            let base = |i: usize| Parameter::Constant(if enabled[i] { 0. } else { fixed[i] });
            let daft = DaftSettings {
                gain: base(0),
                cutoff: base(1),
                resonance: base(2),
                response: Parameter::Constant(num(&case["response"])),
            }
            .compile(num(&case["rate"]) as u32, &mut Vec::new());
            let stim: Vec<Vec<f64>> = case["stim"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| c.as_array().unwrap().iter().map(|v| num(v) as f32 as f64).collect())
                .collect();
            let mut state = ProcessorState::default();
            let mut out: [Vec<f64>; 2] = Default::default();
            for (block, knobs) in schedule.iter().enumerate() {
                let mut buffer: Planar = [[0.; crate::dsp::BLOCK]; 2];
                let range = block * 32..(block + 1) * 32;
                buffer[0][..32].copy_from_slice(&stim[0][range.clone()]);
                buffer[1][..32].copy_from_slice(&stim[1][range]);
                // Route bits as the voice plan writes them: 1 cutoff, 2 resonance, 4 gain.
                let routes = [(1, 1.), (2, 2.), (0, 4.)]
                    .iter()
                    .filter(|(i, _)| enabled[*i])
                    .fold(0., |m, (_, bit)| m + bit);
                let modulation = [
                    if enabled[1] { knobs[1] } else { 0. },
                    if enabled[2] { knobs[2] } else { 0. },
                    if enabled[0] { knobs[0] } else { 0. },
                    routes,
                ];
                daft.process(&mut state, &[], &mut buffer, 32, (block * 32) as u64, modulation);
                for c in 0..2 {
                    out[c].extend_from_slice(&buffer[c][..32]);
                }
            }
            let peak = ["outL", "outR"]
                .iter()
                .flat_map(|key| case[*key].as_array().unwrap().iter().map(num))
                .fold(0f64, |m, v| m.max(v.abs()));
            for (c, key) in [(0, "outL"), (1, "outR")] {
                let want: Vec<f64> = case[key].as_array().unwrap().iter().map(num).collect();
                let worst = out[c].iter().zip(&want).map(|(g, w)| (g - w).abs()).fold(0f64, f64::max) / peak;
                // Worst case: self-oscillating feedback (resonance 0.95) amplifies f32 rounding to about
                // 1.1e-6 of peak; every other case is within 2e-7.
                if worst > 2e-6 {
                    failures.push(format!("{name} {key} worst {worst:e} of peak"));
                }
            }
        }
        failures
    }

    #[test]
    fn native_kemu_vectors_through_the_production_path() {
        let failures = replay(include_str!("daft_kemu_vectors.json"));
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// Native routed knobs (`tools/re/daft_route_clamp_kemu.py`): the BFilterDaft driver clamps each
    /// enabled route to [0, 1] before setParam, so the raw sources here (-0.5, 1.7) must match the
    /// native audio.
    #[test]
    fn native_route_clamp_vectors() {
        let failures = replay(include_str!("daft_route_clamp_kemu.json"));
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}
