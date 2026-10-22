//! Trapezoidal (TPT) state-variable sections in the Kontakt 8 coefficient layout.
//!
//! Decompile: section step `140d947a0`, lowpass-type setup `1419529e0` + `141abcf90`,
//! highpass-type setup `141abe6a0` and the default damping written in `141ac97e0`.
//! Labels: the section algebra, the tangent prewarp and both damping values are CONFIRMED
//! (arithmetic read from the decompile). Naming (lowpass, highpass, cut) is INFERRED from the
//! sweep shapes. Generic: no product names. Glides, the enable-state object and the
//! per-control snap logic are not here; a cut is a static pair of sections.
use sampler_ir::fx::timemod::Cut as CutSpec;

/// Lowpass-type `Q` on the warped cutoff (f32 `1479ab25c` = 0.7498942).
const LOWPASS_Q: f32 = f32::from_bits(0x3f3f_f911);
/// Lowpass-type damping `R` (`141abcf90`, bit pattern `14473775c` = 5/3).
const LOWPASS_R: f32 = f32::from_bits(0x3fd5_5555);
/// Highpass-type damping `R` (`141ac97e0` writes 2.0 to `F+0xbc`).
const HIGHPASS_R: f32 = 2.;
/// Lowpass-type output mix words (`141abe820`, f32 bit patterns): `a x + b bp + c lp`, with
/// a = 10^-0.5 and c = 1 - a. At DC the mix is `a + c = 1`, so a lowpass passes DC at unity.
const LOWPASS_MIX: [f32; 3] = [f32::from_bits(0x3ea1_e89b), f32::from_bits(0x3ed2_0453), f32::from_bits(0x3f2f_0bb2)];

/// Tangent of `w = pi hz / fs` to fifth order: `w + w^3/3 + 2 w^5/15` (`1419529e0`, `141abe6a0`).
pub fn prewarp(hz: f32, fs: f32) -> f32 {
    let w = (f64::from(hz) * std::f64::consts::PI / f64::from(fs)) as f32;
    let w3 = w * w * w;
    w + w3 / 3. + 2. / 15. * w3 * w * w
}

/// Coefficients of one TPT section: warped cutoff `g`, damping `r`.
/// `c2 = 1 / (g^2 + r g + 1)`, `c3 = g c2`, `c4 = g c3` (CONFIRMED, `1419529e0`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Section {
    c2: f32,
    c3: f32,
    c4: f32,
    r: f32,
}

/// The two trapezoidal integrators of one section for one channel.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct State {
    pub s0: f32,
    pub s1: f32,
}

impl Section {
    pub fn new(g: f32, r: f32) -> Self {
        let c2 = 1. / ((g + r) * g + 1.);
        let c3 = g * c2;
        Self { c2, c3, c4: g * c3, r }
    }

    /// A section from the three coefficients a native object stores (`140d947a0` reads only these; `r` is
    /// not part of the object, so it is 0 here).
    pub fn from_coefficients(c2: f32, c3: f32, c4: f32) -> Self {
        Self { c2, c3, c4, r: 0. }
    }

    /// A section from the coefficients and the damping `r` (the object's `+0x2c`) of a native object.
    pub fn from_native(c2: f32, c3: f32, c4: f32, r: f32) -> Self {
        Self { c2, c3, c4, r }
    }

    /// `(c2, c3, c4, r)` as the native object holds them, for checks against native state.
    pub fn parts(&self) -> (f32, f32, f32, f32) {
        (self.c2, self.c3, self.c4, self.r)
    }

    /// Lowpass-type section at `hz` (Kontakt: g = prewarp x Q, R = 5/3).
    pub fn lowpass(hz: f32, fs: f32) -> Self {
        Self::new(prewarp(hz, fs) * LOWPASS_Q, LOWPASS_R)
    }

    /// Highpass-type section at `hz` (Kontakt: g = prewarp, R = 2).
    pub fn highpass(hz: f32, fs: f32) -> Self {
        Self::new(prewarp(hz, fs), HIGHPASS_R)
    }

    /// One sample (`140d947a0`): returns the bandpass `bp` and lowpass `lp` of the state before
    /// the step, then advances both integrators by the trapezoidal rule.
    #[inline(always)]
    pub fn tick(&self, state: &mut State, x: f32) -> (f32, f32) {
        // The native operation order (the rounding of the state updates is part of the vectors): -0 enters as +0,
        // s0' = ((2d c3 + X) + X) - s0 and s1' = ((((2 s0) c3 + s1) + s1 + Y) + Y) - s1 with X = s0 c2, Y = d c4.
        let x = if x == 0. { 0. } else { x };
        let (s0, s1) = (state.s0, state.s1);
        let d = x - s1;
        let xc = s0 * self.c2;
        let yc = d * self.c4;
        let bp = d * self.c3 + xc;
        let lp = self.c3 * s0 + yc + s1;
        state.s0 = ((d + d) * self.c3 + xc + xc) - s0;
        state.s1 = ((((s0 + s0) * self.c3 + s1 + s1) + yc) + yc) - s1;
        (bp, lp)
    }
}

