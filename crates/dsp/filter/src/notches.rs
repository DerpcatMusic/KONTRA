//! A notch phaser: `stages` trapezoidal state-variable allpasses
//! (`x − 4R·band`, damping `2R`) at one LFO-swept corner, inside one loop
//! solved without a delay through its own band-pass, averaged with its
//! input, then mixed with the dry signal, its level held by
//! `1/√(mix/(1 − fb) + 1 − mix)`, and high-passed. It runs at the rate
//! nearest `internal_hz` by octaves (twice the rate at 44.1 or 48 kHz).
//!
//! Per channel and frame: the LFO (a rounded triangle, warped, scaled)
//! moves the corner by octaves and the stages' damping by `log₂` steps;
//! the cascade's response to its input is `A·u + B`, so the loop
//! `u = x + fb·band(A·u + B)` is solved for `u` before the stages run.

use kontra_dsp_core::Halfband;

/// Ticks the LFO and the coefficients hold for.
const CONTROL: u32 = 8;

/// The most allpass stages it runs.
pub const MAX_STAGES: usize = 12;

/// The corner's prewarp: `tan` by its series to `w⁵`, straight on past
/// `w = 0.642171` with slope 1.5257571 (the reference's own).
fn tan(w: f64) -> f64 {
    const KNEE: f64 = 0.642_171;
    let c = w.clamp(1e-6, KNEE);
    let c2 = c * c;
    c + c * c2 / 3.0 + 2.0 / 15.0 * c * c2 * c2 + 1.525_757_1 * (w - KNEE).max(0.0)
}

/// A notch phaser in physical units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NotchSettings {
    /// 1..=[`MAX_STAGES`].
    pub stages: u8,
    /// The corner with the LFO at 0, and the LFO's swing of it, octaves.
    pub hz: f32,
    pub octaves: f32,
    /// The stages' damping is `2·2^r`, `r = spread + spread_swing·lfo`
    /// held within `±spread_limit`.
    pub spread: f32,
    pub spread_swing: f32,
    pub spread_limit: f32,
    /// The loop gain. The loop's band-pass spans the corner `÷a` to `·a`,
    /// `a = r + √(1 + r²)`, `r = loop_width·2^r`, its top edge at most
    /// `loop_top` as `tan(π·f/rate)` at the running rate.
    pub feedback: f32,
    pub loop_width: f32,
    pub loop_top: f32,
    /// The LFO: rate, rounding (0: a triangle), depth, warp and how far the
    /// channels' phases sit either side of it, cycles.
    pub lfo_hz: f32,
    pub rounding: f32,
    pub depth: f32,
    pub warp: f32,
    pub phase: f32,
    /// The stages' sign in the loop and the sum.
    pub sign: f32,
    pub mix: f32,
    /// The output's first-order high-pass.
    pub low_cut_hz: f32,
    /// The rate it runs nearest to, by octaves.
    pub internal_hz: f32,
}

/// One channel's coefficients at this frame's LFO: the stages' corner `g`,
/// `h`, `k = 4R`, `pass = 1 - g*h*k`, `pass_pow = pass^stages`; the loop
/// band-pass's `gb`, `kb`, `hb`, its upper share, and `loop_gain`.
#[derive(Clone, Copy, Debug, Default)]
struct Coeffs {
    g: f64,
    h: f64,
    k: f64,
    pass: f64,
    pass_pow: f64,
    gb: f64,
    kb: f64,
    hb: f64,
    share: f64,
    loop_gain: f64,
}

/// Everything but the resampler.
#[derive(Debug)]
struct Core {
    s: NotchSettings,
    /// The running rate.
    rate: f64,
    cycle: f64,
    /// The states, both channels side by side (their arithmetic is the
    /// same, so they run as pairs): each stage's two integrators, by stage
    /// then channel; the loop band-pass's two integrators; the high-pass's
    /// low-pass state.
    stages: [[[f64; 2]; 2]; MAX_STAGES],
    band: [[f64; 2]; 2],
    low: [f64; 2],
    /// Both channels' coefficients, held for [`CONTROL`] ticks.
    held: [Coeffs; 2],
    until: u32,
    /// The high-pass coefficient `G/(1 + G)`, `G = tan(π·f/rate)`.
    low_cut: f64,
    /// Of the settings alone: `expm1(warp)` and the output level.
    warp_m1: f64,
    level: f64,
    written: f32,
}

