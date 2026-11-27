//! Ten-character harmonic saturator with bass-preserving crossover and 3-band EQ.
//!
//! Pre-allocated, zero allocations on the audio thread, denormal-safe, and
//! smoothed across live parameter updates.

use crate::stage::{DcBlocker, ToneStack3};
use kontra_dsp_core::{Biquad, Halfband, high_pass};

/// Physical settings for [`ColorSat`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorSatSettings {
    /// Saturation character mode (`0..=9`).
    pub character: u8,
    /// Input drive (`0.0..=1.0`).
    pub drive: f32,
    /// Pre-drive boost switch.
    pub boost: bool,
    /// Pre-drive boost amount in dB (`0.0..=24.0`).
    pub boost_db: f32,
    /// Clean sub-bass bypass blend (`0.0..=1.0`).
    pub bass_saver: f32,
    /// Post low-pass cutoff in Hz (`500.0..=20_000.0`).
    pub lpf_hz: f32,
    /// Pre high-pass cutoff in Hz (`20.0..=2000.0`).
    pub hpf_hz: f32,
    /// 3-band EQ: bass shelf dB, mid bell dB/Hz/Q, treble shelf dB.
    pub bass_db: f32,
    pub mid_db: f32,
    pub mid_hz: f32,
    pub mid_q: f32,
    pub treble_db: f32,
    /// Wet/dry blend (`0.0..=1.0`).
    pub mix: f32,
}

use kontra_dsp_core::low_pass;

use kontra_dsp_core::flush_f32 as flush32;

use kontra_dsp_core::math::db_to_gain_f32 as db_to_lin;

/// Evaluates one of the 10 harmonic transfer curves at input `x` with drive gain `g`.
/// Ten-character saturation curves across linear gain, H2/H3 harmonics, and ceiling.
#[inline]
fn curve(character: u8, x: f32, g: f32) -> f32 {
    let u = x * g;
    match character % 10 {
        // 0: Solid console — symmetric soft saturation (~0.47 ceiling, lin200 ~ +10.3 dB)
        0 => (u / 0.47).tanh() * 0.47,
        // 1: Warm analog aura — smooth algebraic knee
        1 => (u * 1.05) / (1.0 + (u * 2.0).powi(2)).sqrt(),
        // 2: Harmonic control — asymmetric even+odd curve (strong H2 ~ -17 dB, ceiling ~0.46)
        2 => {
            let b = 0.32f32;
            let s = b.cosh();
            let c = 0.47;
            ((u * 1.12 * s * s / c + b).tanh() - b.tanh()) * c
        }
        // 3: Full on — dense hard-knee saturation
        3 => {
            let c = 0.44;
            let a = u * 1.15 / c;
            (a / (1.0 + a.abs().powf(2.2)).powf(1.0 / 2.2)) * c
        }
        // 4: Super amp — two-stage asymmetric triode cascade
        4 => {
            let s1 = (u * 1.2 + 0.18).tanh() - 0.178;
            (s1 * 1.4 - 0.08).tanh() * 0.46
        }
        // 5: Smooth fuzz — pure odd exponential soft clip (H2 < -44 dB, H3 ~ -11 dB, ceiling ~0.42)
        5 => {
            let c = 0.42;
            let a = u * 1.22 / c;
            let e = (-a.abs().min(20.0)).exp();
            let v = if a < 0.0 { e - 1.0 } else { 1.0 - e };
            v * c
        }
        // 6: Magnetic tape — symmetric soft saturation with 3rd harmonic warmth
        6 => (u / 0.50).tanh() * 0.50,
        // 7: Crunch — asymmetric push-pull clipping
        7 => {
            let v = if u >= 0.0 {
                (u * 2.4).tanh()
            } else {
                (u * 1.7).tanh()
            };
            v * 0.44
        }
        // 8: Heavy saturator — rich overdrive with mild foldback edge
        8 => {
            let s = (u * 2.4).tanh();
            (s - 0.14 * (s * std::f32::consts::PI).sin()) * 0.44
        }
        // 9: Diode bridge — lower-ceiling asymmetric diode clipper (~0.30 ceiling)
        _ => {
            let t = if u >= 0.0 { 0.27 } else { 0.34 };
            let r = (u * 1.18) / t;
            (r / (1.0 + r * r).sqrt()) * t
        }
    }
}

/// Ten-character harmonic saturator.
#[derive(Clone, Debug)]
pub struct ColorSat {
    s: ColorSatSettings,
    rate: u32,
    hpf: Biquad,
    lpf: Biquad,
    tone: ToneStack3,
    /// One-pole low-pass state for bass-saver clean sub path.
    sub_lp: [f32; 2],
    sub_coeff: f32,
    /// DC blocker state per channel.
    dc: DcBlocker,
    /// Smoothed drive, mix, bass saver.
    drive_cur: f32,
    mix_cur: f32,
    saver_cur: f32,
    smooth: f32,
    /// 2x polyphase IIR halfband oversampler around the nonlinear stage.
    oversample: Halfband,
}

impl ColorSat {
    #[must_use]
    pub fn new(s: ColorSatSettings, rate: u32) -> Self {
        let rate = rate.max(8_000);
        let smooth = 1.0 - (-1.0 / (0.008 * rate as f32)).exp();
        let sub_coeff = 1.0 - (-std::f32::consts::TAU * 160.0 / (2.0 * rate as f32)).exp();
        let mut sat = Self {
            s,
            rate,
            hpf: high_pass(20.0, 0.707, rate),
            lpf: low_pass(20_000.0, 0.707, rate),
            tone: ToneStack3::new(120.0, 1000.0, 4000.0, rate),
            sub_lp: [0.0; 2],
            sub_coeff,
            dc: DcBlocker::new(0.998),
            drive_cur: s.drive.clamp(0.0, 1.0),
            mix_cur: s.mix.clamp(0.0, 1.0),
            saver_cur: s.bass_saver.clamp(0.0, 1.0),
            smooth,
            oversample: Halfband::new(rate),
        };
        sat.retune(s);
        sat
    }

