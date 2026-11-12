//! A bank of [`COMB_VOICES`] stereo combs on one input. Each voice is a
//! delay line fed the input plus its own soft-clipped, rotated feedback
//! through a linear-phase smoother, read by three heads: a swept main head
//! (heard and fed back), its mirror swept the other way (heard through the
//! width), and a still head further back (heard through zero). A triangle
//! LFO sweeps the heads, or moves a window that fades between the voices.
//! Physical settings in [`CombSettings`]; the reference binary's addresses
//! are in the comments.
//!
//! ponytail: it runs at the host rate. The reference runs at twice 44.1 or
//! 48 kHz behind a resampler whose kernel is unread; delays and the
//! smoother's taps here are in host samples, which keeps the loop length and
//! the smoother's response at those rates. Above 88.2 kHz the reference's
//! smoother zero sits at a quarter of the rate, here at half.

use std::f32::consts::{FRAC_PI_4, PI, SQRT_2};

/// The voices in a bank.
pub const COMB_VOICES: usize = 4;
const VOICES: f32 = COMB_VOICES as f32;
/// The heads' floor, samples: the cubic read needs a newer frame (0x144f38a38).
const FLOOR: f32 = 3.0;
/// The loop smoother's delay, samples: its centre tap (4 at twice the rate,
/// 0x1447149a8).
const SMOOTHER_DELAY: f32 = 2.0;
/// The one-pole that rounds the LFO (`FUN_141960d90(+0x3c0, 4.0)`).
const LFO_SMOOTH_HZ: f32 = 4.0;
/// The one-pole on each voice's scan level (0x147699c30 = −2π·80).
const SCAN_SMOOTH_HZ: f32 = 80.0;
/// How long a settings change glides (`FUN_141933c60`, 15 ms).
/// ponytail: the reference ramps the knobs (chord ratios over 10 ms) and
/// re-runs its laws per sample; this glides the physical settings.
const GLIDE_S: f32 = 0.015;
/// The input shelf's damping `k`, Q √½ (0x147697b60).
const SHELF_K: f32 = SQRT_2;
/// The scan window's half-width, voices: 1/2.9 of a third (0x147699be0).
const SCAN_REACH: f32 = 3.0 / 2.9;

/// What moves the scan window round the voices, from the LFO's phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanShape {
    /// From the first voice to the last and back.
    Triangle,
    /// Once round all the voices, rising.
    SawUp,
    /// Once round, falling.
    SawDown,
}

/// A comb bank in physical units: what its voices share once, and per
/// voice only its interval and detune; [`CombBank`] expands the rest.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CombSettings {
    /// The first voice's pitch before its interval and detune, Hz.
    pub root_hz: f32,
    /// Each voice's interval over the root (it also sets the voice's loop
    /// gain) and its detune factor: voice `v` loops at
    /// `root_hz·ratio[v]·detune[v]`.
    pub ratio: [f32; COMB_VOICES],
    pub detune: [f32; COMB_VOICES],
    /// The root's loop gain a pass, before the soft clip: a voice `r` above
    /// takes `feedback^(1/r)`, so all decay alike per second
    /// (`FUN_141960af0`; the detune is not made up).
    pub feedback: f32,
    /// 0..=3: the voices after the first, fading in one by one at equal
    /// power; where the scan window rests (voice 0 at 0).
    pub voices: f32,
    /// 0..=¼: the loop smoother `(d + (1 − 2d)z⁻¹ + dz⁻²)²`'s outer tap
    /// `d`; ¼ is a zero at Nyquist.
    pub damping: f32,
    /// 0..=1: the heads' swing either side of rest, a share of the rest
    /// above the floor.
    pub depth: f32,
    pub lfo_hz: f32,
    /// How much further back the still head reads than the main head's rest.
    pub offset: f32,
    /// ±1: the main head's sign, out and round the loop.
    pub polarity: f32,
    /// 0..=1: the still head and the input folded into each voice.
    pub thru_zero: f32,
    /// 0..=1: the scan window's levels instead of the voices' fade; it
    /// stills the heads.
    pub scan: f32,
    /// The share of its [`ScanShape`] the LFO moves the window.
    pub scan_depth: f32,
    pub scan_shape: ScanShape,
    /// 0..=1: the heads turned apart, up to ±45° (the mirror head in over
    /// the first tenth), and the feedback turned ±2.25° (`FUN_1419614e0`
    /// case 4).
    pub width: f32,
    /// The wet share; the input makes up the rest.
    pub mix: f32,
}

