//! Literal arithmetic oracle from v1 0cb7a8a0; test-only, not linked into the engine.
#![allow(dead_code, private_interfaces)]
use crate::v1_voice_controls::{
    Ahdsr, Flex, Inputs, Mod, Phase, PitchLfo, Source, Target, VolumeLfo,
};
const MAX_STEP: f64 = 32.;
const FIXED_ONE: f64 = (1u64 << 32) as f64;
const SILENT: f32 = 1e-4;
const VOICE_MODS: usize = 8;
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
    ahd_only: bool,
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
            (Self::Flat(a), Self::Decay(k, b)) | (Self::Decay(k, b), Self::Flat(a)) => {
                Some(Self::Decay(k, a * b))
            }
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
    tail.iter()
        .position(|&x| stop(x))
        .map(|k| chunks.len() * 8 + k)
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
            sustain: if p.ahd_only {
                0.
            } else {
                p.sustain.clamp(0.0, 1.0)
            },
            release: exp_coef(p.release, rate),
            ahd_only: p.ahd_only,
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
            ahd_only: false,
            edge: f32::NAN,
        }
    }

    /// Enter the release: the flex segment after the sustain point, or the
    /// AHDSR release.
    pub fn release(&mut self, flex: Option<&Flex>) {
        if self.ahd_only && flex.is_none() {
            return;
        }
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
                    let (level, peak) =
                        affine_until(rest.as_deref_mut(), n, self.level, self.step, stop, short);
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
                    (self.level, _) = affine_until(
                        rest.as_deref_mut(),
                        m,
                        self.level,
                        self.step,
                        |_| false,
                        Short::Unknown,
                    );
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
                            self.stage = if self.ahd_only {
                                Stage::Done
                            } else {
                                Stage::Sustain
                            };
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
                        Some(rest) => affine_until(
                            Some(rest),
                            n,
                            self.level,
                            (self.release, 0.0),
                            |x| x < SILENT,
                            Short::Unknown,
                        ),
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

/// Primary amplitude consumer; Flex and pitch/module sources keep their
/// independently admitted clocks in `Envelope`.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Amplitude {
    Ordinary(Envelope),
    Native(native::Native),
}
impl Amplitude {
    pub fn new(p: &Ahdsr, rate: f32, native: bool) -> Self {
        if native {
            Self::Native(native::Native::new(p, rate))
        } else {
            Self::Ordinary(Envelope::new(p, rate))
        }
    }
    pub fn release(&mut self, flex: Option<&Flex>) {
        match self {
            Self::Ordinary(e) => e.release(flex),
            Self::Native(e) => e.release(),
        }
    }
    pub fn done(&self) -> bool {
        match self {
            Self::Ordinary(e) => e.done(),
            Self::Native(e) => e.done(),
        }
    }
    pub fn level(&self) -> f32 {
        match self {
            Self::Ordinary(e) => e.level(),
            Self::Native(e) => e.level(),
        }
    }
    pub fn phase(&self) -> Phase {
        match self {
            Self::Ordinary(e) => e.phase(),
            Self::Native(e) => e.phase(),
        }
    }
    fn shape(&self, n: usize) -> Option<Shape> {
        match self {
            Self::Ordinary(e) => e.shape(n),
            Self::Native(_) => None,
        }
    }
    pub fn skip(&mut self, n: usize, flex: Option<&Flex>, rate: f32) {
        match self {
            Self::Ordinary(e) => e.skip(n, flex, rate),
            Self::Native(e) => e.skip(n),
        }
    }
    pub fn render(&mut self, out: &mut [f32], flex: Option<&Flex>, rate: f32) {
        match self {
            Self::Ordinary(e) => e.render(out, flex, rate),
            Self::Native(e) => e.render(out),
        }
    }
}

mod native {
    use super::{Ahdsr, Phase};
    const BASE: f32 = 0.075;
    const START: f32 = 1.075;

    #[derive(Clone, Copy, Debug)]
    struct Source {
        phase: Phase,
        counts: [u32; 4],
        coefficients: [f32; 3],
        attack_start: f32,
        attack_positive: bool,
        sustain: f32,
        ahd: bool,
        left: u32,
        state: f32,
        mul: f32,
        offset: f32,
        coefficient: f32,
    }

