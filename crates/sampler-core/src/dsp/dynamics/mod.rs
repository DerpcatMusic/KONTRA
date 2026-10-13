//! Universal dynamics (wave 4): ONE engine, configured by
//! `sampler_ir::fx::dynamics::Config`.
//!
//! `pipeline` runs a frame through a chain of stages: input conditioning,
//! detector, gain computer, ballistics and gain-path smoothing, gain map,
//! look-ahead delay, output (make-up, link, wet/dry, clip). `laws` turns each
//! law into a chain once per block (its knob laws and stage choice; the table
//! there lists them); the stage implementations live by role in `detector`,
//! `computer`, `ballistics` and `output`. The generic law is the configurable
//! textbook chain; the native laws are the recovered cores' own stages.
//!
//! Every law gets the same four live knobs (threshold, ratio, attack,
//! release), re-read once per block, plus the shared stages: stereo link,
//! make-up and wet/dry. Wet/dry is exact against the law's own latency:
//! the dry path is the audio the gain is applied to (`d * (1 + mix * (g - 1))`).

mod ballistics;
mod computer;
mod detector;
mod laws;
mod output;
mod pipeline;
#[cfg(test)]
mod tests;

use super::{
    Parameter, Planar, ProcessorState,
    control::{ControlRamp, ControlRange, PreparedParameter},
};
use pipeline::Shared;
use sampler_ir::fx::dynamics::{Detector, Law, Link};

/// A dynamics stage. The knobs are the live parameters; the `law`'s own
/// threshold/ratio fields and `Config` attack/release are superseded by them
/// (the lowering copies them in as constants).
#[derive(Clone, Copy, Debug)]
pub struct DynamicsSettings {
    pub law: Law,
    pub link: Link,
    /// Linear gain after the law.
    pub makeup: f64,
    /// Wet share, 0..=1.
    pub mix: f64,
    /// dB.
    pub threshold: Parameter,
    /// Generic: ratio:1 (at least 1). Kontakt cores: saved 0..=1.
    pub ratio: Parameter,
    pub attack_seconds: Parameter,
    pub release_seconds: Parameter,
}

/// Longest look-ahead the generic kernel keeps.
const MAX_LOOKAHEAD_SECONDS: f64 = 0.1;

impl DynamicsSettings {
    pub(in crate::dsp) fn valid(&self) -> bool {
        let knobs = [
            self.threshold,
            self.ratio,
            self.attack_seconds,
            self.release_seconds,
        ];
        let [attack, release] = [self.attack_seconds, self.release_seconds].map(|p| p.bounds());
        let ratio = self.ratio.bounds();
        knobs
            .iter()
            .all(|p| p.valid() && !matches!(p, Parameter::Expression { .. }))
            && self.threshold.bounds().iter().all(|v| 10f64.powf(v / 20.).is_finite())
            && attack.iter().chain(&release).all(|v| *v >= 0.)
            && self.makeup.is_finite()
            && self.makeup >= 0.
            && (0. ..=1.).contains(&self.mix)
            && match self.law {
                Law::Generic(g) => {
                    ratio.iter().all(|v| *v >= 1.)
                        && g.knee_db.is_finite()
                        && g.knee_db >= 0.
                        && (0. ..=MAX_LOOKAHEAD_SECONDS).contains(&g.lookahead.seconds())
                        && g.sidechain_highpass_hz.is_finite()
                        && g.sidechain_highpass_hz >= 0.
                        && match g.detector {
                            Detector::Peak | Detector::TruePeak => true,
                            Detector::Rms { window } => window.seconds() >= 0.,
                        }
                }
                // Enhanced takes ln(attack ms): zero has no value.
                Law::KontaktEnhanced(_) => {
                    ratio.iter().all(|v| (0. ..=1.).contains(v)) && attack.iter().all(|v| *v > 0.)
                }
                Law::KontaktClassic(_) | Law::KontaktPro(_) => {
                    ratio.iter().all(|v| (0. ..=1.).contains(v))
                }
                // The saved gain is 6.02 dB per octave: 2^(x/6) must stay finite.
                Law::KontaktLimiter(_) => self
                    .threshold
                    .bounds()
                    .iter()
                    .all(|v| (v / 6.).abs() < 100.),
                // Normalized drive and ceiling (the core clamps to 0..=1).
                // Linear drive, ratio 1..=20 and positive time constants.
                Law::KontaktFeedback { .. } => {
                    self.threshold.bounds().iter().all(|v| *v > 0. && v.is_finite())
                        && ratio.iter().all(|v| (1. ..=20.).contains(v))
                        && attack.iter().chain(&release).all(|v| *v > 0.)
                }
                Law::UviMaximizer(m) => [
                    m.threshold_db,
                    m.attack_ms,
                    m.lookahead_ms,
                    m.release_ms,
                    m.ceiling_db,
                    m.knee_db,
                    m.release_blend,
                    m.slew_ms,
                ]
                .iter()
                .all(|v| v.is_finite())
                    && m.lookahead_ms > 0.
                    && m.mode <= 2,
                Law::KontaktBusComp { threshold, makeup, attack, release, ratio, .. } => {
                    [threshold, makeup, attack, release, ratio].iter().all(|v| (0. ..=1.).contains(v))
                }
                Law::KontaktTransient { attack, sustain, gain, .. } => {
                    [attack, sustain, gain].iter().all(|v| (0. ..=1.).contains(v))
                }
                Law::KontaktHilbert(_) => self
                    .threshold
                    .bounds()
                    .iter()
                    .chain(&ratio)
                    .all(|v| (0. ..=1.).contains(v)),
            }
    }

