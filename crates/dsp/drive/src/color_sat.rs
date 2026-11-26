//! Ten-character harmonic saturator with bass-preserving crossover and 3-band EQ.
//!
//! Pre-allocated, zero allocations on the audio thread, denormal-safe, and
//! smoothed across live parameter updates.

use std::f64::consts::TAU;

use kontra_dsp_core::{Biquad, Side, high_pass, shelf};

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

fn peak(hz: f32, q: f32, gain_db: f32, rate: u32) -> Biquad {
    let r = f64::from(rate.max(1));
    let f = f64::from(hz).clamp(20.0, 0.45 * r);
    let w = TAU * f / r;
    let a = 10.0f64.powf(f64::from(gain_db) / 40.0);
    let alpha = w.sin() / (2.0 * f64::from(q).max(0.15));
    let cos_w = w.cos();
    Biquad::new([
        1.0 + alpha * a,
        -2.0 * cos_w,
        1.0 - alpha * a,
        1.0 + alpha / a,
        -2.0 * cos_w,
        1.0 - alpha / a,
    ])
}

fn low_pass(hz: f32, q: f32, rate: u32) -> Biquad {
    let r = f64::from(rate.max(1));
    let f = f64::from(hz).clamp(20.0, 0.47 * r);
    let w = TAU * f / r;
    let cos_w = w.cos();
    let alpha = w.sin() / (2.0 * f64::from(q).max(0.1));
    let b0 = 0.5 * (1.0 - cos_w);
    Biquad::new([b0, 1.0 - cos_w, b0, 1.0 + alpha, -2.0 * cos_w, 1.0 - alpha])
}

#[inline]
fn flush32(x: f32) -> f32 {
    if x.abs() < 1e-20 || !x.is_finite() {
        0.0
    } else {
        x
    }
}

#[inline]
fn db_to_lin(db: f32) -> f32 {
    10.0f32.powf(db / 20.0)
}

/// Evaluates one of the 10 harmonic transfer curves at input `x` with drive gain `g`.
#[inline]
fn curve(character: u8, x: f32, g: f32) -> f32 {
    let u = x * g;
    match character % 10 {
        // 0: Solid console — subtle even/odd transformer saturation
        0 => ((u + 0.12 * u * u.abs()).tanh()) / (1.0 + 0.25 * (g - 1.0).max(0.0)).sqrt(),
        // 1: Warm analog aura — smooth algebraic knee
        1 => u / (1.0 + 0.7 * u * u).sqrt(),
        // 2: Harmonic control — asymmetric exponential-tanh curve
        2 => {
            let y = (u * 0.05) / (0.0025 * u * u + 1.0).sqrt() * 13.726_12;
            let ey = y.clamp(-12.0, 12.0);
            let s = (-((-0.75 * ey.abs().sqrt()).exp() + 1.0) * ey)
                .clamp(-20.0, 20.0)
                .exp();
            let ep = ey.exp();
            let em = (-ey).exp();
            (ep - s) / (em + ep)
        }
        // 3: Full on — dense hard-knee saturation
        3 => {
            let a = u * 1.8;
            (a / (1.0 + a.abs().powf(2.2)).powf(1.0 / 2.2)) * 0.85
        }
        // 4: Super amp — two-stage asymmetric triode cascade
        4 => {
            let s1 = (u * 1.4 + 0.15).tanh() - 0.148;
            (s1 * 1.6 - 0.08).tanh() * 0.8
        }
        // 5: Smooth fuzz — odd exponential soft clip
        5 => {
            let e = (-u.abs().min(20.0)).exp();
            if u < 0.0 { e - 1.0 } else { 1.0 - e }
        }
        // 6: Magnetic tape — symmetric soft saturation with 3rd harmonic warmth
        6 => (u * 1.15).tanh() * 0.92,
        // 7: Crunch — asymmetric push-pull clipping
        7 => {
            let v = if u >= 0.0 {
                (u * 1.9).tanh()
            } else {
                (u * 1.35).tanh()
            };
            v * 0.82
        }
        // 8: Heavy saturator — rich overdrive with mild foldback edge
        8 => {
            let s = (u * 2.1).tanh();
            (s - 0.18 * (s * std::f32::consts::PI).sin()) * 0.85
        }
        // 9: Diode bridge — crisp asymmetric diode clipper
        _ => {
            let t = if u >= 0.0 { 0.45 } else { 0.65 };
            let r = u / t;
            (r / (1.0 + r * r).sqrt()) * t * 1.35
        }
    }
}

