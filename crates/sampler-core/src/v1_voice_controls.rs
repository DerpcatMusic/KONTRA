//! Controls copied from v1 0cb7a8a0 for W9's whole-voice adapter.
//! Prepared descriptors retain v1 admission; unsupported sources stay in v2.
use crate::Error;
use std::sync::Arc;

pub const MAX_BLOCK: usize = 128;
pub const VOICE_MODS: usize = 8;
pub const PITCH_ENVS: usize = 16;
const FIXED_ONE: f64 = (1u64 << 32) as f64;
const MAX_STEP: f64 = 32.;
const SILENT: f32 = 1e-4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ahdsr {
    pub attack: f32,
    pub curve: f32,
    pub hold: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub ahd_only: bool,
}
impl Ahdsr {
    pub const UNITY: Self = Self {
        attack: 0.,
        curve: 0.,
        hold: 0.,
        decay: 0.,
        sustain: 1.,
        release: f32::INFINITY,
        ahd_only: false,
    };
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Phase {
    Done = 0,
    Attack,
    Hold,
    Decay,
    Sustain,
    Release,
    Flex,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Flex {
    pub points: Box<[FlexPoint]>,
    pub sustain: usize,
}
impl Flex {
    /// Port from v1 0cb7a8a0:src/engine/bank.rs::Flex::from; admission stays with W9.
    pub fn from_kontakt(
        points: &[sampler_ir::kontakt::FlexPoint],
        sustain: u32,
    ) -> Result<Self, Error> {
        if points.is_empty()
            || points.len() > 32
            || sustain as usize >= points.len()
            || points.iter().any(|p| {
                !p.time_ms.is_finite()
                    || p.time_ms < 0.
                    || !(0. ..=1.).contains(&p.level)
                    || !(0. ..=1.).contains(&p.curve)
            })
        {
            return Err(Error::InvalidInput);
        }
        let mut from = 0.;
        let points = points
            .iter()
            .map(|p| {
                let bulge = 2. * p.curve - 1.;
                let point = FlexPoint {
                    seconds: p.time_ms / 1000.,
                    level: p.level,
                    curve: if p.level < from { -bulge } else { bulge },
                };
                from = p.level;
                point
            })
            .collect();
        Ok(Self {
            points,
            sustain: sustain as usize,
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlexPoint {
    pub seconds: f32,
    pub level: f32,
    pub curve: f32,
}
#[derive(Clone, Debug, PartialEq)]
pub struct PitchLfo {
    pub start_phase: f32,
    pub slot: u8,
    pub count: f32,
    pub note_value: f32,
    pub sine: f32,
    pub fade_ms: f32,
    pub depth: f32,
    pub targets: Vec<(u32, f32)>,
    pub bypassed: bool,
}
impl PitchLfo {
    pub(crate) fn frequency(&self, tempo: f32) -> f32 {
        let tempo = if tempo.is_finite() && tempo >= 0.1 {
            tempo
        } else {
            120.
        };
        (tempo / (60. * self.note_value * self.count)).clamp(0.01, 210.)
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct VolumeLfo {
    pub source: PitchLfo,
    pub target: u32,
    pub intensity: f32,
    pub negative: bool,
    pub lag_ms: i16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Velocity,
    Key,
    Constant,
    Cc(u8),
    Bend,
    Pressure,
    Counter,
}
impl Source {
    fn live(self) -> bool {
        matches!(self, Self::Cc(_) | Self::Bend | Self::Pressure)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Volume,
    Pitch,
    Start,
    Attack,
    Release,
    /// Addressed module destination; W9 retains its slot/knob routing.
    Module,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Mod {
    pub route: Option<(Source, Target)>,
    pub intensity: f32,
    pub lag: f32,
    pub curve: Option<Arc<[f32; 128]>>,
}
#[derive(Clone, Copy)]
pub struct Inputs<'a> {
    pub cc: &'a [u8; 128],
    pub cc74: Option<u8>,
    pub bend: f32,
    pub pressure: u8,
    pub note: u8,
    pub velocity: u8,
    pub counter: f32,
    /// Invalidated by controller, MPE or prepared-parameter changes.
    pub stamp: u32,
    /// Member bend for pitch when the MPE master is added by W9.
    pub bend_pitch: Option<f32>,
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
pub struct Envelope {
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
pub enum Shape {
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
enum Amplitude {
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
    fn control_point(&self) -> f32 {
        match self {
            Self::Ordinary(e) => e.level(),
            Self::Native(e) => e.control_point(),
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
        pub(super) fn control_point(&self) -> f32 {
            self.current
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
    pub(super) struct Clock {
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
        pub(super) fn prepared(pitch: &[PitchLfo], volume: &[VolumeLfo], rate: f32) -> Self {
            let mut clock = Self::default();
            for lfo in pitch {
                if lfo.fade_ms > 0. {
                    clock.fades[usize::from(lfo.slot)] = Fade::new(lfo.fade_ms, rate);
                    clock.fade_started |= 1 << lfo.slot;
                }
            }
            for lfo in volume {
                clock.volume[usize::from(lfo.source.slot)] = Some(
                    super::lfo_volume::Target::new(lfo.lag_ms, rate).expect("validated native lag"),
                );
            }
            clock
        }
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
impl Mod {
    /// Shaped value at note start; zero for unmodelled sources.
    pub fn start_value(&self, input: &Inputs) -> f32 {
        self.route
            .map_or(0.0, |(source, _)| self.shape(input.read(source)))
    }

    /// Pinned VoiceFilter::follow: advance this target lag once before hold/process.
    pub fn follow(&self, value: &mut f32, input: &Inputs, frames: usize, rate: f32) {
        if let Some((source, _)) = self.route.filter(|(s, _)| s.live()) {
            approach(
                value,
                self.shape(input.read(source)),
                self.lag,
                frames,
                rate,
            );
        }
    }

    /// Shaped source value; exact at MIDI steps, linear between them.
    pub fn shape(&self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        let Some(curve) = &self.curve else {
            return x;
        };
        let p = x * 127.0;
        let i = (p as usize).min(126);
        curve[i] + (curve[i + 1] - curve[i]) * (p - i as f32)
    }
}

impl Inputs<'_> {
    /// Negotiated MPE master pitch is added independently by the voice;
    /// other bend destinations still read the combined controller normally.
    fn read_mod(&self, source: Source, target: Target, bend_pitch: Option<f32>) -> f32 {
        if source == Source::Bend && target == Target::Pitch {
            if let Some(bend) = bend_pitch {
                return (bend + 1.) * 0.5;
            }
        }
        self.read(source)
    }

    /// Unshaped source value, 0..=1.
    pub fn read(&self, source: Source) -> f32 {
        match source {
            Source::Velocity => f32::from(self.velocity) / 127.0,
            Source::Key => f32::from(self.note) / 127.0,
            Source::Constant => 1.0,
            Source::Cc(cc) => {
                f32::from(if cc == 74 {
                    self.cc74.unwrap_or(self.cc[74])
                } else {
                    self.cc[cc as usize]
                }) / 127.0
            }
            Source::Bend => (self.bend + 1.0) * 0.5,
            Source::Pressure => f32::from(self.pressure) / 127.0,
            Source::Counter => self.counter,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModTable {
    mods: Box<[Mod]>,
    voiced: Box<[u16]>,
    starts: Box<[u16]>,
    times: Box<[u16]>,
}
impl ModTable {
    pub fn assignments(&self) -> &[Mod] {
        &self.mods
    }
    pub fn new(mods: Box<[Mod]>) -> Result<Self, Error> {
        if mods.len() > u16::MAX as usize || mods.iter().any(|m| !valid_mod(m)) {
            return Err(Error::InvalidInput);
        }
        let indices = |want: fn(Target) -> bool| {
            (0..mods.len())
                .filter(|&i| mods[i].route.is_some_and(|(_, t)| want(t)))
                .map(|i| i as u16)
                .collect::<Box<[_]>>()
        };
        let voiced = indices(|t| matches!(t, Target::Volume | Target::Pitch));
        // V1 truncated at eight; the adapter refuses so W9 keeps the full v2 model.
        if voiced.len() > VOICE_MODS {
            return Err(Error::InvalidInput);
        }
        let starts = indices(|t| t == Target::Start);
        let times = indices(|t| matches!(t, Target::Attack | Target::Release));
        Ok(Self {
            mods,
            voiced,
            starts,
            times,
        })
    }
    /// Scale the volume AHDSR's attack and release by their note-start
    /// modulation, with the volume law: `1 - |i|·(1 - v)`. Stored shapers
    /// (velocity 0 → 1, 127 → 0.59 on attack) read as time factors.
    pub(crate) fn scale_envelope(&self, env: &mut Ahdsr, input: &Inputs) {
        for m in self.times.iter().map(|&i| &self.mods[i as usize]) {
            let Some((source, target)) = m.route else {
                continue;
            };
            let v = m.shape(input.read(source));
            let v = if m.intensity < 0.0 { 1.0 - v } else { v };
            let factor = (1.0 - m.intensity.abs() * (1.0 - v)).max(0.0);
            match target {
                Target::Attack => env.attack *= factor,
                _ => env.release *= factor,
            }
        }
    }
    /// Initial per-voice values: every source at its current value, unlagged.
    pub(crate) fn start(&self, input: &Inputs, bend_pitch: Option<f32>) -> [f32; VOICE_MODS] {
        let mut values = [0.0; VOICE_MODS];
        for (value, &i) in values.iter_mut().zip(&self.voiced) {
            let m = &self.mods[i as usize];
            if let Some((source, target)) = m.route {
                *value = m.shape(input.read_mod(source, target, bend_pitch));
            }
        }
        values
    }

    /// Advance live sources over `frames` at `rate` and return the volume
    /// factor, the pitch offset in semitones, and whether every live value
    /// has reached its source: settled, the same inputs give the same
    /// result again whatever the frames.
    ///
    /// Volume: each assignment scales amplitude by `1 - |i|·(1 - v)` for
    /// shaped value `v` (inverted, `1 - v`, when `i < 0`). Pitch: `12·i·v`
    /// semitones, with pitch bend mapped back to -1..=1. The flag is true
    /// when every live source has settled on its input: until the inputs
    /// or the table change, another call returns the same.
    pub(crate) fn modulate(
        &self,
        values: &mut [f32; VOICE_MODS],
        input: &Inputs,
        frames: usize,
        rate: f32,
        bend_pitch: Option<f32>,
    ) -> (f32, f32, bool) {
        let (mut gain, mut semitones, mut settled) = (1.0, 0.0, true);
        for (value, &i) in values.iter_mut().zip(&self.voiced) {
            let m = &self.mods[i as usize];
            let Some((source, target)) = m.route else {
                continue;
            };
            if source.live() {
                let x = m.shape(input.read_mod(source, target, bend_pitch));
                approach(value, x, m.lag, frames, rate);
                settled &= *value == x;
            }
            match target {
                Target::Volume => {
                    let v = if m.intensity < 0.0 {
                        1.0 - *value
                    } else {
                        *value
                    };
                    gain *= 1.0 - m.intensity.abs() * (1.0 - v);
                }
                Target::Pitch => {
                    let v = if source == Source::Bend {
                        *value * 2.0 - 1.0
                    } else {
                        *value
                    };
                    semitones += 12.0 * m.intensity * v;
                }
                Target::Start | Target::Attack | Target::Release | Target::Module => {}
            }
        }
        (gain.max(0.0), semitones, settled)
    }

    /// Sample-start offset as a fraction of the zone's start-mod range.
    pub fn start_offset(&self, input: &Inputs) -> f32 {
        self.starts
            .iter()
            .map(|&i| &self.mods[i as usize])
            .filter_map(|m| Some(m.intensity.abs() * m.shape(input.read(m.route?.0))))
            .sum::<f32>()
            .min(1.0)
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

#[derive(Clone, Debug, PartialEq)]
pub struct PitchEnvelope {
    pub env: Ahdsr,
    pub bypass: bool,
    pub index: u8,
    pub targets: Box<[(u16, f32, Mod)]>,
}
impl PitchEnvelope {
    /// Existing voice control rate, with the same AHDSR as amplitude/filter modulation.
    fn pitch(&self, state: &mut Envelope, frames: usize, rate: f32) -> f32 {
        state.skip(frames, None, rate);
        if self.bypass {
            return 0.;
        }
        self.targets
            .iter()
            .map(|(_, sign, m)| 12. * sign * m.intensity * m.shape(state.level()))
            .sum()
    }
}

fn valid_mod(m: &Mod) -> bool {
    m.intensity.is_finite()
        && m.lag.is_finite()
        && m.lag >= 0.
        && !matches!(m.route, Some((Source::Cc(128..), _)))
        && m.curve
            .as_ref()
            .is_none_or(|c| c.iter().all(|x| x.is_finite()))
}
fn valid_ahdsr(p: &Ahdsr) -> bool {
    [p.attack, p.hold, p.decay]
        .iter()
        .all(|x| x.is_finite() && *x >= 0.)
        && p.release >= 0.
        && !p.release.is_nan()
        && (-1. ..=1.).contains(&p.curve)
        && (0. ..=1.).contains(&p.sustain)
}
fn valid_lfo(p: &PitchLfo) -> bool {
    p.slot < 16
        && (0. ..=1.).contains(&p.start_phase)
        && p.count.is_finite()
        && p.count >= 1.
        && p.note_value.is_finite()
        && p.note_value > 0.
        && p.sine.is_finite()
        && p.sine.abs() <= 1.
        && p.depth.is_finite()
        && p.fade_ms.is_finite()
        && (0. ..=5000.).contains(&p.fade_ms)
        && p.targets.iter().all(|(_, depth)| depth.is_finite())
}

fn same_lfo_clock(a: &PitchLfo, b: &PitchLfo) -> bool {
    a.start_phase == b.start_phase
        && a.slot == b.slot
        && a.count == b.count
        && a.note_value == b.note_value
        && a.sine == b.sine
        && a.fade_ms == b.fade_ms
        && a.bypassed == b.bypassed
}

/// Forward-decoded admitted descriptors; W9 keeps unsupported voices on v2.
#[derive(Clone, Debug, PartialEq)]
pub struct ControlDescription {
    pub amplitude: Ahdsr,
    pub native_amplitude: bool,
    pub flex: Option<Flex>,
    pub pitch_envelopes: Box<[PitchEnvelope]>,
    pub pitch_lfos: Box<[PitchLfo]>,
    pub volume_lfos: Box<[VolumeLfo]>,
    pub mods: ModTable,
}
impl Default for ControlDescription {
    fn default() -> Self {
        Self {
            amplitude: Ahdsr::UNITY,
            native_amplitude: false,
            flex: None,
            pitch_envelopes: Box::new([]),
            pitch_lfos: Box::new([]),
            volume_lfos: Box::new([]),
            mods: ModTable::default(),
        }
    }
}

/// Immutable plan; coefficients are prepared before the audio callback.
#[derive(Clone)]
pub struct ControlPlan {
    description: ControlDescription,
    rate: f32,
    amplitude: Amplitude,
    pitch: [Envelope; PITCH_ENVS],
    lfo: lfo::Clock,
}
impl ControlPlan {
    pub fn prepare(description: ControlDescription, rate: f32) -> Result<Self, Error> {
        let d = &description;
        if !rate.is_finite()
            || rate < 10.
            || !valid_ahdsr(&d.amplitude)
            || d.pitch_envelopes.len() > PITCH_ENVS
            || d.pitch_envelopes.iter().any(|p| {
                !valid_ahdsr(&p.env)
                    || p.targets
                        .iter()
                        .any(|(_, sign, m)| !sign.is_finite() || !valid_mod(m))
            })
            || d.flex.as_ref().is_some_and(|f| {
                f.points.is_empty()
                    || f.points.len() > 255
                    || f.sustain >= f.points.len()
                    || f.points.iter().any(|p| {
                        !p.seconds.is_finite()
                            || p.seconds < 0.
                            || !p.level.is_finite()
                            || !p.curve.is_finite()
                    })
            })
            || d.pitch_lfos.iter().any(|p| !valid_lfo(p))
            || d.volume_lfos.iter().any(|p| {
                !valid_lfo(&p.source)
                    || p.source.fade_ms != 0.
                    || p.lag_ms < 0
                    || !p.intensity.is_finite()
                    || p.intensity < 0.
            })
        {
            return Err(Error::InvalidInput);
        }
        for (i, p) in d.pitch_envelopes.iter().enumerate() {
            if d.pitch_envelopes[..i].iter().any(|q| q.index == p.index) {
                return Err(Error::InvalidInput);
            }
        }
        // One physical source per slot. Shared pitch/volume sources must agree.
        for (i, a) in d.pitch_lfos.iter().enumerate() {
            if d.pitch_lfos[..i].iter().any(|b| b.slot == a.slot) {
                return Err(Error::InvalidInput);
            }
        }
        for (i, a) in d.volume_lfos.iter().enumerate() {
            if d.volume_lfos[..i]
                .iter()
                .any(|b| b.source.slot == a.source.slot)
                || d.pitch_lfos
                    .iter()
                    .any(|b| b.slot == a.source.slot && !same_lfo_clock(b, &a.source))
            {
                return Err(Error::InvalidInput);
            }
        }
        let amplitude = Amplitude::new(&d.amplitude, rate, d.native_amplitude);
        let mut pitch = [Envelope::new(&Ahdsr::UNITY, rate); PITCH_ENVS];
        for (target, p) in pitch.iter_mut().zip(&d.pitch_envelopes) {
            *target = Envelope::new(&p.env, rate);
        }
        let lfo = lfo::Clock::prepared(&d.pitch_lfos, &d.volume_lfos, rate);
        Ok(Self {
            description,
            rate,
            amplitude,
            pitch,
            lfo,
        })
    }
    pub fn description(&self) -> &ControlDescription {
        &self.description
    }
    pub fn rate(&self) -> f32 {
        self.rate
    }
}

/// Fixed-size per-voice storage; resetting/releasing/rendering never allocates.
#[derive(Clone)]
pub struct ControlState {
    amplitude: Amplitude,
    flex: Option<Envelope>,
    pitch: [Envelope; PITCH_ENVS],
    lfo: lfo::Clock,
    mods: [f32; VOICE_MODS],
    modulated: (f32, f32),
    settled: Option<u32>,
    rate: f32,
    start_frame: u64,
    released: bool,
}
impl ControlState {
    pub fn new(plan: &ControlPlan) -> Self {
        Self {
            amplitude: plan.amplitude,
            flex: plan.description.flex.as_ref().map(|_| Envelope::flex()),
            pitch: plan.pitch,
            lfo: plan.lfo,
            mods: [0.; VOICE_MODS],
            modulated: (1., 0.),
            settled: None,
            rate: plan.rate,
            start_frame: 0,
            released: false,
        }
    }
    pub fn reset(
        &mut self,
        plan: &ControlPlan,
        input: &Inputs<'_>,
        start_frame: u64,
        rate: f32,
    ) -> Result<(), Error> {
        if rate != plan.rate {
            return Err(Error::InvalidInput);
        }
        *self = Self::new(plan);
        self.start_frame = start_frame;
        if !plan.description.mods.times.is_empty() {
            let mut params = plan.description.amplitude;
            plan.description.mods.scale_envelope(&mut params, input);
            self.amplitude = Amplitude::new(&params, rate, plan.description.native_amplitude);
        }
        self.mods = plan.description.mods.start(input, input.bend_pitch);
        Ok(())
    }
    pub fn start_frame(&self) -> u64 {
        self.start_frame
    }
    pub fn release(&mut self, plan: &ControlPlan) {
        if self.released {
            return;
        }
        self.released = true;
        self.amplitude.release(None);
        for env in &mut self.pitch[..plan.description.pitch_envelopes.len()] {
            env.release(None);
        }
        if let Some(env) = &mut self.flex {
            env.release(plan.description.flex.as_ref());
        }
    }
    /// V1 Voice::plan's input-stamp cache, followed by independent pitch envelopes.
    pub fn plan_controls(
        &mut self,
        plan: &ControlPlan,
        input: &Inputs<'_>,
        frames: usize,
    ) -> (f32, f32, bool) {
        if self.settled != Some(input.stamp) {
            let (gain, pitch, settled) = plan.description.mods.modulate(
                &mut self.mods,
                input,
                frames,
                self.rate,
                input.bend_pitch,
            );
            self.modulated = (gain, pitch);
            self.settled = settled.then_some(input.stamp);
        }
        let (gain, mut semitones) = self.modulated;
        for (params, state) in plan.description.pitch_envelopes.iter().zip(&mut self.pitch) {
            semitones += params.pitch(state, frames, self.rate);
        }
        (gain, semitones, self.settled == Some(input.stamp))
    }
    /// Read the already advanced voiced target by its original assignment index.
    pub fn modulation_value(&self, plan: &ControlPlan, index: u16) -> Option<f32> {
        plan.description
            .mods
            .voiced
            .iter()
            .position(|&i| i == index)
            .map(|i| self.mods[i])
    }
    /// Pitch consumer endpoint after plan_controls; reading never advances it.
    pub fn pitch_envelope_level(&self, plan: &ControlPlan, index: u8) -> Option<f32> {
        plan.description
            .pitch_envelopes
            .iter()
            .position(|p| p.index == index)
            .map(|i| self.pitch[i].level())
    }
    pub fn amplitude_level(&self) -> f32 {
        self.amplitude.level()
    }
    /// Last published native point, before audio interpolation; read-only.
    pub fn amplitude_control_point(&self) -> f32 {
        self.amplitude.control_point()
    }
    pub fn shape(&self, frames: usize) -> Option<Shape> {
        let flex = match &self.flex {
            Some(e) => e.shape(frames),
            None => Some(Shape::Flat(1.)),
        };
        self.amplitude
            .shape(frames)
            .zip(flex)
            .and_then(|(a, b)| a.times(b))
    }
    pub fn done(&self) -> bool {
        self.amplitude.done() || self.flex.as_ref().is_some_and(Envelope::done)
    }
    pub fn phase(&self) -> Phase {
        self.amplitude.phase()
    }
    pub fn level(&self) -> f32 {
        self.amplitude.level() * self.flex.as_ref().map_or(1., Envelope::level)
    }
    /// W9 supplies the final step after its cached pitch ratio and loop tuning.
    pub fn preview_pitch(
        &self,
        plan: &ControlPlan,
        tempo: f32,
        step: f64,
        positions: &mut [u64],
    ) -> Result<(u64, u64), Error> {
        if positions.len() > MAX_BLOCK || !step.is_finite() || step <= 0. {
            return Err(Error::InvalidInput);
        }
        let mut clock = self.lfo;
        Ok(clock.positions(
            &plan.description.pitch_lfos,
            &[],
            self.rate,
            tempo,
            step,
            positions,
            None,
        ))
    }
    /// Buffers remain separate to retain env → flex → fade → LFO-volume order.
    pub fn render(
        &mut self,
        plan: &ControlPlan,
        tempo: f32,
        step: f64,
        amplitude: &mut [f32],
        mut flex: Option<&mut [f32]>,
        volume: Option<&mut [f32]>,
        positions: &mut [u64],
    ) -> Result<(u64, u64), Error> {
        let n = amplitude.len();
        if n > MAX_BLOCK
            || positions.len() != n
            || !step.is_finite()
            || step <= 0.
            || flex.as_ref().is_some_and(|f| f.len() != n)
            || volume.as_ref().is_some_and(|v| v.len() != n)
            || (self.flex.is_some() && flex.is_none())
            || (!plan.description.volume_lfos.is_empty() && volume.is_none())
        {
            return Err(Error::InvalidInput);
        }
        self.amplitude.render(amplitude, None, self.rate);
        if let Some(out) = flex.as_deref_mut() {
            match &mut self.flex {
                Some(e) => e.render(out, plan.description.flex.as_ref(), self.rate),
                None => out.fill(1.),
            }
        }
        Ok(self.lfo.positions(
            &plan.description.pitch_lfos,
            &plan.description.volume_lfos,
            self.rate,
            tempo,
            step,
            positions,
            volume,
        ))
    }
    /// Call after plan_controls for muted/laned voices, not as a second render.
    pub fn skip(
        &mut self,
        plan: &ControlPlan,
        tempo: f32,
        step: f64,
        frames: usize,
    ) -> Result<(u64, u64), Error> {
        if frames > MAX_BLOCK || !step.is_finite() || step <= 0. {
            return Err(Error::InvalidInput);
        }
        self.amplitude.skip(frames, None, self.rate);
        if let Some(e) = &mut self.flex {
            e.skip(frames, plan.description.flex.as_ref(), self.rate);
        }
        let mut positions = [0; MAX_BLOCK];
        Ok(self.lfo.positions(
            &plan.description.pitch_lfos,
            &plan.description.volume_lfos,
            self.rate,
            tempo,
            step,
            &mut positions[..frames],
            None,
        ))
    }
}