impl Core {
    /// The LFO at `cycles`: a triangle from 0 (rising) whose corners round
    /// as `1 + ρ − √((1 + 2ρ)t² + ρ²)`, warped about its peaks by
    /// `expm1(w·u)/expm1(w)` (its series to `u³`), times the depth.
    fn lfo(&self, cycles: f64) -> f64 {
        let s = &self.s;
        let rho = f64::from(s.rounding);
        let centred = cycles - cycles.round();
        let t = 1.0 - 4.0 * centred.abs();
        let height = 1.0 + rho - ((1.0 + 2.0 * rho) * t * t + rho * rho).sqrt();
        let side = if centred.abs() < 0.25 { -1.0 } else { 1.0 };
        let u = 0.5 * (height * centred.signum() * side + 1.0);
        let w = f64::from(s.warp);
        let x = w * u;
        let warped = match w > 0.0 {
            true => u * (1.0 + x / 2.0 + x * x / 6.0) * w / self.warp_m1,
            false => u,
        };
        side * (2.0 * warped - 1.0) * f64::from(s.depth)
    }

    /// One channel's coefficients with the LFO at `lfo`.
    fn coeffs(&self, lfo: f64) -> Coeffs {
        let s = &self.s;
        let limit = f64::from(s.spread_limit);
        // The stages: corner `g`, damping `2R`.
        let hz = f64::from(s.hz) * (f64::from(s.octaves) * lfo).exp2();
        let g = tan(std::f64::consts::PI * hz / self.rate);
        let r = (f64::from(s.spread) + f64::from(s.spread_swing) * lfo)
            .clamp(-limit, limit)
            .exp2();
        let h = 1.0 / (1.0 + 2.0 * r * g + g * g);
        let k = 4.0 * r;
        let pass = 1.0 - g * h * k;
        let stages = usize::from(s.stages).clamp(1, MAX_STAGES);
        let mut pass_pow = 1.0;
        for _ in 0..stages {
            pass_pow *= pass;
        }
        // The loop band-pass: edges `g/a` and `min(g·a, top)`, centred
        // between them, unity at its peak, scaled by the upper share.
        let wide = f64::from(s.loop_width) * r;
        let a = wide + (1.0 + wide * wide).sqrt();
        let (low, high) = (g / a, (g * a).min(f64::from(s.loop_top)));
        let gb = (low * high).sqrt();
        let kb = (low + high) / gb;
        let hb = 1.0 / (1.0 + kb * gb + gb * gb);
        let share = high / (low + high);
        let loop_gain = f64::from(s.sign) * share * hb * gb * kb;
        Coeffs {
            g,
            h,
            k,
            pass,
            pass_pow,
            gb,
            kb,
            hb,
            share,
            loop_gain,
        }
    }