/// `a` to `b` at `t`, the shape as `b`.
fn lerp(a: &CombSettings, b: &CombSettings, t: f32) -> CombSettings {
    let l = |x: f32, y: f32| x + (y - x) * t;
    let v = |mut x: [f32; COMB_VOICES], y: [f32; COMB_VOICES]| {
        x.iter_mut().zip(y).for_each(|(x, y)| *x = l(*x, y));
        x
    };
    CombSettings {
        root_hz: l(a.root_hz, b.root_hz),
        ratio: v(a.ratio, b.ratio),
        detune: v(a.detune, b.detune),
        feedback: l(a.feedback, b.feedback),
        voices: l(a.voices, b.voices),
        damping: l(a.damping, b.damping),
        depth: l(a.depth, b.depth),
        lfo_hz: l(a.lfo_hz, b.lfo_hz),
        offset: l(a.offset, b.offset),
        polarity: l(a.polarity, b.polarity),
        thru_zero: l(a.thru_zero, b.thru_zero),
        scan: l(a.scan, b.scan),
        scan_depth: l(a.scan_depth, b.scan_depth),
        width: l(a.width, b.width),
        mix: l(a.mix, b.mix),
        scan_shape: b.scan_shape,
    }
}

/// The settings as the sample loop uses them.
#[derive(Clone, Copy, Debug)]
struct Derived {
    s: CombSettings,
    /// Each main head's rest and the still head, samples.
    base: [f32; COMB_VOICES],
    still: [f32; COMB_VOICES],
    /// Each voice's loop gain and level outside a scan.
    feedback: [f32; COMB_VOICES],
    level: [f32; COMB_VOICES],
    /// The shelved input into every loop.
    input: f32,
    /// The swing, the scan's stilling applied (`FUN_141961030`).
    depth: f32,
    /// `(cos, sin)` of the heads' and the feedback's turns.
    pan: [f32; 2],
    cross: [f32; 2],
    /// The mirror head's share.
    mirror: f32,
    /// The smoother's taps: outer, next, centre.
    fir: [f32; 3],
    /// The shelf's `g`, `a`, low-pass and band-pass gains.
    shelf: [f32; 4],
    /// The wet sum's gain before the level normalisation.
    wet: f32,
    step: f64,
}

impl Derived {
    fn new(s: CombSettings, rate: f32, longest: f32) -> Self {
        // The loop: the head reads one behind the newest frame, the
        // smoother adds its delay and the feedback one frame, so a rest
        // of `rate/hz − delay` closes it at `rate/hz` (`FUN_141961f60`).
        let mut base = [FLOOR; COMB_VOICES];
        let mut feedback = [0.0; COMB_VOICES];
        let per_voice = s.ratio.into_iter().zip(s.detune);
        for ((b, f), (ratio, detune)) in base.iter_mut().zip(&mut feedback).zip(per_voice) {
            let hz = s.root_hz * ratio * detune;
            *b = (rate / hz - SMOOTHER_DELAY).max(FLOOR).min(longest);
            *f = s.feedback.max(0.0).powf(1.0 / ratio);
        }
        // Voice 0 always sounds; the others fade in a voice each.
        let mut level = [1.0; COMB_VOICES];
        for (v, l) in level.iter_mut().enumerate().skip(1) {
            *l = (s.voices + 1.0 - v as f32).clamp(0.0, 1.0).sqrt();
        }
        let depth = (1.0 - s.scan).powi(2) * s.depth;
        // The input low shelf offsets the first loop's low build-up: its
        // gain `1 − f₀` (at least 0.1, 0x147699c60), its corner a quarter
        // of the root (0x144e79a60) as the swing nears all (0.99,
        // 0x144f389d0), at most 60 Hz (0x147699c90).
        let f0 = feedback.first().copied().unwrap_or(0.0);
        let gain = (1.0 - f0).max(0.1);
        let hz = (0.25 * s.root_hz / (1.0 - 0.99 * depth)).min(60.0);
        // A Simper low shelf: `HP + √G·k·BP + G·LP`, its corner moved by `G^¼`.
        let g = (PI * hz / (rate * gain.sqrt().sqrt()))
            .clamp(0.0, 1.5)
            .tan();
        let w = s.width;
        // The heads ±π/4 (0x147697ac8), the feedback ±π/80 (0x147699c08),
        // the mirror in over a tenth (10, 0x1447149c8).
        let pan = FRAC_PI_4 * w;
        let cross = PI / 80.0 * w;
        let d = s.damping;
        let tz = s.thru_zero * (1.0 + s.polarity).min(1.0);
        Self {
            s,
            base,
            still: base.map(|b| b * (1.0 + s.offset)),
            feedback,
            level,
            input: (1.0 - s.feedback).max(0.0).sqrt().sqrt(),
            depth,
            pan: [pan.cos(), pan.sin()],
            cross: [cross.cos(), cross.sin()],
            mirror: 0.5 * (10.0 * w).min(1.0),
            fir: [d * d, 2.0 * d - 4.0 * d * d, 1.0 - 4.0 * d + 6.0 * d * d],
            shelf: [
                g,
                1.0 / (1.0 + g * (g + SHELF_K)),
                gain - 1.0,
                (gain.sqrt() - 1.0) * SHELF_K,
            ],
            // 0.5 (0x14470bae8), less 0.25 (0x144e79a60) through zero
            // unless inverted.
            wet: s.mix * (0.5 - 0.25 * tz),
            step: f64::from(s.lfo_hz / rate),
        }
    }
}