/// A cut pair in series: an optional lowpass-type section, then an optional highpass-type
/// section (`141aa3ec0` order). Each stage is bypassed when absent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cut {
    lowpass: Option<Section>,
    highpass: Option<Section>,
}

/// Per-channel states of a [`Cut`]: `[lowpass, highpass]`.
pub type CutState = [State; 2];

impl Cut {
    pub fn new(spec: CutSpec, fs: f32) -> Self {
        Self {
            lowpass: spec.lowpass_hz.map(|hz| Section::lowpass(hz, fs)),
            highpass: spec.highpass_hz.map(|hz| Section::highpass(hz, fs)),
        }
    }

    /// One sample through the cut. `state` is the channel's pair of section states.
    #[inline(always)]
    pub fn tick(&self, state: &mut CutState, x: f32) -> f32 {
        let mut y = x;
        if let Some(lp) = &self.lowpass {
            let (bp, lo) = lp.tick(&mut state[0], x);
            y = LOWPASS_MIX[0] * x + LOWPASS_MIX[1] * bp + LOWPASS_MIX[2] * lo;
        }
        if let Some(hp) = &self.highpass {
            let (bp, lo) = hp.tick(&mut state[1], y);
            y = y - hp.r * bp - lo;
        }
        y
    }
}

/// Exponent base of the saturator polynomial: `10^(0.05 p)` (f32 `1447148c0` = 0.05).
const SAT_K: f32 = 0.05;
/// Exponents `p2..p6` of `141abe910`, passed by slot 1 (`141a9e300`): (0, -40, -45, -49, -40).
const SAT_EXPONENTS: [f32; 5] = [0., -40., -45., -49., -40.];
/// Damping R of both saturator sections (`141ac9c50` writes 2.0 when `144714994` is nonzero).
const SATURATOR_R: f32 = 2.;

/// Saturator (`141aa44a0`). Section B (2205 Hz) takes `tanh(d x)`; its out6 `f` feeds the polynomial
/// `P(f) = c4 f^2 + c8 f^3 + c0c f^4 + c10 f^5`, which drives section A (5 Hz). Both sections have
/// R = 2 and Q = 1 (`141ac9c50`). Output `g (tanh(d x) + P - 2 A.out5 - A.out6)`, with c14 = c18 = 1,
/// `d = 10^a`, `g = 10^(-a/2)`. CONFIRMED arithmetic. Wired on the Replika line input (`dsp/timemod`,
/// `Line::saturation`), path 0 of `0141aa0000.c` 5452-5474.
/// ponytail: `powf` per sample for `d` and `g`; the decompile refreshes them only when the amount moves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Saturator {
    /// `c4, c8, c0c, c10` of `P(f)`, each divided by the raw base (`141abe910`).
    poly: [f32; 4],
    pre: Section,
    post: Section,
}

/// Per-channel states of a [`Saturator`]: `[section B, section A]`.
pub type SaturatorState = [State; 2];

impl Saturator {
    pub fn new(fs: f32) -> Self {
        let [p2, p3, p4, p5, p6] = SAT_EXPONENTS.map(|e| 10f32.powf(e * SAT_K));
        let base = p2 + 3. * p4 + 5. * p6;
        Self {
            poly: [
                (2. * p3 + 8. * p5) / base,
                (-4. * p4 - 20. * p6) / base,
                -8. * p5 / base,
                16. * p6 / base,
            ],
            pre: Section::new(prewarp(2205., fs), SATURATOR_R),
            post: Section::new(prewarp(5., fs), SATURATOR_R),
        }
    }

