//! Voices: envelopes, fades, source windows and the per-block render kernel.

use super::{
    DECLICK, EventId, MAX_BLOCK,
    bank::{Bank, Span},
    filter::{FilterKey, LaneFilter, VoiceFilter},
    map::{FOREVER, PlayMap, Run},
    params::{Inputs, VOICE_MODS},
    stream::Slot,
};
use crate::audio::Frame;
use std::time::{Duration, Instant};

/// Highest playback increment (source frames per output frame): a 192 kHz
/// sample three octaves up at 48 kHz.
pub(crate) const MAX_STEP: f64 = 32.0;
/// Source frames one block can read: the pitched span plus interpolation taps.
pub(crate) const WINDOW: usize = MAX_BLOCK * MAX_STEP as usize + 8;
/// 1.0 in the kernel's 32.32 fixed-point positions.
const FIXED_ONE: f64 = (1u64 << 32) as f64;
/// Envelope level treated as silence (−80 dB); decays below it end the voice.
const SILENT: f32 = 1e-4;
/// Longest a voice starting with nothing resident waits for its first
/// streamed frames before it plays on regardless (see [`Voice::waits`]).
pub(crate) const START_HOLD: f32 = 0.05;
/// Streamed frames a waiting voice wants published before it starts:
/// more than a block reads at any pitch the streamer then keeps ahead of.
const START_LEAD: u64 = 1024;
/// Offline renders wait at most this long for one streamed window.
const OFFLINE_WAIT: Duration = Duration::from_secs(5);
/// Seconds a voice stays muted before its stream pauses.
const PAUSE_AFTER: f32 = 0.1;

/// Attack-hold-decay-sustain-release amplitude envelope, times in seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ahdsr {
    pub attack: f32,
    /// Attack shape, -1..=1: 0 is linear, positive convex (fast rise),
    /// negative concave (slow start).
    pub curve: f32,
    pub hold: f32,
    pub decay: f32,
    /// Linear sustain level, 0–1.
    pub sustain: f32,
    pub release: f32,
}

/// Where an envelope is, as the editor shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Phase {
    Done = 0,
    Attack,
    Hold,
    Decay,
    Sustain,
    Release,
    /// On a flex envelope's segments.
    Flex,
}

impl Phase {
    pub fn from_u8(x: u8) -> Self {
        match x {
            1 => Self::Attack,
            2 => Self::Hold,
            3 => Self::Decay,
            4 => Self::Sustain,
            5 => Self::Release,
            6 => Self::Flex,
            _ => Self::Done,
        }
    }
}

impl Ahdsr {
    /// The attack, decay and release as a voice renders them, each sampled
    /// at `points + 1` even steps over its own time: the engine's envelope
    /// run at a rate that fits the stage into `points` frames. The release
    /// falls from the sustain level. A stage of no time is one point.
    pub fn trace(&self, points: usize) -> [Vec<f32>; 3] {
        let n = points.max(1);
        let sustain = self.sustain.clamp(0.0, 1.0);
        let base = Ahdsr { attack: 0.0, curve: 0.0, hold: 0.0, decay: 0.0, sustain, release: f32::INFINITY };
        let timed = |seconds: f32| seconds > 0.0 && seconds.is_finite();
        let mut out = [vec![1.0], vec![sustain], vec![0.0]];
        if timed(self.attack) {
            let env = Ahdsr { attack: self.attack, curve: self.curve, sustain: 1.0, ..base };
            let mut e = Envelope::new(&env, n as f32 / self.attack);
            out[0] = vec![0.0; n + 1];
            e.render(&mut out[0][1..], None, 1.0);
        }
        if timed(self.decay) {
            // No attack: the first frame is at full level, the decay follows.
            let mut e = Envelope::new(&Ahdsr { decay: self.decay, ..base }, n as f32 / self.decay);
            out[1] = vec![0.0; n + 1];
            e.render(&mut out[1], None, 1.0);
        }
        if timed(self.release) {
            // Full level, then the sustain level (no decay time), then let go.
            let mut e = Envelope::new(&Ahdsr { release: self.release, ..base }, n as f32 / self.release);
            let mut lead = [0.0; 2];
            e.render(&mut lead, None, 1.0);
            e.release(None);
            out[2] = vec![e.level(); n + 1];
            e.render(&mut out[2][1..], None, 1.0);
        }
        out
    }

    /// Holds 1 until the voice ends another way: the partner of a lone flex envelope.
    pub const UNITY: Self = Self {
        attack: 0.0,
        curve: 0.0,
        hold: 0.0,
        decay: 0.0,
        sustain: 1.0,
        release: f32::INFINITY,
    };
}

/// Breakpoint (flex) envelope: glides from silence through `points` and
/// holds at `points[sustain]` while the key is down.
#[derive(Clone, Debug, PartialEq)]
pub struct Flex {
    pub points: Box<[FlexPoint]>,
    pub sustain: usize,
}

/// A flex envelope point, reached from the previous level.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlexPoint {
    pub seconds: f32,
    /// Linear level, 0–1.
    pub level: f32,
    /// Segment shape like [`Ahdsr::curve`]: positive moves fast early.
    pub curve: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Attack,
    Hold,
    Decay,
    Sustain,
    Release,
    /// Start the glide toward this flex point from the current level.
    Enter(u8),
    /// Gliding toward this flex point.
    Point(u8),
    Done,
}

/// Curved attack, then exponential decay and release (−60 dB over the stage
/// time, like Kontakt's AHDSR); or a flex envelope's curved segments.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Envelope {
    stage: Stage,
    level: f32,
    /// Glide step of the attack or flex segment, `level = level * step.0 +
    /// step.1`: a one-pole glide whose target lies past the end (convex),
    /// before the start (concave), or a line.
    step: (f32, f32),
    /// Frames left in the hold stage or flex segment.
    left: u32,
    decay: f32,
    sustain: f32,
    release: f32,
    /// The attack's or decay's [`edge`], NaN until a skip works it out.
    edge: f32,
}

/// A block of envelope gain as a level times a per-frame decay (1 for a
/// held level): voices of one decay share the block's curve.
#[derive(Clone, Copy, Debug)]
enum Shape {
    Flat(f32),
    /// `(per-frame factor, level)`, as the release renders it.
    Decay(f32, f32),
}

impl Shape {
    fn times(self, other: Self) -> Option<Self> {
        match (self, other) {
            (Self::Flat(a), Self::Flat(b)) => Some(Self::Flat(a * b)),
            (Self::Flat(a), Self::Decay(k, b)) | (Self::Decay(k, b), Self::Flat(a)) => Some(Self::Decay(k, a * b)),
            (Self::Decay(..), Self::Decay(..)) => None,
        }
    }
}

/// Per-frame multiplier that reaches −60 dB after `seconds`.
fn exp_coef(seconds: f32, rate: f32) -> f32 {
    if seconds > 0.0 {
        0.001f32.powf(1.0 / (seconds * rate))
    } else {
        0.0
    }
}

/// Exponent of the attack curve at |curve| = 1: `(1 - e^(-k t)) / (1 - e^(-k))`
/// with `k = CURVE_STEEPNESS * curve`. Kontakt's exact law is unverified.
const CURVE_STEEPNESS: f32 = 5.0;

/// Per-frame step from `from` to `to` over `frames` (at least 1), bent by
/// `curve`.
fn glide(from: f32, to: f32, frames: f32, curve: f32) -> (f32, f32) {
    // A sub-frame attack (Areia's legato sets ~0.1 µs) would overflow the
    // curved step to inf and the level to NaN: silence.
    let frames = frames.max(1.0);
    let k = CURVE_STEEPNESS * curve.clamp(-1.0, 1.0);
    if k.abs() < 1e-3 {
        return (1.0, (to - from) / frames);
    }
    // The shape is a one-pole glide toward `target`, reached asymptotically.
    let target = from + (to - from) / -(-k).exp_m1();
    let mul = (-k / frames).exp();
    (mul, target * (1.0 - mul))
}

/// Write `x = x * mul + add` per frame into `out`, from `x` = `level`;
/// returns the last value. Eight frames at a time from powers of the step,
/// so the frames don't wait on each other as a frame-by-frame loop does.
fn affine(out: &mut [f32], level: f32, step: (f32, f32)) -> f32 {
    let (pow, off) = powers(step);
    let mut x = level;
    let (chunks, tail) = out.as_chunks_mut::<8>();
    for c in chunks {
        for k in 0..8 {
            c[k] = pow[k] * x + off[k];
        }
        x = c[7];
    }
    for (k, t) in tail.iter_mut().enumerate() {
        *t = pow[k] * x + off[k];
    }
    out.last().copied().unwrap_or(level)
}

/// The step taken 1..=8 times: frame `k` of an eight-frame chunk from `x`
/// is `pow[k] * x + off[k]`.
fn powers((mul, add): (f32, f32)) -> ([f32; 8], [f32; 8]) {
    let (mut pow, mut off) = ([0f32; 8], [0f32; 8]);
    let (mut p, mut o) = (1f32, 0f32);
    for k in 0..8 {
        (p, o) = (p * mul, o * mul + add);
        (pow[k], off[k]) = (p, o);
    }
    (pow, off)
}

/// Levels a chunk can start from with no frame stopping a glide (see
/// [`edge`]): below the edge for a rising test, above it for a falling one.
#[derive(Clone, Copy)]
enum Short {
    Unknown,
    Below(f32),
    Above(f32),
}

impl Short {
    fn holds(self, x: f32) -> bool {
        match self {
            Self::Unknown => false,
            Self::Below(edge) => x < edge,
            Self::Above(edge) => x > edge,
        }
    }
}

/// The level where a glide's `stop` test starts to hold in a chunk, for a
/// test that holds from some level up (`rising`) or down. Frame `k` of a
/// chunk from `x`, `pow[k] * x + off[k]` rounded, never falls as `x` rises
/// when the powers are not negative, so neither does the test; a chunk from
/// a level short of the edge has no frame that stops, and skipping it takes
/// its last frame alone, the same value. Bisected over the finite floats in
/// order: at most 32 tests of eight frames, once per stage. Never short
/// (the far infinity) for a step it cannot vouch for.
fn edge(step: (f32, f32), rising: bool, stop: &impl Fn(f32) -> bool) -> f32 {
    let (never, always) = match rising {
        true => (f32::NEG_INFINITY, f32::INFINITY),
        false => (f32::INFINITY, f32::NEG_INFINITY),
    };
    let (pow, off) = powers(step);
    if !(step.0 >= 0.0 && pow.iter().chain(&off).all(|v| v.is_finite())) {
        return never;
    }
    let stops = |x: f32| pow.iter().zip(&off).any(|(&p, &o)| stop(p * x + o));
    // The floats in order as integers (-0 just below +0, which tests alike).
    let key = |x: f32| (x.to_bits() as i32) ^ ((x.to_bits() as i32 >> 31) & i32::MAX);
    let float = |k: i32| f32::from_bits((k ^ ((k >> 31) & i32::MAX)) as u32);
    // `short` never stops; `far` does.
    let (mut short, mut far) = match rising {
        true => (key(f32::MIN), key(f32::MAX)),
        false => (key(f32::MAX), key(f32::MIN)),
    };
    if stops(float(short)) {
        return never;
    }
    if !stops(float(far)) {
        return always;
    }
    while short.abs_diff(far) > 1 {
        let mid = ((i64::from(short) + i64::from(far)) / 2) as i32;
        if stops(float(mid)) {
            far = mid;
        } else {
            short = mid;
        }
    }
    float(far)
}

