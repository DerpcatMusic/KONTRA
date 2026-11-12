//! [`ir::Ensemble`] on the public [`Choir`], plus the input's direct share.
//! It never allocates once built.

use kontra_dsp::delay::{Choir, ChoirSettings};
use kontra_ir as ir;

use crate::processors::Wet;

pub struct Ensemble {
    choir: Choir,
    direct: f32,
}

fn settings(e: &ir::Ensemble) -> ChoirSettings {
    let v = e.voices;
    let first = v.delay.first().copied().unwrap_or(0.0);
    let rate = v.rate.map(|r| r * e.hz);
    ChoirSettings {
        delay_s: v.delay.map(|d| d * e.span),
        swing_s: rate.map(|hz| if hz > 0.0 { e.sweep / hz } else { 0.0 }),
        lfo_hz: rate,
        start: v.start,
        bend: v.bend,
        wobble_hz: std::array::from_fn(|k| {
            rate.get(k).copied().unwrap_or(0.0) * v.wobble_rate.get(k).copied().unwrap_or(0.0)
        }),
        wobble: v.wobble,
        smooth_hz: v.smooth_hz,
        filter: v.filter,
        // A line `k` times the first's length decays alike per second.
        loop_gain: v.delay.map(|d| match first > 0.0 {
            true => e.feedback.powf(d / first),
            false => 0.0,
        }),
        scatter: e.scatter,
        sign: match e.polarity {
            ir::Polarity::Normal => 1.0,
            ir::Polarity::Inverted => -1.0,
        },
        input: e.input,
        level: e.levels,
        angle: e.angles,
        bus: e.shelf,
        gain: e.wet,
    }
}

impl Ensemble {
    /// Not real-time: allocates.
    pub fn new(e: ir::Ensemble, rate: u32) -> Self {
        Self {
            choir: Choir::new(settings(&e), rate),
            direct: e.direct,
        }
    }
}

impl Wet for Ensemble {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        const CHUNK: usize = 64;
        for block in io.chunks_mut(CHUNK) {
            let mut dry = [[0.0f32; 2]; CHUNK];
            if let Some(dry) = dry.get_mut(..block.len()) {
                dry.copy_from_slice(block);
            }
            self.choir.process(block);
            for (y, x) in block.iter_mut().zip(dry) {
                *y = [y[0] + self.direct * x[0], y[1] + self.direct * x[1]];
            }
        }
    }

    fn memory(&self) -> Option<usize> {
        Some(self.choir.memory())
    }

    fn written(&self) -> f32 {
        self.choir.written()
    }

    fn idle(&mut self, silence: &mut [[f32; 2]]) {
        self.choir.idle(silence.len());
    }

    fn set(&mut self, kind: ir::EffectKind, _tempo: f32) {
        if let ir::EffectKind::Ensemble(e) = kind {
            self.choir.set(settings(&e));
            self.direct = e.direct;
        }
    }
}
