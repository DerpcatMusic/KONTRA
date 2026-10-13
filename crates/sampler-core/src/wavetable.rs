//! Kontakt wavetable readout. Native cycles have 2048 frames; their sample
//! rate and mapped root note do not set the oscillator's fundamental.
//! https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/classic-view

use crate::Frame;
use crate::source::ReadFrames;
use sampler_ir::Wavetable;

pub(super) const CYCLE: usize = 2048;

/// NI KSP Reference, Engine Parameters / Source Module, WT_FORM_MODE.
/// Native shared phase-form routine 0x140567aa0: PWM 0x140567d2d,
/// asymmetric modes 0x140567d5b..0x140567e5f, Flip 0x140567e64,
/// Seesaw 0x140567f83 and Bend2 0x140567bf7..0x140567c84.
/// Only measured phase forms are admitted; amplitude forms need another kernel.
fn warp(phase: f32, amount: f32, kind: u8) -> f32 {
    let amount = amount.clamp(0., 1.);
    match kind {
        7 => {
            let read = phase / (1. - amount * 0.9375);
            return if read < 1. { read } else { 0. };
        }
        11 => {
            let flip = if amount < 0.5 {
                phase < amount * 2.
            } else {
                f64::from(phase) >= f64::from(amount) * 2. - 1.
            };
            return if flip { 1. - phase } else { phase };
        }
        17 => {
            return if amount == 0. {
                phase
            } else if amount == 1. {
                1. - phase
            } else if phase < 1. - amount {
                phase / (1. - amount)
            } else {
                (amount - (phase - (1. - amount))) / amount
            };
        }
        31..=33 => {
            let bend = match kind {
                31 => amount * -0.9,
                32 => amount * 0.9,
                _ => (amount - 0.5) * 1.8,
            };
            let denominator = (phase - 0.5).abs() * (bend * 4.) - bend - 1.;
            return (0.5 / denominator) * ((phase + phase - 1.) * (bend - 1.)) + 0.5;
        }
        _ => {}
    }
    let offset = match kind {
        8 => -amount * 0.9,
        9 => amount * 0.9,
        10 => (amount - 0.5) * 1.8,
        14 => -amount * 0.98,
        15 => amount * 0.98,
        16 => (amount - 0.5) * 1.96,
        _ => return phase,
    };
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

#[cfg(test)]
mod tests {
    #[test]
    fn phase_forms_match_original_kontakt_instructions() {
        let receipt: serde_json::Value = serde_json::from_str(include_str!("wavetable_native_vectors.json")).unwrap();
        assert_eq!(receipt["entry"], "0x140567aa0");
        assert_eq!(receipt["helper_substitutions"].as_array().unwrap().len(), 0);
        for case in receipt["cases"].as_array().unwrap() {
            let mode = case[0].as_u64().unwrap() as u8;
            let phase = case[1].as_f64().unwrap() as f32;
            let amount = case[2].as_f64().unwrap() as f32;
            let native = case[3].as_f64().unwrap() as f32;
            let actual = super::warp(phase, amount, mode);
            assert!((actual - native).abs() <= 2e-7,
                "mode{mode} phase{phase} amount{amount}: actual{actual}, native{native}");
        }
    }
}
