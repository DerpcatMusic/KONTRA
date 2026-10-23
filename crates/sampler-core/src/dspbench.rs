//! Bench-only hooks for `sampler-dspbench` (feature `dspbench`). The plugin never enables it.
//! Each wrapper drives the engine's own code path, so the bench measures what ships.

use crate::dsp::filter::cascade::{Cascade, CascadeSettings};
use crate::dsp::{BLOCK, Parameter, Planar};

pub use crate::dsp::db_limiter::DbLimiter;
pub use crate::dsp::filter::cascade::Legacy;

/// The shipping Kontakt legacy cascade (`CascadeSettings::compile` + `Cascade::process`), mono.
pub struct LegacyCascade {
    class: Legacy,
    rate: u32,
    unit: Cascade,
    cells: Vec<[f64; 2]>,
    block: Planar,
    at: u64,
}

impl LegacyCascade {
    /// `cutoff` and `resonance` are the normalised knobs (0..1), as in the Kontakt oracle cases.
    pub fn new(class: Legacy, cutoff: f64, resonance: f64, rate: u32) -> Self {
        let unit = compile(class, cutoff, resonance, rate);
        let cells = vec![[0.0; 2]; unit.cells()];
        Self { class, rate, unit, cells, block: [[0.0; BLOCK]; 2], at: 0 }
    }

    /// Move the knobs live: the engine's own control clock slews toward the new targets, and the
    /// stage states are kept (as when a host automates the knob).
    pub fn set_knobs(&mut self, cutoff: f64, resonance: f64) {
        self.unit = compile(self.class, cutoff, resonance, self.rate);
    }

    /// Mono in, left channel out, in `BLOCK`-frame chunks. Returns true if a state went non-finite.
    pub fn process(&mut self, input: &[f32], out: &mut [f32]) -> bool {
        let mut bad = false;
        for (xs, ys) in input.chunks(BLOCK).zip(out.chunks_mut(BLOCK)) {
            let len = xs.len();
            for (dst, &x) in self.block[0].iter_mut().zip(xs) {
                *dst = f64::from(x);
            }
            bad |= self.unit.process(&mut self.cells, &[], &mut self.block, len, self.at, [0.0; 4]);
            for (dst, src) in ys.iter_mut().zip(&self.block[0]) {
                *dst = *src as f32;
            }
            self.at += len as u64;
        }
        bad
    }
}

fn compile(class: Legacy, cutoff: f64, resonance: f64, rate: u32) -> Cascade {
    CascadeSettings { class, cutoff: Parameter::Constant(cutoff), resonance: Parameter::Constant(resonance) }
        .compile(rate, &mut Vec::new())
}

pub use crate::resample::ResampleQuality;

/// The engine's own sample-rate kernel (`resample::Kernel`), mono: `Realtime` is cubic at unity and
/// short sinc when pitched down; `High` uses the long sinc for every non-unity step.
pub struct Resampler {
    kernel: crate::resample::Kernel,
}

impl Resampler {
    pub fn new(quality: ResampleQuality) -> Self {
        Self { kernel: crate::resample::Kernel::new(quality) }
    }

    /// Resample `input` by `step` input frames per output frame (`rate_in / rate_out`).
    pub fn resample(&self, input: &[f32], step: f64) -> Vec<f32> {
        let last = input.len() as i64 - 1;
        let frame = |i: i64| {
            let v = input.get(i.clamp(0, last.max(0)) as usize).copied().unwrap_or(0.0);
            [v, v]
        };
        let outs = ((input.len() as f64 - 1.0) / step).max(0.0) as usize;
        (0..outs)
            .map(|n| {
                let pos = n as f64 * step;
                let base = pos.floor();
                self.kernel.sample(pos - base, step, |o| frame(base as i64 + o))[0]
            })
            .collect()
    }
}