    /// Frames the stage delays the signal by.
    pub fn latency_frames(&self, rate: u32) -> u32 {
        laws::latency(&self.law, rate) as u32
    }

    /// Off-audio static transfer of the generic law at a steady `input_db`:
    /// `(output_db, reduction_db)` before make-up; reduction is signed by
    /// direction (positive lowers, negative raises). Live knobs, native laws
    /// and invalid settings are rejected.
    pub fn transfer_db(&self, input_db: f64) -> Result<(f64, f64), crate::Error> {
        let Law::Generic(g) = self.law else {
            return Err(crate::Error::InvalidInput);
        };
        let knobs = [self.threshold, self.ratio, self.attack_seconds, self.release_seconds];
        if !self.valid()
            || !input_db.is_finite()
            || !knobs.iter().all(|p| matches!(p, Parameter::Constant(_)))
        {
            return Err(crate::Error::InvalidInput);
        }
        let input = (input_db / computer::DB_PER_NEPER).exp();
        if !input.is_finite() || input <= 0. {
            return Err(crate::Error::InvalidInput);
        }
        let k = knobs.map(|p| p.bounds()[0]);
        let reduction = laws::generic_ratio(&g, k, 1).target(input);
        let signed = if g.direction == sampler_ir::fx::dynamics::Direction::Upward {
            -reduction
        } else {
            reduction
        };
        let output = input * (-signed / computer::DB_PER_NEPER).exp();
        Ok((computer::DB_PER_NEPER * output.ln(), signed))
    }

    pub(in crate::dsp) fn prepare(
        self,
        rate: u32,
        bindings: &mut Vec<ControlRange>,
        cells: &mut usize,
    ) -> Result<Dynamics, crate::Error> {
        let parameters = [
            self.threshold,
            self.ratio,
            self.attack_seconds,
            self.release_seconds,
        ];
        let constant = parameters
            .iter()
            .all(|p| matches!(p, Parameter::Constant(_)))
            .then(|| parameters.map(|p| p.bounds()[0]));
        let offset = *cells;
        let own = laws::cells(&self.law, rate).ok_or(crate::Error::InvalidInput)?;
        *cells = offset.checked_add(own).ok_or(crate::Error::Capacity)?;
        Ok(Dynamics {
            law: self.law,
            link: self.link,
            parameters: parameters.map(|p| p.compile(bindings)),
            constant,
            rate,
            makeup: self.makeup,
            mix: self.mix,
            offset,
            cells: own,
        })
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Dynamics {
    law: Law,
    link: Link,
    parameters: [PreparedParameter; 4],
    /// The four knobs when none is live.
    constant: Option<[f64; 4]>,
    rate: u32,
    makeup: f64,
    mix: f64,
    /// First cell of this stage in the slab, and how many it owns.
    offset: usize,
    cells: usize,
}

impl Dynamics {
    /// Cells of the per-voice (or per-bus) slab this stage owns.
    pub(crate) fn cells(&self) -> std::ops::Range<usize> {
        self.offset..self.offset + self.cells
    }

    pub(crate) fn trace_parameters(&self) -> [(&'static str, PreparedParameter); 8] {
        let kind = match self.law {
            Law::Generic(_) => 0.,
            Law::KontaktClassic(_) => 1.,
            Law::KontaktEnhanced(_) => 2.,
            Law::KontaktPro(_) => 3.,
            Law::KontaktLimiter(_) => 4.,
            Law::KontaktHilbert(_) => 5.,
            Law::KontaktFeedback { .. } => 6.,
            Law::KontaktTransient { .. } => 7.,
            Law::KontaktBusComp { .. } => 8.,
            Law::UviMaximizer(_) => 9.,
        };
        [
            ("threshold_db", self.parameters[0]),
            ("ratio", self.parameters[1]),
            ("attack_seconds", self.parameters[2]),
            ("release_seconds", self.parameters[3]),
            ("makeup", PreparedParameter::Constant(self.makeup)),
            ("mix", PreparedParameter::Constant(self.mix)),
            ("link", PreparedParameter::Constant(self.link as u8 as f64)),
            ("law", PreparedParameter::Constant(kind)),
        ]
    }

    /// Process `len` planar frames in place. `cells` is this stage's slice of
    /// the slab (`self.cells()`).
    pub(super) fn process(
        &self,
        state: &mut ProcessorState,
        cells: &mut [[f64; 2]],
        parameters: &[ControlRamp],
        block: &mut Planar,
        len: usize,
        at: u64,
    ) {
        if len == 0 {
            return;
        }
        let knobs = self
            .constant
            .unwrap_or_else(|| self.parameters.map(|p| p.value(parameters, at, None)));
        let shared = Shared {
            link: self.link,
            makeup: self.makeup,
            mix: self.mix,
        };
        laws::process(self.law, &shared, knobs, self.rate, state, cells, block, len);
    }
}
