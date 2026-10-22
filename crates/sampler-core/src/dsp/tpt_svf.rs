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
        let d = x - state.s1;
        let bp = d * self.c3 + state.s0 * self.c2;
        let lp = self.c3 * state.s0 + d * self.c4 + state.s1;
        state.s0 = 2. * bp - state.s0;
        state.s1 = 2. * lp - state.s1;
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
}