    /// One frame at the running rate.
    #[inline]
    fn tick(&mut self, frame: [f32; 2]) -> [f32; 2] {
        let s = self.s;
        let stages = usize::from(s.stages).clamp(1, MAX_STAGES);
        let fb = f64::from(s.feedback);
        let mix = f64::from(s.mix);
        let level = self.level;
        let sign = f64::from(s.sign);
        let lfo_inc = f64::from(s.lfo_hz) / self.rate;
        if self.until == 0 {
            // At the middle of the ticks it holds for.
            let mid = self.cycle + f64::from(CONTROL - 1) / 2.0 * lfo_inc;
            let lfo = [1.0, -1.0].map(|side| self.lfo(mid + side * f64::from(s.phase)));
            self.held = lfo.map(|lfo| self.coeffs(lfo));
            self.until = CONTROL;
        }
        self.until -= 1;
        let [c0, c1] = self.held;
        let [x0, x1] = [f64::from(frame[0]), f64::from(frame[1])];
        let used = self.stages.get_mut(..stages).unwrap_or_default();
        // The cascade as `A·u + B` (`A = pass_pow` precomputed per control tick).
        let (mut off0, mut off1) = (0.0f64, 0.0f64);
        for stage in used.iter() {
            off0 = off0 * c0.pass - c0.h * (stage[0][0] - c0.g * stage[0][1]);
            off1 = off1 * c1.pass - c1.h * (stage[1][0] - c1.g * stage[1][1]);
        }
        // The loop solved for `u`, then the stages run on it.
        let [b0_1, b0_2] = self.band[0];
        let [b1_1, b1_2] = self.band[1];
        let u0 = (x0 + fb * ((b0_1 - c0.gb * b0_2) * c0.hb + c0.loop_gain * off0))
            / (1.0 - fb * c0.loop_gain * c0.pass_pow);
        let u1 = (x1 + fb * ((b1_1 - c1.gb * b1_2) * c1.hb + c1.loop_gain * off1))
            / (1.0 - fb * c1.loop_gain * c1.pass_pow);
        let (mut t0, mut t1) = (u0, u1);
        for stage in used.iter_mut() {
            let v0 = ((t0 * c0.k - stage[0][1]) * c0.g + stage[0][0]) * c0.h;
            t0 -= v0;
            let s00 = 2.0 * v0 - stage[0][0];
            let s01 = stage[0][1] + 2.0 * c0.g * v0;
            stage[0][0] = if s00.abs() < 1e-20 { 0.0 } else { s00 };
            stage[0][1] = if s01.abs() < 1e-20 { 0.0 } else { s01 };

            let v1 = ((t1 * c1.k - stage[1][1]) * c1.g + stage[1][0]) * c1.h;
            t1 -= v1;
            let s10 = 2.0 * v1 - stage[1][0];
            let s11 = stage[1][1] + 2.0 * c1.g * v1;
            stage[1][0] = if s10.abs() < 1e-20 { 0.0 } else { s10 };
            stage[1][1] = if s11.abs() < 1e-20 { 0.0 } else { s11 };
        }
        let tail0 = sign * t0;
        let tail1 = sign * t1;
        let vb0 = ((c0.share * tail0 * c0.kb - b0_2) * c0.gb + b0_1) * c0.hb;
        let vb1 = ((c1.share * tail1 * c1.kb - b1_2) * c1.gb + b1_1) * c1.hb;
        let nb00 = 2.0 * vb0 - b0_1;
        let nb01 = b0_2 + 2.0 * c0.gb * vb0;
        let nb10 = 2.0 * vb1 - b1_1;
        let nb11 = b1_2 + 2.0 * c1.gb * vb1;
        self.band[0][0] = if nb00.abs() < 1e-20 { 0.0 } else { nb00 };
        self.band[0][1] = if nb01.abs() < 1e-20 { 0.0 } else { nb01 };
        self.band[1][0] = if nb10.abs() < 1e-20 { 0.0 } else { nb10 };
        self.band[1][1] = if nb11.abs() < 1e-20 { 0.0 } else { nb11 };

        let wet0 = 0.5 * (u0 + tail0);
        let wet1 = 0.5 * (u1 + tail1);
        let sum0 = (x0 + (wet0 - x0) * mix) * level;
        let sum1 = (x1 + (wet1 - x1) * mix) * level;
        let d0 = (sum0 - self.low[0]) * self.low_cut;
        let d1 = (sum1 - self.low[1]) * self.low_cut;
        let lp0 = d0 + self.low[0];
        let lp1 = d1 + self.low[1];
        self.low[0] = d0 + lp0;
        self.low[1] = d1 + lp1;
        self.written = self.written.max(u0.abs() as f32).max(u1.abs() as f32);
        let next = self.cycle + lfo_inc;
        self.cycle = next - next.floor();
        [(sum0 - lp0) as f32, (sum1 - lp1) as f32]
    }

    fn set(&mut self, s: NotchSettings) {
        self.s = s;
        self.until = 0;
        let g = (std::f64::consts::PI * f64::from(s.low_cut_hz) / self.rate).tan();
        self.low_cut = g / (1.0 + g);
        self.warp_m1 = f64::from(s.warp).exp_m1();
        let (fb, mix) = (f64::from(s.feedback), f64::from(s.mix));
        self.level = 1.0 / (mix / (1.0 - fb) + 1.0 - mix).sqrt();
    }
}

/// See [`NotchSettings`]; it never allocates.
#[derive(Debug)]
pub struct Notches {
    core: Core,
    /// Twice the rate, when that is the octave nearest `internal_hz`.
    /// ponytail: a quarter of it (at 32 kHz and below) runs at twice, and
    /// above 127 kHz it runs at the host rate, not half.
    halfband: Option<Halfband>,
}

impl Notches {
    #[must_use]
    pub fn new(s: NotchSettings, rate: u32) -> Self {
        let octaves = (f64::from(s.internal_hz) / f64::from(rate.max(1)))
            .log2()
            .round();
        let halfband = (octaves >= 1.0).then(|| Halfband::new(rate));
        let factor = if halfband.is_some() { 2.0 } else { 1.0 };
        let mut core = Core {
            s,
            rate: f64::from(rate.max(1)) * factor,
            cycle: 0.0,
            stages: [[[0.0; 2]; 2]; MAX_STAGES],
            band: [[0.0; 2]; 2],
            low: [0.0; 2],
            held: [Coeffs::default(); 2],
            until: 0,
            low_cut: 0.0,
            warp_m1: 0.0,
            level: 0.0,
            written: 0.0,
        };
        core.set(s);
        Self { core, halfband }
    }

