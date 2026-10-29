//! One modulated delay line (an echo, a chorus, a flanger) and one phaser;
//! design in docs/architecture-v3/03-DSP.md. Formats load their laws into
//! [`Settings`] and [`PhaserSettings`]. Both put out the wet signal only.

use std::f64::consts::TAU;

use kontra_dsp_core::{Biquad, Side, high_pass, shelf};

/// The longest line [`ModDelay::new`] allocates, ms.
pub const MAX_MS: f32 = 6000.0;
/// The time a line takes to glide to a new length, so a change never clicks.
const GLIDE_S: f32 = 0.02;
/// How far the second head's LFO runs ahead, cycles.
const SECOND_AHEAD: f64 = 0.015;

/// A delay line in physical units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// The head's delay at the LFO's centre.
    pub ms: f32,
    /// The LFO's swing either side of `ms`.
    pub depth_ms: f32,
    /// A second head at half the delay, its LFO slightly ahead.
    pub second_head: bool,
    /// The input channels into the lines; row `j` feeds line `j`.
    pub input: [[f32; 2]; 2],
    /// The heads back into the lines.
    pub feedback: [[f32; 2]; 2],
    /// The heads to the output.
    pub output: [[f32; 2]; 2],
    /// A one-pole low-pass on each head; `f32::INFINITY` for none.
    pub damping_hz: f32,
    /// A one-pole high-pass on each head; 0 for none.
    pub low_cut_hz: f32,
    /// The low cut is a second-order high-pass, Q ½, not the one-pole.
    pub steep_low_cut: bool,
    /// A high shelf on the heads, inside the loop.
    pub high_shelf: Option<Shelf>,
    /// How much shorter the left line runs and longer the right, ms.
    pub spread_ms: f32,
    /// The LFO's speed.
    pub lfo_hz: f32,
    /// Cycles the right LFO runs ahead of the left.
    pub phase: f32,
}

/// A high shelf: `gain_db` above `hz`, its knee the cookbook's S.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shelf {
    pub hz: f32,
    pub gain_db: f32,
    pub slope: f32,
}

/// The LFO at `cycles`: a sine that falls first.
fn lfo(cycles: f64) -> f32 {
    -sine(cycles) as f32
}

/// `sin(2π·cycles)` from an odd polynomial on the quarter wave (Taylor to
/// x¹¹, error under 1e-7): a few multiplies where libm's `sin` is the
/// largest cost of a modulated delay.
#[must_use]
pub fn sine(cycles: f64) -> f64 {
    let mut t = cycles - cycles.round();
    if t > 0.25 {
        t = 0.5 - t;
    } else if t < -0.25 {
        t = -0.5 - t;
    }
    let x = TAU * t;
    let x2 = x * x;
    let p = -1.0 / 39_916_800.0 * x2 + 1.0 / 362_880.0;
    let p = p * x2 - 1.0 / 5_040.0;
    let p = p * x2 + 1.0 / 120.0;
    let p = p * x2 - 1.0 / 6.0;
    x * (p * x2 + 1.0)
}

/// A triangle at `cycles` that falls first, as [`lfo`].
fn triangle(cycles: f64) -> f32 {
    (4.0 * ((cycles + 0.25).rem_euclid(1.0) - 0.5).abs() - 1.0) as f32
}

/// Flushes a value that would decay into the subnormal range.
fn flush(x: f32) -> f32 {
    if x.abs() < 1e-20 { 0.0 } else { x }
}

/// One-pole coefficient for a corner at `hz`; 1 passes everything.
fn one_pole(hz: f32, rate: f32) -> f32 {
    1.0 - (-std::f32::consts::TAU * hz.max(0.0) / rate).exp()
}

/// `m·x`.
fn apply(m: [[f32; 2]; 2], x: [f32; 2]) -> [f32; 2] {
    m.map(|[a, b]| a * x[0] + b * x[1])
}

/// A stereo delay line with one or two LFO-swept heads whose filtered sum
/// matrices mix back into the lines and out; see [`Settings`].
#[derive(Debug)]
pub struct ModDelay {
    s: Settings,
    rate: f32,
    ring: Box<[[f32; 2]]>,
    /// Where the next frame goes.
    at: usize,
    /// The head's delay as it glides, frames; `None` until the first frame.
    delay: Option<f32>,
    glide: f32,
    damping: f32,
    low_cut: f32,
    /// The left LFO's phase, cycles.
    cycle: f64,
    /// Each head's low-pass state, per channel.
    lp: [[f32; 2]; 2],
    /// Each head's high-pass (its own low-pass) state, per channel.
    hp: [[f32; 2]; 2],
    /// The steep low cut and the shelf, on the heads' sum.
    cut: Option<Biquad>,
    shelf: Option<Biquad>,
}

