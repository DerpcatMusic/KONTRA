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

impl Compressor {
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
                let target = if detected[c] > cfs.threshold {
                    (DB_PER_NEPER * detected[c].ln() - cfs.threshold_db) * cfs.slope
                } else {
                    0.
                };
                let coefficient = if target > reduction[c] {
                    cfs.attack
                } else {
                    cfs.release
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