    pub fn set(&mut self, s: NotchSettings) {
        self.core.set(s);
    }

    /// Replaces `io` with the effect's output (its dry share included).
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        self.core.written = 0.0;
        let core = &mut self.core;
        match &mut self.halfband {
            Some(h) => h.process(io, |f| core.tick(f)),
            None => io.iter_mut().for_each(|f| *f = core.tick(*f)),
        }
    }

    /// Host frames after which silent input leaves it silent.
    // ponytail: 2 s covers its longest ring (loop gain 0.95 through a
    // band-pass of Q ½ at most); derive it if a slower tail turns up.
    #[must_use]
    pub fn memory(&self) -> usize {
        let factor = if self.halfband.is_some() { 2.0 } else { 1.0 };
        (2.0 * self.core.rate / factor) as usize
    }

    /// The loudest loop value the last [`Self::process`] ran.
    #[must_use]
    pub fn written(&self) -> f32 {
        self.core.written
    }

    /// Silent frames: the LFO moves on as [`Self::process`] would move it.
    pub fn idle(&mut self, frames: usize) {
        let factor = if self.halfband.is_some() { 2.0 } else { 1.0 };
        let next = self.core.cycle
            + f64::from(self.core.s.lfo_hz) / self.core.rate * factor * frames as f64;
        self.core.cycle = next - next.floor();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A still phaser of `stages` at 1 kHz.
    fn settings(stages: u8, feedback: f32) -> NotchSettings {
        NotchSettings {
            stages,
            hz: 1000.0,
            octaves: 0.0,
            spread: 0.0,
            spread_swing: 0.0,
            spread_limit: 2.7,
            feedback,
            loop_width: 1.0 / (0.25 / f32::from(stages)).tan(),
            loop_top: 0.1,
            lfo_hz: 0.0,
            rounding: 0.0,
            depth: 0.0,
            warp: 0.1,
            phase: 0.0,
            sign: 1.0,
            mix: 1.0,
            low_cut_hz: 5.0,
            internal_hz: 90_000.0,
        }
    }

    fn sine(hz: f32, frames: usize) -> Vec<[f32; 2]> {
        (0..frames)
            .map(|i| {
                let x = (std::f32::consts::TAU * hz * i as f32 / 48_000.0).sin();
                [x, x]
            })
            .collect()
    }

    /// The settled level of a sine at `hz` through a still phaser.
    fn gain(hz: f32) -> f32 {
        let mut n = Notches::new(settings(2, 0.0), 48_000);
        let mut io = sine(hz, 48_000);
        n.process(&mut io);
        let tail = &io[24_000..];
        (tail.iter().map(|f| f[0] * f[0]).sum::<f32>() / tail.len() as f32 * 2.0).sqrt()
    }

    #[test]
    fn two_stages_notch_either_side_of_the_corner_and_the_loop_stays_bounded() {
        // Two allpasses at Q ½ turn 90° each at corner·(√2 ∓ 1): ½(x + AP)
        // cancels there; at the corner they turn 360° and pass.
        for hz in [414.2, 2414.2] {
            assert!(gain(hz) < 0.03, "{hz}: {}", gain(hz));
        }
        assert!((gain(1000.0) - 1.0).abs() < 0.01, "{}", gain(1000.0));
        // The loop, solved per sample, at every stage count: feedback moved
        // to its top (0.95) over the first second while the LFO sweeps the
        // corner ±4.7 octaves and the damping across its whole range.
        for stages in 1..=MAX_STAGES as u8 {
            let swept = |feedback| NotchSettings {
                octaves: 4.67,
                spread_swing: 2.7,
                lfo_hz: 2.3,
                depth: 1.0,
                ..settings(stages, feedback)
            };
            let mut n = Notches::new(swept(0.0), 48_000);
            let mut io = sine(500.0, 4 * 48_000);
            for (k, block) in io.chunks_mut(64).enumerate() {
                n.set(swept((k as f32 / 750.0).min(1.0) * 0.95));
                n.process(block);
            }
            let peak = |s: &[[f32; 2]]| s.iter().flatten().fold(0.0f32, |m, v| m.max(v.abs()));
            let (early, late) = io.split_at(3 * 48_000);
            let (early, late) = (peak(early.get(48_000..).unwrap_or_default()), peak(late));
            assert!(
                late.is_finite() && late < 8.0 && late <= 1.05 * early,
                "{stages} stages: {early} then {late}"
            );
        }
    }
}
