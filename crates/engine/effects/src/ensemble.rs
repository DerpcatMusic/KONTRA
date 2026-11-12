//! [`ir::Ensemble`] on the public [`ModDelay`]: one line per voice, summed
//! with the input's direct shares, an optional scan fading the voices in
//! turn. It never allocates once built.

use kontra_dsp::delay::{ModDelay, Settings};
use kontra_ir as ir;

use crate::processors::Wet;

/// Frames per pass through the voices (a stack scratch); the scan's gains
/// step once a pass.
const CHUNK: usize = 64;

/// The largest loop gain a voice runs at.
pub const LOOP: f32 = 0.98;

pub struct Ensemble {
    voices: [ModDelay; ir::ENSEMBLE_VOICES],
    /// Which voices sound (a silent voice only keeps its clock).
    live: [bool; ir::ENSEMBLE_VOICES],
    e: ir::Ensemble,
    /// The scan LFO, cycles.
    cycles: f64,
    rate: f32,
}

fn settings(v: &ir::EnsembleVoice) -> Settings {
    let swing = 0.5 * v.depth_ms.max(0.0);
    let g = v.feedback.clamp(-LOOP, LOOP);
    Settings {
        ms: v.ms + swing,
        depth_ms: swing,
        second_head: false,
        input: v.input,
        // Kontakt's 100 % is 0.999: a comb that charges for ~15 s. Clamped
        // where 30 s renders stop growing (see #1345's grid).
        feedback: [[g, 0.0], [0.0, g]],
        output: v.output,
        damping_hz: v.damping_hz,
        low_cut_hz: v.low_cut_hz,
        steep_low_cut: false,
        high_shelf: None,
        spread_ms: 0.0,
        lfo_hz: v.hz,
        phase: v.phase,
    }
}

/// The scan's 0..1 shape at `cycles`.
fn shape(s: ir::ScanShape, cycles: f64) -> f32 {
    let x = cycles.rem_euclid(1.0) as f32;
    match s {
        ir::ScanShape::SawUp => x,
        ir::ScanShape::SawDown => 1.0 - x,
        ir::ScanShape::Triangle => 1.0 - (2.0 * x - 1.0).abs(),
    }
}

/// Voice `k`'s level under scan `s` at `cycles`; voices past the ring sound.
fn scanned(s: &ir::Scan, cycles: f64, k: usize) -> f32 {
    let ring = f32::from(s.voices.max(1));
    if k >= usize::from(s.voices) {
        return 1.0;
    }
    let at = (s.position + s.depth * shape(s.shape, cycles)).rem_euclid(ring);
    let d = (at - k as f32).abs();
    let d = d.min(ring - d);
    if d >= s.width {
        return 0.0;
    }
    (std::f32::consts::FRAC_PI_2 * d / s.width).cos().powi(2)
}

impl Ensemble {
    /// Lines long enough for `max_ms`, each LFO at its voice's start. Not
    /// real-time: allocates.
    pub fn new(e: ir::Ensemble, rate: u32, max_ms: f32) -> Self {
        let voices = e.voices.map(|v| {
            let mut line = ModDelay::new(settings(&v), rate, max_ms);
            // Its LFO starts `start` cycles in: the public clock, run on.
            // ponytail: retire once ModDelay takes a start phase (BOARD).
            if v.hz > 0.0 {
                line.idle((v.start.rem_euclid(1.0) / v.hz * rate as f32) as usize);
            }
            line
        });
        let mut out = Self {
            voices,
            live: [false; ir::ENSEMBLE_VOICES],
            e,
            cycles: 0.0,
            rate: rate as f32,
        };
        out.apply(e);
        out
    }

    fn apply(&mut self, e: ir::Ensemble) {
        for ((line, live), v) in self.voices.iter_mut().zip(&mut self.live).zip(&e.voices) {
            line.set(settings(v));
            *live = v.output != [[0.0; 2]; 2];
        }
        self.e = e;
    }

    /// Each voice's scan level for this pass, and the direct matrix.
    fn levels(&self) -> ([f32; ir::ENSEMBLE_VOICES], ir::Matrix) {
        let mut levels = [1.0; ir::ENSEMBLE_VOICES];
        let mut direct = [[self.e.dry, 0.0], [0.0, self.e.dry]];
        for (k, (level, v)) in levels.iter_mut().zip(&self.e.voices).enumerate() {
            if let Some(s) = &self.e.scan {
                *level = scanned(s, self.cycles, k);
            }
            for (row, d) in direct.iter_mut().zip(v.direct) {
                row[0] += *level * d[0];
                row[1] += *level * d[1];
            }
        }
        (levels, direct)
    }

    fn advance(&mut self, frames: usize) {
        if let Some(s) = &self.e.scan {
            self.cycles = (self.cycles + f64::from(s.hz) * frames as f64 / f64::from(self.rate))
                .rem_euclid(1.0);
        }
    }
}

impl Wet for Ensemble {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        let mut scratch = [[0.0f32; 2]; CHUNK];
        for block in io.chunks_mut(CHUNK) {
            let n = block.len();
            let (levels, direct) = self.levels();
            let mut sum = [[0.0f32; 2]; CHUNK];
            for ((line, live), level) in self.voices.iter_mut().zip(self.live).zip(levels) {
                let (Some(s), Some(acc)) = (scratch.get_mut(..n), sum.get_mut(..n)) else {
                    continue;
                };
                if !live {
                    // Silent voices keep their clocks in step.
                    line.idle(n);
                    continue;
                }
                s.copy_from_slice(block);
                line.process(s);
                for (a, y) in acc.iter_mut().zip(s.iter()) {
                    a[0] += level * y[0];
                    a[1] += level * y[1];
                }
            }
            for (x, y) in block.iter_mut().zip(sum.iter()) {
                let [[a, b], [c, d]] = direct;
                *x = [a * x[0] + b * x[1] + y[0], c * x[0] + d * x[1] + y[1]];
            }
            self.advance(n);
        }
    }

    fn memory(&self) -> Option<usize> {
        self.voices.iter().map(ModDelay::memory).max()
    }

    fn written(&self) -> f32 {
        self.voices
            .iter()
            .map(ModDelay::written)
            .fold(0.0, f32::max)
    }

    fn idle(&mut self, silence: &mut [[f32; 2]]) {
        for line in &mut self.voices {
            line.idle(silence.len());
        }
        // The scan steps once a pass, as in `process`.
        for chunk in silence.chunks(CHUNK) {
            self.advance(chunk.len());
        }
    }

    fn set(&mut self, kind: ir::EffectKind, _tempo: f32) {
        if let ir::EffectKind::Ensemble(e) = kind {
            self.apply(e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scan_window_rings_round_its_voices() {
        let s = ir::Scan {
            voices: 4,
            width: 1.0,
            position: 3.5,
            depth: 0.0,
            hz: 0.0,
            shape: ir::ScanShape::SawUp,
        };
        // Half way from the last voice back to the first: both at half power.
        let at = |k| scanned(&s, 0.0, k);
        assert!((at(3) - 0.5).abs() < 1e-6 && (at(0) - 0.5).abs() < 1e-6);
        assert_eq!((at(1), at(2), at(4)), (0.0, 0.0, 1.0));
    }
}