    impl Source {
        fn new(p: &Ahdsr, rate: f32) -> Self {
            let rate = rate * (1. / 32.);
            let counts = [p.attack, p.hold, p.decay, p.release].map(|t| (t * rate) as u32);
            // Native physical setter rounds the base/ratio to f32 before pow.
            let base =
                (((1. - p.curve.abs()) as f64 * 500_000f64.ln()) - 20_000f64.ln()).exp() as f32;
            let positive = p.curve > 0.;
            let start = if positive { 1. + base } else { base };
            let ratio = if positive {
                base / start
            } else {
                (base + 1.) / base
            };
            let pow = |ratio: f64, n| {
                if n == 0 {
                    1.
                } else {
                    ratio.powf(1. / n as f64) as f32
                }
            };
            let coefficients = [
                pow(ratio as f64, counts[0]),
                (43f64 / 3.).powf(-1. / counts[2].max(1) as f64) as f32,
                (43f64 / 3.).powf(-1. / counts[3].max(1) as f64) as f32,
            ];
            let mut source = Self {
                phase: Phase::Attack,
                counts,
                coefficients,
                attack_start: start,
                attack_positive: positive,
                sustain: (p.sustain + BASE) - BASE,
                ahd: p.ahd_only,
                left: 0,
                state: BASE,
                mul: 0.,
                offset: 0.,
                coefficient: 1.,
            };
            source.enter(Phase::Attack);
            source
        }

        fn value(&self) -> f32 {
            (self.state - BASE) * self.mul + self.offset
        }

        fn enter(&mut self, mut phase: Phase) {
            // Zero-duration stages have no published point. Sustain/Done are
            // indefinite; every other stage is bounded by its prepared count.
            loop {
                let (count, next) = match phase {
                    Phase::Attack => (self.counts[0], Phase::Hold),
                    Phase::Hold => (self.counts[1], Phase::Decay),
                    Phase::Decay => (
                        self.counts[2],
                        if self.ahd {
                            Phase::Done
                        } else {
                            Phase::Sustain
                        },
                    ),
                    Phase::Release => (self.counts[3], Phase::Done),
                    _ => break,
                };
                if count != 0 {
                    break;
                }
                phase = next;
            }
            let old = self.value();
            self.phase = phase;
            (
                self.state,
                self.mul,
                self.offset,
                self.coefficient,
                self.left,
            ) = match phase {
                Phase::Attack => (
                    self.attack_start,
                    if self.attack_positive { -1. } else { 1. },
                    if self.attack_positive {
                        self.attack_start - BASE
                    } else {
                        BASE - self.attack_start
                    },
                    self.coefficients[0],
                    self.counts[0],
                ),
                Phase::Hold => (START, 1., 0., 1., self.counts[1]),
                Phase::Decay => (
                    START,
                    if self.ahd { 1. } else { 1. - self.sustain },
                    if self.ahd { 0. } else { self.sustain },
                    self.coefficients[1],
                    self.counts[2],
                ),
                Phase::Sustain => (self.sustain + BASE, 1., 0., 1., 0),
                Phase::Release => (START, old, 0., self.coefficients[2], self.counts[3]),
                _ => (BASE, 0., 0., 1., 0),
            };
        }

        fn release(&mut self) {
            if !self.ahd && !matches!(self.phase, Phase::Release | Phase::Done) {
                self.enter(Phase::Release);
            }
        }

        fn point(&mut self) -> f32 {
            let value = self.value();
            self.state *= self.coefficient;
            if self.left != 0 {
                self.left -= 1;
                if self.left == 0 {
                    self.enter(match self.phase {
                        Phase::Attack => Phase::Hold,
                        Phase::Hold => Phase::Decay,
                        Phase::Decay => {
                            if self.ahd {
                                Phase::Done
                            } else {
                                Phase::Sustain
                            }
                        }
                        Phase::Release => Phase::Done,
                        _ => self.phase,
                    });
                }
            }
            value
        }
    }

    #[derive(Clone, Copy, Debug)]
    pub(crate) struct Native {
        source: Source,
        previous: f32,
        current: f32,
        level: f32,
        offset: u8,
        primed: bool,
    }

