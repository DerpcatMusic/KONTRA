//! The algorithmic reverb: one 8-line feedback delay network (Hadamard
//! feedback, a one-pole damping filter per line) behind a predelay and four
//! input allpasses. Formats load their own laws into [`Settings`].

use crate::fade::{Fade, flush};
use kontra_dsp_core::DelayLine;
use moose_dsp::time::coeff;

/// What a reverb sounds like, in physical units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// Time for the tail to fall 60 dB.
    pub seconds: f32,
    /// Room size: scales the delay lines, 1 is a mid-size room.
    pub size: f32,
    /// 0..=1: how much faster the highs die than the lows.
    pub damping: f32,
    /// 0..=1: input allpass gain over 0.7, the tail's early density.
    pub diffusion: f32,
    pub predelay_ms: f32,
    /// Output low-pass corner.
    pub high_cut_hz: f32,
    /// Output gain below 200 Hz.
    pub low_shelf_db: f32,
    /// 0 mono, 1 full width.
    pub stereo: f32,
    /// How far the lines' lengths swing (all but the first, half of them a
    /// quarter cycle apart), and how fast.
    pub modulation_ms: f32,
    pub modulation_hz: f32,
}

/// Mutually prime line lengths at size 1, ms.
const LINES_MS: [f32; 8] = [29.7, 37.1, 41.1, 43.7, 53.0, 59.9, 67.1, 79.3];
const DIFFUSERS_MS: [f32; 4] = [4.7, 3.6, 12.7, 9.3];
const SHELF_HZ: f32 = 200.0;
/// The largest size and predelay [`Fdn::set`] accepts; the lines are allocated for them.
pub(crate) const MAX_SIZE: f32 = 4.0;
pub(crate) const MAX_PREDELAY_MS: f32 = 2000.0;
/// The widest line swing [`Fdn::set`] accepts; the lines have room for it.
const MAX_MODULATION_MS: f32 = 5.0;
/// Gains, shelf and width glide over about this long.
const GLIDE_S: f32 = 0.01;

/// Delay samples for `ms` at `rate`, at least one.
fn samples(ms: f32, rate: f32) -> usize {
    (ms * 0.001 * rate).round().max(1.0) as usize
}

/// One-pole low-pass coefficient for `hz` at `rate`.
fn pole(hz: f32, rate: f32) -> f32 {
    (-std::f32::consts::TAU * hz.clamp(1.0, 0.49 * rate) / rate).exp()
}

pub struct Fdn {
    rate: f32,
    fade: Fade,
    predelay: DelayLine,
    diffusers: Vec<DelayLine>,
    diffusion: f32,
    lines: Vec<DelayLine>,
    /// Per line: feedback gain for the decay, and its damping filter state.
    gains: [f32; 8],
    damp: f32,
    lows: [f32; 8],
    high_cut: f32,
    /// Line swing in samples (peak to peak), its phase in cycles and step.
    swing: f32,
    cycle: f64,
    cycle_step: f64,
    shelf: f32,
    shelf_gain: f32,
    /// Output filter states, per channel: high cut, shelf.
    out: [[f32; 2]; 2],
    stereo: f32,
    /// Where the line gains, shelf gain and width glide to: a change that
    /// keeps the line lengths moves them over about one fade, not at once.
    to: Glide,
    ease: f32,
    /// Silence-sleep tracking: starts asleep (all lines zero); re-enters sleep
    /// once both input and internal lines stay below -140 dB for the full
    /// maximum active delay length.
    asleep: bool,
    quiet_frames: usize,
}

#[derive(Clone, Copy)]
struct Glide {
    gains: [f32; 8],
    shelf_gain: f32,
    stereo: f32,
}

