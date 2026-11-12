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
/// `h` and `k = 4R`; the loop band-pass's `gb`, `kb`, `hb` and its upper
/// share.
#[derive(Clone, Copy, Debug, Default)]
struct Coeffs {
    g: f64,
    h: f64,
    k: f64,
    gb: f64,
    kb: f64,
    hb: f64,
    share: f64,
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
        // The loop band-pass: edges `g/a` and `min(g·a, top)`, centred
        // between them, unity at its peak, scaled by the upper share.
        let wide = f64::from(s.loop_width) * r;
        let a = wide + (1.0 + wide * wide).sqrt();
        let (low, high) = (g / a, (g * a).min(f64::from(s.loop_top)));
        let gb = (low * high).sqrt();
        let kb = (low + high) / gb;
        let hb = 1.0 / (1.0 + kb * gb + gb * gb);
        let share = high / (low + high);
        Coeffs {
            g,
            h,
            k,
            gb,
            kb,
            hb,
            share,
        }
    }

    /// One frame at the running rate.
    fn tick(&mut self, frame: [f32; 2]) -> [f32; 2] {
        let s = self.s;
        let stages = usize::from(s.stages).clamp(1, MAX_STAGES);
        let fb = f64::from(s.feedback);
        let mix = f64::from(s.mix);
        let level = self.level;
        let sign = f64::from(s.sign);
        if self.until == 0 {
            // At the middle of the ticks it holds for.
            let mid = self.cycle + f64::from(CONTROL - 1) / 2.0 * f64::from(s.lfo_hz) / self.rate;
            let lfo = [1.0, -1.0].map(|side| self.lfo(mid + side * f64::from(s.phase)));
            self.held = lfo.map(|lfo| self.coeffs(lfo));
            self.until = CONTROL;
        }
        self.until -= 1;
        let c = self.held;
        let x = frame.map(f64::from);
        let used = self.stages.get_mut(..stages).unwrap_or_default();
        // The cascade as `A·u + B`.
        let (mut gain, mut offset) = ([1.0f64; 2], [0.0f64; 2]);
        for stage in used.iter() {
            for (((a, b), [i1, i2]), c) in gain.iter_mut().zip(&mut offset).zip(stage).zip(&c) {
                let pass = 1.0 - c.g * c.h * c.k;
                (*a, *b) = (*a * pass, *b * pass - c.h * (i1 - c.g * i2));
            }
        }
        // The loop solved for `u`, then the stages run on it.
        let mut u = [0.0f64; 2];
        let solve = (u.iter_mut().zip(&x).zip(&self.band).zip(&c)).zip(gain.iter().zip(&offset));
        for ((((u, &x), [b1, b2]), c), (&gain, &offset)) in solve {
            let loop_gain = sign * c.share * c.hb * c.gb * c.kb;
            *u = (x + fb * ((b1 - c.gb * b2) * c.hb + loop_gain * offset))
                / (1.0 - fb * loop_gain * gain);
        }
        let mut tail = u;
        for stage in used.iter_mut() {
            for ((tail, [i1, i2]), c) in tail.iter_mut().zip(stage).zip(&c) {
                let v = ((*tail * c.k - *i2) * c.g + *i1) * c.h;
                *tail -= v;
                *i1 = 2.0 * v - *i1;
                *i2 += 2.0 * c.g * v;
            }
        }
        let mut out = [0.0f32; 2];
        let rest = (out
            .iter_mut()
            .zip(&x)
            .zip(&mut self.band)
            .zip(&mut self.low))
        .zip(c.iter().zip(u.iter().zip(&tail)));
        for ((((y, &x), band), low), (c, (&u, &tail))) in rest {
            let tail = sign * tail;
            let [b1, b2] = *band;
            let v = ((c.share * tail * c.kb - b2) * c.gb + b1) * c.hb;
            *band = [2.0 * v - b1, b2 + 2.0 * c.gb * v];
            let wet = 0.5 * (u + tail);
            let sum = (x + (wet - x) * mix) * level;
            // The high-pass: what a trapezoidal one-pole low-pass leaves.
            let d = (sum - *low) * self.low_cut;
            let lp = d + *low;
            *low = d + lp;
            self.written = self.written.max(u.abs() as f32);
            *y = (sum - lp) as f32;
        }
        // Only the stages in use move; the rest were flushed when they ran.
        let moved = used.as_flattened_mut().as_flattened_mut();
        for v in moved.iter_mut().chain(self.band.as_flattened_mut()) {
            if v.abs() < 1e-20 {
                *v = 0.0;
            }
        }
        let next = self.cycle + f64::from(s.lfo_hz) / self.rate;
        self.cycle = next - next.floor();
        out
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