    impl Native {
        pub fn new(p: &Ahdsr, rate: f32) -> Self {
            Self {
                source: Source::new(p, rate),
                previous: 0.,
                current: 0.,
                level: 0.,
                offset: 0,
                primed: false,
            }
        }
        pub fn release(&mut self) {
            self.source.release();
        }
        pub fn done(&self) -> bool {
            self.source.phase == Phase::Done
                && self.primed
                && self.previous == 0.
                && self.current == 0.
        }
        pub fn phase(&self) -> Phase {
            self.source.phase
        }
        pub fn level(&self) -> f32 {
            self.level
        }
        fn frame(&mut self) -> f32 {
            if self.offset == 0 {
                self.previous = self.current;
                // Volume destination clamps only the published point, never the
                // source recurrence or the arbitrary-stage release baseline.
                self.current = self.source.point().max(0.);
                if !self.primed {
                    self.previous = self.current;
                    self.primed = true;
                }
            }
            self.level =
                self.previous + (self.current - self.previous) * (self.offset as f32 * (1. / 32.));
            self.offset = (self.offset + 1) & 31;
            self.level
        }
        pub fn skip(&mut self, n: usize) {
            for _ in 0..n {
                self.frame();
            }
        }
        pub fn render(&mut self, out: &mut [f32]) {
            for x in out {
                *x = self.frame();
            }
        }
    }
}
mod lfo_volume {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct Target {
        value: f32,
        initialized: bool,
        alpha: Option<f32>,
    }

    impl Target {
        /// Prepare the native positive-lag coefficient outside the control-point loop.
        /// Negative signed lag selects a different native ramp and is not admitted.
        pub fn new(lag_ms: i16, rate: f32) -> Option<Self> {
            if lag_ms < 0 {
                return None;
            }
            let rate = if rate.is_finite() && rate >= 10. {
                rate
            } else {
                44_100.
            };
            let alpha = (lag_ms != 0).then(|| {
                let points = f64::from(lag_ms) * f64::from(rate) * 0.00003125;
                (1. - (-(2. * std::f64::consts::LN_10) / points).exp()) as f32
            });
            Some(Self {
                value: 0.,
                initialized: false,
                alpha,
            })
        }

        /// Evaluate once per native control point. `negative` is the proved target
        /// flag0x02, not the separately serialized inversion/shaper metadata.
        /// No upper/lower clamp here: the final combined volume buffer is clamped
        /// nonnegative by the native audio consumer after all targets have run.
        pub fn point(&mut self, source: f32, input: f32, intensity: f32, negative: bool) -> f32 {
            let source = if negative { -source } else { source };
            let source = (source + 1.) * 0.5;
            match self.alpha {
                Some(alpha) if self.initialized => {
                    let delta = source - self.value;
                    if delta.abs() >= 1e-5 {
                        self.value += delta * alpha;
                    }
                }
                _ => self.value = source,
            }
            self.initialized = true;
            input * (1. - (1. - self.value) * intensity)
        }
    }
}
mod lfo {
    use super::{FIXED_ONE, MAX_STEP, PitchLfo, VolumeLfo};
    /// One note's native source phases and retained pitch/volume interpolators.
    /// The native source clock pauses when bypassed; its audio interpolation
    /// offset advances separately, including short event fragments.
    #[derive(Clone, Copy, Debug, Default)]
    pub(crate) struct Clock {
        phase: [f64; 16],
        previous: f32,
        current: f32,
        offset: u8,
        initialized: bool,
        fades: [Fade; 16],
        fade_started: u16,
        volume: [Option<super::lfo_volume::Target>; 16],
        volume_previous: f32,
        volume_current: f32,
        volume_initialized: bool,
    }

    #[derive(Clone, Copy, Debug, Default)]
    struct Fade {
        remaining: u32,
        value: f32,
        factor: f32,
    }

    impl Fade {
        fn new(ms: f32, rate: f32) -> Self {
            // Native time getter returns milliseconds; DSP runs at rate / 32.
            let remaining = (ms * (rate / 32.) * 0.001) as u32;
            let factor = if remaining == 0 {
                1.
            } else {
                (1. + 1. / f64::from(0.3f32)).powf(1. / f64::from(remaining)) as f32
            };
            Self {
                remaining,
                value: 0.3,
                factor,
            }
        }

        fn next(&mut self) -> f32 {
            if self.remaining == 0 {
                return 1.;
            }
            let gain = self.value - 0.3;
            // v71/v72 set the native legacy switch false: its ceiling is 1,
            // unlike v73's optional .3..1.3 mode. Only N points are scaled.
            self.value = (self.value * self.factor).clamp(0., 1.);
            self.remaining -= 1;
            gain
        }
    }

