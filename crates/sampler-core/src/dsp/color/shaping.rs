//! Falcon Workstation kernels recovered from original bytes:
//! DSP_FORMAT_SPECIFICATION "WaveShaper rectifier kernels" and "Formant Crusher
//! fractional decimation". Only these sub-kernels are specified; the effects'
//! gains, filters and mixing around them are separate work.
use super::{Planar, ProcessorState};

/// Port from v1 0cb7a8a0:src/fx/blocks.rs, Classic Saturation only.
pub(super) fn saturation(x: f64, shape: f32) -> f64 {
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
    pub(super) fn apply(self, x: f64) -> f64 {
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

/// Formant Crusher's decimator: a held input sampled every `period` frames
/// (fractional, never rounded) with a linear ramp toward it, mixed by `blend`
/// (1 holds, 0 follows the ramp).
#[derive(Clone, Copy, Debug)]
pub struct Decimator {
    pub period: f64,
    pub blend: f64,
}

impl Decimator {
    pub(super) fn valid(&self) -> bool {
        self.period.is_finite() && self.period >= 1. && self.blend.is_finite()
    }

    /// Per channel `z[c] = [current, held]`, `aux = [delta left, delta right,
    /// phase]`. Both channels share one phase: it only depends on the period.
    pub(super) fn process(&self, state: &mut ProcessorState, block: &mut Planar, len: usize) {
        let mut phase = state.aux[2];
        for i in 0..len {
            let reload = phase <= 0.;
            for c in 0..2 {
                let [mut current, mut held] = state.z[c];
                if reload {
                    held = block[c][i];
                    state.aux[c] = (held - current) / self.period;
                }
                let old = current;
                current += state.aux[c];
                block[c][i] = (held - old) * self.blend + old;
                state.z[c] = [current, held];
            }
            if reload {
                phase += self.period;
            }
            phase -= 1.;
        }
        state.aux[2] = phase;
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