/// [`affine`] over `len` frames into `out`, or without writing them when
/// `out` is `None`: the same values either way, from the same eight-frame
/// powers, so an envelope that skips frames lands where one that renders
/// them does, bit for bit. Returns the first frame where `stop` holds and
/// its value, or the last frame's value (`level` for no frames).
fn affine_until(
    out: Option<&mut [f32]>,
    len: usize,
    level: f32,
    step: (f32, f32),
    stop: impl Fn(f32) -> bool,
    short: Short,
) -> (f32, Option<usize>) {
    if let Some(out) = out {
        let out = &mut out[..len];
        let last = affine(out, level, step);
        return match first(out, &stop) {
            Some(i) => (out[i], Some(i)),
            None => (last, None),
        };
    }
    let (pow, off) = powers(step);
    let mut x = level;
    for chunk in 0..len / 8 {
        // No frame of the chunk can stop: its last is all that is needed.
        if short.holds(x) {
            x = pow[7] * x + off[7];
            continue;
        }
        let c: [f32; 8] = std::array::from_fn(|k| pow[k] * x + off[k]);
        // Every frame is tested, as `position` would: a chunk's end alone
        // could step back over the threshold by a rounding.
        if let Some(k) = first(&c, &stop) {
            return (c[k], Some(chunk * 8 + k));
        }
        x = c[7];
    }
    let tail = len % 8;
    if tail > 0 {
        let c: [f32; 8] = std::array::from_fn(|k| pow[k] * x + off[k]);
        if let Some(k) = first(&c[..tail], &stop) {
            return (c[k], Some(len - tail + k));
        }
        x = c[tail - 1];
    }
    (x, None)
}

/// [`affine_until`] of a release (`x = x * mul`, `mul` in `[0, 1]`, from
/// `x >= 0`) to below [`SILENT`], unwritten: the same values, but as
/// the powers of `mul` only fall, so does each chunk's frames, and its last
/// frame alone says whether the chunk reaches silence. One multiply per
/// eight frames, as muted and laned voices only move on.
fn decay_until_silent(len: usize, level: f32, mul: f32) -> (f32, Option<usize>) {
    let mut p = 1f32;
    let pow: [f32; 8] = std::array::from_fn(|_| {
        p *= mul;
        p
    });
    let mut x = level;
    for chunk in 0..len / 8 {
        // `pow[k] * x + 0.0` in `affine_until`: the same value.
        let last = pow[7] * x;
        if last < SILENT {
            let c: [f32; 8] = std::array::from_fn(|k| pow[k] * x);
            let k = c.iter().position(|&x| x < SILENT).unwrap_or(7);
            return (c[k], Some(chunk * 8 + k));
        }
        x = last;
    }
    let (x, end) = affine_until(None, len % 8, x, (mul, 0.0), |x| x < SILENT, Short::Unknown);
    (x, end.map(|k| len / 8 * 8 + k))
}

/// The first of `xs` where `stop` holds: eight at a time without a branch
/// each, as the frames of a glide rarely stop.
fn first(xs: &[f32], stop: &impl Fn(f32) -> bool) -> Option<usize> {
    let (chunks, tail) = xs.as_chunks::<8>();
    for (c, chunk) in chunks.iter().enumerate() {
        if chunk.iter().fold(false, |hit, &x| hit | stop(x)) {
            return chunk.iter().position(|&x| stop(x)).map(|k| c * 8 + k);
        }
    }
    tail.iter().position(|&x| stop(x)).map(|k| chunks.len() * 8 + k)
}

impl Envelope {
    pub fn new(p: &Ahdsr, rate: f32) -> Self {
        let attack = p.attack * rate;
        Self {
            stage: Stage::Attack,
            level: 0.0,
            step: if attack > 0.0 {
                glide(0.0, 1.0, attack, p.curve)
            } else {
                (1.0, 1.0)
            },
            left: (p.hold.max(0.0) * rate) as u32,
            decay: exp_coef(p.decay, rate),
            sustain: p.sustain.clamp(0.0, 1.0),
            release: exp_coef(p.release, rate),
            edge: f32::NAN,
        }
    }

    /// A flex envelope's state; its points are passed to `render`.
    pub fn flex() -> Self {
        Self {
            stage: Stage::Enter(0),
            level: 0.0,
            step: (1.0, 0.0),
            left: 0,
            decay: 0.0,
            sustain: 0.0,
            release: 0.0,
            edge: f32::NAN,
        }
    }

    /// Enter the release: the flex segment after the sustain point, or the
    /// AHDSR release.
    pub fn release(&mut self, flex: Option<&Flex>) {
        self.stage = match (self.stage, flex) {
            (Stage::Done, _) => Stage::Done,
            (_, Some(flex)) => Stage::Enter((flex.sustain + 1) as u8),
            _ => Stage::Release,
        };
    }

    pub fn done(&self) -> bool {
        self.stage == Stage::Done
    }

    /// The gain the envelope has reached.
    pub fn level(&self) -> f32 {
        self.level
    }

    /// The stage, for the editor's playheads: see [`Phase`].
    pub fn phase(&self) -> Phase {
        match self.stage {
            Stage::Attack => Phase::Attack,
            Stage::Hold => Phase::Hold,
            Stage::Decay => Phase::Decay,
            Stage::Sustain => Phase::Sustain,
            Stage::Release => Phase::Release,
            Stage::Enter(_) | Stage::Point(_) => Phase::Flex,
            Stage::Done => Phase::Done,
        }
    }

    /// The next `n` frames as one [`Shape`], when the stage holds for all
    /// of them: a held level, or a release that does not end in them.
    fn shape(&self, n: usize) -> Option<Shape> {
        match self.stage {
            Stage::Sustain if self.level > SILENT => Some(Shape::Flat(self.level)),
            Stage::Hold if self.left as usize >= n => Some(Shape::Flat(self.level)),
            // Twice the threshold: rounding cannot end it early.
            Stage::Release if self.level * self.release.powi(n as i32) >= 2.0 * SILENT => {
                Some(Shape::Decay(self.release, self.level))
            }
            _ => None,
        }
    }

    /// Advance over `frames` exactly as [`Envelope::render`] would, without
    /// computing each frame: held stages jump, glides step eight frames at
    /// a time.
    pub fn skip(&mut self, frames: usize, flex: Option<&Flex>, rate: f32) {
        self.run(None, frames, flex, rate);
    }

    /// Write one gain per frame; `flex` is the envelope's points, if it is one.
    pub fn render(&mut self, out: &mut [f32], flex: Option<&Flex>, rate: f32) {
        let len = out.len();
        self.run(Some(out), len, flex, rate);
    }

    /// The current stage's [`edge`], worked out once.
    fn edge(&mut self, step: (f32, f32), rising: bool, stop: &impl Fn(f32) -> bool) -> f32 {
        if self.edge.is_nan() {
            self.edge = edge(step, rising, stop);
        }
        self.edge
    }

    /// The stage machine over `len` frames, writing them to `out` if given.
    fn run(&mut self, mut out: Option<&mut [f32]>, len: usize, flex: Option<&Flex>, rate: f32) {
        let mut i = 0;
        while i < len {
            let n = len - i;
            let mut rest = out.as_deref_mut().map(|o| &mut o[i..]);
            let written = match self.stage {
                Stage::Attack => {
                    let stop = |x: f32| x >= 1.0;
                    let short = match rest {
                        Some(_) => Short::Unknown,
                        None => Short::Below(self.edge(self.step, true, &stop)),
                    };
                    let (level, peak) = affine_until(rest.as_deref_mut(), n, self.level, self.step, stop, short);
                    self.level = level;
                    match peak {
                        Some(peak) => {
                            if let Some(rest) = rest {
                                rest[peak] = 1.0;
                            }
                            self.level = 1.0;
                            self.stage = Stage::Hold;
                            peak + 1
                        }
                        None => n,
                    }
                }
                Stage::Hold => {
                    let m = n.min(self.left as usize);
                    if let Some(rest) = rest {
                        rest[..m].fill(self.level);
                    }
                    self.left -= m as u32;
                    if self.left == 0 {
                        self.stage = Stage::Decay;
                        self.edge = f32::NAN;
                    }
                    m
                }
                Stage::Enter(i) => {
                    self.stage = match flex.and_then(|f| f.points.get(i as usize)) {
                        Some(p) => {
                            let frames = (p.seconds * rate).round().max(1.0);
                            self.step = glide(self.level, p.level, frames, p.curve);
                            self.left = frames as u32;
                            Stage::Point(i)
                        }
                        None => Stage::Done,
                    };
                    0
                }
                Stage::Point(i) => {
                    let m = n.min(self.left as usize);
                    (self.level, _) =
                        affine_until(rest.as_deref_mut(), m, self.level, self.step, |_| false, Short::Unknown);
                    self.left -= m as u32;
                    if self.left == 0 {
                        let point = flex.and_then(|f| Some((f.sustain, f.points.get(i as usize)?)));
                        self.stage = match point {
                            Some((sustain, p)) => {
                                // Land exactly on the point.
                                self.level = p.level;
                                if let Some(rest) = rest {
                                    rest[m - 1] = p.level;
                                }
                                if i as usize == sustain {
                                    Stage::Sustain
                                } else {
                                    Stage::Enter(i + 1)
                                }
                            }
                            None => Stage::Done,
                        };
                    }
                    m
                }
                Stage::Decay => {
                    let sustain = self.sustain;
                    let step = (self.decay, sustain * (1.0 - self.decay));
                    let stop = |x: f32| x - sustain <= SILENT;
                    let short = match rest {
                        Some(_) => Short::Unknown,
                        None => Short::Above(self.edge(step, false, &stop)),
                    };
                    let (level, end) = affine_until(rest, n, self.level, step, stop, short);
                    self.level = level;
                    match end {
                        Some(end) => {
                            self.level = sustain;
                            self.stage = Stage::Sustain;
                            end + 1
                        }
                        None => n,
                    }
                }
                Stage::Sustain if self.level > SILENT => {
                    if let Some(rest) = rest {
                        rest.fill(self.level);
                    }
                    n
                }
                Stage::Release => {
                    let (level, end) = match rest {
                        Some(rest) => {
                            affine_until(Some(rest), n, self.level, (self.release, 0.0), |x| x < SILENT, Short::Unknown)
                        }
                        None => decay_until_silent(n, self.level, self.release),
                    };
                    self.level = level;
                    match end {
                        Some(end) => {
                            self.stage = Stage::Done;
                            end + 1
                        }
                        None => n,
                    }
                }
                Stage::Sustain | Stage::Done => {
                    self.stage = Stage::Done;
                    self.level = 0.0;
                    if let Some(rest) = rest {
                        rest.fill(0.0);
                    }
                    n
                }
            };
            i += written;
        }
    }
}

/// Linear gain ramp for steals, chokes and scripted fades.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Fade {
    value: f32,
    target: f32,
    step: f32,
    left: u32,
    /// End the voice once the ramp reaches silence.
    stop: bool,
}

impl Fade {
    pub const FULL: Self = Self {
        value: 1.0,
        target: 1.0,
        step: 0.0,
        left: 0,
        stop: false,
    };

    pub fn start(&mut self, target: f32, frames: u32, stop: bool) {
        self.target = target;
        self.stop = stop;
        self.left = frames;
        if frames == 0 {
            self.value = target;
        } else {
            self.step = (target - self.value) / frames as f32;
        }
    }

    /// Restart from silence and rise to unity.
    pub fn fade_in(&mut self, frames: u32) {
        self.value = 0.0;
        self.start(1.0, frames, false);
    }

    pub fn dying(&self) -> bool {
        self.stop && self.target <= 0.0
    }