impl ModDelay {
    /// Room for `max_ms` of delay plus the swing (at most [`MAX_MS`]). Not
    /// real-time: allocates.
    #[must_use]
    pub fn new(s: Settings, rate: u32, max_ms: f32) -> Self {
        let rate = rate as f32;
        let ms = (max_ms.max(s.ms) + s.depth_ms.abs() + s.spread_ms.abs()).clamp(0.0, MAX_MS);
        let frames = (ms * 0.001 * rate) as usize + 8;
        let mut d = Self {
            s,
            rate,
            ring: vec![[0.0; 2]; frames].into(),
            at: 0,
            delay: None,
            glide: 1.0 - (-1.0 / (GLIDE_S * rate)).exp(),
            damping: 1.0,
            low_cut: 0.0,
            cycle: 0.0,
            lp: [[0.0; 2]; 2],
            hp: [[0.0; 2]; 2],
            cut: None,
            shelf: None,
        };
        d.set(s);
        d
    }

    /// Changes the settings while it plays; never allocates. The delay
    /// glides to its new length.
    pub fn set(&mut self, s: Settings) {
        self.s = s;
        self.damping = one_pole(s.damping_hz, self.rate).clamp(0.0, 1.0);
        let rate = self.rate as u32;
        let steep = s.steep_low_cut && s.low_cut_hz > 0.0;
        let one_pole_hz = if s.steep_low_cut { 0.0 } else { s.low_cut_hz };
        self.low_cut = one_pole(one_pole_hz, self.rate).clamp(0.0, 1.0);
        let cut = steep.then(|| high_pass(s.low_cut_hz, 0.5, rate));
        let tone = s
            .high_shelf
            .map(|t| shelf(t.hz, t.gain_db, t.slope, Side::High, rate));
        // A moved filter keeps its state, so a sweep never clicks.
        for (old, new) in [(&mut self.cut, cut), (&mut self.shelf, tone)] {
            match (old, new) {
                (Some(old), Some(new)) => old.retune(&new),
                (old, new) => *old = new,
            }
        }
    }

    /// The sample `d` frames back (at least 2), 4-point cubic.
    fn read(&self, c: usize, d: f32) -> f32 {
        let n = self.ring.len();
        // `max` maps NaN to the floor, so the index never underflows.
        let d = d.max(2.0).min(n.saturating_sub(3) as f32);
        let whole = d as usize;
        let t = d - whole as f32;
        // `back` is within 1..n and `self.at` below n: one wrap, no division.
        let at = |back: usize| {
            let i = self.at + n - back;
            self.ring
                .get(if i >= n { i - n } else { i })
                .and_then(|f| f.get(c))
                .copied()
                .unwrap_or(0.0)
        };
        let [ym1, y0, y1, y2] = [at(whole - 1), at(whole), at(whole + 1), at(whole + 2)];
        // Catmull-Rom from y0 (t = 0) to y1 (t = 1).
        let a = -0.5 * ym1 + 1.5 * y0 - 1.5 * y1 + 0.5 * y2;
        let b = ym1 - 2.5 * y0 + 2.0 * y1 - 0.5 * y2;
        let k = -0.5 * ym1 + 0.5 * y1;
        ((a * t + b) * t + k) * t + y0
    }

