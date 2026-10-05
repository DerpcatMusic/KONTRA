//! Envelope parameters, state, arithmetic, and voice amplitude/fade consumers.

/// Envelope level treated as silence (−80 dB); decays below it end the voice.
const SILENT: f32 = 1e-4;

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
    /// Explicit KSP AHD Only: one-shot attack/hold/decay, independent of key release.
    pub ahd_only: bool,
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
    pub(super) fn finite(&self) -> bool {
        [self.attack, self.curve, self.hold, self.decay, self.sustain, self.release]
            .into_iter().all(f32::is_finite)
    }

    pub(super) fn prepared_valid(&self) -> bool {
        // UNITY deliberately holds until another voice owner supplies the end.
        self.finite() || *self == Self::UNITY
    }

    /// The attack, decay and release as a voice renders them, each sampled
    /// at `points + 1` even steps over its own time: the engine's envelope
    /// run at a rate that fits the stage into `points` frames. The release
    /// falls from the sustain level. A stage of no time is one point.
    pub fn trace(&self, points: usize) -> [Vec<f32>; 3] {
        let n = points.max(1);
        let sustain = if self.ahd_only {
            0.
        } else {
            self.sustain.clamp(0.0, 1.0)
        };
        let base = Ahdsr {
            attack: 0.0,
            curve: 0.0,
            hold: 0.0,
            decay: 0.0,
            sustain,
            release: f32::INFINITY,
            ahd_only: false,
        };
        let timed = |seconds: f32| seconds > 0.0 && seconds.is_finite();
        let mut out = [vec![1.0], vec![sustain], vec![0.0]];
        if timed(self.attack) {
            let env = Ahdsr {
                attack: self.attack,
                curve: self.curve,
                sustain: 1.0,
                ..base
            };
            let mut e = Envelope::new(&env, n as f32 / self.attack);
            out[0] = vec![0.0; n + 1];
            e.render(&mut out[0][1..], None, 1.0);
        }
        if timed(self.decay) {
            // No attack: the first frame is at full level, the decay follows.
            let mut e = Envelope::new(
                &Ahdsr {
                    decay: self.decay,
                    ..base
                },
                n as f32 / self.decay,
            );
            out[1] = vec![0.0; n + 1];
            e.render(&mut out[1], None, 1.0);
        }
        if !self.ahd_only && timed(self.release) {
            // Full level, then the sustain level (no decay time), then let go.
            let mut e = Envelope::new(
                &Ahdsr {
                    release: self.release,
                    ..base
                },
                n as f32 / self.release,
            );
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
        ahd_only: false,
    };
}

/// Breakpoint (flex) envelope: glides from silence through `points` and
/// holds at `points[sustain]` while the key is down.
#[derive(Clone, Debug, PartialEq)]
pub struct Flex {
    pub points: Box<[FlexPoint]>,
    pub sustain: usize,
    /// Saved, admitted Oneshot with Loop disabled; ignores key release.
    pub one_shot: bool,
    /// Admitted wider saved loop; return enters start + 1 from the current level.
    pub loop_start: Option<usize>,
}

impl Flex {
    pub(super) fn finite(&self) -> bool {
        self.points.iter().all(|p| [p.seconds, p.level, p.curve].into_iter().all(f32::is_finite))
    }
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
    Enter(usize),
    /// Gliding toward this flex point.
    Point(usize),
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
    /// Sustain/release floor; ordinary amplitude holds every positive level
    /// and captures a relative endpoint on its first release.
    floor: f32,
    ahd_only: bool,
    /// The attack's or decay's [`edge`], NaN until a skip works it out.
    edge: f32,
    /// Amplitude/Flex retain their eight-frame arithmetic group. Module and
    /// pitch consumers already own a scalar clock and keep that recurrence.
    clock: Option<AffineClock>,
}

#[derive(Clone, Copy, Debug)]
struct AffineClock {
    base: f32,
    offset: u8,
}