    pub fn finished(&self) -> bool {
        self.dying() && self.left == 0
    }

    pub fn value(&self) -> f32 {
        self.value
    }

    /// Not ramping: one gain all block.
    fn steady(&self) -> bool {
        self.left == 0
    }

    /// Advance over `frames` as [`Fade::apply`] would.
    fn skip(&mut self, frames: usize) {
        for _ in 0..frames.min(self.left as usize) {
            self.left -= 1;
            self.value = if self.left == 0 {
                self.target
            } else {
                self.value + self.step
            };
        }
    }

    fn apply(&mut self, amp: &mut [f32]) {
        if self.left == 0 {
            if self.value != 1.0 {
                amp.iter_mut().for_each(|a| *a *= self.value);
            }
            return;
        }
        for a in amp {
            if self.left > 0 {
                self.left -= 1;
                self.value = if self.left == 0 {
                    self.target
                } else {
                    self.value + self.step
                };
            }
            *a *= self.value;
        }
    }
}

/// A voice's streaming slot and how far its data is known to be published.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Stream {
    pub slot: u16,
    pub tag: u16,
    pub trusted: u64,
    /// Stopped while the voice is muted; the slot stays the voice's.
    pub paused: bool,
}

/// Voices whose blocks resample alike: the same destination, step, fraction
/// of the position and envelope curve. Resampling is linear, so their source
/// frames sum at the source rate, weighted by their gains, and the lane
/// interpolates once for all of them.
/// Packed in two words, so it passes in registers: `route` holds the step
/// (32.32, under 2^38), the bus plus one (0 for the output) at bit 48 and a
/// set top bit; `shape` the fraction of the position and the decay's bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Lane {
    route: std::num::NonZeroU64,
    shape: u64,
}

impl Lane {
    const STEP: u64 = (1 << 48) - 1;

    /// `step` and `base` in 32.32; `base` in `[1, 2)`, the window
    /// starting one frame before the position. `decay` is the bits of the
    /// per-frame decay factor, 0 for a held gain.
    fn new(bus: Option<u8>, step: u64, base: u64, decay: u32) -> Self {
        let bus = bus.map_or(0, |b| u64::from(b) + 1);
        Self {
            route: Self::route(bus << 48 | (step & Self::STEP)),
            shape: (base & 0xFFFF_FFFF) | u64::from(decay) << 32,
        }
    }

    const SOLO: u64 = 1 << 62;

    /// With the top bit set, never zero.
    fn route(bits: u64) -> std::num::NonZeroU64 {
        std::num::NonZeroU64::new(bits | 1 << 63).unwrap_or(std::num::NonZeroU64::MAX)
    }

    /// A voice alone in its lane, rendering itself: one per voice (see
    /// [`Lanes::add`]).
    fn solo(bus: Option<u8>) -> Self {
        let bus = bus.map_or(0, |b| u64::from(b) + 1);
        Self {
            route: Self::route(Self::SOLO | bus << 48),
            shape: 0,
        }
    }

    pub fn is_solo(&self) -> bool {
        self.route.get() & Self::SOLO != 0
    }

    pub fn bus(&self) -> Option<u8> {
        ((self.route.get() >> 48) as u8).checked_sub(1)
    }

    pub fn step(&self) -> u64 {
        self.route.get() & Self::STEP
    }

    pub fn base(&self) -> u64 {
        (1 << 32) | (self.shape & 0xFFFF_FFFF)
    }

    fn decay(&self) -> u32 {
        (self.shape >> 32) as u32
    }

    fn hash(&self) -> u64 {
        (self.route.get() ^ self.shape.rotate_left(29)).wrapping_mul(0x9E37_79B9_7F4A_7C15)
    }

    /// Source frames one block of `n` frames reads.
    pub fn count(&self, n: usize) -> usize {
        ((self.base() + self.step() * (n as u64 - 1)) >> 32) as usize + 4
    }

    /// Resample the summed window `acc` into `left`/`right` with the
    /// lane's envelope curve (`amp` is scratch).
    pub fn mix(&self, acc: &[Frame], amp: &mut [f32], left: &mut [f32], right: &mut [f32]) {
        let n = left.len();
        self.curve(&mut amp[..n]);
        mix(acc, self.base(), self.step(), &amp[..n], [1.0; 2], [0.0; 2], left, right);
    }

    /// The envelope's curve over the block, relative to the voices' weights.
    pub fn curve(&self, amp: &mut [f32]) {
        match self.decay() {
            0 => amp.fill(1.0),
            decay => _ = affine(amp, 1.0, (f32::from_bits(decay), 0.0)),
        }
    }
}

/// One block's voices by [`Lane`], in the order lanes first appear, each a
/// chain of its voices: an open-addressed table, so grouping is linear in
/// the voices, with no sort and no allocation.
pub(crate) struct Lanes {
    /// `(stamp, index into `lanes`)`; entries of older stamps are free.
    table: Box<[(u32, u16)]>,
    stamp: u32,
    /// Each lane, its filter class ([`Plan::class`]), its first and last
    /// voice and how many it has.
    pub lanes: Vec<(Lane, u64, u16, u16, u16)>,
    /// Each lane's filter, for classes other than 0.
    pub keys: Box<[FilterKey]>,
    /// The next voice of each voice's lane, and each voice's lane.
    next: Box<[u16]>,
    of: Box<[u16]>,
}

impl Lanes {
    pub fn new(voices: usize) -> Self {
        Self {
            table: vec![(0, 0); (2 * voices).next_power_of_two()].into_boxed_slice(),
            stamp: 0,
            lanes: Vec::with_capacity(voices),
            keys: vec![FilterKey::default(); voices].into_boxed_slice(),
            next: vec![0; voices].into_boxed_slice(),
            of: vec![0; voices].into_boxed_slice(),
        }
    }

    pub fn clear(&mut self) {
        self.stamp = self.stamp.wrapping_add(1);
        if self.stamp == 0 {
            self.table.fill((0, 0));
            self.stamp = 1;
        }
        self.lanes.clear();
    }

    #[inline(always)]
    pub fn add(&mut self, mut lane: Lane, class: u64, key: &FilterKey, voice: u16) {
        if lane.is_solo() {
            lane.shape = u64::from(voice);
        }
        let mask = self.table.len() - 1;
        let mut h = ((lane.hash() ^ class.wrapping_mul(0xD6E8_FEB8_6659_FD93)) >> 40) as usize & mask;
        loop {
            let (stamp, i) = self.table[h];
            if stamp != self.stamp {
                self.of[voice as usize] = self.lanes.len() as u16;
                self.table[h] = (self.stamp, self.lanes.len() as u16);
                if class != 0 {
                    self.keys[self.lanes.len()] = *key;
                }
                self.lanes.push((lane, class, voice, voice, 1));
                return;
            }
            let entry = &mut self.lanes[i as usize];
            if entry.0 == lane && entry.1 == class && (class == 0 || self.keys[i as usize] == *key) {
                self.of[voice as usize] = i;
                self.next[entry.3 as usize] = voice;
                (entry.3, entry.4) = (voice, entry.4 + 1);
                return;
            }
            h = (h + 1) & mask;
        }
    }

    /// `voice` has no lane this block.
    pub fn skip(&mut self, voice: u16) {
        self.of[voice as usize] = u16::MAX;
    }

    /// Whether `voice` renders in its lane this block: it shares it, or
    /// its filter.
    pub fn shared(&self, voice: u16) -> bool {
        self.lanes.get(self.of[voice as usize] as usize).is_some_and(|l| l.4 > 1 || l.1 != 0)
    }

    /// The voices of the lane starting at `first`, `count` of them.
    pub fn members(&self, first: u16, count: u16) -> impl Iterator<Item = u16> + '_ {
        std::iter::successors(Some(first), |&v| Some(self.next[v as usize])).take(count as usize)
    }
}

/// A voice's next block as [`Voice::plan`] works it out.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Plan {
    pub n: usize,
    /// Source frames per output frame, 32.32 fixed point.
    pub step: u64,
    pub target: [f32; 2],
    pub muted: bool,
    /// Output frames to the sample's end, when it ends within the block.
    pub declick: Option<f32>,
    /// Per-channel gain in its lane.
    pub weights: [f32; 2],
    /// The lane's group filter: 0 for none, else the hash of its key
    /// ([`VoiceFilter::held`]) and bus.
    pub class: u64,
}

/// One playing zone. Plain data; the engine owns the storage.
pub(crate) struct Voice {
    pub event: EventId,
    pub group: u32,
    pub voice_group: Option<u16>,
    pub channel: u8,
    pub note: u8,
    pub velocity: u8,
    /// Counter timing belongs to this event, independent of same-key retriggers.
    pub counter_start: u64,
    pub counter_stop: Option<u64>,
    pub owner: Option<(u8, u8)>,
    pub input_channel: Option<u8>,
    /// The key or event is still down.
    pub held: bool,
    /// This voice was held at the sostenuto pedal's down edge.
    pub sostenuto: bool,
    /// The envelope has been released.
    pub released: bool,
    /// Started by a note release; ignores later note-offs and pedal changes.
    pub release_trigger: bool,
    /// A former MPE note keeps its expression when its member channel is reused.
    pub frozen_expression: Option<super::Expression>,
    pub age: u64,
    pub sample: u32,
    pub span: u32,
    pub map: PlayMap,
    pub wraps: u64,
    /// Virtual path length for the current wraps.
    pub length: u64,
    /// Virtual frames below this are resident; the rest stream.
    pub limit: u64,
    /// Virtual playback position.
    pub pos: f64,
    /// Source frames per output frame before group tune, modulation and
    /// scripted tuning.
    pub step: f64,
    /// Scripted tuning ratio.
    pub tune: f64,
    /// Cached `(semitones, ratio)` of group, instrument and modulated pitch.
    pub pitch: (f32, f64),
    /// Current value of each of the group's voiced modulation assignments.
    pub mods: [f32; VOICE_MODS],
    /// The last modulation result `(gain, semitones)`, and the input stamp
    /// ([`Context::inputs`]) it holds for while the modulation is settled.
    pub modulated: (f32, f32),
    pub settled: Option<u32>,
    pub stream: Option<Stream>,
    /// AHDSR envelope (the engine defaults without one, unity with only a flex).
    pub env: Envelope,
    /// The group's flex envelope, multiplied with `env`.
    pub flex: Option<Envelope>,
    pub fade: Fade,
    /// Zone gain with velocity and key crossfades; group volume and
    /// modulation apply per block.
    pub base_level: f32,
    /// Scripted event volume.
    pub volume: f32,
    /// Zone pan.
    pub base_pan: f32,
    /// Scripted event pan, added to zone and group pan.
    pub pan: f32,
    /// Channel gains reached at the end of the last block.
    pub gains: [f32; 2],
    /// Frames the voice has been muted for, to pause its stream.
    pub muted: u32,
    /// Frames the voice may still wait, unmoved, for its first streamed
    /// frames; 0 once it plays (see [`Voice::waits`]).
    pub hold: u32,
    /// Group insert filter state (untouched when the group has none).
    pub filter: VoiceFilter,
    pub plan: Plan,
}

/// Borrowed state shared by all voices of one render block.
pub(crate) struct Context<'a> {
    pub bank: &'a Bank,
    pub slots: &'a [Slot],
    pub cc: &'a [[u8; 128]; 16],
    pub bend: &'a [f32; 16],
    pub mpe_zone: Option<(u8, u16)>,
    pub mpe_master_bend_range: Option<f32>,
    pub pressure: &'a [u8; 16],
    /// Per-channel/key MPE and note expression.
    pub expression: &'a [[super::Expression; 128]; 16],
    /// Instrument tune in semitones.
    pub tune: f32,
    pub rate: f32,
    pub blocking: bool,
    /// Changes whenever anything modulation reads may have: controllers,
    /// bend, pressure or group parameters.
    pub inputs: u32,
}

