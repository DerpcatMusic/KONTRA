//! Universal dynamics (wave 4): one stereo kernel family, configured by
//! `sampler_ir::fx::dynamics::Config`.
//!
//! * `generic`: the textbook kernel (detector, knee, direction, topology,
//!   look-ahead, side-chain high-pass) every format may use.
//! * `classic`, `enhanced`, `pro`: the three Kontakt Compressor cores, each
//!   reproduced from Kontakt 8.13.1 (dispatcher `0x1409047a0`).
//!
//! Every kernel gets the same four live knobs (threshold, ratio, attack,
//! release), re-read once per block, plus the shared stages: stereo link,
//! make-up and wet/dry. Wet/dry is exact against the kernel's own latency:
//! the dry path is the audio the gain is applied to (`d * (1 + mix * (g - 1))`).

mod classic;
mod enhanced;
pub(super) mod generic;
mod hilbert;
mod limiter;
mod pro;

use super::{
    Parameter, Planar, ProcessorState,
    control::{ControlRamp, ControlRange, PreparedParameter},
};
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
                            Detector::Peak => true,
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
        match self.law {
            Law::Generic(g) => lookahead_frames(g.lookahead.seconds(), rate) as u32,
            Law::KontaktPro(_) => pro::LATENCY as u32,
            Law::KontaktLimiter(_) => limiter::LATENCY as u32,
            Law::KontaktHilbert(_) => hilbert::latency(rate) as u32,
            Law::KontaktClassic(_) | Law::KontaktEnhanced(_) => 0,
        }
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
        let input = (input_db / generic::DB_PER_NEPER).exp();
        if !input.is_finite() || input <= 0. {
            return Err(crate::Error::InvalidInput);
        }
        let k = knobs.map(|p| p.bounds()[0]);
        let c = generic::Coefficients::new(&g, k, 48_000);
        let reduction = c.target(input);
        let signed = if g.direction == sampler_ir::fx::dynamics::Direction::Upward {
            -reduction
        } else {
            reduction
        };
        let output = input * (-signed / generic::DB_PER_NEPER).exp();
        Ok((generic::DB_PER_NEPER * output.ln(), signed))
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
        let own = match self.law {
            Law::Generic(g) => lookahead_frames(g.lookahead.seconds(), rate),
            Law::KontaktPro(_) => pro::CELLS,
            Law::KontaktLimiter(_) => limiter::CELLS,
            Law::KontaktHilbert(_) => hilbert::cells(rate),
            Law::KontaktClassic(_) | Law::KontaktEnhanced(_) => 0,
        };
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

fn lookahead_frames(seconds: f64, rate: u32) -> usize {
    (seconds * f64::from(rate)).round() as usize
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
        match self.law {
            Law::Generic(g) => generic::Kernel::new(&g, knobs, self.rate, self.cells)
                .process(&shared, state, cells, block, len),
            Law::KontaktClassic(_) => classic::process(&shared, knobs, self.rate, state, block, len),
            Law::KontaktEnhanced(_) => {
                enhanced::process(&shared, knobs, self.rate, state, block, len)
            }
            Law::KontaktPro(_) => pro::process(&shared, knobs, self.rate, state, cells, block, len),
            Law::KontaktLimiter(_) => {
                limiter::process(&shared, knobs, self.rate, state, cells, block, len)
            }
            Law::KontaktHilbert(_) => {
                hilbert::process(&shared, knobs, self.rate, state, cells, block, len)
            }
        }
    }
}

/// The stages around a law.
pub(super) struct Shared {
    pub link: Link,
    pub makeup: f64,
    pub mix: f64,
}

impl Shared {
    /// Factor on the audio the gain `g` applies to: make-up, then wet/dry.
    #[inline(always)]
    pub fn factor(&self, g: f64) -> f64 {
        let wet = g * self.makeup;
        if self.mix == 1. {
            wet
        } else {
            1. + self.mix * (wet - 1.)
        }
    }
}
