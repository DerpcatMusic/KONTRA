//! Ballistics stages: how the computed amount moves in time (followers,
//! envelopes, and the smoothing filters on the gain path).

use super::computer::{FeedbackCurve, table_gain};
use super::detector::EPS_18;
use super::pipeline::{Frame, Stage, Window};
use super::ProcessorState;

/// Generic: smooth-branching one-pole of the gain change in dB (attack while
/// it grows, release while it falls; Giannoulis et al. 2012), then the gain
/// through the dB table. Linked, channel 0 drives both. State `z[c][0]`.
pub(super) struct SmoothBranch {
    pub attack: f64,
    pub release: f64,
    pub linked: bool,
    pub upward: bool,
    change: [f64; 2],
}

impl SmoothBranch {
    pub fn new(attack: f64, release: f64, linked: bool, upward: bool, state: &ProcessorState) -> Self {
        Self { attack, release, linked, upward, change: [state.z[0][0], state.z[1][0]] }
    }
}

impl Stage for SmoothBranch {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        let (attack, release, upward) = (self.attack, self.release, self.upward);
        let channel = |change: &mut f64, target: f64| {
            let coefficient = if target > *change { attack } else { release };
            let mut value = target + coefficient * (*change - target);
            if value < 1e-12 {
                value = 0.;
            }
            *change = value;
            if value == 0. {
                1.
            } else if upward {
                table_gain(value)
            } else {
                table_gain(-value)
            }
        };
        let [c0, c1] = &mut self.change;
        let g0 = channel(c0, f.level[0]);
        f.gain = if self.linked {
            *c1 = *c0;
            [g0; 2]
        } else {
            [g0, channel(c1, f.level[1])]
        };
    }

    #[inline(always)]
    fn store(&mut self, state: &mut ProcessorState) {
        (state.z[0][0], state.z[1][0]) = (self.change[0], self.change[1]);
    }
}

/// Kontakt Classic (core `0x140ab2250`): per channel a hold `a`, set to 1 by
/// the trigger and otherwise decaying, and an attack-limited follower `b`
/// chasing it from below (falling to it at once); a correction keeps `a`
/// within 1.5 % of `1 + b`. `level` becomes `b`. State `z[c] = [a, b]`, f64.
pub(super) struct HoldFollower {
    pub attack: f64,
    pub release: f64,
    z: [[f64; 2]; 2],
}

/// Constants at `0x144fc3fe0`, `0x144fc3fd8`, `0x144fc3df8`, `0x144fc4000`.
const HOLD_EDGE: f64 = 0.985;
const CORRECT_BELOW: f64 = 0.97;
const CORRECT_SCALE: f64 = 1.015_228_426_395_939;
const CORRECT_OFFSET: f64 = 0.015_228_426_395_939_021;

impl HoldFollower {
    pub fn new(attack: f64, release: f64, state: &ProcessorState) -> Self {
        Self { attack, release, z: state.z }
    }
}

impl Stage for HoldFollower {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        for ch in 0..2 {
            let [mut a, mut b] = self.z[ch];
            if f.level[ch] == 0. || a >= HOLD_EDGE {
                a *= self.release;
            } else {
                a = 1.;
            }
            let held = a;
            if held <= b {
                b = held;
            } else {
                b = 1. - (1. - b) * self.attack;
                if held <= b {
                    b = held;
                }
            }
            if held < CORRECT_BELOW && (b + 1.) / (held + 1.) < HOLD_EDGE {
                a = b * CORRECT_SCALE + CORRECT_OFFSET;
            }
            self.z[ch] = [a, b];
            f.level[ch] = b;
        }
    }

    #[inline(always)]
    fn store(&mut self, state: &mut ProcessorState) {
        state.z = self.z;
    }
}

/// Kontakt Enhanced (`0x140ab2c80`): a two-pole attack (velocity, filtered)
/// and a released hold over it; `level` becomes the hold. State
/// `z[c] = [velocity, filtered]`, `aux[c]` the hold, f32.
pub(super) struct Velocity {
    pub attack: f32,
    pub release: f32,
    z: [[f32; 2]; 2],
    hold: [f32; 2],
}