impl Context<'_> {
    /// Modulation inputs for a note on `channel`.
    pub fn inputs(&self, channel: u8, note: u8, velocity: u8, expression: super::Expression) -> Inputs<'_> {
        let c = channel as usize & 15;
        let master = self.mpe_zone.filter(|(_, members)| members & (1 << c) != 0).map(|(master, _)| master as usize);
        Inputs {
            cc: &self.cc[c],
            cc74: master.map(|m| expression.member_cc74.unwrap_or(self.cc[c][74]).saturating_add(self.cc[m][74]).min(127)),
            bend: self.bend[c] + master.map_or(0., |m| self.bend[m]),
            pressure: master.map_or(self.pressure[c], |m| expression.member_pressure.unwrap_or(self.pressure[c]).max(self.pressure[m])),
            note,
            velocity,
            // Fixed at the note start, never read live.
            counter: 0.0,
        }
    }
}

/// Preallocated per-engine render buffers.
pub(crate) struct Scratch {
    pub window: Box<[Frame]>,
    pub amp: [f32; MAX_BLOCK],
    pub flex: [f32; MAX_BLOCK],
    /// A filtered voice's own output before it joins the mix.
    pub out: [[f32; MAX_BLOCK]; 2],
    /// A lane's summed window.
    pub acc: Box<[Frame]>,
}

impl Default for Scratch {
    fn default() -> Self {
        Self {
            window: vec![[0.0; 2]; WINDOW].into_boxed_slice(),
            amp: [0.0; MAX_BLOCK],
            flex: [0.0; MAX_BLOCK],
            out: [[0.0; MAX_BLOCK]; 2],
            acc: vec![[0.0; 2]; WINDOW].into_boxed_slice(),
        }
    }
}

/// Balance law shared with the rack: the far side attenuates linearly.
#[inline]
pub(crate) fn balance(gain: f32, pan: f32) -> [f32; 2] {
    // Scripts set gains and pans: NaN or infinity silences, never poisons, the mix.
    if !gain.is_finite() || pan.is_nan() {
        return [0.0; 2];
    }
    [gain * (1.0 - pan.max(0.0)), gain * (1.0 + pan.min(0.0))]
}

impl Voice {
    /// Whether the voice sits out this block of `n` frames, silent and
    /// unmoved, waiting for its first streamed frames: it started with
    /// none resident (a bare bank, or a start offset past the resident
    /// range). Rather than lose its attack to an underrun it starts that
    /// much later, by at most [`START_HOLD`]. Offline renders wait in
    /// [`Voice::copy_streamed`] instead.
    pub fn waits(&mut self, cx: &Context, n: usize) -> bool {
        if self.hold == 0 {
            return false;
        }
        let need = (self.limit + START_LEAD).min(self.length);
        // The preload landed meanwhile (`Engine::upgrade_bank`): the start is
        // resident, or the stream is paused for the voice to reconfigure.
        let resident = self.limit > (self.pos as u64).saturating_sub(1);
        let ready = cx.blocking
            || resident
            || self.stream.is_none_or(|s| {
                s.paused || cx.slots[s.slot as usize].published(s.tag).is_some_and(|end| end >= need)
            });
        if ready || self.hold <= n as u32 {
            self.hold = 0;
            return false;
        }
        self.hold -= n as u32;
        true
    }

    /// Peak gain the voice ended its last block at, sample aside.
    pub fn level(&self) -> f32 {
        let flex = self.flex.as_ref().map_or(1.0, Envelope::level);
        self.gains[0].max(self.gains[1]) * self.env.level() * flex * self.fade.value()
    }

    /// Work out one block (at most [`MAX_BLOCK`] frames) before rendering
    /// it: modulation, pitch and gain into [`Voice::plan`], and the [`Lane`]
    /// it can mix in. `bus` is the group's, with its gains when it only
    /// passes to the output through a fader at rest.
    #[inline(always)]
    pub fn plan(&mut self, cx: &Context, n: usize, bus: Option<u8>, through: Option<[f32; 2]>) -> Option<Lane> {
        let group = &cx.bank.settings[self.group as usize];
        let key = self.owner.map_or(self.note, |(_, key)| key);
        let x = self.frozen_expression.unwrap_or(cx.expression[self.channel as usize & 15][key as usize & 127]);
        let inputs = cx.inputs(self.channel, self.note, self.velocity, x);
        let master = cx.mpe_zone.filter(|(_, members)| members & (1 << self.channel) != 0)
            .and_then(|(master, _)| cx.mpe_master_bend_range.map(|range| (cx.bend[master as usize], range)));
        let bend_pitch = master.map(|_| cx.bend[self.channel as usize]);
        // Settled modulation (every controller at rest, as held ones soon
        // are) gives the same result until an input changes: the group's
        // table, a cache miss per voice, is not read.
        if self.settled != Some(cx.inputs) {
            let (gain, semitones, settled) = group.mods.modulate(&mut self.mods, &inputs, n, cx.rate, bend_pitch);
            self.modulated = (gain, semitones);
            self.settled = settled.then_some(cx.inputs);
        }
        let (modulation, semitones) = self.modulated;
        let initial = self.pitch.0.is_nan();
        let semitones = semitones + group.tune + cx.tune + x.tune + master.map_or(0., |(bend, range)| bend * range);
        if semitones != self.pitch.0 {
            self.pitch = (semitones, 2f64.powf(f64::from(semitones) / 12.0));
        }
        let step = (self.step * self.tune * self.pitch.1).min(MAX_STEP);
        let level = self.base_level * group.gain * modulation * self.volume * x.gain;
        let target = balance(
            level,
            (self.base_pan + group.pan + self.pan + x.pan).clamp(-1.0, 1.0),
        );
        // Same-frame host expression may arrive after an unscripted note was
        // spawned. Start at the final initial value; later edits still ramp.
        if initial { self.gains = target; }
        let muted = target == [0.0; 2] && self.gains == [0.0; 2];
        // A sample ending mid-waveform ramps out over its last millisecond.
        let end = ((self.length as f64 - self.pos) / step) as f32;
        let declick = end < n as f32 + DECLICK * cx.rate;
        let mut plan = Plan {
            n,
            // 32.32 fixed point: exact, cheap to index.
            step: (step * FIXED_ONE) as u64,
            target,
            muted,
            declick: declick.then_some(end),
            weights: [0.0; 2],
            class: 0,
        };
        // A filter held all block is linear and time-invariant: voices of
        // any group with it at the same settings can share it.
        let class = match &group.filter {
            None => Some(0),
            Some(_) if muted => None,
            Some(f) => {
                self.filter.follow(f, &group.mods, &inputs, n, cx.rate);
                self.filter.hold(f, &group.mods, cx.rate).map(|hash| hash ^ bus.map_or(0, |b| u64::from(b) + 1) << 48)
            }
        };
        let mut lane = None;
        // One gain all block, before any filter: the voice's frames can be
        // summed, weighted, with others resampled alike.
        if let Some(class) = class.filter(|_| !muted && !declick && self.gains == target && self.fade.steady()) {
            let flex = match &self.flex {
                Some(env) => env.shape(n),
                None => Some(Shape::Flat(1.0)),
            };
            if let Some(shape) = self.env.shape(n).zip(flex).and_then(|(a, b)| a.times(b)) {
                let (level, decay) = match shape {
                    Shape::Flat(level) => (level, 0),
                    Shape::Decay(k, level) => (level, k.to_bits()),
                };
                let a = level * self.fade.value();
                let first = self.pos as i64 - 1;
                // Bus gains fold into the weights only ahead of no filter.
                let through = through.filter(|_| class == 0);
                let g = through.unwrap_or([1.0; 2]);
                plan.weights = [a * target[0] * g[0], a * target[1] * g[1]];
                plan.class = class;
                let base = ((self.pos - first as f64) * FIXED_ONE) as u64;
                lane = Some(Lane::new(bus.filter(|_| through.is_none()), plan.step, base, decay));
            }
        }
        // Resampled its own way, a voice still shares its filter.
        if let Some(class) = class.filter(|&c| c != 0 && lane.is_none()) {
            plan.class = class;
            lane = Some(Lane::solo(bus));
        }
        self.plan = plan;
        lane
    }

    /// Mix the planned block into `left`/`right`, through the group's
    /// filter unless `bare` (its lane filters). Returns `(alive, underrun)`.
    pub fn render(
        &mut self,
        cx: &Context,
        scratch: &mut Scratch,
        left: &mut [f32],
        right: &mut [f32],
        bare: bool,
    ) -> (bool, bool) {
        let Plan { n, step, target, muted, declick, .. } = self.plan;
        let group = &cx.bank.settings[self.group as usize];
        let own = group.filter.as_ref().filter(|_| !bare);
        let amp = &mut scratch.amp[..n];
        let flex = &mut scratch.flex[..n];
        if muted {
            // Unheard: the envelopes and fade only move on, to where
            // rendering them would have left them.
            self.env.skip(n, None, cx.rate);
            if let Some(env) = &mut self.flex {
                env.skip(n, group.flex.as_ref(), cx.rate);
            }
            self.fade.skip(n);
        } else {
            self.env.render(amp, None, cx.rate);
            if let Some(env) = &mut self.flex {
                env.render(flex, group.flex.as_ref(), cx.rate);
                amp.iter_mut().zip(flex.iter()).for_each(|(a, f)| *a *= f);
            }
            self.fade.apply(amp);
            if let Some(end) = declick {
                let declick = DECLICK * cx.rate;
                for (i, a) in amp.iter_mut().enumerate() {
                    *a *= ((end - i as f32) / declick).clamp(0.0, 1.0);
                }
            }
        }

        let mut underrun = false;
        if muted {
            // Muted all block, as scripts mute the crossfade layers and mic
            // positions not heard: nothing to render. The voice keeps its
            // place and envelope, and its filter rests as silence would
            // leave it, so it returns as if it had played on. Muted a while,
            // it stops streaming too: disk reads and decoding for voices no
            // one hears were most of the streamers' work.
            // Once is enough: only rendering (which unmutes) moves it again,
            // and its state is most of a muted voice's cache lines.
            if self.muted == 0 {
                self.filter.rest();
            }
            self.muted = self.muted.saturating_add(n as u32);
            // Still in the resident head, resuming is a voice start as usual.
            if self.muted as f32 >= PAUSE_AFTER * cx.rate || (self.pos as u64) < self.limit {
                self.pause(cx);
            }
        } else {
            let (window, base) = self.window(cx, &mut scratch.window, &mut underrun);
            let delta = [
                (target[0] - self.gains[0]) / n as f32,
                (target[1] - self.gains[1]) / n as f32,
            ];
            let [out_l, out_r] = &mut scratch.out;
            let (l, r) = match own {
                Some(_) => {
                    out_l[..n].fill(0.0);
                    out_r[..n].fill(0.0);
                    (&mut out_l[..n], &mut out_r[..n])
                }
                None => (&mut left[..n], &mut right[..n]),
            };
            mix(window, base, step, amp, self.gains, delta, l, r);
            self.gains = target;
            if let Some(filter) = own {
                let (l, r) = (&mut out_l[..n], &mut out_r[..n]);
                self.filter.process(filter, &group.mods, &mut scratch.flex, l, r, cx.rate);
                left[..n].iter_mut().zip(l.iter()).for_each(|(o, x)| *o += x);
                right[..n].iter_mut().zip(r.iter()).for_each(|(o, x)| *o += x);
            }
        }
        (self.advance(cx), underrun)
    }

