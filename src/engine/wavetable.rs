//! Kontakt wavetable readout. Native cycles have 2048 frames; their sample
//! rate and mapped root note do not set the oscillator's fundamental.
//! https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/classic-view

use crate::audio::{Frame, Pcm};

pub(super) const CYCLE: usize = 2048;

/// Until their native laws are proven, active phase forms, randomized starts
/// and the modulation oscillator retain the importer's unsupported warning.
pub(super) fn supported(source: &crate::import::Wavetable, tracking: bool) -> bool {
    tracking && form_supported(source.form1_type) && form_supported(source.form2_type)
        && source.inharmonic_mode == 0 && source.mod_type == 0
        && source.phase_random == 0.
        && source.position.is_finite() && source.phase.is_finite()
        && source.form1.is_finite() && source.form2.is_finite()
}

pub(super) fn form_supported(kind: i32) -> bool { matches!(kind, 0 | 16) }

/// ASYM2MP moves the cycle midpoint while keeping both endpoints fixed.
/// Native shared phase-form routine: 140567aa0, switch16→140567e4d.
fn warp(phase: f32, amount: f32, kind: i32) -> f32 {
    if kind != 16 { return phase; }
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
    pub fn render(
        self,
        data: &Pcm,
        source: &crate::import::Wavetable,
        mut phase: f64,
        step: f64,
        out: &mut [Frame],
    ) -> f64 {
        let position = f64::from(source.position.clamp(0., 1.)) * (self.cycles - 1) as f64;
        let lo = position as usize;
        let hi = (lo + 1).min(self.cycles - 1);
        let blend = position.fract() as f32;
        for frame in out {
            let read_phase = warp(warp((phase / CYCLE as f64) as f32,
                source.form1, source.form1_type), source.form2, source.form2_type) as f64 * CYCLE as f64;
            let a = self.read(data, lo, read_phase);
            let b = if lo == hi || blend == 0. {
                a
            } else {
                self.read(data, hi, read_phase)
            };
            *frame = std::array::from_fn(|c| a[c] + (b[c] - a[c]) * blend);
            phase = (phase + step).rem_euclid(CYCLE as f64);
        }
        phase
    }

    fn read(self, data: &Pcm, cycle: usize, phase: f64) -> Frame {
        let index = phase as usize;
        let taps: [Frame; 4] = std::array::from_fn(|i| {
            let at = self.first + cycle * CYCLE + (index + CYCLE + i - 1) % CYCLE;
            let mut frame = [[0.; 2]; 1];
            data.window(at, &mut frame)
                .expect("prepared resident wavetable")[0]
        });
        super::voice::hermite(&taps, phase.fract() as f32)
    }
}

pub(super) fn step(note: u8, rate: f64) -> f64 {
    440. * 2f64.powf((f64::from(note) - 69.) / 12.) * CYCLE as f64 / rate
}

/// An oscillator only needs the increment modulo one cycle. Reduce before
/// fixed-point conversion: its uncapped requested pitch remains in Voice,
/// while every rendered increment fits the common clock without saturation.
pub(super) fn clock(step: f64) -> u64 {
    if step.is_finite() {
        (step.rem_euclid(CYCLE as f64) * super::voice::FIXED_ONE) as u64
    } else {
        // A floating-point frequency overflow has no representable cycle
        // fraction; keep its aliased clock stationary instead of producing NaN.
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frequency_and_morph_follow_cycles_without_sample_pitch() {
        let frames: Vec<Frame> = (0..CYCLE)
            .map(|i| {
                let x = (i as f64 * std::f64::consts::TAU / CYCLE as f64).sin() as f32;
                [x; 2]
            })
            .chain((0..CYCLE).map(|i| {
                let x = (i as f64 * std::f64::consts::TAU / CYCLE as f64).sin() as f32;
                [-x; 2]
            }))
            .collect();
        let data = Pcm::pack(&frames, false);
        let table = Table::new(0, frames.len()).unwrap();
        for note in [33, 69, 96, 127] {
            let delta = step(note, 48_000.);
            let mut out = [[0.; 2]; 128];
            let mut phase = 0.;
            for block in 0..16 {
                phase = table.render(&data, &crate::import::Wavetable::default(), phase, delta, &mut out);
                for (i, frame) in out.iter().enumerate() {
                    let expected = (((block * 128 + i) as f64 * delta) * std::f64::consts::TAU
                        / CYCLE as f64)
                        .sin();
                    assert!(
                        (f64::from(frame[0]) - expected).abs() < 2e-6,
                        "note {note}, block {block}, frame {i}"
                    );
                }
            }
            table.render(&data, &crate::import::Wavetable { position: 0.5, ..Default::default() }, 0., delta, &mut out);
            assert!(out.iter().flatten().all(|x| x.abs() < 1e-7));
            table.render(&data, &crate::import::Wavetable { position: 1., ..Default::default() }, 0., delta, &mut out);
            assert!(out[1][0] < 0.);
        }
        assert!(Table::new(0, CYCLE - 1).is_none());
        assert!(Table::new(1, CYCLE + 1).is_none());
    }

    #[test]
    fn clock_reduces_before_fixed_point_conversion_without_a_frequency_ceiling() {
        for step in [0., f64::MIN_POSITIVE, 13.75, 2048., 2048.125, 1e12, 1e280, f64::MAX] {
            let clock = clock(step);
            assert!(clock < (CYCLE as u64) << 32);
            let reduced = clock as f64 / super::voice::FIXED_ONE;
            assert!((step.rem_euclid(CYCLE as f64) - reduced).abs() <= 1. / super::voice::FIXED_ONE);
            for n in [1, 3, 127, 64, 128, 17] {
                let advanced = (clock * n) as f64 / super::voice::FIXED_ONE;
                assert!(advanced.is_finite());
                assert_eq!((advanced % CYCLE as f64),
                    ((clock * n) % ((CYCLE as u64) << 32)) as f64 / super::voice::FIXED_ONE);
            }
        }
        assert_eq!(clock(f64::INFINITY), 0);
        assert_eq!(clock(f64::NAN), 0);
    }

    #[test]
    fn asym2mp_keeps_center_neutral_and_moves_midpoint_without_changing_period() {
        for amount in [0., 0.25, 0.5, 0.75, 1.] {
            let offset = (amount - 0.5) * 1.96;
            let midpoint = 0.5 + offset * 0.5;
            assert_eq!(warp(0., amount, 16), 0.);
            assert_eq!(warp(1., amount, 16), 1.);
            assert!((warp(midpoint, amount, 16) - 0.5).abs() < 1e-6);
            for i in 0..2048 {
                let phase = i as f32 / 2048.;
                let expected = if phase < midpoint { phase * 0.5 / midpoint }
                    else { 0.5 + (phase - midpoint) * 0.5 / (1. - midpoint) };
                assert!((warp(phase, amount, 16) - expected).abs() < 3e-6);
                assert_eq!(warp(phase, 0.5, 16), phase);
                assert_eq!(warp(phase, amount, 0), phase);
            }
        }
        assert!(supported(&crate::import::Wavetable { form1_type: 16, form1: 0.5,
            inharmonic: 0.5, inharmonic_mode: 0, ..Default::default() }, true));
        assert!(!supported(&crate::import::Wavetable { form1_type: 17, ..Default::default() }, true));
    }
}