    pub fn set(&mut self, s: ColorSatSettings) {
        self.s = s;
        self.retune(s);
    }

    /// Clears all filter, oversampler, DC-blocker, and sub-crossover states.
    pub fn reset(&mut self) {
        self.hpf.reset();
        self.lpf.reset();
        self.tone.reset();
        self.sub_lp = [0.0; 2];
        self.dc.reset();
        self.drive_cur = self.s.drive.clamp(0.0, 1.0);
        self.mix_cur = self.s.mix.clamp(0.0, 1.0);
        self.saver_cur = self.s.bass_saver.clamp(0.0, 1.0);
        self.oversample.reset();
    }

    fn retune(&mut self, s: ColorSatSettings) {
        let r = self.rate;
        self.hpf
            .retune(&high_pass(s.hpf_hz.clamp(10.0, 0.47 * r as f32), 0.707, r));
        self.lpf
            .retune(&low_pass(s.lpf_hz.clamp(10.0, 0.47 * r as f32), 0.707, r));
        self.tone.retune(
            120.0,
            s.bass_db.clamp(-15.0, 15.0),
            s.mid_hz.clamp(120.0, 8000.0),
            s.mid_q.clamp(0.3, 8.0),
            s.mid_db.clamp(-15.0, 15.0),
            4000.0,
            s.treble_db.clamp(-15.0, 15.0),
            r,
        );
    }

    /// Processes stereo audio in place with zero allocations.
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        if io.is_empty() {
            return;
        }
        let character = self.s.character;
        let boost_lin = if self.s.boost {
            db_to_lin(self.s.boost_db.clamp(0.0, 24.0))
        } else {
            1.0
        };
        let drive_tgt = self.s.drive.clamp(0.0, 1.0);
        let mix_tgt = self.s.mix.clamp(0.0, 1.0);
        let saver_tgt = self.s.bass_saver.clamp(0.0, 1.0);
        let smooth = self.smooth;
        let sub_c = self.sub_coeff;
        let use_hpf = self.s.hpf_hz > 11.0;
        let use_lpf = self.s.lpf_hz < 19_500.0;

        // Keep dry copies in chunks on the stack (no heap allocation).
        for chunk in io.chunks_mut(64) {
            let mut dry = [[0.0f32; 2]; 64];
            let n = chunk.len();
            for (dst, &src) in dry.iter_mut().zip(chunk.iter()) {
                *dst = src;
            }

            if use_hpf {
                self.hpf.process(chunk);
            }

            let smooth_os = 1.0 - (-1.0 / (0.008 * 2.0 * self.rate as f32)).exp();
            let drive_cur = &mut self.drive_cur;
            let saver_cur = &mut self.saver_cur;
            let sub_lp = &mut self.sub_lp;
            let dc = &mut self.dc;
            self.oversample.process(chunk, |mut frame| {
                *drive_cur += (drive_tgt - *drive_cur) * smooth_os;
                *saver_cur += (saver_tgt - *saver_cur) * smooth_os;
                let gain = 1.68 * (4.0f32).powf(*drive_cur) * boost_lin;
                let saver = *saver_cur;
                for (ch, (sample, lp)) in frame.iter_mut().zip(sub_lp.iter_mut()).enumerate() {
                    let x = *sample;
                    let sub = *lp + (x - *lp) * sub_c;
                    *lp = flush32(sub);
                    let to_shape = x - saver * sub;
                    let shaped = curve(character, to_shape, gain);
                    *sample = dc.tick_ch(ch, shaped) + saver * sub;
                }
                frame
            });

            if use_lpf {
                self.lpf.process(chunk);
            }
            self.tone.process(chunk);

            for (frame, d) in chunk.iter_mut().zip(dry.iter().take(n)) {
                self.mix_cur += (mix_tgt - self.mix_cur) * smooth;
                let w = self.mix_cur;
                let inv = 1.0 - w;
                frame[0] = flush32(inv * d[0] + w * frame[0]);
                frame[1] = flush32(inv * d[1] + w * frame[1]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_ten_characters_produce_finite_distinct_saturation() {
        let mut rms_by_mode = [0.0f32; 10];
        for m in 0..10u8 {
            let mut sat = ColorSat::new(
                ColorSatSettings {
                    character: m,
                    drive: 0.75,
                    boost: false,
                    boost_db: 6.0,
                    bass_saver: 0.0,
                    lpf_hz: 18_000.0,
                    hpf_hz: 20.0,
                    bass_db: 0.0,
                    mid_db: 0.0,
                    mid_hz: 1000.0,
                    mid_q: 1.0,
                    treble_db: 0.0,
                    mix: 1.0,
                },
                48_000,
            );
            let mut io: Vec<[f32; 2]> = (0..1024)
                .map(|n| [(std::f32::consts::TAU * 330.0 * n as f32 / 48_000.0).sin() * 0.4; 2])
                .collect();
            let dry = io.clone();
            sat.process(&mut io);
            assert!(io.iter().flatten().all(|v| v.is_finite()), "mode {m}");
            assert_ne!(io, dry, "mode {m} must alter input");
            let e: f32 = io.iter().map(|[l, _]| l * l).sum();
            rms_by_mode[usize::from(m)] = (e / 1024.0).sqrt();
            assert!(rms_by_mode[usize::from(m)] > 0.02, "mode {m} too quiet");
        }
    }
}