impl AffineClock {
    fn new(base: f32) -> Self {
        Self { base, offset: 0 }
    }
}

/// A block of envelope gain as a level times a per-frame decay (1 for a
/// held level): voices of one decay share the block's curve.
#[derive(Clone, Copy, Debug)]
pub(super) enum Shape {
    Flat(f32),
    /// `(per-frame factor, level)`, as the release renders it.
    Decay(f32, f32),
}

impl Shape {
    pub(super) fn times(self, other: Self) -> Option<Self> {
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
    // Keep the increment when a long, nearly linear segment's multiplier
    // rounds to one. Subtracting the rounded multiplier would freeze it.
    (mul, target * -(-k / frames).exp_m1())
}

/// Write `x = x * mul + add` per frame into `out`, from `x` = `level`;
/// returns the last value. Eight frames at a time from powers of the step,
/// so the frames don't wait on each other as a frame-by-frame loop does.
pub(super) fn affine(out: &mut [f32], level: f32, step: (f32, f32)) -> f32 {
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

/// The existing eight-frame powers, with the unfinished group's origin and
/// position retained by its envelope rather than restarted by a host fragment.
fn affine_until_clock(
    mut out: Option<&mut [f32]>,
    len: usize,
    level: f32,
    step: (f32, f32),
    stop: impl Fn(f32) -> bool,
    short: Short,
    clock: &mut AffineClock,
) -> (f32, Option<usize>) {
    let (pow, off) = powers(step);
    let (mut x, mut at) = (level, 0);
    while at < len {
        let offset = clock.offset as usize;
        let n = (8 - offset).min(len - at);
        if out.is_none() && offset == 0 && n == 8 && short.holds(clock.base) {
            x = pow[7] * clock.base + off[7];
            clock.base = x;
            at += 8;
            continue;
        }
        let chunk: [f32; 8] = std::array::from_fn(|k| pow[k] * clock.base + off[k]);
        let part = &chunk[offset..offset + n];
        let end = first(part, &stop);
        let used = end.map_or(n, |i| i + 1);
        if let Some(out) = out.as_deref_mut() {
            out[at..at + used].copy_from_slice(&part[..used]);
        }
        x = part[used - 1];
        clock.offset += used as u8;
        if clock.offset == 8 {
            *clock = AffineClock::new(x);
        }
        if let Some(end) = end {
            return (x, Some(at + end));
        }
        at += used;
    }
    (x, None)
}

/// [`affine_until`] of a release (`x = x * mul`, `mul` in `[0, 1]`, from
/// `x >= 0`) to its retained floor, unwritten: the same values, but as
/// the powers of `mul` only fall, so does each chunk's frames, and its last
/// frame alone says whether the chunk reaches silence. One multiply per
/// eight frames, as muted and laned voices only move on.
fn decay_until_silent(len: usize, level: f32, mul: f32, floor: f32) -> (f32, Option<usize>) {
    let mut p = 1f32;
    let pow: [f32; 8] = std::array::from_fn(|_| {
        p *= mul;
        p
    });
    let mut x = level;
    for chunk in 0..len / 8 {
        // `pow[k] * x + 0.0` in `affine_until`: the same value.
        let last = pow[7] * x;
        if last <= 0. || last < floor {
            let c: [f32; 8] = std::array::from_fn(|k| pow[k] * x);
            let k = c.iter().position(|&x| x <= 0. || x < floor).unwrap_or(7);
            return (c[k], Some(chunk * 8 + k));
        }
        x = last;
    }
    let (x, end) = affine_until(
        None,
        len % 8,
        x,
        (mul, 0.0),
        |x| x <= 0. || x < floor,
        Short::Unknown,
    );
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
    #[cfg(test)]
    pub(super) fn use_scalar_clock_for_test(&mut self) {
        self.clock = None;
    }

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
            floor: SILENT,
            ahd_only: p.ahd_only,
            edge: f32::NAN,
            clock: None,
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
            // A held Flex point can be audible after gain, or rise again in
            // its release segments. Its finite points own completion.
            floor: 0.,
            ahd_only: false,
            edge: f32::NAN,
            clock: Some(AffineClock::new(0.)),
        }
    }