/// Where the window sits, voices from the first, at LFO phase `phase`.
fn sweep(s: &CombSettings, phase: f32) -> f32 {
    let shape = match s.scan_shape {
        ScanShape::Triangle => (VOICES - 1.0) * (1.0 - (1.0 - 2.0 * phase).abs()),
        ScanShape::SawUp => VOICES * phase,
        ScanShape::SawDown => VOICES * (1.0 - phase),
    };
    s.voices + s.scan_depth * shape
}

/// Each voice's level with the window at `at`: a triangle [`SCAN_REACH`]
/// either side of the voice on a circle of all the voices, under a square
/// root (`FUN_141961db0`; its wrapped centres are the circle's).
fn window(at: f32) -> [f32; COMB_VOICES] {
    let at = at.rem_euclid(VOICES);
    let mut out = [0.0; COMB_VOICES];
    for (v, l) in out.iter_mut().enumerate() {
        let d = (at - v as f32).abs();
        *l = (1.0 - d.min(VOICES - d) / SCAN_REACH)
            .clamp(0.0, 1.0)
            .sqrt();
    }
    out
}

/// `fb/√(1 + fb²/4)`: the feedback into the line, within ±2.
fn soft_clip(x: f32) -> f32 {
    x / (1.0 + 0.25 * x * x).sqrt()
}

/// `[[c, −s], [s, c]]·x`.
fn rotate([c, s]: [f32; 2], x: [f32; 2]) -> [f32; 2] {
    [c * x[0] - s * x[1], s * x[0] + c * x[1]]
}

/// `a + b`, per channel.
fn add(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] + b[0], a[1] + b[1]]
}

/// `k·a`, per channel.
fn scale(k: f32, a: [f32; 2]) -> [f32; 2] {
    a.map(|x| k * x)
}

/// Flushes a value that would decay into the subnormal range.
fn flush(x: f32) -> f32 {
    if x.abs() < 1e-20 { 0.0 } else { x }
}

/// The LFO and what follows it: its phase, its rounded triangle and the
/// voices' smoothed scan levels.
#[derive(Clone, Copy, Debug)]
struct Clock {
    phase: f64,
    lfo: f32,
    levels: [f32; COMB_VOICES],
    lfo_pole: f32,
    scan_pole: f32,
}

