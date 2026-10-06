//! Prepared per-voice modulation: LFOs, envelopes and note/controller sources
//! routed to gain, pan, pitch, filter cutoff/Q, a per-voice tone filter and
//! sample start.
//!
//! Rate: sources and routes are evaluated once per render chunk, at most
//! [`crate::dsp::BLOCK`] frames, at the chunk's end time. Gain and pan ramp
//! linearly across the chunk from the previous control point; pitch, cutoff,
//! Q and tone hold the chunk's midpoint value. Each kind of work is dispatched
//! once per chunk and voice, never per sample.
//!
//! Layout: every prepared program is flat arrays (sources, routes, shapes);
//! runtime state is structure-of-arrays sized at runtime construction for the
//! largest program, so rendering never allocates.
use crate::{Envelope, EnvelopeCurve, Error, envelope::EnvelopeState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LfoShape {
    Sine,
    Triangle,
    Square,
    SawUp,
    SawDown,
    SampleAndHold,
    /// A new random value each cycle, reached linearly by the cycle's end.
    Random,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LfoRate {
    Hertz(f64),
    /// Cycle length in quarter-note beats at [`crate::Runtime::set_tempo`].
    Beats(f64),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lfo {
    pub shape: LfoShape,
    pub rate: LfoRate,
    /// Cycle position at the start, 0..1.
    pub phase: f64,
    /// Silent frames after the voice starts.
    pub delay: u32,
    /// Frames of linear depth ramp after the delay.
    pub fade: u32,
    /// Per-voice cycle from `phase`; otherwise one cycle on the runtime clock.
    pub retrigger: bool,
}

/// What a route reads. Lfo is bipolar (-1..=1); every other source is 0..=1.
#[derive(Clone, Copy, Debug)]
pub enum ModSource {
    Lfo(Lfo),
    /// Gated by the voice's family: releases with it.
    Envelope(Envelope),
    /// Index into [`ModProgram::breakpoints`]; gated like `Envelope`.
    Breakpoints(usize),
    Velocity,
    /// Note number / 127.
    Key,
    /// Current effective value of a controller in the note's performance domain.
    Controller(u8),
    /// The note's expression pressure (MPE channel pressure, poly pressure).
    Pressure,
    /// The note's expression timbre (MPE CC74).
    Timbre,
    /// A uniform value drawn once per voice.
    Random,
    Constant,
    /// `clamp(1 − held / frames, 0, 1)`: the share of a `frames` countdown
    /// left when the key was released (now, while it is down).
    ReleaseCounter {
        frames: u32,
    },
}

impl ModSource {
    fn bipolar(&self) -> bool {
        matches!(self, Self::Lfo(_))
    }
}

/// Destination and law; `v` is the route's transformed source value and `u`
/// its unipolar view ((v + 1) / 2 for bipolar sources).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModTarget {
    /// gain × (1 − depth·(1 − u)).
    Attenuate,
    /// gain × 10^(depth·v / 20); depth in decibels.
    Decibels,
    /// pan + depth·v, saturated to -1..=1.
    Pan,
    /// pitch + depth·v semitones.
    Pitch,
    /// Every voice-chain state-variable filter's cutoff × 2^(depth·v / 12).
    Cutoff,
    /// Every voice-chain state-variable filter's Q × 10^(depth·v / 20).
    Resonance,
    /// A per-voice low-pass, open (bypassed) at 0, closing by depth·v semitones
    /// below the open cutoff when the sum is negative.
    Tone,
    /// Start offset of depth·u × the region's start range, at voice start only.
    SampleStart,
}

#[derive(Clone, Copy, Debug)]
pub struct ModRoute {
    /// Index into [`ModProgram::sources`].
    pub source: usize,
    pub target: ModTarget,
    pub depth: f64,
    /// Unipolar `1 - v`, bipolar `-v`, before the shape.
    pub invert: bool,
    /// Index into [`ModProgram::shapes`].
    pub shape: Option<usize>,
    /// One-pole lag frames to reach 99% of a step (Kontakt's lag law).
    pub lag: u32,
    /// Depth multiplier from a second source (a source × source product).
    pub scale: Option<ModScale>,
}

/// Multiplies a route's depth by `shape(x)` of another source, `x` its unipolar
/// view; without a shape the multiplier is `x`. Evaluated with the route.
#[derive(Clone, Copy, Debug)]
pub struct ModScale {
    /// Index into [`ModProgram::sources`].
    pub source: usize,
    /// Index into [`ModProgram::shapes`]; its output is used unmapped.
    pub shape: Option<usize>,
}

impl ModRoute {
    pub fn new(source: usize, target: ModTarget, depth: f64) -> Self {
        Self {
            source,
            target,
            depth,
            invert: false,
            shape: None,
            lag: 0,
            scale: None,
        }
    }
}

/// A point a breakpoint envelope glides to from the previous level (0 at the
/// start) over `frames`, along `curve`.
#[derive(Clone, Copy, Debug)]
pub struct Breakpoint {
    pub frames: u32,
    pub level: f32,
    pub curve: EnvelopeCurve,
}

/// A multi-segment envelope. It holds at `points[sustain]` while gated; a
/// release glides from the current level through the points after
/// `sustain` (jumping there if it is not reached yet), then holds the last.
#[derive(Clone, Debug, Default)]
pub struct Breakpoints {
    pub points: Vec<Breakpoint>,
    pub sustain: Option<usize>,
}

/// One voice modulation program, shared by the regions bound to it.
#[derive(Clone, Debug, Default)]
pub struct ModProgram {
    pub sources: Vec<ModSource>,
    pub breakpoints: Vec<Breakpoints>,
    pub routes: Vec<ModRoute>,
    /// Piecewise-linear transfer curves over 0..=1 as ascending (x, y) points.
    pub shapes: Vec<Vec<(f64, f64)>>,
}

#[derive(Clone, Copy)]
enum Prepared {
    Lfo(Lfo),
    Envelope(usize),
    /// Index into the program's breakpoint envelopes and the voice's segments.
    Breakpoints(usize),
    Velocity,
    Key,
    Controller(u8),
    Pressure,
    Timbre,
    Random,
    Constant,
    ReleaseCounter(u32),
}

struct Program {
    sources: Box<[Prepared]>,
    bipolar: Box<[bool]>,
    envelopes: Box<[Envelope]>,
    breakpoints: Box<[Breakpoints]>,
    routes: Box<[ModRoute]>,
    shapes: Box<[Box<[(f32, f32)]>]>,
    /// Whether any route reaches each kind of output, so unused work is skipped.
    filter: bool,
    tone: bool,
    start: bool,
}

/// Immutable per-plan programs and region bindings.
#[derive(Default)]
pub(crate) struct VoiceModulation {
    programs: Box<[Program]>,
    regions: Box<[Option<u32>]>,
    start_ranges: Box<[u32]>,
    sources: usize,
    envelopes: usize,
    breakpoints: usize,
    routes: usize,
}

impl VoiceModulation {
    pub fn new(
        programs: Vec<ModProgram>,
        regions: Vec<Option<usize>>,
        start_ranges: Vec<u32>,
    ) -> Result<Self, Error> {
        if start_ranges.len() != regions.len()
            || regions.iter().flatten().any(|p| *p >= programs.len())
            || programs.len() > u32::MAX as usize
        {
            return Err(Error::InvalidInput);
        }
        let mut compiled = Vec::with_capacity(programs.len());
        for program in programs {
            let mut envelopes = Vec::new();
            let sources = program
                .sources
                .iter()
                .map(|source| {
                    Ok(match *source {
                        ModSource::Lfo(lfo) => {
                            let rate = match lfo.rate {
                                LfoRate::Hertz(r) | LfoRate::Beats(r) => r,
                            };
                            if !(rate.is_finite() && rate > 0.0)
                                || !(0.0..=1.0).contains(&lfo.phase)
                            {
                                return Err(Error::InvalidInput);
                            }
                            Prepared::Lfo(lfo)
                        }
                        ModSource::Envelope(envelope) => {
                            envelopes.push(envelope);
                            Prepared::Envelope(envelopes.len() - 1)
                        }
                        ModSource::Breakpoints(index) => {
                            let b = program.breakpoints.get(index).ok_or(Error::InvalidInput)?;
                            if b.points.len() > u32::MAX as usize - 1
                                || b.sustain.is_some_and(|s| s >= b.points.len())
                                || b.points.iter().any(|p| !p.level.is_finite())
                            {
                                return Err(Error::InvalidInput);
                            }
                            Prepared::Breakpoints(index)
                        }
                        ModSource::Velocity => Prepared::Velocity,
                        ModSource::Key => Prepared::Key,
                        ModSource::Controller(cc) if cc < 128 => Prepared::Controller(cc),
                        ModSource::Controller(_) => return Err(Error::InvalidInput),
                        ModSource::Pressure => Prepared::Pressure,
                        ModSource::Timbre => Prepared::Timbre,
                        ModSource::Random => Prepared::Random,
                        ModSource::Constant => Prepared::Constant,
                        ModSource::ReleaseCounter { frames: 0 } => {
                            return Err(Error::InvalidInput);
                        }
                        ModSource::ReleaseCounter { frames } => Prepared::ReleaseCounter(frames),
                    })
                })
                .collect::<Result<Box<[_]>, Error>>()?;
            let mut shapes = Vec::with_capacity(program.shapes.len());
            for shape in &program.shapes {
                if shape.is_empty()
                    || shape.windows(2).any(|w| w[1].0 < w[0].0)
                    || shape
                        .iter()
                        .any(|&(x, y)| !(0.0..=1.0).contains(&x) || !y.is_finite())
                {
                    return Err(Error::InvalidInput);
                }
                shapes.push(
                    shape
                        .iter()
                        .map(|&(x, y)| (x as f32, y as f32))
                        .collect::<Box<[_]>>(),
                );
            }
            for route in &program.routes {
                if route.source >= sources.len()
                    || !route.depth.is_finite()
                    || route.shape.is_some_and(|s| s >= shapes.len())
                    || route.scale.is_some_and(|s| {
                        s.source >= sources.len() || s.shape.is_some_and(|s| s >= shapes.len())
                    })
                {
                    return Err(Error::InvalidInput);
                }
            }
            let reaches =
                |targets: &[ModTarget]| program.routes.iter().any(|r| targets.contains(&r.target));
            compiled.push(Program {
                bipolar: program.sources.iter().map(ModSource::bipolar).collect(),
                sources,
                envelopes: envelopes.into_boxed_slice(),
                breakpoints: program.breakpoints.into_boxed_slice(),
                filter: reaches(&[ModTarget::Cutoff, ModTarget::Resonance]),
                tone: reaches(&[ModTarget::Tone]),
                start: reaches(&[ModTarget::SampleStart]),
                routes: program.routes.into_boxed_slice(),
                shapes: shapes.into_boxed_slice(),
            });
        }
        Ok(Self {
            sources: compiled.iter().map(|p| p.sources.len()).max().unwrap_or(0),
            envelopes: compiled
                .iter()
                .map(|p| p.envelopes.len())
                .max()
                .unwrap_or(0),
            breakpoints: compiled
                .iter()
                .map(|p| p.breakpoints.len())
                .max()
                .unwrap_or(0),
            routes: compiled.iter().map(|p| p.routes.len()).max().unwrap_or(0),
            programs: compiled.into_boxed_slice(),
            regions: regions.into_iter().map(|p| p.map(|p| p as u32)).collect(),
            start_ranges: start_ranges.into_boxed_slice(),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.programs.is_empty()
    }

    pub fn program(&self, region: usize) -> Option<u32> {
        self.regions.get(region).copied().flatten()
    }

    /// Start offset in source frames for a voice of `region` starting now.
    pub fn start_offset(&self, region: usize, inputs: &Inputs<'_>, seed: u64) -> u32 {
        let Some(program) = self.program(region) else {
            return 0;
        };
        let program = &self.programs[program as usize];
        if !program.start {
            return 0;
        }
        let mut fraction = 0.0;
        for route in program
            .routes
            .iter()
            .filter(|r| r.target == ModTarget::SampleStart)
        {
            // Lfo and envelope values at the voice's first frame.
            let at_start = |index: usize| match program.sources[index] {
                Prepared::Lfo(lfo) if lfo.delay == 0 && lfo.fade == 0 => {
                    wave(lfo.shape, lfo.phase, seed ^ index as u64)
                }
                Prepared::Lfo(_) | Prepared::Envelope(_) | Prepared::Breakpoints(_) => 0.0,
                other => other.input(inputs, seed, index),
            };
            let bipolar = program.bipolar[route.source];
            let v = program.transform(route, at_start(route.source), bipolar);
            let scale = route
                .scale
                .map_or(1.0, |s| program.scale(s, at_start(s.source)));
            fraction += route.depth * scale * unipolar(v, bipolar);
        }
        (fraction.clamp(0.0, 1.0) * f64::from(self.start_ranges[region])) as u32
    }
}

impl Prepared {
    fn input(self, inputs: &Inputs<'_>, seed: u64, index: usize) -> f64 {
        match self {
            Self::Velocity => inputs.velocity,
            Self::Key => inputs.key,
            Self::Controller(cc) => f64::from(inputs.controllers[usize::from(cc)]) * FULL_SCALE,
            Self::Pressure => f64::from(inputs.pressure) * FULL_SCALE,
            Self::Timbre => f64::from(inputs.timbre) * FULL_SCALE,
            Self::Random => uniform(hash(seed, index as u64, u64::MAX)),
            Self::Constant => 1.0,
            Self::ReleaseCounter(frames) => {
                (1.0 - inputs.held as f64 / f64::from(frames)).clamp(0.0, 1.0)
            }
            Self::Lfo(_) | Self::Envelope(_) | Self::Breakpoints(_) => {
                unreachable!("stateful sources")
            }
        }
    }
}

const FULL_SCALE: f64 = 1.0 / u32::MAX as f64;

impl Program {
    fn scale(&self, scale: ModScale, raw: f64) -> f64 {
        let x = unipolar(raw, self.bipolar[scale.source]);
        scale.shape.map_or(x, |shape| {
            f64::from(evaluate(&self.shapes[shape], x as f32))
        })
    }

    fn transform(&self, route: &ModRoute, raw: f64, bipolar: bool) -> f64 {
        let mut v = if route.invert {
            if bipolar { -raw } else { 1.0 - raw }
        } else {
            raw
        };
        if let Some(shape) = route.shape {
            let x = unipolar(v, bipolar);
            let y = evaluate(&self.shapes[shape], x as f32) as f64;
            v = if bipolar { 2.0 * y - 1.0 } else { y };
        }
        v
    }
}

fn unipolar(v: f64, bipolar: bool) -> f64 {
    if bipolar { (v + 1.0) * 0.5 } else { v }
}

fn evaluate(points: &[(f32, f32)], x: f32) -> f32 {
    let after = points.partition_point(|p| p.0 < x);
    match after {
        0 => points[0].1,
        n if n == points.len() => points[n - 1].1,
        n => {
            let (a, b) = (points[n - 1], points[n]);
            let span = b.0 - a.0;
            if span > 0.0 {
                a.1 + (b.1 - a.1) * (x - a.0) / span
            } else {
                b.1
            }
        }
    }
}

/// splitmix64 of a seed, source and cycle: deterministic per voice and cycle,
/// and identical across voices for free-running sources (seed 0).
fn hash(seed: u64, source: u64, cycle: u64) -> u64 {
    let mut z = seed
        .wrapping_add(source.wrapping_mul(0x9e37_79b9_7f4a_7c15))
        .wrapping_add(cycle.wrapping_mul(0xbf58_476d_1ce4_e5b9));
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

fn uniform(bits: u64) -> f64 {
    (bits >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
}

/// Waveform value at cycle position `phase` (any real; the integer part counts
/// cycles for the random shapes).
fn wave(shape: LfoShape, phase: f64, seed: u64) -> f64 {
    let cycle = phase.floor();
    let t = phase - cycle;
    match shape {
        LfoShape::Sine => (t * std::f64::consts::TAU).sin(),
        LfoShape::Triangle => {
            if t < 0.25 {
                4.0 * t
            } else if t < 0.75 {
                2.0 - 4.0 * t
            } else {
                4.0 * t - 4.0
            }
        }
        LfoShape::Square => {
            if t < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        LfoShape::SawUp => 2.0 * t - 1.0,
        LfoShape::SawDown => 1.0 - 2.0 * t,
        LfoShape::SampleAndHold => 2.0 * uniform(hash(seed, 0, cycle as i64 as u64)) - 1.0,
        LfoShape::Random => {
            let from = 2.0 * uniform(hash(seed, 0, cycle as i64 as u64)) - 1.0;
            let to = 2.0 * uniform(hash(seed, 0, (cycle as i64 + 1) as u64)) - 1.0;
            from + (to - from) * t
        }
    }
}

/// Note and controller inputs read by a voice's sources.
pub(crate) struct Inputs<'a> {
    pub velocity: f64,
    pub key: f64,
    pub pressure: u32,
    pub timbre: u32,
    pub controllers: &'a [u32; 128],
    /// Frames from the note's admission to its key release (or now).
    pub held: u64,
}

impl<'a> Inputs<'a> {
    pub fn new(
        note: &crate::Note,
        expression: crate::Expression,
        controllers: &'a [u32; 128],
        held: u64,
    ) -> Self {
        Self {
            velocity: note.velocity,
            key: f64::from(note.pitch.key()) / 127.0,
            pressure: expression.pressure,
            timbre: expression.timbre,
            controllers,
            held,
        }
    }
}

/// Per-chunk results a voice renders with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Outputs {
    pub gains: [f32; 2],
    pub pitch: f64,
    /// Cutoff and Q factors for the voice chain's state-variable filters.
    pub filter: [f64; 2],
    /// Semitones below the tone filter's open cutoff; 0 bypasses it.
    pub tone: f64,
}

impl Default for Outputs {
    fn default() -> Self {
        Self {
            gains: [1.0; 2],
            pitch: 0.0,
            filter: [1.0; 2],
            tone: 0.0,
        }
    }
}

/// Mutable per-voice state for one plan generation, sized at construction.
pub(crate) struct VoiceModState {
    sources: usize,
    envelopes: usize,
    breakpoints: usize,
    routes: usize,
    program: Box<[Option<u32>]>,
    seed: Box<[u64]>,
    age: Box<[u32]>,
    outputs: Box<[Outputs]>,
    /// The control point before `outputs`.
    previous: Box<[Outputs]>,
    /// Runtime frames of `previous` and `outputs`.
    times: Box<[[u64; 2]]>,
    /// Tone filter state: two integrator pairs (left/right).
    tone: Box<[[[f64; 2]; 2]]>,
    phase: Box<[f64]>,
    values: Box<[f64]>,
    lagged: Box<[f64]>,
    envelope: Box<[EnvelopeState]>,
    segments: Box<[Segment]>,
}

/// A voice's position in one breakpoint envelope.
#[derive(Clone, Copy, Debug, Default)]
struct Segment {
    /// The point being glided to; past the end, the last level holds.
    index: u32,
    age: u32,
    from: f32,
    level: f32,
    released: bool,
}

impl Segment {
    fn advance(&mut self, envelope: &Breakpoints, mut frames: u32) -> f32 {
        while let Some(point) = envelope.points.get(self.index as usize) {
            let left = point.frames - self.age;
            if frames < left {
                self.age += frames;
                let t = f64::from(self.age) / f64::from(point.frames);
                self.level = self.from + (point.level - self.from) * point.curve.value(t) as f32;
                return self.level;
            }
            frames -= left;
            self.age = point.frames;
            self.level = point.level;
            if !self.released && envelope.sustain == Some(self.index as usize) {
                return self.level;
            }
            self.index += 1;
            self.age = 0;
            self.from = self.level;
        }
        self.level
    }

    fn release(&mut self, envelope: &Breakpoints) {
        if self.released {
            return;
        }
        self.released = true;
        if let Some(sustain) = envelope.sustain
            && self.index as usize <= sustain
        {
            self.index = sustain as u32 + 1;
            self.age = 0;
            self.from = self.level;
        }
    }
}

impl VoiceModState {
    pub fn new(modulation: &VoiceModulation, voices: usize) -> Result<Self, Error> {
        let slots = |n: usize| n.checked_mul(voices).ok_or(Error::Capacity);
        if modulation.is_empty() {
            return Ok(Self::empty());
        }
        Ok(Self {
            sources: modulation.sources,
            envelopes: modulation.envelopes,
            breakpoints: modulation.breakpoints,
            routes: modulation.routes,
            program: crate::dsp::allocate(voices)?,
            seed: crate::dsp::allocate(voices)?,
            age: crate::dsp::allocate(voices)?,
            outputs: crate::dsp::allocate(voices)?,
            previous: crate::dsp::allocate(voices)?,
            times: crate::dsp::allocate(voices)?,
            tone: crate::dsp::allocate(voices)?,
            phase: crate::dsp::allocate(slots(modulation.sources)?)?,
            values: crate::dsp::allocate(slots(modulation.sources)?)?,
            lagged: crate::dsp::allocate(slots(modulation.routes)?)?,
            envelope: std::iter::repeat_n(
                EnvelopeState::new(Envelope::default()),
                slots(modulation.envelopes)?,
            )
            .collect(),
            segments: crate::dsp::allocate(slots(modulation.breakpoints)?)?,
        })
    }

    fn empty() -> Self {
        Self {
            sources: 0,
            envelopes: 0,
            breakpoints: 0,
            routes: 0,
            program: Box::new([]),
            seed: Box::new([]),
            age: Box::new([]),
            outputs: Box::new([]),
            previous: Box::new([]),
            times: Box::new([]),
            tone: Box::new([]),
            phase: Box::new([]),
            values: Box::new([]),
            lagged: Box::new([]),
            envelope: Box::new([]),
            segments: Box::new([]),
        }
    }

    pub fn program(&self, voice: usize) -> Option<u32> {
        self.program.get(voice).copied().flatten()
    }

    /// Bind a starting voice and compute its first control point, so the first
    /// chunk ramps from the onset values rather than from unity.
    pub fn start(
        &mut self,
        modulation: &VoiceModulation,
        voice: usize,
        region: usize,
        inputs: &Inputs<'_>,
        clock: Clock,
        seed: u64,
    ) {
        let Some(program) = self.program.get_mut(voice) else {
            return;
        };
        *program = modulation.program(region);
        let Some(index) = *program else {
            return;
        };
        let p = &modulation.programs[index as usize];
        self.seed[voice] = seed;
        self.age[voice] = 0;
        self.tone[voice] = [[0.0; 2]; 2];
        for (i, source) in p.sources.iter().enumerate() {
            if let Prepared::Lfo(lfo) = source {
                self.phase[voice * self.sources + i] = lfo.phase;
            }
        }
        for (i, envelope) in p.envelopes.iter().enumerate() {
            self.envelope[voice * self.envelopes + i] = EnvelopeState::new(*envelope);
        }
        for segment in &mut self.segments[voice * self.breakpoints..][..p.breakpoints.len()] {
            *segment = Segment::default();
        }
        self.outputs[voice] = self.evaluate(p, voice, inputs, clock, 0, true);
        self.previous[voice] = self.outputs[voice];
        self.times[voice] = [clock.now; 2];
    }

    pub fn release(&mut self, modulation: &VoiceModulation, voice: usize) {
        if let Some(program) = self.program(voice) {
            let p = &modulation.programs[program as usize];
            for state in &mut self.envelope[voice * self.envelopes..][..p.envelopes.len()] {
                state.release();
            }
            let segments = &mut self.segments[voice * self.breakpoints..][..p.breakpoints.len()];
            for (segment, envelope) in segments.iter_mut().zip(&p.breakpoints) {
                segment.release(envelope);
            }
        }
    }

    pub fn stop(&mut self, voice: usize) {
        if let Some(program) = self.program.get_mut(voice) {
            *program = None;
        }
    }

    /// The control ramp covering a segment starting at `clock.now`. Points
    /// sit on the absolute `CELL` grid (and the voice onset), so a segment
    /// entering a new cell evaluates its end once and segments inside a cell
    /// reuse it: output does not depend on how the host splits blocks.
    /// Segments must not cross a grid line.
    pub fn advance(
        &mut self,
        modulation: &VoiceModulation,
        voice: usize,
        inputs: &Inputs<'_>,
        mut clock: Clock,
    ) -> Ramp {
        let [begin, end] = self.times[voice];
        if let (Some(program), true) = (self.program(voice), clock.now >= end) {
            let p = &modulation.programs[program as usize];
            let target = (clock.now / CELL + 1) * CELL;
            clock.now = target;
            let next = self.evaluate(p, voice, inputs, clock, (target - end) as u32, false);
            self.previous[voice] = std::mem::replace(&mut self.outputs[voice], next);
            self.times[voice] = [end, target];
        } else if clock.now >= end {
            return Ramp {
                from: self.outputs[voice],
                to: self.outputs[voice],
                begin,
                end,
            };
        }
        let [begin, end] = self.times[voice];
        Ramp {
            from: self.previous[voice],
            to: self.outputs[voice],
            begin,
            end,
        }
    }

    fn evaluate(
        &mut self,
        p: &Program,
        voice: usize,
        inputs: &Inputs<'_>,
        clock: Clock,
        frames: u32,
        onset: bool,
    ) -> Outputs {
        let seed = self.seed[voice];
        let age = self.age[voice].saturating_add(frames);
        self.age[voice] = age;
        let values = &mut self.values[voice * self.sources..][..p.sources.len()];
        let phases = &mut self.phase[voice * self.sources..][..p.sources.len()];
        for (i, source) in p.sources.iter().enumerate() {
            values[i] = match *source {
                Prepared::Lfo(lfo) => {
                    let hz = match lfo.rate {
                        LfoRate::Hertz(hz) => hz,
                        LfoRate::Beats(beats) => clock.tempo / (60.0 * beats),
                    };
                    let (phase, seed) = if lfo.retrigger {
                        phases[i] += hz * f64::from(frames) / clock.rate;
                        (phases[i], seed ^ i as u64)
                    } else {
                        // ponytail: free cycles read absolute time, so a tempo change jumps phase.
                        (lfo.phase + hz * clock.now as f64 / clock.rate, i as u64)
                    };
                    let depth = if age < lfo.delay {
                        0.0
                    } else if lfo.fade == 0 {
                        1.0
                    } else {
                        (f64::from(age - lfo.delay) / f64::from(lfo.fade)).min(1.0)
                    };
                    depth * wave(lfo.shape, phase, seed)
                }
                Prepared::Envelope(e) => {
                    f64::from(self.envelope[voice * self.envelopes + e].advance(frames))
                }
                Prepared::Breakpoints(b) => f64::from(
                    self.segments[voice * self.breakpoints + b].advance(&p.breakpoints[b], frames),
                ),
                other => other.input(inputs, seed, i),
            };
        }
        let mut gain = 1.0;
        let mut decibels = 0.0;
        let mut pan = 0.0;
        let mut out = Outputs::default();
        let mut resonance = 0.0;
        let mut cutoff = 0.0;
        let lagged = &mut self.lagged[voice * self.routes..][..p.routes.len()];
        for (route, lagged) in p.routes.iter().zip(lagged) {
            let bipolar = p.bipolar[route.source];
            let mut v = p.transform(route, values[route.source], bipolar);
            if route.lag != 0 && !onset {
                // 99% of a step in `lag` frames: Kontakt's one-pole lag law.
                let alpha = 1.0
                    - (-2.0 * std::f64::consts::LN_10 * f64::from(frames) / f64::from(route.lag))
                        .exp();
                v = *lagged + (v - *lagged) * alpha;
            }
            *lagged = v;
            let d = route
                .scale
                .map_or(route.depth, |s| route.depth * p.scale(s, values[s.source]));
            match route.target {
                ModTarget::Attenuate => gain *= 1.0 - d * (1.0 - unipolar(v, bipolar)),
                ModTarget::Decibels => decibels += d * v,
                ModTarget::Pan => pan += d * v,
                ModTarget::Pitch => out.pitch += d * v,
                ModTarget::Cutoff => cutoff += d * v,
                ModTarget::Resonance => resonance += d * v,
                ModTarget::Tone => out.tone += d * v,
                ModTarget::SampleStart => {}
            }
        }
        let gain = (gain * 10f64.powf(decibels / 20.0)).max(0.0);
        let pan = pan.clamp(-1.0, 1.0);
        out.gains = [
            (gain * (1.0 - pan.max(0.0))) as f32,
            (gain * (1.0 + pan.min(0.0))) as f32,
        ];
        if p.filter {
            out.filter = [(cutoff / 12.0).exp2(), 10f64.powf(resonance / 20.0)];
        }
        out.tone = if p.tone { out.tone.min(0.0) } else { 0.0 };
        out
    }

    /// Mix one rendered segment starting at runtime frame `at` into `output`,
    /// ramping gains along `ramp` and running the voice's tone filter at the
    /// ramp's midpoint when closed.
    pub fn mix(
        &mut self,
        voice: usize,
        chunk: &mut [crate::Frame],
        output: &mut [crate::Frame],
        ramp: Ramp,
        at: u64,
        rate: f64,
    ) {
        let Ramp {
            from,
            to,
            begin,
            end,
        } = ramp;
        let tone = (from.tone + to.tone) * 0.5;
        if tone < 0.0 {
            let hz = (TONE_OPEN * rate * (tone / 12.0).exp2()).max(10.0);
            let g = (std::f64::consts::PI * hz / rate).tan();
            // Butterworth TPT state-variable low-pass.
            let k = std::f64::consts::SQRT_2;
            let a1 = 1.0 / (1.0 + g * (g + k));
            let (a2, a3) = (g * a1, g * g * a1);
            let state = &mut self.tone[voice];
            for frame in chunk.iter_mut() {
                for (channel, sample) in frame.iter_mut().enumerate() {
                    let [ic1, ic2] = &mut state[channel];
                    let v3 = f64::from(*sample) - *ic2;
                    let v1 = a1 * *ic1 + a2 * v3;
                    let v2 = *ic2 + a2 * *ic1 + a3 * v3;
                    *ic1 = 2.0 * v1 - *ic1;
                    *ic2 = 2.0 * v2 - *ic2;
                    *sample = v2 as f32;
                }
            }
            for channel in state.iter_mut() {
                *channel = channel.map(crate::dsp::flush);
            }
        } else {
            // Open: the filter passes its input, so its integrators follow it.
            self.tone[voice] = [[0.0; 2]; 2];
        }
        let len = end.saturating_sub(begin).max(1) as f32;
        let step = [
            (to.gains[0] - from.gains[0]) / len,
            (to.gains[1] - from.gains[1]) / len,
        ];
        let offset = at.saturating_sub(begin) as f32;
        for (i, (out, frame)) in output.iter_mut().zip(chunk.iter()).enumerate() {
            let at = offset + (i + 1) as f32;
            out[0] += frame[0] * (from.gains[0] + step[0] * at);
            out[1] += frame[1] * (from.gains[1] + step[1] * at);
        }
    }
}

/// Control grid on the runtime clock, in frames.
pub(crate) const CELL: u64 = crate::dsp::BLOCK as u64;

/// Control points around a segment: `from` at runtime frame `begin`, `to` at
/// `end`. Gains ramp between them; pitch, cutoff, Q and tone hold the midpoint.
#[derive(Clone, Copy)]
pub(crate) struct Ramp {
    pub from: Outputs,
    pub to: Outputs,
    pub begin: u64,
    pub end: u64,
}

/// The tone filter's open cutoff as a fraction of the sample rate.
const TONE_OPEN: f64 = 0.45;

/// Shared time inputs: output rate, tempo and the chunk end on the runtime clock.
#[derive(Clone, Copy)]
pub(crate) struct Clock {
    pub rate: f64,
    pub tempo: f64,
    pub now: u64,
}

impl crate::Runtime {
    /// Host tempo for beat-synced LFOs, in quarter notes per minute.
    pub fn set_tempo(&mut self, bpm: f64) -> Result<(), Error> {
        if !(bpm.is_finite() && bpm > 0.0) {
            return Err(Error::InvalidInput);
        }
        self.tempo = bpm;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waves_start_where_their_documented_shapes_do() {
        for (shape, values) in [
            (LfoShape::Sine, [0.0, 1.0, 0.0, -1.0]),
            (LfoShape::Triangle, [0.0, 1.0, 0.0, -1.0]),
            (LfoShape::Square, [1.0, 1.0, -1.0, -1.0]),
            (LfoShape::SawUp, [-1.0, -0.5, 0.0, 0.5]),
            (LfoShape::SawDown, [1.0, 0.5, 0.0, -0.5]),
        ] {
            for (i, expected) in values.into_iter().enumerate() {
                let actual = wave(shape, i as f64 * 0.25, 0);
                assert!((actual - expected).abs() < 1e-12, "{shape:?} {i}: {actual}");
            }
        }
        // Held within a cycle, new across cycles, identical for the same seed.
        let held = wave(LfoShape::SampleAndHold, 3.1, 7);
        assert_eq!(held, wave(LfoShape::SampleAndHold, 3.9, 7));
        assert_ne!(held, wave(LfoShape::SampleAndHold, 4.1, 7));
        let random = |p| wave(LfoShape::Random, p, 7);
        assert!((random(4.0) - wave(LfoShape::SampleAndHold, 4.0, 7)).abs() < 1e-12);
        assert!((random(4.5) - (random(4.0) + random(5.0)) * 0.5).abs() < 1e-12);
        for p in 0..1000 {
            let v = random(p as f64 * 0.37);
            assert!((-1.0..=1.0).contains(&v));
        }
    }

    #[test]
    fn shapes_interpolate_and_clamp_at_their_ends() {
        let points = [(0.0, 0.2), (0.5, 1.0), (1.0, 0.0)];
        assert_eq!(evaluate(&points, 0.0), 0.2);
        assert_eq!(evaluate(&points, 0.25), 0.6);
        assert_eq!(evaluate(&points, 0.75), 0.5);
        assert_eq!(evaluate(&[(0.5, 0.3)], 0.1), 0.3);
        assert_eq!(evaluate(&[(0.5, 0.3)], 0.9), 0.3);
    }
}