    impl Clock {
        pub fn skip_bypassed(&mut self, n: usize) {
            self.offset = ((usize::from(self.offset) + n) & 31) as u8;
        }

        /// Relative source positions for one ordinary sampler block. Previewing a
        /// copied clock during planning gives the same reach as rendering it.
        pub fn positions(
            &mut self,
            lfos: &[PitchLfo],
            volume_lfos: &[VolumeLfo],
            rate: f32,
            tempo: f32,
            step: f64,
            out: &mut [u64],
            mut volume_out: Option<&mut [f32]>,
        ) -> (u64, u64) {
            let n = out.len();
            if n == 0 {
                return (0, 0);
            }
            let active = lfos.iter().any(|l| !l.bypassed);
            let volume_active = volume_lfos.iter().any(|l| !l.source.bypassed);
            // A coefficient is prepared once per note/target, outside the retained
            // control-point loop. Bypassing never discards this target state.
            for lfo in volume_lfos.iter().filter(|l| !l.source.bypassed) {
                self.volume[usize::from(lfo.source.slot)].get_or_insert_with(|| {
                    super::lfo_volume::Target::new(lfo.lag_ms, rate)
                        .expect("prepared nonnegative volume target lag")
                });
            }
            let mut position = 0u64;
            for (i, frame) in out.iter_mut().enumerate() {
                let pitch = if active {
                    // Voice planning fragments are not native control intervals.
                    // Sample the source at the retained note-clock boundary, even
                    // when a command or another loop voice split this render call.
                    if self.offset == 0 || !self.initialized {
                        let mut point = 0.;
                        for lfo in lfos.iter().filter(|l| !l.bypassed) {
                            let hz = f64::from(lfo.frequency(tempo));
                            let phase = (f64::from(lfo.start_phase)
                                + self.phase[lfo.slot as usize]
                                + i as f64 * hz / f64::from(rate))
                            .rem_euclid(1.);
                            let mut gain = 12. * lfo.depth * lfo.sine / lfo.sine.abs().max(1.);
                            if lfo.fade_ms > 0. {
                                let slot = usize::from(lfo.slot);
                                let bit = 1 << slot;
                                if self.fade_started & bit == 0 {
                                    self.fades[slot] = Fade::new(lfo.fade_ms, rate);
                                    self.fade_started |= bit;
                                }
                                gain *= self.fades[slot].next();
                            }
                            // Multi negates sine; ordinary sine is not admitted.
                            point -= (phase * std::f64::consts::TAU).sin() as f32 * gain;
                        }
                        self.previous = self.current;
                        self.current = point;
                        if !self.initialized {
                            self.previous = point;
                            self.initialized = true;
                        }
                    }
                    self.previous + (self.current - self.previous) * (f32::from(self.offset) / 32.)
                } else {
                    0.
                };
                let volume = if volume_active {
                    if self.offset == 0 || !self.volume_initialized {
                        let mut point = 1.;
                        for lfo in volume_lfos.iter().filter(|l| !l.source.bypassed) {
                            let source = &lfo.source;
                            let slot = usize::from(source.slot);
                            let phase = (f64::from(source.start_phase)
                                + self.phase[slot]
                                + i as f64 * f64::from(source.frequency(tempo)) / f64::from(rate))
                            .rem_euclid(1.);
                            // The admitted volume source has no fade. Its bipolar
                            // signal enters the native target before unipolar range
                            // conversion and target lag; no intermediate clamp.
                            let signal = -(phase * std::f64::consts::TAU).sin() as f32
                                * source.sine
                                / source.sine.abs().max(1.);
                            let target =
                                self.volume[slot].as_mut().expect("prepared volume target");
                            point = target.point(signal, point, lfo.intensity, lfo.negative);
                        }
                        self.volume_previous = self.volume_current;
                        self.volume_current = point.max(0.);
                        if !self.volume_initialized {
                            self.volume_previous = self.volume_current;
                            self.volume_initialized = true;
                        }
                    }
                    self.volume_previous
                        + (self.volume_current - self.volume_previous)
                            * (f32::from(self.offset) / 32.)
                } else {
                    1.
                };
                if let Some(output) = volume_out.as_deref_mut() {
                    output[i] = volume;
                }
                *frame = position;
                let ratio = if active {
                    2f64.powf(f64::from(pitch) / 12.)
                } else {
                    1.
                };
                position += ((step * ratio).min(MAX_STEP) * FIXED_ONE) as u64;
                self.offset = (self.offset + 1) & 31;
            }
            for lfo in lfos.iter().filter(|l| !l.bypassed) {
                let phase = &mut self.phase[lfo.slot as usize];
                *phase = (*phase + f64::from(lfo.frequency(tempo)) * n as f64 / f64::from(rate))
                    .rem_euclid(1.);
            }
            for lfo in volume_lfos.iter().filter(|l| !l.source.bypassed) {
                let source = &lfo.source;
                // A single native source can feed pitch and volume. Neither the
                // second target nor a copied planning clock advances it twice.
                if !lfos.iter().any(|l| l.slot == source.slot && !l.bypassed) {
                    let phase = &mut self.phase[usize::from(source.slot)];
                    *phase = (*phase
                        + f64::from(source.frequency(tempo)) * n as f64 / f64::from(rate))
                    .rem_euclid(1.);
                }
            }
            (position, out.last().copied().unwrap_or(0))
        }
    }
}