    /// Replaces `io` with the wet signal.
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        let s = self.s;
        let per_ms = 0.001 * self.rate;
        let longest = self.ring.len().saturating_sub(3) as f32;
        let target = (s.ms * per_ms).max(0.0).min(longest);
        let mut delay = self.delay.unwrap_or(target);
        let swing = s.depth_ms * per_ms;
        let spread = [-s.spread_ms * per_ms, s.spread_ms * per_ms];
        let step = f64::from(s.lfo_hz / self.rate);
        let offsets = [0.0, f64::from(s.phase)];
        for frame in io {
            delay += (target - delay) * self.glide;
            let mut y = [0.0; 2];
            let heads = [(delay, 0.0), (delay * 0.5, SECOND_AHEAD)];
            let heads = if s.second_head {
                &heads[..]
            } else {
                &heads[..1]
            };
            for (c, ((out, offset), spread)) in y.iter_mut().zip(offsets).zip(spread).enumerate() {
                for (h, &(base, ahead)) in heads.iter().enumerate() {
                    // The delay grows as the LFO falls.
                    let x = self.read(c, base + spread - swing * lfo(self.cycle + offset + ahead));
                    let lp = self.lp.get_mut(h).and_then(|l| l.get_mut(c));
                    let hp = self.hp.get_mut(h).and_then(|l| l.get_mut(c));
                    if let (Some(lp), Some(hp)) = (lp, hp) {
                        *lp = flush(*lp + (x - *lp) * self.damping);
                        *hp = flush(*hp + (*lp - *hp) * self.low_cut);
                        *out += *lp - *hp;
                    }
                }
            }
            for filter in [&mut self.cut, &mut self.shelf].into_iter().flatten() {
                filter.process(std::slice::from_mut(&mut y));
            }
            let [a, b] = apply(s.input, *frame);
            let [c, d] = apply(s.feedback, y);
            let line = [a + c, b + d];
            if let Some(slot) = self.ring.get_mut(self.at) {
                *slot = line.map(flush);
            }
            self.at += 1;
            if self.at >= self.ring.len() {
                self.at = 0;
            }
            self.cycle = (self.cycle + step).fract();
            *frame = apply(s.output, y);
        }
        self.delay = Some(delay);
    }
}

/// The most allpasses a [`Phaser`] runs.
pub const MAX_STAGES: usize = 24;

/// A phaser in physical units: `stages` first-order allpasses whose corner
/// the LFO sweeps, `hz + spread_hz·(e^(exponent·lfo) − 1)`, around a feedback
/// loop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhaserSettings {
    /// 1..=[`MAX_STAGES`].
    pub stages: u8,
    /// A triangle LFO instead of the sine (both fall first).
    pub triangle: bool,
    /// The allpasses' corner with the LFO at its centre.
    pub hz: f32,
    /// How far the corner moves for an exponent's worth of LFO.
    pub spread_hz: f32,
    /// The sweep's curvature: the LFO is scaled by it inside `e^x`.
    pub exponent: f32,
    /// The last stage's output back into the first.
    pub feedback: f32,
    /// The LFO's speed.
    pub lfo_hz: f32,
    /// Cycles the right LFO runs ahead of the left.
    pub phase: f32,
}

/// LFO-swept first-order allpasses around a feedback loop; see
/// [`PhaserSettings`].
#[derive(Debug)]
pub struct Phaser {
    s: PhaserSettings,
    rate: f32,
    cycle: f64,
    /// Each stage's last input and output, per channel.
    x: [[f32; MAX_STAGES]; 2],
    y: [[f32; MAX_STAGES]; 2],
}

impl Phaser {
    /// A phaser at `rate` Hz; it never allocates.
    #[must_use]
    pub fn new(s: PhaserSettings, rate: u32) -> Self {
        Self {
            s,
            rate: rate as f32,
            cycle: 0.0,
            x: [[0.0; MAX_STAGES]; 2],
            y: [[0.0; MAX_STAGES]; 2],
        }
    }

    /// Changes the settings while it plays.
    pub fn set(&mut self, s: PhaserSettings) {
        self.s = s;
    }

    /// Replaces `io` with the wet signal.
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        let s = self.s;
        let step = f64::from(s.lfo_hz / self.rate);
        let offsets = [0.0, f64::from(s.phase)];
        let scale = std::f32::consts::PI / self.rate;
        let stages = usize::from(s.stages).clamp(1, MAX_STAGES);
        let shape = if s.triangle { triangle } else { lfo };
        for frame in io {
            let states = self.x.iter_mut().zip(self.y.iter_mut());
            for ((v, offset), (xs, ys)) in frame.iter_mut().zip(offsets).zip(states) {
                let sweep = s.spread_hz * ((s.exponent * shape(self.cycle + offset)).exp() - 1.0);
                let w = ((s.hz + sweep) * scale).clamp(1e-5, 0.99);
                let a = (1.0 - w) / (1.0 + w);
                let (Some(xs), Some(ys)) = (xs.get_mut(..stages), ys.get_mut(..stages)) else {
                    continue;
                };
                let mut input = *v + s.feedback * ys.last().copied().unwrap_or(0.0);
                for (x, y) in xs.iter_mut().zip(ys.iter_mut()) {
                    let out = flush((input + *y) * a - *x);
                    *x = input;
                    *y = out;
                    input = out;
                }
                *v = input;
            }
            self.cycle = (self.cycle + step).fract();
        }
    }
}
