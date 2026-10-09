use super::{
    Parameter, Planar, ProcessorState,
    control::{ControlRamp, ControlRange, PreparedParameter},
};

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
    pub threshold_db: Parameter,
    /// Input dB over the threshold per output dB over it; at least 1.
    pub ratio: Parameter,
    pub attack_seconds: Parameter,
    pub release_seconds: Parameter,
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
    /// Resolve live controls to constants before evaluating this static curve.
    pub fn transfer_db(self, input_db: f64) -> Result<(f64, f64), crate::Error> {
        if !self.valid() || !input_db.is_finite()
            || ![self.threshold_db, self.ratio, self.attack_seconds, self.release_seconds]
                .iter().all(|p| matches!(p, Parameter::Constant(_))) {
            return Err(crate::Error::InvalidInput);
        }
        let input = (input_db / DB_PER_NEPER).exp();
        if !input.is_finite() || input <= 0. {
            return Err(crate::Error::InvalidInput);
        }
        let kernel = self.prepare(1, &mut Vec::new());
        let mut reduction = kernel.constant.unwrap().target(input);
        let output = input * kernel.gain(&mut reduction);
        Ok((DB_PER_NEPER * output.ln(), reduction))
    }

    pub(super) fn valid(&self) -> bool {
        [
            self.threshold_db,
            self.ratio,
            self.attack_seconds,
            self.release_seconds,
        ]
        .iter()
        .all(|p| p.valid() && !matches!(p, Parameter::Expression { .. }))
            && self
                .threshold_db
                .bounds()
                .iter()
                .all(|v| 10f64.powf(v / 20.).is_finite())
            && self.ratio.bounds().iter().all(|v| *v >= 1.)
            && [self.attack_seconds, self.release_seconds]
                .iter()
                .all(|p| p.bounds().iter().all(|v| *v >= 0.))
            && self.makeup.is_finite()
            && self.makeup >= 0.
    }

    pub(super) fn prepare(self, rate: u32, bindings: &mut Vec<ControlRange>) -> Compressor {
        let parameters = [
            self.threshold_db,
            self.ratio,
            self.attack_seconds,
            self.release_seconds,
        ];
        let constant = parameters
            .iter()
            .all(|p| matches!(p, Parameter::Constant(_)))
            .then(|| coefficients(parameters.map(|p| p.bounds()[0]), rate));
        Compressor {
            parameters: parameters.map(|p| p.compile(bindings)),
            rate,
            constant,
            makeup: self.makeup,
            link: self.link,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Compressor {
    parameters: [PreparedParameter; 4],
    rate: u32,
    constant: Option<Coefficients>,
    makeup: f64,
    link: bool,
}

#[derive(Clone, Copy)]
struct Coefficients {
    threshold_db: f64,
    threshold: f64,
    slope: f64,
    attack: f64,
    release: f64,
}

fn coefficients([threshold_db, ratio, attack, release]: [f64; 4], rate: u32) -> Coefficients {
    let coefficient = |seconds: f64| {
        let frames = seconds * f64::from(rate);
        if frames <= 0. {
            0.
        } else {
            (-1. / frames).exp()
        }
    };
    Coefficients {
        threshold_db,
        threshold: 10f64.powf(threshold_db / 20.),
        slope: 1. - 1. / ratio,
        attack: coefficient(attack),
        release: coefficient(release),
    }
}

const DB_PER_NEPER: f64 = 8.685_889_638_065_037;

impl Coefficients {
    #[inline(always)]
    fn target(&self, detected: f64) -> f64 {
        if detected > self.threshold {
            (DB_PER_NEPER * detected.ln() - self.threshold_db) * self.slope
        } else {
            0.
        }
    }
}

impl Compressor {
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

    pub(crate) fn trace_parameters(&self) -> [(&'static str, PreparedParameter); 6] {
        [
            ("threshold_db", self.parameters[0]),
            ("ratio", self.parameters[1]),
            ("attack_seconds", self.parameters[2]),
            ("release_seconds", self.parameters[3]),
            ("makeup", PreparedParameter::Constant(self.makeup)),
            ("link", PreparedParameter::Constant(f64::from(self.link))),
        ]
    }

    fn cached(&self, state: &mut ProcessorState, values: [f64; 4]) -> Coefficients {
        if state.aux[9] == 0. || state.aux[..4] != values {
            let c = coefficients(values, self.rate);
            state.aux[..4].copy_from_slice(&values);
            state.aux[4..9].copy_from_slice(&[
                c.threshold_db,
                c.threshold,
                c.slope,
                c.attack,
                c.release,
            ]);
            state.aux[9] = 1.;
        }
        Coefficients {
            threshold_db: state.aux[4],
            threshold: state.aux[5],
            slope: state.aux[6],
            attack: state.aux[7],
            release: state.aux[8],
        }
    }

    /// Compress `len` planar frames in place. The smoothed reduction (dB) of
    /// each channel persists in `state.z[channel][0]`.
    pub(super) fn process(
        &self,
        state: &mut ProcessorState,
        parameters: &[ControlRamp],
        block: &mut Planar,
        len: usize,
        at: u64,
    ) {
        if len == 0 {
            return;
        }
        let held = self.constant.or_else(|| {
            let start = self.parameters.map(|p| p.value(parameters, at, None));
            let end = self
                .parameters
                .map(|p| p.value(parameters, at + len as u64 - 1, None));
            (start == end).then(|| self.cached(state, start))
        });
        let mut reduction = [state.z[0][0], state.z[1][0]];
        let [left, right] = block;
        for (i, (l, r)) in left[..len].iter_mut().zip(&mut right[..len]).enumerate() {
            let cfs = held.unwrap_or_else(|| {
                self.cached(
                    state,
                    self.parameters
                        .map(|p| p.value(parameters, at + i as u64, None)),
                )
            });
            let mean = ((*l + *r) * 0.5).abs();
            let detected = if self.link {
                [mean; 2]
            } else {
                [l.abs(), r.abs()]
            };
            let mut gain = [self.makeup; 2];
            for c in 0..if self.link { 1 } else { 2 } {
                let target = cfs.target(detected[c]);
                let coefficient = if target > reduction[c] {
                    cfs.attack
                } else {
                    cfs.release
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
                            let settings = CompressorSettings { threshold_db: Parameter::Constant(threshold_db), ratio: Parameter::Constant(ratio), makeup, link,
                                attack_seconds: Parameter::Constant(0.), release_seconds: Parameter::Constant(0.) };
                            let kernel = settings.prepare(rate, &mut Vec::new());
                            let mut state = ProcessorState::default();
                            // Both directions exercise attack/release branch selection.
                            for descending in [false, true] {
                                for step in -480..=144 {
                                    let input_db = if descending { -336 - step } else { step } as f64 * 0.25;
                                    let input = (input_db / DB_PER_NEPER).exp();
                                    let mut block = [[0.; super::super::BLOCK]; 2];
                                    block[0][0] = input; block[1][0] = input;
                                    kernel.process(&mut state, &[], &mut block, 1, 0);
                                    let (output_db, reduction_db) = settings.transfer_db(input_db).unwrap();
                                    for c in 0..2 {
                                        assert_eq!(output_db.to_bits(), (DB_PER_NEPER * block[c][0].ln()).to_bits());
                                        assert_eq!(reduction_db.to_bits(), state.z[c][0].to_bits());
                                    }
                                }
                            }
                            let timed = CompressorSettings { attack_seconds: Parameter::Constant(0.01), release_seconds: Parameter::Constant(0.1), ..settings };
                            assert_eq!(timed.transfer_db(-6.).unwrap(), settings.transfer_db(-6.).unwrap());
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn transfer_db_rejects_invalid_settings_and_unrepresentable_levels() {
        let settings = CompressorSettings { threshold_db: Parameter::Constant(-24.), ratio: Parameter::Constant(4.), makeup: 1., link: true,
            attack_seconds: Parameter::Constant(0.), release_seconds: Parameter::Constant(0.) };
        let live = CompressorSettings { threshold_db: Parameter::Control(ControlRange {
            control: crate::ControlId(1), low: -60., high: 0., ramp_frames: 0,
        }), ..settings };
        assert!(live.valid());
        assert!(live.transfer_db(0.).is_err(), "a live binding has no resolved detector threshold");
        for input in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MAX, -f64::MAX] {
            assert!(settings.transfer_db(input).is_err());
        }
        for invalid in [CompressorSettings { ratio: Parameter::Constant(0.5), ..settings }, CompressorSettings { makeup: -1., ..settings },
            CompressorSettings { attack_seconds: Parameter::Constant(-1.), ..settings }, CompressorSettings { threshold_db: Parameter::Constant(f64::NAN), ..settings }] {
            assert!(invalid.transfer_db(0.).is_err());
        }
    }
}