impl Fdn {
    /// Not real-time: allocates the delay lines for the largest size and predelay.
    #[must_use]
    pub fn new(s: Settings, rate: u32) -> Self {
        let rate = rate.max(1) as f32;
        let mut fdn = Self {
            rate,
            fade: Fade::new(s, rate),
            predelay: DelayLine::new(samples(MAX_PREDELAY_MS, rate)),
            diffusers: DIFFUSERS_MS
                .iter()
                .map(|&ms| DelayLine::new(samples(ms, rate)))
                .collect(),
            diffusion: 0.0,
            lines: LINES_MS
                .iter()
                .map(|&ms| DelayLine::new(samples(ms * MAX_SIZE + 2.0 * MAX_MODULATION_MS, rate)))
                .collect(),
            gains: [0.0; 8],
            damp: 0.0,
            lows: [0.0; 8],
            high_cut: 0.0,
            swing: 0.0,
            cycle: 0.0,
            cycle_step: 0.0,
            shelf: 0.0,
            shelf_gain: 0.0,
            out: [[0.0; 2]; 2],
            stereo: 0.0,
            to: Glide {
                gains: [0.0; 8],
                shelf_gain: 0.0,
                stereo: 0.0,
            },
            ease: coeff(GLIDE_S * rate),
            asleep: true,
            quiet_frames: 0,
        };
        fdn.apply(s);
        fdn.asleep = true;
        fdn.quiet_frames = 0;
        (fdn.gains, fdn.shelf_gain, fdn.stereo) = (fdn.to.gains, fdn.to.shelf_gain, fdn.to.stereo);
        fdn
    }

    /// Changes every setting while it plays, by the [`Fade`] rule. Never allocates.
    pub fn set(&mut self, s: Settings) {
        if let Some(s) = self.fade.set(s) {
            self.apply(s);
        }
    }

    /// Clears all delay lines, damping/output states, and fades back to silence.
    pub fn reset(&mut self) {
        self.fade.reset();
        self.predelay.reset();
        for d in &mut self.diffusers {
            d.reset();
        }
        for line in &mut self.lines {
            line.reset();
        }
        self.lows = [0.0; 8];
        self.out = [[0.0; 2]; 2];
        self.cycle = 0.0;
        (self.gains, self.shelf_gain, self.stereo) =
            (self.to.gains, self.to.shelf_gain, self.to.stereo);
        self.asleep = true;
        self.quiet_frames = 0;
    }

    /// Writes `s` into the lines and coefficients. Never allocates.
    fn apply(&mut self, s: Settings) {
        let rate = self.rate;
        let size = s.size.clamp(0.25, MAX_SIZE);
        let seconds = s.seconds.max(0.05);
        let mut grew = false;
        for ((line, &ms), g) in self.lines.iter_mut().zip(&LINES_MS).zip(&mut self.to.gains) {
            let len = samples(ms * size, rate);
            grew |= line.set_len(len);
            *g = 10f32.powf(-3.0 * len as f32 / (seconds * rate));
        }
        grew |= self
            .predelay
            .set_len(samples(s.predelay_ms.clamp(0.0, MAX_PREDELAY_MS), rate));
        if grew {
            // A longer delay window may expose earlier non-zero buffer slots: wake to flush naturally.
            self.asleep = false;
            self.quiet_frames = 0;
        }
        self.diffusion = 0.7 * s.diffusion.clamp(0.0, 1.0);
        self.damp = 0.85 * s.damping.clamp(0.0, 1.0);
        self.high_cut = pole(s.high_cut_hz, rate);
        self.swing = 2.0 * s.modulation_ms.clamp(0.0, MAX_MODULATION_MS) * 0.001 * rate;
        self.cycle_step = f64::from(s.modulation_hz.max(0.0) / rate);
        self.shelf = pole(SHELF_HZ, rate);
        self.to.shelf_gain = 10f32.powf(s.low_shelf_db / 20.0) - 1.0;
        self.to.stereo = s.stereo.clamp(0.0, 1.0);
    }

    /// This frame's wet gain. At the bottom of a fade-out the pending settings
    /// apply, then the gain climbs back.
    fn duck(&mut self) -> f32 {
        let (g, pending) = self.fade.tick();
        if let Some(s) = pending {
            self.apply(s);
        }
        g
    }

    #[inline]
    fn glides_settled(&self) -> bool {
        let k = self.ease;
        let still = |a: f32, b: f32| (a + (b - a) * k).to_bits() == a.to_bits();
        still(self.shelf_gain, self.to.shelf_gain)
            && still(self.stereo, self.to.stereo)
            && self
                .gains
                .iter()
                .zip(self.to.gains)
                .all(|(&g, to)| still(g, to))
    }