impl Clock {
    /// Moves one frame on; the rounded triangle, −1..=1.
    fn tick(&mut self, d: &Derived) -> f32 {
        let phase = self.phase as f32;
        let triangle = (2.0 - 4.0 * phase).abs() - 1.0;
        self.lfo = self.lfo_pole * self.lfo + (1.0 - self.lfo_pole) * triangle;
        // The window moves only while scanning (`+0x5cc == 2`); elsewhere
        // its levels hold where they were.
        if d.s.scan > 0.0 {
            let target = window(sweep(&d.s, phase));
            for (l, t) in self.levels.iter_mut().zip(target) {
                *l = self.scan_pole * *l + (1.0 - self.scan_pole) * t;
            }
        }
        let next = self.phase + d.step;
        self.phase = if next >= 1.0 { next.fract() } else { next };
        self.lfo
    }
}

/// One voice's state: its last feedback and its smoother's last 5 inputs.
#[derive(Clone, Copy, Debug)]
struct Voice {
    feedback: [f32; 2],
    smoother: [[f32; 2]; 5],
}

/// See the module docs and [`CombSettings`].
#[derive(Debug)]
pub struct CombBank {
    rate: f32,
    /// The longest main-head rest the ring holds, samples.
    longest: f32,
    /// A glide from `from` to `to` with `left` of `glide` frames to go.
    from: CombSettings,
    to: CombSettings,
    left: u32,
    glide: u32,
    now: Derived,
    /// Every voice's line, a frame each.
    ring: Box<[[[f32; 2]; COMB_VOICES]]>,
    /// The newest frame.
    at: usize,
    voices: [Voice; COMB_VOICES],
    /// The shelf's two states, per channel.
    shelf: [[f32; 2]; 2],
    clock: Clock,
    written: f32,
}

impl CombBank {
    /// Lines for a voice down to `lowest_hz` at the full swing: the main
    /// head reaches twice the rest, the still head less. The LFO starts at
    /// phase 0. Not real-time: allocates.
    #[must_use]
    pub fn new(s: CombSettings, rate: u32, lowest_hz: f32) -> Self {
        let rate = rate as f32;
        let longest = (rate / lowest_hz).clamp(FLOOR, rate);
        let now = Derived::new(s, rate, longest);
        let pole = |hz: f32| (-2.0 * PI * hz / rate).exp();
        // ponytail: the reference's start phase is random (a MINSTD
        // draw); this starts at 0, the window and the rounded LFO at rest.
        let clock = Clock {
            phase: 0.0,
            lfo: 1.0,
            levels: window(sweep(&s, 0.0)),
            lfo_pole: pole(LFO_SMOOTH_HZ),
            scan_pole: pole(SCAN_SMOOTH_HZ),
        };
        Self {
            rate,
            longest,
            from: s,
            to: s,
            left: 0,
            glide: ((GLIDE_S * rate) as u32).max(1),
            now,
            ring: vec![[[0.0; 2]; COMB_VOICES]; 2 * longest as usize + 8].into(),
            at: 0,
            voices: [Voice {
                feedback: [0.0; 2],
                smoother: [[0.0; 2]; 5],
            }; COMB_VOICES],
            shelf: [[0.0; 2]; 2],
            clock,
            written: 0.0,
        }
    }

    /// Glides to `s` from where it is; never allocates.
    pub fn set(&mut self, s: CombSettings) {
        if s == self.to {
            return;
        }
        self.from = self.now.s;
        self.to = s;
        self.left = self.glide;
    }

    /// One frame of the glide.
    fn glide(&mut self) {
        if self.left == 0 {
            return;
        }
        self.left -= 1;
        let t = 1.0 - self.left as f32 / self.glide as f32;
        self.now = Derived::new(lerp(&self.from, &self.to, t), self.rate, self.longest);
    }

    /// The ring's length, frames: how long silent input can still echo.
    #[must_use]
    pub fn memory(&self) -> usize {
        self.ring.len()
    }

    /// The loudest value the last [`Self::process`] left in its lines,
    /// loops and filters.
    #[must_use]
    pub fn written(&self) -> f32 {
        self.written
    }

    /// Lets `frames` silent frames pass: the glide, the LFO and its
    /// smoothers move on frame by frame as [`Self::process`] moves them.
    pub fn idle(&mut self, frames: usize) {
        for _ in 0..frames {
            self.glide();
            self.clock.tick(&self.now);
        }
        self.at = (self.at + frames) % self.ring.len().max(1);
    }

