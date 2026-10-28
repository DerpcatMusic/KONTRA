//! One modulated delay line (an echo, a chorus, a flanger) and one phaser;
//! design in docs/architecture-v3/03-DSP.md. Formats load their laws into
//! [`Settings`] and [`PhaserSettings`]. Both put out the wet signal only.

use std::f64::consts::TAU;

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
    /// Gain of each input channel into its line.
    pub input: [f32; 2],
    /// The heads' output back into the same line, and into the other one.
    pub feedback: f32,
    /// See `feedback`.
    pub cross: f32,
    /// A one-pole low-pass on each head; `f32::INFINITY` for none.
    pub damping_hz: f32,
    /// The LFO's speed.
    pub lfo_hz: f32,
    /// Cycles the right LFO runs ahead of the left.
    pub phase: f32,
}

/// The LFO at `cycles`: a sine that falls first.
fn lfo(cycles: f64) -> f32 {
    -(TAU * cycles).sin() as f32
}

/// Flushes a value that would decay into the subnormal range.
fn flush(x: f32) -> f32 {
    if x.abs() < 1e-20 { 0.0 } else { x }
}

/// One-pole coefficient for a corner at `hz`; 1 passes everything.
fn one_pole(hz: f32, rate: f32) -> f32 {
    1.0 - (-std::f32::consts::TAU * hz.max(0.0) / rate).exp()
}

/// A stereo delay line with one or two LFO-swept heads, feedback and
/// cross-feedback; see [`Settings`].
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
    /// The left LFO's phase, cycles.
    cycle: f64,
    /// Each head's low-pass state, per channel.
    lp: [[f32; 2]; 2],
}

impl ModDelay {
    /// Room for `max_ms` of delay plus the swing (at most [`MAX_MS`]). Not
    /// real-time: allocates.
    #[must_use]
    pub fn new(s: Settings, rate: u32, max_ms: f32) -> Self {
        let rate = rate as f32;
        let ms = (max_ms.max(s.ms) + s.depth_ms.abs()).clamp(0.0, MAX_MS);
        let frames = (ms * 0.001 * rate) as usize + 8;
        let mut d = Self {
            s,
            rate,
            ring: vec![[0.0; 2]; frames].into(),
            at: 0,
            delay: None,
            glide: 1.0 - (-1.0 / (GLIDE_S * rate)).exp(),
            damping: 1.0,
            cycle: 0.0,
            lp: [[0.0; 2]; 2],
        };
        d.set(s);
        d
    }

    /// Changes the settings while it plays; never allocates. The delay
    /// glides to its new length.
    pub fn set(&mut self, s: Settings) {
        self.s = s;
        self.damping = one_pole(s.damping_hz, self.rate).clamp(0.0, 1.0);
    }

    /// The sample `d` frames back (at least 2), 4-point cubic.
    fn read(&self, c: usize, d: f32) -> f32 {
        let n = self.ring.len();
        // `max` maps NaN to the floor, so the index never underflows.
        let d = d.max(2.0).min(n.saturating_sub(3) as f32);
        let whole = d as usize;
        let t = d - whole as f32;
        let at = |back: usize| {
            self.ring
                .get((self.at + n - back) % n)
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
            for (c, (out, offset)) in y.iter_mut().zip(offsets).enumerate() {
                for (h, &(base, ahead)) in heads.iter().enumerate() {
                    // The delay grows as the LFO falls.
                    let x = self.read(c, base - swing * lfo(self.cycle + offset + ahead));
                    if let Some(lp) = self.lp.get_mut(h).and_then(|l| l.get_mut(c)) {
                        *lp = flush(*lp + (x - *lp) * self.damping);
                        *out += *lp;
                    }
                }
            }
            let line = [
                frame[0] * s.input[0] + y[0] * s.feedback + y[1] * s.cross,
                frame[1] * s.input[1] + y[1] * s.feedback + y[0] * s.cross,
            ];
            if let Some(slot) = self.ring.get_mut(self.at) {
                *slot = line.map(flush);
            }
            self.at = (self.at + 1) % self.ring.len();
            self.cycle = (self.cycle + step).fract();
            *frame = y;
        }
        self.delay = Some(delay);
    }
}

/// A phaser in physical units: four first-order allpasses whose corner the
/// LFO sweeps, `hz + spread_hz·(e^(exponent·lfo) − 1)`, around a feedback loop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhaserSettings {
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

/// Four LFO-swept first-order allpasses around a feedback loop; see
/// [`PhaserSettings`].
#[derive(Debug)]
pub struct Phaser {
    s: PhaserSettings,
    rate: f32,
    cycle: f64,
    /// Each stage's last input and output, per channel.
    x: [[f32; 4]; 2],
    y: [[f32; 4]; 2],
}

impl Phaser {
    /// A phaser at `rate` Hz; it never allocates.
    #[must_use]
    pub fn new(s: PhaserSettings, rate: u32) -> Self {
        Self {
            s,
            rate: rate as f32,
            cycle: 0.0,
            x: [[0.0; 4]; 2],
            y: [[0.0; 4]; 2],
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
        for frame in io {
            let states = self.x.iter_mut().zip(self.y.iter_mut());
            for ((v, offset), (xs, ys)) in frame.iter_mut().zip(offsets).zip(states) {
                let sweep = s.spread_hz * ((s.exponent * lfo(self.cycle + offset)).exp() - 1.0);
                let w = ((s.hz + sweep) * scale).clamp(1e-5, 0.99);
                let a = (1.0 - w) / (1.0 + w);
                let mut input = *v + s.feedback * ys[3];
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
