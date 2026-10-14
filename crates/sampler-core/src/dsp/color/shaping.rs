//! Falcon Workstation kernels recovered from original bytes:
//! DSP_FORMAT_SPECIFICATION "WaveShaper rectifier kernels" and "Formant Crusher
//! fractional decimation". Only these sub-kernels are specified; the effects'
//! gains, filters and mixing around them are separate work.
use super::super::Planar;

/// Port from v1 0cb7a8a0:src/fx/blocks.rs, Classic Saturation only.
pub(crate) fn saturation(x: f64, shape: f32) -> f64 {
    let x = x as f32;
    let y = if shape.abs() <= 0.0001 {
        x
    } else if shape < 0. {
        let q = 0.975 * (4. + 4. * shape) + 0.1;
        (x * x + q) * x * (1. / (1. + q))
    } else {
        let amount = 4. * shape;
        let magnitude = (if amount >= 1. { amount * x } else { x }).abs().min(1.);
        let bent = (2. * magnitude - magnitude * magnitude).copysign(x);
        if amount >= 1. {
            bent
        } else {
            (1. - amount) * x + amount * bent
        }
    };
    f64::from(y)
}

/// WaveShaper's two rectifier branches (internal modes 6 and 7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rectifier {
    /// Clears the sign bit: `|x|`, negative zero becomes zero.
    Full,
    /// `max(x, +0)`: negatives, signed zeros and NaNs become positive zero.
    Half,
}

impl Rectifier {
    pub(crate) fn apply(self, x: f64) -> f64 {
        match self {
            Self::Full => x.abs(),
            Self::Half => {
                if x > 0. {
                    x
                } else {
                    0.
                }
            }
        }
    }
}

/// Formant Crusher's decimator (0x1412d43c0): a held input sampled every `period` frames
/// (fractional, never rounded) with a linear ramp toward it, mixed by `blend` (1 holds, 0
/// follows the ramp); f32 values, f64 phase, as the native.
#[derive(Clone, Copy, Debug)]
pub(super) struct Decimate {
    period: f64,
    blend: f32,
}

impl Decimate {
    pub(super) fn new(d: sampler_ir::fx::color::Decimator, rate: u32) -> Self {
        let fs = f64::from(rate as f32);
        Self { period: 1.0 / f64::from((d.hertz[1] / fs + d.hertz[0] / fs) as f32), blend: d.blend }
    }

    pub(super) fn period(&self) -> f64 {
        self.period
    }

    pub(super) fn blend(&self) -> f32 {
        self.blend
    }

    /// Per channel `z[c] = [current, held]` and `delta`; one phase per channel too, though
    /// both channels advance it alike.
    pub(super) fn process(&self, state: &mut DecimatorState, block: &mut Planar, len: usize) {
        let period = self.period as f32;
        for c in 0..2 {
            let mut phase = state.phase;
            let [mut current, mut held] = state.z[c];
            for v in &mut block[c][..len] {
                if phase <= 0.0 {
                    held = *v as f32;
                    state.delta[c] = (held - current) / period;
                    phase += self.period;
                }
                let old = current;
                current = old + state.delta[c];
                *v = f64::from(((held - old) * self.blend) + old);
                phase -= 1.0;
            }
            state.z[c] = [current, held];
            if c == 1 {
                state.phase = phase;
            }
        }
    }
}

/// [`Decimate`] history.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct DecimatorState {
    pub(super) z: [[f32; 2]; 2],
    pub(super) delta: [f32; 2],
    pub(super) phase: f64,
}

impl DecimatorState {
    pub(super) fn finite(&self) -> bool {
        self.z.iter().flatten().chain(&self.delta).all(|v| v.is_finite()) && self.phase.is_finite()
    }
}

/// The color chain's RBJ biquad, transposed direct form II in f32 (UVI 0x141646fa0); the
/// design is the filter family's (`cascade::rbj`).
#[derive(Clone, Copy, Debug)]
pub(super) struct Biquad {
    c: [f32; 5],
    pub(super) post: bool,
}

impl Biquad {
    pub(super) fn new(b: sampler_ir::fx::color::Biquad, rate: u32) -> Self {
        let c = super::super::filter::cascade::rbj(b.mode.min(2), b.hertz / f64::from(rate as f32), b.q);
        Self { c: c.map(|v| v as f32), post: b.post }
    }

    pub(super) fn process(&self, state: &mut [[f32; 2]; 2], block: &mut Planar, len: usize) {
        let [b0, b1, b2, a1, a2] = self.c;
        for (s, channel) in state.iter_mut().zip(block.iter_mut()) {
            for v in &mut channel[..len] {
                let x = *v as f32;
                let y = (x * b0) + s[0];
                s[0] = ((x * b1) + s[1]) - (y * a1);
                s[1] = (x * b2) - (y * a2);
                *v = f64::from(y);
            }
        }
    }
}

#[cfg(test)]
mod saturation_tests {
    use super::saturation;

    #[test]
    fn classic_saturation_matches_v1_measured_vectors() {
        // Independent decimal transfer vectors retained from v1's native-law test.
        let (mut error, mut signal) = (0., 0.);
        for (shape, input, expected) in [
            (-1., 0.5, 0.1590909090909091),
            (-0.5, 0.25, 0.17315573770491804),
            (0., -4., -4.),
            (0.0001, 0.01, 0.01),
            (0.125, 0.25, 0.34375),
            (0.25, 0.5, 0.75),
            (0.5, -0.25, -0.75),
            (1., 0.01, 0.0784),
            (1., 4., 1.),
        ] {
            let actual = saturation(input, shape);
            error += (actual - expected).powi(2);
            signal += expected * expected;
            assert!(
                (actual - expected).abs() < 2e-7,
                "shape={shape}, x={input}: {actual} != {expected}"
            );
        }
        let null_db = 10. * (error / signal).log10();
        assert!(null_db < -130.);
        println!("Classic Saturation decimal-vector null: {null_db:.2} dB");
    }
}