    /// Add the planned block's source frames, times its lane weights, to
    /// `acc` (the lane's window): the lane resamples the sum once. Returns
    /// `(alive, underrun)`.
    pub fn accumulate(
        &mut self,
        cx: &Context,
        buf: &mut [Frame],
        acc: &mut [Frame],
        filter: Option<&mut LaneFilter>,
    ) -> (bool, bool) {
        let Plan { n, target, weights, .. } = self.plan;
        let group = &cx.bank.settings[self.group as usize];
        // The lane renders the envelope's curve; the voice's own only moves on.
        self.env.skip(n, None, cx.rate);
        if let Some(env) = &mut self.flex {
            env.skip(n, group.flex.as_ref(), cx.rate);
        }
        let (first, _, count) = self.reach();
        self.muted = 0;
        self.resume(cx);
        // Resident frames add in straight from storage, never decoded to memory.
        // Resident frames add in straight from storage, streamed ones from
        // the ring: never copied to memory first.
        let mut underrun = false;
        let resident = self.resident(cx.bank, first, count);
        if let Some(filter) = filter {
            // The frames also move the voice's filter state on: decoded once.
            filter.begin(&self.filter);
            let mut add = |src: &[Frame], at: usize| {
                accumulate(src, weights, &mut acc[at..]);
                filter.dots(src, at);
            };
            if let Some((span, at)) = resident {
                add(span.data.window(at, &mut buf[..count]).unwrap_or_default(), 0);
            } else if let Some([a, b]) = self.streamed(cx, first, count) {
                add(a, 0);
                add(b, a.len());
            } else {
                underrun = self.gather(cx, first, &mut buf[..count]);
                add(&buf[..count], 0);
            }
            filter.end(&mut self.filter, weights);
        } else if !resident.is_some_and(|(span, at)| span.data.accumulate(at, weights, acc)) {
            match self.streamed(cx, first, count) {
                Some([a, b]) => {
                    let (head, tail) = acc.split_at_mut(a.len());
                    accumulate(a, weights, head);
                    accumulate(b, weights, tail);
                }
                None => {
                    underrun = self.gather(cx, first, &mut buf[..count]);
                    accumulate(&buf[..count], weights, acc);
                }
            }
        }
        self.gains = target;
        (self.advance(cx), underrun)
    }

    /// The `count` frames from `first` in the stream ring, as its two
    /// runs, when all of them are streamed and published.
    fn streamed<'b>(&mut self, cx: &Context<'b>, first: i64, count: usize) -> Option<[&'b [Frame]; 2]> {
        let (Ok(v), Some(stream)) = (u64::try_from(first), self.stream.as_mut()) else {
            return None;
        };
        let need = v + count as u64;
        if v < self.limit || need > self.length {
            return None;
        }
        let slot = &cx.slots[stream.slot as usize];
        if let Some(end) = slot.published(stream.tag) {
            stream.trusted = stream.trusted.max(end);
        }
        slot.runs(v, count).filter(|_| stream.trusted >= need)
    }

    /// The planned block's source frames: the first (one before the
    /// position, the cubic's left tap), the position's offset from it
    /// (32.32) and how many.
    fn reach(&self) -> (i64, u64, usize) {
        let first = self.pos as i64 - 1;
        let base = ((self.pos - first as f64) * FIXED_ONE) as u64;
        let count = ((base + self.plan.step * (self.plan.n as u64 - 1)) >> 32) as usize + 4;
        (first, base, count)
    }

    /// The source frames the planned block reads (see [`Voice::reach`]) and
    /// the position's offset into them.
    fn window<'b>(&mut self, cx: &Context<'b>, buf: &'b mut [Frame], underrun: &mut bool) -> (&'b [Frame], u64) {
        self.muted = 0;
        self.resume(cx);
        let (first, base, count) = self.reach();
        let buf = &mut buf[..count];
        if let Some((span, at)) = self.resident(cx.bank, first, count) {
            // In range, so always `Some`.
            return (span.data.window(at, buf).unwrap_or_default(), base);
        }
        *underrun = self.gather(cx, first, buf);
        (buf, base)
    }

    /// Move past the planned block; whether the voice plays on.
    fn advance(&mut self, cx: &Context) -> bool {
        self.pos += (self.plan.step * self.plan.n as u64) as f64 / FIXED_ONE;
        if let Some(stream) = self.stream.filter(|s| !s.paused) {
            cx.slots[stream.slot as usize].release_below((self.pos as u64).saturating_sub(1));
        }
        !self.env.done()
            && !self.flex.as_ref().is_some_and(Envelope::done)
            && !self.fade.finished()
            && self.pos < self.length as f64
    }

    /// Stop streaming while muted, keeping the slot.
    fn pause(&mut self, cx: &Context) {
        if let Some(stream) = self.stream.as_mut().filter(|s| !s.paused && self.limit != FOREVER) {
            cx.slots[stream.slot as usize].stop();
            stream.paused = true;
        }
    }

    /// Stream again from the position (never from behind it, which would
    /// decode everything played while muted).
    fn resume(&mut self, cx: &Context) {
        if let Some(stream) = self.stream.as_mut().filter(|s| s.paused) {
            let first = (self.pos as u64).saturating_sub(1);
            let from = self.limit.max(first);
            let slot = &cx.slots[stream.slot as usize];
            stream.tag = slot.configure(self.sample, &self.map, self.wraps, from, first);
            (stream.trusted, stream.paused) = (from, false);
        }
    }

    /// Fast path: the window of `count` frames from `first` is one
    /// contiguous, unblended resident run: its span and offset there.
    fn resident<'b>(&self, bank: &'b Bank, first: i64, count: usize) -> Option<(&'b Span, usize)> {
        let v = u64::try_from(first).ok()?;
        if v + count as u64 > self.limit {
            return None;
        }
        let run = self.map.run(v, self.wraps)?;
        if run.reverse || run.blend.is_some() || run.len < count as u64 {
            return None;
        }
        let span = self.span(bank);
        let at = run.frame.checked_sub(span.start)? as usize;
        (at + count <= span.data.len()).then_some((span, at))
    }

    fn span<'b>(&self, bank: &'b Bank) -> &'b Span {
        &bank.samples[self.sample as usize].spans[self.span as usize]
    }

    /// Assemble the window from resident runs and the stream ring; returns
    /// true if streamed frames were missing (they play as silence).
    fn gather(&mut self, cx: &Context, first: i64, out: &mut [Frame]) -> bool {
        let lead = usize::try_from(-first).unwrap_or(0).min(out.len());
        out[..lead].fill([0.0; 2]);
        let mut i = lead;
        let mut underrun = false;
        while i < out.len() {
            let v = (first + i as i64) as u64;
            let rest = &mut out[i..];
            let n = if v < self.limit {
                let Some(run) = self.map.run(v, self.wraps) else {
                    rest.fill([0.0; 2]);
                    break;
                };
                let n = run.len.min(self.limit - v).min(rest.len() as u64) as usize;
                copy_run(self.span(cx.bank), &run, &mut rest[..n]);
                n
            } else {
                let n = self.length.saturating_sub(v).min(rest.len() as u64) as usize;
                if n == 0 {
                    rest.fill([0.0; 2]);
                    break;
                }
                underrun |= self.copy_streamed(cx, v, &mut rest[..n]);
                n
            };
            i += n;
        }
        underrun
    }

    fn copy_streamed(&mut self, cx: &Context, v: u64, out: &mut [Frame]) -> bool {
        let Some(stream) = &mut self.stream else {
            out.fill([0.0; 2]);
            return true;
        };
        let slot = &cx.slots[stream.slot as usize];
        let need = v + out.len() as u64;
        let refresh = |stream: &mut Stream| {
            if let Some(end) = slot.published(stream.tag) {
                stream.trusted = stream.trusted.max(end);
            }
        };
        refresh(stream);
        if cx.blocking && stream.trusted < need {
            let deadline = Instant::now() + OFFLINE_WAIT;
            while stream.trusted < need && Instant::now() < deadline {
                std::thread::yield_now();
                refresh(stream);
            }
        }
        let ready = stream.trusted.saturating_sub(v).min(out.len() as u64) as usize;
        slot.copy(v, &mut out[..ready]);
        out[ready..].fill([0.0; 2]);
        ready < out.len()
    }

    /// Release the envelope and, for release-ended loops, re-plan the path.
    /// Slots come from and return to `free`.
    pub fn release(&mut self, bank: &Bank, free: &mut Vec<u16>) {
        if self.released {
            return;
        }
        self.released = true;
        self.held = false;
        self.env.release(None);
        self.filter.release();
        if let Some(env) = &mut self.flex {
            env.release(bank.settings[self.group as usize].flex.as_ref());
        }
        let wraps = self.map.wraps(self.pos as u64 + 3);
        if wraps == self.wraps {
            return;
        }
        let diverge = self.map.divergence(wraps);
        self.wraps = wraps;
        self.length = self.map.len(wraps);
        let span = self.span(bank);
        let first = (self.pos as u64).saturating_sub(1);
        self.limit = self.map.resident_limit(first, wraps, span.start, span.end());
        let slots = bank.slots();
        if self.limit == FOREVER {
            if let Some(stream) = self.stream.take() {
                slots[stream.slot as usize].stop();
                free.push(stream.slot);
            }
            return;
        }
        // A paused stream restarts on the new path when the voice is heard.
        if self.stream.is_some_and(|s| s.paused) {
            return;
        }
        let (slot, trusted) = match self.stream {
            Some(stream) => (stream.slot, stream.trusted),
            None => match free.pop() {
                Some(slot) => (slot, self.limit),
                None => return,
            },
        };
        // Published frames before the divergence stay valid; restart after them.
        let from = self.limit.max(trusted.min(diverge));
        let tag =
            slots[slot as usize].configure(self.sample, &self.map, wraps, from, first);
        self.stream = Some(Stream {
            slot,
            tag,
            trusted: from,
            paused: false,
        });
    }
}

/// Copy a resident run into `out`, applying reverse order and loop crossfades.
fn copy_run(span: &Span, run: &Run, out: &mut [Frame]) {
    let n = out.len();
    let at = |frame: u64| frame.checked_sub(span.start).map(|f| f as usize);
    if run.reverse {
        let decoded = at(run.frame)
            .and_then(|top| (top + 1).checked_sub(n))
            .is_some_and(|low| span.data.decode(low, out));
        if decoded {
            out.reverse();
        } else {
            out.fill([0.0; 2]);
        }
        return;
    }
    if !at(run.frame).is_some_and(|s| span.data.decode(s, out)) {
        out.fill([0.0; 2]);
        return;
    }
    let (Some(blend), Some(partner)) = (run.blend, run.blend.and_then(|b| at(b.partner))) else {
        return;
    };
    // Partners decode in stack-sized chunks: no allocation on the audio thread.
    let mut partners = [[0.0; 2]; 64];
    for (c, chunk) in out.chunks_mut(partners.len()).enumerate() {
        let done = c * partners.len();
        let partners = &mut partners[..chunk.len()];
        if !span.data.decode(partner + done, partners) {
            return;
        }
        for (i, (o, p)) in chunk.iter_mut().zip(partners.iter()).enumerate() {
            *o = blend.apply((done + i) as u64, *o, *p);
        }
    }
}