    /// Replaces `io` with the reverb of its mono sum (the wet path only).
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        if io.is_empty() {
            return;
        }
        let input_silent = io.iter().all(|f| f[0].abs() < 1e-7 && f[1].abs() < 1e-7);
        if self.asleep && input_silent {
            if !self.fade.is_settled() {
                for _ in 0..io.len() {
                    self.duck();
                }
            }
            if !self.glides_settled() {
                let decay = (1.0 - self.ease).powi(io.len() as i32);
                for (g, to) in self.gains.iter_mut().zip(self.to.gains) {
                    *g = to + (*g - to) * decay;
                }
                self.shelf_gain =
                    self.to.shelf_gain + (self.shelf_gain - self.to.shelf_gain) * decay;
                self.stereo = self.to.stereo + (self.stereo - self.to.stereo) * decay;
            }
            self.cycle = (self.cycle + self.cycle_step * io.len() as f64).fract();
            io.fill([0.0; 2]);
            return;
        }
        self.asleep = false;

        let settled = self.fade.is_settled() && self.glides_settled();
        let fixed_wet = self.fade.gain();
        let modulated = self.swing > 0.0;
        let (mut sin_p, mut cos_p) = if modulated {
            (std::f64::consts::TAU * self.cycle).sin_cos()
        } else {
            (0.0, 1.0)
        };
        let (step_s, step_c) = if modulated {
            (std::f64::consts::TAU * self.cycle_step).sin_cos()
        } else {
            (0.0, 1.0)
        };
        let mut block_peak = 0.0f32;
        for frame in io.iter_mut() {
            let wet = if settled {
                fixed_wet
            } else {
                let w = self.duck();
                let k = self.ease;
                for (g, to) in self.gains.iter_mut().zip(self.to.gains) {
                    *g += (to - *g) * k;
                }
                self.shelf_gain += (self.to.shelf_gain - self.shelf_gain) * k;
                self.stereo += (self.to.stereo - self.stereo) * k;
                w
            };
            let [l, r] = *frame;
            let mut x = self.predelay.read_len();
            self.predelay.push(0.5 * (l + r));
            // Schroeder allpasses: y = −g·x + d, d' = x + g·y.
            let diff = self.diffusion;
            for d in &mut self.diffusers {
                let y = d.read_len() - diff * x;
                d.push(x + diff * y);
                x = y;
            }
            let mut y = [0.0f32; 8];
            let damp = self.damp;
            if modulated {
                let s1 = self.swing * (0.5 + 0.5 * sin_p as f32);
                let s2 = self.swing * (0.5 + 0.5 * cos_p as f32);
                let next_s = sin_p * step_c + cos_p * step_s;
                let next_c = cos_p * step_c - sin_p * step_s;
                sin_p = next_s;
                cos_p = next_c;
                let offsets = [0.0, s2, s1, s2, s1, s2, s1, s2];
                for (i, ((v, (line, low)), (g, off))) in y
                    .iter_mut()
                    .zip(self.lines.iter().zip(&mut self.lows))
                    .zip(self.gains.into_iter().zip(offsets))
                    .enumerate()
                {
                    let out = if i == 0 {
                        line.read_len()
                    } else {
                        line.read_late(off)
                    };
                    *low = out + damp * (*low - out);
                    *v = *low * g;
                }
            } else {
                for ((v, (line, low)), g) in y
                    .iter_mut()
                    .zip(self.lines.iter().zip(&mut self.lows))
                    .zip(self.gains)
                {
                    let out = line.read_len();
                    *low = out + damp * (*low - out);
                    *v = *low * g;
                }
            }
            let [a, b, c, d, e, f, g, h] = y;
            let (left, right) = (a - c + e - g, b - d + f - h);
            hadamard(&mut y);
            const SIGNS: [f32; 8] = [1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0];
            for ((line, v), sign) in self.lines.iter_mut().zip(y).zip(SIGNS) {
                let w = flush(v + sign * x);
                block_peak = block_peak.max(w.abs());
                line.push(w);
            }
            let (mid, side) = (0.5 * (left + right), 0.5 * (left - right) * self.stereo);
            let hc = self.high_cut;
            let sh = self.shelf;
            let sg = self.shelf_gain;
            let wet_half = wet * 0.5;
            let vl = mid + side;
            let vr = mid - side;
            let cut_l = vl + hc * (self.out[0][0] - vl);
            let low_l = cut_l + sh * (self.out[0][1] - cut_l);
            self.out[0] = [cut_l, low_l];
            let cut_r = vr + hc * (self.out[1][0] - vr);
            let low_r = cut_r + sh * (self.out[1][1] - cut_r);
            self.out[1] = [cut_r, low_r];
            let out_l = wet_half * (cut_l + sg * low_l);
            let out_r = wet_half * (cut_r + sg * low_r);
            block_peak = block_peak.max(out_l.abs()).max(out_r.abs());
            *frame = [out_l, out_r];
        }
        self.cycle = (self.cycle + self.cycle_step * io.len() as f64).fract();
        for s in self.lows.iter_mut().chain(self.out.as_flattened_mut()) {
            if s.abs() < 1e-20 {
                *s = 0.0;
            }
        }
        if input_silent
            && block_peak < 1e-7
            && self.lows.iter().all(|v| v.abs() < 1e-7)
            && self.out.iter().flatten().all(|v| v.abs() < 1e-7)
        {
            self.quiet_frames = self.quiet_frames.saturating_add(io.len());
            let max_line = self.lines.iter().map(DelayLine::len).max().unwrap_or(0);
            let diff_sum: usize = self.diffusers.iter().map(DelayLine::len).sum();
            let max_mem =
                self.predelay.len() + diff_sum + max_line + self.swing.ceil() as usize + 8;
            if self.quiet_frames >= max_mem {
                self.asleep = true;
            }
        } else {
            self.quiet_frames = 0;
        }
    }
}