pub struct EnvelopeOracle {
    amp: Amplitude,
    flex: Option<Envelope>,
}
impl EnvelopeOracle {
    pub fn new(params: &Ahdsr, native: bool, flex: bool, rate: f32) -> Self {
        Self {
            amp: Amplitude::new(params, rate, native),
            flex: flex.then(Envelope::flex),
        }
    }
    pub fn render(
        &mut self,
        amp: &mut [f32],
        out: Option<&mut [f32]>,
        flex: Option<&Flex>,
        rate: f32,
    ) {
        self.amp.render(amp, None, rate);
        if let (Some(state), Some(out)) = (&mut self.flex, out) {
            state.render(out, flex, rate);
        }
    }
    pub fn skip(&mut self, n: usize, flex: Option<&Flex>, rate: f32) {
        self.amp.skip(n, None, rate);
        if let Some(state) = &mut self.flex {
            state.skip(n, flex, rate);
        }
    }
    pub fn release(&mut self, flex: Option<&Flex>) {
        self.amp.release(None);
        if let Some(state) = &mut self.flex {
            state.release(flex);
        }
    }
    pub fn level(&self) -> f32 {
        self.amp.level() * self.flex.as_ref().map_or(1., Envelope::level)
    }
    pub fn phase(&self) -> Phase {
        self.amp.phase()
    }
    pub fn done(&self) -> bool {
        self.amp.done() || self.flex.as_ref().is_some_and(Envelope::done)
    }
}
#[derive(Clone, Copy, Default)]
pub struct LfoOracle(lfo::Clock);
impl LfoOracle {
    pub fn positions(
        &mut self,
        pitch: &[PitchLfo],
        volume: &[VolumeLfo],
        rate: f32,
        tempo: f32,
        step: f64,
        positions: &mut [u64],
        out: Option<&mut [f32]>,
    ) -> (u64, u64) {
        self.0
            .positions(pitch, volume, rate, tempo, step, positions, out)
    }
}
fn shape(m: &Mod, x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    let Some(curve) = &m.curve else {
        return x;
    };
    let p = x * 127.0;
    let i = (p as usize).min(126);
    curve[i] + (curve[i + 1] - curve[i]) * (p - i as f32)
}
fn read(input: &Inputs<'_>, source: Source, target: Target) -> f32 {
    if source == Source::Bend && target == Target::Pitch {
        if let Some(bend) = input.bend_pitch {
            return (bend + 1.) * 0.5;
        }
    }
    match source {
        Source::Velocity => f32::from(input.velocity) / 127.0,
        Source::Key => f32::from(input.note) / 127.0,
        Source::Constant => 1.0,
        Source::Cc(cc) => {
            f32::from(if cc == 74 {
                input.cc74.unwrap_or(input.cc[74])
            } else {
                input.cc[cc as usize]
            }) / 127.0
        }
        Source::Bend => (input.bend + 1.0) * 0.5,
        Source::Pressure => f32::from(input.pressure) / 127.0,
        Source::Counter => input.counter,
    }
}
/// Move a lagged `value` toward `x` over `frames`. Settled (as a held
/// controller soon is) it costs no exp: within 1e-6 it lands on `x`, which
/// steps smaller than half a float's spacing would never reach.
fn approach(value: &mut f32, x: f32, lag: f32, frames: usize, rate: f32) {
    if (x - *value).abs() <= 1e-6 {
        *value = x;
    } else {
        *value += (x - *value) * lag_factor(lag, frames, rate);
    }
}