impl Velocity {
    pub fn new(attack: f32, release: f32, state: &ProcessorState) -> Self {
        Self {
            attack,
            release,
            z: state.z.map(|c| c.map(|v| v as f32)),
            hold: [state.aux[0] as f32, state.aux[1] as f32],
        }
    }
}

impl Stage for Velocity {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        for ch in 0..2 {
            let detected = f.level[ch] as f32;
            let [velocity, filtered] = self.z[ch];
            let velocity = (detected - (velocity * 1.8 + filtered)) * self.attack + velocity;
            let filtered = (velocity * self.attack + 1e-12) + filtered;
            let held = (self.hold[ch] * self.release).max(filtered);
            self.z[ch] = [velocity, filtered];
            self.hold[ch] = held;
            f.level[ch] = f64::from(held);
        }
    }

    #[inline(always)]
    fn store(&mut self, state: &mut ProcessorState) {
        state.z = self.z.map(|c| c.map(f64::from));
        state.aux[..2].copy_from_slice(&self.hold.map(f64::from));
    }
}

/// Kontakt Pro (`0x140ab35d0`): falling levels follow a one-pole release,
/// rising ones pass at once, into the RBJ low-pass (DF1, its previous input
/// doubling as the follower, with the core's anti-denormal rotation).
/// State `aux[2..6]` (x1, x2, y1, y2), f32.
pub(super) struct ReleaseLowPass {
    pub release: f32,
    pub b: (f32, f32, f32, f32),
    m: [f32; 4],
}

const EPS_PRO: f32 = f32::from_bits(0x2193_92ef); // 0x144f385a4

impl ReleaseLowPass {
    pub fn new(release: f32, b: (f32, f32, f32, f32), state: &ProcessorState) -> Self {
        Self { release, b, m: std::array::from_fn(|i| state.aux[2 + i] as f32) }
    }
}

impl Stage for ReleaseLowPass {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        let [x1, x2, y1, y2] = self.m;
        let (b0, b1, a1, a2) = self.b;
        let mut excess = f.level[0] as f32;
        let step = excess - x1;
        if step <= 0. {
            excess = step * self.release + x1;
        }
        let filtered = excess * b0 + x1 * b1 + y1 * a1 + y2 * a2 + x2 * b0;
        self.m = [excess + EPS_PRO, x1 + EPS_PRO, filtered + EPS_PRO, y1 + EPS_PRO];
        f.level = [f64::from(filtered); 2];
    }

    #[inline(always)]
    fn store(&mut self, state: &mut ProcessorState) {
        state.aux[2..6].copy_from_slice(&self.m.map(f64::from));
    }
}

/// Kontakt Limiter (`0x140904890`): the level rises at once and falls by
/// `ln 2 / release_frames` per frame. State `aux[2]`, f32.
pub(super) struct PeakRelease {
    pub release: f32,
    follower: f32,
}

impl PeakRelease {
    pub fn new(release: f32, state: &ProcessorState) -> Self {
        Self { release, follower: state.aux[2] as f32 }
    }
}

impl Stage for PeakRelease {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        let peak = f.level[0] as f32;
        let step = peak - self.follower;
        let level = if step <= 0. { step * self.release + self.follower } else { peak };
        self.follower = level + EPS_PRO;
        f.level = [f64::from(level); 2];
    }

    #[inline(always)]
    fn store(&mut self, state: &mut ProcessorState) {
        state.aux[2] = f64::from(self.follower);
    }
}

/// Kontakt Limiter: the windowed level through the RBJ low-pass (DF2).
/// State `aux[3..5]` (w1, w2), f32.
pub(super) struct LowPass2 {
    pub b: (f32, f32, f32, f32),
    w: [f32; 2],
}

impl LowPass2 {
    pub fn new(b: (f32, f32, f32, f32), state: &ProcessorState) -> Self {
        Self { b, w: [state.aux[3] as f32, state.aux[4] as f32] }
    }
}

