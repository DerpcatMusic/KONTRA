//! Kontakt wavetable readout. Native cycles have 2048 frames; their sample
//! rate and mapped root note do not set the oscillator's fundamental.
//! https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/classic-view

use crate::audio::{Frame, Pcm};

pub(super) const CYCLE: usize = 2048;

/// Until their native laws are proven, active phase forms, randomized starts
/// and the modulation oscillator retain the importer's unsupported warning.
pub(super) fn supported(source: &crate::import::Wavetable, tracking: bool) -> bool {
    tracking && source.form1_type == 0 && source.form2_type == 0
        && source.inharmonic == 0. && source.mod_type == 0
        && source.phase_random == 0.
        && source.position.is_finite() && source.phase.is_finite()
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
        position: f32,
        mut phase: f64,
        step: f64,
        out: &mut [Frame],
    ) -> f64 {
        let position = f64::from(position.clamp(0., 1.)) * (self.cycles - 1) as f64;
        let lo = position as usize;
        let hi = (lo + 1).min(self.cycles - 1);
        let blend = position.fract() as f32;
        for frame in out {
            let a = self.read(data, lo, phase);
            let b = if lo == hi || blend == 0. {
                a
            } else {
                self.read(data, hi, phase)
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
                phase = table.render(&data, 0., phase, delta, &mut out);
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
            table.render(&data, 0.5, 0., delta, &mut out);
            assert!(out.iter().flatten().all(|x| x.abs() < 1e-7));
            table.render(&data, 1., 0., delta, &mut out);
            assert!(out[1][0] < 0.);
        }
        assert!(Table::new(0, CYCLE - 1).is_none());
        assert!(Table::new(1, CYCLE + 1).is_none());
    }
}