/// 4-point, 3rd-order Hermite (Catmull-Rom) interpolation between `q[1]` and `q[2]`.
#[inline(always)]
fn hermite(q: &[Frame; 4], t: f32) -> Frame {
    std::array::from_fn(|c| {
        let (xm1, x0, x1, x2) = (q[0][c], q[1][c], q[2][c], q[3][c]);
        let c1 = 0.5 * (x1 - xm1);
        let c2 = xm1 - 2.5 * x0 + 2.0 * x1 - 0.5 * x2;
        let c3 = 0.5 * (x2 - xm1) + 1.5 * (x0 - x1);
        ((c3 * t + c2) * t + c1) * t + x0
    })
}

/// The inner loop: resample `window` from `base` by `step` (32.32 fixed
/// point), apply per-frame amplitude and ramped channel gains, and accumulate.
/// Dispatches to an AVX-512 kernel that works sixteen frames at a time, or
/// an AVX2 one that works eight (3.2x the scalar loop at 1000 voices), when
/// the CPU has them.
#[allow(clippy::too_many_arguments)]
fn mix(
    window: &[Frame],
    base: u64,
    step: u64,
    amp: &[f32],
    gains: [f32; 2],
    delta: [f32; 2],
    left: &mut [f32],
    right: &mut [f32],
) {
    // Whole steps from a whole position (a note on its root at the
    // sample's rate, or octaves up): the cubic is its centre tap, exactly.
    const FRACTION: u64 = (1 << 32) - 1;
    if step & FRACTION == 0 && base & FRACTION == 0 {
        #[cfg(target_arch = "x86_64")]
        if std::arch::is_x86_feature_detected!("avx2") {
            // SAFETY: the running CPU supports AVX2.
            return unsafe { taps_avx2(window, base >> 32, step >> 32, amp, gains, delta, left, right) };
        }
        return taps(window, base >> 32, step >> 32, amp, gains, delta, left, right);
    }
    #[cfg(target_arch = "x86_64")]
    if crate::audio::avx512() && std::arch::is_x86_feature_detected!("fma") {
        // SAFETY: the running CPU supports every feature `mix_avx512` is compiled for.
        return unsafe { mix_avx512(window, base, step, amp, gains, delta, left, right) };
    }
    #[cfg(target_arch = "x86_64")]
    if std::arch::is_x86_feature_detected!("avx2") && std::arch::is_x86_feature_detected!("fma") {
        // SAFETY: the running CPU supports every feature `mix_avx2` is compiled for.
        return unsafe { mix_avx2(window, base, step, amp, gains, delta, left, right, 0) };
    }
    mix_body(window, base, step, amp, gains, delta, left, right, 0);
}

/// [`mix_avx2`] sixteen frames a pass: each pair of rows (frames `k` and
/// `k + 8`) shares a register through the same in-lane transpose, and one
/// two-source permute per tap puts the sixteen frames back in order. The
/// same arithmetic, element for element; the rest goes eight at a time.
#[cfg(target_arch = "x86_64")]
#[allow(clippy::too_many_arguments)]
#[target_feature(enable = "avx2,fma,avx512f,avx512bw,avx512vl")]
fn mix_avx512(
    window: &[Frame],
    base: u64,
    step: u64,
    amp: &[f32],
    gains: [f32; 2],
    delta: [f32; 2],
    left: &mut [f32],
    right: &mut [f32],
) {
    use std::arch::x86_64::*;
    let n = left.len().min(right.len()).min(amp.len());
    let full = n / 16 * 16;
    let fits = full > 0
        && base >> 32 >= 1
        && ((base + step * (full as u64 - 1)) >> 32) as usize + 3 <= window.len();
    let done = if fits { full } else { 0 };
    let frames = window.as_ptr().cast::<f32>();
    let v = _mm512_set1_ps;
    let lanes = _mm512_setr_ps(0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0);
    let lane_steps = _mm512_mullo_epi32(
        _mm512_set1_epi32(step as u32 as i32),
        _mm512_setr_epi32(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15),
    );
    // Quarter q of the transposed pair (a, b) holds frames 4q..4q+4 of rows
    // (a) 0..4, (b) 4..8 in its low and high halves, and 8..16 above.
    let low = _mm512_setr_epi32(0, 1, 2, 3, 16, 17, 18, 19, 8, 9, 10, 11, 24, 25, 26, 27);
    let high = _mm512_setr_epi32(4, 5, 6, 7, 20, 21, 22, 23, 12, 13, 14, 15, 28, 29, 30, 31);
    for i in (0..done).step_by(16) {
        let p = base + step * i as u64;
        // SAFETY: `fits` bounds every frame's taps j-1..=j+2 inside `window`,
        // and i+16 <= n bounds `amp`, `left` and `right`.
        unsafe {
            let row = |k: u64| {
                let j = |k: u64| ((p + step * k) >> 32) as usize;
                let lo = _mm256_loadu_ps(frames.add(2 * (j(k) - 1)));
                let hi = _mm256_loadu_ps(frames.add(2 * (j(k + 8) - 1)));
                _mm512_castpd_ps(_mm512_insertf64x4(_mm512_castpd256_pd512(_mm256_castps_pd(lo)), _mm256_castps_pd(hi), 1))
            };
            let (r0, r1, r2, r3) = (row(0), row(1), row(2), row(3));
            let (r4, r5, r6, r7) = (row(4), row(5), row(6), row(7));
            let (t0, t1) = (_mm512_unpacklo_ps(r0, r1), _mm512_unpackhi_ps(r0, r1));
            let (t2, t3) = (_mm512_unpacklo_ps(r2, r3), _mm512_unpackhi_ps(r2, r3));
            let (t4, t5) = (_mm512_unpacklo_ps(r4, r5), _mm512_unpackhi_ps(r4, r5));
            let (t6, t7) = (_mm512_unpacklo_ps(r6, r7), _mm512_unpackhi_ps(r6, r7));
            let (s0, s1) = (_mm512_shuffle_ps(t0, t2, 0x44), _mm512_shuffle_ps(t0, t2, 0xEE));
            let (s2, s3) = (_mm512_shuffle_ps(t1, t3, 0x44), _mm512_shuffle_ps(t1, t3, 0xEE));
            let (s4, s5) = (_mm512_shuffle_ps(t4, t6, 0x44), _mm512_shuffle_ps(t4, t6, 0xEE));
            let (s6, s7) = (_mm512_shuffle_ps(t5, t7, 0x44), _mm512_shuffle_ps(t5, t7, 0xEE));
            // Tap k of channel c for all sixteen frames: column 2k + c.
            let taps = [
                [_mm512_permutex2var_ps(s0, low, s4), _mm512_permutex2var_ps(s1, low, s5)],
                [_mm512_permutex2var_ps(s2, low, s6), _mm512_permutex2var_ps(s3, low, s7)],
                [_mm512_permutex2var_ps(s0, high, s4), _mm512_permutex2var_ps(s1, high, s5)],
                [_mm512_permutex2var_ps(s2, high, s6), _mm512_permutex2var_ps(s3, high, s7)],
            ];
            let frac = _mm512_add_epi32(_mm512_set1_epi32(p as u32 as i32), lane_steps);
            let t = _mm512_mul_ps(
                _mm512_cvtepi32_ps(_mm512_srli_epi32(frac, 8)),
                v(1.0 / (1 << 24) as f32),
            );
            let a = _mm512_loadu_ps(amp.as_ptr().add(i));
            let fi = _mm512_add_ps(v(i as f32), lanes);
            for (c, out) in [left.as_mut_ptr(), right.as_mut_ptr()].into_iter().enumerate() {
                let (xm1, x0, x1, x2) = (taps[0][c], taps[1][c], taps[2][c], taps[3][c]);
                let c1 = _mm512_mul_ps(v(0.5), _mm512_sub_ps(x1, xm1));
                let c2 = _mm512_sub_ps(
                    _mm512_add_ps(
                        _mm512_sub_ps(xm1, _mm512_mul_ps(v(2.5), x0)),
                        _mm512_mul_ps(v(2.0), x1),
                    ),
                    _mm512_mul_ps(v(0.5), x2),
                );
                let c3 = _mm512_add_ps(
                    _mm512_mul_ps(v(0.5), _mm512_sub_ps(x2, xm1)),
                    _mm512_mul_ps(v(1.5), _mm512_sub_ps(x0, x1)),
                );
                let y = _mm512_add_ps(_mm512_mul_ps(c3, t), c2);
                let y = _mm512_add_ps(_mm512_mul_ps(y, t), c1);
                let y = _mm512_add_ps(_mm512_mul_ps(y, t), x0);
                let gain = _mm512_add_ps(v(gains[c]), _mm512_mul_ps(v(delta[c]), fi));
                let o = out.add(i);
                let sum = _mm512_mul_ps(_mm512_mul_ps(y, a), gain);
                _mm512_storeu_ps(o, _mm512_add_ps(_mm512_loadu_ps(o), sum));
            }
        }
    }
    mix_avx2(window, base, step, amp, gains, delta, left, right, done);
}

#[cfg(target_arch = "x86_64")]
#[allow(clippy::too_many_arguments)]
#[target_feature(enable = "avx2")]
fn taps_avx2(
    window: &[Frame],
    first: u64,
    stride: u64,
    amp: &[f32],
    gains: [f32; 2],
    delta: [f32; 2],
    left: &mut [f32],
    right: &mut [f32],
) {
    taps(window, first, stride, amp, gains, delta, left, right);
}

/// [`mix`] at a whole step from a whole position: frame `i` is window
/// frame `first + i * stride`, the value the cubic takes there, and the
/// gains apply in `mix_body`'s order, so the output is the same bit for bit.
#[allow(clippy::too_many_arguments)]
#[inline(always)]
fn taps(
    window: &[Frame],
    first: u64,
    stride: u64,
    amp: &[f32],
    gains: [f32; 2],
    delta: [f32; 2],
    left: &mut [f32],
    right: &mut [f32],
) {
    let first = first as usize;
    let n = left.len().min(right.len()).min(amp.len());
    let frames = window.get(first..).unwrap_or_default();
    let (left, right, amp) = (&mut left[..n], &mut right[..n], &amp[..n]);
    if stride == 1 {
        for (i, (((l, r), a), x)) in left.iter_mut().zip(right.iter_mut()).zip(amp).zip(frames).enumerate() {
            let fi = i as f32;
            *l += x[0] * a * (gains[0] + delta[0] * fi);
            *r += x[1] * a * (gains[1] + delta[1] * fi);
        }
        return;
    }
    let frames = frames.iter().step_by(stride as usize);
    for (i, (((l, r), a), x)) in left.iter_mut().zip(right.iter_mut()).zip(amp).zip(frames).enumerate() {
        let fi = i as f32;
        *l += x[0] * a * (gains[0] + delta[0] * fi);
        *r += x[1] * a * (gains[1] + delta[1] * fi);
    }
}