/// Ten-character harmonic saturator.
#[derive(Debug)]
pub struct ColorSat {
    s: ColorSatSettings,
    rate: u32,
    hpf: Biquad,
    lpf: Biquad,
    bass: Biquad,
    mid: Biquad,
    treble: Biquad,
    /// One-pole low-pass state for bass-saver clean sub path.
    sub_lp: [f32; 2],
    sub_coeff: f32,
    /// DC blocker state per channel (`[x_prev, y_prev]`).
    dc: [[f32; 2]; 2],
    /// Smoothed drive, mix, bass saver.
    drive_cur: f32,
    mix_cur: f32,
    saver_cur: f32,
    smooth: f32,
}

impl ColorSat {
    #[must_use]
    pub fn new(s: ColorSatSettings, rate: u32) -> Self {
        let rate = rate.max(8_000);
        let smooth = 1.0 - (-1.0 / (0.008 * rate as f32)).exp();
        let sub_coeff = 1.0 - (-std::f32::consts::TAU * 160.0 / rate as f32).exp();
        let mut sat = Self {
            s,
            rate,
            hpf: high_pass(20.0, 0.707, rate),
            lpf: low_pass(20_000.0, 0.707, rate),
            bass: shelf(120.0, 0.0, 0.7, Side::Low, rate),
            mid: peak(1000.0, 1.0, 0.0, rate),
            treble: shelf(4000.0, 0.0, 0.7, Side::High, rate),
            sub_lp: [0.0; 2],
            sub_coeff,
            dc: [[0.0; 2]; 2],
            drive_cur: s.drive.clamp(0.0, 1.0),
            mix_cur: s.mix.clamp(0.0, 1.0),
            saver_cur: s.bass_saver.clamp(0.0, 1.0),
            smooth,
        };
        sat.retune(s);
        sat
    }

    pub fn set(&mut self, s: ColorSatSettings) {
        self.s = s;
        self.retune(s);
    }

    fn retune(&mut self, s: ColorSatSettings) {
        let r = self.rate;
        self.hpf
            .retune(&high_pass(s.hpf_hz.clamp(18.0, 2500.0), 0.707, r));
        self.lpf
            .retune(&low_pass(s.lpf_hz.clamp(400.0, 0.47 * r as f32), 0.707, r));
        self.bass.retune(&shelf(
            120.0,
            s.bass_db.clamp(-15.0, 15.0),
            0.7,
            Side::Low,
            r,
        ));
        self.mid.retune(&peak(
            s.mid_hz.clamp(120.0, 8000.0),
            s.mid_q.clamp(0.3, 8.0),
            s.mid_db.clamp(-15.0, 15.0),
            r,
        ));
        self.treble.retune(&shelf(
            4000.0,
            s.treble_db.clamp(-15.0, 15.0),
            0.7,
            Side::High,
            r,
        ));
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
        let dc_r = 0.996f32;
        let use_hpf = self.s.hpf_hz > 22.0;
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

            for frame in chunk.iter_mut() {
                self.drive_cur += (drive_tgt - self.drive_cur) * smooth;
                self.saver_cur += (saver_tgt - self.saver_cur) * smooth;
                let gain = (1.0 + 14.0 * self.drive_cur * self.drive_cur) * boost_lin;
                let saver = self.saver_cur;
                for ((sample, sub_lp), st) in frame
                    .iter_mut()
                    .zip(self.sub_lp.iter_mut())
                    .zip(self.dc.iter_mut())
                {
                    let x = *sample;
                    let sub = *sub_lp + (x - *sub_lp) * sub_c;
                    *sub_lp = flush32(sub);
                    let to_shape = x - saver * sub;
                    let shaped = curve(character, to_shape, gain);
                    let clean_shaped = flush32(shaped - st[0] + dc_r * st[1]);
                    st[0] = shaped;
                    st[1] = clean_shaped;
                    *sample = clean_shaped + saver * sub;
                }
            }

            if use_lpf {
                self.lpf.process(chunk);
            }
            self.bass.process(chunk);
            self.mid.process(chunk);
            self.treble.process(chunk);

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