impl Stage for LowPass2 {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        let (b0, b1, a1, a2) = self.b;
        let [w1, w2] = self.w;
        let w = f.level[0] as f32 + w1 * a1 + w2 * a2;
        let y = w * b0 + w1 * b1 + w2 * b0;
        self.w = [w + EPS_PRO, w1 + EPS_PRO];
        f.level = [f64::from(y); 2];
    }

    #[inline(always)]
    fn store(&mut self, state: &mut ProcessorState) {
        state.aux[3..5].copy_from_slice(&self.w.map(f64::from));
    }
}

/// Kontakt Transparent Limiter: the dB level rises at once and falls with
/// the release, then a symmetric one-pole smooths it. State `aux[3..5]`, f32.
pub(super) struct HilbertRelease {
    pub release: f32,
    pub smooth: f32,
    released: f32,
    smoothed: f32,
}

impl HilbertRelease {
    pub fn new(release: f32, smooth: f32, state: &ProcessorState) -> Self {
        Self { release, smooth, released: state.aux[3] as f32, smoothed: state.aux[4] as f32 }
    }
}

impl Stage for HilbertRelease {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        let mut db = f.level[0] as f32;
        if db <= self.released {
            db = (self.released - db) * self.release + db;
        }
        self.released = db;
        self.smoothed = (self.smoothed - db) * self.smooth + db;
        f.level = [f64::from(self.smoothed); 2];
    }

    #[inline(always)]
    fn store(&mut self, state: &mut ProcessorState) {
        state.aux[3] = f64::from(self.released);
        state.aux[4] = f64::from(self.smoothed);
    }
}

/// Kontakt Transparent Limiter: the gain through an `L`-tap Hann FIR, as the
/// core runs it: the sum minus the first DFT bin's real part, over `L`, the
/// resonator re-synchronised from fresh accumulators once per period.
/// State `aux[9..14]` (sum, y1, y2, frames since the re-sync, cursor);
/// cells: the `L`-frame gain history, then two of fresh accumulators.
pub(super) struct Hann<'a> {
    pub cos: f64,
    pub scale: f64,
    pub period: usize,
    history: &'a mut [[f64; 2]],
    fresh: &'a mut [[f64; 2]],
    sum: f64,
    y1: f64,
    y2: f64,
    since: usize,
    at: usize,
}

impl<'a> Hann<'a> {
    pub fn new(cells: &'a mut [[f64; 2]], cos: f64, period: usize, state: &ProcessorState) -> Self {
        let (history, fresh) = cells.split_at_mut(cells.len() - 2);
        let a = &state.aux;
        Self {
            cos,
            scale: 1. / history.len() as f64,
            period,
            history,
            fresh,
            sum: a[9],
            y1: a[10],
            y2: a[11],
            since: a[12] as usize,
            at: a[13] as usize,
        }
    }
}

impl Stage for Hann<'_> {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        let gain = f.gain[0];
        let taps = self.history.len();
        let d = gain - self.history[self.at][0];
        self.history[self.at][0] = gain;
        self.at += 1;
        if self.at == taps {
            self.at = 0;
        }
        self.sum += d;
        let y0 = self.y1 * 2. * self.cos + d - self.y2;
        let smoothed = (self.sum - (y0 - self.y1 * self.cos)) * self.scale;
        (self.y2, self.y1) = (self.y1, y0);
        self.since += 1;
        if self.since > self.period - taps {
            let [[s, a], [b, _]] = [self.fresh[0], self.fresh[1]];
            let f0 = a * 2. * self.cos + gain - b;
            self.fresh[0] = [s + gain, f0];
            self.fresh[1][0] = a;
            if self.since >= self.period {
                (self.sum, self.y1, self.y2) = (self.fresh[0][0], self.fresh[0][1], self.fresh[1][0]);
                self.fresh[0] = [0.; 2];
                self.fresh[1] = [0.; 2];
                self.since = 0;
            }
        }
        f.gain = [smoothed; 2];
    }

    #[inline(always)]
    fn store(&mut self, state: &mut ProcessorState) {
        state.aux[9..14].copy_from_slice(&[self.sum, self.y1, self.y2, self.since as f64, self.at as f64]);
    }
}

