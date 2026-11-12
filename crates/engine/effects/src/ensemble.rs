//! [`ir::Ensemble`] on the public [`ModDelay`]: one line per voice, summed
//! with the input's direct share. It never allocates once built.

use kontra_dsp::delay::{ModDelay, Settings};
use kontra_ir as ir;

use crate::processors::Wet;

/// Frames per pass through the voices (a stack scratch).
const CHUNK: usize = 64;

/// The largest loop gain a voice runs at.
pub const LOOP: f32 = 0.98;

pub struct Ensemble {
    voices: [ModDelay; ir::ENSEMBLE_VOICES],
    /// Which voices sound (a silent voice is skipped).
    live: [bool; ir::ENSEMBLE_VOICES],
    direct: f32,
}

fn settings(e: &ir::Ensemble, v: &ir::EnsembleVoice) -> Settings {
    let swing = if v.hz > 0.0 {
        0.5 * (e.sweep / v.hz).max(0.0)
    } else {
        0.0
    };
    let diagonal = |g: f32| [[g, 0.0], [0.0, g]];
    let first = e.voices.first().map_or(v.ms, |f| f.ms);
    let length = if first > 0.0 { v.ms / first } else { 1.0 };
    let feedback = e.feedback.signum() * e.feedback.abs().powf(length);
    let (s, c) = v.angle.sin_cos();
    let g = v.level;
    Settings {
        ms: v.ms + swing,
        depth_ms: swing,
        second_head: false,
        input: diagonal(e.input),
        // Kontakt's 100 % is 0.999: a comb that charges for ~15 s. Clamped
        // where 30 s renders stop growing (see the PR's grid).
        feedback: diagonal(feedback.clamp(-LOOP, LOOP)),
        output: [[g * c, -g * s], [g * s, g * c]],
        damping_hz: if v.high_pass {
            f32::INFINITY
        } else {
            v.corner_hz
        },
        low_cut_hz: if v.high_pass { v.corner_hz } else { 0.0 },
        steep_low_cut: false,
        high_shelf: None,
        spread_ms: 0.0,
        lfo_hz: v.hz,
        phase: 0.0,
    }
}

impl Ensemble {
    /// Lines long enough for `max_ms`, each LFO at its voice's start. Not
    /// real-time: allocates.
    pub fn new(e: ir::Ensemble, rate: u32, max_ms: f32) -> Self {
        let voices = e.voices.map(|v| {
            let mut line = ModDelay::new(settings(&e, &v), rate, max_ms);
            // Its LFO starts `start` cycles in: the public clock, run on.
            if v.hz > 0.0 {
                line.idle((v.start.rem_euclid(1.0) / v.hz * rate as f32) as usize);
            }
            line
        });
        let mut out = Self {
            voices,
            live: [false; ir::ENSEMBLE_VOICES],
            direct: e.direct,
        };
        out.apply(e);
        out
    }

    fn apply(&mut self, e: ir::Ensemble) {
        for ((line, live), v) in self.voices.iter_mut().zip(&mut self.live).zip(&e.voices) {
            line.set(settings(&e, v));
            *live = v.level != 0.0;
        }
        self.direct = e.direct;
    }
}

impl Wet for Ensemble {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        let mut scratch = [[0.0f32; 2]; CHUNK];
        for block in io.chunks_mut(CHUNK) {
            let n = block.len();
            let mut sum = [[0.0f32; 2]; CHUNK];
            for (line, live) in self.voices.iter_mut().zip(self.live) {
                let (Some(s), Some(acc)) = (scratch.get_mut(..n), sum.get_mut(..n)) else {
                    continue;
                };
                s.copy_from_slice(block);
                if live {
                    line.process(s);
                } else {
                    // Silent voices keep their clocks in step.
                    line.idle(n);
                    continue;
                }
                for (a, y) in acc.iter_mut().zip(s.iter()) {
                    a[0] += y[0];
                    a[1] += y[1];
                }
            }
            for (x, y) in block.iter_mut().zip(sum.iter()) {
                *x = [self.direct * x[0] + y[0], self.direct * x[1] + y[1]];
            }
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
    }

    fn set(&mut self, kind: ir::EffectKind, _tempo: f32) {
        if let ir::EffectKind::Ensemble(e) = kind {
            self.apply(e);
        }
    }
}
