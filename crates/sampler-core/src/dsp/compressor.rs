use super::{Planar, ProcessorState};

/// Feed-forward compressor. DSP_SYSTEM_INVENTORY "Subtype selection and
/// compressor linking": with `link` the detector sees the absolute value of the
/// signed arithmetic channel mean, so equal opposite-polarity channels read
/// zero; without it each channel is detected alone. The spec certifies only
/// that linking property. The level law below is the textbook one: hard knee
/// over `threshold_db`, gain reduction `(level - threshold)(1 - 1/ratio)` dB,
/// one-pole smoothing of the reduction in dB (attack while it grows, release
/// while it falls), then `makeup`.
// ponytail: unverified - threshold/ratio/time laws are not recovered from the
// original; confirm against a Kontakt or Falcon rendering.
#[derive(Clone, Copy, Debug)]
pub struct CompressorSettings {
    pub threshold_db: f64,
    /// Input dB over the threshold per output dB over it; at least 1.
    pub ratio: f64,
    pub attack_seconds: f64,
    pub release_seconds: f64,
    /// Linear gain after the reduction.
    pub makeup: f64,
    pub link: bool,
}

impl CompressorSettings {
    pub(super) fn valid(&self) -> bool {
        self.threshold_db.is_finite()
            && self.ratio.is_finite()
            && self.ratio >= 1.
            && self.attack_seconds.is_finite()
            && self.attack_seconds >= 0.
            && self.release_seconds.is_finite()
            && self.release_seconds >= 0.
            && self.makeup.is_finite()
            && self.makeup >= 0.
    }

    pub(super) fn prepare(self, rate: u32) -> Compressor {
        let coefficient = |seconds: f64| {
            let frames = seconds * f64::from(rate);
            if frames <= 0. {
                0.
            } else {
                (-1. / frames).exp()
            }
        };
        Compressor {
            threshold_db: self.threshold_db,
            threshold: 10f64.powf(self.threshold_db / 20.),
            slope: 1. - 1. / self.ratio,
            attack: coefficient(self.attack_seconds),
            release: coefficient(self.release_seconds),
            makeup: self.makeup,
            link: self.link,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Compressor {
    threshold_db: f64,
    threshold: f64,
    slope: f64,
    attack: f64,
    release: f64,
    makeup: f64,
    link: bool,
}

const DB_PER_NEPER: f64 = 8.685_889_638_065_037;

impl Compressor {
    /// Compress `len` planar frames in place. The smoothed reduction (dB) of
    /// each channel persists in `state.z[channel][0]`.
    pub(super) fn process(&self, state: &mut ProcessorState, block: &mut Planar, len: usize) {
        let mut reduction = [state.z[0][0], state.z[1][0]];
        let [left, right] = block;
        for (l, r) in left[..len].iter_mut().zip(&mut right[..len]) {
            let mean = ((*l + *r) * 0.5).abs();
            let detected = if self.link {
                [mean; 2]
            } else {
                [l.abs(), r.abs()]
            };
            let mut gain = [self.makeup; 2];
            for c in 0..if self.link { 1 } else { 2 } {
                let target = if detected[c] > self.threshold {
                    (DB_PER_NEPER * detected[c].ln() - self.threshold_db) * self.slope
                } else {
                    0.
                };
                let coefficient = if target > reduction[c] {
                    self.attack
                } else {
                    self.release
                };
                reduction[c] = target + coefficient * (reduction[c] - target);
                if reduction[c] < 1e-12 {
                    reduction[c] = 0.;
                } else {
                    gain[c] *= (-reduction[c] / DB_PER_NEPER).exp();
                }
            }
            if self.link {
                (reduction[1], gain[1]) = (reduction[0], gain[0]);
            }
            (*l, *r) = (*l * gain[0], *r * gain[1]);
        }
        (state.z[0][0], state.z[1][0]) = (reduction[0], reduction[1]);
    }
}