/// UVI Maximizer (`0x141667480`): slew-limited downwards, then an attack /
/// two-speed release envelope (the slow one 100 times slower, `blend` mixes
/// them), then held by the sliding maximum over the look-ahead window.
/// State `aux[0..4]` (slew, fast, slow, blended), window from `aux[8]`, f32.
pub(super) struct SlewEnvelope<'a> {
    pub offset: f32,
    pub slew: f32,
    pub attack: f32,
    pub fast: f32,
    pub slow: f32,
    pub blend: f32,
    pub window: Window<'a>,
    s: [f32; 4],
}

impl<'a> SlewEnvelope<'a> {
    pub fn new(k: [f32; 6], window: Window<'a>, state: &ProcessorState) -> Self {
        let [offset, slew, attack, fast, slow, blend] = k;
        Self { offset, slew, attack, fast, slow, blend, window, s: std::array::from_fn(|i| state.aux[i] as f32) }
    }
}

impl Stage for SlewEnvelope<'_> {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        let v = f.level[0] as f32;
        let [slew, fast, slow, out] = self.s;
        let step = (v + -self.offset) - slew;
        let z = slew + if step < 0. { step.max(-self.slew) } else { step };
        let (fast, slow) = if out <= z {
            let e = (1. - self.attack) * z;
            (fast * self.attack + e, slow * self.attack + e)
        } else {
            ((1. - self.fast) * z + fast * self.fast, (1. - self.slow) * z + slow * self.slow)
        };
        let out = (slow - fast) * self.blend + fast;
        self.s = [z, fast, slow, out];
        f.level = [self.window.push(f64::from(out)); 2];
    }

    #[inline(always)]
    fn store(&mut self, state: &mut ProcessorState) {
        state.aux[..4].copy_from_slice(&self.s.map(f64::from));
        self.window.store(state);
    }
}

/// UVI Maximizer (`0x141667180`): the gain averaged twice over `n` frames.
/// State `aux[4..7]` (two sums, cursor), cells `n`.
pub(super) struct MovingAverage2<'a> {
    ring: &'a mut [[f64; 2]],
    sums: [f64; 2],
    at: usize,
}

impl<'a> MovingAverage2<'a> {
    pub fn new(ring: &'a mut [[f64; 2]], state: &ProcessorState) -> Self {
        Self { ring, sums: [state.aux[4], state.aux[5]], at: state.aux[6] as usize }
    }
}

impl Stage for MovingAverage2<'_> {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        let n = self.ring.len();
        let mut g = f.gain[0] as f32;
        for s in 0..2 {
            let old = self.ring[self.at][s] as f32;
            self.ring[self.at][s] = f64::from(g);
            self.sums[s] += f64::from(g) - f64::from(old);
            g = (self.sums[s] / n as f64) as f32;
        }
        self.at = (self.at + 1) % n;
        f.gain = [f64::from(g); 2];
    }

    #[inline(always)]
    fn store(&mut self, state: &mut ProcessorState) {
        state.aux[4..7].copy_from_slice(&[self.sums[0], self.sums[1], self.at as f64]);
    }
}

/// Solid Bus Comp (`0x141a95cb0`): the request through the attack / release
/// one-pole, or with auto release a second, slower envelope the request
/// falls back to; the gain is `2^(-32 env)`. State `aux[0..2]` envelope,
/// `aux[4..6]` the slow envelope, f64.
pub(super) struct BusEnvelope<const AUTO: bool> {
    pub curve: super::computer::BusRatio,
    pub attack: f64,
    pub release: f64,
    /// Auto-release up / down coefficients (`AUTO` only).
    pub auto: [f64; 2],
    env: [f64; 2],
    slow: [f64; 2],
}

