//! Falcon Workstation kernels recovered from original bytes:
//! DSP_FORMAT_SPECIFICATION "WaveShaper rectifier kernels" and "Formant Crusher
//! fractional decimation". Only these sub-kernels are specified; the effects'
//! gains, filters and mixing around them are separate work.
use super::{Planar, ProcessorState};

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
