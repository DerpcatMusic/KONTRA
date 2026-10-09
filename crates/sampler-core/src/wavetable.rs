//! Kontakt wavetable readout. Native cycles have 2048 frames; their sample
//! rate and mapped root note do not set the oscillator's fundamental.
//! https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/classic-view

use crate::Frame;
use crate::source::ReadFrames;
use sampler_ir::Wavetable;

pub(super) const CYCLE: usize = 2048;

/// ASYM2MP moves the cycle midpoint while keeping both endpoints fixed.
/// Native shared phase-form routine: 140567aa0, switch16→140567e4d.
fn warp(phase: f32, amount: f32, kind: u8) -> f32 {
    if kind != 16 {
        return phase;
    }
    let offset = (amount.clamp(0., 1.) - 0.5) * 1.96;
    if f64::from(phase) < 0.5 + f64::from(offset * 0.5) {
        phase / (1. + offset)
    } else {
        (phase - 1.) / (1. - offset) + 1.
    }
}

/// Resident, complete cycles prepared by the bank loader.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Table {
    pub first: usize,
    pub cycles: usize,
}

impl Table {
    pub fn new(first: usize, end: usize) -> Option<Self> {
        let frames = end.checked_sub(first)?;
        (first % CYCLE == 0 && frames > 0 && frames % CYCLE == 0).then_some(Self {
            first,
            cycles: frames / CYCLE,
        })
    }

    /// Cubic readout within a cycle, linear morph between adjacent cycles.
    /// Phase stays within one cycle, including when a block crosses its seam.
    pub(super) fn render_frames(
        self,
        mut read: impl FnMut(usize) -> Frame,
        source: &Wavetable,
        mut phase: f64,
        step: f64,
        out: &mut [Frame],
    ) -> f64 {
        let position = f64::from(source.position.clamp(0., 1.)) * (self.cycles - 1) as f64;
        let lo = position as usize;
        let hi = (lo + 1).min(self.cycles - 1);
        let blend = position.fract() as f32;
        let lo = self.first + lo * CYCLE;
        let hi = self.first + hi * CYCLE;
        for frame in out {
            let read_phase = warp(
                warp(
                    (phase / CYCLE as f64) as f32,
                    source.form1,
                    source.form1_type,
                ),
                source.form2,
                source.form2_type,
            ) as f64
                * CYCLE as f64;
            let a = Self::read(&mut read, lo, read_phase);
            let b = if lo == hi || blend == 0. {
                a
            } else {
                Self::read(&mut read, hi, read_phase)
            };
            *frame = std::array::from_fn(|c| a[c] + (b[c] - a[c]) * blend);
            phase = (phase + step).rem_euclid(CYCLE as f64);
        }
        phase
    }

    fn read(read: &mut impl FnMut(usize) -> Frame, first: usize, phase: f64) -> Frame {
        let index = phase as usize;
        let taps: [Frame; 4] =
            std::array::from_fn(|i| read(first + (index + CYCLE + i - 1) % CYCLE));
        hermite(&taps, phase.fract() as f32)
    }
}

// port from v1 0cb7a8a0:src/engine/voice.rs::hermite, unchanged arithmetic.
fn hermite(q: &[Frame; 4], t: f32) -> Frame {
    std::array::from_fn(|c| {
        let (xm1, x0, x1, x2) = (q[0][c], q[1][c], q[2][c], q[3][c]);
        let c1 = 0.5 * (x1 - xm1);
        let c2 = xm1 - 2.5 * x0 + 2.0 * x1 - 0.5 * x2;
        let c3 = 0.5 * (x2 - xm1) + 1.5 * (x0 - x1);
        ((c3 * t + c2) * t + c1) * t + x0
    })
}

impl Table {
    pub(super) fn sample(
        self,
        pcm: &(impl ReadFrames + ?Sized),
        source: &Wavetable,
        phase: f64,
    ) -> Option<Frame> {
        let mut ready = true;
        let mut out = [[0.; 2]; 1];
        self.render_frames(
            |at| {
                pcm.frame(at).unwrap_or_else(|| {
                    ready = false;
                    [0.; 2]
                })
            },
            source,
            phase,
            0.,
            &mut out,
        );
        ready.then_some(out[0])
    }
}