impl<const AUTO: bool> BusEnvelope<AUTO> {
    pub fn new(curve: super::computer::BusRatio, attack: f64, release: f64, auto: [f64; 2], state: &ProcessorState) -> Self {
        Self { curve, attack, release, auto, env: [state.aux[0], state.aux[1]], slow: [state.aux[4], state.aux[5]] }
    }
}

impl<const AUTO: bool> BusEnvelope<AUTO> {
    /// One channel; called with constant `ch` so the frame stays in registers.
    #[inline(always)]
    fn channel(&mut self, tap: f64, ch: usize) -> f64 {
        let d = self.curve.depth(tap);
        let env = self.env[ch];
        let next = if !AUTO {
            let c = if d <= env { self.release } else { self.attack };
            (1. - c) * d + env * c
        } else {
            let [up, down] = self.auto;
            let c = if d <= self.slow[ch] { down } else { up };
            let slow = (1. - c) * d + c * self.slow[ch];
            self.slow[ch] = slow;
            if d > env {
                (1. - self.attack) * d + env * self.attack
            } else {
                (1. - self.release) * d.max(slow) + env * self.release
            }
        };
        self.env[ch] = next;
        (next * -32.).clamp(-32., 0.).exp2()
    }
}

impl<const AUTO: bool> Stage for BusEnvelope<AUTO> {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        f.gain = [self.channel(f.level[0], 0), self.channel(f.level[1], 1)];
    }

    #[inline(always)]
    fn store(&mut self, state: &mut ProcessorState) {
        state.aux[..2].copy_from_slice(&self.env);
        state.aux[4..6].copy_from_slice(&self.slow);
    }
}

/// Kontakt Transient Master (`0x141a4a9d0`): six one-pole envelopes on the
/// detector level. The attack signal is a fast-minus-slow difference, gated,
/// high-passed (2 Hz) and peak-held; the sustain signal is a slow envelope
/// over a medium one, less the gated fast-over-medium excess. `level`
/// becomes `[attack, sustain]`. State `aux[4..10]` the envelopes less their
/// initial values, `aux[10..12]` the 2 Hz high-pass, f32.
pub(super) struct SixEnvelopes {
    pub k: [f32; 9],
    pub sub: [f32; 3],
    e: [f32; 6],
    hp: [f32; 2],
}

/// Initial envelopes (`0x141aa0b80`): e1, e2, e3, e4, e5, e6.
const INIT: [u32; 6] = [0x3e86_24dd, 0x3ed9_999a, 0, 0x3ece_5604, 0x3ece_5604, 0x3f21_47ae];

impl SixEnvelopes {
    pub fn new(k: [f32; 9], sub: [f32; 3], state: &ProcessorState) -> Self {
        Self {
            k,
            sub,
            e: std::array::from_fn(|i| (f64::from(f32::from_bits(INIT[i])) + state.aux[4 + i]) as f32),
            hp: [state.aux[10] as f32, state.aux[11] as f32],
        }
    }
}

impl Stage for SixEnvelopes {
    #[inline(always)]
    fn tick(&mut self, fr: &mut Frame) {
        let [k1u, k1d, k2u, k2d, k3d, k4d, k5u, k6u, k6d] = self.k;
        let k5d = k4d;
        let step = |env: f32, target: f32, up: f32, down: f32| {
            let d = target - env;
            (if d > 0. { up } else { down }) * d + env
        };
        let (e, c) = (&mut self.e, self.sub);
        let l = fr.level[0] as f32;
        e[0] = step(e[0], l, k1u, k1d);
        e[1] = step(e[1], l, k2u, k2d);
        let mut v = e[0] - e[1];
        if v > f32::from_bits(0x3d98_5f07) {
            v *= f32::from_bits(0x3f92_233a);
        }
        let w = (v - f32::from_bits(0x3d4e_703b)).max(0.).min(f32::from_bits(0x3df5_c28f));
        let y = (c[1] * self.hp[0] + w * c[0]) - c[2] * self.hp[1];
        self.hp = [w, y];
        let d = y.abs() - e[2];
        e[2] = if d <= 0. { d * k3d + e[2] } else { y.abs() };
        let d = l - e[3];
        e[3] = if d <= 0. { d * k4d + e[3] } else { l };
        e[4] = step(e[4], l, k5u, k5d);
        let s = ((e[3] - e[4]) - f32::from_bits(0x3d23_d70a)).max(0.).min(10_000.);
        e[5] = step(e[5], l, k6u, k6d);
        let sustain = ((e[5] - e[4]) - s).max(0.).min(10_000.);
        fr.level = [f64::from(e[2]), f64::from(sustain)];
    }