    /// The input shelf on `x` (`FUN_14195f600`, a trapezoidal SVF).
    fn shelve(&mut self, x: [f32; 2]) -> [f32; 2] {
        let [g, a, low, band] = self.now.shelf;
        let [s1s, s2s] = &mut self.shelf;
        let mut out = x;
        for ((y, s1), s2) in out.iter_mut().zip(s1s.iter_mut()).zip(s2s.iter_mut()) {
            let v1 = (*s1 + g * (*y - *s2)) * a;
            let lp = *s2 + g * v1;
            *s1 = flush(2.0 * v1 - *s1);
            *s2 = flush(*s2 + 2.0 * g * v1);
            *y += low * lp + band * v1;
        }
        out
    }

    /// The 4-point Catmull-Rom read of voice `v`, `back` frames behind
    /// the newest (`FUN_14195d9b0`).
    fn read(&self, v: usize, back: f32) -> [f32; 2] {
        let n = self.ring.len();
        let back = back.max(1.0).min(n.saturating_sub(3) as f32);
        let whole = back as usize;
        let t = back - whole as f32;
        let at = |b: usize| {
            let i = self.at + n - b;
            self.ring
                .get(if i >= n { i - n } else { i })
                .and_then(|f| f.get(v))
                .copied()
                .unwrap_or([0.0; 2])
        };
        let [ym1, y0, y1, y2] = [at(whole - 1), at(whole), at(whole + 1), at(whole + 2)];
        let cr = |ym1: f32, y0: f32, y1: f32, y2: f32| {
            let a = 1.5 * (y0 - y1) + 0.5 * (y2 - ym1);
            let b = ym1 - 2.5 * y0 + 2.0 * y1 - 0.5 * y2;
            ((a * t + b) * t + 0.5 * (y1 - ym1)) * t + y0
        };
        [
            cr(ym1[0], y0[0], y1[0], y2[0]),
            cr(ym1[1], y0[1], y1[1], y2[1]),
        ]
    }