    /// Enter the release: the flex segment after the sustain point, or the
    /// AHDSR release.
    pub fn release(&mut self, flex: Option<&Flex>) {
        if self.ahd_only && flex.is_none() || flex.is_some_and(|f| f.one_shot) {
            return;
        }
        let stage = match (self.stage, flex) {
            (Stage::Done, _) => Stage::Done,
            (_, Some(flex)) => Stage::Enter(flex.sustain.saturating_add(1)),
            _ => Stage::Release,
        };
        if stage != self.stage {
            self.reset_clock();
        }
        self.stage = stage;
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
    pub(super) fn shape(&self, n: usize) -> Option<Shape> {
        match self.stage {
            Stage::Sustain if self.level > self.floor => Some(Shape::Flat(self.level)),
            Stage::Hold if self.left as usize >= n => Some(Shape::Flat(self.level)),
            // Twice the threshold: rounding cannot end it early.
            // A normalized shared decay reconstructs each voice from a new
            // fragment weight. Retained amplitude clocks render their own
            // release; flat/source/filter sharing remains available.
            Stage::Release if self.clock.is_none() => {
                let end = self.level * self.release.powi(n as i32);
                (end > 0. && end >= 2.0 * self.floor)
                    .then_some(Shape::Decay(self.release, self.level))
            }
            _ => None,
        }
    }

    /// Advance over `frames` exactly as [`Envelope::render`] would, without
    /// computing each frame: held stages jump, glides step eight frames at
    /// a time.
    pub fn skip(&mut self, frames: usize, flex: Option<&Flex>, rate: f32) {
        let _ = self.run(None, frames, flex, rate);
    }

    /// Write one gain per frame; `flex` is the envelope's points, if it is one.
    pub fn render(&mut self, out: &mut [f32], flex: Option<&Flex>, rate: f32) {
        let len = out.len();
        let _ = self.run(Some(out), len, flex, rate);
    }

    pub(super) fn skip_terminal(
        &mut self,
        frames: usize,
        flex: Option<&Flex>,
        rate: f32,
    ) -> Option<usize> {
        self.run(None, frames, flex, rate)
    }

    pub(super) fn render_terminal(
        &mut self,
        out: &mut [f32],
        flex: Option<&Flex>,
        rate: f32,
    ) -> Option<usize> {
        let len = out.len();
        self.run(Some(out), len, flex, rate)
    }

    /// Pitch integrates every frame into source travel. Its recurrence must
    /// keep the same rounding across partitions: the amplitude fast path's
    /// eight-frame powers are only approximately equal to scalar steps.
    pub(crate) fn pitch_frame(&mut self, rate: f32) -> f32 {
        let mut level = [0.];
        self.render(&mut level, None, rate);
        level[0]
    }

    pub(super) fn skip_pitch(&mut self, frames: usize, rate: f32) {
        for _ in 0..frames {
            self.pitch_frame(rate);
        }
    }

    /// The current stage's [`edge`], worked out once.
    fn edge(&mut self, step: (f32, f32), rising: bool, stop: &impl Fn(f32) -> bool) -> f32 {
        if self.edge.is_nan() {
            self.edge = edge(step, rising, stop);
        }
        self.edge
    }

    fn reset_clock(&mut self) {
        if let Some(clock) = &mut self.clock {
            *clock = AffineClock::new(self.level);
        }
    }

    fn affine(
        &mut self,
        out: Option<&mut [f32]>,
        len: usize,
        step: (f32, f32),
        stop: impl Fn(f32) -> bool,
        short: Short,
    ) -> (f32, Option<usize>) {
        match &mut self.clock {
            Some(clock) => affine_until_clock(out, len, self.level, step, stop, short, clock),
            None => affine_until(out, len, self.level, step, stop, short),
        }
    }

    /// The stage machine over `len` frames, writing them to `out` if given.
    fn run(
        &mut self,
        mut out: Option<&mut [f32]>,
        len: usize,
        flex: Option<&Flex>,
        rate: f32,
    ) -> Option<usize> {
        let mut terminal = self.done().then_some(0);
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
                    let (level, peak) = self.affine(rest.as_deref_mut(), n, self.step, stop, short);
                    self.level = level;
                    match peak {
                        Some(peak) => {
                            if let Some(rest) = rest {
                                rest[peak] = 1.0;
                            }
                            self.level = 1.0;
                            self.stage = Stage::Hold;
                            self.reset_clock();
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
                        self.reset_clock();
                    }
                    m
                }
                Stage::Enter(i) => {
                    self.stage = match flex.and_then(|f| f.points.get(i)) {
                        Some(p) => {
                            let frames = (p.seconds * rate).round().max(1.0);
                            self.step = glide(self.level, p.level, frames, p.curve);
                            self.left = frames as u32;
                            self.reset_clock();
                            Stage::Point(i)
                        }
                        None => Stage::Done,
                    };
                    0
                }
                Stage::Point(i) => {
                    let m = n.min(self.left as usize);
                    (self.level, _) =
                        self.affine(rest.as_deref_mut(), m, self.step, |_| false, Short::Unknown);
                    self.left -= m as u32;
                    if self.left == 0 {
                        let point = flex.and_then(|f| Some((f.sustain, f.points.get(i)?)));
                        self.stage = match point {
                            Some((sustain, p)) => {
                                // Land exactly on the point.
                                self.level = p.level;
                                self.reset_clock();
                                if let Some(rest) = rest {
                                    rest[m - 1] = p.level;
                                }
                                if i == sustain && !flex.is_some_and(|f| f.one_shot) {
                                    match flex.and_then(|f| f.loop_start) {
                                        Some(start) => Stage::Enter(start.saturating_add(1)),
                                        None => Stage::Sustain,
                                    }
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
                    let (level, end) = self.affine(rest, n, step, stop, short);
                    self.level = level;
                    match end {
                        Some(end) => {
                            self.level = sustain;
                            self.reset_clock();
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
                Stage::Sustain if flex.is_some() || self.level > self.floor => {
                    if let Some(rest) = rest {
                        rest.fill(self.level);
                    }
                    n
                }
                Stage::Release => {
                    let floor = self.floor;
                    let (level, end) = if self.clock.is_some() {
                        self.affine(
                            rest,
                            n,
                            (self.release, 0.),
                            |x| x <= 0. || x < floor,
                            Short::Unknown,
                        )
                    } else {
                        match rest {
                            Some(rest) => affine_until(
                                Some(rest),
                                n,
                                self.level,
                                (self.release, 0.0),
                                |x| x <= 0. || x < self.floor,
                                Short::Unknown,
                            ),
                            None => decay_until_silent(n, self.level, self.release, self.floor),
                        }
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
                    terminal.get_or_insert(i);
                    self.stage = Stage::Done;
                    self.level = 0.0;
                    if let Some(rest) = rest {
                        rest.fill(0.0);
                    }
                    n
                }
            };
            i += written;
            if self.done() {
                terminal.get_or_insert(i);
            }
        }
        // Final flex segments and silent sustain enter Done at zero sample
        // cost. Capture their boundary even when it coincides with the end of
        // this fragment; the existing state machine and PCM remain untouched.
        if terminal.is_none()
            && match self.stage {
                Stage::Enter(point) => flex
                    .and_then(|flex| flex.points.get(point))
                    .is_none(),
                Stage::Sustain => flex.is_none() && self.level <= self.floor,
                _ => false,
            }
        {
            terminal = Some(i);
        }
        terminal
    }
}

/// Primary amplitude consumer; Flex and pitch/module sources keep their
/// independently admitted clocks in `Envelope`.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Amplitude {
    Ordinary(Envelope),
    Native(super::ahdsr::Native),
    Shared(super::ahdsr::Shared),
}
impl Amplitude {
    pub(super) fn new_shared(p: &Ahdsr, rate: f32) -> Self {
        Self::Shared(super::ahdsr::Shared::new(p, rate))
    }
    pub(super) fn shared(&self) -> Option<&super::ahdsr::Shared> {
        if let Self::Shared(e) = self { Some(e) } else { None }
    }
    pub(super) fn shared_mut(&mut self) -> Option<&mut super::ahdsr::Shared> {
        if let Self::Shared(e) = self { Some(e) } else { None }
    }
    pub(super) fn release_is_sealed(&self) -> bool {
        match self { Self::Shared(e) => e.release_is_sealed(), _ => true }
    }
    pub(super) fn render_cached(&mut self, points: &[f32], out: &mut [f32]) -> Option<usize> {
        self.shared_mut().expect("shared AHDSR consumer").render_cached(points, out)
    }
    pub(super) fn skip_cached(&mut self, points: &[f32], n: usize) -> Option<usize> {
        self.shared_mut().expect("shared AHDSR consumer").skip_cached(points, n)
    }
    pub fn new(p: &Ahdsr, rate: f32, native: bool) -> Self {
        if native {
            Self::Native(super::ahdsr::Native::new(p, rate))
        } else {
            let mut e = Envelope::new(p, rate);
            e.floor = 0.;
            e.clock = Some(AffineClock::new(e.level));
            Self::Ordinary(e)
        }
    }
    pub(super) fn force_completion(&mut self) {
        if let Self::Native(env) = self { env.force_completion(); }
    }
    pub fn release(&mut self, flex: Option<&Flex>) {
        match self {
            Self::Ordinary(e) => {
                if flex.is_none() && !e.ahd_only && !matches!(e.stage, Stage::Release | Stage::Done)
                {
                    e.floor = e.level * SILENT;
                }
                e.release(flex);
            }
            Self::Native(e) => e.release(),
            // The actual delivered-release transaction has already checked
            // this clone's sealed receipt; its countdown owns source release.
            Self::Shared(_) => {},
        }
    }
    pub fn done(&self) -> bool {
        match self {
            Self::Ordinary(e) => e.done(),
            Self::Native(e) => e.done(),
            Self::Shared(e) => e.done(),
        }
    }
    pub fn level(&self) -> f32 {
        match self {
            Self::Ordinary(e) => e.level(),
            Self::Native(e) => e.level(),
            Self::Shared(e) => e.level(),
        }
    }
    pub fn phase(&self) -> Phase {
        match self {
            Self::Ordinary(e) => e.phase(),
            Self::Native(e) => e.phase(),
            Self::Shared(e) => e.phase(),
        }
    }
    pub(super) fn shape(&self, n: usize) -> Option<Shape> {
        match self {
            Self::Ordinary(e) => e.shape(n),
            Self::Native(_) | Self::Shared(_) => None,
        }
    }
    #[cfg(test)]
    pub fn skip(&mut self, n: usize, flex: Option<&Flex>, rate: f32) {
        match self {
            Self::Ordinary(e) => e.skip(n, flex, rate),
            Self::Native(e) => e.skip(n),
            Self::Shared(_) => unreachable!("shared AHDSR requires its sealed source cache"),
        }
    }
    #[cfg(test)]
    pub fn render(&mut self, out: &mut [f32], flex: Option<&Flex>, rate: f32) {
        match self {
            Self::Ordinary(e) => e.render(out, flex, rate),
            Self::Native(e) => e.render(out),
            Self::Shared(_) => unreachable!("shared AHDSR requires its sealed source cache"),
        }
    }

    pub(super) fn skip_terminal(
        &mut self,
        n: usize,
        flex: Option<&Flex>,
        rate: f32,
    ) -> Option<usize> {
        match self {
            Self::Ordinary(e) => e.skip_terminal(n, flex, rate),
            Self::Native(e) => e.skip_terminal(n),
            Self::Shared(_) => unreachable!("shared AHDSR requires its sealed source cache"),
        }
    }

    pub(super) fn render_terminal(
        &mut self,
        out: &mut [f32],
        flex: Option<&Flex>,
        rate: f32,
    ) -> Option<usize> {
        match self {
            Self::Ordinary(e) => e.render_terminal(out, flex, rate),
            Self::Native(e) => e.render_terminal(out),
            Self::Shared(_) => unreachable!("shared AHDSR requires its sealed source cache"),
        }
    }
}

/// Live primary-source switch plus the separate native no-owner stop fade.
/// Engine owns event offsets and retires at the exact fade boundary; Kontakt's
/// wrapper-span census is a separately documented scheduling limitation.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PrimaryAmplitude {
    pub bypassed: bool,
    fallback: Option<NativeFallbackFade>,
    frames: u32,
}
impl PrimaryAmplitude {
    pub fn new(bypassed: bool, rate: f32) -> Self {
        Self { bypassed, fallback: None, frames: (10.0f32 * 0.001f32 * rate + 0.5f32).max(2.) as u32 }
    }
    pub fn release(&mut self) {
        if self.bypassed && self.fallback.is_none() {
            self.fallback = Some(NativeFallbackFade {
                value: 1., step: -1. / (self.frames - 1) as f32, left: self.frames,
            });
        }
    }
    pub fn fading(&self) -> bool { self.fallback.is_some() }
    pub fn done(&self) -> bool { self.fallback.is_some_and(|fade| fade.left == 0) }
    pub fn run(&mut self, n: usize, out: Option<&mut [f32]>) -> Option<usize> {
        let fade = self.fallback.as_mut()?;
        let terminal = (fade.left as usize <= n).then_some(fade.left as usize);
        match out {
            Some(out) => for amp in out {
                *amp *= fade.value;
                fade.advance();
            },
            None => for _ in 0..n { fade.advance(); },
        }
        terminal
    }
}

#[derive(Clone, Copy, Debug)]
struct NativeFallbackFade { value: f32, step: f32, left: u32 }
impl NativeFallbackFade {
    fn advance(&mut self) {
        if self.left != 0 {
            self.value = (self.value + self.step).clamp(0., 1.);
            self.left -= 1;
        }
    }
}

/// Linear gain ramp for steals, chokes and scripted fades.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Fade {
    value: f32,
    target: f32,
    step: f32,
    pub(super) left: u32,
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
    pub(super) fn steady(&self) -> bool {
        self.left == 0
    }

    /// Advance over `frames` as [`Fade::apply`] would.
    pub(super) fn skip(&mut self, frames: usize) {
        for _ in 0..frames.min(self.left as usize) {
            self.left -= 1;
            self.value = if self.left == 0 {
                self.target
            } else {
                self.value + self.step
            };
        }
    }

    pub(super) fn apply(&mut self, amp: &mut [f32]) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::MAX_BLOCK;

    #[test]
    fn ordinary_amplitude_relative_release_preserves_skip_partition_and_duplicate_off() {
        let params = Ahdsr {
            sustain: 5e-5,
            release: 0.05,
            ..Ahdsr::UNITY
        };
        let create = || {
            let mut amp = Amplitude::new(&params, 48_000., false);
            amp.render(&mut [0.; 2], None, 48_000.);
            amp
        };
        let (mut block, mut scalar, mut skip) = (create(), create(), create());
        let mut generic = Envelope::new(&params, 48_000.);
        generic.render(&mut [0.; 3], None, 48_000.);
        assert!(
            generic.done(),
            "modulation envelopes keep their existing endpoint law"
        );
        for amp in [&mut block, &mut scalar, &mut skip] {
            amp.release(None);
        }
        let mut end = None;
        assert_eq!(
            crate::plugin::tests::allocations(|| {
                for chunk in 0..32 {
                    let (mut a, mut b) = ([0.; 128], [0.; 128]);
                    block.render(&mut a, None, 48_000.);
                    for sample in &mut b {
                        scalar.render(std::slice::from_mut(sample), None, 48_000.);
                    }
                    skip.skip(128, None, 48_000.);
                    assert!(a.iter().zip(&b).all(|(a, b)| (a - b).abs() * 10000. < 1e-6));
                    assert!((block.level() - scalar.level()).abs() * 10000. < 1e-6);
                    assert_eq!(block.phase(), scalar.phase());
                    assert_eq!(block.phase(), skip.phase());
                    assert_eq!(block.level(), skip.level());
                    if chunk == 3 {
                        let Amplitude::Ordinary(e) = &mut block else {
                            unreachable!()
                        };
                        let before = (e.level, e.floor, e.phase());
                        block.release(None);
                        let Amplitude::Ordinary(e) = &block else {
                            unreachable!()
                        };
                        assert_eq!(
                            (e.level, e.floor, e.phase()),
                            before,
                            "duplicate off must retain the original release endpoint"
                        );
                    }
                    if block.done() && end.is_none() {
                        end = Some((chunk + 1) * 128);
                    }
                }
            }),
            0
        );
        // The authored coefficient reaches -60 dB in 50 ms; a -80 dB
        // relative endpoint is therefore at 50 ms * 80 / 60 = 3200 frames.
        // Float rounding can put that crossing in the next 128-frame chunk.
        assert!(
            matches!(end, Some(3200 | 3328)),
            "relative release endpoint {end:?}"
        );
        assert!(block.done() && scalar.done() && skip.done());
        for sustain in [0., 5e-5] {
            let mut one_shot = Amplitude::new(
                &Ahdsr {
                    sustain,
                    ahd_only: true,
                    ..params
                },
                48_000.,
                false,
            );
            one_shot.release(None);
            one_shot.render(&mut [0.; 3], None, 48_000.);
            assert!(one_shot.done(), "AHD stays one-shot");
        }
        let mut zero = Amplitude::new(
            &Ahdsr {
                sustain: 0.,
                ..params
            },
            48_000.,
            false,
        );
        zero.render(&mut [0.; 3], None, 48_000.);
        assert!(
            zero.done(),
            "exact zero sustain keeps its existing completion"
        );
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
            ahd_only: false,
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
                FlexPoint {
                    seconds: 0.013,
                    level: 1.0,
                    curve: 0.7,
                },
                FlexPoint {
                    seconds: 0.05,
                    level: 0.3,
                    curve: -0.4,
                },
                FlexPoint {
                    seconds: 0.2,
                    level: 0.0,
                    curve: 0.2,
                },
            ]
            .into(),
            sustain: 1,
            one_shot: false,
            loop_start: None,
        };
        let ahdsr = |curve| Ahdsr {
            attack: 0.011,
            curve,
            hold: 0.003,
            decay: 0.07,
            sustain: 0.4,
            release: 0.09,
            ahd_only: false,
        };
        let cases = [
            (Envelope::new(&ahdsr(0.0), rate), None),
            (Envelope::new(&ahdsr(0.8), rate), None),
            (Envelope::new(&ahdsr(-0.6), rate), None),
            (
                Envelope::new(
                    &Ahdsr {
                        sustain: 0.0,
                        ..ahdsr(1.0)
                    },
                    rate,
                ),
                None,
            ),
            (
                Envelope::new(
                    &Ahdsr {
                        attack: 1e-7,
                        decay: 0.0,
                        ..ahdsr(-1.0)
                    },
                    rate,
                ),
                None,
            ),
            (
                Envelope::new(
                    &Ahdsr {
                        attack: 0.3,
                        sustain: 1.0,
                        ..ahdsr(0.3)
                    },
                    rate,
                ),
                None,
            ),
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
                assert_eq!(
                    rendered.level().to_bits(),
                    skipped.level().to_bits(),
                    "block {block}"
                );
                assert_eq!(
                    (rendered.stage, rendered.left),
                    (skipped.stage, skipped.left),
                    "block {block}"
                );
            }
            assert!(skipped.done());
        }
    }

    /// No frame of a chunk from a level short of a glide's edge stops it,
    /// and one from the edge does: checked frame by frame.
    #[test]
    fn edges_leave_no_stopping_frame_short_of_them() {
        let rate = 48_000.0;
        let frames = |x: f32, (pow, off): ([f32; 8], [f32; 8])| {
            std::array::from_fn::<f32, 8, _>(|k| pow[k] * x + off[k])
        };
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
        cases.extend(attacks.map(|step| {
            (
                step,
                true,
                Box::new(|x: f32| x >= 1.0) as Box<dyn Fn(f32) -> bool>,
            )
        }));
        cases.extend(decays.map(|(d, s)| {
            (
                (d, s * (1.0 - d)),
                false,
                Box::new(move |x: f32| x - s <= SILENT) as _,
            )
        }));
        for (step, rising, stop) in &cases {
            let e = edge(*step, *rising, stop);
            let short = if *rising {
                Short::Below(e)
            } else {
                Short::Above(e)
            };
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
                    assert!(
                        (o - x).abs() <= 1e-5 * x.abs().max(1.0),
                        "{mul} {add} at {i}: {o} vs {x}"
                    );
                }
                assert_eq!(last, out.last().copied().unwrap_or(0.25));
            }
        }
    }

    /// The editor's envelope drawing follows the laws voices play by.
    #[test]
    fn trace_follows_the_envelope_laws() {
        let env = Ahdsr {
            attack: 0.3,
            curve: 0.0,
            hold: 0.1,
            decay: 2.0,
            sustain: 0.25,
            release: 4.0,
            ahd_only: false,
        };
        let [attack, decay, release] = env.trace(100);
        assert_eq!((attack.len(), decay.len(), release.len()), (101, 101, 101));
        assert_eq!(attack[0], 0.0);
        assert!(
            (attack[50] - 0.5).abs() < 1e-4,
            "linear attack: {}",
            attack[50]
        );
        assert!((attack[100] - 1.0).abs() < 1e-4);
        // −60 dB of the distance to sustain over the decay time.
        assert_eq!(decay[0], 1.0);
        let at = |t: f32| 0.25 + 0.75 * 0.001f32.powf(t);
        assert!(
            (decay[50] - at(0.5)).abs() < 1e-4,
            "{} vs {}",
            decay[50],
            at(0.5)
        );
        assert_eq!(release[0], 0.25);
        assert!((release[50] - 0.25 * 0.001f32.powf(0.5)).abs() < 1e-4);
        // A convex attack rises early; a stage of no time is a point.
        let fast = Ahdsr {
            curve: 1.0,
            decay: 0.0,
            ..env
        }
        .trace(100);
        assert!(fast[0][25] > 0.5);
        assert_eq!(fast[1], vec![0.25]);
    }

    #[test]
    fn sub_frame_curved_attack_is_instant_not_nan() {
        for curve in [-1.0, 1.0] {
            let p = Ahdsr {
                attack: 1e-7,
                curve,
                hold: 0.0,
                decay: 0.0,
                sustain: 1.0,
                release: 0.3,
                ahd_only: false,
            };
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
                ahd_only: false,
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
            one_shot: false,
            loop_start: None,
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

#[cfg(test)]
#[path = "primary_amp_tests.rs"]
mod primary_amp_tests;

#[cfg(all(test, feature = "plugin"))]
#[path = "wnt_tests.rs"]
mod wnt_tests;

#[cfg(all(test, feature = "plugin"))]
#[path = "flex_index_tests.rs"]
mod flex_index_tests;

#[cfg(test)]
#[path = "envelope_validation_tests.rs"]
mod envelope_validation_tests;