/// One-pole smoothing step over `frames`: the share of the distance covered.
fn lag_factor(lag: f32, frames: usize, rate: f32) -> f32 {
    if lag <= 0.0 {
        1.0
    } else {
        1.0 - (-(frames as f32) / (lag * rate)).exp()
    }
}

pub struct ModOracle {
    mods: Box<[Mod]>,
    values: [f32; VOICE_MODS],
}
impl ModOracle {
    pub fn new(mods: &[Mod], input: &Inputs<'_>) -> Self {
        let mods: Box<[_]> = mods
            .iter()
            .filter(|m| matches!(m.route, Some((_, Target::Volume | Target::Pitch))))
            .cloned()
            .collect();
        let mut values = [0.; VOICE_MODS];
        for (value, m) in values.iter_mut().zip(&mods) {
            if let Some((source, target)) = m.route {
                *value = shape(m, read(input, source, target));
            }
        }
        Self { mods, values }
    }
    pub fn modulate(&mut self, input: &Inputs<'_>, frames: usize, rate: f32) -> (f32, f32, bool) {
        let (mut gain, mut semitones, mut settled) = (1., 0., true);
        for (value, m) in self.values.iter_mut().zip(&self.mods) {
            let Some((source, target)) = m.route else {
                continue;
            };
            if matches!(source, Source::Cc(_) | Source::Bend | Source::Pressure) {
                let x = shape(m, read(input, source, target));
                approach(value, x, m.lag, frames, rate);
                settled &= *value == x;
            }
            match target {
                Target::Volume => {
                    let v = if m.intensity < 0. {
                        1. - *value
                    } else {
                        *value
                    };
                    gain *= 1. - m.intensity.abs() * (1. - v);
                }
                Target::Pitch => {
                    let v = if source == Source::Bend {
                        *value * 2. - 1.
                    } else {
                        *value
                    };
                    semitones += 12. * m.intensity * v;
                }
                _ => {}
            }
        }
        (gain.max(0.), semitones, settled)
    }
}

pub struct PitchOracle(Box<[Envelope]>);
impl PitchOracle {
    pub fn new(params: &[crate::v1_voice_controls::PitchEnvelope], rate: f32) -> Self {
        Self(params.iter().map(|p| Envelope::new(&p.env, rate)).collect())
    }
    pub fn release(&mut self) {
        for e in &mut self.0 {
            e.release(None);
        }
    }
    pub fn pitch(
        &mut self,
        params: &[crate::v1_voice_controls::PitchEnvelope],
        n: usize,
        rate: f32,
    ) -> f32 {
        let mut semitones = 0.;
        for (p, state) in params.iter().zip(&mut self.0) {
            state.skip(n, None, rate);
            if !p.bypass {
                let value: f32 = p
                    .targets
                    .iter()
                    .map(|(_, sign, m)| 12. * sign * m.intensity * shape(m, state.level()))
                    .sum();
                semitones += value;
            }
        }
        semitones
    }
}

pub fn scale_envelope(env: &mut Ahdsr, mods: &[Mod], input: &Inputs<'_>) {
    for m in mods
        .iter()
        .filter(|m| matches!(m.route, Some((_, Target::Attack | Target::Release))))
    {
        let Some((source, target)) = m.route else {
            continue;
        };
        let v = shape(m, read(input, source, target));
        let v = if m.intensity < 0. { 1. - v } else { v };
        let factor = (1. - m.intensity.abs() * (1. - v)).max(0.);
        match target {
            Target::Attack => env.attack *= factor,
            _ => env.release *= factor,
        }
    }
}

// Literal params.rs Mod::start_value/follow for the filter consumer.
pub fn filter_start(m: &Mod, input: &Inputs<'_>) -> f32 {
    m.route.map_or(0.0, |(source, _)| {
        shape(m, read(input, source, Target::Module))
    })
}
pub fn filter_follow(m: &Mod, value: &mut f32, input: &Inputs<'_>, n: usize, rate: f32) {
    if let Some((source, _)) = m
        .route
        .filter(|(s, _)| matches!(s, Source::Cc(_) | Source::Bend | Source::Pressure))
    {
        approach(
            value,
            shape(m, read(input, source, Target::Module)),
            m.lag,
            n,
            rate,
        );
    }
}