    /// Replaces `io` with the output: the wet sum and the input's share.
    ///
    /// ponytail: one frame at a time, ~100 µs per 512 frames at 48 kHz
    /// (half a 2 % budget). Upgrade: the four voices as one 4-lane SIMD
    /// pass (smoother, clip, cubic reads), the clock and heads in spans,
    /// `Derived` only while gliding.
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        let mut written = 0.0f32;
        for frame in io {
            self.glide();
            let lfo = self.clock.tick(&self.now);
            let d = self.now;
            let s = d.s;
            let x = *frame;
            let xf = self.shelve(x);
            let feed = scale(d.input, xf);
            // The lines: input and clipped feedback through the smoother.
            self.at = (self.at + 1) % self.ring.len().max(1);
            let [outer, next, centre] = d.fir;
            let mut column = [[0.0f32; 2]; COMB_VOICES];
            for (voice, y) in self.voices.iter_mut().zip(column.iter_mut()) {
                voice.smoother.rotate_right(1);
                voice.smoother[0] = add(feed, voice.feedback.map(soft_clip));
                let [h0, h1, h2, h3, h4] = voice.smoother;
                let sides = add(scale(outer, add(h0, h4)), scale(next, add(h1, h3)));
                *y = add(sides, scale(centre, h2)).map(flush);
                written = y.iter().fold(written, |m, v| m.max(v.abs()));
            }
            if let Some(f) = self.ring.get_mut(self.at) {
                *f = column;
            }
            // The heads; the main one feeds back, rotated.
            let levels = self.clock.levels;
            let (mut sum, mut total) = ([0.0f32; 2], 0.0f32);
            let mut feedback = [[0.0f32; 2]; COMB_VOICES];
            let per_voice = d.base.iter().zip(d.still).zip(d.feedback).zip(d.level);
            for (v, ((((&base, still), gain), level), (fb, scan))) in
                per_voice.zip(feedback.iter_mut().zip(levels)).enumerate()
            {
                let swing = d.depth * lfo * (base - FLOOR);
                // The head sits one past the newest frame.
                let a = self.read(v, base + swing - 1.0);
                let b = self.read(v, base - swing - 1.0);
                let c = self.read(v, still - 1.0);
                let [cos, sin] = d.cross;
                let turn = if v % 2 == 0 { [cos, -sin] } else { [cos, sin] };
                *fb = rotate(turn, a).map(|y| flush(s.polarity * gain * y));
                // The input with the new, unclipped feedback; through zero
                // folds in the still head (and, inverted, cancels the input).
                let x0 = add(feed, *fb);
                let t = add(x0, scale(s.thru_zero, add(scale(s.polarity, x0), c)));
                let [pc, ps] = d.pan;
                let ya = rotate([pc, -ps], add(scale(s.polarity, a), t));
                let yb = rotate([pc, ps], add(scale(s.polarity, b), t));
                let level = s.scan * scan + (1.0 - s.scan) * level;
                let y = add(scale(1.0 - d.mirror, ya), scale(d.mirror, yb));
                sum = add(sum, scale(level, y));
                total += level;
            }
            for (voice, fb) in self.voices.iter_mut().zip(feedback) {
                voice.feedback = fb;
            }
            // `M·(0.5 − 0.25·tz)/√Σ level` and the input's `1 − M`.
            let gain = d.wet / total.max(1e-12).sqrt();
            *frame = add(scale(gain, sum), scale(1.0 - s.mix, x));
        }
        let held = self
            .voices
            .iter()
            .flat_map(|v| v.smoother.into_iter().chain([v.feedback]).flatten());
        let states = held.chain(self.shelf.into_iter().flatten());
        self.written = states.fold(written, |m, v| m.max(v.abs()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Voice 0 alone at `hz`, fed back at `feedback`, no swing, all wet.
    fn one(hz: f32, feedback: f32) -> CombSettings {
        CombSettings {
            root_hz: hz,
            ratio: [1.0; COMB_VOICES],
            detune: [1.0; COMB_VOICES],
            feedback,
            voices: 0.0,
            damping: 0.0,
            depth: 0.0,
            lfo_hz: 1.0,
            offset: 0.0,
            polarity: 1.0,
            thru_zero: 0.0,
            scan: 0.0,
            scan_depth: 0.0,
            scan_shape: ScanShape::Triangle,
            width: 0.0,
            mix: 1.0,
        }
    }

    /// The loop is `rate/hz` with the smoother's delay in it: an impulse
    /// comes out at once, then one frame short of the loop, then a loop
    /// apart (the input shelf's low smear stays under 0.02).
    #[test]
    fn echoes_are_a_loop_apart() {
        let mut bank = CombBank::new(one(400.0, 0.5), 48_000, 20.0);
        let mut io = vec![[0.0f32; 2]; 400];
        io[0] = [1.0, 0.0];
        bank.process(&mut io);
        let hits: Vec<usize> = (io.iter().enumerate())
            .filter(|(_, f)| f[0].abs() > 0.02)
            .map(|(n, _)| n)
            .collect();
        assert_eq!(hits, [0, 119, 239, 359]);
    }

    /// Full feedback on a loud constant: the clip holds the loop within
    /// `input + 2` a pass, where a linear loop grows without end.
    #[test]
    fn the_soft_clip_bounds_the_loop() {
        let mut bank = CombBank::new(one(400.0, 1.0), 48_000, 20.0);
        let mut io = vec![[10.0f32; 2]; 48_000];
        bank.process(&mut io);
        let loudest = io.iter().flatten().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(loudest.is_finite() && loudest < 20.0, "{loudest}");
        assert!(soft_clip(1e6) <= 2.0 && soft_clip(-1e6) >= -2.0);
    }

    /// On a voice the window's neighbours sit at `√(1 − 2.9/3)` and the
    /// far voice at 0; the circle wraps after the last voice.
    #[test]
    fn the_scan_window_fades_round_a_circle() {
        let side = (1.0f32 - 2.9 / 3.0).sqrt();
        let want = [1.0, side, 0.0, side];
        for at in [0.0, 4.0] {
            let got = window(at);
            assert!(
                got.iter().zip(want).all(|(g, w)| (g - w).abs() < 1e-6),
                "{got:?}"
            );
        }
        let half = window(0.5);
        assert!(
            (half[0] - half[1]).abs() < 1e-6 && half[2] == 0.0,
            "{half:?}"
        );
    }
}