    /// One sample at saturation amount `a` (0..1).
    pub fn tick(&self, state: &mut SaturatorState, x: f32, a: f32) -> f32 {
        let d = 10f32.powf(a);
        let g = 10f32.powf(-0.5 * a);
        let drive = (d * x).tanh();
        let (_, f) = self.pre.tick(&mut state[0], drive);
        let [c4, c8, c0c, c10] = self.poly;
        let p = (((c10 * f + c0c) * f + c8) * f + c4) * f * f;
        let (a5, a6) = self.post.tick(&mut state[1], p);
        g * (drive + p - self.post.r * a5 - a6)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sampler_ir::fx::timemod::{linear_sweep_hz, log_sweep_hz};

    const FS: f32 = 48_000.;

    /// Decompile constants: LOWCUT (index 2) is `10^(2v + 1)` Hz, a highpass 10 Hz to 1 kHz on a
    /// log law (f32 `144714a00` = 10, `141abe6a0`). HIGHCUT (index 3) is `200 + 19800 v` Hz,
    /// a lowpass 200 Hz to 20 kHz on a linear law (f32 `144f38ce4` = 200, f64 `1479ab650` = 19800).
    #[test]
    fn svf_cutoff_laws_hit_the_decompiled_endpoints() {
        let close = |a: f32, b: f32| (a - b).abs() <= 1e-3 * b;
        assert!(close(log_sweep_hz(0., 10., 1000.), 10.));
        assert!(close(log_sweep_hz(1., 10., 1000.), 1000.));
        assert!(close(log_sweep_hz(0.5, 10., 1000.), 100.), "log, not linear: midpoint is 10^(2 x 0.5 + 1)");
        assert!(close(linear_sweep_hz(0., 200., 20_000.), 200.));
        assert!(close(linear_sweep_hz(1., 200., 20_000.), 20_000.));
        assert!(close(linear_sweep_hz(0.5, 200., 20_000.), 10_100.));
    }

    #[test]
    fn svf_highpass_of_dc_decays_and_lowpass_passes_dc() {
        let dc = 1.;
        let ticks = 2 * FS as usize;
        let high = Cut::new(CutSpec { highpass_hz: Some(10.), lowpass_hz: None }, FS);
        let mut hs: CutState = Default::default();
        let mut y = 0.;
        for _ in 0..ticks {
            y = high.tick(&mut hs, dc);
        }
        assert!(y.abs() < 1e-3, "highpass of DC must decay to 0, got {y}");

        let low = Cut::new(CutSpec { highpass_hz: None, lowpass_hz: Some(200.) }, FS);
        let mut ls: CutState = Default::default();
        for _ in 0..ticks {
            y = low.tick(&mut ls, dc);
        }
        assert!((y - 1.).abs() < 1e-3, "lowpass must pass DC at unity, got {y}");
    }

    /// Literal values from the decoded exponents (`141abe910`, constants 1447148c0 = 0.05,
    /// 1447149d0 = 3, 1447149f0 = 5, 1447149f8 = 8, 144f38e04 = -4, 14471cb14 = 20, 144fc4d20 = -8,
    /// 144714a10 = 16, exponents -40, -45, -49, -40): base 1.0668702, normalised by the base.
    #[test]
    fn saturator_polynomial_is_the_normalised_exponent_sum() {
        let s = Saturator::new(FS);
        let want = [0.0453523, -0.2085480, -0.0266059, 0.1499714];
        for (got, want) in s.poly.iter().zip(want) {
            assert!((got - want).abs() < 1e-5, "{got} vs {want}");
        }
    }

    /// Both saturator sections: R = 2 (`141ac9c50`), Q = 1, cutoffs 2205 Hz and 5 Hz (`1479ab60c`,
    /// `1447149f0`). `c3 / c2` is the warped cutoff g.
    #[test]
    fn saturator_sections_are_r_two_at_their_decoded_cutoffs() {
        let s = Saturator::new(FS);
        assert_eq!((s.pre.r, s.post.r), (2., 2.));
        let w = |hz: f64| {
            let w = hz * std::f64::consts::PI / f64::from(FS);
            (w + w * w * w / 3. + 2. * w.powi(5) / 15.) as f32
        };
        assert!((s.pre.c3 / s.pre.c2 - w(2205.)).abs() < 1e-5);
        assert!((s.post.c3 / s.post.c2 - w(5.)).abs() < 1e-7);
    }

    /// At DC the lowpass sections pass their input (bp -> 0, lp -> input), so P cancels against
    /// `2 A.out5 + A.out6` and the saturator settles to `g tanh(d x)`, with d = 10^a, g = 10^(-a/2).
    #[test]
    fn saturator_dc_output_is_g_times_tanh_of_the_drive() {
        let s = Saturator::new(FS);
        for a in [0.0f32, 0.5, 1.0] {
            let mut st: SaturatorState = Default::default();
            let mut y = 0.;
            for _ in 0..200_000 {
                y = s.tick(&mut st, 1e-3, a);
            }
            let want = 10f32.powf(-0.5 * a) * (10f32.powf(a) * 1e-3).tanh();
            assert!((y - want).abs() <= 1e-3 * want, "a={a}: {y} vs {want}");
        }
    }

    /// Full amount (a = 1): drive d = 10^1 = 10 and gain g = 10^-0.5 (`141aa44a0` with `141accd10`),
    /// so the DC limit at x = 0.1 is 10^-0.5 tanh(10 x) = 10^-0.5 tanh(1).
    #[test]
    fn replika_saturator_full_amount_is_ten_and_minus_half_power_of_drive() {
        let s = Saturator::new(FS);
        let mut st: SaturatorState = Default::default();
        let mut y = 0.;
        for _ in 0..200_000 {
            y = s.tick(&mut st, 0.1, 1.);
        }
        let want = 10f32.powf(-0.5) * 1f32.tanh();
        assert!((y - want).abs() <= 1e-3 * want, "{y} vs {want}");
    }
}