/// `acc += window · weights` per channel, over `acc`: one voice's share
/// of its [`Lane`].
fn accumulate(window: &[Frame], weights: [f32; 2], acc: &mut [Frame]) {
    #[cfg(target_arch = "x86_64")]
    if std::arch::is_x86_feature_detected!("avx2") {
        // SAFETY: the running CPU supports AVX2.
        return unsafe { accumulate_avx2(window, weights, acc) };
    }
    accumulate_body(window, weights, acc);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
fn accumulate_avx2(window: &[Frame], weights: [f32; 2], acc: &mut [Frame]) {
    accumulate_body(window, weights, acc);
}

#[inline(always)]
fn accumulate_body(window: &[Frame], weights: [f32; 2], acc: &mut [Frame]) {
    let n = acc.len().min(window.len());
    let (acc, window) = (acc[..n].as_flattened_mut(), window[..n].as_flattened());
    let w: [f32; 16] = std::array::from_fn(|k| weights[k & 1]);
    let (chunks, tail) = acc.as_chunks_mut::<16>();
    let (xs, xtail) = window.as_chunks::<16>();
    for (a, x) in chunks.iter_mut().zip(xs) {
        for k in 0..16 {
            a[k] += w[k] * x[k];
        }
    }
    for (k, (a, x)) in tail.iter_mut().zip(xtail).enumerate() {
        *a += w[k] * x;
    }
}

#[cfg(target_arch = "x86_64")]
#[allow(clippy::too_many_arguments)]
#[target_feature(enable = "avx2,fma")]
fn mix_avx2(
    window: &[Frame],
    base: u64,
    step: u64,
    amp: &[f32],
    gains: [f32; 2],
    delta: [f32; 2],
    left: &mut [f32],
    right: &mut [f32],
    from: usize,
) {
    use std::arch::x86_64::*;
    let n = left.len().min(right.len()).min(amp.len());
    // Eight frames per pass: each loads its four stereo taps with one
    // 256-bit load, an 8x8 transpose turns them into one vector per tap and
    // channel, and the Hermite runs across the eight frames at once. The
    // arithmetic matches `mix_body` operation for operation, so the output
    // is bit-identical whichever path a frame takes.
    // From frame `from` on, eight at a time.
    let full = from + n.saturating_sub(from) / 8 * 8;
    let fits = full > from
        && base >> 32 >= 1
        && ((base + step * (full as u64 - 1)) >> 32) as usize + 3 <= window.len();
    let done = if fits { full } else { from };
    let frames = window.as_ptr().cast::<f32>();
    let v = _mm256_set1_ps;
    let lanes = _mm256_setr_ps(0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0);
    let lane_steps = _mm256_mullo_epi32(
        _mm256_set1_epi32(step as u32 as i32),
        _mm256_setr_epi32(0, 1, 2, 3, 4, 5, 6, 7),
    );
    for i in (from..done).step_by(8) {
        let p = base + step * i as u64;
        // SAFETY: `fits` bounds every frame's taps j-1..=j+2 inside `window`,
        // and i+8 <= n bounds `amp`, `left` and `right`.
        unsafe {
            let row = |k: u64| {
                let j = ((p + step * k) >> 32) as usize;
                _mm256_loadu_ps(frames.add(2 * (j - 1)))
            };
            let (r0, r1, r2, r3) = (row(0), row(1), row(2), row(3));
            let (r4, r5, r6, r7) = (row(4), row(5), row(6), row(7));
            let (t0, t1) = (_mm256_unpacklo_ps(r0, r1), _mm256_unpackhi_ps(r0, r1));
            let (t2, t3) = (_mm256_unpacklo_ps(r2, r3), _mm256_unpackhi_ps(r2, r3));
            let (t4, t5) = (_mm256_unpacklo_ps(r4, r5), _mm256_unpackhi_ps(r4, r5));
            let (t6, t7) = (_mm256_unpacklo_ps(r6, r7), _mm256_unpackhi_ps(r6, r7));
            let (s0, s1) = (_mm256_shuffle_ps(t0, t2, 0x44), _mm256_shuffle_ps(t0, t2, 0xEE));
            let (s2, s3) = (_mm256_shuffle_ps(t1, t3, 0x44), _mm256_shuffle_ps(t1, t3, 0xEE));
            let (s4, s5) = (_mm256_shuffle_ps(t4, t6, 0x44), _mm256_shuffle_ps(t4, t6, 0xEE));
            let (s6, s7) = (_mm256_shuffle_ps(t5, t7, 0x44), _mm256_shuffle_ps(t5, t7, 0xEE));
            // Tap k of channel c for all eight frames: column 2k + c.
            let taps = [
                [_mm256_permute2f128_ps(s0, s4, 0x20), _mm256_permute2f128_ps(s1, s5, 0x20)],
                [_mm256_permute2f128_ps(s2, s6, 0x20), _mm256_permute2f128_ps(s3, s7, 0x20)],
                [_mm256_permute2f128_ps(s0, s4, 0x31), _mm256_permute2f128_ps(s1, s5, 0x31)],
                [_mm256_permute2f128_ps(s2, s6, 0x31), _mm256_permute2f128_ps(s3, s7, 0x31)],
            ];
            let frac = _mm256_add_epi32(_mm256_set1_epi32(p as u32 as i32), lane_steps);
            let t = _mm256_mul_ps(
                _mm256_cvtepi32_ps(_mm256_srli_epi32(frac, 8)),
                v(1.0 / (1 << 24) as f32),
            );
            let a = _mm256_loadu_ps(amp.as_ptr().add(i));
            let fi = _mm256_add_ps(v(i as f32), lanes);
            for (c, out) in [left.as_mut_ptr(), right.as_mut_ptr()].into_iter().enumerate() {
                let (xm1, x0, x1, x2) = (taps[0][c], taps[1][c], taps[2][c], taps[3][c]);
                let c1 = _mm256_mul_ps(v(0.5), _mm256_sub_ps(x1, xm1));
                let c2 = _mm256_sub_ps(
                    _mm256_add_ps(
                        _mm256_sub_ps(xm1, _mm256_mul_ps(v(2.5), x0)),
                        _mm256_mul_ps(v(2.0), x1),
                    ),
                    _mm256_mul_ps(v(0.5), x2),
                );
                let c3 = _mm256_add_ps(
                    _mm256_mul_ps(v(0.5), _mm256_sub_ps(x2, xm1)),
                    _mm256_mul_ps(v(1.5), _mm256_sub_ps(x0, x1)),
                );
                let y = _mm256_add_ps(_mm256_mul_ps(c3, t), c2);
                let y = _mm256_add_ps(_mm256_mul_ps(y, t), c1);
                let y = _mm256_add_ps(_mm256_mul_ps(y, t), x0);
                let gain = _mm256_add_ps(v(gains[c]), _mm256_mul_ps(v(delta[c]), fi));
                let o = out.add(i);
                let sum = _mm256_mul_ps(_mm256_mul_ps(y, a), gain);
                _mm256_storeu_ps(o, _mm256_add_ps(_mm256_loadu_ps(o), sum));
            }
        }
    }
    mix_body(window, base, step, amp, gains, delta, left, right, done);
}

#[allow(clippy::too_many_arguments)]
#[inline(always)]
fn mix_body(
    window: &[Frame],
    base: u64,
    step: u64,
    amp: &[f32],
    gains: [f32; 2],
    delta: [f32; 2],
    left: &mut [f32],
    right: &mut [f32],
    from: usize,
) {
    let frames = left.iter_mut().zip(right.iter_mut()).zip(amp);
    for (i, ((l, r), a)) in frames.enumerate().skip(from) {
        let p = base + step * i as u64;
        let j = (p >> 32) as usize;
        let t = ((p as u32) >> 8) as f32 * (1.0 / (1 << 24) as f32);
        let Some(q) = window.get(j - 1..).and_then(<[Frame]>::first_chunk::<4>) else {
            break;
        };
        let [yl, yr] = hermite(q, t);
        let fi = i as f32;
        *l += yl * a * (gains[0] + delta[0] * fi);
        *r += yr * a * (gains[1] + delta[1] * fi);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The AVX2 and centre-tap kernels match the scalar one bit for bit, at
    /// any step, length and window slack (short windows fall back to scalar).
    #[test]
    fn simd_mix_matches_scalar_exactly() {
        let window: Vec<Frame> = (0..700)
            .map(|i| [(i as f32 * 0.37).sin(), (i as f32 * 0.11).cos()])
            .collect();
        let amp: Vec<f32> = (0..MAX_BLOCK).map(|i| 1.0 - i as f32 / 300.0).collect();
        // Whole steps from a whole position take the centre-tap path.
        for (step, n, len, base) in [
            (1.0, 128, 700, 1.3),
            (0.2718, 128, 700, 1.3),
            (1.3717, 77, 700, 1.3),
            (4.0, 128, 700, 1.3),
            (1.0, 128, 130, 1.3),
            (1.0, 7, 12, 1.3),
            (1.0, 128, 700, 1.0),
            (2.0, 128, 700, 1.0),
            (3.0, 77, 700, 1.0),
            (1.0, 128, 132, 1.0),
            // Sixteen at a time, then eight, then single frames.
            (0.8123, 31, 700, 1.7),
            (1.9, 16, 40, 1.2),
            (0.5, 128, 67, 1.9),
        ] {
            let step = (step * FIXED_ONE) as u64;
            let base = (base * FIXED_ONE) as u64;
            type Kernel = fn(&[Frame], u64, u64, &[f32], [f32; 2], [f32; 2], &mut [f32], &mut [f32]);
            // Every kernel this CPU runs, not only the one dispatch picks.
            let mut kernels: Vec<Kernel> = vec![mix];
            #[cfg(target_arch = "x86_64")]
            {
                use std::arch::is_x86_feature_detected as has;
                if has!("avx2") && has!("fma") {
                    // SAFETY: the running CPU supports AVX2 and FMA.
                    kernels.push(|w, b, s, a, g, d, l, r| unsafe { mix_avx2(w, b, s, a, g, d, l, r, 0) });
                }
                if crate::audio::avx512() && has!("fma") {
                    // SAFETY: the running CPU supports AVX-512 F, BW and VL, AVX2 and FMA.
                    kernels.push(|w, b, s, a, g, d, l, r| unsafe { mix_avx512(w, b, s, a, g, d, l, r) });
                }
            }
            let run = |kernel: Option<Kernel>| {
                let (mut l, mut r) = (vec![0.25; n], vec![-0.5; n]);
                let args = (&window[..len], base, step, &amp[..n], [0.7, 0.3], [0.001, -0.002]);
                match kernel {
                    Some(k) => k(args.0, args.1, args.2, args.3, args.4, args.5, &mut l, &mut r),
                    None => mix_body(args.0, args.1, args.2, args.3, args.4, args.5, &mut l, &mut r, 0),
                }
                (l, r)
            };
            for kernel in kernels {
                assert_eq!(run(Some(kernel)), run(None), "step {step:#x}, {n} frames");
            }
        }
    }

    /// Nanoseconds per 128-frame block of each mix kernel, best of many:
    /// `cargo test --release --lib mix_speed -- --ignored --nocapture`
    #[test]
    #[ignore]
    #[cfg(target_arch = "x86_64")]
    fn mix_speed() {
        use std::arch::is_x86_feature_detected as has;
        let window: Vec<Frame> = (0..WINDOW).map(|i| [(i as f32 * 0.37).sin(), (i as f32 * 0.11).cos()]).collect();
        let amp = [0.9f32; MAX_BLOCK];
        let (mut l, mut r) = ([0f32; MAX_BLOCK], [0f32; MAX_BLOCK]);
        let base = (1.3 * FIXED_ONE) as u64;
        type Kernel = fn(&[Frame], u64, u64, &[f32], [f32; 2], [f32; 2], &mut [f32], &mut [f32]);
        let mut kernels: Vec<(&str, Kernel)> =
            vec![("scalar", |w, b, s, a, g, d, l, r| mix_body(w, b, s, a, g, d, l, r, 0))];
        if has!("avx2") && has!("fma") {
            // SAFETY: the running CPU supports AVX2 and FMA.
            kernels.push(("avx2", |w, b, s, a, g, d, l, r| unsafe { mix_avx2(w, b, s, a, g, d, l, r, 0) }));
        }
        if crate::audio::avx512() && has!("fma") {
            // SAFETY: the running CPU supports AVX-512 F, BW and VL, AVX2 and FMA.
            kernels.push(("avx512", |w, b, s, a, g, d, l, r| unsafe { mix_avx512(w, b, s, a, g, d, l, r) }));
        }
        for step in [0.53, 1.06, 1.87] {
            let step = (step * FIXED_ONE) as u64;
            for (name, kernel) in &kernels {
                let mut best = f64::MAX;
                for _ in 0..200 {
                    let t = std::time::Instant::now();
                    for _ in 0..100 {
                        kernel(&window, base, step, &amp, [0.7, 0.3], [0.0; 2], &mut l, &mut r);
                    }
                    best = best.min(t.elapsed().as_secs_f64() / 100.0);
                }
                println!("step {:.2} {name}: {:.0} ns per block", step as f64 / FIXED_ONE, best * 1e9);
            }
        }
        assert!(l[0].is_finite());
    }

    #[test]
    fn envelope_stages() {
        let rate = 1000.0;
        let p = Ahdsr {
            attack: 0.01,
            curve: 0.0,
            hold: 0.005,
            decay: 0.1,
            sustain: 0.5,
            release: 0.1,
        };
        let mut env = Envelope::new(&p, rate);
        let mut out = [0.0; 400];
        env.render(&mut out, None, rate);
        assert!((out[4] - 0.5).abs() < 1e-5, "linear attack");
        let peak = out.iter().position(|&x| x == 1.0).unwrap();
        assert!((9..=10).contains(&peak));
        assert!(out[peak..=peak + 5].iter().all(|&x| x == 1.0), "hold");
        // −60 dB of the distance to sustain after the decay time.
        assert!(
            (out[peak + 105] - 0.5005).abs() < 1e-5,
            "{}",
            out[peak + 105]
        );
        assert_eq!(out[399], 0.5);
        env.release(None);
        env.render(&mut out[..100], None, rate);
        assert!(
            (out[99] - 0.0005).abs() < 1e-4,
            "release reaches −60 dB at its time"
        );
        env.render(&mut out[..100], None, rate);
        assert!(env.done());
    }

    /// A muted voice's envelope skips its frames yet lands exactly where
    /// rendering them would: un-muted or released later, it sounds the same.
    #[test]
    fn skipped_envelopes_land_where_rendered_ones_do() {
        let rate = 48_000.0;
        let flex = Flex {
            points: [
                FlexPoint { seconds: 0.013, level: 1.0, curve: 0.7 },
                FlexPoint { seconds: 0.05, level: 0.3, curve: -0.4 },
                FlexPoint { seconds: 0.2, level: 0.0, curve: 0.2 },
            ]
            .into(),
            sustain: 1,
        };
        let ahdsr = |curve| Ahdsr { attack: 0.011, curve, hold: 0.003, decay: 0.07, sustain: 0.4, release: 0.09 };
        let cases = [
            (Envelope::new(&ahdsr(0.0), rate), None),
            (Envelope::new(&ahdsr(0.8), rate), None),
            (Envelope::new(&ahdsr(-0.6), rate), None),
            (Envelope::new(&Ahdsr { sustain: 0.0, ..ahdsr(1.0) }, rate), None),
            (Envelope::new(&Ahdsr { attack: 1e-7, decay: 0.0, ..ahdsr(-1.0) }, rate), None),
            (Envelope::new(&Ahdsr { attack: 0.3, sustain: 1.0, ..ahdsr(0.3) }, rate), None),
            (Envelope::flex(), Some(&flex)),
        ];
        for (start, flex) in cases {
            let (mut rendered, mut skipped) = (start, start);
            let mut out = [0.0; MAX_BLOCK];
            // Uneven blocks, released partway, until both are done.
            for block in 0..400 {
                if block == 60 {
                    rendered.release(flex);
                    skipped.release(flex);
                }
                let n = [128, 77, 1, 8, 13][block % 5];
                rendered.render(&mut out[..n], flex, rate);
                skipped.skip(n, flex, rate);
                assert_eq!(rendered.level().to_bits(), skipped.level().to_bits(), "block {block}");
                assert_eq!((rendered.stage, rendered.left), (skipped.stage, skipped.left), "block {block}");
            }
            assert!(skipped.done());
        }
    }

    /// No frame of a chunk from a level short of a glide's edge stops it,
    /// and one from the edge does: checked frame by frame.
    #[test]
    fn edges_leave_no_stopping_frame_short_of_them() {
        let rate = 48_000.0;
        let frames = |x: f32, (pow, off): ([f32; 8], [f32; 8])| std::array::from_fn::<f32, 8, _>(|k| pow[k] * x + off[k]);
        let mut seed = 7u32;
        let mut level = move || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 8) as f32 / (1 << 24) as f32 * 1.5
        };
        let attacks = [1.0, 3.7, 100.0, 4800.0, 480_000.0]
            .into_iter()
            .flat_map(|n| [-1.0, -0.3, 0.0, 0.5, 1.0].map(|c| glide(0.0, 1.0, n, c)));
        let decays = [0.0, 0.001, 0.07, 30.0]
            .into_iter()
            .flat_map(|t| [0.0, 0.4, 1.0].map(|s| (exp_coef(t, rate), s)));
        let mut cases: Vec<((f32, f32), bool, Box<dyn Fn(f32) -> bool>)> = Vec::new();
        cases.extend(attacks.map(|step| (step, true, Box::new(|x: f32| x >= 1.0) as Box<dyn Fn(f32) -> bool>)));
        cases.extend(decays.map(|(d, s)| ((d, s * (1.0 - d)), false, Box::new(move |x: f32| x - s <= SILENT) as _)));
        for (step, rising, stop) in &cases {
            let e = edge(*step, *rising, stop);
            let short = if *rising { Short::Below(e) } else { Short::Above(e) };
            let stops = |x: f32| frames(x, powers(*step)).into_iter().any(|y| stop(y));
            if e.is_finite() {
                let before = if *rising { e.next_down() } else { e.next_up() };
                assert!(short.holds(before) && !stops(before), "{step:?}");
                assert!(!short.holds(e) && stops(e), "{step:?}");
            }
            for _ in 0..2000 {
                let x = level();
                assert!(!short.holds(x) || !stops(x), "{step:?} from {x}");
            }
        }
    }

    #[test]
    fn affine_steps_match_frame_by_frame() {
        for (mul, add) in [(1.0, 0.01), (0.9993, 0.0007), (1.02, -0.001), (0.0, 0.5)] {
            for len in [0, 1, 7, 8, 9, 127, 128] {
                let mut out = vec![0.0; len];
                let last = affine(&mut out, 0.25, (mul, add));
                let mut x = 0.25f32;
                for (i, &o) in out.iter().enumerate() {
                    x = x * mul + add;
                    assert!((o - x).abs() <= 1e-5 * x.abs().max(1.0), "{mul} {add} at {i}: {o} vs {x}");
                }
                assert_eq!(last, out.last().copied().unwrap_or(0.25));
            }
        }
    }

    /// The editor's envelope drawing follows the laws voices play by.
    #[test]
    fn trace_follows_the_envelope_laws() {
        let env = Ahdsr { attack: 0.3, curve: 0.0, hold: 0.1, decay: 2.0, sustain: 0.25, release: 4.0 };
        let [attack, decay, release] = env.trace(100);
        assert_eq!((attack.len(), decay.len(), release.len()), (101, 101, 101));
        assert_eq!(attack[0], 0.0);
        assert!((attack[50] - 0.5).abs() < 1e-4, "linear attack: {}", attack[50]);
        assert!((attack[100] - 1.0).abs() < 1e-4);
        // −60 dB of the distance to sustain over the decay time.
        assert_eq!(decay[0], 1.0);
        let at = |t: f32| 0.25 + 0.75 * 0.001f32.powf(t);
        assert!((decay[50] - at(0.5)).abs() < 1e-4, "{} vs {}", decay[50], at(0.5));
        assert_eq!(release[0], 0.25);
        assert!((release[50] - 0.25 * 0.001f32.powf(0.5)).abs() < 1e-4);
        // A convex attack rises early; a stage of no time is a point.
        let fast = Ahdsr { curve: 1.0, decay: 0.0, ..env }.trace(100);
        assert!(fast[0][25] > 0.5);
        assert_eq!(fast[1], vec![0.25]);
    }

    #[test]
    fn sub_frame_curved_attack_is_instant_not_nan() {
        for curve in [-1.0, 1.0] {
            let p = Ahdsr { attack: 1e-7, curve, hold: 0.0, decay: 0.0, sustain: 1.0, release: 0.3 };
            let mut out = [0.0; 8];
            Envelope::new(&p, 48_000.0).render(&mut out, None, 48_000.0);
            assert!(out[2..].iter().all(|&x| x == 1.0), "{curve}: {out:?}");
        }
    }

    #[test]
    fn attack_curve_bends_and_keeps_its_time() {
        let rate = 1000.0;
        let attack = |curve| {
            let p = Ahdsr {
                attack: 0.1,
                curve,
                hold: 0.0,
                decay: 0.0,
                sustain: 1.0,
                release: 0.1,
            };
            let mut out = [0.0; 120];
            Envelope::new(&p, rate).render(&mut out, None, rate);
            out
        };
        // (1 - e^(-k t)) / (1 - e^(-k)), k = 5 · curve.
        let expected = |k: f32, t: f32| (-k * t).exp_m1() / (-k).exp_m1();
        for curve in [-1.0, -0.33, 0.0, 0.5, 1.0] {
            let out = attack(curve);
            for frame in [19, 49, 79] {
                let t = (frame + 1) as f32 / 100.0;
                let want = if curve == 0.0 {
                    t
                } else {
                    expected(5.0 * curve, t)
                };
                assert!(
                    (out[frame] - want).abs() < 1e-3,
                    "curve {curve} at {t}: {} vs {want}",
                    out[frame]
                );
            }
            let peak = out.iter().position(|&x| x == 1.0).unwrap();
            assert!((99..=100).contains(&peak), "curve {curve} peaks at {peak}");
        }
        // Positive is convex (fast rise), negative concave (slow start).
        assert!(attack(1.0)[19] > 0.6 && attack(-1.0)[49] < 0.08);
    }

    #[test]
    fn flex_envelope_glides_holds_and_releases() {
        let rate = 1000.0;
        let point = |seconds, level, curve| FlexPoint {
            seconds,
            level,
            curve,
        };
        let flex = Flex {
            points: [
                point(0.01, 1.0, 0.0),
                point(0.01, 0.5, 0.0),
                point(0.02, 0.0, 1.0),
            ]
            .into(),
            sustain: 1,
        };
        let mut env = Envelope::flex();
        let mut out = [0.0; 100];
        env.render(&mut out, Some(&flex), rate);
        assert!((out[4] - 0.5).abs() < 1e-6 && out[9] == 1.0, "attack");
        assert!((out[14] - 0.75).abs() < 1e-6 && out[19] == 0.5, "decay");
        assert!(out[20..].iter().all(|&x| x == 0.5), "sustain point holds");
        env.release(Some(&flex));
        env.render(&mut out[..30], Some(&flex), rate);
        // Convex release: more than halfway down after a quarter of its time.
        assert!(out[4] < 0.25, "{}", out[4]);
        assert_eq!(out[19], 0.0);
        assert!(env.done());

        // Released before the sustain point: straight to the release segment.
        let mut env = Envelope::flex();
        env.render(&mut out[..5], Some(&flex), rate);
        env.release(Some(&flex));
        env.render(&mut out[..30], Some(&flex), rate);
        assert!(out[0] < 0.5 && env.done());
    }
}