    #[inline(always)]
    fn store(&mut self, state: &mut ProcessorState) {
        for (i, v) in self.e.iter().enumerate() {
            state.aux[4 + i] = f64::from(*v) - f64::from(f32::from_bits(INIT[i]));
        }
        state.aux[10] = f64::from(self.hp[0]);
        state.aux[11] = f64::from(self.hp[1]);
    }
}

/// Kontakt Feedback Compressor (`0x141aa80d0`): 2 (4 in high quality)
/// sub-steps per frame. Each reads the detector times the last gain (the
/// feedback tap), lets a peak follower rise with the attack and relax with
/// the release toward a floor while two coupled one-poles (178.8 rad/s per
/// sub-step) average it, and asks the curve for the gain. Cells `[A, D, E,
/// G]` per detector, f64, zero from reset (native: no init but bypass).
pub(super) struct FeedbackLoop<'a> {
    pub curve: FeedbackCurve,
    pub attack: f32,
    pub release: f32,
    pub couple: f32,
    pub floor: f32,
    pub steps: usize,
    pub linked: bool,
    pub bypass: bool,
    cells: &'a mut [[f64; 2]],
    s: [[f64; 4]; 2],
}

pub(super) const FEEDBACK_CELLS: usize = 4;

impl<'a> FeedbackLoop<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        curve: FeedbackCurve,
        [attack, release, couple, floor]: [f32; 4],
        steps: usize,
        linked: bool,
        bypass: bool,
        cells: &'a mut [[f64; 2]],
    ) -> Self {
        let s = std::array::from_fn(|ch| std::array::from_fn(|k| cells[k][ch]));
        Self { curve, attack, release, couple, floor, steps, linked, bypass, cells, s }
    }

    /// One sub-step on detector state `[a, d, e, g]`.
    #[inline(always)]
    fn step(&self, det: f64, s: &mut [f64; 4]) {
        let [mut a, mut d, mut e, _] = *s;
        let v = det * s[3];
        if v >= e {
            e = (v - e) * f64::from(self.attack) + e;
        }
        let eps = f64::from(EPS_18);
        let couple = f64::from(self.couple);
        let e2 = ((f64::from(self.floor) - e) * f64::from(self.release) + e) + (a - e) * couple + eps;
        d = (a - d) * couple + d + eps;
        a = (e2 + d) * 0.5 + eps;
        e = e2;
        *s = [a, d, e, eps + self.curve.gain(a)];
    }
}

impl Stage for FeedbackLoop<'_> {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        let run = |this: &Self, mut st: [f64; 4], det: f64| {
            for _ in 0..this.steps {
                this.step(det, &mut st);
            }
            st
        };
        self.s[0] = run(self, self.s[0], f.level[0]);
        if !self.linked {
            self.s[1] = run(self, self.s[1], f.level[1]);
        }
        if self.bypass {
            let floor = f64::from(self.floor);
            self.s = [[floor, floor, floor, 1.]; 2];
        }
        let g = |k: usize| f64::from(self.s[k][3] as f32);
        f.gain = if self.linked { [g(0); 2] } else { [g(0), g(1)] };
    }

    #[inline(always)]
    fn store(&mut self, _: &mut ProcessorState) {
        for (k, cell) in self.cells.iter_mut().enumerate().take(FEEDBACK_CELLS) {
            *cell = [self.s[0][k], self.s[1][k]];
        }
    }
}