/// The orthonormal 8-point Walsh–Hadamard transform, in place (unrolled 3-stage butterfly).
#[inline(always)]
fn hadamard(v: &mut [f32; 8]) {
    let [a0, a1, a2, a3, a4, a5, a6, a7] = *v;
    let (b0, b1) = (a0 + a1, a0 - a1);
    let (b2, b3) = (a2 + a3, a2 - a3);
    let (b4, b5) = (a4 + a5, a4 - a5);
    let (b6, b7) = (a6 + a7, a6 - a7);

    let (c0, c2) = (b0 + b2, b0 - b2);
    let (c1, c3) = (b1 + b3, b1 - b3);
    let (c4, c6) = (b4 + b6, b4 - b6);
    let (c5, c7) = (b5 + b7, b5 - b7);

    const SCALE: f32 = std::f32::consts::FRAC_1_SQRT_2 * 0.5;
    *v = [
        (c0 + c4) * SCALE,
        (c1 + c5) * SCALE,
        (c2 + c6) * SCALE,
        (c3 + c7) * SCALE,
        (c0 - c4) * SCALE,
        (c1 - c5) * SCALE,
        (c2 - c6) * SCALE,
        (c3 - c7) * SCALE,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_settings() -> Settings {
        Settings {
            seconds: 0.4,
            size: 0.6,
            damping: 0.5,
            diffusion: 0.7,
            predelay_ms: 5.0,
            high_cut_hz: 8_000.0,
            low_shelf_db: 0.0,
            stereo: 1.0,
            modulation_ms: 0.8,
            modulation_hz: 0.7,
        }
    }

    #[test]
    fn fdn_sleeps_on_silence_and_wakes_within_120db_of_always_awake() {
        let rate = 48_000u32;
        let s = test_settings();
        let mut sleeping = Fdn::new(s, rate);
        let mut awake = Fdn::new(s, rate);
        assert!(sleeping.asleep, "fresh Fdn starts asleep");

        // Feed burst -> 2.5s silence (enters sleep) -> second burst.
        let total = 3 * rate as usize;
        let mut buf_a = vec![[0.0f32; 2]; total];
        for (i, frame) in buf_a.iter_mut().enumerate() {
            if i < 2048 || (i >= 2 * rate as usize + 24_000 && i < 2 * rate as usize + 26_048) {
                let x = (i as f32 * 0.13).sin() * 0.5;
                *frame = [x, -x * 0.7];
            }
        }
        let mut buf_b = buf_a.clone();
        let mut saw_sleep = false;
        for (ca, cb) in buf_a.chunks_mut(256).zip(buf_b.chunks_mut(256)) {
            sleeping.process(ca);
            if sleeping.asleep {
                saw_sleep = true;
            }
            awake.asleep = false;
            awake.quiet_frames = 0;
            awake.process(cb);
        }
        assert!(saw_sleep, "Fdn should enter sleep after tail decays");
        let worst = buf_a
            .iter()
            .flatten()
            .zip(buf_b.iter().flatten())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        let peak = buf_b.iter().flatten().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(
            peak > 0.05 && worst < 1e-5 * peak,
            "sleep/wake null error {worst:e} (peak {peak:e})"
        );
    }
}
