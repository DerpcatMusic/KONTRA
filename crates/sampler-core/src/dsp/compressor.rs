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
    /// Off-audio static playback curve: `(output_db, reduction_db)`.
    /// `input_db` is the detector level; reduction is nonnegative, before makeup.
    /// Attack/release history and channel linking are outside this static curve.
    /// Reject invalid settings or a level outside positive finite f64 amplitude.
    /// Zero makeup returns negative infinity for the output level.
    pub fn transfer_db(self, input_db: f64) -> Result<(f64, f64), crate::Error> {
        if !self.valid() || !input_db.is_finite() {
            return Err(crate::Error::InvalidInput);
        }
        let input = (input_db / DB_PER_NEPER).exp();
        if !input.is_finite() || input <= 0. {
            return Err(crate::Error::InvalidInput);
        }
        let kernel = self.prepare(1);
        let mut reduction = kernel.target(input);
        let output = input * kernel.gain(&mut reduction);
        Ok((DB_PER_NEPER * output.ln(), reduction))
    }

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
    #[inline(always)]
    fn target(&self, detected: f64) -> f64 {
        if detected > self.threshold {
            (DB_PER_NEPER * detected.ln() - self.threshold_db) * self.slope
        } else {
            0.
        }
    }

    #[inline(always)]
    fn gain(&self, reduction: &mut f64) -> f64 {
        let mut gain = self.makeup;
        if *reduction < 1e-12 {
            *reduction = 0.;
        } else {
            gain *= (-*reduction / DB_PER_NEPER).exp();
        }
        gain
    }

    pub(crate) fn trace_parameters(&self) -> [(&'static str, f64); 6] {
        [("threshold_db", self.threshold_db), ("ratio", 1. / (1. - self.slope)),
            ("attack_coefficient", self.attack), ("release_coefficient", self.release),
            ("makeup", self.makeup), ("link", f64::from(self.link))]
    }

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
                let target = self.target(detected[c]);
                let coefficient = if target > reduction[c] {
                    self.attack
                } else {
                    self.release
                };
                reduction[c] = target + coefficient * (reduction[c] - target);
                gain[c] = self.gain(&mut reduction[c]);
            }
            if self.link {
                (reduction[1], gain[1]) = (reduction[0], gain[0]);
            }
            (*l, *r) = (*l * gain[0], *r * gain[1]);
        }
        (state.z[0][0], state.z[1][0]) = (reduction[0], reduction[1]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfer_db_matches_audio_curve_bits_on_a_sweep() {
        for rate in [44100, 48000, 96000] {
            for threshold_db in [-60., -24., 0.] {
                for ratio in [1., 1.01, 2., 4., 20.] {
                    for makeup in [0., 0.25, 1., 2.] {
                        for link in [false, true] {
                            let settings = CompressorSettings { threshold_db, ratio, makeup, link,
                                attack_seconds: 0., release_seconds: 0. };
                            let kernel = settings.prepare(rate);
                            let mut state = ProcessorState::default();
                            // Both directions exercise attack/release branch selection.
                            for descending in [false, true] {
                                for step in -480..=144 {
                                    let input_db = if descending { -336 - step } else { step } as f64 * 0.25;
                                    let input = (input_db / DB_PER_NEPER).exp();
                                    let mut block = [[0.; super::super::BLOCK]; 2];
                                    block[0][0] = input; block[1][0] = input;
                                    kernel.process(&mut state, &mut block, 1);
                                    let (output_db, reduction_db) = settings.transfer_db(input_db).unwrap();
                                    for c in 0..2 {
                                        assert_eq!(output_db.to_bits(), (DB_PER_NEPER * block[c][0].ln()).to_bits());
                                        assert_eq!(reduction_db.to_bits(), state.z[c][0].to_bits());
                                    }
                                }
                            }
                            let timed = CompressorSettings { attack_seconds: 0.01, release_seconds: 0.1, ..settings };
                            assert_eq!(timed.transfer_db(-6.).unwrap(), settings.transfer_db(-6.).unwrap());
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn transfer_db_rejects_invalid_settings_and_unrepresentable_levels() {
        let settings = CompressorSettings { threshold_db: -24., ratio: 4., makeup: 1., link: true,
            attack_seconds: 0., release_seconds: 0. };
        for input in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MAX, -f64::MAX] {
            assert!(settings.transfer_db(input).is_err());
        }
        for invalid in [CompressorSettings { ratio: 0.5, ..settings }, CompressorSettings { makeup: -1., ..settings },
            CompressorSettings { attack_seconds: -1., ..settings }, CompressorSettings { threshold_db: f64::NAN, ..settings }] {
            assert!(invalid.transfer_db(0.).is_err());
        }
    }
}
