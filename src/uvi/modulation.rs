//! Native UVI control graph. No legacy fixed-size modulation-slot lowering.
//!
//! Format observations: locally owned Program XML; native reference access was
//! UVI Workstation 3.1.16 executable (static code inspection, not clean-room).
//! Mapper interpolation, integer rounding, polarity and source inversion follow
//! observed arithmetic. Script ramps and parameter units are documented at
//! https://lua.uvi.net/group___voice.html and https://lua.uvi.net/_elements.html.
//! No preset tables, commercial scripts or executable code are included here.
use super::{
    playback::PathIndex,
    program::{NodeId, Program},
};
use anyhow::{Context, Result, bail, ensure};
use rustc_hash::FxHashMap;
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet},
};

/// An existing graph node cannot execute as a modulation source.
#[derive(Debug)]
pub(crate) struct UnsupportedSourceKind {
    pub(crate) node: NodeId,
}
impl std::fmt::Display for UnsupportedSourceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Unsupported UVI source kind at node {}", self.node)
    }
}
impl std::error::Error for UnsupportedSourceKind {}

/// A known source node is rejected by an existing scalar admission gate.
/// Private error identity only; no Program attributes or public report fields.
#[derive(Debug)]
pub(crate) struct UnsupportedSourceSetting {
    pub(crate) node: NodeId,
    pub(crate) source_kind: &'static str,
    pub(crate) parameter: &'static str,
    pub(crate) observed: f64,
}
impl UnsupportedSourceSetting {
    /// Validate node/kind/parameter/scalar shape, not a cross-Program fingerprint.
    /// Production binding comes from handling ModulationGraph::new(program)'s
    /// error against that same Program in the same preflight invocation.
    pub(crate) fn node_in(&self, program: &Program) -> Option<NodeId> {
        let known_parameter = match self.source_kind {
            "LFO" => matches!(self.parameter, "WaveFormType" | "Retrigger" | "Smooth"),
            "StepEnvelope" => matches!(self.parameter, "SyncToHost" | "Retrigger" | "InterpolationMode" | "Smooth" | "Bipolar" | "Depth" | "ManualTrigger" | "Bypass"),
            _ => false,
        };
        (known_parameter && self.observed.is_finite()
            && program.nodes.get(self.node).is_some_and(|node| node.kind == self.source_kind))
            .then_some(self.node)
    }
}
impl std::fmt::Display for UnsupportedSourceSetting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Keep the original public error wording and first-cause identity.
        match (self.source_kind, self.parameter) {
            ("LFO", "WaveFormType") => write!(f, "Unverified UVI LFO waveform type at node {}", self.node),
            ("LFO", "Retrigger") => write!(f, "Unverified UVI square LFO trigger mode at node {}", self.node),
            ("LFO", "Smooth") => write!(f, "Unimplemented UVI deterministic LFO smoothing at node {}", self.node),
            _ => write!(f, "Unverified UVI {} {} at node {}", self.source_kind, self.parameter, self.node),
        }
    }
}
impl std::error::Error for UnsupportedSourceSetting {}
fn source_setting_gate(node: NodeId, source_kind: &'static str, parameter: &'static str, observed: f64, admitted: bool) -> Result<()> {
    if admitted { Ok(()) }
    else { Err(UnsupportedSourceSetting { node, source_kind, parameter, observed }.into()) }
}

pub type Parameter = (NodeId, String);
type SourceStateKey = (NodeId, Option<u32>, Option<u64>);
type BuiltinStateKey = (u8, Option<u32>, Option<u64>);
pub const FIDELITY_DIAGNOSTIC: &str = "Native UVI control graph uses measured Mode0 gain, matrix, Ratio, Depth, Value, EQ GainScale, OnePole and LFO frequency laws; LFO sample scheduling, nonaligned target interpolation, host block segmentation, live envelope-control smoothing and float-rounding parity remain unverified against reference audio; random LFO and stochastic-source clock seeds, cross-voice RNG ordering and rare Gaussian float rounding cannot be reconstructed from serialized programs";
const LIMIT: usize = 100_000;
const DEPTH: usize = 128;

/// Authoritative host beat at a renderer frame. This clock is independent
/// from elapsed audio time and can change position at a transport snapshot.
#[derive(Clone, Copy, Debug)]
pub struct HostPosition {
    pub frame: u64,
    pub beat: f64,
    pub playing: bool,
}

pub struct Inputs {
    /// Render rate used by native 32-sample control clocks.
    pub sample_rate: f64,
    /// Host tempo for synchronized sources, in beats per minute.
    pub host_tempo: f64,
    /// None selects the synthetic offline clock; hosted rendering supplies snapshots.
    pub host_position: Option<HostPosition>,
    /// Reference processing block controlling Constant snap checks.
    pub control_block_frames: u32,
    pub key: u8,
    /// Fractional event tuning, in semitones, before source normalization.
    pub tune_semitones: f64,
    pub velocity: u8,
    pub controllers: [u8; 128],
    /// Signed, normalized MIDI pitch bend, [-1, 1].
    pub pitch_bend: f64,
    pub channel_pressure: f64,
    pub poly_pressure: f64,
    pub time_seconds: f64,
    pub voice_time_seconds: f64,
    pub voice: Option<u32>,
    /// Renderer instance identity; script modulation remains bound to logical voice.
    pub instance: Option<u64>,
    /// Voice age at the actual gate-off, after sustain-pedal handling.
    pub note_off_time_seconds: Option<f64>,
    pub script_values: HashMap<u8, f64>,
}
impl Default for Inputs {
    fn default() -> Self {
        Self {
            sample_rate: 48000.,
            host_tempo: 120.,
            host_position: None,
            control_block_frames: 256,
            key: 60,
            tune_semitones: 0.,
            velocity: 127,
            controllers: [0; 128],
            pitch_bend: 0.,
            channel_pressure: 0.,
            poly_pressure: 0.,
            time_seconds: 0.,
            voice_time_seconds: 0.,
            voice: None,
            instance: None,
            note_off_time_seconds: None,
            script_values: HashMap::new(),
        }
    }
}

// MIDI controller bytes are valid exactly when every high bit is clear.
#[inline]
fn controllers_valid(controllers: &[u8; 128]) -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        use std::arch::x86_64::{_mm_loadu_si128, _mm_movemask_epi8, _mm_or_si128, _mm_setzero_si128};
        // SSE2 is baseline on x86_64. Each load stays inside its 16-byte
        // chunk; unaligned loads require no additional array alignment.
        unsafe {
            let mut bits = _mm_setzero_si128();
            for chunk in controllers.chunks_exact(16) {
                bits = _mm_or_si128(bits, _mm_loadu_si128(chunk.as_ptr().cast()));
            }
            _mm_movemask_epi8(bits) == 0
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        controllers.iter().all(|cc| *cc < 128)
    }
}

#[derive(Debug, Clone)]
enum Source {
    Key,
    KeyFollow,
    LinearKeyFollow,
    Velocity,
    Controller(u8),
    Bend,
    Pressure,
    PolyPressure,
    OrganPan,
    Random(bool),
    Alternate,
    Node(NodeId),
}
#[derive(Debug, Clone)]
struct Connection {
    mode: u32,
    node: NodeId,
    // Bound after all parameter slots exist; named live writes keep slots stable.
    ratio_slot: usize,
    source: Source,
    mapper: Option<NodeId>,
}
#[derive(Debug)]
struct Mapper {
    samples: Vec<f64>,
    min: f64,
    max: f64,
    discrete: bool,
    integer: bool,
}
impl Mapper {
    fn apply(&self, value: f64, bipolar: bool) -> f64 {
        let pos = if bipolar { (value + 1.) * 0.5 } else { value };
        let pos = pos.clamp(0., 1.);
        // Native loader duplicates the final entry. Discrete lookup uses N;
        // interpolated lookup uses N-1. Min/Max scale the output's two halves.
        let raw = if self.discrete {
            self.samples[((pos * self.samples.len() as f64) as usize).min(self.samples.len() - 1)]
        } else {
            let x = pos * (self.samples.len() - 1) as f64;
            let i = x as usize;
            let j = (i + 1).min(self.samples.len() - 1);
            self.samples[i] * (1. - (x - i as f64)) + self.samples[j] * (x - i as f64)
        };
        let value = if raw < 0. {
            raw * -self.min
        } else {
            raw * self.max
        };
        if self.integer {
            (value + 0.5).floor()
        } else {
            value
        }
    }
}
#[derive(Debug, Clone, Copy)]
struct Ramp {
    start: f64,
    target: f64,
    begin: f64,
    duration: f64,
    order: u64,
}
impl Ramp {
    fn value(self, time: f64) -> f64 {
        if self.duration == 0. {
            self.target
        } else {
            self.start
                + (self.target - self.start) * ((time - self.begin) / self.duration).clamp(0., 1.)
        }
    }
}

// Native Constant state belongs to the per-voice control variable. The
// producer writes one point per 32 frames; audio targets interpolate points.
#[derive(Clone)]
struct ConstantClock {
    rate: f64,
    block_frames: u32,
    frame: u64,
    integrated: u64,
    point_frame: u64,
    current: f32,
    point: f32,
    target: f32,
}
impl ConstantClock {
    fn integrate(&mut self, frames: u64) {
        let alpha1 = 1_f32 - 0.33_f32.powf(100_f32 / self.rate as f32);
        let alpha32 = 1_f32 - 0.33_f32.powf(3200_f32 / self.rate as f32);
        if frames == 32 {
            self.current += (self.target - self.current) * alpha32;
        } else {
            for _ in 0..frames {
                self.current += (self.target - self.current) * alpha1;
            }
        }
        self.integrated += frames;
    }
    fn snap(&mut self) {
        let alpha32 = 1_f32 - 0.33_f32.powf(3200_f32 / self.rate as f32);
        if ((self.target - self.current) * alpha32).abs() < 0.0000001 {
            self.current = self.target;
            self.point = self.target;
        }
    }
    fn advance(&mut self, frame: u64, target: f32) -> Result<f64> {
        ensure!(frame >= self.frame, "UVI Constant clock moved backwards");
        // Split only at control boundaries or target changes. In particular,
        // use one f32 alpha32 update for a full tick, not 32 rounded alpha1
        // updates. MIDI events inside a tick leave a partial alpha1 segment.
        let mut steps = 0;
        while self.point_frame <= frame.saturating_sub(32) && frame >= 32 {
            let next = self.point_frame + 32;
            self.integrate(next - self.integrated);
            self.point_frame = next;
            self.point = self.current;
            if next.is_multiple_of(u64::from(self.block_frames)) {
                self.snap();
            }
            steps += 1;
            if steps == 4096 {
                ensure!(
                    self.current == self.target,
                    "UVI Constant clock cannot settle within resource limit"
                );
                self.point = self.current;
                self.point_frame = frame / 32 * 32;
                self.integrated = self.point_frame;
            }
        }
        if target != self.target {
            self.integrate(frame - self.integrated);
            self.target = target;
            self.snap();
        }
        self.frame = frame;
        Ok(f64::from(self.point))
    }
}
#[derive(Clone, Copy)]
struct DahSettings {
    durations: [f32; 4],
    curves: [f64; 3],
    sustain: f64,
    release: f32,
    note_off_retrigger: bool,
    one_shot: bool,
    amplitude: f64,
}
#[derive(Clone)]
struct DahClock {
    rate: f64,
    origin: u64,
    block_frames: u32,
    frame: u64,
    stage: i8,
    remaining: f64,
    elapsed: u64,
    denominator: u64,
    released: bool,
    pending_release: bool,
    release_level: f64,
}
fn control_span(frame: u64, origin: u64, block_frames: u32) -> u64 {
    32.min(u64::from(block_frames) - (origin + frame) % u64::from(block_frames))
}
fn envelope_curve(curve: f64, position: f64) -> f64 {
    let position = position.clamp(0., 1.);
    let curve = curve.clamp(-0.9998, 0.9998);
    if curve == 0. {
        position
    } else {
        let k = 2. * ((1. + curve) / (1. - curve)).ln();
        (k * position).exp_m1() / k.exp_m1()
    }
}
impl DahClock {
    fn next_stage(&mut self, mut carry: f64, settings: DahSettings) {
        loop {
            self.stage += 1;
            if self.stage >= 4 {
                if settings.one_shot {
                    self.stage = 6;
                } else if self.pending_release {
                    self.release_with_budget(settings.sustain, carry, settings);
                }
                return;
            }
            let duration = f64::from(settings.durations[self.stage as usize]);
            if duration.floor() == 0. {
                continue;
            }
            if duration <= carry {
                carry = (carry - duration).max(0.);
                continue;
            }
            // Native stage durations are f32, but fractional overflow is
            // carried in double precision before flooring the next ramp.
            self.remaining = duration - carry;
            self.denominator = self.remaining.floor() as u64;
            self.elapsed = carry.floor() as u64;
            return;
        }
    }
    fn value(&self, settings: DahSettings) -> f64 {
        let position = if self.denominator == 0 {
            1.
        } else {
            self.elapsed as f64 / self.denominator as f64
        };
        match self.stage {
            0 | 6 => 0.,
            1 => envelope_curve(settings.curves[0], position),
            2 => 1.,
            3 => 1. - (1. - settings.sustain) * envelope_curve(settings.curves[1], position),
            4 => settings.sustain,
            5 => self.release_level * (1. - envelope_curve(settings.curves[2], position)),
            _ => 0.,
        }
    }
    fn release(&mut self, level: f64, settings: DahSettings) {
        self.release_with_budget(level, 0., settings);
    }
    fn release_with_budget(&mut self, level: f64, carry: f64, settings: DahSettings) {
        self.pending_release = false;
        self.release_level = level;
        if f64::from(settings.release) <= carry || settings.release.floor() == 0. {
            self.stage = 6;
            return;
        }
        self.remaining = f64::from(settings.release) - carry;
        self.denominator = self.remaining.floor() as u64;
        self.elapsed = carry.floor() as u64;
        self.stage = 5;
    }
    fn step(&mut self, frames: u64, settings: DahSettings) {
        if self.stage < 4 || self.stage == 5 {
            if self.remaining > frames as f64 {
                self.remaining -= frames as f64;
                self.elapsed += frames;
            } else if self.stage == 5 {
                self.stage = 6;
            } else {
                self.next_stage((frames as f64 - self.remaining).max(0.), settings);
            }
        }
        self.frame += frames;
    }
    fn advance(&mut self, frame: u64, off: Option<u64>, settings: DahSettings) -> Result<f64> {
        ensure!(frame >= self.frame, "UVI DAHDSR clock moved backwards");
        let mut steps = 0;
        loop {
            let gate = off.filter(|off| !self.released && *off <= frame);
            if gate.is_some_and(|off| off <= self.frame) {
                self.released = true;
                if settings.one_shot {
                    // AHD has no release stage. The event still splits the
                    // native control segment and recomputes its exact level.
                } else if settings.note_off_retrigger && self.stage < 4 {
                    self.pending_release = true;
                } else {
                    self.release(self.value(settings), settings);
                }
                continue;
            }
            let next = (self.frame + control_span(self.frame, self.origin, self.block_frames))
                .min(gate.unwrap_or(u64::MAX));
            if next > frame {
                break;
            }
            self.step(next - self.frame, settings);
            steps += 1;
            ensure!(steps <= LIMIT, "UVI DAHDSR control-step limit exceeded");
        }
        // Native lookahead remains a full tick even when the audio segment
        // ends at a host block boundary. State advances only the actual span.
        let mut endpoint = self.clone();
        endpoint.step(32, settings);
        let fraction = (frame - self.frame) as f64 / 32.;
        Ok(
            (self.value(settings) * (1. - fraction) + endpoint.value(settings) * fraction)
                * settings.amplitude,
        )
    }
}

#[derive(Clone, Copy)]
struct MultiStep {
    duration: f32,
    level: f32,
    curve: f64,
}
struct MultiSettings<'a> {
    steps: &'a [MultiStep],
    loop_points: Option<(usize, usize)>,
    release: Option<usize>,
}
#[derive(Clone)]
struct MultiClock {
    rate: f64,
    origin: u64,
    block_frames: u32,
    frame: u64,
    index: usize,
    remaining: f64,
    elapsed: u64,
    denominator: u64,
    start: f64,
    target: f64,
    held: bool,
    released: bool,
    kill_frame: Option<u64>,
}
impl MultiClock {
    fn enter(
        &mut self,
        mut index: usize,
        mut carry: f64,
        settings: &MultiSettings<'_>,
    ) -> Result<()> {
        let mut transitions = 0;
        loop {
            transitions += 1;
            ensure!(
                transitions <= LIMIT,
                "UVI MultiEnvelope zero-duration loop exceeds limit"
            );
            if !self.released
                && let Some((begin, end)) = settings.loop_points
                && index > end
            {
                if begin == end {
                    self.held = true;
                    return Ok(());
                }
                index = begin + 1;
            }
            if index >= settings.steps.len() {
                self.held = true;
                let end = self.origin + self.frame;
                self.kill_frame = Some(
                    end.div_ceil(u64::from(self.block_frames)) * u64::from(self.block_frames)
                        - self.origin,
                );
                return Ok(());
            }
            let step = settings.steps[index];
            self.index = index;
            self.start = self.target;
            self.target = f64::from(step.level);
            self.remaining = f64::from(step.duration) - carry;
            self.elapsed = carry.floor() as u64;
            self.denominator = self.remaining.max(0.).floor() as u64;
            if self.remaining >= 1. {
                self.held = false;
                return Ok(());
            }
            carry = (-self.remaining).max(0.);
            index += 1;
        }
    }
    fn value(&self, settings: &MultiSettings<'_>) -> f64 {
        if self.held {
            self.target
        } else {
            let position = if self.denominator == 0 {
                1.
            } else {
                self.elapsed as f64 / self.denominator as f64
            };
            self.start
                + (self.target - self.start)
                    * envelope_curve(settings.steps[self.index].curve, position)
        }
    }
    fn step(&mut self, frames: u64, settings: &MultiSettings<'_>) -> Result<()> {
        self.frame += frames;
        if !self.held {
            self.remaining -= frames as f64;
            self.elapsed += frames;
            if self.remaining <= 0. {
                self.enter(self.index + 1, -self.remaining, settings)?;
            }
        }
        Ok(())
    }
    fn advance(
        &mut self,
        frame: u64,
        off: Option<u64>,
        settings: &MultiSettings<'_>,
    ) -> Result<f64> {
        ensure!(
            frame >= self.frame,
            "UVI MultiEnvelope clock moved backwards"
        );
        let mut ticks = 0;
        loop {
            let gate = off.filter(|off| !self.released && *off <= frame);
            if gate.is_some_and(|off| off <= self.frame) {
                self.released = true;
                if let Some(release) = settings.release {
                    self.target = self.value(settings);
                    self.held = false;
                    self.kill_frame = None;
                    self.enter(release, 0., settings)?;
                }
                continue;
            }
            let next = (self.frame + control_span(self.frame, self.origin, self.block_frames))
                .min(gate.unwrap_or(u64::MAX));
            if next > frame {
                break;
            }
            self.step(next - self.frame, settings)?;
            ticks += 1;
            ensure!(
                ticks <= LIMIT,
                "UVI MultiEnvelope control-step limit exceeded"
            );
        }
        if self.kill_frame.is_some_and(|kill| frame >= kill) {
            return Ok(0.);
        }
        let mut endpoint = self.clone();
        endpoint.step(32, settings)?;
        let fraction = (frame - self.frame) as f64 / 32.;
        Ok(self.value(settings) * (1. - fraction) + endpoint.value(settings) * fraction)
    }
}

struct ControlSegment {
    start: u64,
    end: u64,
    points: Vec<f32>,
}
impl ControlSegment {
    fn value(&self, frame: u64) -> f64 {
        let offset = frame - self.start;
        let index = (offset / 32) as usize;
        let slope = (self.points[index + 1] - self.points[index]) * (1_f32 / 32.);
        f64::from(self.points[index] + (offset % 32) as f32 * slope)
    }
}
#[derive(Clone)]
struct SmoothRandomClock {
    seed: u32,
    rate: f32,
    depth: f32,
    sd: f32,
    cached: Option<f32>,
    first: f32,
    second: f32,
}
impl SmoothRandomClock {
    fn new(seed: u32, rate: f32, depth: f32, voice: bool) -> Self {
        Self {
            seed: ((seed.max(1) as u64 - 1) % 2147483646 + 1) as u32,
            rate,
            depth,
            sd: if voice { 1.0 / 3.0 } else { 1.0 },
            cached: None,
            first: 0.0,
            second: 0.0,
        }
    }
    fn uniform(&mut self) -> f32 {
        self.seed = ((self.seed as u64 * 48271) % 2147483647) as u32;
        (self.seed as f32 - 1.0) / 2147483648.0
    }
    fn gaussian(&mut self) -> f32 {
        if let Some(g) = self.cached.take() {
            return g * self.sd;
        }
        for _ in 0..LIMIT {
            let x = self.uniform() * 2.0 - 1.0;
            let y = self.uniform() * 2.0 - 1.0;
            let r = x * x + y * y;
            if r >= 1.0 || x == 0.0 || y == 0.0 {
                continue;
            }
            let (x, y, r, log_r) = if r <= 1.0e-4 {
                // Range reduction for the polar pair; this rare path is statically
                // grounded, though no authored render has forced it deliberately.
                let exponent = ((x.abs().max(y.abs()).to_bits() >> 23) & 255) as i32 - 127;
                let scale = 2.0_f32.powi(-exponent);
                let x = x * scale;
                let y = y * scale;
                let r = x * x + y * y;
                let log_r = r.ln() + exponent as f32 * 4.0_f32.ln();
                (x, y, r, log_r)
            } else {
                (x, y, r, r.ln())
            };
            let k = (-2.0 * log_r / r).sqrt();
            self.cached = Some(k * y);
            return k * x * self.sd;
        }
        f32::NAN
    }
    fn random_start(&mut self) {
        let y = self.gaussian();
        self.first = y;
        self.second = y;
    }
    fn point(&mut self, n: u32, fs: f32) -> f32 {
        let a = if n == 0 {
            -1.0
        } else {
            -((-(std::f64::consts::TAU) * self.rate as f64) / (fs / n as f32) as f64).exp() as f32
        };
        let q = 1.0 - a * a;
        let g = self.gaussian();
        self.first = q.sqrt() * g - a * self.first;
        self.second = q / (1.0 + a * a).sqrt() * self.first - a * self.second;
        let y = self.second;
        let y2 = y * y;
        let y4 = y2 * y2;
        let y8 = y4 * y4;
        y / (1.0 + y8).sqrt().sqrt().sqrt() * self.depth
    }
    // Raw points at 0,32,...; interpolate consecutive points using j/32.
    // Partial lookahead runs on a COPY and is not committed. Full scope's final
    // endpoint equals its last emitted point; next scope jumps to its new state.
    fn controls(&mut self, n: u32, fs: f32, bipolar: bool) -> Vec<f32> {
        let mut p = Vec::new();
        let mut i = 0;
        while i < n {
            p.push(self.point((n - i).min(32), fs));
            i += 32;
        }
        let mut preview = self.clone();
        p.push(preview.point(n.wrapping_neg() & 31, fs));
        if !bipolar {
            for y in &mut p {
                *y = y.abs();
            }
        } // Native live Bipolar setter oracle proved abs before interpolation.
        p
    }
}
#[derive(Clone)]
struct DrunkClock {
    seed: u32,
    step: f32,
    rate: f32,
    bias: f32,
    bipolar: bool,
    walk: f32,
    y: f32,
    direction: f32,
}
impl DrunkClock {
    fn new(
        seed: u32,
        initial: f32,
        step: f32,
        rate: f32,
        bias: f32,
        bipolar: bool,
        voice: bool,
    ) -> Self {
        let y = if !voice {
            0.0
        } else if bipolar {
            initial
        } else {
            initial.max(0.0)
        };
        Self {
            seed,
            step,
            rate,
            bias,
            bipolar,
            walk: y,
            y,
            direction: 1.0,
        }
    }
    fn controls(&mut self, n: u32, fs: f32) -> Vec<f32> {
        let mut p = Vec::new();
        let mut i = 0;
        while i < n {
            let k = (n - i).min(32);
            p.push(self.y);
            self.seed = self.seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let u = self.seed as f32 * (1.0 / 4294967296.0);
            let mut delta = k as f32 * self.step / fs;
            if u + u + self.bias < 1.0 {
                delta = -delta;
            }
            self.walk += delta * self.direction;
            let low = if self.bipolar { -1.0 } else { 0.0 };
            while self.walk < low || self.walk > 1.0 {
                if self.walk < low {
                    self.walk = low + (self.walk - low).abs();
                    self.direction = -self.direction;
                }
                if self.walk > 1.0 {
                    self.walk = 1.0 - (self.walk - 1.0).abs();
                    self.direction = -self.direction;
                }
            }
            let duration = k.max((fs / self.rate) as u32);
            self.y += k as f32 / duration as f32 * (self.walk - self.y);
            i += k;
        }
        let last = *p.last().unwrap();
        let k = if n & 31 == 0 { 32 } else { n & 31 };
        p.push(last + (self.y - last) * (32.0 / k as f32));
        p
    }
}

#[derive(Clone, Copy, PartialEq)]
struct StochasticSettings {
    rate: f32,
    depth: f32,
    step: f32,
    bias: f32,
    bipolar: bool,
}
enum StochasticSource {
    Smooth(SmoothRandomClock),
    Drunk(DrunkClock),
}
struct StochasticClock {
    sample_rate: f64,
    block_frames: u32,
    origin: u64,
    cursor: u64,
    source: StochasticSource,
    settings: StochasticSettings,
    segment: Option<ControlSegment>,
}
impl StochasticClock {
    fn controls(&mut self, frames: u32, settings: StochasticSettings) -> Vec<f32> {
        self.settings = settings;
        match &mut self.source {
            StochasticSource::Smooth(source) => {
                source.rate = settings.rate;
                source.depth = settings.depth;
                source.controls(frames, self.sample_rate as f32, settings.bipolar)
            }
            StochasticSource::Drunk(source) => {
                source.rate = settings.rate;
                source.step = settings.step;
                source.bias = settings.bias;
                source.bipolar = settings.bipolar;
                source.controls(frames, self.sample_rate as f32)
            }
        }
    }
    fn advance(
        &mut self,
        frame: u64,
        hint: Option<u64>,
        off: Option<u64>,
        settings: StochasticSettings,
    ) -> Result<f64> {
        let block = u64::from(self.block_frames);
        let boundary = |at: u64| (at / block + 1) * block;
        let planned = hint.unwrap_or_else(|| boundary(frame)).min(boundary(frame));
        ensure!(
            planned > frame,
            "Invalid UVI stochastic control segment end"
        );
        if let Some(segment) = &self.segment
            && frame < segment.end
        {
            ensure!(
                frame >= segment.start,
                "UVI stochastic clock moved backwards"
            );
            ensure!(
                planned == segment.end && settings == self.settings,
                "UVI stochastic source changed after processing its segment"
            );
            return Ok(segment.value(frame));
        }
        for _ in 0..LIMIT {
            let mut end = boundary(self.cursor);
            if self.cursor <= frame {
                end = end.min(planned);
            }
            if let Some(off) = off
                && self.cursor < off
            {
                end = end.min(off);
            }
            ensure!(
                end > self.cursor && end - self.cursor <= 65536,
                "Invalid UVI stochastic processing span"
            );
            let points = self.controls((end - self.cursor) as u32, settings);
            ensure!(
                points.iter().all(|point| point.is_finite()),
                "Nonfinite UVI stochastic control points"
            );
            self.segment = Some(ControlSegment {
                start: self.cursor,
                end,
                points,
            });
            self.cursor = end;
            if frame < end {
                return Ok(self.segment.as_ref().unwrap().value(frame));
            }
        }
        bail!("UVI stochastic control-step limit exceeded")
    }
}
struct AttackDecayClock {
    rate: f64,
    origin: u64,
    block_frames: u32,
    attack: f32,
    decay: f32,
    a: f32,
    d: f32,
    qa: f32,
    qd: f32,
    norm: f32,
    done: bool,
    completion: Option<u64>,
    cursor: u64,
    segment: Option<ControlSegment>,
}
impl AttackDecayClock {
    fn new(rate: f64, origin: u64, block_frames: u32, attack: f32, decay: f32) -> Result<Self> {
        let attack_time = decay * attack.clamp(0.01, 0.99);
        let qa = (-3. / (f64::from(attack_time) * rate)).exp() as f32;
        let qd = (-3. / (f64::from(decay) * rate)).exp() as f32;
        let peak = (qa.ln() / qd.ln()).ln() / (qd / qa).ln();
        let norm = 1. / (qd.powf(peak) - qa.powf(peak));
        ensure!(
            norm.is_finite() && norm > 0.,
            "Unrepresentable UVI AttackDecayEnv coefficients"
        );
        let power32 = |mut value: f32| {
            for _ in 0..5 {
                value *= value;
            }
            value
        };
        Ok(Self {
            rate,
            origin,
            block_frames,
            attack,
            decay,
            a: 1.,
            d: 1.,
            qa: power32(qa),
            qd: power32(qd),
            norm,
            done: false,
            completion: None,
            cursor: origin,
            segment: None,
        })
    }
    fn controls(&mut self, frames: u32) -> Vec<f32> {
        let mut points = Vec::with_capacity(frames.div_ceil(32) as usize + 1);
        let mut remaining = frames;
        let mut count = 32;
        while remaining > 0 {
            count = remaining.min(32);
            points.push(if self.done {
                0.
            } else {
                (self.d - self.a) * self.norm
            });
            let (qa, qd) = if count == 32 {
                (self.qa, self.qd)
            } else {
                let fraction = count as f32 / 32.;
                (self.qa.powf(fraction), self.qd.powf(fraction))
            };
            self.a *= qa;
            self.d *= qd;
            remaining -= count;
        }
        let endpoint = if self.done {
            0.
        } else {
            (self.d - self.a) * self.norm
        };
        let last = *points.last().unwrap();
        points.push(last + (32_f32 / count as f32) * (endpoint - last));
        if endpoint < 0.00001 {
            self.done = true;
        }
        points
    }
    fn advance(&mut self, frame: u64, hint: Option<u64>, off: Option<u64>) -> Result<f64> {
        let block_frames = u64::from(self.block_frames);
        let boundary = |at: u64| (at / block_frames + 1) * block_frames;
        let planned = hint.unwrap_or_else(|| boundary(frame)).min(boundary(frame));
        ensure!(planned > frame, "Invalid UVI planned control segment end");
        if let Some(segment) = &self.segment
            && frame < segment.end
        {
            ensure!(
                frame >= segment.start,
                "UVI AttackDecayEnv clock moved backwards"
            );
            ensure!(
                planned == segment.end,
                "UVI AttackDecayEnv segment changed after processing began"
            );
            return Ok(segment.value(frame));
        }
        let mut segments = 0;
        loop {
            let mut end = boundary(self.cursor);
            if self.cursor <= frame {
                end = end.min(planned);
            }
            if let Some(off) = off
                && self.cursor < off
            {
                end = end.min(off);
            }
            ensure!(
                end > self.cursor && end - self.cursor <= 65536,
                "Invalid UVI AttackDecayEnv processing span"
            );
            let points = self.controls((end - self.cursor) as u32);
            if self.done && self.completion.is_none() {
                self.completion = Some(end);
            }
            self.segment = Some(ControlSegment {
                start: self.cursor,
                end,
                points,
            });
            self.cursor = end;
            segments += 1;
            ensure!(
                segments <= LIMIT,
                "UVI AttackDecayEnv control-step limit exceeded"
            );
            if frame < end {
                return Ok(self.segment.as_ref().unwrap().value(frame));
            }
        }
    }
}

#[derive(Clone, Copy)]
struct AnalogSettings {
    attack: f32,
    decay: f32,
    release: f32,
    sustain: f32,
    punch: f32,
    attack_decay: bool,
    amplitude: f64,
}
#[derive(Clone)]
struct AnalogClock {
    rate: f64,
    origin: u64,
    block_frames: u32,
    frame: u64,
    value: f32,
    // 1 attack, 2 decay, 3 sustain, 4 release, 0 finished.
    stage: u8,
    released: bool,
}
impl AnalogClock {
    fn step(&mut self, frames: u64, settings: AnalogSettings) {
        let partial = |alpha: f32| {
            if frames == 32 {
                alpha
            } else {
                1. - (1. - alpha).powf(frames as f32 / 32.)
            }
        };
        match self.stage {
            1 => {
                let peak = 1. + settings.punch;
                self.value += (1.5 * peak - self.value) * partial(settings.attack);
                self.value = self.value.min(peak);
                if self.value >= peak {
                    self.stage = 2;
                }
            }
            2 => {
                self.value *= 1. - partial(settings.decay);
                if settings.attack_decay {
                    if self.value <= 0.00001 {
                        self.value = 0.;
                        self.stage = 0;
                    }
                } else if self.value <= settings.sustain {
                    self.stage = 3;
                }
            }
            3 => {
                // The native producer snaps the decay undershoot at the next
                // processing-block boundary, not at the sustain transition.
                if (self.origin + self.frame + frames).is_multiple_of(u64::from(self.block_frames))
                {
                    self.value = settings.sustain;
                }
            }
            4 => {
                self.value *= 1. - partial(settings.release);
                if self.value <= 0.00001 {
                    self.value = 0.;
                    self.stage = 0;
                }
            }
            _ => {}
        }
        self.frame += frames;
    }
    fn advance(&mut self, frame: u64, off: Option<u64>, settings: AnalogSettings) -> Result<f64> {
        ensure!(
            frame >= self.frame,
            "UVI Analog envelope clock moved backwards"
        );
        let mut steps = 0;
        loop {
            let gate = off.filter(|off| !self.released && !settings.attack_decay && *off <= frame);
            if gate.is_some_and(|off| off <= self.frame) {
                self.released = true;
                if self.stage != 0 {
                    self.stage = 4;
                }
                continue;
            }
            let next = (self.frame + control_span(self.frame, self.origin, self.block_frames))
                .min(gate.unwrap_or(u64::MAX));
            if next > frame {
                break;
            }
            self.step(next - self.frame, settings);
            steps += 1;
            ensure!(
                steps <= LIMIT,
                "UVI Analog envelope control-step limit exceeded"
            );
        }
        let mut endpoint = self.clone();
        endpoint.step(32, settings);
        // A sustain snap at a block boundary is a step, not an interpolated
        // 32-frame ramp. Attack/decay/release array points interpolate.
        let end = if self.stage == 3 && endpoint.stage == 3 {
            self.value
        } else {
            endpoint.value
        };
        let fraction = (frame - self.frame) as f64 / 32.;
        Ok(
            (f64::from(self.value.min(1.)) * (1. - fraction) + f64::from(end.min(1.)) * fraction)
                * settings.amplitude,
        )
    }
}

#[derive(Clone, Debug)]
struct Wave6 {
    pub phase: u32,
    pub previous_phase: u32,
    pub random: f32,
    pub smooth_previous: f32,
    pub previous_step: u32,
    pub reset_pending: bool,
    pub phase_parameter: f32,
}
impl Default for Wave6 {
    fn default() -> Self {
        Self {
            phase: 0,
            previous_phase: 0,
            random: 0.0,
            smooth_previous: 0.0,
            previous_step: 32,
            reset_pending: false,
            phase_parameter: 0.0,
        }
    }
}
fn random_value(seed: &mut u32) -> f32 {
    *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    ((*seed as f32) * (1.0f32 / 4_294_967_296.0)) * 2.0 - 1.0
}
impl Wave6 {
    // Native note callback consumes one RNG value even when legato suppresses reset.
    // Caller: reset=true for retrigger1; retrigger2 only on first held note.
    fn note(&mut self, seed: &mut u32, reset: bool) {
        if reset {
            self.reset_pending = true;
        }
        self.random = random_value(seed);
    }
    // The returned last endpoint is a lookahead value, not committed filter state.
    fn controls(
        &mut self,
        seed: &mut u32,
        frequency: f32,
        rate: f32,
        smooth_seconds: f32,
        frames: u32,
    ) -> Vec<f32> {
        if self.reset_pending {
            self.phase = ((self.phase_parameter * 4_294_967_296.0) as u64) as u32;
            self.previous_phase = self.phase;
            self.smooth_previous = 0.0;
            self.previous_step = 32;
        }
        let increment = ((16_777_216.0f32 / rate) * (frequency * 256.0)) as u32;
        let mut raw = Vec::with_capacity(frames.div_ceil(32) as usize + 1);
        let mut steps = Vec::with_capacity(frames.div_ceil(32) as usize);
        let mut remaining = frames;
        while remaining > 0 {
            let step = remaining.min(32);
            if increment != 0 {
                for _ in 0..step {
                    if self.phase <= self.previous_phase {
                        self.random = random_value(seed);
                    }
                    self.previous_phase = self.phase;
                    self.phase = self.phase.wrapping_add(increment);
                }
            }
            if self.reset_pending {
                self.smooth_previous = self.random;
                self.reset_pending = false;
            }
            raw.push(self.random);
            steps.push(step);
            remaining -= step;
        }
        raw.push(self.random); // Native random final endpoint duplicates last raw value.
        if smooth_seconds > 0.0 {
            smooth_controls(
                &mut raw,
                &steps,
                smooth_seconds,
                rate,
                &mut self.smooth_previous,
                &mut self.previous_step,
            );
        }
        raw
    }
}
// Also usable on deterministic wave0/9 raw control arrays, whose final endpoint
// is evaluated at the next phase rather than duplicated like waveform6.
fn smooth_controls(
    values: &mut [f32],
    steps: &[u32],
    seconds: f32,
    rate: f32,
    previous: &mut f32,
    previous_step: &mut u32,
) {
    let q = (1.0f32 / 3.0).powf(1.0f32 / (seconds * rate));
    let mut retention32 = q;
    for _ in 0..5 {
        retention32 *= retention32;
    }
    for (x, &step) in values.iter_mut().zip(steps) {
        let retention = if *previous_step == 32 {
            retention32
        } else {
            q.powf(*previous_step as f32)
        };
        *previous = *x + (*previous - *x) * retention;
        *x = *previous;
        *previous_step = step;
    }
    if let Some(last) = values.last_mut() {
        // Native lookahead uses powf even when the final step is32.
        *last = *last + (*previous - *last) * q.powf(*previous_step as f32);
    }
}

struct RandomLfoClock {
    unit: Wave6,
    block: u64,
    block_start: u64,
    controls: Vec<f32>,
    rate: f64,
    block_frames: u32,
}
struct LfoClock {
    time: f64,
    frequency: f64,
    phase: f64,
}
struct TriangleLfoClock {
    wave: f64,
    amplitude: f32,
    bipolar: bool,
    frame: u64,
    phase: u32,
    increment: u32,
    phase_parameter: f32,
    origin: u64,
    delay_frames: u64,
    rise_frames: u64,
    rate: f64,
    block_frames: u32,
}
fn triangle_lfo_value(phase: u32) -> f32 {
    // Original table constructor stores i/64 quarter segments. Preserve the
    // source generator's two weighted float32 products and final addition.
    let point = |index: u32| {
        let position = index as f32 * (1_f32 / 64.);
        if index < 64 { position }
        else if index < 192 { 2. - position }
        else { position - 4. }
    };
    let index = phase >> 24;
    let fraction = (phase & 0x00ff_ffff) as f32 * (1_f32 / 16777216.);
    point(index) * (1. - fraction) + point(index + 1) * fraction
}
// Native built-in square uses a 256-entry +/-1 table, interpolates
// index127 to128, and holds the final entry until uint32 phase wraps.
fn square_lfo_value(phase: u32) -> f32 {
    let index = phase >> 24;
    if index < 127 {
        1.
    } else if index == 127 {
        1. - 2. * ((phase & 0x00ff_ffff) as f32 * (1_f32 / 16777216.))
    } else {
        -1.
    }
}
struct AbsoluteClock {
    producer: ConstantClock,
    filtered: HashMap<NodeId, f32>,
    published: HashMap<NodeId, f32>,
}
struct CachedParameter {
    key: Parameter,
    law: TargetLaw,
    number: Option<f64>,
    override_value: Option<f64>,
    slot: usize,
    edges: Vec<Connection>,
}
// Only immutable target dispatch is compiled. Values, source clocks, connection
// order and the evaluation pass remain owned by the existing paths below.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TargetLaw {
    Unverified,
    Add,
    Factor,
    MatrixFactor,
    AnalogTime,
    LfoFrequency,
    FilterFrequency,
    Unit,
    Drive,
    DelayTime,
    Boolean,
    ChorusSpeed,
    ChorusCrossover,
    ChorusDepth,
    GainScale,
}
impl TargetLaw {
    fn new(kind: &str, name: &str) -> Self {
        if !supports_target(kind, name) && !wavetable_control(kind, name) {
            return Self::Unverified;
        }
        if (matches!(name, "Gain" | "Volume" | "Ratio" | "Depth") && kind != "WhiteChorus")
            || (kind == "GainMatrix" && name.starts_with("Gain_"))
            || (kind == "DAHDSR" && matches!(name, "AttackTime" | "DecayTime"))
        {
            return if kind == "GainMatrix" { Self::MatrixFactor } else { Self::Factor };
        }
        match (kind, name) {
            ("AnalogADSR", "AttackTime" | "DecayTime" | "ReleaseTime") => Self::AnalogTime,
            ("LFO", "Freq") => Self::LfoFrequency,
            ("OnePole" | "XpanderFilter", "Freq") => Self::FilterFrequency,
            ("XpanderFilter", "Q" | "Fat") | ("DualDelay", "Feedback")
            | ("WhiteChorus", "Mix") => Self::Unit,
            ("XpanderFilter", "Drive") => Self::Drive,
            ("DAHDSR", "DelayTime") => Self::DelayTime,
            ("XpanderFilter", "Bypass") => Self::Boolean,
            ("WhiteChorus", "Speed") => Self::ChorusSpeed,
            ("WhiteChorus", "Crossover") => Self::ChorusCrossover,
            ("WhiteChorus", "Depth") => Self::ChorusDepth,
            ("DigitalEq", "GainScale") => Self::GainScale,
            _ if wavetable_control(kind, name) => Self::Unit,
            _ => Self::Add,
        }
    }
}
/// One bounded slot per compiled numeric parameter. An epoch invalidates all
/// values without clearing or reallocating the buffer between voices/frames.
struct MemoScratch {
    epoch: u64,
    values: Vec<f64>,
    stamps: Vec<u64>,
    sources: FxHashMap<NodeId, (f64, bool)>,
}
impl MemoScratch {
    fn new(slots: usize) -> Self {
        Self {
            epoch: 1,
            values: vec![0.; slots],
            stamps: vec![0; slots],
            sources: FxHashMap::default(),
        }
    }
    fn begin(&mut self) {
        self.sources.clear();
        self.epoch = self.epoch.wrapping_add(1);
        if self.epoch == 0 {
            self.stamps.fill(0);
            self.epoch = 1;
        }
    }
    fn get(&self, slot: usize) -> Option<f64> {
        (self.stamps[slot] == self.epoch).then_some(self.values[slot])
    }
    fn insert(&mut self, slot: usize, value: f64) {
        self.values[slot] = value;
        self.stamps[slot] = self.epoch;
    }
}
#[derive(Clone, Copy)]
enum Overrides<'a> {
    External(&'a HashMap<Parameter, f64>),
    Registered,
}
// This caches the existing admitted fixed/global Step projection, not a native
// host-generation policy. Tables are immutable for this graph owner. Every live
// scalar gate still runs before querying the prepared points.
#[derive(Clone, Copy, PartialEq, Eq)]
struct StepProjectionKey {
    start: u64,
    block_start: u64,
    beat: u64,
    rate: u64,
    tempo: u64,
    frequency: u32,
    count: usize,
    position: Option<(u64, u64, bool)>,
}
struct StepProjection {
    key: StepProjectionKey,
    phases: [f64; 9],
    points: [f32; 9],
}
pub struct ModulationGraph {
    parents: Vec<Option<NodeId>>,
    kinds: Vec<String>,
    bases: Vec<BTreeMap<String, String>>,
    cached_parameters: Vec<FxHashMap<String, usize>>,
    parameter_values: Vec<CachedParameter>,
    node_target_slots: Vec<Vec<usize>>,
    connections: HashMap<Parameter, Vec<Connection>>,
    node_targets: Vec<Vec<Parameter>>,
    memo: RefCell<MemoScratch>,
    absolute_order: Vec<NodeId>,
    absolute_clocks: HashMap<NodeId, AbsoluteClock>,
    absolute_audio_bases: HashMap<Parameter, f64>,
    absolute_seen_values: HashMap<NodeId, f32>,
    absolute_block: Option<u64>,
    target_sources: HashMap<Parameter, HashSet<NodeId>>,
    mappers: HashMap<NodeId, Mapper>,
    tables: HashMap<NodeId, Vec<f64>>,
    step_settings: RefCell<FxHashMap<NodeId, (f64, f64, f32, u32)>>,
    step_projection: RefCell<FxHashMap<NodeId, StepProjection>>,
    ramps: HashMap<(u8, Option<NodeId>, Option<u32>), Ramp>,
    script_ranges: HashMap<u8, bool>,
    event_order: u64,
    scoped_ramps: bool,
    analog_clocks: RefCell<FxHashMap<SourceStateKey, AnalogClock>>,
    dah_clocks: RefCell<FxHashMap<SourceStateKey, DahClock>>,
    multi_clocks: RefCell<FxHashMap<SourceStateKey, MultiClock>>,
    multi_steps: HashMap<NodeId, Vec<NodeId>>,
    attack_decay_clocks: RefCell<FxHashMap<SourceStateKey, AttackDecayClock>>,
    stochastic_clocks: RefCell<FxHashMap<SourceStateKey, StochasticClock>>,
    global_stochastic: Vec<NodeId>,
    builtin_seeds: RefCell<[u32; 2]>,
    alternate_next: RefCell<f32>,
    builtin_values: RefCell<FxHashMap<BuiltinStateKey, f32>>,
    builtin_targets: HashMap<NodeId, u8>,
    control_segment_end: Option<u64>,
    random_seeds: RefCell<FxHashMap<NodeId, u32>>,
    random_lfo_clocks: RefCell<FxHashMap<SourceStateKey, RandomLfoClock>>,
    lfo_clocks: RefCell<FxHashMap<SourceStateKey, LfoClock>>,
    triangle_lfo_clocks: RefCell<FxHashMap<SourceStateKey, TriangleLfoClock>>,
    constant_clocks: RefCell<FxHashMap<SourceStateKey, ConstantClock>>,
}
fn number(attrs: &BTreeMap<String, String>, name: &str, default: f64) -> Result<f64> {
    let value = attrs
        .get(name)
        .map(|v| v.parse::<f64>())
        .transpose()
        .with_context(|| format!("Invalid UVI modulation attribute {name}"))?
        .unwrap_or(default);
    ensure!(
        value.is_finite(),
        "Nonfinite UVI modulation attribute {name}"
    );
    Ok(value)
}
fn flag(attrs: &BTreeMap<String, String>, name: &str, default: bool) -> Result<bool> {
    let value = number(attrs, name, f64::from(default))?;
    ensure!(
        value == 0. || value == 1.,
        "Invalid UVI modulation Boolean {name}"
    );
    Ok(value == 1.)
}
fn table(text: &str) -> Result<Vec<f64>> {
    let mut values = Vec::new();
    for v in text.split_whitespace() {
        ensure!(values.len() < 65536, "UVI modulation table exceeds limit");
        let v = v
            .replace(',', ".")
            .parse::<f64>()
            .context("Malformed UVI modulation table")?;
        ensure!(
            v.is_finite() && v.abs() <= f64::from(f32::MAX),
            "Out-of-range UVI modulation table entry"
        );
        values.push(v);
    }
    ensure!(
        values.len() >= 2,
        "UVI modulation table requires at least two values"
    );
    Ok(values)
}
fn scope(program: &Program, id: NodeId) -> Option<NodeId> {
    let mut p = program.nodes[id].parent;
    while let Some(id) = p {
        if matches!(
            program.nodes[id].kind.as_str(),
            "Program" | "Layer" | "Keygroup" | "AuxEffect" | "EffectRack"
        ) {
            return Some(id);
        }
        p = program.nodes[id].parent;
    }
    None
}
fn mapper_path(
    program: &Program,
    owner: NodeId,
    path: &str,
    paths: &PathIndex<'_>,
    mappers: &HashMap<NodeId, HashMap<&str, Option<NodeId>>>,
) -> Result<NodeId> {
    if path.contains('/') || path.starts_with('$') {
        return paths.resolve(owner, path);
    }
    let mut at = if matches!(
        program.nodes[owner].kind.as_str(),
        "Program" | "Layer" | "Keygroup" | "AuxEffect" | "EffectRack"
    ) {
        Some(owner)
    } else {
        scope(program, owner)
    };
    while let Some(id) = at {
        if let Some(target) = mappers.get(&id).and_then(|names| names.get(path)) {
            return target.context("Ambiguous UVI mapper in scope");
        }
        at = scope(program, id);
    }
    bail!("Unresolved UVI mapper reference")
}
// A measured endpoint converter is not sufficient to admit a live route:
// its controller CURRENT/FUTURE points must be prepared before conversion.
fn wavetable_control(kind: &str, name: &str) -> bool {
    kind == "WaveTableOscillator" && matches!(name, "PhaseDistortionAmount" | "WaveIndex")
}
pub fn supports_target(kind: &str, name: &str) -> bool {
    matches!(
        (kind, name),
        (
            "SamplePlayer" | "MinBlepGenerator" | "WaveTableOscillator" | "FmOscillator",
            "Pitch" | "Gain"
        ) | ("Program" | "Layer" | "Keygroup", "Gain")
            | ("Gain", "Volume")
            | ("SignalConnection", "Ratio")
            | ("LFO", "Depth" | "Phase" | "Freq")
            | ("ConstantModulation", "Value")
            | ("OnePole", "Freq")
            | ("DigitalEq", "GainScale")
            | ("DAHDSR", "AttackTime" | "DecayTime" | "DelayTime")
            | ("DualDelay", "Feedback" | "Mix")
            | ("DualDelayX", "Mix")
            | ("WhiteChorus", "Mix" | "Speed" | "Depth" | "Crossover")
            | ("XpanderFilter", "Freq" | "Q" | "Fat" | "Drive" | "Bypass")
            | ("AnalogADSR", "AttackTime" | "DecayTime" | "ReleaseTime")
    ) || (kind == "GainMatrix"
        && name.strip_prefix("Gain_").is_some_and(|rest| {
            let Some((input, output)) = rest.split_once('_') else {
                return false;
            };
            [input, output]
                .iter()
                .all(|s| s.parse::<usize>().is_ok_and(|n| (1..=12).contains(&n)))
        }))
}
/// Mode1 sets a normalized parameter through its own converter. Mode0's
/// physical-unit arithmetic does not establish this absolute conversion.
pub fn supports_absolute_target(kind: &str, name: &str) -> bool {
    matches!(
        (kind, name),
        ("ConstantModulation", "Value")
            | ("Gain", "Volume")
            | ("OnePole", "Freq")
            | ("XpanderFilter", "Freq" | "Q" | "Drive")
            | ("WhiteChorus", "Mix" | "Speed" | "Depth" | "Crossover")
            | ("DualDelay", "Mix")
            | ("Layer", "Mute" | "Gain")
            | ("SamplePlayer", "Gain")
            | ("SparkVerb", "Mix")
            | ("WaveShaper", "Mix" | "Knee")
            | ("AnalogADSR", "DecayTime")
            | ("CrossOverFilter", "LowFrequency" | "HighFrequency")
            | ("ThreeBandShelves", "GainLow" | "GainHigh")
            | (
                "Phasor" | "CrossPhaser" | "PhasorFilter" | "Tremolo",
                "Depth"
            )
            | ("SignalConnection", "Ratio")
            | (
                "Gain"
                    | "OnePole"
                    | "XpanderFilter"
                    | "WhiteChorus"
                    | "DualDelay"
                    | "GainMatrix"
                    | "SparkVerb"
                    | "WaveShaper"
                    | "CrossOverFilter"
                    | "ThreeBandShelves"
                    | "Phasor"
                    | "CrossPhaser"
                    | "PhasorFilter"
                    | "Tremolo"
                    | "Redux"
                    | "Redux2"
                    | "AuxEffect"
                    | "ScriptProcessor",
                "Bypass"
            )
    ) || (kind == "GainMatrix" && supports_target(kind, name))
}
fn absolute_value(kind: &str, name: &str, normalized: f32) -> Result<f64> {
    let normalized = normalized.clamp(0., 1.);
    let value = match (kind, name) {
        ("ConstantModulation", "Value")
        | ("DualDelay" | "WhiteChorus" | "SparkVerb" | "WaveShaper", "Mix")
        | ("XpanderFilter", "Q")
        | ("SignalConnection", "Ratio")
        | ("Phasor" | "CrossPhaser" | "PhasorFilter" | "Tremolo", "Depth") => normalized,
        ("OnePole" | "XpanderFilter", "Freq")
        | ("CrossOverFilter", "LowFrequency" | "HighFrequency") => {
            20_f32 * 1000_f32.powf(normalized)
        }
        ("ThreeBandShelves", "GainLow" | "GainHigh") => 48. * normalized - 24.,
        ("AnalogADSR", "DecayTime") => {
            // Native offset-log registration spans .0001..10 seconds.
            (0.0011_f64 * (10.001_f64 / 0.0011).powf(f64::from(normalized))) as f32 - 0.001
        }
        ("XpanderFilter", "Drive") => 40. * normalized - 20.,
        ("WaveShaper", "Knee") => 20. * normalized - 10.,
        ("WhiteChorus", "Speed") => 0.1_f32 * 10_f32.powf(normalized),
        ("WhiteChorus", "Depth") => 1. + 39. * normalized,
        ("WhiteChorus", "Crossover") => 20_f32 * 250_f32.powf(normalized),
        ("Layer", "Mute")
        | (
            "Gain" | "OnePole" | "XpanderFilter" | "WhiteChorus" | "DualDelay" | "GainMatrix"
            | "SparkVerb" | "WaveShaper" | "CrossOverFilter" | "ThreeBandShelves" | "Phasor"
            | "CrossPhaser" | "PhasorFilter" | "Tremolo" | "Redux" | "Redux2" | "AuxEffect"
            | "ScriptProcessor",
            "Bypass",
        ) => f32::from(normalized >= 0.5),
        ("GainMatrix", name) if supports_target(kind, name) => 2. * normalized - 1.,
        ("Gain", "Volume") | ("Layer" | "SamplePlayer", "Gain") => {
            // Native registration anchors physical unity at normalized .8
            // for insert/oscillator gain and .7 for Layer gain. Amplitude
            // endpoints are +12/+6 dB respectively. These semantic anchors
            // determine the power curve, without a fitted exponent.
            let db = if kind == "Gain" { 12_f32 } else { 6_f32 };
            let anchor = if kind == "Layer" { 0.7_f32 } else { 0.8_f32 };
            let maximum = (db * (10_f32.ln() / 20.)).exp();
            let exponent = (1_f32 / maximum).ln() / anchor.ln();
            normalized.powf(exponent) * maximum
        }
        _ => bail!("Unverified UVI Mode1 target conversion for {kind}.{name}"),
    };
    Ok(f64::from(value))
}

impl ModulationGraph {
    /// Absolute end of the native processing segment, known before its first
    /// sample. A requested output length is not a host processing boundary.
    pub fn set_control_segment_end_frame(&mut self, end: Option<u64>) {
        self.control_segment_end = end;
    }
    pub fn requires_planned_segments(&self) -> bool {
        self.connections.values().flatten().any(|connection|
            matches!(connection.source,Source::Node(node) if matches!(self.kinds[node].as_str(),"AttackDecayEnv"|"StdRandom"|"Drunk")))
    }
    pub fn new(program: &Program) -> Result<Self> {
        ensure!(
            program.nodes.len() <= LIMIT && program.connections.len() <= LIMIT,
            "UVI modulation graph exceeds limit"
        );
        let paths = PathIndex::new(program);
        let mut mapper_names: HashMap<NodeId, HashMap<&str, Option<NodeId>>> = HashMap::new();
        for (id, node) in program.nodes.iter().enumerate() {
            if node.kind == "ControlSignalMapper"
                && let (Some(owner), Some(name)) = (scope(program, id), node.name.as_deref())
            {
                mapper_names.entry(owner).or_default().entry(name)
                    .and_modify(|target| *target = None).or_insert(Some(id));
            }
        }
        let mut cached_parameters: Vec<BTreeMap<String, CachedParameter>> = program
            .nodes
            .iter()
            .enumerate()
            .map(|(id, node)| {
                node.attributes
                    .iter()
                    .filter_map(|(name, text)| {
                        text.parse::<f64>().ok().map(|number| {
                            (
                                name.clone(),
                                CachedParameter {
                                    key: (id, name.clone()),
                                    law: TargetLaw::new(&node.kind, name),
                                    number: number.is_finite().then_some(number),
                                    override_value: None,
                                    slot: 0,
                                    edges: Vec::new(),
                                },
                            )
                        })
                    })
                    .collect()
            })
            .collect();
        let mut graph = Self {
            parents: program.nodes.iter().map(|node| node.parent).collect(),
            kinds: program.nodes.iter().map(|n| n.kind.clone()).collect(),
            bases: program.nodes.iter().map(|n| n.attributes.clone()).collect(),
            cached_parameters: vec![FxHashMap::default(); program.nodes.len()],
            parameter_values: Vec::new(),
            node_target_slots: vec![Vec::new(); program.nodes.len()],
            connections: HashMap::new(),
            node_targets: vec![Vec::new(); program.nodes.len()],
            memo: RefCell::new(MemoScratch::new(0)),
            absolute_order: Vec::new(),
            absolute_clocks: HashMap::new(),
            absolute_audio_bases: HashMap::new(),
            absolute_seen_values: HashMap::new(),
            absolute_block: None,
            target_sources: HashMap::new(),
            mappers: HashMap::new(),
            tables: HashMap::new(),
            step_settings: RefCell::new(FxHashMap::default()),
            step_projection: RefCell::new(FxHashMap::default()),
            ramps: HashMap::new(),
            script_ranges: HashMap::new(),
            event_order: 0,
            scoped_ramps: false,
            analog_clocks: RefCell::new(FxHashMap::default()),
            dah_clocks: RefCell::new(FxHashMap::default()),
            multi_clocks: RefCell::new(FxHashMap::default()),
            multi_steps: HashMap::new(),
            attack_decay_clocks: RefCell::new(FxHashMap::default()),
            stochastic_clocks: RefCell::new(FxHashMap::default()),
            global_stochastic: Vec::new(),
            builtin_seeds: RefCell::new([1; 2]),
            alternate_next: RefCell::new(1.),
            builtin_values: RefCell::new(FxHashMap::default()),
            builtin_targets: HashMap::new(),
            control_segment_end: None,
            random_seeds: RefCell::new(FxHashMap::default()),
            random_lfo_clocks: RefCell::new(FxHashMap::default()),
            lfo_clocks: RefCell::new(FxHashMap::default()),
            triangle_lfo_clocks: RefCell::new(FxHashMap::default()),
            constant_clocks: RefCell::new(FxHashMap::default()),
        };
        for (id, n) in program.nodes.iter().enumerate() {
            match n.kind.as_str() {
                "StepEnvelope" => {
                    let steps = number(&n.attributes, "NumSteps", 16.)?;
                    ensure!((1. ..=128.).contains(&steps) && steps.fract() == 0., "Invalid UVI StepEnvelope step count at node {id}");
                    let levels = n.attributes.get("Levels").context("UVI StepEnvelope Levels are missing")?;
                    ensure!(levels.split_whitespace().count() <= 128, "UVI StepEnvelope Levels exceed limit");
                    let values: Vec<f64> = levels.split_whitespace().map(|v| v.replace(',', ".").parse()).collect::<std::result::Result<_, _>>()?;
                    ensure!(values.len() >= steps as usize && values.len() <= 128 && values.iter().all(|v| (-1. ..=1.).contains(v)), "Invalid UVI StepEnvelope Levels at node {id}");
                    for (name, expected) in [("SyncToHost", 1.), ("Retrigger", 0.), ("InterpolationMode", 0.), ("Smooth", 0.), ("Bipolar", 0.), ("Depth", 1.), ("ManualTrigger", 0.), ("Bypass", 0.)] {
                        let observed = number(&n.attributes, name, if name == "Depth" { 1. } else { 0. })?;
                        source_setting_gate(id, "StepEnvelope", name, observed, observed == expected)?;
                    }
                    graph.tables.insert(id, values);
                }
                "MultiEnvelope" => {
                    let container = program
                        .nodes
                        .iter()
                        .position(|child| child.parent == Some(id) && child.kind == "Steps")
                        .context("UVI MultiEnvelope has no Steps")?;
                    let steps: Vec<_> = program
                        .nodes
                        .iter()
                        .enumerate()
                        .filter_map(|(node, child)| {
                            (child.parent == Some(container) && child.kind == "Step")
                                .then_some(node)
                        })
                        .collect();
                    ensure!(
                        !steps.is_empty() && steps.len() <= 65536,
                        "Invalid UVI MultiEnvelope step count"
                    );
                    graph.multi_steps.insert(id, steps);
                }
                "ControlSignalMapper" => {
                    let min = number(&n.attributes, "Min", 0.)?;
                    let max = number(&n.attributes, "Max", 1.)?;
                    ensure!(
                        min <= 0. && max >= 0.,
                        "Unsupported UVI mapper output range"
                    );
                    graph.mappers.insert(
                        id,
                        Mapper {
                            samples: table(&n.text)?,
                            min,
                            max,
                            discrete: flag(&n.attributes, "Discrete", false)?,
                            integer: flag(&n.attributes, "Integer", false)?,
                        },
                    );
                }
                "UserTable" => {
                    if let Some(parent) = n.parent {
                        let samples = table(&n.text)?;
                        ensure!(
                            program.nodes[parent].kind != "LFO" || samples.len() == 256,
                            "Native UVI LFO UserTable must contain 256 entries"
                        );
                        graph.tables.insert(parent, samples);
                    }
                }
                "ScriptEventModulation" => {
                    let event = number(&n.attributes, "EventId", 0.)?;
                    ensure!(
                        (0. ..=127.).contains(&event) && event.fract() == 0.,
                        "Invalid script modulation EventId"
                    );
                    let bipolar = flag(&n.attributes, "Bipolar", true)?;
                    if let Some(old) = graph.script_ranges.insert(event as u8, bipolar) {
                        ensure!(
                            old == bipolar,
                            "Conflicting script modulation polarities for one EventId"
                        );
                    }
                }
                _ => {}
            }
        }
        for c in &program.connections {
            ensure!(
                c.mode <= 1,
                "Unsupported UVI ConnectionMode {} at node {}",
                c.mode,
                c.node
            );
            for (key, value) in &program.nodes[c.node].attributes {
                ensure!(
                    key != "Bipolar",
                    "Unverified UVI connection polarity override at node {}",
                    c.node
                );
                if key.contains("Transform") {
                    ensure!(
                        value == "0" || value.is_empty(),
                        "Unsupported UVI connection transformation at node {}",
                        c.node
                    );
                }
            }
            let source = match c.source.as_str() {
                "@VoiceParam Key" => Source::Key,
                "@VoiceParam KeyFollow" => Source::KeyFollow,
                "@VoiceParam LinearKeyFollow" => Source::LinearKeyFollow,
                "@VoiceParam Velocity" => Source::Velocity,
                "@PitchBend" => Source::Bend,
                "@ChanAfterTouch" => Source::Pressure,
                "@OrganPan" | "@Random" | "@UnipolarRandom" | "@Alternate" => {
                    let mut owner = Some(c.owner);
                    while owner.is_some_and(|node| program.nodes[node].kind != "Keygroup") {
                        owner = owner.and_then(|node| program.nodes[node].parent);
                    }
                    ensure!(
                        owner.is_some(),
                        "Unverified UVI note source outside Keygroup voice context"
                    );
                    if c.source == "@OrganPan" {
                        Source::OrganPan
                    } else if c.source == "@Alternate" {
                        *graph.builtin_targets.entry(c.owner).or_default() |= 4;
                        Source::Alternate
                    } else {
                        let bipolar = c.source == "@Random";
                        *graph.builtin_targets.entry(c.owner).or_default() |=
                            1 << u8::from(bipolar);
                        Source::Random(bipolar)
                    }
                }
                "@PolyAfterTouch" => Source::PolyPressure,
                s if s.starts_with("@MIDI CC ") => {
                    let cc = s[9..].parse::<u8>().context("Invalid UVI MIDI CC source")?;
                    ensure!(cc < 128, "Invalid UVI MIDI CC source");
                    Source::Controller(cc)
                }
                s if s.starts_with('@') => {
                    bail!("Unsupported UVI control source at node {}", c.node)
                }
                s => {
                    let id = paths.resolve(c.owner, s)?;
                    if !matches!(
                        program.nodes[id].kind.as_str(),
                        "ConstantModulation"
                            | "ScriptEventModulation"
                            | "LFO"
                            | "StepEnvelope"
                            | "AnalogADSR"
                            | "DAHDSR"
                            | "AHD"
                            | "MultiEnvelope"
                            | "AttackDecayEnv"
                            | "StdRandom"
                            | "Drunk"
                    ) {
                        return Err(UnsupportedSourceKind { node: id }.into());
                    }
                    if program.nodes[id].kind == "LFO" {
                        let attributes = &program.nodes[id].attributes;
                        let wave = number(attributes, "WaveFormType", 0.)?;
                        source_setting_gate(id, "LFO", "WaveFormType", wave,
                            [0., 1., 2., 6., 9.].contains(&wave))?;
                        if wave == 1. {
                            let retrigger = number(attributes, "Retrigger", 1.)?;
                            source_setting_gate(id, "LFO", "Retrigger", retrigger, retrigger == 1.)?;
                        }
                        let smooth = number(attributes, "Smooth", 0.)?;
                        ensure!(
                            (0. ..=1.).contains(&smooth),
                            "Invalid UVI LFO smoothing at node {id}"
                        );
                        source_setting_gate(id, "LFO", "Smooth", smooth, wave == 6. || smooth == 0.)?;
                        if wave == 9. {
                            ensure!(
                                graph.tables.contains_key(&id),
                                "UVI user LFO table is missing at node {id}"
                            );
                        }
                    }
                    Source::Node(id)
                }
            };
            let mapper = if c.mapper.is_empty() {
                None
            } else {
                Some(mapper_path(program, c.owner, &c.mapper, &paths, &mapper_names)?)
            };
            ensure!(
                mapper.is_none_or(|id| graph.mappers.contains_key(&id)),
                "UVI Mapper reference has wrong node kind"
            );
            if c.mode == 1 {
                ensure!(
                    number(
                        &program.nodes[c.node].attributes,
                        "SignalConnectionVersion",
                        0.
                    )? == 1.,
                    "Unverified legacy UVI Mode1 connection at node {}",
                    c.node
                );
                ensure!(
                    matches!(source, Source::Node(id) if program.nodes[id].kind == "ConstantModulation"),
                    "Unverified UVI Mode1 producer at node {}",
                    c.node
                );
                if let Some(mapper) = mapper {
                    ensure!(
                        graph.mappers[&mapper].min == 0. && graph.mappers[&mapper].max == 1.,
                        "Unverified UVI Mode1 mapper range at node {}",
                        c.node
                    );
                }
            }
            graph
                .connections
                .entry((c.owner, c.destination.clone()))
                .or_default()
                .push(Connection {
                    mode: c.mode,
                    node: c.node,
                    ratio_slot: 0, // Bound before the completed graph is returned.
                    source,
                    mapper,
                });
        }
        for p in graph.connections.keys() {
            graph.node_targets[p.0].push(p.clone());
        }
        // Validate the full graph, including currently bypassed connections:
        // a script can enable those, so bypass cannot hide a dependency cycle.
        let mut finished = HashSet::new();
        let mut active = HashSet::new();
        for (node, name) in graph.connections.keys() {
            ensure!(
                graph.kinds[*node] != "LFO"
                    || number(&graph.bases[*node], "WaveFormType", 0.)? != 1.
                    || !matches!(
                        name.as_str(),
                        "Freq" | "Depth" | "Phase" | "DelayTime" | "RiseTime"
                    ),
                "Unverified connected UVI square LFO parameter {name} at node {node}"
            );
        }
        for p in graph.connections.keys() {
            graph.visit(p, &mut finished, &mut active, 0)?;
        }
        for p in graph.connections.keys() {
            let mut sources = HashSet::new();
            graph.collect_sources(p, &mut HashSet::new(), &mut sources);
            graph.target_sources.insert(p.clone(), sources);
        }
        let mut absolute_sources = graph
            .connections
            .values()
            .flatten()
            .filter_map(|connection| match (connection.mode, &connection.source) {
                (1, Source::Node(node)) => Some(*node),
                _ => None,
            })
            .collect::<Vec<_>>();
        absolute_sources.extend(graph.connections.iter().filter_map(|(parameter, edges)| {
            (graph.kinds[parameter.0] == "ConstantModulation"
                && parameter.1 == "Value"
                && edges.iter().any(|edge| edge.mode == 1))
            .then_some(parameter.0)
        }));
        // Native processes declared global sources in collection order. A
        // receiver earlier than its sender has already produced this block.
        absolute_sources.sort_unstable();
        absolute_sources.dedup();
        graph.absolute_order = absolute_sources;
        graph.global_stochastic = graph
            .target_sources
            .values()
            .flatten()
            .copied()
            .filter(|node| {
                matches!(graph.kinds[*node].as_str(), "StdRandom" | "Drunk")
                    && scope(program, *node).is_some_and(|owner| {
                        matches!(graph.kinds[owner].as_str(), "Program" | "Layer")
                    })
            })
            .collect();
        graph.global_stochastic.sort_unstable();
        graph.global_stochastic.dedup();
        let mut keys = Vec::new();
        for parameter in graph.connections.keys() {
            keys.push(parameter.clone());
            keys.extend(graph.dependencies(parameter));
        }
        for connection in graph.connections.values().flatten() {
            for name in ["Ratio", "Bypass", "Inverted"] {
                keys.push((connection.node, name.into()));
            }
        }
        for node in graph
            .target_sources
            .values()
            .flatten()
            .copied()
            .collect::<HashSet<_>>()
        {
            for name in [
                "Bypass",
                "Bipolar",
                "Retrigger",
                "NoteOffRetrigger",
                "Smooth",
                "SyncToHost",
                "WaveFormType",
                "VelocityAmount",
                "VelocitySens",
                "TriggerMode",
                "RandomStart",
                "InitialValue",
                "LoopStart",
                "LoopEnd",
                "ReleaseStep",
                "NumSteps",
                "Style",
                "AttackCurve",
                "DecayCurve",
                "ReleaseCurve",
                "ManualTrigger",
                "InvertVelocity",
                "KeyToAttack",
                "VelToAttack",
                "KeyToDecay",
                "VelToDecay",
                "Punch",
                "AttackDecayMode",
                "DynamicRange",
            ] {
                keys.push((node, name.into()));
            }
        }
        for key in keys {
            cached_parameters[key.0]
                .entry(key.1.clone())
                .or_insert(CachedParameter {
                    law: TargetLaw::new(&graph.kinds[key.0], &key.1),
                    key,
                    number: None,
                    override_value: None,
                    slot: 0,
                    edges: Vec::new(),
                });
        }
        // Named writes and recursive source settings retain the name index.
        // Compiled targets borrow records by the memo's stable slot.
        for (node, parameters) in cached_parameters.into_iter().enumerate() {
            for (name, mut cached) in parameters {
                let slot = graph.parameter_values.len();
                cached.slot = slot;
                graph.cached_parameters[node].insert(name, slot);
                graph.parameter_values.push(cached);
            }
        }
        // Every connection's Ratio key was included above, even if XML omitted
        // the default. Resolve it once instead of hashing its name per edge/frame.
        for connection in graph.connections.values_mut().flatten() {
            connection.ratio_slot = graph.cached_parameters[connection.node]["Ratio"];
        }
        for cached in &mut graph.parameter_values {
            cached.edges = graph.connections.get(&cached.key).cloned().unwrap_or_default();
        }
        for (node, targets) in graph.node_targets.iter().enumerate() {
            graph.node_target_slots[node] = targets
                .iter()
                .map(|p| graph.cached_parameters[p.0][&p.1])
                .collect();
        }
        graph.memo = RefCell::new(MemoScratch::new(graph.parameter_values.len()));
        Ok(graph)
    }
    pub fn is_absolute_source_parameter(&self, node: NodeId, name: &str) -> bool {
        name == "Value" && self.absolute_order.contains(&node)
    }
    fn collect_sources(
        &self,
        p: &Parameter,
        visited: &mut HashSet<Parameter>,
        sources: &mut HashSet<NodeId>,
    ) {
        if !visited.insert(p.clone()) {
            return;
        }
        for connection in self.connections.get(p).into_iter().flatten() {
            if let Source::Node(node) = connection.source {
                sources.insert(node);
            }
        }
        for dependency in self.dependencies(p) {
            self.collect_sources(&dependency, visited, sources);
        }
    }
    /// Array-producing native sources, including those reached through Ratio routes.
    pub fn target_has_dynamic_source(&self, target: &Parameter) -> bool {
        self.target_sources.get(target).is_some_and(|sources| {
            sources.iter().any(|node| {
                matches!(
                    self.kinds[*node].as_str(),
                    "LFO"
                        | "StepEnvelope"
                        | "DAHDSR"
                        | "AHD"
                        | "AnalogADSR"
                        | "MultiEnvelope"
                        | "AttackDecayEnv"
                        | "StdRandom"
                        | "Drunk"
                )
            })
        })
    }
    fn dependencies(&self, p: &Parameter) -> Vec<Parameter> {
        let mut deps = Vec::new();
        for c in self.connections.get(p).into_iter().flatten() {
            deps.push((c.node, "Ratio".into()));
            deps.push((c.node, "Bypass".into()));
            if let Source::Node(n) = c.source {
                for name in match self.kinds[n].as_str() {
                    "ConstantModulation" => &["Value", "Bypass"][..],
                    "ScriptEventModulation" => &["EventId", "Bypass"][..],
                    "LFO" => &["Freq", "Depth", "Phase", "DelayTime", "RiseTime", "Bypass"][..],
                    "AttackDecayEnv" => &["Attack", "DecayTime", "Bypass"][..],
                    "StdRandom" => &["Rate", "Depth", "Bypass"][..],
                    "Drunk" => &["Rate", "Step", "Bias", "Bypass"][..],
                    "MultiEnvelope" => &["Speed", "Bypass"][..],
                    "StepEnvelope" => &["Freq", "Depth", "Smooth", "ManualTrigger", "Bypass"][..],
                    "AHD" => &["AttackTime", "HoldTime", "DecayTime", "Bypass"][..],
                    "DAHDSR" => &[
                        "DelayTime",
                        "AttackTime",
                        "HoldTime",
                        "DecayTime",
                        "SustainLevel",
                        "ReleaseTime",
                        "Bypass",
                    ][..],
                    "AnalogADSR" => &[
                        "AttackTime",
                        "DecayTime",
                        "SustainLevel",
                        "ReleaseTime",
                        "Bypass",
                    ][..],
                    _ => &[],
                } {
                    deps.push((n, (*name).into()));
                }
            }
        }
        deps
    }
    fn visit(
        &self,
        p: &Parameter,
        finished: &mut HashSet<Parameter>,
        active: &mut HashSet<Parameter>,
        depth: usize,
    ) -> Result<()> {
        ensure!(
            depth < DEPTH,
            "UVI modulation dependency depth exceeds limit"
        );
        if finished.contains(p) {
            return Ok(());
        }
        ensure!(
            active.insert(p.clone()),
            "Cyclic UVI modulation dependency at node {} parameter {}",
            p.0,
            p.1
        );
        for d in self.dependencies(p) {
            self.visit(&d, finished, active, depth + 1)?;
        }
        active.remove(p);
        finished.insert(p.clone());
        Ok(())
    }
    pub fn set_script_modulation(
        &mut self,
        id: u8,
        start: Option<f64>,
        target: f64,
        ramp_ms: f64,
        voice: Option<u32>,
        time_seconds: f64,
    ) -> Result<()> {
        self.set_script_modulation_scoped(None, id, start, target, ramp_ms, voice, time_seconds)
    }
    /// Layer emissions address descendant receiver sources. A Program-owned
    /// source remains global even when a Layer voice references it.
    #[allow(clippy::too_many_arguments)]
    pub fn set_script_modulation_scoped(
        &mut self,
        layer: Option<NodeId>,
        id: u8,
        start: Option<f64>,
        target: f64,
        ramp_ms: f64,
        voice: Option<u32>,
        time_seconds: f64,
    ) -> Result<()> {
        ensure!(
            layer.is_none_or(|node| self.kinds.get(node).is_some_and(|kind| kind == "Layer")),
            "Invalid UVI script modulation issuing Layer"
        );
        ensure!(id < 128, "Invalid UVI script modulation EventId");
        // Native accepts every API EventId, including signals with no current
        // ScriptEventModulation listener. Keep their explicit voice identity.
        let bipolar = self.script_ranges.get(&id).copied().unwrap_or(true);
        let low = if bipolar { -1. } else { 0. };
        ensure!(
            target.is_finite()
                && (low..=1.).contains(&target)
                && ramp_ms.is_finite()
                && ramp_ms >= 0.
                && time_seconds.is_finite()
                && time_seconds >= 0.,
            "Invalid UVI script modulation event"
        );
        let previous = self
            .ramp(id, voice, layer)
            .map(|r| r.value(time_seconds))
            .unwrap_or(0.);
        let start = start.unwrap_or(previous);
        ensure!(
            start.is_finite() && (low..=1.).contains(&start),
            "Invalid UVI script modulation start"
        );
        ensure!(
            self.ramps.len() < LIMIT || self.ramps.contains_key(&(id, layer, voice)),
            "UVI script modulation state exceeds limit"
        );
        self.event_order = self
            .event_order
            .checked_add(1)
            .context("UVI modulation event sequence exhausted")?;
        self.scoped_ramps |= layer.is_some();
        self.ramps.insert(
            (id, layer, voice),
            Ramp {
                start,
                target,
                begin: time_seconds,
                duration: ramp_ms * 0.001,
                order: self.event_order,
            },
        );
        Ok(())
    }
    fn ramp(&self, id: u8, voice: Option<u32>, mut receiver: Option<NodeId>) -> Option<&Ramp> {
        let mut selected = [
            self.ramps.get(&(id, None, voice)),
            self.ramps.get(&(id, None, None)),
        ]
        .into_iter()
        .flatten()
        .max_by_key(|ramp| ramp.order);
        if !self.scoped_ramps {
            return selected;
        }
        while let Some(node) = receiver {
            if self.kinds[node] == "Layer" {
                for candidate in [
                    self.ramps.get(&(id, Some(node), voice)),
                    self.ramps.get(&(id, Some(node), None)),
                ]
                .into_iter()
                .flatten()
                {
                    if selected.is_none_or(|ramp| candidate.order > ramp.order) {
                        selected = Some(candidate);
                    }
                }
            }
            receiver = self.parents[node];
        }
        selected
    }
    /// Retire one renderer instance without removing shared script ramps.
    pub fn remove_instance(&mut self, voice: u32, instance: u64) {
        let retained = |key: &SourceStateKey| key.1 != Some(voice) || key.2 != Some(instance);
        self.builtin_values
            .get_mut()
            .retain(|(_, v, i), _| *v != Some(voice) || *i != Some(instance));
        self.analog_clocks.get_mut().retain(|key, _| retained(key));
        self.dah_clocks.get_mut().retain(|key, _| retained(key));
        self.multi_clocks.get_mut().retain(|key, _| retained(key));
        self.attack_decay_clocks
            .get_mut()
            .retain(|key, _| retained(key));
        self.stochastic_clocks
            .get_mut()
            .retain(|key, _| retained(key));
        self.constant_clocks
            .get_mut()
            .retain(|key, _| retained(key));
        self.lfo_clocks.get_mut().retain(|key, _| retained(key));
        self.triangle_lfo_clocks
            .get_mut()
            .retain(|key, _| retained(key));
        self.random_lfo_clocks
            .get_mut()
            .retain(|key, _| retained(key));
    }
    pub fn remove_voice(&mut self, voice: u32) {
        self.ramps.retain(|(_, _, v), _| *v != Some(voice));
        self.builtin_values
            .get_mut()
            .retain(|(_, v, _), _| *v != Some(voice));
        self.analog_clocks
            .get_mut()
            .retain(|(_, v, _), _| *v != Some(voice));
        self.dah_clocks
            .get_mut()
            .retain(|(_, v, _), _| *v != Some(voice));
        self.multi_clocks
            .get_mut()
            .retain(|(_, v, _), _| *v != Some(voice));
        self.attack_decay_clocks
            .get_mut()
            .retain(|(_, v, _), _| *v != Some(voice));
        self.stochastic_clocks
            .get_mut()
            .retain(|(_, v, _), _| *v != Some(voice));
        self.constant_clocks
            .get_mut()
            .retain(|(_, v, _), _| *v != Some(voice));
        self.lfo_clocks
            .get_mut()
            .retain(|(_, v, _), _| *v != Some(voice));
        self.triangle_lfo_clocks
            .get_mut()
            .retain(|(_, v, _), _| *v != Some(voice));
        self.random_lfo_clocks
            .get_mut()
            .retain(|(_, v, _), _| *v != Some(voice));
    }
    /// Every serialized target requiring an as-yet unverified conversion,
    /// including currently bypassed routes that scripts may enable later.
    pub fn unsupported_targets(&self) -> Vec<Parameter> {
        let mut targets = self
            .connections
            .keys()
            .filter(|p| {
                self.connections[*p].iter().any(|c| {
                    if c.mode == 1 {
                        !supports_absolute_target(&self.kinds[p.0], &p.1)
                    } else {
                        !supports_target(&self.kinds[p.0], &p.1)
                    }
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        targets.sort();
        targets
    }
    /// Publish native absolute routes once per processing block. A source's
    /// first one-pole feeds a connection one-pole; its final actual 32-frame
    /// point is committed before destination audio for that same block.
    /// Serialized source values do not cause an initial parameter write.
    pub fn control_updates(
        &mut self,
        input: &Inputs,
        live: &HashMap<Parameter, f64>,
    ) -> Result<Vec<(Parameter, f64)>> {
        let external = live;
        let live = Overrides::External(live);
        if !self.global_stochastic.is_empty() {
            self.validate(input, live)?;
            let mut memo = self.memo.borrow_mut();
            memo.begin();
            // Program/Layer free-running sources process silent segments too.
            // Otherwise earlier musical cuts would lose their RNG draws.
            for &node in &self.global_stochastic {
                if self.setting(node, "TriggerMode", 1., live)? == 0. {
                    self.source(&Source::Node(node), input, live, &mut memo, 0)?;
                }
            }
        }
        if self.absolute_order.is_empty() {
            return Ok(Vec::new());
        }
        self.validate(input, live)?;
        ensure!(
            input.voice.is_none(),
            "UVI absolute updates require Program context"
        );
        let frame = (input.time_seconds * input.sample_rate + 0.000001).floor() as u64;
        let block = u64::from(input.control_block_frames);
        if frame.is_multiple_of(block) && self.absolute_block == Some(frame) {
            return Ok(Vec::new());
        }
        let mut current_live = HashMap::new();
        let mut updates = Vec::new();
        // The declaration order is immutable after construction. Copy one ID
        // at a time instead of allocating a new order for every audio frame.
        for index in 0..self.absolute_order.len() {
            let node = self.absolute_order[index];
            ensure!(
                !self
                    .connections
                    .get(&(node, "Value".into()))
                    .is_some_and(|c| c.iter().any(|c| c.mode == 0)),
                "Unverified UVI Mode1 producer Value modulation at node {node}"
            );
            ensure!(
                !self.boolean(node, "Bypass", false, live)?,
                "Unverified bypassed UVI Mode1 producer at node {node}"
            );
            let style = self.setting(node, "Style", 0., live)?;
            ensure!(
                style == 0. || style == 1.,
                "Unverified UVI Mode1 producer Style"
            );
            let parameter = (node, "Value".into());
            let target = current_live
                .get(&parameter)
                .copied()
                .unwrap_or(self.base(&parameter, live)?)
                .clamp(0., 1.);
            let target = if style == 1. {
                f32::from(target > 0.5)
            } else {
                target as f32
            };
            if !frame.is_multiple_of(block) {
                let initial = number(&self.bases[node], "Value", 0.)?.clamp(0., 1.) as f32;
                ensure!(
                    self.absolute_seen_values
                        .get(&node)
                        .copied()
                        .unwrap_or(initial)
                        == target,
                    "Unverified nonaligned UVI Mode1 producer change at node {node}"
                );
                continue;
            }
            let parameter = (node, "Value".into());
            if self
                .connections
                .get(&parameter)
                .is_some_and(|edges| edges.iter().any(|edge| edge.mode == 1))
            {
                self.absolute_audio_bases
                    .insert(parameter, f64::from(target));
            }
            let mut edges = self
                .connections
                .iter()
                .flat_map(|(p, connections)| {
                    connections
                        .iter()
                        .filter(move |c| {
                            c.mode == 1 && matches!(c.source, Source::Node(n) if n == node)
                        })
                        .map(move |c| (p.clone(), c))
                })
                .collect::<Vec<_>>();
            edges.sort_by_key(|(_, c)| c.node);
            if edges.is_empty() {
                continue;
            }
            for (_, edge) in &edges {
                for name in ["Ratio", "Offset", "Inverted", "Bypass"] {
                    if let Some(value) = external.get(&(edge.node, name.into())) {
                        ensure!(
                            *value
                                == number(
                                    &self.bases[edge.node],
                                    name,
                                    f64::from(name == "Ratio")
                                )?,
                            "Unverified live UVI Mode1 connection control {name} at node {}",
                            edge.node
                        );
                    }
                }
            }
            let initial = number(&self.bases[node], "Value", 0.)?.clamp(0., 1.);
            let initial = if style == 1. {
                f32::from(initial > 0.5)
            } else {
                initial as f32
            };
            let clock = self
                .absolute_clocks
                .entry(node)
                .or_insert_with(|| AbsoluteClock {
                    producer: ConstantClock {
                        rate: input.sample_rate,
                        block_frames: input.control_block_frames,
                        frame: 0,
                        integrated: 0,
                        point_frame: 0,
                        current: initial,
                        point: initial,
                        target: initial,
                    },
                    filtered: edges.iter().map(|(_, edge)| (edge.node, initial)).collect(),
                    published: edges.iter().map(|(_, edge)| (edge.node, initial)).collect(),
                });
            ensure!(
                clock.producer.rate == input.sample_rate
                    && clock.producer.block_frames == input.control_block_frames,
                "UVI Mode1 processing clock changed"
            );
            if clock.producer.frame > frame {
                continue;
            } // Already previewed this block.
            let first = if frame == 0 {
                0
            } else {
                clock.producer.point_frame + 32
            };
            let end = frame + block - 32;
            ensure!(
                end >= first && (end - first) / 32 <= LIMIT as u64,
                "UVI Mode1 control clock exceeds limit"
            );
            let alpha = 1_f32 - 0.33_f32.powf(3200_f32 / input.sample_rate as f32);
            for at in (first..=end).step_by(32) {
                let value = clock.producer.advance(
                    at,
                    if at < frame {
                        clock.producer.target
                    } else {
                        target
                    },
                )? as f32;
                for (_, edge) in &edges {
                    let filtered = clock.filtered.get_mut(&edge.node).expect("compiled edge");
                    *filtered += (value - *filtered) * alpha;
                    if at.is_multiple_of(block) && ((value - *filtered) * alpha).abs() < 0.0000001 {
                        *filtered = value;
                    }
                }
            }
            for (parameter, edge) in edges {
                if flag(&self.bases[edge.node], "Bypass", false)? {
                    continue;
                }
                let value = clock.filtered[&edge.node];
                if clock.published[&edge.node] == value {
                    continue;
                }
                clock.published.insert(edge.node, value);
                let ratio = number(&self.bases[edge.node], "Ratio", 1.)?;
                let inverted = flag(&self.bases[edge.node], "Inverted", false)?;
                let mut mapped = if inverted {
                    1. - f64::from(value)
                } else {
                    f64::from(value)
                };
                if let Some(mapper) = edge.mapper {
                    mapped = self.mappers[&mapper].apply(mapped, false);
                }
                if ratio < 0. {
                    mapped = 1. - mapped;
                }
                let normalized =
                    number(&self.bases[edge.node], "Offset", 0.)? + ratio.abs() * mapped;
                let value =
                    absolute_value(&self.kinds[parameter.0], &parameter.1, normalized as f32)?;
                current_live.insert(parameter.clone(), value);
                updates.push((parameter, value));
            }
        }
        if frame.is_multiple_of(block) {
            for node in &self.absolute_order {
                let parameter = (*node, "Value".into());
                let value = current_live
                    .get(&parameter)
                    .copied()
                    .unwrap_or(self.base(&parameter, live)?)
                    .clamp(0., 1.);
                let style = self.setting(*node, "Style", 0., live)?;
                self.absolute_seen_values.insert(
                    *node,
                    if style == 1. {
                        f32::from(value > 0.5)
                    } else {
                        value as f32
                    },
                );
            }
            self.absolute_block = Some(frame);
        }
        Ok(updates)
    }
    /// Evaluate native parameter values. Unverified audio-target conversions
    /// fail explicitly; use `deltas` to inspect their graph contributions.
    pub fn evaluate(
        &self,
        input: &Inputs,
        live: &HashMap<Parameter, f64>,
    ) -> Result<HashMap<Parameter, f64>> {
        let live = Overrides::External(live);
        self.validate(input, live)?;
        let mut memo = self.memo.borrow_mut();
        memo.begin();
        for p in self.connections.keys() {
            self.value(p, input, live, &mut memo, 0)?;
        }
        Ok(self
            .connections
            .keys()
            .map(|p| {
                let slot = self.cached_parameters[p.0][&p.1];
                (p.clone(), memo.get(slot).expect("evaluated target"))
            })
            .collect())
    }
    /// Evaluate selected target nodes, recursively retaining their complete
    /// source and nested Ratio dependencies. Used per voice by the renderer.
    pub fn evaluate_nodes(
        &self,
        input: &Inputs,
        live: &HashMap<Parameter, f64>,
        nodes: &HashSet<NodeId>,
    ) -> Result<HashMap<Parameter, f64>> {
        let mut result = HashMap::new();
        self.evaluate_nodes_emit(input, Overrides::External(live), nodes, |p, value| {
            result.insert(p.clone(), value);
        })?;
        Ok(result)
    }
    /// Reuse the graph's compiled memo and emit borrowed target keys. Nothing
    /// is emitted on an evaluation error; callers may reuse their output slots.
    pub fn evaluate_nodes_into(
        &mut self,
        input: &Inputs,
        live: &HashMap<Parameter, f64>,
        nodes: &HashSet<NodeId>,
        emit: impl FnMut(&Parameter, f64),
    ) -> Result<()> {
        self.evaluate_nodes_emit(input, Overrides::External(live), nodes, emit)
    }
    /// Renderer-owned numeric writes are validated before touching stored state.
    pub(crate) fn update_live_parameter(
        &mut self,
        node: NodeId,
        name: &str,
        value: f64,
    ) -> Result<()> {
        ensure!(
            node < self.bases.len() && value.is_finite(),
            "Invalid UVI live parameter override"
        );
        if let Some(&slot) = self.cached_parameters[node].get(name) {
            self.parameter_values[slot].override_value = Some(value);
        } else {
            let memo = self.memo.get_mut();
            let slot = memo.values.len();
            memo.values.push(0.);
            memo.stamps.push(0);
            self.parameter_values.push(CachedParameter {
                key: (node, name.into()),
                law: TargetLaw::new(&self.kinds[node], name),
                number: None,
                override_value: Some(value),
                slot,
                edges: Vec::new(),
            });
            self.cached_parameters[node].insert(name.into(), slot);
        }
        if self.kinds[node] == "StepEnvelope" {
            self.step_projection.get_mut().remove(&node);
        }
        Ok(())
    }
    pub(crate) fn evaluate_registered_nodes_into(
        &mut self,
        input: &Inputs,
        nodes: &HashSet<NodeId>,
        emit: impl FnMut(&Parameter, f64),
    ) -> Result<()> {
        self.evaluate_nodes_emit(input, Overrides::Registered, nodes, emit)
    }
    pub(crate) fn release_registered_finished(
        &self,
        input: &Inputs,
        nodes: &HashSet<NodeId>,
    ) -> Result<bool> {
        ensure!(
            nodes.iter().all(|node| *node < self.bases.len()),
            "Invalid UVI modulation target node"
        );
        self.release_nodes_finished(input, Overrides::Registered, nodes)
    }
    fn evaluate_nodes_emit(
        &self,
        input: &Inputs,
        live: Overrides<'_>,
        nodes: &HashSet<NodeId>,
        mut emit: impl FnMut(&Parameter, f64),
    ) -> Result<()> {
        self.validate(input, live)?;
        ensure!(
            nodes.iter().all(|id| *id < self.bases.len()),
            "Invalid UVI modulation target node"
        );
        let mut memo = self.memo.borrow_mut();
        memo.begin();
        // Native note initialization draws builtin values for bypassed routes
        // too. Preserve source order before processing this voice's targets.
        for (node, mask) in &self.builtin_targets {
            if nodes.contains(node) {
                for bipolar in [false, true] {
                    if mask & (1 << u8::from(bipolar)) != 0 {
                        self.builtin_random(input, bipolar)?;
                    }
                }
                if mask & 4 != 0 {
                    self.builtin_alternate(input)?;
                }
            }
        }
        for node in nodes {
            for &slot in &self.node_target_slots[*node] {
                let cached = &self.parameter_values[slot];
                self.value_cached(&cached.key, Some(cached), input, live, &mut memo, 0)?;
            }
        }
        for node in nodes {
            for &slot in &self.node_target_slots[*node] {
                let cached = &self.parameter_values[slot];
                emit(&cached.key, memo.get(slot).expect("evaluated target"));
            }
        }
        Ok(())
    }
    /// Inspect routed ratio-times-source sums before target conversion.
    /// These are control-domain signals, not physical-unit parameter deltas.
    pub fn deltas(
        &self,
        input: &Inputs,
        live: &HashMap<Parameter, f64>,
    ) -> Result<HashMap<Parameter, f64>> {
        let live = Overrides::External(live);
        self.validate(input, live)?;
        let mut memo = self.memo.borrow_mut();
        memo.begin();
        let mut result = HashMap::new();
        for p in self.connections.keys() {
            result.insert(p.clone(), self.delta(p, input, live, &mut memo, 0)?);
        }
        Ok(result)
    }
    fn validate(&self, input: &Inputs, live: Overrides<'_>) -> Result<()> {
        ensure!(
            input.key < 128
                && input.tune_semitones.is_finite()
                && input.tune_semitones.abs() <= f64::from(f32::MAX)
                && input.velocity < 128
                && controllers_valid(&input.controllers),
            "Invalid UVI MIDI modulation inputs"
        );
        ensure!(
            (-1. ..=1.).contains(&input.pitch_bend)
                && (0. ..=1.).contains(&input.channel_pressure)
                && (0. ..=1.).contains(&input.poly_pressure),
            "Invalid UVI pressure/bend modulation inputs"
        );
        ensure!(
            input.sample_rate.is_finite()
                && input.host_tempo.is_finite()
                && input.host_tempo >= 0.
                && input.host_tempo <= f64::from(f32::MAX)
                && input.host_position.is_none_or(|position| position.beat.is_finite())
                && (1000. ..=768000.).contains(&input.sample_rate)
                && (32..=65536).contains(&input.control_block_frames)
                && input.control_block_frames.is_multiple_of(32)
                && input.time_seconds.is_finite()
                && input.time_seconds >= 0.
                && input.time_seconds * input.sample_rate < (u64::MAX - 65536) as f64
                && input.voice_time_seconds.is_finite()
                && input.voice_time_seconds >= 0.
                && input
                    .note_off_time_seconds
                    .is_none_or(|t| t.is_finite() && t >= 0. && t <= input.voice_time_seconds),
            "Invalid UVI modulation clock"
        );
        if let Overrides::External(live) = live {
            ensure!(
                live.iter()
                    .all(|((n, _), v)| *n < self.bases.len() && v.is_finite()),
                "Invalid UVI live parameter override"
            );
        }
        Ok(())
    }
    fn cached_parameter(&self, node: NodeId, name: &str) -> Option<&CachedParameter> {
        self.cached_parameters[node]
            .get(name)
            .map(|&slot| &self.parameter_values[slot])
    }
    fn setting(&self, node: NodeId, name: &str, default: f64, live: Overrides<'_>) -> Result<f64> {
        if let Some(cached) = self.cached_parameter(node, name) {
            let override_value = match live {
                Overrides::External(live) => live.get(&cached.key).copied(),
                Overrides::Registered => cached.override_value,
            };
            if let Some(value) = override_value {
                return Ok(value);
            }
            if let Some(value) = cached.number {
                return Ok(value);
            }
        } else if let Overrides::External(live) = live {
            if let Some((_, value)) = live
                .iter()
                .find(|((id, parameter), _)| *id == node && parameter == name)
            {
                // Public live overrides may introduce a field absent from XML.
                return Ok(*value);
            }
        }
        number(&self.bases[node], name, default)
    }
    fn boolean(
        &self,
        node: NodeId,
        name: &str,
        default: bool,
        live: Overrides<'_>,
    ) -> Result<bool> {
        let value = self.setting(node, name, f64::from(default), live)?;
        ensure!(
            value == 0. || value == 1.,
            "Invalid live UVI modulation Boolean {name}"
        );
        Ok(value == 1.)
    }
    fn base(&self, p: &Parameter, live: Overrides<'_>) -> Result<f64> {
        self.base_cached(
            p,
            self.cached_parameters
                .get(p.0)
                .and_then(|node| node.get(&p.1))
                .map(|&slot| &self.parameter_values[slot]),
            live,
        )
    }
    fn base_cached(
        &self,
        p: &Parameter,
        cached: Option<&CachedParameter>,
        live: Overrides<'_>,
    ) -> Result<f64> {
        let override_value = match live {
            Overrides::External(live) => live.get(p).copied(),
            Overrides::Registered => cached.and_then(|cached| cached.override_value),
        };
        if let Some(value) = override_value {
            return Ok(value);
        }
        if let Some(value) = cached.and_then(|cached| cached.number) {
            return Ok(value);
        }
        let default = match (self.kinds[p.0].as_str(), p.1.as_str()) {
            (
                "SamplePlayer"
                | "MinBlepGenerator"
                | "WaveTableOscillator"
                | "FmOscillator"
                | "Program"
                | "Layer"
                | "Keygroup",
                "Gain",
            )
            | ("Gain", "Volume")
            | ("DigitalEq", "GainScale")
            | ("SignalConnection", "Ratio")
            | ("LFO" | "StepEnvelope", "Depth") => 1.,
            ("LFO", "Freq") => 0.5,
            ("StepEnvelope", "Freq") => 1.,
            ("MultiEnvelope", "Speed") => 1.,
            ("OnePole" | "XpanderFilter", "Freq") => 1000.,
            ("XpanderFilter", "Fat") => 1.,
            ("DualDelay", "Feedback") => 0.3,
            ("DualDelay" | "DualDelayX", "Mix") => 0.5,
            ("WhiteChorus", "Mix") => 1.,
            ("WhiteChorus", "Speed") => 0.2,
            ("WhiteChorus", "Depth") => 5.,
            ("WhiteChorus", "Crossover") => 20.,
            ("AnalogADSR", "AttackTime") => 0.001,
            ("AnalogADSR", "DecayTime") => 0.05,
            ("AnalogADSR", "ReleaseTime") => 0.01,
            ("AnalogADSR" | "DAHDSR", "SustainLevel") => 1.,
            ("DAHDSR", "ReleaseTime") => 0.05,
            ("AHD", "HoldTime") => 1.,
            ("AHD", "DecayTime") => 0.1,
            ("StdRandom", "Rate" | "Depth") => 1.,
            ("Drunk", "Rate") => 100.,
            ("Drunk", "Step") => 4.,
            ("AttackDecayEnv", "Attack") => 0.1,
            ("AttackDecayEnv", "DecayTime") => 0.3,
            ("GainMatrix", name) if name.starts_with("Gain_") => {
                let parts = name[5..].split('_').collect::<Vec<_>>();
                f64::from(parts.len() == 2 && parts[0] == parts[1])
            }
            _ => 0.,
        };
        number(&self.bases[p.0], &p.1, default)
    }
    fn value_named(
        &self,
        node: NodeId,
        name: &str,
        input: &Inputs,
        live: Overrides<'_>,
        memo: &mut MemoScratch,
        depth: usize,
    ) -> Result<f64> {
        if let Some(cached) = self.cached_parameter(node, name) {
            self.value_cached(&cached.key, Some(cached), input, live, memo, depth)
        } else {
            self.value(&(node, name.into()), input, live, memo, depth)
        }
    }
    fn value(
        &self,
        p: &Parameter,
        input: &Inputs,
        live: Overrides<'_>,
        memo: &mut MemoScratch,
        depth: usize,
    ) -> Result<f64> {
        ensure!(
            depth < DEPTH,
            "UVI modulation evaluation depth exceeds limit"
        );
        self.value_cached(
            p,
            self.cached_parameter(p.0, &p.1),
            input,
            live,
            memo,
            depth,
        )
    }
    fn value_cached(
        &self,
        p: &Parameter,
        cached: Option<&CachedParameter>,
        input: &Inputs,
        live: Overrides<'_>,
        memo: &mut MemoScratch,
        depth: usize,
    ) -> Result<f64> {
        ensure!(
            depth < DEPTH,
            "UVI modulation evaluation depth exceeds limit"
        );
        if let Some(value) = cached.and_then(|cached| memo.get(cached.slot)) {
            return Ok(value);
        }
        let edges = cached.map_or(&[][..], |cached| cached.edges.as_slice());
        let relative = edges.iter().any(|c| c.mode == 0);
        let law = cached.map_or(TargetLaw::Unverified, |cached| cached.law);
        ensure!(
            !relative || law != TargetLaw::Unverified,
            "Unverified UVI modulation target conversion at node {} parameter {}",
            p.0,
            p.1
        );
        let base = self
            .absolute_audio_bases
            .get(p)
            .copied()
            .unwrap_or(self.base_cached(p, cached, live)?);
        let value = if !relative {
            base
        } else {
            match law {
                TargetLaw::Factor | TargetLaw::MatrixFactor => {
                    let mut factor = 1.;
                    for c in edges {
                        if c.mode != 0 {
                            continue;
                        }
                        if self.boolean(c.node, "Bypass", false, live)? {
                            continue;
                        }
                        let cached_ratio = &self.parameter_values[c.ratio_slot];
                        let ratio = self.value_cached(&cached_ratio.key, Some(cached_ratio), input, live, memo, depth + 1)?;
                        let (mut source, bipolar) = self.source(&c.source, input, live, memo, depth + 1)?;
                        if self.boolean(c.node, "Inverted", false, live)? {
                            source = if bipolar { -source } else { 1. - source };
                        }
                        if let Some(id) = c.mapper {
                            source = self.mappers[&id].apply(source, bipolar);
                        }
                        let source = if bipolar { (source + 1.) * 0.5 } else { source };
                        let ratio = ratio.clamp(-1., 1.);
                        factor *= 1. - ratio.max(0.) + ratio * source;
                    }
                    if law == TargetLaw::MatrixFactor {
                        (base + 1.) * factor - 1.
                    } else {
                        base * factor
                    }
                }
                TargetLaw::AnalogTime => {
                    // Native logarithmic time converter has an offset; its physical
                    // range alone does not determine the modulation span.
                    let delta = self.delta_edges(edges, input, live, memo, depth + 1)?;
                    let offset = f64::from(0.001_f32);
                    let min = f64::from(0.0001_f32);
                    let max = 10.;
                    let span = ((max + offset) / (min + offset)).ln();
                    let shifted = ((base.clamp(min, max) + offset).ln() + delta * span).exp() as f32;
                    f64::from((shifted - offset as f32).clamp(min as f32, max as f32))
                }
                TargetLaw::LfoFrequency => {
                    // Workstation original renders: base1 + ratio.1*source1 =>3Hz,
                    // ratio.25 =>6Hz; base2 + ratio.25*source.5 =>4.5Hz.
                    (base + 20. * self.delta_edges(edges, input, live, memo, depth + 1)?).clamp(0., 20.)
                }
                TargetLaw::FilterFrequency => {
                    ensure!(base > 0., "Invalid UVI filter frequency base");
                    let delta = self.delta_edges(edges, input, live, memo, depth + 1)?;
                    (base * 1000_f64.powf(delta)).clamp(20., 20000.)
                }
                TargetLaw::Unit => {
                    // WaveTable endpoint controls, feedback and chorus Mix share this
                    // converter; their existing consumer/route gates are unchanged.
                    (base + self.delta_edges(edges, input, live, memo, depth + 1)?).clamp(0., 1.)
                }
                TargetLaw::Drive => {
                    (base + 40. * self.delta_edges(edges, input, live, memo, depth + 1)?).clamp(-20., 20.)
                }
                TargetLaw::DelayTime => {
                    (base + 10. * self.delta_edges(edges, input, live, memo, depth + 1)?).clamp(0., 10.)
                }
                TargetLaw::Boolean => {
                    f64::from(
                        (base + self.delta_edges(edges, input, live, memo, depth + 1)?).clamp(0., 1.)
                            >= 0.5,
                    )
                }
                TargetLaw::ChorusSpeed => {
                    let delta = self.delta_edges(edges, input, live, memo, depth + 1)?;
                    (base * 10_f64.powf(delta)).clamp(0.1, 1.)
                }
                TargetLaw::ChorusCrossover => {
                    let delta = self.delta_edges(edges, input, live, memo, depth + 1)?;
                    (base * 250_f64.powf(delta)).clamp(20., 5000.)
                }
                TargetLaw::ChorusDepth => {
                    let delta = self.delta_edges(edges, input, live, memo, depth + 1)?;
                    (base + 39. * delta).clamp(1., 40.)
                }
                TargetLaw::GainScale => {
                    (base + 4. * self.delta_edges(edges, input, live, memo, depth + 1)?).clamp(-2., 2.)
                }
                TargetLaw::Add => {
                    // DualDelay Mix deliberately stays raw here: its consumer clamps
                    // the smoothed current value. Other additive laws also stay raw.
                    base + self.delta_edges(edges, input, live, memo, depth + 1)?
                }
                TargetLaw::Unverified => unreachable!("target support checked above"),
            }
        };
        ensure!(value.is_finite(), "Nonfinite UVI modulation result");
        if let Some(cached) = cached {
            memo.insert(cached.slot, value);
        }
        Ok(value)
    }
    fn delta(
        &self,
        p: &Parameter,
        input: &Inputs,
        live: Overrides<'_>,
        memo: &mut MemoScratch,
        depth: usize,
    ) -> Result<f64> {
        let edges = self.cached_parameter(p.0, &p.1)
            .map_or(&[][..], |cached| cached.edges.as_slice());
        self.delta_edges(edges, input, live, memo, depth)
    }
    fn delta_edges(
        &self,
        edges: &[Connection],
        input: &Inputs,
        live: Overrides<'_>,
        memo: &mut MemoScratch,
        depth: usize,
    ) -> Result<f64> {
        let mut sum = 0.;
        for c in edges {
            if c.mode != 0 {
                continue;
            }
            if self.boolean(c.node, "Bypass", false, live)? {
                continue;
            }
            let cached_ratio = &self.parameter_values[c.ratio_slot];
            let ratio = self.value_cached(&cached_ratio.key, Some(cached_ratio), input, live, memo, depth + 1)?;
            let (mut value, bipolar) = self.source(&c.source, input, live, memo, depth + 1)?;
            if self.boolean(c.node, "Inverted", false, live)? {
                value = if bipolar { -value } else { 1. - value };
            }
            if let Some(id) = c.mapper {
                value = self.mappers[&id].apply(value, bipolar);
            }
            sum += ratio * value;
        }
        ensure!(sum.is_finite(), "Nonfinite UVI modulation sum");
        Ok(sum)
    }
    fn builtin_alternate(&self, input: &Inputs) -> Result<f64> {
        let key = (2, input.voice, input.instance);
        let mut values = self.builtin_values.borrow_mut();
        ensure!(
            values.len() < LIMIT || values.contains_key(&key),
            "UVI builtin note state exceeds limit"
        );
        let value = values.entry(key).or_insert_with(|| {
            let mut next = self.alternate_next.borrow_mut();
            let value = *next;
            *next = -value;
            value
        });
        Ok(f64::from(*value))
    }
    fn builtin_random(&self, input: &Inputs, bipolar: bool) -> Result<f64> {
        let key = (u8::from(bipolar), input.voice, input.instance);
        let mut values = self.builtin_values.borrow_mut();
        ensure!(
            values.len() < LIMIT || values.contains_key(&key),
            "UVI builtin random state exceeds limit"
        );
        let value = values.entry(key).or_insert_with(|| {
            let mut seeds = self.builtin_seeds.borrow_mut();
            let seed = &mut seeds[usize::from(bipolar)];
            *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let value = *seed as f32 * (1_f32 / 4294967296.);
            if bipolar { value * 2. - 1. } else { value }
        });
        Ok(f64::from(*value))
    }
    fn source(
        &self,
        s: &Source,
        input: &Inputs,
        live: Overrides<'_>,
        memo: &mut MemoScratch,
        depth: usize,
    ) -> Result<(f64, bool)> {
        let (value, bipolar) = match *s {
            Source::Key | Source::KeyFollow | Source::LinearKeyFollow => {
                // Native note sources include the event's fractional tune.
                // KeyFollow has an asymmetric bipolar response: the lower
                // half is quadratic, the upper half a square root. Authored
                // original renders verify both the curve and its polarity.
                let key = (f32::from(input.key) + input.tune_semitones as f32).clamp(0., 127.);
                let step = 1_f32 / 60.;
                match *s {
                    Source::Key => (f64::from(key * (1_f32 / 127.)), false),
                    Source::KeyFollow => {
                        let value = if key < 60. {
                            key * key * (step * step) - 1.
                        } else {
                            f64::from((key.min(120.) - 60.) * step).sqrt() as f32
                        };
                        (f64::from(value), true)
                    }
                    _ => (f64::from(((key - 60.) * step).clamp(-1., 1.)), true),
                }
            }
            Source::Velocity => (f64::from(input.velocity) / 127., false),
            Source::Controller(cc) => (f64::from(input.controllers[usize::from(cc)]) / 127., false),
            Source::Bend => (input.pitch_bend, true),
            Source::Pressure => (input.channel_pressure, false),
            Source::Random(bipolar) => (self.builtin_random(input, bipolar)?, bipolar),
            Source::Alternate => (self.builtin_alternate(input)?, true),
            Source::PolyPressure => (input.poly_pressure, false),
            Source::OrganPan => {
                let value = f32::from(127 - input.key) * (1_f32 / 254.);
                (
                    f64::from(if input.key.is_multiple_of(2) {
                        value
                    } else {
                        -value
                    }),
                    true,
                )
            }
            Source::Node(n) => {
                let cache_source = matches!(self.kinds[n].as_str(), "ConstantModulation" | "ScriptEventModulation" | "LFO");
                if cache_source {
                    if let Some(&value) = memo.sources.get(&n) {
                        ensure!(depth + 1 + usize::from(self.kinds[n] == "LFO") < DEPTH, "UVI modulation evaluation depth exceeds limit");
                        return Ok(value);
                    }
                }
                let bipolar = self.boolean(
                    n,
                    "Bipolar",
                    matches!(
                        self.kinds[n].as_str(),
                        "LFO" | "ScriptEventModulation" | "StdRandom" | "Drunk"
                    ),
                    live,
                )?;
                let bypass = self.value_named(n, "Bypass", input, live, memo, depth + 1)?;
                ensure!(
                    bypass == 0. || bypass == 1.,
                    "Invalid UVI modulation source Bypass"
                );
                if self.kinds[n] == "StepEnvelope" {
                    self.step_gate(n, input, live)?;
                }
                if bypass != 0. {
                    return Ok((0., bipolar));
                }
                let v = match self.kinds[n].as_str() {
                    "ConstantModulation" => {
                        let v = self
                            .value_named(n, "Value", input, live, memo, depth + 1)?
                            .clamp(0., 1.);
                        let style = self.setting(n, "Style", 0., live)?;
                        ensure!(
                            style == 0. || style == 1.,
                            "Unsupported UVI Constant style at node {n}"
                        );
                        let target = if style == 1. {
                            f32::from(v > 0.5)
                        } else {
                            v as f32
                        };
                        let clock = input.time_seconds;
                        let position = clock * input.sample_rate;
                        ensure!(position < u64::MAX as f64, "UVI Constant clock overflow");
                        let frame = (position + 0.000001).floor() as u64;
                        let value = {
                            let mut clocks = self.constant_clocks.borrow_mut();
                            let key = (n, input.voice, input.instance);
                            ensure!(
                                clocks.len() < LIMIT || clocks.contains_key(&key),
                                "UVI Constant state exceeds limit"
                            );
                            let state = clocks.entry(key).or_insert(ConstantClock {
                                rate: input.sample_rate,
                                block_frames: input.control_block_frames,
                                frame,
                                integrated: frame,
                                point_frame: frame / 32 * 32,
                                current: target,
                                point: target,
                                target,
                            });
                            ensure!(
                                state.rate == input.sample_rate
                                    && state.block_frames == input.control_block_frames,
                                "UVI Constant control clock configuration changed during playback"
                            );
                            state.advance(frame, target)?
                        };
                        if bipolar { 2. * value - 1. } else { value }
                    }
                    "ScriptEventModulation" => {
                        let id = self.value_named(n, "EventId", input, live, memo, depth + 1)?;
                        ensure!(
                            (0. ..=127.).contains(&id) && id.fract() == 0.,
                            "Invalid live UVI script EventId"
                        );
                        let id = id as u8;
                        let v = input.script_values.get(&id).copied().unwrap_or_else(|| {
                            self.ramp(id, input.voice, Some(n))
                                .map(|r| r.value(input.time_seconds))
                                .unwrap_or(0.)
                        });
                        ensure!(
                            v.is_finite() && (if bipolar { -1. } else { 0. }..=1.).contains(&v),
                            "Invalid UVI script source value"
                        );
                        v
                    }
                    "LFO" => self.lfo(n, bipolar, input, live, memo, depth + 1)?,
                    "StepEnvelope" => self.step(n, input, live)?,
                    "DAHDSR" | "AHD" => {
                        let value = self.dah(n, input, live, memo, depth + 1)?;
                        if bipolar { 2. * value - 1. } else { value }
                    }
                    "StdRandom" | "Drunk" => {
                        self.stochastic(n, input, live, memo, depth + 1, bipolar)?
                    }
                    "AttackDecayEnv" => {
                        let value = self.attack_decay(n, input, live, memo, depth + 1)?;
                        if bipolar { 2. * value - 1. } else { value }
                    }
                    "MultiEnvelope" => {
                        let value = self.multi(n, input, live, memo, depth + 1)?;
                        if bipolar { 2. * value - 1. } else { value }
                    }
                    "AnalogADSR" => {
                        let value = self.analog(n, input, live, memo, depth + 1)?;
                        if bipolar { 2. * value - 1. } else { value }
                    }
                    _ => bail!("Unsupported UVI modulation source"),
                };
                // Repeated f64-clock reads can change signed zero or reject
                // a derived infinite frequency. Preserve those original paths.
                if cache_source && (self.kinds[n] != "LFO" || self.lfo_clocks.borrow()
                    .get(&(n, input.voice, input.instance))
                    .is_none_or(|clock| clock.frequency.is_finite() && clock.phase.to_bits() != (-0f64).to_bits())) {
                    ensure!(v.is_finite(), "Nonfinite UVI modulation source");
                    memo.sources.insert(n, (v, bipolar));
                }
                (v, bipolar)
            }
        };
        ensure!(value.is_finite(), "Nonfinite UVI modulation source");
        Ok((value, bipolar))
    }
    fn step_gate(&self, n: NodeId, input: &Inputs, live: Overrides<'_>) -> Result<()> {
        // Workstation 4.0.9 authored native caller fixtures cover the global,
        // host-synchronized, unsmoothed table source. Other modes stay gated.
        for (name, expected) in [
            ("SyncToHost", 1.),
            ("Retrigger", 0.),
            ("InterpolationMode", 0.),
            ("Smooth", 0.),
            ("Bipolar", 0.),
            ("Depth", 1.),
            ("ManualTrigger", 0.),
            ("Bypass", 0.),
        ] {
            let observed = self.setting(n, name, if name == "Depth" { 1. } else { 0. }, live)?;
            source_setting_gate(n, "StepEnvelope", name, observed, observed == expected)?;
        }
        ensure!(
            self.node_targets[n].is_empty(),
            "Unverified connected UVI StepEnvelope parameter at node {n}"
        );
        let steps = self.setting(n, "NumSteps", 16., live)?;
        ensure!(
            steps == number(&self.bases[n], "NumSteps", 16.)?,
            "Unverified live UVI StepEnvelope step count at node {n}"
        );
        let frequency = self.setting(n, "Freq", 1., live)? as f32;
        ensure!(
            frequency > 0. && frequency <= 20. && input.host_tempo > 0.,
            "Invalid UVI StepEnvelope frequency/tempo at node {n}"
        );
        if let Some(position) = input.host_position {
            let frame = (input.time_seconds * input.sample_rate + 0.000001).floor() as u64;
            ensure!(
                position.playing,
                "Unverified stopped UVI StepEnvelope host clock at node {n}"
            );
            ensure!(
                position.beat >= 0. && position.frame <= frame,
                "Invalid UVI StepEnvelope host position at node {n}"
            );
            ensure!(
                position.frame.is_multiple_of(32),
                "Unverified unaligned UVI StepEnvelope transport snapshot at node {n}"
            );
            // A Q32-aligned snapshot does not establish a fresh source generation.
            // Native same-generation queries retain already-prepared block points.
            ensure!(
                position.frame.is_multiple_of(u64::from(input.control_block_frames)),
                "Unverified midblock UVI StepEnvelope transport snapshot at node {n}"
            );
        }
        let settings = (
            input.sample_rate,
            input.host_tempo,
            frequency,
            input.control_block_frames,
        );
        let mut previous = self.step_settings.borrow_mut();
        let initial = previous.entry(n).or_insert(settings);
        ensure!(
            initial.0 == settings.0
                && initial.2 == settings.2
                && initial.3 == settings.3
                && (input.host_position.is_some() || initial.1 == settings.1),
            "Unverified live UVI StepEnvelope frequency/clock change at node {n}"
        );
        Ok(())
    }
    fn step(&self, n: NodeId, input: &Inputs, live: Overrides<'_>) -> Result<f64> {
        let values = self
            .tables
            .get(&n)
            .context("UVI StepEnvelope Levels are missing")?;
        let count = self.setting(n, "NumSteps", 16., live)? as usize;
        let frequency = f64::from(self.setting(n, "Freq", 1., live)? as f32);
        ensure!(
            input.time_seconds * input.sample_rate < (u64::MAX - 65536) as f64,
            "UVI StepEnvelope clock overflow"
        );
        let frame = (input.time_seconds * input.sample_rate + 0.000001).floor() as u64;
        let block = u64::from(input.control_block_frames);
        let block_start = frame / block * block;
        let (start, beat) = if let Some(position) = input.host_position {
            let start = block_start.max(position.frame);
            let beat = position.beat
                + (start - position.frame) as f64 / input.sample_rate * input.host_tempo / 60.;
            (start, beat)
        } else {
            (
                block_start,
                block_start as f64 / input.sample_rate * input.host_tempo / 60.,
            )
        };
        let tick = (frame - start) / 32;
        // Native consumes the supplied host beat at the block start, then
        // advances double phase at each 32-frame point. Only a snapshot at a
        // logical block boundary may start a fresh host-position projection.
        let mut phase = beat / frequency;
        let increment = 32.
            * (f64::from(input.host_tempo as f32) * (1. / 60.)
                / f64::from(input.sample_rate as f32))
            * (1. / frequency);
        let (left, right) = if block == 256 {
            let key = StepProjectionKey {
                start,
                block_start,
                beat: beat.to_bits(),
                rate: input.sample_rate.to_bits(),
                tempo: input.host_tempo.to_bits(),
                frequency: (frequency as f32).to_bits(),
                count,
                position: input.host_position.map(|p| (p.frame, p.beat.to_bits(), p.playing)),
            };
            let mut prepared = self.step_projection.borrow_mut();
            if let Some(points) = prepared.get(&n).filter(|points| points.key == key) {
                phase = points.phases[tick as usize];
                ensure!(phase.is_finite() && phase >= 0. && phase + increment < f64::from(i32::MAX),
                    "UVI StepEnvelope phase overflow");
                (points.points[tick as usize], points.points[tick as usize + 1])
            } else {
                let mut phases = [0.; 9];
                phases[0] = phase;
                for index in 1..9 {
                    phases[index] = phases[index - 1] + increment;
                }
                phase = phases[tick as usize];
                // Validate only the queried point pair, as before. Future phases
                // do not make a currently valid query fail early near i32::MAX.
                ensure!(phase.is_finite() && phase >= 0. && phase + increment < f64::from(i32::MAX),
                    "UVI StepEnvelope phase overflow");
                let points = phases.map(|phase|
                    values[(phase.floor() as u64 % count as u64) as usize] as f32);
                let pair = (points[tick as usize], points[tick as usize + 1]);
                prepared.insert(n, StepProjection { key, phases, points });
                pair
            }
        } else {
            for _ in 0..tick {
                phase += increment;
            }
            ensure!(phase.is_finite() && phase >= 0. && phase + increment < f64::from(i32::MAX),
                "UVI StepEnvelope phase overflow");
            (values[(phase.floor() as u64 % count as u64) as usize] as f32,
                values[((phase + increment).floor() as u64 % count as u64) as usize] as f32)
        };
        Ok(f64::from(
            left + (right - left) * ((frame - start - tick * 32) as f32 / 32.),
        ))
    }
    fn dah(
        &self,
        n: NodeId,
        input: &Inputs,
        live: Overrides<'_>,
        memo: &mut MemoScratch,
        depth: usize,
    ) -> Result<f64> {
        ensure!(
            self.setting(n, "Retrigger", 1., live)? == 1.,
            "Unverified shared UVI DAHDSR trigger mode at node {n}"
        );
        let one_shot = self.kinds[n] == "AHD";
        let mut durations = [0.; 4];
        for (index, name) in ["DelayTime", "AttackTime", "HoldTime", "DecayTime"]
            .into_iter()
            .enumerate()
        {
            if one_shot && index == 0 {
                continue;
            }
            let max = if index == 3 { 30. } else { 10. };
            durations[index] = self
                .value_named(n, name, input, live, memo, depth + 1)?
                .clamp(0., max) as f32
                * input.sample_rate as f32;
        }
        let velocity = f64::from(input.velocity) / 127.;
        let amount = self.setting(n, "VelocityAmount", 0., live)?.clamp(0., 1.);
        let sensitivity = self.setting(n, "VelocitySens", 0.75, live)?.clamp(-1., 1.);
        let velocity_factor = if sensitivity == 1. {
            f64::from(input.velocity == 127)
        } else {
            velocity.powf(1. - (1. - sensitivity).log2())
        };
        let settings = DahSettings {
            durations,
            curves: [
                self.setting(n, "AttackCurve", 0., live)?,
                self.setting(n, "DecayCurve", 0., live)?,
                self.setting(n, "ReleaseCurve", 0., live)?,
            ],
            sustain: if one_shot {
                0.
            } else {
                self.value_named(n, "SustainLevel", input, live, memo, depth + 1)?
                    .clamp(0., 1.)
            },
            release: if one_shot {
                0.
            } else {
                self.value_named(n, "ReleaseTime", input, live, memo, depth + 1)?
                    .clamp(0., 20.) as f32
                    * input.sample_rate as f32
            },
            note_off_retrigger: self.boolean(n, "NoteOffRetrigger", false, live)?,
            one_shot,
            amplitude: 1. - amount + amount * velocity_factor,
        };
        let position = input.voice_time_seconds * input.sample_rate;
        ensure!(
            position < (u64::MAX - 32) as f64,
            "UVI DAHDSR clock overflow"
        );
        let frame = (position + 0.000001).floor() as u64;
        let off = input
            .note_off_time_seconds
            .map(|time| (time * input.sample_rate + 0.000001).floor() as u64);
        let mut clocks = self.dah_clocks.borrow_mut();
        let key = (n, input.voice, input.instance);
        ensure!(
            clocks.len() < LIMIT || clocks.contains_key(&key),
            "UVI DAHDSR state exceeds limit"
        );
        let clock = clocks.entry(key).or_insert_with(|| {
            let mut clock = DahClock {
                rate: input.sample_rate,
                origin: ((input.time_seconds * input.sample_rate + 0.000001).floor() as u64)
                    .saturating_sub(frame),
                block_frames: input.control_block_frames,
                frame: 0,
                stage: -1,
                remaining: 0.,
                elapsed: 0,
                denominator: 0,
                released: false,
                pending_release: false,
                release_level: 0.,
            };
            clock.next_stage(0., settings);
            clock
        });
        ensure!(
            clock.rate == input.sample_rate && clock.block_frames == input.control_block_frames,
            "UVI DAHDSR clock configuration changed during playback"
        );
        clock.advance(frame, off, settings)
    }
    fn stochastic(
        &self,
        n: NodeId,
        input: &Inputs,
        live: Overrides<'_>,
        memo: &mut MemoScratch,
        depth: usize,
        bipolar: bool,
    ) -> Result<f64> {
        let mode = self.setting(n, "TriggerMode", 1., live)?;
        ensure!(
            mode == 0. || mode == 1.,
            "Unverified UVI stochastic trigger mode at node {n}"
        );
        let smooth = self.kinds[n] == "StdRandom";
        let rate = self.value_named(n, "Rate", input, live, memo, depth + 1)? as f32;
        let depth_value = if smooth {
            self.value_named(n, "Depth", input, live, memo, depth + 1)? as f32
        } else {
            1.
        };
        let step = if smooth {
            0.
        } else {
            self.value_named(n, "Step", input, live, memo, depth + 1)? as f32
        };
        let bias = if smooth {
            0.
        } else {
            self.value_named(n, "Bias", input, live, memo, depth + 1)? as f32
        };
        ensure!(
            (if smooth { 0. } else { 0.1 }..=1000.).contains(&rate)
                && (0. ..=1.).contains(&depth_value)
                && (smooth || ((0.1..=1000.).contains(&step) && (-1. ..=1.).contains(&bias))),
            "Invalid UVI stochastic source parameters at node {n}"
        );
        let mut owner = self.parents[n];
        while owner.is_some_and(|node| {
            !matches!(self.kinds[node].as_str(), "Program" | "Layer" | "Keygroup")
        }) {
            owner = owner.and_then(|node| self.parents[node]);
        }
        let program_clock =
            owner.is_some_and(|node| matches!(self.kinds[node].as_str(), "Program" | "Layer"));
        let frame = (input.time_seconds * input.sample_rate + 0.000001).floor() as u64;
        let age = (input.voice_time_seconds * input.sample_rate + 0.000001).floor() as u64;
        let voice_origin = frame.saturating_sub(age);
        let origin = if mode == 0. && program_clock {
            0
        } else {
            voice_origin
        };
        let key = if mode == 0. {
            (n, None, None)
        } else {
            (n, input.voice, input.instance)
        };
        let settings = StochasticSettings {
            rate,
            depth: depth_value,
            step,
            bias,
            bipolar,
        };
        let mut clocks = self.stochastic_clocks.borrow_mut();
        ensure!(
            clocks.len() < LIMIT || clocks.contains_key(&key),
            "UVI stochastic state exceeds limit"
        );
        if let std::collections::hash_map::Entry::Vacant(entry) = clocks.entry(key) {
            // Clock-based native seeds are unavailable offline. A stable seed
            // preserves the measured RNG and filter laws, with an explicit
            // fidelity diagnostic rather than a cross-process parity claim.
            let seed = if mode == 0. && smooth {
                1
            } else {
                (n as u32)
                    .wrapping_mul(1664525)
                    .wrapping_add(input.instance.unwrap_or(0) as u32)
                    .wrapping_add(1)
            };
            let source = if smooth {
                let mut source = SmoothRandomClock::new(seed, rate, depth_value, mode == 1.);
                if mode == 1. && self.boolean(n, "RandomStart", false, live)? {
                    source.random_start();
                }
                StochasticSource::Smooth(source)
            } else {
                let initial = self.setting(n, "InitialValue", 0., live)? as f32;
                ensure!(
                    (-1. ..=1.).contains(&initial),
                    "Invalid UVI Drunk initial value"
                );
                StochasticSource::Drunk(DrunkClock::new(
                    seed,
                    initial,
                    step,
                    rate,
                    bias,
                    bipolar,
                    mode == 1.,
                ))
            };
            entry.insert(StochasticClock {
                sample_rate: input.sample_rate,
                block_frames: input.control_block_frames,
                origin,
                cursor: origin,
                source,
                settings,
                segment: None,
            });
        }
        let clock = clocks.get_mut(&key).unwrap();
        ensure!(
            clock.sample_rate == input.sample_rate
                && clock.block_frames == input.control_block_frames
                && (mode == 0. || clock.origin == origin),
            "UVI stochastic clock configuration changed"
        );
        let off = input
            .note_off_time_seconds
            .map(|off| voice_origin + (off * input.sample_rate + 0.000001).floor() as u64);
        clock.advance(frame, self.control_segment_end, off, settings)
    }

    fn attack_decay(
        &self,
        n: NodeId,
        input: &Inputs,
        live: Overrides<'_>,
        memo: &mut MemoScratch,
        depth: usize,
    ) -> Result<f64> {
        let attack = self.value_named(n, "Attack", input, live, memo, depth + 1)? as f32;
        let decay = self.value_named(n, "DecayTime", input, live, memo, depth + 1)? as f32;
        ensure!(
            decay > 0. && decay.is_finite(),
            "Invalid UVI AttackDecayEnv decay time"
        );
        let frame = (input.time_seconds * input.sample_rate + 0.000001).floor() as u64;
        let age = (input.voice_time_seconds * input.sample_rate + 0.000001).floor() as u64;
        let origin = frame.saturating_sub(age);
        let off = input
            .note_off_time_seconds
            .map(|off| origin + (off * input.sample_rate + 0.000001).floor() as u64);
        let key = (n, input.voice, input.instance);
        let mut clocks = self.attack_decay_clocks.borrow_mut();
        ensure!(
            clocks.len() < LIMIT || clocks.contains_key(&key),
            "UVI AttackDecayEnv state exceeds limit"
        );
        if let std::collections::hash_map::Entry::Vacant(entry) = clocks.entry(key) {
            entry.insert(AttackDecayClock::new(
                input.sample_rate,
                origin,
                input.control_block_frames,
                attack,
                decay,
            )?);
        }
        let clock = clocks.get_mut(&key).unwrap();
        ensure!(
            clock.rate == input.sample_rate
                && clock.origin == origin
                && clock.block_frames == input.control_block_frames,
            "UVI AttackDecayEnv clock configuration changed"
        );
        ensure!(
            clock.attack == attack && clock.decay == decay,
            "Unverified live UVI AttackDecayEnv coefficient changes"
        );
        clock.advance(frame, self.control_segment_end, off)
    }

    fn multi(
        &self,
        n: NodeId,
        input: &Inputs,
        live: Overrides<'_>,
        memo: &mut MemoScratch,
        depth: usize,
    ) -> Result<f64> {
        ensure!(
            self.setting(n, "Smooth", 0., live)? == 0.
                && self.setting(n, "VelocityAmount", 0., live)? == 0.
                && !self.boolean(n, "NoteOffRetrigger", false, live)?,
            "Unverified UVI MultiEnvelope smoothing/velocity/off-retrigger at node {n}"
        );
        let mode = self.setting(n, "Retrigger", 1., live)?;
        ensure!(
            (0. ..=2.).contains(&mode) && mode.fract() == 0.,
            "Invalid UVI MultiEnvelope trigger mode"
        );
        let mut owner = self.parents[n];
        while owner.is_some_and(|node| {
            !matches!(self.kinds[node].as_str(), "Program" | "Layer" | "Keygroup")
        }) {
            owner = owner.and_then(|node| self.parents[node]);
        }
        let per_group = owner.is_some_and(|node| self.kinds[node] == "Keygroup");
        ensure!(
            mode != 2. || per_group,
            "Unverified shared UVI MultiEnvelope legato mode at node {n}"
        );
        let global = mode == 0. && !per_group;
        let speed = self.value_named(n, "Speed", input, live, memo, depth + 1)? as f32;
        ensure!(
            speed.is_finite() && speed > 0.,
            "Invalid UVI MultiEnvelope speed"
        );
        let sync = self.boolean(n, "SyncToHost", false, live)?;
        let step_nodes = &self.multi_steps[&n];
        let mut steps = Vec::with_capacity(step_nodes.len());
        for &node in step_nodes {
            let time = self.setting(node, "Time", 0., live)? as f32;
            let mut duration = time / speed;
            if sync {
                duration *= 59.999996_f32 / input.host_tempo as f32;
            }
            duration *= input.sample_rate as f32;
            let level = self.setting(node, "DestLevel", 0., live)? as f32;
            ensure!(
                duration.is_finite() && duration >= 0. && level.is_finite(),
                "Invalid UVI MultiEnvelope step"
            );
            steps.push(MultiStep {
                duration,
                level,
                curve: self.setting(node, "Curve", 0., live)?,
            });
        }
        let index = |name| -> Result<Option<usize>> {
            let value = self.setting(n, name, -1., live)?;
            ensure!(value.fract() == 0., "Invalid UVI MultiEnvelope point index");
            Ok((value >= 0.).then(|| (value as usize).min(steps.len() - 1)))
        };
        let loop_points = index("LoopStart")?.zip(index("LoopEnd")?);
        ensure!(
            loop_points.is_none_or(|(begin, end)| begin <= end),
            "Invalid UVI MultiEnvelope loop points"
        );
        let settings = MultiSettings {
            steps: &steps,
            loop_points,
            release: index("ReleaseStep")?,
        };
        let position = if global {
            input.time_seconds
        } else {
            input.voice_time_seconds
        } * input.sample_rate;
        ensure!(
            position < (u64::MAX - 32) as f64,
            "UVI MultiEnvelope clock overflow"
        );
        let frame = (position + 0.000001).floor() as u64;
        let key = if global {
            (n, None, None)
        } else {
            (n, input.voice, input.instance)
        };
        let off = if mode == 1. {
            input
                .note_off_time_seconds
                .map(|time| (time * input.sample_rate + 0.000001).floor() as u64)
        } else {
            None
        };
        let mut clocks = self.multi_clocks.borrow_mut();
        ensure!(
            clocks.len() < LIMIT || clocks.contains_key(&key),
            "UVI MultiEnvelope state exceeds limit"
        );
        let clock = clocks.entry(key).or_insert(MultiClock {
            rate: input.sample_rate,
            origin: if global {
                0
            } else {
                ((input.time_seconds * input.sample_rate + 0.000001).floor() as u64)
                    .saturating_sub(frame)
            },
            block_frames: input.control_block_frames,
            frame: 0,
            index: usize::MAX,
            remaining: 0.,
            elapsed: 0,
            denominator: 0,
            start: 0.,
            target: 0.,
            held: false,
            released: false,
            kill_frame: None,
        });
        if clock.index == usize::MAX {
            clock.enter(0, 0., &settings)?;
        }
        ensure!(
            clock.rate == input.sample_rate && clock.block_frames == input.control_block_frames,
            "UVI MultiEnvelope clock configuration changed"
        );
        clock.advance(frame, off, &settings)
    }

    fn analog(
        &self,
        n: NodeId,
        input: &Inputs,
        live: Overrides<'_>,
        memo: &mut MemoScratch,
        depth: usize,
    ) -> Result<f64> {
        ensure!(
            self.setting(n, "TriggerMode", 0., live)? == 0.,
            "Unverified shared UVI Analog envelope trigger mode at node {n}"
        );
        ensure!(
            self.setting(n, "ManualTrigger", 0., live)? == 0.,
            "Unimplemented UVI Analog manual trigger at node {n}"
        );
        let velocity = f64::from(input.velocity) / 127.;
        let timing_velocity = if self.boolean(n, "InvertVelocity", false, live)? {
            1. - velocity
        } else {
            velocity
        };
        let coefficient =
            |time: f64| (1. - (2_f64 / 3.).powf(32. / (input.sample_rate * time))) as f32;
        let time = |name: &str,
                    key_name: &str,
                    velocity_name: &str,
                    memo: &mut MemoScratch|
         -> Result<f64> {
            let base = self
                .value_named(n, name, input, live, memo, depth + 1)?
                .clamp(0.0001, 10.);
            let key_depth = self.setting(n, key_name, 0., live)?.clamp(-2., 2.);
            let velocity_depth = self.setting(n, velocity_name, 0., live)?.clamp(-1., 1.);
            Ok(base
                * 2_f64.powf((f64::from(input.key) + input.tune_semitones - 60.) * key_depth / 12.)
                * 10000_f64.powf(timing_velocity * velocity_depth))
        };
        let settings = AnalogSettings {
            attack: coefficient(time("AttackTime", "KeyToAttack", "VelToAttack", memo)?),
            decay: coefficient(time("DecayTime", "KeyToDecay", "VelToDecay", memo)?),
            release: coefficient(
                self.value_named(n, "ReleaseTime", input, live, memo, depth + 1)?
                    .clamp(0.0001, 10.),
            ),
            sustain: self
                .value_named(n, "SustainLevel", input, live, memo, depth + 1)?
                .clamp(0., 1.) as f32,
            punch: self.setting(n, "Punch", 0., live)?.clamp(0., 1.) as f32,
            attack_decay: self.boolean(n, "AttackDecayMode", false, live)?,
            amplitude: 10_f64.powf(
                (velocity - 1.) * self.setting(n, "DynamicRange", 0., live)?.clamp(0., 40.) / 20.,
            ),
        };
        let position = input.voice_time_seconds * input.sample_rate;
        ensure!(
            position < (u64::MAX - 32) as f64,
            "UVI Analog envelope clock overflow"
        );
        let frame = (position + 0.000001).floor() as u64;
        let off = input
            .note_off_time_seconds
            .map(|time| (time * input.sample_rate + 0.000001).floor() as u64);
        let mut clocks = self.analog_clocks.borrow_mut();
        let key = (n, input.voice, input.instance);
        ensure!(
            clocks.len() < LIMIT || clocks.contains_key(&key),
            "UVI Analog envelope state exceeds limit"
        );
        let clock = clocks.entry(key).or_insert(AnalogClock {
            rate: input.sample_rate,
            origin: ((input.time_seconds * input.sample_rate + 0.000001).floor() as u64)
                .saturating_sub(frame),
            block_frames: input.control_block_frames,
            frame: 0,
            value: 0.,
            stage: 1,
            released: false,
        });
        ensure!(
            clock.rate == input.sample_rate && clock.block_frames == input.control_block_frames,
            "UVI Analog envelope clock configuration changed during playback"
        );
        clock.advance(frame, off, settings)
    }
    pub fn has_release_envelopes(&self, nodes: &HashSet<NodeId>) -> bool {
        nodes
            .iter()
            .flat_map(|node| self.node_targets.get(*node).into_iter().flatten())
            .flat_map(|target| self.target_sources.get(target).into_iter().flatten())
            .any(|node| {
                matches!(
                    self.kinds[*node].as_str(),
                    "AnalogADSR" | "DAHDSR" | "AHD" | "MultiEnvelope" | "AttackDecayEnv"
                )
            })
    }
    /// Release completion belongs to the render instance, not logical script ID.
    pub fn release_finished(
        &self,
        input: &Inputs,
        live: &HashMap<Parameter, f64>,
        nodes: &HashSet<NodeId>,
    ) -> Result<bool> {
        self.release_nodes_finished(input, Overrides::External(live), nodes)
    }
    fn release_nodes_finished(
        &self,
        input: &Inputs,
        live: Overrides<'_>,
        nodes: &HashSet<NodeId>,
    ) -> Result<bool> {
        self.validate(input, live)?;
        let mut memo = self.memo.borrow_mut();
        memo.begin();
        let mut envelopes = HashSet::new();
        for target in nodes
            .iter()
            .flat_map(|node| self.node_targets.get(*node).into_iter().flatten())
        {
            for node in self.target_sources.get(target).into_iter().flatten() {
                if matches!(
                    self.kinds[*node].as_str(),
                    "AnalogADSR" | "DAHDSR" | "AHD" | "MultiEnvelope" | "AttackDecayEnv"
                ) {
                    envelopes.insert(*node);
                }
            }
        }
        for node in envelopes {
            if self.value_named(node, "Bypass", input, live, &mut memo, 0)? != 0. {
                continue;
            }
            match self.kinds[node].as_str() {
                "AnalogADSR" => {
                    self.analog(node, input, live, &mut memo, 0)?;
                    if self
                        .analog_clocks
                        .borrow()
                        .get(&(node, input.voice, input.instance))
                        .is_some_and(|clock| clock.stage != 0)
                    {
                        return Ok(false);
                    }
                }
                "DAHDSR" | "AHD" => {
                    self.dah(node, input, live, &mut memo, 0)?;
                    if self
                        .dah_clocks
                        .borrow()
                        .get(&(node, input.voice, input.instance))
                        .is_some_and(|clock| clock.stage != 6)
                    {
                        return Ok(false);
                    }
                }
                "AttackDecayEnv" => {
                    self.attack_decay(node, input, live, &mut memo, 0)?;
                    if self
                        .attack_decay_clocks
                        .borrow()
                        .get(&(node, input.voice, input.instance))
                        .is_some_and(|clock| {
                            clock.completion.is_none_or(|end| {
                                input.time_seconds * input.sample_rate < end as f64
                            })
                        })
                    {
                        return Ok(false);
                    }
                }
                "MultiEnvelope" => {
                    self.multi(node, input, live, &mut memo, 0)?;
                    let clocks = self.multi_clocks.borrow();
                    let clock = clocks
                        .get(&(node, input.voice, input.instance))
                        .or_else(|| clocks.get(&(node, None, None)));
                    if clock.is_some_and(|clock| {
                        clock.kill_frame.is_none_or(|kill| {
                            input.time_seconds * input.sample_rate < (clock.origin + kill) as f64
                        })
                    }) {
                        return Ok(false);
                    }
                }
                _ => bail!("Unimplemented UVI envelope release at node {node}"),
            }
        }
        Ok(true)
    }
    fn random_lfo(
        &self,
        n: NodeId,
        input: &Inputs,
        frequency: f64,
        phase: f64,
        smooth: f64,
    ) -> Result<f64> {
        let position = input.time_seconds * input.sample_rate;
        ensure!(
            position < (u64::MAX - 65536) as f64,
            "UVI random LFO clock overflow"
        );
        let frame = (position + 0.000001).floor() as u64;
        let block = frame / u64::from(input.control_block_frames);
        let key = (n, input.voice, input.instance);
        let mut seeds = self.random_seeds.borrow_mut();
        // Native constructors use a process-clock seed. Its clock origin is
        // unavailable in serialized programs; keep the same stochastic law.
        let seed = seeds.entry(n).or_insert_with(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|time| time.as_millis() as u32)
                .unwrap_or(0)
        });
        let mut clocks = self.random_lfo_clocks.borrow_mut();
        ensure!(
            clocks.len() < LIMIT || clocks.contains_key(&key),
            "UVI random LFO state exceeds limit"
        );
        let state = clocks.entry(key).or_insert_with(|| {
            let mut unit = Wave6 {
                phase_parameter: phase as f32,
                ..Wave6::default()
            };
            unit.note(seed, true);
            let frames =
                input.control_block_frames - (frame % u64::from(input.control_block_frames)) as u32;
            let controls = unit.controls(
                seed,
                frequency as f32,
                input.sample_rate as f32,
                smooth as f32,
                frames,
            );
            RandomLfoClock {
                unit,
                block,
                block_start: frame,
                controls,
                rate: input.sample_rate,
                block_frames: input.control_block_frames,
            }
        });
        ensure!(
            state.rate == input.sample_rate && state.block_frames == input.control_block_frames,
            "UVI random LFO clock configuration changed during playback"
        );
        ensure!(
            block >= state.block && frame >= state.block_start,
            "UVI random LFO clock moved backwards"
        );
        ensure!(
            block - state.block <= LIMIT as u64,
            "UVI random LFO control-block limit exceeded"
        );
        while state.block < block {
            state.block += 1;
            state.block_start = state.block * u64::from(state.block_frames);
            state.controls = state.unit.controls(
                seed,
                frequency as f32,
                state.rate as f32,
                smooth as f32,
                state.block_frames,
            );
        }
        let offset = frame - state.block_start;
        let index = (offset / 32) as usize;
        ensure!(
            index + 1 < state.controls.len(),
            "Invalid UVI random LFO control-point index"
        );
        let block_end = (state.block + 1) * u64::from(state.block_frames);
        let step = (block_end - (state.block_start + index as u64 * 32)).min(32);
        let fraction = (offset % 32) as f32 / step as f32;
        let left = state.controls[index];
        Ok(f64::from(
            left + (state.controls[index + 1] - left) * fraction,
        ))
    }
    fn lfo(
        &self,
        n: NodeId,
        bipolar: bool,
        input: &Inputs,
        live: Overrides<'_>,
        memo: &mut MemoScratch,
        depth: usize,
    ) -> Result<f64> {
        let smooth = self.setting(n, "Smooth", 0., live)?;
        ensure!(
            (0. ..=1.).contains(&smooth),
            "Invalid UVI LFO smoothing at node {n}"
        );
        let retrigger = self.setting(n, "Retrigger", 1., live)?;
        ensure!(
            retrigger == 0. || retrigger == 1.,
            "Unsupported UVI LFO trigger mode at node {n}"
        );
        let clock = if retrigger == 1. {
            input.voice_time_seconds
        } else {
            input.time_seconds
        };
        let delay = self.value_named(n, "DelayTime", input, live, memo, depth + 1)?;
        let rise = self.value_named(n, "RiseTime", input, live, memo, depth + 1)?;
        let frequency = self.value_named(n, "Freq", input, live, memo, depth + 1)?;
        let freq = if self.boolean(n, "SyncToHost", false, live)? {
            ensure!(frequency > 0., "Invalid synchronized UVI LFO beat period");
            f64::from((input.host_tempo as f32 * (1_f32 / 60.)) / frequency as f32)
        } else {
            frequency
        };
        let phase = self.value_named(n, "Phase", input, live, memo, depth + 1)?;
        let amplitude = self.value_named(n, "Depth", input, live, memo, depth + 1)?;
        ensure!(
            delay >= 0. && rise >= 0. && freq >= 0. && (0. ..=1.).contains(&amplitude),
            "Invalid UVI LFO parameters"
        );
        let wave = self.setting(n, "WaveFormType", 0., live)?;
        ensure!(
            (0. ..=9.).contains(&wave) && wave.fract() == 0.,
            "Invalid UVI LFO waveform type"
        );
        ensure!(
            wave != 1.
                || !self.node_targets[n].iter().any(|(_, name)| matches!(
                    name.as_str(),
                    "Freq" | "Depth" | "Phase" | "DelayTime" | "RiseTime"
                )),
            "Unverified connected UVI square LFO parameter at node {n}"
        );
        // Square waveform transitions have no measured clock-state law. Check
        // before branching so switching away from square cannot evade the gate.
        if let Some(state) =
            self.triangle_lfo_clocks
                .borrow()
                .get(&(n, input.voice, input.instance))
        {
            ensure!(
                (state.wave != 1. && wave != 1.) || state.wave == wave,
                "Unverified live UVI square LFO waveform change at node {n}"
            );
        }
        if wave == 6. {
            ensure!(
                retrigger == 1. && delay == 0. && rise == 0.,
                "Unverified UVI random LFO delay/rise/trigger mode at node {n}"
            );
            let raw = self.random_lfo(n, input, freq, phase, smooth)?;
            return Ok((if bipolar { raw } else { (raw + 1.) * 0.5 }) * amplitude);
        }
        source_setting_gate(n, "LFO", "Smooth", smooth, smooth == 0.)?;
        if wave == 1. || wave == 2. {
            ensure!(
                wave != 1. || retrigger == 1.,
                "Unverified UVI square LFO trigger mode at node {n}"
            );
            // Authored native squares and triangles use an integer phase accumulator and
            // interpolate 32-frame control points, including triangle corners.
            let rate = input.sample_rate as f32;
            let increment = ((16777216_f32 / rate) * (freq as f32 * 256.)).floor();
            ensure!(
                (0. ..4294967296.).contains(&increment),
                "Unsupported UVI triangle LFO phase increment"
            );
            ensure!(
                input.time_seconds * input.sample_rate < (u64::MAX - 65536) as f64,
                "UVI triangle LFO clock overflow"
            );
            let frame = (input.time_seconds * input.sample_rate + 0.000001).floor() as u64;
            let age = (clock * input.sample_rate + 0.000001).floor() as u64;
            ensure!(age <= frame, "Invalid UVI triangle LFO voice clock");
            let origin = frame - age;
            let duration = |seconds: f64| -> Result<u64> {
                let frames = ((seconds as f32 * rate) / 32.).ceil() * 32.;
                ensure!(
                    frames.is_finite() && frames >= 0. && f64::from(frames) < u64::MAX as f64,
                    "Invalid UVI triangle LFO duration"
                );
                Ok(frames as u64)
            };
            let delay_frames = duration(delay)?;
            let rise_frames = duration(rise)?;
            if age < delay_frames {
                return Ok(0.);
            }
            let start = origin
                .checked_add(delay_frames)
                .context("UVI triangle LFO clock overflow")?;
            let block_frames = u64::from(input.control_block_frames);
            let block_start = frame / block_frames * block_frames;
            let interval_start = block_start.max(start);
            let point_frame = interval_start + (frame - interval_start) / 32 * 32;
            let elapsed = point_frame - start;
            let phase_parameter = phase as f32;
            let phase_start = (phase_parameter.rem_euclid(1.) * 4294967296.) as u32;
            let key = (n, input.voice, input.instance);
            let mut clocks = self.triangle_lfo_clocks.borrow_mut();
            ensure!(
                clocks.len() < LIMIT || clocks.contains_key(&key),
                "UVI triangle LFO state exceeds limit"
            );
            let state = clocks.entry(key).or_insert(TriangleLfoClock {
                wave,
                amplitude: amplitude as f32,
                bipolar,
                frame: point_frame,
                phase: phase_start.wrapping_add((increment as u32).wrapping_mul(elapsed as u32)),
                increment: increment as u32,
                phase_parameter,
                origin,
                delay_frames,
                rise_frames,
                rate: input.sample_rate,
                block_frames: input.control_block_frames,
            });
            ensure!(
                wave != 1.
                    || (state.increment == increment as u32
                        && state.amplitude == amplitude as f32
                        && state.bipolar == bipolar),
                "Unverified live UVI square LFO frequency/depth/polarity change at node {n}"
            );
            ensure!(
                state.origin == origin
                    && state.delay_frames == delay_frames
                    && state.rise_frames == rise_frames
                    && state.phase_parameter == phase_parameter
                    && state.rate == input.sample_rate
                    && state.block_frames == input.control_block_frames,
                "Unverified live UVI triangle LFO phase/timing change at node {n}"
            );
            ensure!(
                point_frame >= state.frame,
                "UVI triangle LFO clock moved backwards"
            );
            if point_frame != state.frame {
                state.phase = state.phase.wrapping_add(
                    state
                        .increment
                        .wrapping_mul((point_frame - state.frame) as u32),
                );
                state.frame = point_frame;
                state.increment = increment as u32;
            }
            let value = |phase, elapsed: u64| {
                let raw = if wave == 1. {
                    square_lfo_value(phase)
                } else {
                    triangle_lfo_value(phase)
                };
                let raw = if bipolar { raw } else { (raw + 1.) * 0.5 };
                let rise = if rise_frames == 0 {
                    1.
                } else {
                    (elapsed as f32 / rise_frames as f32).min(1.)
                };
                raw * amplitude as f32 * rise
            };
            let left = value(state.phase, elapsed);
            // Native last-point Rise lookahead advances one extra source frame;
            // the next host block recomputes its ordinary control point.
            let extra = u64::from(point_frame + 32 == block_start + block_frames);
            let right = value(
                state.phase.wrapping_add(state.increment.wrapping_mul(32)),
                elapsed + 32 + extra,
            );
            return Ok(f64::from(
                left + (right - left) * ((frame - point_frame) as f32 / 32.),
            ));
        }
        if clock < delay {
            return Ok(0.);
        }
        let t = clock - delay;
        // Integrate frequency changes rather than replacing elapsed*time
        // with the new rate (which would jump the waveform's phase).
        let accumulated = {
            let mut clocks = self.lfo_clocks.borrow_mut();
            let key = (n, input.voice, input.instance);
            ensure!(
                clocks.len() < LIMIT || clocks.contains_key(&key),
                "UVI LFO state exceeds limit"
            );
            let state = clocks.entry(key).or_insert(LfoClock {
                time: t,
                frequency: freq,
                phase: (t * freq).rem_euclid(1.),
            });
            ensure!(t >= state.time, "UVI LFO evaluation clock moved backwards");
            state.phase = (state.phase + (t - state.time) * state.frequency).rem_euclid(1.);
            state.time = t;
            state.frequency = freq;
            state.phase
        };
        let pos = (phase + accumulated).rem_euclid(1.);
        let wave = self.setting(n, "WaveFormType", 0., live)?;
        ensure!(
            (0. ..=9.).contains(&wave) && wave.fract() == 0.,
            "Invalid UVI LFO waveform type"
        );
        let value = match wave as u32 {
            0 => (pos * std::f64::consts::TAU).sin(),
            9 => {
                let table = self
                    .tables
                    .get(&n)
                    .context("UVI user LFO table is missing")?;
                let x = pos * table.len() as f64;
                let i = x as usize;
                let f = x - i as f64;
                table[i] * (1. - f) + table[(i + 1) % table.len()] * f
            }
            _ => bail!("Unverified UVI LFO waveform type at node {n}"),
        };
        let value = if bipolar { value } else { (value + 1.) * 0.5 };
        Ok(value * amplitude * if rise == 0. { 1. } else { (t / rise).min(1.) })
    }
}

#[cfg(test)]
mod tests {
    // Authored invariant cases, not new native-fidelity evidence. Central
    // verification must run these and compare the retained old/new corpus.
    #[test]
    fn compiled_target_laws_preserve_physical_endpoints_and_live_writes() {
        for (kind, name, base, ratio, cc, expected) in [
            ("SamplePlayer", "Gain", 2., 0.5, 0, 1.),
            ("SamplePlayer", "Gain", 2., -0.5, 127, 1.),
            ("GainMatrix", "Gain_1_1", 0.5, 0.5, 0, -0.25),
            ("GainMatrix", "Gain_2_1", 0.5, -0.5, 127, -0.25),
            ("DAHDSR", "AttackTime", 2., 0.5, 0, 1.),
            ("DAHDSR", "DecayTime", 2., -0.5, 127, 1.),
            ("AnalogADSR", "AttackTime", 1., 1., 127, 10.),
            ("AnalogADSR", "ReleaseTime", 1., -1., 127, f64::from(0.0001_f32)),
            ("LFO", "Freq", 1., 0.25, 127, 6.),
            ("OnePole", "Freq", 20., 1., 127, 20000.),
            ("XpanderFilter", "Freq", 20000., -1., 127, 20.),
            ("XpanderFilter", "Q", 0.25, 1., 127, 1.),
            ("XpanderFilter", "Fat", 0.25, -0.5, 127, 0.),
            ("DualDelay", "Feedback", 0.25, 1., 127, 1.),
            ("WhiteChorus", "Mix", 0.25, -0.5, 127, 0.),
            ("WaveTableOscillator", "PhaseDistortionAmount", 0.25, 1., 127, 1.),
            ("WaveTableOscillator", "WaveIndex", 0.25, -0.5, 127, 0.),
            ("XpanderFilter", "Drive", 0., 0.25, 127, 10.),
            ("DAHDSR", "DelayTime", 1., 0.25, 127, 3.5),
            ("XpanderFilter", "Bypass", 0., 0.5, 127, 1.),
            ("XpanderFilter", "Bypass", 0., 0.499, 127, 0.),
            ("WhiteChorus", "Speed", 0.1, 1., 127, 1.),
            ("WhiteChorus", "Crossover", 20., 1., 127, 5000.),
            ("WhiteChorus", "Depth", 1., 1., 127, 40.),
            ("DigitalEq", "GainScale", 0., 0.5, 127, 2.),
            ("DigitalEq", "GainScale", 0., -0.5, 127, -2.),
            ("DualDelay", "Mix", 0.25, -1., 127, -0.75),
            ("DualDelayX", "Mix", 0.25, 1., 127, 1.25),
            ("SamplePlayer", "Pitch", 3., 2., 127, 5.),
        ] {
            let xml = format!(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><{kind} SamplePath="authored.wav" {name}="{base}"><Connections><SignalConnection Source="@MIDI CC 1" Destination="{name}" Ratio="{ratio}"/></Connections></{kind}></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#);
            let program = parse_program(&xml).unwrap();
            let mut graph = ModulationGraph::new(&program).unwrap();
            let node = program.nodes.iter().position(|n| n.kind == kind).unwrap();
            let connection = program.connections[0].node;
            let key = (node, name.into());
            let nodes = HashSet::from([node]);
            let mut input = Inputs::default();
            input.controllers[1] = cc;
            let actual = graph.evaluate_nodes(&input, &HashMap::new(), &nodes).unwrap()[&key];
            assert_eq!(actual.to_bits(), expected.to_bits(), "{kind}.{name}");
            // Every pass must still read CURRENT physical values and Ratio;
            // the immutable law must not freeze either at construction.
            for (physical, strength) in [(base + 0.125, -ratio), (base, ratio)] {
                graph.update_live_parameter(node, name, physical).unwrap();
                graph.update_live_parameter(connection, "Ratio", strength).unwrap();
                let live = HashMap::from([(key.clone(), physical), ((connection, "Ratio".into()), strength)]);
                let external = graph.evaluate_nodes(&input, &live, &nodes).unwrap();
                let mut registered = HashMap::new();
                graph.evaluate_registered_nodes_into(&input, &nodes, |p, v| { registered.insert(p.clone(), v.to_bits()); }).unwrap();
                assert_eq!(registered[&key], external[&key].to_bits(), "{kind}.{name}");
            }
        }
    }
    #[test]
    fn compiled_target_laws_preserve_error_order_and_unconnected_dynamic_values() {
        for (kind, name, base, ratio, expected) in [
            ("OnePole", "Freq", "0", "1", "Invalid UVI filter frequency base"),
            ("GainMatrix", "Gain_13_1", "NaN", "1", "Unverified UVI modulation target conversion"),
            ("WhiteChorus", "Unknown", "NaN", "1", "Unverified UVI modulation target conversion"),
        ] {
            let program = parse_program(&format!(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><{kind} SamplePath="authored.wav" {name}="{base}"><Connections><SignalConnection Source="@MIDI CC 1" Destination="{name}" Ratio="{ratio}"/></Connections></{kind}></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#)).unwrap();
            let graph = ModulationGraph::new(&program).unwrap();
            let node = program.nodes.iter().position(|n| n.kind == kind).unwrap();
            let cached = graph.cached_parameter(node, name).unwrap();
            let live = HashMap::from([((program.connections[0].node, "Ratio".into()), f64::NAN)]);
            let mut memo = graph.memo.borrow_mut();
            memo.begin();
            let error = graph.value_cached(&cached.key, Some(cached), &Inputs::default(), Overrides::External(&live), &mut memo, 0).unwrap_err();
            assert!(error.to_string().starts_with(expected), "{error:#}");
        }
        for (kind, name, expected) in [("GainMatrix", "Gain_1_1", 1_f64), ("GainMatrix", "Gain_2_1", 0.), ("SamplePlayer", "Gain", 1.), ("LFO", "Freq", 20.)] {
            let program = parse_program(&format!(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><{kind} SamplePath="authored.wav"><Connections><SignalConnection Source="@MIDI CC 1" Destination="{name}"/></Connections></{kind}></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#)).unwrap();
            let graph = ModulationGraph::new(&program).unwrap();
            let node = program.nodes.iter().position(|n| n.kind == kind).unwrap();
            let mut input = Inputs::default();
            input.controllers[1] = 127;
            assert_eq!(graph.evaluate_nodes(&input, &HashMap::new(), &HashSet::from([node])).unwrap()[&(node, name.into())].to_bits(), expected.to_bits());
        }
        let program = parse_program("<Program/>").unwrap();
        let mut graph = ModulationGraph::new(&program).unwrap();
        let node = program.root;
        // Dynamically registered, unconnected unsupported properties were
        // legal numeric reads before this dispatch change and remain legal.
        for value in [-0., 0.75] {
            graph.update_live_parameter(node, "CustomNumeric", value).unwrap();
            let cached = graph.cached_parameter(node, "CustomNumeric").unwrap();
            let mut memo = graph.memo.borrow_mut();
            memo.begin();
            assert_eq!(graph.value_cached(&cached.key, Some(cached), &Inputs::default(), Overrides::Registered, &mut memo, 0).unwrap().to_bits(), value.to_bits());
            // Depth admission still precedes a previously populated memo hit.
            let error = graph.value_cached(&cached.key, Some(cached), &Inputs::default(), Overrides::Registered, &mut memo, DEPTH).unwrap_err();
            assert_eq!(error.to_string(), "UVI modulation evaluation depth exceeds limit");
        }
    }

    #[test]
    fn native_step_envelope_global_points_and_unmeasured_modes_stay_gated() {
        let levels = (0..16)
            .map(|i| ((i * 37 % 129) as f64 / 128.).to_string())
            .collect::<Vec<_>>()
            .join(" ");
        let xml = format!(
            r#"<Program><ControlSignalSources><StepEnvelope Name="Seq" SyncToHost="1" Retrigger="0" Freq=".25" NumSteps="16" Levels="{levels}"/></ControlSignalSources><Layers><Layer><Keygroups><Keygroup Gain="1"><Connections><SignalConnection Source="$Program/Seq" Destination="Gain" Ratio="1"/></Connections></Keygroup></Keygroups></Layer></Layers></Program>"#
        );
        let program = parse_program(&xml).unwrap();
        let graph = ModulationGraph::new(&program).unwrap();
        let target = program
            .nodes
            .iter()
            .position(|n| n.kind == "Keygroup")
            .unwrap();
        let source = program
            .nodes
            .iter()
            .position(|n| n.kind == "StepEnvelope")
            .unwrap();
        for (frame, expected) in [
            (0, 0.),
            (5999, 0.135498046875),
            (6000, 0.14453125),
            (6001, 0.153564453125),
            (6015, 0.280029296875),
            (11983, 0.424560546875),
            (11999, 0.569091796875),
        ] {
            let input = Inputs {
                time_seconds: frame as f64 / 48000.,
                ..Default::default()
            };
            assert_eq!(
                graph.evaluate(&input, &HashMap::new()).unwrap()[&(target, "Gain".into())],
                expected
            );
        }
        for (name, value) in [
            ("Smooth", 0.1),
            ("InterpolationMode", 1.),
            ("Retrigger", 1.),
            ("Bipolar", 1.),
            ("SyncToHost", 0.),
            ("Depth", 0.5),
            ("ManualTrigger", 1.),
            ("Bypass", 1.),
            ("NumSteps", 8.),
            ("Freq", 0.5),
        ] {
            let overrides = HashMap::from([((source, name.into()), value)]);
            assert!(
                graph.evaluate(&Inputs::default(), &overrides).is_err(),
                "{name}"
            );
        }
        // Global host-position projection survives voice replacement and a
        // caller-position rewind; a new graph owns a new fixed clock setup.
        for voice in [1, 2] {
            let input = Inputs {
                time_seconds: 6000. / 48000.,
                voice: Some(voice),
                instance: Some(u64::from(voice)),
                ..Default::default()
            };
            assert_eq!(
                graph.evaluate(&input, &HashMap::new()).unwrap()[&(target, "Gain".into())],
                0.14453125
            );
        }
        assert_eq!(
            graph.evaluate(&Inputs::default(), &HashMap::new()).unwrap()[&(target, "Gain".into())],
            0.
        );
        let replacement = ModulationGraph::new(&program).unwrap();
        assert_eq!(
            replacement
                .evaluate(&Inputs::default(), &HashMap::new())
                .unwrap()[&(target, "Gain".into())],
            0.
        );
        let tempo_change = Inputs {
            host_tempo: 121.,
            ..Default::default()
        };
        assert!(graph.evaluate(&tempo_change, &HashMap::new()).is_err());
        for (name, value) in [
            ("Smooth", ".1"),
            ("InterpolationMode", "1"),
            ("Retrigger", "1"),
            ("Bipolar", "1"),
            ("SyncToHost", "0"),
            ("Depth", ".5"),
        ] {
            let mut changed = parse_program(&xml).unwrap();
            changed.nodes[source]
                .attributes
                .insert(name.into(), value.into());
            assert!(ModulationGraph::new(&changed).is_err(), "{name}");
        }
    }
    use super::*;
    use crate::uvi::program::parse_program;
    #[test]
    fn native_triangle_lfo_weighted_table_rounding() {
        let program = parse_program(r#"<Program><ControlSignalSources><LFO Name="Osc" WaveFormType="2" Freq="5.5014190673828125" Phase=".499" Depth="1" Bipolar="1" Retrigger="1" Smooth="0"/></ControlSignalSources><Layers><Layer><Keygroups><Keygroup Gain="1"><Connections><SignalConnection Source="$Program/Osc" Destination="Gain" Ratio="1"/></Connections></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let graph = ModulationGraph::new(&program).unwrap();
        let target = program.nodes.iter().position(|node| node.kind == "Keygroup").unwrap();
        let input = Inputs { time_seconds: 64. / 48000., voice_time_seconds: 64. / 48000., voice: Some(1), instance: Some(1), ..Inputs::default() };
        let actual = graph.deltas(&input, &HashMap::new()).unwrap()[&(target, "Gain".into())] as f32;
        // Authored original generator: rate48k, phase.499, block256, frame64.
        assert_eq!(actual.to_bits(), 0xbccf97c0);
    }
    #[test]
    fn indexed_mapper_names_preserve_nearest_scope_and_ambiguity() {
        let mut p = parse_program(r#"<Program><Mappers><ControlSignalMapper Name="Curve">0 1</ControlSignalMapper><ControlSignalMapper Name="Curve">0 1</ControlSignalMapper></Mappers><Layers><Layer><Mappers><ControlSignalMapper Name="Curve">0 1</ControlSignalMapper></Mappers><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a"><Connections><SignalConnection Source="@MIDI CC 1" Destination="Pitch" Mapper="Curve" Ratio="1"/></Connections></SamplePlayer></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let local = p.nodes.iter().enumerate().find(|(id, node)| node.kind == "ControlSignalMapper" && scope(&p, *id).is_some_and(|owner| p.nodes[owner].kind == "Layer")).unwrap().0;
        let graph = ModulationGraph::new(&p).unwrap();
        assert_eq!(graph.connections[&(p.sample_zones[0].player, "Pitch".into())][0].mapper, Some(local));
        p.nodes[local].name = Some("Other".into());
        let error = ModulationGraph::new(&p).err().unwrap();
        assert!(error.to_string().contains("Ambiguous UVI mapper in scope"));
    }
    #[test]
    fn native_square_lfo_table_endpoints_and_wrap() {
        let xml = r#"<Program><ControlSignalSources><LFO Name="Osc" WaveFormType="1" Freq="2" Phase="0" Depth="1" Retrigger="1" Bipolar="0" Smooth="0"/></ControlSignalSources><Layers><Layer><Keygroups><Keygroup Gain="1"><Connections><SignalConnection Source="$Program/Osc" Destination="Gain" Ratio="1"/></Connections></Keygroup></Keygroups></Layer></Layers></Program>"#;
        let program = parse_program(xml).unwrap();
        let graph = ModulationGraph::new(&program).unwrap();
        let group = program
            .nodes
            .iter()
            .position(|n| n.kind == "Keygroup")
            .unwrap();
        // Original authored 48k DC source, including the table's index127
        // transition and final held entry before uint32 wrap.
        for (frame, expected) in [
            (11904, 1.),
            (11936, 0.6833572387695312),
            (11968, 0.3420257568359375),
            (12000, 0.00069427490234375),
            (12032, 0.),
            (23968, 0.),
            (24000, 0.),
            (24001, 0.03125),
            (24031, 0.96875),
            (24032, 1.),
        ] {
            let time = frame as f64 / 48000.;
            let input = Inputs {
                time_seconds: time,
                voice_time_seconds: time,
                voice: Some(1),
                instance: Some(1),
                ..Inputs::default()
            };
            assert_eq!(
                graph.evaluate(&input, &HashMap::new()).unwrap()[&(group, "Gain".into())],
                expected
            );
        }
        assert_eq!(square_lfo_value(127 << 24), 1.);
        assert_eq!(square_lfo_value(128 << 24), -1.);
        assert_eq!(square_lfo_value(u32::MAX), -1.);
    }
    #[test]
    fn square_lfo_unmeasured_smoothing_and_trigger_modes_remain_gated() {
        let xml = r#"<Program><ControlSignalSources><LFO Name="Osc" WaveFormType="1" Freq="2" Retrigger="1" Smooth="0"/></ControlSignalSources><Layers><Layer><Keygroups><Keygroup><Connections><SignalConnection Source="$Program/Osc" Destination="Gain" Ratio="1"/></Connections></Keygroup></Keygroups></Layer></Layers></Program>"#;
        for changed in [
            xml.replace("Smooth=\"0\"", "Smooth=\".01\""),
            xml.replace("Retrigger=\"1\"", "Retrigger=\"0\""),
            xml.replace("Retrigger=\"1\"", "Retrigger=\"2\""),
            xml.replace("WaveFormType=\"1\"", "WaveFormType=\"3\""),
        ] {
            assert!(ModulationGraph::new(&parse_program(&changed).unwrap()).is_err());
        }
        let program = parse_program(xml).unwrap();
        let graph = ModulationGraph::new(&program).unwrap();
        let source = program.nodes.iter().position(|n| n.kind == "LFO").unwrap();
        let connected = xml.replace("Smooth=\"0\"/>",
            "Smooth=\"0\"><Connections><SignalConnection Source=\"@MIDI CC 1\" Destination=\"Freq\" Ratio=\".1\"/></Connections></LFO>");
        assert!(ModulationGraph::new(&parse_program(&connected).unwrap()).is_err());
        graph.evaluate(&Inputs::default(), &HashMap::new()).unwrap();
        let later = Inputs {
            time_seconds: 32. / 48000.,
            voice_time_seconds: 32. / 48000.,
            ..Inputs::default()
        };
        for (name, value) in [("Freq", 4.), ("Depth", 0.5), ("Bipolar", 0.)] {
            assert!(
                graph
                    .evaluate(&later, &HashMap::from([((source, name.into()), value)]))
                    .is_err()
            );
        }
        for (name, value) in [("Smooth", 0.01), ("Retrigger", 0.)] {
            assert!(
                graph
                    .evaluate(
                        &Inputs::default(),
                        &HashMap::from([((source, name.into()), value)])
                    )
                    .is_err()
            );
        }
    }
    #[test]
    fn square_lfo_live_waveform_transitions_remain_gated() {
        for (from, to) in [(1, 2), (2, 1), (1, 0)] {
            let xml = format!(
                r#"<Program><ControlSignalSources><LFO Name="Osc" WaveFormType="{from}" Freq="2" Depth="1" Retrigger="1" Smooth="0"/></ControlSignalSources><Layers><Layer><Keygroups><Keygroup Gain="1"><Connections><SignalConnection Source="$Program/Osc" Destination="Gain" Ratio="1"/></Connections></Keygroup></Keygroups></Layer></Layers></Program>"#
            );
            let program = parse_program(&xml).unwrap();
            let graph = ModulationGraph::new(&program).unwrap();
            let source = program.nodes.iter().position(|n| n.kind == "LFO").unwrap();
            graph.evaluate(&Inputs::default(), &HashMap::new()).unwrap();
            let later = Inputs {
                time_seconds: 32. / 48000.,
                voice_time_seconds: 32. / 48000.,
                ..Inputs::default()
            };
            assert!(
                graph
                    .evaluate(
                        &later,
                        &HashMap::from([((source, "WaveFormType".into()), f64::from(to))])
                    )
                    .is_err(),
                "accepted live waveform {from} -> {to}"
            );
        }
    }
    #[test]
    fn square_lfo_live_selection_cannot_bypass_connected_parameter_gate() {
        let xml = r#"<Program><ControlSignalSources><LFO Name="Osc" WaveFormType="2" Freq="2" Retrigger="1" Smooth="0"><Connections><SignalConnection Source="@MIDI CC 1" Destination="Freq" Ratio=".1"/></Connections></LFO></ControlSignalSources><Layers><Layer><Keygroups><Keygroup Gain="1"><Connections><SignalConnection Source="$Program/Osc" Destination="Gain" Ratio="1"/></Connections></Keygroup></Keygroups></Layer></Layers></Program>"#;
        let program = parse_program(xml).unwrap();
        let graph = ModulationGraph::new(&program).unwrap();
        let source = program
            .nodes
            .iter()
            .position(|node| node.kind == "LFO")
            .unwrap();
        assert!(
            graph
                .evaluate(
                    &Inputs::default(),
                    &HashMap::from([((source, "WaveFormType".into()), 1.)])
                )
                .is_err()
        );
    }
    #[test]
    fn compiled_targets_keep_keyed_results_across_rates_live_growth_and_scope_changes() {
        let p = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="authored_a" Gain="0.8"><Connections><SignalConnection Source="@MIDI CC 1" Destination="Gain" Ratio="0.6"><Connections><SignalConnection Source="@MIDI CC 2" Destination="Ratio" Ratio="0.3"/></Connections></SignalConnection></Connections></SamplePlayer></Oscillators></Keygroup><Keygroup><Oscillators><SamplePlayer SamplePath="authored_b" Gain="0.4"><Connections><SignalConnection Source="@MIDI CC 3" Destination="Gain" Ratio="-0.5"/></Connections></SamplePlayer></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let players = p.nodes.iter().enumerate().filter(|(_, n)| n.kind == "SamplePlayer").map(|(id, _)| id).collect::<Vec<_>>();
        for rate in [1000., 8000., 22050., 44100., 48000., 88200., 96000., 176400., 192000., 384000., 768000.] {
            let mut graph = ModulationGraph::new(&p).unwrap();
            let slots = players.iter().map(|&node| graph.node_target_slots[node].clone()).collect::<Vec<_>>();
            // Appended host-only fields force backing-record growth. Compiled
            // targets must still observe subsequent writes to their original slots.
            for index in 0..1024 {
                graph.update_live_parameter(p.root, &format!("Authored{index}"), index as f64).unwrap();
            }
            for frame in [0, 1, 31, 32, 255, 256, 1023] {
                let mut input = Inputs { sample_rate: rate, time_seconds: frame as f64 / rate, voice_time_seconds: frame as f64 / rate, voice: Some(7), instance: Some(2), ..Inputs::default() };
                input.controllers[1] = (frame % 128) as u8;
                input.controllers[2] = (127 - frame % 128) as u8;
                input.controllers[3] = (frame * 3 % 128) as u8;
                for (index, &node) in players.iter().enumerate() {
                    graph.update_live_parameter(node, "Gain", 0.25 + index as f64 * 0.125).unwrap();
                    assert_eq!(graph.node_target_slots[node], slots[index]);
                    let nodes = HashSet::from([node]);
                    let mut expected = BTreeMap::new();
                    {
                        let mut memo = graph.memo.borrow_mut();
                        memo.begin();
                        for target in &graph.node_targets[node] {
                            let value = graph.value(target, &input, Overrides::Registered, &mut memo, 0).unwrap();
                            expected.insert(target.clone(), value.to_bits());
                        }
                    }
                    let mut actual = BTreeMap::new();
                    graph.evaluate_registered_nodes_into(&input, &nodes, |target, value| { actual.insert(target.clone(), value.to_bits()); }).unwrap();
                    assert_eq!(actual, expected);
                }
            }
        }
    }

    #[test]
    fn cached_helpers_preserve_eager_base_errors_depth_and_zero_emission() {
        let program = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a"/></Oscillators><Inserts><OnePole Freq="NaN"><Connections><SignalConnection Source="@MIDI CC 1" Destination="Freq" Ratio="1"/></Connections></OnePole></Inserts></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut graph = ModulationGraph::new(&program).unwrap();
        let target = (
            program
                .nodes
                .iter()
                .position(|node| node.kind == "OnePole")
                .unwrap(),
            "Freq".into(),
        );
        let nodes = HashSet::from([target.0]);
        let input = Inputs::default();
        let mut live = HashMap::new();
        graph.absolute_audio_bases.insert(target.clone(), 1000.);
        let mut count = 0;
        assert!(
            graph
                .evaluate_nodes_into(&input, &live, &nodes, |_, _| count += 1)
                .is_err()
        );
        assert_eq!(count, 0);
        live.insert(target.clone(), 3.);
        graph
            .evaluate_nodes_into(&input, &live, &nodes, |_, value| {
                assert_eq!(value, 1000.);
                count += 1;
            })
            .unwrap();
        assert_eq!(count, 1);
        // Even a populated memo cannot bypass the recursive depth boundary.
        let mut memo = graph.memo.borrow_mut();
        assert!(
            graph
                .value_named(
                    target.0,
                    "Freq",
                    &input,
                    Overrides::External(&live),
                    &mut memo,
                    DEPTH
                )
                .is_err()
        );
        drop(memo);
        live.clear();
        assert!(
            graph
                .evaluate_nodes_into(&input, &live, &nodes, |_, _| count += 1)
                .is_err()
        );
        assert_eq!(count, 1);
    }
    #[test]
    fn cached_delta_edges_preserve_serialized_floating_sum_order() {
        let program = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a" Pitch="3"><Connections><SignalConnection Source="@MIDI CC 1" Destination="Pitch" Ratio="10000000000000000"/><SignalConnection Source="@MIDI CC 1" Destination="Pitch" Ratio="1"/><SignalConnection Source="@MIDI CC 1" Destination="Pitch" Ratio="-10000000000000000"/></Connections></SamplePlayer></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let graph = ModulationGraph::new(&program).unwrap();
        let target = (program.sample_zones[0].player, "Pitch".into());
        let mut input = Inputs::default();
        input.controllers[1] = 127;
        // This is an authored arithmetic-order regression, not a claim that
        // vendor targets accept such extreme physical modulation ratios.
        assert_eq!(graph.deltas(&input, &HashMap::new()).unwrap()[&target], 0.);
        assert_eq!(
            graph.evaluate(&input, &HashMap::new()).unwrap()[&target],
            3.
        );
    }
    #[test]
    fn indexed_evaluation_reuses_storage_and_emits_only_complete_results() {
        let program = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a" Pitch="3" Gain="1"><Connections><SignalConnection Source="@MIDI CC 1" Destination="Pitch" Ratio="2"><Connections><SignalConnection Source="@MIDI CC 2" Destination="Ratio" Ratio="1"/></Connections></SignalConnection><SignalConnection Source="@MIDI CC 1" Destination="Gain" Ratio=".5"/></Connections></SamplePlayer></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut graph = ModulationGraph::new(&program).unwrap();
        let player = program.sample_zones[0].player;
        let target = (player, "Pitch".into());
        let nodes = HashSet::from([player]);
        let mut input = Inputs::default();
        input.controllers[2] = 127;
        let mut live = HashMap::new();
        let mut output = HashMap::new();
        let (values, stamps, slots) = {
            let memo = graph.memo.borrow();
            (
                memo.values.as_ptr(),
                memo.stamps.as_ptr(),
                memo.values.len(),
            )
        };
        for cc in 0..128 {
            input.controllers[1] = cc;
            live.insert(target.clone(), f64::from(cc));
            let expected = graph.evaluate_nodes(&input, &live, &nodes).unwrap();
            output.clear();
            graph
                .evaluate_nodes_into(&input, &live, &nodes, |p, v| {
                    output.insert(p.clone(), v);
                })
                .unwrap();
            assert_eq!(expected, output);
            assert_eq!(output[&target], f64::from(cc) + 2. * f64::from(cc) / 127.);
        }
        live.clear();
        input.controllers[1] = 0;
        // Epoch wrap must invalidate every prior dependency, including Ratio.
        graph.memo.borrow_mut().epoch = u64::MAX;
        graph
            .evaluate_nodes_into(&input, &live, &nodes, |p, v| {
                output.insert(p.clone(), v);
            })
            .unwrap();
        assert_eq!(output[&target], 3.);
        let memo = graph.memo.borrow();
        assert_eq!(
            (
                memo.values.as_ptr(),
                memo.stamps.as_ptr(),
                memo.values.len()
            ),
            (values, stamps, slots)
        );
        drop(memo);
        let mut emitted = 0;
        input.controllers[0] = 128;
        assert!(
            graph
                .evaluate_nodes_into(&input, &live, &nodes, |_, _| emitted += 1)
                .is_err()
        );
        assert_eq!(emitted, 0);
        input.controllers[0] = 0;
        assert!(
            graph
                .evaluate_nodes_into(
                    &input,
                    &live,
                    &HashSet::from([program.nodes.len()]),
                    |_, _| emitted += 1
                )
                .is_err()
        );
        assert_eq!(emitted, 0);
        // A dependency failure after successful targets must publish nothing.
        let ratio = program.connections[0].node;
        live.insert((ratio, "Bypass".into()), 0.5);
        assert!(
            graph
                .evaluate_nodes_into(&input, &live, &nodes, |_, _| emitted += 1)
                .is_err()
        );
        assert_eq!(emitted, 0);
        live.clear();
        graph
            .evaluate_nodes_into(&input, &live, &nodes, |_, _| emitted += 1)
            .unwrap();
        assert_eq!(emitted, 2);
    }
    #[test]
    fn bound_connection_ratio_slots_keep_live_writes_and_nested_order() {
        let p = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a" Gain="1" Pitch="3"><Connections><SignalConnection Source="@MIDI CC 1" Destination="Gain" Ratio=".5"><Connections><SignalConnection Source="@MIDI CC 2" Destination="Ratio" Ratio=".25"/></Connections></SignalConnection><SignalConnection Source="@MIDI CC 3" Destination="Pitch"><Connections><SignalConnection Source="@MIDI CC 4" Destination="Ratio" Ratio=".75"/></Connections></SignalConnection></Connections></SamplePlayer></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut graph = ModulationGraph::new(&p).unwrap();
        let player = p.sample_zones[0].player;
        let nodes = HashSet::from([player]);
        let outer = p.connections.iter().filter(|c| c.owner == player).map(|c| c.node).collect::<Vec<_>>();
        let bindings = graph.parameter_values.iter().flat_map(|parameter| parameter.edges.iter())
            .map(|edge| (edge.node, edge.ratio_slot)).collect::<Vec<_>>();
        for &(node, slot) in &bindings {
            assert_eq!(graph.parameter_values[slot].key, (node, "Ratio".into()));
        }
        for name in ["Gain", "Pitch"] {
            let parameter = graph.cached_parameter(player, name).unwrap();
            let expected = p.connections.iter().filter(|c| c.owner == player && c.destination == name)
                .map(|c| c.node).collect::<Vec<_>>();
            assert_eq!(parameter.edges.iter().map(|c| c.node).collect::<Vec<_>>(), expected);
        }
        // New unrelated registered fields append slots without moving bindings.
        graph.update_live_parameter(p.root, "AuthoredNewNumeric", 7.).unwrap();
        assert_eq!(graph.parameter_values.iter().flat_map(|p| p.edges.iter())
            .map(|e| (e.node, e.ratio_slot)).collect::<Vec<_>>(), bindings);
        let omitted = graph.cached_parameter(outer[1], "Ratio").unwrap();
        assert!(omitted.number.is_none());
        let mut initial = Inputs::default();
        initial.controllers[4] = 127; // Nested factor = 1; omitted base stays 1.
        {
            let mut memo = graph.memo.borrow_mut();
            memo.begin();
            let named = graph.value_named(outer[1], "Ratio", &initial, Overrides::Registered, &mut memo, 0).unwrap();
            memo.begin();
            let bound = graph.value_cached(&omitted.key, Some(omitted), &initial, Overrides::Registered, &mut memo, 0).unwrap();
            assert_eq!(named.to_bits(), 1f64.to_bits());
            assert_eq!(bound.to_bits(), named.to_bits());
        }
        let mut live = HashMap::new();
        let mut input = Inputs::default();
        for cc in [0, 32, 91, 127] {
            input.controllers[1..=4].fill(cc);
            for (node, value) in [(outer[0], 0.2 + f64::from(cc) / 100.), (outer[1], 2.)] {
                graph.update_live_parameter(node, "Ratio", value).unwrap();
                live.insert((node, "Ratio".into()), value);
            }
            // Compare the bound dispatch to the retained named dispatch on the
            // same graph, including omitted-default and nested Ratio records.
            for &(node, slot) in &bindings {
                for overrides in [Overrides::External(&live), Overrides::Registered] {
                    let cached = &graph.parameter_values[slot];
                    let mut memo = graph.memo.borrow_mut();
                    memo.begin();
                    let named = graph.value_named(node, "Ratio", &input, overrides, &mut memo, 0).unwrap();
                    memo.begin();
                    let bound = graph.value_cached(&cached.key, Some(cached), &input, overrides, &mut memo, 0).unwrap();
                    assert_eq!(bound.to_bits(), named.to_bits());
                    let named_error = graph.value_named(node, "Ratio", &input, overrides, &mut memo, DEPTH).unwrap_err();
                    let bound_error = graph.value_cached(&cached.key, Some(cached), &input, overrides, &mut memo, DEPTH).unwrap_err();
                    assert_eq!(format!("{bound_error:#}"), format!("{named_error:#}"));
                }
            }
            let external = graph.evaluate_nodes(&input, &live, &nodes).unwrap().into_iter()
                .map(|(p, v)| (p, v.to_bits())).collect::<BTreeMap<_, _>>();
            let mut registered = BTreeMap::new();
            graph.evaluate_registered_nodes_into(&input, &nodes, |p, v| { registered.insert(p.clone(), v.to_bits()); }).unwrap();
            assert_eq!(registered, external); // Gain factor and Pitch delta paths.
        }
        // Connection gates still reject before any target is emitted.
        graph.update_live_parameter(outer[0], "Bypass", 0.5).unwrap();
        let mut emitted = 0;
        let error = graph.evaluate_registered_nodes_into(&input, &nodes, |_, _| emitted += 1).unwrap_err();
        assert!(error.to_string().contains("Invalid live UVI modulation Boolean Bypass"));
        assert_eq!(emitted, 0);
    }

    #[test]
    fn compiled_parameters_keep_live_overrides_defaults_and_recursive_mapping() {
        let program = parse_program(r#"<Program><ControlSignalSources><ConstantModulation Name="Unused" Value=".4"/></ControlSignalSources><Mappers><ControlSignalMapper Name="Curve" Min="0" Max="1">0 1</ControlSignalMapper></Mappers><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a" Pitch="3"><Connections><SignalConnection Name="Outer" Source="@MIDI CC 1" Destination="Pitch" Ratio="2" Mapper="Curve"><Connections><SignalConnection Source="@MIDI CC 2" Destination="Ratio" Ratio="1"/></Connections></SignalConnection></Connections></SamplePlayer></Oscillators><Inserts><GainMatrix/><OnePole Freq="NaN"/></Inserts></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let graph = ModulationGraph::new(&program).unwrap();
        let player = program.sample_zones[0].player;
        let id = |kind: &str| {
            program
                .nodes
                .iter()
                .position(|node| node.kind == kind)
                .unwrap()
        };
        let outer = program
            .connections
            .iter()
            .find(|connection| connection.owner == player)
            .unwrap()
            .node;
        let mut input = Inputs::default();
        input.controllers[1] = 127;
        input.controllers[2] = 127;
        let parameter = (player, "Pitch".into());
        let mut live = HashMap::new();
        assert_eq!(graph.evaluate(&input, &live).unwrap()[&parameter], 5.);
        live.insert(parameter.clone(), 7.);
        assert_eq!(graph.evaluate(&input, &live).unwrap()[&parameter], 9.);
        live.insert((outer, "Ratio".into()), 6.);
        assert_eq!(graph.evaluate(&input, &live).unwrap()[&parameter], 13.);
        live.insert((outer, "Inverted".into()), 1.);
        assert_eq!(graph.evaluate(&input, &live).unwrap()[&parameter], 7.);
        live.insert((outer, "Bypass".into()), 1.);
        assert_eq!(graph.evaluate(&input, &live).unwrap()[&parameter], 7.);
        live.clear();
        assert_eq!(graph.evaluate(&input, &live).unwrap()[&parameter], 5.);
        assert_eq!(
            graph
                .base(&(player, "Gain".into()), Overrides::External(&live))
                .unwrap(),
            1.
        );
        assert_eq!(
            graph
                .base(
                    &(id("GainMatrix"), "Gain_1_1".into()),
                    Overrides::External(&live)
                )
                .unwrap(),
            1.
        );
        assert_eq!(
            graph
                .base(
                    &(id("GainMatrix"), "Gain_2_1".into()),
                    Overrides::External(&live)
                )
                .unwrap(),
            0.
        );
        let unused = id("ConstantModulation");
        let custom = (unused, "CustomNumeric".into());
        assert_eq!(
            graph
                .setting(unused, "CustomNumeric", 0.25, Overrides::External(&live))
                .unwrap(),
            0.25
        );
        live.insert(custom.clone(), 0.75);
        assert_eq!(
            graph
                .setting(unused, "CustomNumeric", 0.25, Overrides::External(&live))
                .unwrap(),
            0.75
        );
        live.insert(custom.clone(), 1.);
        assert_eq!(
            graph
                .setting(unused, "CustomNumeric", 0.25, Overrides::External(&live))
                .unwrap(),
            1.
        );
        live.remove(&custom);
        assert_eq!(
            graph
                .setting(unused, "CustomNumeric", 0.25, Overrides::External(&live))
                .unwrap(),
            0.25
        );
        let invalid = (id("OnePole"), "Freq".into());
        assert!(graph.base(&invalid, Overrides::External(&live)).is_err());
        live.insert(invalid.clone(), 1000.);
        assert_eq!(
            graph.base(&invalid, Overrides::External(&live)).unwrap(),
            1000.
        );
        live.remove(&invalid);
        assert!(graph.base(&invalid, Overrides::External(&live)).is_err());
    }
    #[test]
    fn native_triangle_lfo_fixed_phase_control_points_rise_and_preflight() {
        let cases: &[(&str, &[(u64, f64)])] = &[
            (
                "Freq=\"2\"",
                &[
                    (0, 0.5),
                    (31, 0.5025832653),
                    (32, 0.5026666522),
                    (5999, 0.9986665249),
                    (6000, 0.9986666441),
                    (6016, 0.9986693859),
                    (12000, 0.5000054240),
                    (24000, 0.4999891520),
                ],
            ),
            (
                "Freq=\"2\" Phase=\".25\"",
                &[
                    (0, 1.),
                    (32, 0.9973333478),
                    (12000, 0.000005424022675),
                    (24000, 0.9999891520),
                ],
            ),
            (
                "Freq=\"20\"",
                &[
                    (592, 0.9866666794),
                    (600, 0.9900001287),
                    (608, 0.9933335185),
                    (624, 0.9800001979),
                    (2400, 0.4999992251),
                ],
            ),
            (
                "Freq=\"4.1452475\" Depth=\".124\" RiseTime=\"1.0681459\"",
                &[
                    (0, 0.),
                    (32, 0.00003910565283),
                    (255, 0.0003366463643),
                    (256, 0.0003367809113),
                    (2785, 0.006604612805),
                    (2815, 0.006713249721),
                    (2816, 0.006714486983),
                    (51296, 0.07938920707),
                ],
            ),
            (
                "Freq=\"4.1452475\" Depth=\".124\" RiseTime=\"1.0681459\" Bipolar=\"1\"",
                &[
                    (0, 0.5),
                    (255, 0.5000272989),
                    (256, 0.5000273585),
                    (2815, 0.5033097267),
                    (2816, 0.5033108592),
                    (51296, 0.5173891783),
                ],
            ),
            (
                "Freq=\"2\" RiseTime=\".0006666666666666666\"",
                &[(0, 0.), (31, 0.4869583249), (32, 0.5026666522)],
            ),
            (
                "Freq=\"2\" RiseTime=\".001\"",
                &[(32, 0.2513333261), (64, 0.5053333044)],
            ),
            (
                "Freq=\"2\" DelayTime=\".001\"",
                &[(0, 0.), (32, 0.), (64, 0.5), (256, 0.5159999132)],
            ),
        ];
        for (settings, points) in cases {
            let polarity = if settings.contains("Bipolar=") {
                ""
            } else {
                " Bipolar=\"0\""
            };
            let xml = format!(
                r#"<Program><ControlSignalSources><LFO Name="Osc" WaveFormType="2" Retrigger="1" {settings}{polarity}/></ControlSignalSources><Layers><Layer><Keygroups><Keygroup Gain="1"><Connections><SignalConnection Source="$Program/Osc" Destination="Gain" Ratio="1"/></Connections></Keygroup></Keygroups></Layer></Layers></Program>"#
            );
            let program = parse_program(&xml).unwrap();
            let graph = ModulationGraph::new(&program).unwrap();
            let group = program
                .nodes
                .iter()
                .position(|node| node.kind == "Keygroup")
                .unwrap();
            for &(frame, expected) in *points {
                let time = frame as f64 / 48000.;
                let input = Inputs {
                    time_seconds: time,
                    voice_time_seconds: time,
                    voice: Some(1),
                    instance: Some(1),
                    ..Inputs::default()
                };
                let value =
                    graph.evaluate(&input, &HashMap::new()).unwrap()[&(group, "Gain".into())];
                assert!(
                    (value - expected).abs() < 0.0000002,
                    "{settings} at {frame}: {value} != {expected}"
                );
            }
            let unknown = xml.replace("WaveFormType=\"2\"", "WaveFormType=\"4\"");
            assert!(ModulationGraph::new(&parse_program(&unknown).unwrap()).is_err());
        }
    }
    #[test]
    fn native_builtin_random_shared_draws_and_instance_lifetime() {
        let p=parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup Gain="1"><Connections><SignalConnection Source="@Random" Destination="Gain" Ratio="1" Bypass="1"/></Connections><Oscillators><SamplePlayer SamplePath="a"><Connections><SignalConnection Source="@Random" Destination="Pitch" Ratio="12"/></Connections></SamplePlayer></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut graph = ModulationGraph::new(&p).unwrap();
        let mut input = Inputs {
            voice: Some(7),
            instance: Some(1),
            ..Inputs::default()
        };
        graph.builtin_seeds.replace([841, 184]);
        // Native two-Keygroup stems: a shared generator emits alternating
        // draws A,B; each instance holds its value across all target owners.
        for (instance, expected) in [
            0.30737759749103444,
            0.9680597141675171,
            0.8058558741112352,
            0.4788782233480501,
            0.013868807611801832,
            0.2006070338784757,
            0.6545398866256286,
            0.2121805911937416,
            0.11136263763872707,
            0.15232035809647118,
            0.2822447953203625,
            0.7430048573661444,
            0.35629671999884316,
            0.019558610205064587,
        ]
        .into_iter()
        .enumerate()
        {
            input.instance = Some(instance as u64 + 1);
            let raw = graph.builtin_random(&input, true).unwrap();
            assert!((f64::from((raw as f32 + 1.) * 0.5) - expected).abs() < 0.00000003);
            assert_eq!(graph.builtin_random(&input, true).unwrap(), raw);
        }
        input.instance = Some(30);
        let group = p
            .nodes
            .iter()
            .position(|node| node.kind == "Keygroup")
            .unwrap();
        let nodes = HashSet::from([group]);
        graph
            .evaluate_nodes(&input, &HashMap::new(), &nodes)
            .unwrap();
        assert!(
            graph
                .builtin_values
                .borrow()
                .contains_key(&(1, Some(7), Some(30)))
        );
        graph.remove_instance(7, 30);
        assert!(
            !graph
                .builtin_values
                .borrow()
                .contains_key(&(1, Some(7), Some(30)))
        );
        for expected in [
            0.5619995668751752,
            0.5604333834952392,
            0.6591907765148308,
            0.7986306516162879,
            0.8822001911694924,
            0.4466467168432694,
            0.8458700742787615,
        ] {
            input.instance = Some(input.instance.unwrap() + 1);
            assert!((graph.builtin_random(&input, false).unwrap() - expected).abs() < 0.00000003);
        }
        graph.remove_voice(7);
        assert!(graph.builtin_values.borrow().is_empty());
        let p=parse_program(r#"<Program><Connections><SignalConnection Source="@Random" Destination="Gain" Ratio="1"/></Connections></Program>"#).unwrap();
        assert!(ModulationGraph::new(&p).is_err());
    }
    #[test]
    fn native_alternate_counts_keygroup_instances_and_holds_note_value() {
        let p=parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Connections><SignalConnection Source="@Alternate" Destination="Gain" Ratio="1"/></Connections></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut graph = ModulationGraph::new(&p).unwrap();
        for note in 0..7 {
            // Original two-Keygroup stems: A is always active and B silent;
            // alternation belongs to each DSP instance, not MIDI-key parity.
            for (group, expected) in [(0, 1.), (1, -1.)] {
                let input = Inputs {
                    voice: Some(note),
                    instance: Some(u64::from(note) * 2 + group + 1),
                    ..Inputs::default()
                };
                assert_eq!(graph.builtin_alternate(&input).unwrap(), expected);
                assert_eq!(graph.builtin_alternate(&input).unwrap(), expected);
            }
            graph.remove_voice(note);
        }
        assert!(graph.builtin_values.borrow().is_empty());
    }
    #[test]
    fn authored_native_wavetable_mode0_endpoints_keep_live_routes_gated() {
        // Twelve authored native sine-table comparisons are byte-identical
        // after settling: six goals for each destination, including both
        // clipping endpoints and a nonbinary CC value. Moving constant-table
        // captures additionally establish conversion before interpolation.
        for name in ["PhaseDistortionAmount", "WaveIndex"] {
            for (base, ratio, cc, expected) in [
                (0.5, -0.5, 0, 0.5),
                (0.5, -0.5, 64, 0.24803149606299213),
                (0.5, -0.5, 127, 0.),
                (0.25, 0.5, 64, 0.5019685039370079),
                (0.9, 0.5, 127, 1.),
                (0.1, -0.5, 127, 0.),
            ] {
                let p = parse_program(&format!(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><WaveTableOscillator {name}="{base}"><Connections><SignalConnection Source="@MIDI CC 1" Destination="{name}" Ratio="{ratio}" ConnectionMode="0" SignalConnectionVersion="1"/></Connections></WaveTableOscillator></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#)).unwrap();
                let owner = p
                    .nodes
                    .iter()
                    .position(|n| n.kind == "WaveTableOscillator")
                    .unwrap();
                let graph = ModulationGraph::new(&p).unwrap();
                let mut input = Inputs::default();
                input.controllers[1] = cc;
                let values = graph.evaluate(&input, &HashMap::new()).unwrap();
                assert!((values[&(owner, name.into())] - expected).abs() < 1e-12);
                assert!(!supports_target("WaveTableOscillator", name));
                assert!(!supports_absolute_target("WaveTableOscillator", name));
                assert_eq!(graph.unsupported_targets(), [(owner, name.into())]);
            }
        }
    }
    #[test]
    fn native_delay_mix_keeps_raw_goal_until_consumer_clamp() {
        for kind in ["DualDelay", "DualDelayX"] {
            let p=parse_program(&format!(r#"<Program><Inserts><{kind} Mix=".2"><Connections><SignalConnection Source="@MIDI CC 1" Destination="Mix" Ratio="-.5"/></Connections></{kind}></Inserts></Program>"#)).unwrap();
            let effect = p.nodes.iter().position(|node| node.kind == kind).unwrap();
            let graph = ModulationGraph::new(&p).unwrap();
            assert!(graph.unsupported_targets().is_empty());
            for (cc, expected) in [(0, 0.2), (64, -0.05196850393700785), (127, -0.3)] {
                let mut input = Inputs::default();
                input.controllers[1] = cc;
                let values = graph.evaluate(&input, &HashMap::new()).unwrap();
                assert!((values[&(effect, "Mix".into())] - expected).abs() < 0.0000000001);
            }
        }
    }
    #[test]
    fn native_poly_aftertouch_exact_name_and_unipolar_value() {
        let xml = r#"<Program><Layers><Layer><Keygroups><Keygroup Gain="1"><Connections><SignalConnection Source="@PolyAfterTouch" Destination="Gain" Ratio="1"/></Connections></Keygroup></Keygroups></Layer></Layers></Program>"#;
        let p = parse_program(xml).unwrap();
        let graph = ModulationGraph::new(&p).unwrap();
        let group = p
            .nodes
            .iter()
            .position(|node| node.kind == "Keygroup")
            .unwrap();
        for raw in [0., 64. / 127., 1.] {
            let input = Inputs {
                poly_pressure: raw,
                ..Inputs::default()
            };
            let values = graph.evaluate(&input, &HashMap::new()).unwrap();
            assert_eq!(values[&(group, "Gain".into())], raw);
        }
        for invalid in ["@PolyPressure", "@PolyAftertouch"] {
            let p = parse_program(&xml.replace("@PolyAfterTouch", invalid)).unwrap();
            assert!(ModulationGraph::new(&p).is_err());
        }
    }
    #[test]
    fn native_stochastic_points_partial_segments_and_global_scope() {
        let mut source = SmoothRandomClock::new(1866398178, 300., 0.7, true);
        let points = source.controls(256, 48000., true);
        for (index, expected) in [
            (0, 0.5419383645057678),
            (1, 0.47156980633735657),
            (2, 0.74765944480896),
            (3, 0.7814011573791504),
            (4, 0.7124236226081848),
            (7, 0.5312150120735168),
        ] {
            assert!((((points[index] + 1.) * 0.5) as f64 - expected).abs() < 0.0000001);
        }
        assert_eq!(points[7], points[8]);
        let settings = StochasticSettings {
            rate: 300.,
            depth: 0.7,
            step: 0.,
            bias: 0.,
            bipolar: true,
        };
        let mut clock = StochasticClock {
            sample_rate: 48000.,
            block_frames: 256,
            origin: 0,
            cursor: 0,
            source: StochasticSource::Smooth(SmoothRandomClock::new(2057079015, 300., 0.7, true)),
            settings,
            segment: None,
        };
        for (frame, expected) in [
            (0, 0.5282008051872253),
            (12, 0.5376879572868347),
            (13, 0.5526376962661743),
            (32, 0.5306155681610107),
            (45, 0.5155478119850159),
            (77, 0.5073060393333435),
            (224, 0.6338350772857666),
            (256, 0.6554079055786133),
            (288, 0.7138343453407288),
        ] {
            let end = if frame < 13 {
                13
            } else {
                (frame / 256 + 1) * 256
            };
            let raw = clock.advance(frame, Some(end), Some(13), settings).unwrap();
            let normalized = (raw as f32 + 1.) * 0.5;
            assert!(
                (f64::from(normalized) - expected).abs() < 0.00000012,
                "Std frame{frame}: {normalized} != {expected}"
            );
        }
        let mut drunk = DrunkClock::new(1, 0., 100., 100., 1., true, true);
        let mut model = StochasticClock {
            sample_rate: 48000.,
            block_frames: 256,
            origin: 0,
            cursor: 0,
            source: StochasticSource::Drunk(drunk.clone()),
            settings: StochasticSettings {
                rate: 100.,
                depth: 1.,
                step: 100.,
                bias: 1.,
                bipolar: true,
            },
            segment: None,
        };
        for (frame, expected) in [
            (0, 0.5),
            (32, 0.5022222399711609),
            (64, 0.506518542766571),
            (96, 0.5127506256103516),
            (128, 0.5207894444465637),
            (256, 0.5687206387519836),
            (512, 0.7169595956802368),
            (1024, 0.6624647378921509),
        ] {
            let raw = model.advance(frame, None, None, model.settings).unwrap();
            assert!(
                (f64::from((raw as f32 + 1.) * 0.5) - expected).abs() < 0.00000012,
                "Drunk frame{frame}"
            );
        }
        drunk.bipolar = false;
        let points = drunk.controls(13, 48000.);
        assert_eq!(points.len(), 2);
        assert!(points[1] > points[0]);
        for (scope, expected) in [
            ("Program", 0.8390142917633057),
            ("Keygroup", 0.5684139132499695),
        ] {
            let source = r#"<ControlSignalSources><StdRandom Name="Src" Rate="300" Depth=".7" TriggerMode="0" Bipolar="1"/></ControlSignalSources>"#;
            let xml = format!(
                r#"<Program>{}<Layers><Layer Name="L"><Keygroups><Keygroup Name="KG" Gain="1">{}<Connections><SignalConnection Source="{}" Destination="Gain" Ratio="1"/></Connections></Keygroup></Keygroups></Layer></Layers></Program>"#,
                if scope == "Program" { source } else { "" },
                if scope == "Keygroup" { source } else { "" },
                if scope == "Program" {
                    "$Program/Src"
                } else {
                    "$Keygroup/Src"
                }
            );
            let p = parse_program(&xml).unwrap();
            let graph = ModulationGraph::new(&p).unwrap();
            assert!(graph.requires_planned_segments());
            let group = p.nodes.iter().position(|n| n.kind == "Keygroup").unwrap();
            assert!(graph.target_has_dynamic_source(&(group, "Gain".into())));
            let input = Inputs {
                time_seconds: 12000. / 48000.,
                voice: Some(1),
                instance: Some(1),
                ..Inputs::default()
            };
            let values = graph
                .evaluate_nodes(&input, &HashMap::new(), &HashSet::from([group]))
                .unwrap();
            assert!((values[&(group, "Gain".into())] - expected).abs() < 0.00000012);
        }
        let p=parse_program(r#"<Program><ControlSignalSources><Drunk Name="Src" TriggerMode="2"/></ControlSignalSources><Connections><SignalConnection Source="$Program/Src" Destination="Gain" Ratio="1"/></Connections></Program>"#).unwrap();
        let graph = ModulationGraph::new(&p).unwrap();
        assert!(graph.evaluate(&Inputs::default(), &HashMap::new()).is_err());
    }
    #[test]
    fn native_fm_gain_and_pitch_mode_zero() {
        let p=parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><FmOscillator Gain="1" Pitch="0"><Connections><SignalConnection Source="@MIDI CC 1" Destination="Gain" Ratio=".5"/><SignalConnection Source="@MIDI CC 1" Destination="Pitch" Ratio="12"/></Connections></FmOscillator></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let oscillator = p
            .nodes
            .iter()
            .position(|node| node.kind == "FmOscillator")
            .unwrap();
        let graph = ModulationGraph::new(&p).unwrap();
        assert!(graph.unsupported_targets().is_empty());
        for (cc, gain) in [(0, 0.5), (64, 0.751968502998), (127, 1.)] {
            let mut input = Inputs::default();
            input.controllers[1] = cc;
            let values = graph.evaluate(&input, &HashMap::new()).unwrap();
            assert!((values[&(oscillator, "Gain".into())] - gain).abs() < 0.00000003);
            assert_eq!(
                values[&(oscillator, "Pitch".into())],
                12. * f64::from(cc) / 127.
            );
        }
    }
    #[test]
    fn native_organ_pan_key_sign_and_half_range() {
        let p = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Connections><SignalConnection Source="@OrganPan" Destination="Gain" Ratio="1"/></Connections></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let graph = ModulationGraph::new(&p).unwrap();
        for (key, expected) in [
            (0, 0.5),
            (1, -0.4960629940032959),
            (60, 0.26377952098846436),
            (61, -0.25984251499176025),
            (126, 0.0039370059967041016),
            (127, 0.),
        ] {
            let input = Inputs {
                key,
                ..Inputs::default()
            };
            let (raw, bipolar) = graph
                .source(
                    &Source::OrganPan,
                    &input,
                    Overrides::External(&HashMap::new()),
                    &mut MemoScratch::new(0),
                    0,
                )
                .unwrap();
            assert!(bipolar);
            // Gain normalization loses a few low bits when recovering raw
            // values near zero from the independent PCM capture.
            assert!((raw - expected).abs() < 0.00000003);
        }
    }
    #[test]
    fn native_attack_decay_endpoints_and_planned_partial_segment() {
        let p=parse_program(r#"<Program><Layers><Layer Name="Layer"><Keygroups><Keygroup Name="Group" Gain="1"><ControlSignalSources><AttackDecayEnv Name="Env" Attack="0" DecayTime=".2"/></ControlSignalSources><Connections><SignalConnection Source="$Keygroup/Env" Destination="Gain" Ratio="1"/></Connections></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let group = p.nodes.iter().position(|n| n.kind == "Keygroup").unwrap();
        let env = p
            .nodes
            .iter()
            .position(|n| n.kind == "AttackDecayEnv")
            .unwrap();
        let mut graph = ModulationGraph::new(&p).unwrap();
        assert!(graph.requires_planned_segments());
        let nodes = HashSet::from([group]);
        let target = (group, "Gain".into());
        assert!(graph.target_has_dynamic_source(&target));
        let mut live = HashMap::new();
        let mut input = Inputs {
            voice: Some(1),
            instance: Some(1),
            ..Inputs::default()
        };
        for (instance, attack, finish, points) in [
            (
                1,
                0.,
                37120,
                [
                    (32, 0.6583798528),
                    (160, 0.9994615316),
                    (9600, 0.05269504711),
                ],
            ),
            (
                2,
                0.25,
                39424,
                [
                    (32, 0.06193083525),
                    (160, 0.2804397345),
                    (9600, 0.1053846106),
                ],
            ),
            (
                3,
                0.75,
                44032,
                [
                    (32, 0.03123934940),
                    (160, 0.1490772218),
                    (9600, 0.2984586358),
                ],
            ),
        ] {
            input.instance = Some(instance);
            input.note_off_time_seconds = None;
            live.insert((env, "Attack".into()), attack);
            for (frame, expected) in points {
                input.time_seconds = f64::from(frame) / input.sample_rate;
                input.voice_time_seconds = input.time_seconds;
                let value = graph.evaluate_nodes(&input, &live, &nodes).unwrap()[&target];
                assert!(
                    (value - expected).abs() < 0.0000002,
                    "attack={attack} frame={frame} value={value}"
                );
            }
            input.time_seconds = f64::from(finish - 1) / input.sample_rate;
            input.voice_time_seconds = input.time_seconds;
            assert!(!graph.release_finished(&input, &live, &nodes).unwrap());
            input.time_seconds = f64::from(finish) / input.sample_rate;
            input.voice_time_seconds = input.time_seconds;
            assert!(graph.release_finished(&input, &live, &nodes).unwrap());
            assert_eq!(
                graph.evaluate_nodes(&input, &live, &nodes).unwrap()[&target],
                0.
            );
        }
        input.instance = Some(4);
        live.insert((env, "Attack".into()), 0.);
        for (frame, expected) in [
            (0, 0.),
            (96, 0.9742403030),
            (112, 0.9895552397),
            (113, 0.9905124307),
            (114, 0.9908066392),
            (128, 0.9949254990),
            (145, 0.9999269843),
            (256, 0.9764893055),
        ] {
            let end = if frame < 113 {
                113
            } else if frame < 256 {
                256
            } else {
                512
            };
            graph.set_control_segment_end_frame(Some(end));
            input.time_seconds = f64::from(frame) / input.sample_rate;
            input.voice_time_seconds = input.time_seconds;
            input.note_off_time_seconds = (frame >= 113).then_some(113. / input.sample_rate);
            let value = graph.evaluate_nodes(&input, &live, &nodes).unwrap()[&target];
            assert!(
                (value - expected).abs() < 0.0000002,
                "partial frame={frame} value={value}"
            );
        }
        graph.set_control_segment_end_frame(Some(300));
        assert!(
            graph
                .evaluate_nodes(&input, &live, &nodes)
                .unwrap_err()
                .to_string()
                .contains("segment changed")
        );
        graph.remove_instance(1, 4);
        assert!(
            !graph
                .attack_decay_clocks
                .borrow()
                .contains_key(&(env, Some(1), Some(4)))
        );
    }
    #[test]
    fn native_multi_envelope_loops_release_and_block_completion() {
        let p=parse_program(r#"<Program><Layers><Layer Name="Layer"><Keygroups><Keygroup Name="Group" Gain="1"><ControlSignalSources><MultiEnvelope Name="Env" Retrigger="1" LoopStart="0" LoopEnd="2" ReleaseStep="3"><Steps><Step Time=".1" DestLevel="1"/><Step Time=".1" DestLevel=".25"/><Step Time=".1" DestLevel=".75"/><Step Time=".1" DestLevel="0"/></Steps></MultiEnvelope></ControlSignalSources><Connections><SignalConnection Source="$Keygroup/Env" Destination="Gain" Ratio="1"/></Connections></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let group = p.nodes.iter().position(|n| n.kind == "Keygroup").unwrap();
        let env = p
            .nodes
            .iter()
            .position(|n| n.kind == "MultiEnvelope")
            .unwrap();
        let first = p.nodes.iter().position(|n| n.kind == "Step").unwrap();
        let graph = ModulationGraph::new(&p).unwrap();
        let nodes = HashSet::from([group]);
        let target = (group, "Gain".into());
        let mut input = Inputs {
            voice: Some(1),
            instance: Some(1),
            ..Inputs::default()
        };
        let mut live = HashMap::new();
        // Original authored two-segment loop excludes the LoopStart point.
        for (frame, expected) in [
            (4800, 1.),
            (9600, 0.25),
            (14400, 0.75),
            (16800, 0.5),
            (19200, 0.25),
            (21600, 0.5),
            (24000, 0.75),
        ] {
            input.voice_time_seconds = f64::from(frame) / input.sample_rate;
            input.time_seconds = input.voice_time_seconds;
            let value = graph.evaluate_nodes(&input, &live, &nodes).unwrap()[&target];
            assert!(
                (value - expected).abs() < 0.000001,
                "loop frame={frame} value={value}"
            );
        }
        live.insert((first, "Curve".into()), 0.5);
        live.insert((env, "LoopEnd".into()), 0.);
        input.instance = Some(2);
        input.voice_time_seconds = 0.;
        input.time_seconds = 0.;
        graph.evaluate_nodes(&input, &live, &nodes).unwrap();
        input.note_off_time_seconds = Some(113. / input.sample_rate);
        for (frame, expected) in [
            (112, 0.006579224020),
            (113, 0.006635937839),
            (114, 0.006634555291),
            (2513, 0.003317968221),
            (4913, 0.000011016692),
        ] {
            input.note_off_time_seconds = (frame >= 113).then_some(113. / input.sample_rate);
            input.voice_time_seconds = f64::from(frame) / input.sample_rate;
            input.time_seconds = input.voice_time_seconds;
            let value = graph.evaluate_nodes(&input, &live, &nodes).unwrap()[&target];
            assert!(
                (value - expected).abs() < 0.0000001,
                "release frame={frame} value={value}"
            );
        }
        input.voice_time_seconds = 5120. / input.sample_rate;
        input.time_seconds = input.voice_time_seconds;
        assert!(graph.release_finished(&input, &live, &nodes).unwrap());
        assert_eq!(
            graph.evaluate_nodes(&input, &live, &nodes).unwrap()[&target],
            0.
        );
        // Original KG modes0/2 continue looping after noteoff, for both
        // SamplePlayer PlayRelease settings. They have no fabricated gate tail.
        for (instance, mode) in [(3, 0.), (4, 2.)] {
            input.instance = Some(instance);
            input.voice_time_seconds = 256. / input.sample_rate;
            input.time_seconds = input.voice_time_seconds;
            live.insert((env, "Retrigger".into()), mode);
            assert!(!graph.release_finished(&input, &live, &nodes).unwrap());
        }
        live.insert((env, "Smooth".into()), 1.);
        assert!(
            graph
                .evaluate_nodes(&input, &live, &nodes)
                .unwrap_err()
                .to_string()
                .contains("smoothing")
        );
    }
    #[test]
    fn native_ahd_one_shot_fractional_stages_and_velocity() {
        let p = parse_program(r#"<Program><ControlSignalSources><AHD Name="Env" AttackTime=".0011" HoldTime=".0013" DecayTime=".0017" AttackCurve="-.5" DecayCurve=".5"/></ControlSignalSources><Layers><Layer Name="Layer"><Keygroups><Keygroup Name="Group" Gain="1"><Connections><SignalConnection Source="$Program/Env" Destination="Gain" Ratio="1"/></Connections></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let group = p.nodes.iter().position(|n| n.kind == "Keygroup").unwrap();
        let env = p.nodes.iter().position(|n| n.kind == "AHD").unwrap();
        let graph = ModulationGraph::new(&p).unwrap();
        let target = (group, "Gain".into());
        let nodes = HashSet::from([group]);
        assert!(graph.has_release_envelopes(&nodes));
        assert!(graph.target_has_dynamic_source(&target));
        let mut input = Inputs {
            voice: Some(1),
            instance: Some(1),
            ..Inputs::default()
        };
        let mut live = HashMap::new();
        // Original authored PCM observations, including carried fractional
        // stage budgets. These are source levels before scalar gain smoothing.
        for (frame, expected) in [
            (0, 0.),
            (32, 0.8339776397),
            (64, 1.),
            (96, 1.),
            (128, 0.9407931566),
            (160, 0.6069626808),
            (192, 0.),
        ] {
            input.voice_time_seconds = f64::from(frame) / input.sample_rate;
            let value = graph.evaluate_nodes(&input, &live, &nodes).unwrap()[&target];
            assert!(
                (value - expected).abs() < 0.000001,
                "frame={frame} value={value}"
            );
        }
        input.voice_time_seconds = 256. / input.sample_rate;
        assert!(graph.release_finished(&input, &live, &nodes).unwrap());
        live.insert((env, "AttackTime".into()), 0.002);
        live.insert((env, "AttackCurve".into()), 0.5);
        live.insert((env, "HoldTime".into()), 0.005);
        live.insert((env, "DecayTime".into()), 0.04);
        input.instance = Some(5);
        input.voice_time_seconds = 0.;
        graph.evaluate_nodes(&input, &live, &nodes).unwrap();
        for (frame, expected) in [
            (12, 0.0506289303),
            (13, 0.0433179177),
            (45, 0.2251153737),
            (96, 0.8388281465),
        ] {
            input.note_off_time_seconds = (frame >= 13).then_some(13. / input.sample_rate);
            input.voice_time_seconds = f64::from(frame) / input.sample_rate;
            let value = graph.evaluate_nodes(&input, &live, &nodes).unwrap()[&target];
            assert!((value - expected).abs() < 0.000001);
        }
        live.insert((env, "AttackTime".into()), 0.);
        live.insert((env, "HoldTime".into()), 0.1);
        live.insert((env, "DecayTime".into()), 0.2);
        live.insert((env, "VelocityAmount".into()), 1.);
        input.velocity = 64;
        input.note_off_time_seconds = Some(113. / input.sample_rate);
        for (instance, sensitivity, expected) in [
            (2, 0.5, 0.2539525032),
            (3, -0.5, 0.7524453998),
            (4, 0., 0.503937006),
        ] {
            input.instance = Some(instance);
            input.voice_time_seconds = 256. / input.sample_rate;
            live.insert((env, "VelocitySens".into()), sensitivity);
            let value = graph.evaluate_nodes(&input, &live, &nodes).unwrap()[&target];
            assert!((value - expected).abs() < 0.000001);
            assert!(!graph.release_finished(&input, &live, &nodes).unwrap());
        }
    }
    #[test]
    fn native_dahdsr_fractional_stages_and_release_capture() {
        let p = parse_program(r#"<Program><ControlSignalSources><DAHDSR Name="Env" DelayTime="0.001" AttackTime="0.001" HoldTime="0.001" DecayTime="0.001" SustainLevel="0.25" ReleaseTime="0.002"/></ControlSignalSources><Layers><Layer Name="Layer"><Keygroups><Keygroup Name="Group" Gain="1"><Connections><SignalConnection Source="$Program/Env" Destination="Gain" Ratio="1"/></Connections><Oscillators><SamplePlayer Name="Osc" SamplePath="authored.wav"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let group = p.nodes.iter().position(|n| n.kind == "Keygroup").unwrap();
        let env = p.nodes.iter().position(|n| n.kind == "DAHDSR").unwrap();
        let graph = ModulationGraph::new(&p).unwrap();
        let nodes = HashSet::from([group]);
        let target = (group, "Gain".into());
        let mut input = Inputs {
            voice: Some(1),
            instance: Some(1),
            ..Inputs::default()
        };
        let mut live = HashMap::new();
        for (frame, expected) in [
            (32, 0.),
            (64, 0.46875),
            (96, 1.),
            (128, 1.),
            (160, 0.6484375),
            (192, 0.25),
        ] {
            input.voice_time_seconds = f64::from(frame) / input.sample_rate;
            let value = graph.evaluate_nodes(&input, &live, &nodes).unwrap()[&target];
            assert!(
                (value - expected).abs() < 0.000001,
                "stage frame={frame} value={value}"
            );
        }
        for name in ["DelayTime", "HoldTime", "DecayTime"] {
            live.insert((env, name.into()), 0.);
        }
        live.insert((env, "AttackTime".into()), 0.002);
        live.insert((env, "AttackCurve".into()), 0.5);
        live.insert((env, "ReleaseCurve".into()), 0.5);
        for (instance, off, expected) in [(2, 13., 0.0433179177), (3, 49., 0.2586818933)] {
            input.instance = Some(instance);
            input.note_off_time_seconds = None;
            input.voice_time_seconds = 0.;
            graph.evaluate_nodes(&input, &live, &nodes).unwrap();
            input.note_off_time_seconds = Some(off / input.sample_rate);
            input.voice_time_seconds = off / input.sample_rate;
            let value = graph.evaluate_nodes(&input, &live, &nodes).unwrap()[&target];
            assert!(
                (value - expected).abs() < 0.000001,
                "off={off} capture={value}"
            );
            assert!(!graph.release_finished(&input, &live, &nodes).unwrap());
            input.voice_time_seconds = (off + 96.) / input.sample_rate;
            assert_eq!(
                graph.evaluate_nodes(&input, &live, &nodes).unwrap()[&target],
                0.
            );
            input.voice_time_seconds = (off + 128.) / input.sample_rate;
            assert!(graph.release_finished(&input, &live, &nodes).unwrap());
        }
        live.insert((env, "HoldTime".into()), 0.002);
        live.insert((env, "SustainLevel".into()), 0.4);
        live.insert((env, "NoteOffRetrigger".into()), 1.);
        input.instance = Some(4);
        input.note_off_time_seconds = None;
        input.voice_time_seconds = 0.;
        graph.evaluate_nodes(&input, &live, &nodes).unwrap();
        input.note_off_time_seconds = Some(13. / input.sample_rate);
        for (frame, expected) in [
            (13, 0.0433179177),
            (128, 1.),
            (192, 0.6326491237),
            (237, 0.2897369266),
            (255, 0.1695764363),
            (256, 0.1849824935),
            (288, 0.),
        ] {
            input.voice_time_seconds = f64::from(frame) / input.sample_rate;
            let value = graph.evaluate_nodes(&input, &live, &nodes).unwrap()[&target];
            assert!(
                (value - expected).abs() < 0.000001,
                "pending frame={frame} value={value}"
            );
        }
    }
    #[test]
    fn native_absolute_reversed_declaration_delays_audio_one_block() {
        fn authored(reverse: bool) -> Vec<f32> {
            let source = r#"<ConstantModulation Name="Src" Value="0"/>"#;
            let target = r#"<ConstantModulation Name="Target" Value="0"><Connections><SignalConnection Source="$Program/Src" Destination="Value" Ratio="1" ConnectionMode="1" SignalConnectionVersion="1"/></Connections></ConstantModulation>"#;
            let controls = if reverse {
                format!("{target}{source}")
            } else {
                format!("{source}{target}")
            };
            let xml = format!(
                r#"<Program><ControlSignalSources>{controls}</ControlSignalSources><Layers><Layer><Keygroups><Keygroup><Oscillators>
                <SamplePlayer SamplePath="authored.wav" Gain="1"><Connections><SignalConnection Source="$Program/Target" Destination="Gain" Ratio="1"/></Connections></SamplePlayer>
                </Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#
            );
            let p = parse_program(&xml).unwrap();
            let source = p
                .nodes
                .iter()
                .position(|node| node.name.as_deref() == Some("Src"))
                .unwrap();
            let player = p.sample_zones[0].player;
            let mut graph = ModulationGraph::new(&p).unwrap();
            let mut live = HashMap::new();
            let mut input = Inputs::default();
            let mut audio = Vec::new();
            let (mut point, mut endpoint) = (0_f32, 0_f32);
            let alpha = 1_f32 - 0.33_f32.powf(3200_f32 / input.sample_rate as f32);
            for frame in 0..65536 {
                input.time_seconds = f64::from(frame) / input.sample_rate;
                input.voice_time_seconds = input.time_seconds;
                input.voice = None;
                if frame == 32768 {
                    live.insert((source, "Value".into()), 1.);
                }
                live.extend(graph.control_updates(&input, &live).unwrap());
                input.voice = Some(12);
                let values = graph
                    .evaluate_nodes(&input, &live, &HashSet::from([player]))
                    .unwrap();
                if frame % 32 == 0 {
                    point = endpoint;
                    endpoint = point + (values[&(player, "Gain".into())] as f32 - point) * alpha;
                }
                audio.push(point + (endpoint - point) * ((frame % 32) as f32 / 32.));
            }
            audio
        }
        let forward = authored(false);
        let reverse = authored(true);
        // Independently authored native renders prove this exact shift over
        // 61,232 held-note samples. The synthetic graph need retain the same
        // source clock state and delayed producer input, not just getters.
        assert_eq!(&reverse[33024..], &forward[32768..65280]);
        assert_eq!(reverse[33024], 0.);
        for (frame, expected) in [
            (33024, 0.011_372_049_f32),
            (33280, 0.05630897),
            (33792, 0.26161796),
        ] {
            assert!(
                (forward[frame] - expected).abs() < 0.00000015,
                "frame={frame}"
            );
        }
    }
    #[test]
    fn native_script_modulation_sender_scope_follows_source_ancestry() {
        let p = parse_program(r#"<Program><ControlSignalSources><ScriptEventModulation Name="Global" EventId="1" Bipolar="1"/></ControlSignalSources>
            <Layers><Layer Name="A"><ControlSignalSources><ScriptEventModulation Name="LocalA" EventId="1" Bipolar="1"/></ControlSignalSources></Layer>
            <Layer Name="B"><ControlSignalSources><ScriptEventModulation Name="LocalB" EventId="1" Bipolar="1"/></ControlSignalSources></Layer></Layers></Program>"#).unwrap();
        let id = |name| {
            p.nodes
                .iter()
                .position(|node| node.name.as_deref() == Some(name))
                .unwrap()
        };
        let (layer, global, a, b) = (id("A"), id("Global"), id("LocalA"), id("LocalB"));
        for voice in [None, Some(12)] {
            let mut graph = ModulationGraph::new(&p).unwrap();
            graph
                .set_script_modulation_scoped(Some(layer), 1, None, 0.5, 0., voice, 0.)
                .unwrap();
            assert_eq!(graph.ramp(1, Some(12), Some(a)).unwrap().value(0.), 0.5);
            assert!(graph.ramp(1, Some(12), Some(global)).is_none());
            assert!(graph.ramp(1, Some(12), Some(b)).is_none());
            graph
                .set_script_modulation(1, None, 0.75, 0., None, 0.)
                .unwrap();
            for source in [global, a, b] {
                assert_eq!(
                    graph.ramp(1, Some(12), Some(source)).unwrap().value(0.),
                    0.75
                );
            }
            graph
                .set_script_modulation_scoped(Some(layer), 1, None, 1., 100., Some(12), 0.)
                .unwrap();
            assert_eq!(graph.ramp(1, Some(12), Some(a)).unwrap().value(0.05), 0.875);
            assert_eq!(
                graph.ramp(1, Some(12), Some(global)).unwrap().value(0.05),
                0.75
            );
            assert_eq!(graph.ramp(1, Some(12), Some(b)).unwrap().value(0.05), 0.75);
            graph.remove_voice(12);
            assert_eq!(graph.ramp(1, Some(12), Some(a)).unwrap().value(0.05), 0.75);
            assert!(
                graph
                    .set_script_modulation_scoped(Some(global), 1, None, 1., 0., None, 0.)
                    .is_err()
            );
        }
    }
    #[test]
    fn native_absolute_producer_block_callbacks_and_target_converters() {
        let p = parse_program(r#"<Program><ControlSignalSources>
            <ConstantModulation Name="Src" Value="0" Bipolar="0"/>
            <ConstantModulation Name="Target" Value="0.8"><Connections>
              <SignalConnection Source="$Program/Src" Destination="Value" Ratio="1" ConnectionMode="1" SignalConnectionVersion="1"/>
            </Connections></ConstantModulation></ControlSignalSources></Program>"#).unwrap();
        let source = p
            .nodes
            .iter()
            .position(|n| n.name.as_deref() == Some("Src"))
            .unwrap();
        let target = p
            .nodes
            .iter()
            .position(|n| n.name.as_deref() == Some("Target"))
            .unwrap();
        let parameter = (target, "Value".into());
        let mut graph = ModulationGraph::new(&p).unwrap();
        let mut input = Inputs::default();
        let mut live = HashMap::new();
        // Static source zero does not overwrite the serialized target .8.
        assert!(graph.control_updates(&input, &live).unwrap().is_empty());
        assert_eq!(
            graph.evaluate(&input, &live).unwrap()[&parameter],
            f64::from(0.8_f32)
        );
        // At block128 an authored native setter changes source0 to1. The
        // published endpoints were independently read by Lua at next block.
        input.time_seconds = 32768. / input.sample_rate;
        live.insert((source, "Value".into()), 1.);
        for (block, expected) in [
            0.10663980990648,
            0.31733468174934,
            0.51793825626373,
            0.67547661066055,
        ]
        .into_iter()
        .enumerate()
        {
            input.time_seconds = (32768. + block as f64 * 256.) / input.sample_rate;
            let writes = graph.control_updates(&input, &live).unwrap();
            assert_eq!(writes.len(), 1);
            assert_eq!(writes[0].0, parameter);
            assert!((writes[0].1 - expected).abs() < 0.00000008);
            live.extend(writes);
        }
        input.time_seconds += 1. / input.sample_rate;
        assert!(graph.control_updates(&input, &live).unwrap().is_empty());
        live.insert((source, "Value".into()), 0.5);
        assert!(
            graph
                .control_updates(&input, &live)
                .unwrap_err()
                .to_string()
                .contains("nonaligned")
        );
        assert_eq!(absolute_value("OnePole", "Freq", 0.).unwrap(), 20.);
        assert_eq!(absolute_value("OnePole", "Freq", 1.).unwrap(), 20000.);
        assert!((absolute_value("Gain", "Volume", 0.8).unwrap() - 1.).abs() < 0.000001);
        assert_eq!(absolute_value("Gain", "Volume", 0.).unwrap(), 0.);
        assert!((absolute_value("Gain", "Volume", 0.5).unwrap() - 0.05447886).abs() < 0.000001);
        assert!(absolute_value("DigitalEq", "GainScale", 0.5).is_err());
        for (kind, name, normalized, expected) in [
            ("WhiteChorus", "Mix", 0.25, 0.25),
            ("DualDelay", "Mix", 0.25, 0.25),
            ("XpanderFilter", "Q", 0.25, 0.25),
            ("XpanderFilter", "Drive", 0.25, -10.),
            ("WhiteChorus", "Speed", 0.25, 0.1778279394),
            ("WhiteChorus", "Depth", 0.25, 10.75),
            ("WhiteChorus", "Crossover", 0.25, 79.52706909),
            ("GainMatrix", "Gain_1_1", 0.25, -0.5),
            ("SparkVerb", "Mix", 0.25, 0.25),
            ("WaveShaper", "Mix", 0.25, 0.25),
            ("WaveShaper", "Knee", 0.25, -5.),
            ("Layer", "Gain", 0.5, 0.5211858153),
            ("SamplePlayer", "Gain", 0.5, 0.2334075719),
            ("Layer", "Mute", 0.49, 0.),
            ("Layer", "Mute", 0.5, 1.),
            ("XpanderFilter", "Bypass", 0.5, 1.),
            ("CrossOverFilter", "LowFrequency", 0.25, 112.4682693),
            ("CrossOverFilter", "HighFrequency", 0.5, 632.4555054),
            ("ThreeBandShelves", "GainLow", 0.25, -12.),
            ("ThreeBandShelves", "GainHigh", 0.75, 12.),
            ("AnalogADSR", "DecayTime", 0.25, 0.00974126346),
            ("AnalogADSR", "DecayTime", 0.5, 0.10388612747),
            ("AnalogADSR", "DecayTime", 0.75, 1.02319049835),
            ("SignalConnection", "Ratio", 0.25, 0.25),
            ("Phasor", "Depth", 0.25, 0.25),
            ("CrossPhaser", "Depth", 0.75, 0.75),
            ("PhasorFilter", "Depth", 0.5, 0.5),
            ("Tremolo", "Depth", 0.25, 0.25),
        ] {
            assert!(supports_absolute_target(kind, name));
            assert!((absolute_value(kind, name, normalized).unwrap() - expected).abs() < 0.00001);
        }

        for kind in [
            "CrossOverFilter",
            "ThreeBandShelves",
            "Phasor",
            "CrossPhaser",
            "PhasorFilter",
            "Tremolo",
            "Redux",
            "Redux2",
            "AuxEffect",
            "ScriptProcessor",
        ] {
            assert!(supports_absolute_target(kind, "Bypass"));
            for (normalized, expected) in [(0.49, 0.), (0.5, 1.), (1., 1.)] {
                assert_eq!(
                    absolute_value(kind, "Bypass", normalized).unwrap(),
                    expected
                );
            }
        }
        // Nonlinear mapper and signed Ratio distinguish three separate
        // operations: explicit inversion before mapping, then negative-Ratio
        // inversion after mapping. These are native block callback getters.
        for (ratio, inverted, expected) in [(1., 0, 0.), (1., 1, 0.340040326), (-0.5, 0, 0.5)] {
            let fixture = format!(
                r#"<Program><Mappers><ControlSignalMapper Name="Map" Min="0" Max="1">0 0 1</ControlSignalMapper></Mappers>
                <ControlSignalSources><ConstantModulation Name="Src" Value="0.25"/>
                <ConstantModulation Name="Target" Value="0.8"><Connections><SignalConnection Source="$Program/Src" Destination="Value"
                Mapper="Map" Ratio="{ratio}" Inverted="{inverted}" ConnectionMode="1" SignalConnectionVersion="1"/></Connections></ConstantModulation>
                </ControlSignalSources></Program>"#
            );
            let p = parse_program(&fixture).unwrap();
            let source = p
                .nodes
                .iter()
                .position(|n| n.name.as_deref() == Some("Src"))
                .unwrap();
            let mut graph = ModulationGraph::new(&p).unwrap();
            let mut input = Inputs::default();
            assert!(
                graph
                    .control_updates(&input, &HashMap::new())
                    .unwrap()
                    .is_empty()
            );
            input.time_seconds = 256. / input.sample_rate;
            let writes = graph
                .control_updates(&input, &HashMap::from([((source, "Value".into()), 1.)]))
                .unwrap();
            assert_eq!(writes.len(), 1);
            assert!((writes[0].1 - expected).abs() < 0.0000001);
        }
        let cyclic = r#"<Program><ControlSignalSources>
            <ConstantModulation Name="A"><Connections><SignalConnection Source="$Program/B" Destination="Value" ConnectionMode="1" SignalConnectionVersion="1"/></Connections></ConstantModulation>
            <ConstantModulation Name="B"><Connections><SignalConnection Source="$Program/A" Destination="Value" ConnectionMode="1" SignalConnectionVersion="1"/></Connections></ConstantModulation>
            </ControlSignalSources></Program>"#;
        assert!(ModulationGraph::new(&parse_program(cyclic).unwrap()).is_err());
    }
    #[test]
    fn native_random_lfo_clock_and_smoothing() {
        // State recovered from an authored original-render fixture. The
        // process-clock constructor seed is deliberately not asserted.
        let first = 2_306_941_631_u32;
        let mut seed = first
            .wrapping_sub(1_013_904_223)
            .wrapping_mul(4_276_115_653);
        let mut unit = Wave6::default();
        let mut points = Vec::new();
        for _ in 0..40 {
            let controls = unit.controls(&mut seed, 5., 48000., 0., 256);
            assert_eq!(controls[7], controls[8]);
            points.extend_from_slice(&controls[..8]);
        }
        assert_eq!(points[0], 0.07425344);
        assert_eq!(points[9568 / 32], 0.07425344);
        assert_eq!(points[9600 / 32], -0.8711909);
        let mut previous = 0.;
        let mut step = 32;
        let mut points = vec![1.; 9];
        smooth_controls(
            &mut points,
            &[32; 8],
            0.05,
            48000.,
            &mut previous,
            &mut step,
        );
        assert!((points[1] - 0.028870344).abs() < 0.0000002);
        assert_eq!(previous, points[7]);
        assert!(points[8] > previous); // Lookahead is not committed state.
    }
    #[test]
    fn native_analog_control_points_and_partial_release() {
        let p = parse_program(r#"<Program><ControlSignalSources><AnalogADSR Name="Env" AttackTime="0.002" DecayTime="0.2" SustainLevel="0.4" ReleaseTime="0.002"/></ControlSignalSources><Layers><Layer Name="Layer"><Keygroups><Keygroup Name="Group" Gain="1"><Connections><SignalConnection Source="$Program/Env" Destination="Gain" Ratio="1"/></Connections><Oscillators><SamplePlayer Name="Osc" SamplePath="authored.wav"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let group = p.nodes.iter().position(|n| n.kind == "Keygroup").unwrap();
        let graph = ModulationGraph::new(&p).unwrap();
        let target = (group, "Gain".into());
        let nodes = HashSet::from([group]);
        let mut input = Inputs {
            voice: Some(1),
            instance: Some(11),
            ..Inputs::default()
        };
        let live = HashMap::new();
        assert!(graph.has_release_envelopes(&nodes));
        assert!(graph.target_has_dynamic_source(&target));
        for (frame, expected) in [(0, 0.), (96, 0.49999997), (112, 0.56320973)] {
            input.voice_time_seconds = f64::from(frame) / input.sample_rate;
            let value = graph.evaluate_nodes(&input, &live, &nodes).unwrap()[&target];
            assert!(
                (value - expected).abs() < 0.000002,
                "frame={frame} value={value}"
            );
        }
        input.note_off_time_seconds = Some(113. / input.sample_rate);
        input.voice_time_seconds = 113. / input.sample_rate;
        let value = graph.evaluate_nodes(&input, &live, &nodes).unwrap()[&target];
        assert!(
            (value - 0.56928396225).abs() < 0.000002,
            "partial release={value}"
        );
        assert!(!graph.release_finished(&input, &live, &nodes).unwrap());
        input.voice_time_seconds = 145. / input.sample_rate;
        let value = graph.evaluate_nodes(&input, &live, &nodes).unwrap()[&target];
        assert!((value - 0.49731535).abs() < 0.000002);
        input.voice_time_seconds = 1.;
        assert!(graph.release_finished(&input, &live, &nodes).unwrap());
        // A duplicate logical ID has independently timed envelope state.
        input.instance = Some(12);
        input.note_off_time_seconds = None;
        input.voice_time_seconds = 0.;
        assert_eq!(
            graph.evaluate_nodes(&input, &live, &nodes).unwrap()[&target],
            0.
        );
    }
    #[test]
    fn native_graph_nested_mapper_script_controller_and_rejection() {
        let xml = r#"<Program><ControlSignalSources><ConstantModulation Name="Macro" Value="0"/><ScriptEventModulation Name="Script" EventId="2" Bipolar="0"/></ControlSignalSources><Mappers><ControlSignalMapper Name="Curve" Min="-2" Max="3" Integer="0" Discrete="0">-1,000000 0,000000 1,000000</ControlSignalMapper></Mappers><Layers><Layer Name="Layer"><Keygroups><Keygroup Name="Group"><Oscillators><SamplePlayer Name="Osc" SamplePath="authored.wav" Pitch="0"><Connections><SignalConnection Name="Outer" Source="@MIDI CC 1" Destination="Pitch" Ratio="4" Mapper="Curve"><Connections><SignalConnection Source="$Program/Script" Destination="Ratio" Ratio="1"/></Connections></SignalConnection></Connections></SamplePlayer></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#;
        let p = parse_program(xml).unwrap();
        let player = p.sample_zones[0].player;
        let mut graph = ModulationGraph::new(&p).unwrap();
        graph
            .set_script_modulation(101, None, -0.5, 20., Some(12), 0.)
            .unwrap();
        assert!(
            graph
                .set_script_modulation(128, None, 0.5, 20., Some(12), 0.)
                .is_err()
        );
        graph
            .set_script_modulation(2, Some(0.), 1., 100., Some(12), 0.)
            .unwrap();
        let mut input = Inputs {
            voice: Some(12),
            time_seconds: 0.05,
            ..Default::default()
        };
        input.controllers[1] = 127;
        assert_eq!(
            graph.evaluate(&input, &HashMap::new()).unwrap()[&(player, "Pitch".into())],
            6.
        );
        input.controllers[1] = 0;
        assert_eq!(
            graph.evaluate(&input, &HashMap::new()).unwrap()[&(player, "Pitch".into())],
            -4.
        );
        assert_eq!(
            Mapper {
                samples: vec![0., 0.5, 1.],
                min: 0.,
                max: 1.,
                discrete: true,
                integer: false
            }
            .apply(0.4, false),
            0.5
        );
        assert_eq!(
            Mapper {
                samples: vec![-1., 0., 1.],
                min: -2.,
                max: 3.,
                discrete: false,
                integer: true
            }
            .apply(-0.25, true),
            0.
        );
        graph
            .set_script_modulation(2, None, 0.25, 0., None, 0.05)
            .unwrap();
        input.controllers[1] = 127;
        assert_eq!(
            graph.evaluate(&input, &HashMap::new()).unwrap()[&(player, "Pitch".into())],
            3.
        );
        for (kind, name, base, ratios, expected) in [
            ("Gain", "Volume", 1., vec![-1.], 1. - 64. / 127.),
            (
                "Gain",
                "Volume",
                1.,
                vec![0.5, 0.5],
                (0.5 + 0.5 * 64. / 127_f64).powi(2),
            ),
            ("GainMatrix", "Gain_1_1", 1., vec![1.], 2. * 64. / 127. - 1.),
            (
                "OnePole",
                "Freq",
                100.,
                vec![0.5],
                100. * 1000_f64.powf(0.5 * 64. / 127.),
            ),
            ("DigitalEq", "GainScale", 0., vec![0.5], 2. * 64. / 127.),
            ("LFO", "Freq", 1., vec![0.25], 1. + 5. * 64. / 127.),
            (
                "WhiteChorus",
                "Mix",
                0.25,
                vec![0.5],
                0.25 + 0.5 * 64. / 127.,
            ),
            (
                "WhiteChorus",
                "Depth",
                5.,
                vec![0.5],
                5. + 19.5 * 64. / 127.,
            ),
            (
                "WhiteChorus",
                "Speed",
                0.2,
                vec![0.5],
                0.2 * 10_f64.powf(0.5 * 64. / 127.),
            ),
            (
                "WhiteChorus",
                "Crossover",
                20.,
                vec![0.5],
                20. * 250_f64.powf(0.5 * 64. / 127.),
            ),
        ] {
            let connections=ratios.iter().map(|r|format!(r#"<SignalConnection Source="@MIDI CC 1" Destination="{name}" Ratio="{r}"/>"#)).collect::<String>();
            let fixture = format!(
                r#"<Program><Inserts><{kind} Name="Processor" {name}="{base}"><Connections>{connections}</Connections></{kind}></Inserts></Program>"#
            );
            let p = parse_program(&fixture).unwrap();
            let g = ModulationGraph::new(&p).unwrap();
            let id = p.nodes.iter().position(|n| n.kind == kind).unwrap();
            input.controllers[1] = 64;
            assert!(
                (g.evaluate(&input, &HashMap::new()).unwrap()[&(id, name.into())] - expected).abs()
                    < 1e-10
            );
        }
        let lfo_fixture = r#"<Program><ControlSignalSources><LFO Name="Lfo" Freq="1" Depth="0.5" Bipolar="1" Retrigger="1" WaveFormType="9"><UserTable>1 1 1 1</UserTable><Connections><SignalConnection Source="@MIDI CC 1" Destination="Depth" Ratio="1"/></Connections></LFO></ControlSignalSources><Layers><Layer Name="Layer"><Keygroups><Keygroup Name="Group"><Oscillators><SamplePlayer Name="Osc" SamplePath="authored.wav" Pitch="0"><Connections><SignalConnection Source="$Program/Lfo" Destination="Pitch" Ratio="4"/></Connections></SamplePlayer></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#;
        let lfo_fixture = lfo_fixture.replace("1 1 1 1", &vec!["1"; 256].join(" "));
        let p = parse_program(&lfo_fixture).unwrap();
        let player = p.sample_zones[0].player;
        let g = ModulationGraph::new(&p).unwrap();
        input.controllers[1] = 0;
        assert_eq!(
            g.evaluate(&input, &HashMap::new()).unwrap()[&(player, "Pitch".into())],
            0.
        );
        input.controllers[1] = 127;
        assert_eq!(
            g.evaluate(&input, &HashMap::new()).unwrap()[&(player, "Pitch".into())],
            2.
        );
        let triangle = (0..256)
            .map(|i| {
                let i = f64::from(i);
                (if i < 64. {
                    i / 64.
                } else if i < 192. {
                    2. - i / 64.
                } else {
                    i / 64. - 4.
                })
                .to_string()
            })
            .collect::<Vec<_>>()
            .join(" ");
        let fixture = lfo_fixture.replace(&vec!["1"; 256].join(" "), &triangle);
        let p = parse_program(&fixture).unwrap();
        let g = ModulationGraph::new(&p).unwrap();
        let lfo = p.nodes.iter().position(|n| n.kind == "LFO").unwrap();
        let player = p.sample_zones[0].player;
        input.voice_time_seconds = 0.;
        input.time_seconds = 0.;
        assert_eq!(
            g.evaluate_nodes(&input, &HashMap::new(), &HashSet::from([player]))
                .unwrap()[&(player, "Pitch".into())],
            0.
        );
        input.voice_time_seconds = 0.25;
        input.time_seconds = 0.25;
        assert_eq!(
            g.evaluate(&input, &HashMap::new()).unwrap()[&(player, "Pitch".into())],
            2.
        );
        let live = HashMap::from([((lfo, "Freq".into()), 2.)]);
        assert_eq!(
            g.evaluate(&input, &live).unwrap()[&(player, "Pitch".into())],
            2.
        );
        input.voice_time_seconds = 0.375;
        input.time_seconds = 0.375;
        assert_eq!(
            g.evaluate(&input, &live).unwrap()[&(player, "Pitch".into())],
            0.
        );
        let unknown = xml.replace("@MIDI CC 1", "@Unknown");
        assert!(ModulationGraph::new(&parse_program(&unknown).unwrap()).is_err());
        // Native stepped-controller oracle: Constant contributes a separate
        // f32 one-pole before the SamplePlayer target's own gain smoother.
        // Its first held point is zero, second point alpha32=.07124549.
        let constant_xml = r#"<Program><ControlSignalSources><ConstantModulation Name="Step" Value="0" Bipolar="0"><Connections><SignalConnection Source="@MIDI CC 1" Destination="Value" Ratio="1"/></Connections></ConstantModulation></ControlSignalSources><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="authored.wav" Gain="1"><Connections><SignalConnection Source="$Program/Step" Destination="Gain" Ratio="1"/></Connections></SamplePlayer></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#;
        let constant_program = parse_program(constant_xml).unwrap();
        let constant_player = constant_program.sample_zones[0].player;
        let constant_graph = ModulationGraph::new(&constant_program).unwrap();
        let mut step_input = Inputs::default();
        let param = (constant_player, "Gain".into());
        assert_eq!(
            constant_graph
                .evaluate(&step_input, &HashMap::new())
                .unwrap()[&param],
            0.
        );
        step_input.controllers[1] = 127;
        assert_eq!(
            constant_graph
                .evaluate(&step_input, &HashMap::new())
                .unwrap()[&param],
            0.
        );
        for frame in 1..=64 {
            step_input.time_seconds = f64::from(frame) / 48000.;
            let value = constant_graph
                .evaluate(&step_input, &HashMap::new())
                .unwrap()[&param];
            if frame < 32 {
                assert_eq!(value, 0.);
            }
            if frame == 32 {
                assert!((value - 0.071245493).abs() < 0.00000005);
            }
            if frame == 64 {
                assert!((value - 0.137415066).abs() < 0.00000005);
            }
        }
        step_input.time_seconds = 10_000_000. / 48000.;
        assert_eq!(
            constant_graph
                .evaluate(&step_input, &HashMap::new())
                .unwrap()[&param],
            1.
        );

        let partial_graph = ModulationGraph::new(&constant_program).unwrap();
        step_input = Inputs::default();
        partial_graph
            .evaluate(&step_input, &HashMap::new())
            .unwrap();
        step_input.time_seconds = 5. / 48000.;
        step_input.controllers[1] = 127;
        partial_graph
            .evaluate(&step_input, &HashMap::new())
            .unwrap();
        step_input.time_seconds = 32. / 48000.;
        let partial = partial_graph
            .evaluate(&step_input, &HashMap::new())
            .unwrap()[&param];
        // Native outputGain at globalframe36 is .000538418: four samples
        // into gain's next tick, so Constant's previous point is recovered.
        assert!((partial * 0.071245493 * 4. / 32. - 0.000538418).abs() < 0.00000001);

        // Two-stage reference output becomes exactly zero at frame6720
        // with host64 and frame6912 with host256. The Constant stage snaps
        // at5568/5632; the target gain smoother contributes the remaining tail.
        for (block_frames, zero_frame) in [(64, 5568), (256, 5632)] {
            let snap_graph = ModulationGraph::new(&constant_program).unwrap();
            let mut snap_input = Inputs {
                control_block_frames: block_frames,
                ..Default::default()
            };
            snap_input.controllers[1] = 64;
            snap_graph.evaluate(&snap_input, &HashMap::new()).unwrap();
            snap_input.controllers[1] = 0;
            snap_graph.evaluate(&snap_input, &HashMap::new()).unwrap();
            for frame in 1..=zero_frame {
                snap_input.time_seconds = f64::from(frame) / 48000.;
                let value = snap_graph.evaluate(&snap_input, &HashMap::new()).unwrap()[&param];
                if frame < zero_frame {
                    assert!(value > 0.);
                } else {
                    assert_eq!(value, 0.);
                }
            }
        }

        let pressure_program=parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="authored.wav"><Connections><SignalConnection Source="@ChanAfterTouch" Destination="Gain" Ratio="1"/></Connections></SamplePlayer></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let pressure = ModulationGraph::new(&pressure_program).unwrap();
        let pressure_target = (pressure_program.sample_zones[0].player, "Gain".into());
        for (raw, expected) in [(0, 0.), (64, 0.503937006), (127, 1.)] {
            let pressure_input = Inputs {
                channel_pressure: f64::from(raw) / 127.,
                ..Default::default()
            };
            assert!(
                (pressure.evaluate(&pressure_input, &HashMap::new()).unwrap()[&pressure_target]
                    - expected)
                    .abs()
                    < 0.0000001
            );
        }
        let invalid_pressure=parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="authored.wav"><Connections><SignalConnection Source="@ChannelPressure" Destination="Gain" Ratio="1"/></Connections></SamplePlayer></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        assert!(ModulationGraph::new(&invalid_pressure).is_err());
        // Independent original-reference note-source oracle, including event
        // tuning and upper/lower clipping. Values are audible gain factors.
        for (source, note, tune, expected) in [
            ("KeyFollow", 48, 0., 0.32000005),
            ("KeyFollow", 60, 0., 0.5),
            ("KeyFollow", 72, 0., 0.7236068),
            ("KeyFollow", 60, 0.5, 0.545643568),
            ("KeyFollow", 60, -0.5, 0.491701454),
            ("KeyFollow", 0, -12., 0.),
            ("KeyFollow", 127, 12., 1.),
            ("Key", 48, 12., 0.472440958),
            ("Key", 60, 0.5, 0.476377964),
            ("LinearKeyFollow", 60, 0.5, 0.504166663),
        ] {
            let xml = format!(
                r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="authored.wav"><Connections><SignalConnection Source="@VoiceParam {source}" Destination="Gain" Ratio="1"/></Connections></SamplePlayer></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#
            );
            let p = parse_program(&xml).unwrap();
            let g = ModulationGraph::new(&p).unwrap();
            let i = Inputs {
                key: note,
                tune_semitones: tune,
                ..Default::default()
            };
            let v = g.evaluate(&i, &HashMap::new()).unwrap()
                [&(p.sample_zones[0].player, "Gain".into())];
            assert!((v - expected).abs() < 0.0000001, "{source} {note} {tune}");
        }

        let cycle = r#"<Program><ControlSignalSources><ConstantModulation Name="Loop" Value="0"><Connections><SignalConnection Source="$Program/Loop" Destination="Value" Ratio="1"/></Connections></ConstantModulation></ControlSignalSources></Program>"#;
        assert!(ModulationGraph::new(&parse_program(cycle).unwrap()).is_err());
        let unsupported = xml.replace("Destination=\"Pitch\"", "Destination=\"Q\"");
        let p = parse_program(&unsupported).unwrap();
        let g = ModulationGraph::new(&p).unwrap();
        assert!(g.evaluate(&input, &HashMap::new()).is_err());
    }
}

#[cfg(test)]
mod registered_proof {
    use super::*;
    use crate::uvi::program::parse_program;
    #[test]
    fn controller_predicate_matches_scalar_for_every_position_and_byte() {
        let graph = ModulationGraph::new(&fixture()).unwrap();
        let empty = HashMap::new();
        let mut input = Inputs::default();
        input.controllers = std::array::from_fn(|i| (i % 128) as u8);
        let check = |input: &Inputs| {
            let expected = input.controllers.iter().all(|cc| *cc < 128);
            assert_eq!(controllers_valid(&input.controllers), expected);
            for live in [Overrides::Registered, Overrides::External(&empty)] {
                let result = graph.validate(input, live);
                if expected {
                    assert!(result.is_ok());
                } else {
                    assert_eq!(result.unwrap_err().to_string(), "Invalid UVI MIDI modulation inputs");
                }
            }
        };
        for position in 0..128 {
            let saved = input.controllers[position];
            for byte in 0..=u8::MAX {
                input.controllers[position] = byte;
                check(&input);
            }
            input.controllers[position] = saved;
        }
        // Deterministic mixed arrays exercise simultaneous high bits in
        // different lanes, plus arrays whose values all remain admissible.
        let mut state = 0x63ac_0125_8e94_b7d1_u64;
        for round in 0..512 {
            for byte in &mut input.controllers {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                *byte = state as u8;
                if round % 2 == 0 {
                    *byte &= 127;
                }
            }
            check(&input);
        }
    }

    #[test]
    fn controller_predicate_keeps_public_and_registered_error_precedence() {
        let mut graph = ModulationGraph::new(&fixture()).unwrap();
        let nodes = HashSet::from([usize::MAX]);
        let live = HashMap::from([((usize::MAX, "Invalid".into()), f64::NAN)]);
        let mut input = Inputs::default();
        input.controllers[127] = 128;
        input.pitch_bend = f64::NAN;
        input.sample_rate = f64::NAN;
        for expected in [
            "Invalid UVI MIDI modulation inputs",
            "Invalid UVI pressure/bend modulation inputs",
            "Invalid UVI modulation clock",
        ] {
            let mut emitted = 0;
            let external = graph.evaluate_nodes_into(&input, &live, &nodes, |_, _| emitted += 1);
            assert_eq!(external.unwrap_err().to_string(), expected);
            let registered = graph.evaluate_registered_nodes_into(&input, &nodes, |_, _| emitted += 1);
            assert_eq!(registered.unwrap_err().to_string(), expected);
            assert_eq!(emitted, 0);
            match expected {
                "Invalid UVI MIDI modulation inputs" => input.controllers[127] = 127,
                "Invalid UVI pressure/bend modulation inputs" => input.pitch_bend = 0.,
                _ => input.sample_rate = 48000.,
            }
        }
        assert_eq!(graph.evaluate_nodes(&input, &live, &nodes).unwrap_err().to_string(),
            "Invalid UVI live parameter override");
        let mut emitted = 0;
        assert_eq!(graph.evaluate_registered_nodes_into(&input, &nodes, |_, _| emitted += 1)
            .unwrap_err().to_string(), "Invalid UVI modulation target node");
        assert_eq!(emitted, 0);
    }

    fn fixture() -> Program {
        parse_program(r#"<Program><ControlSignalSources><ConstantModulation Name="Unused" Value="0.4"/></ControlSignalSources><Mappers><ControlSignalMapper Name="Curve" Min="0" Max="1">0 1</ControlSignalMapper></Mappers><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a" Pitch="3"><Connections><SignalConnection Name="Outer" Source="@MIDI CC 1" Destination="Pitch" Ratio="2" Mapper="Curve"><Connections><SignalConnection Source="@MIDI CC 2" Destination="Ratio" Ratio="1"/></Connections></SignalConnection><SignalConnection Source="@MIDI CC 3" Destination="Gain" Ratio="0.5"/></Connections></SamplePlayer></Oscillators><Inserts><GainMatrix/><OnePole Freq="NaN"><Connections><SignalConnection Source="@MIDI CC 1" Destination="Freq" Ratio="1"/></Connections></OnePole></Inserts></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap()
    }
    fn bits(
        graph: &mut ModulationGraph,
        input: &Inputs,
        nodes: &HashSet<NodeId>,
    ) -> Result<BTreeMap<Parameter, u64>> {
        let mut out = BTreeMap::new();
        graph.evaluate_registered_nodes_into(input, nodes, |p, v| {
            out.insert(p.clone(), v.to_bits());
        })?;
        Ok(out)
    }
    fn lfo_memo_fixture(wave: u32) -> Program {
        let user = if wave == 9 { format!("<UserTable>{}</UserTable>", (0..256).map(|i| ((i as f64 / 256. * std::f64::consts::TAU).sin()).to_string()).collect::<Vec<_>>().join(" ")) } else { String::new() };
        parse_program(&format!(r#"<Program><ControlSignalSources><LFO Name="L" WaveFormType="{wave}" Freq="2" Phase="-0" Depth="1" Retrigger="1" Smooth="0" Bipolar="1">{user}</LFO></ControlSignalSources><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a"><Connections><SignalConnection Source="$Program/L" Destination="Gain" Ratio="1"/></Connections></SamplePlayer></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#)).unwrap()
    }
    fn lfo_memo_state(graph: &ModulationGraph, node: NodeId, input: &Inputs) -> Vec<u64> {
        let key = (node, input.voice, input.instance);
        let mut bits = Vec::new();
        if let Some(c) = graph.lfo_clocks.borrow().get(&key) {
            bits.extend([c.time.to_bits(), c.frequency.to_bits(), c.phase.to_bits()]);
        }
        if let Some(c) = graph.triangle_lfo_clocks.borrow().get(&key) {
            bits.extend([c.frame, c.phase as u64, c.increment as u64, c.wave.to_bits(), c.origin,
                c.delay_frames, c.rise_frames, c.rate.to_bits(), c.block_frames as u64,
                c.phase_parameter.to_bits() as u64, c.amplitude.to_bits() as u64, c.bipolar as u64]);
        }
        if let Some(c) = graph.random_lfo_clocks.borrow().get(&key) {
            bits.extend([c.block, c.block_start, c.rate.to_bits(), c.block_frames as u64,
                c.unit.phase as u64, c.unit.previous_phase as u64, c.unit.random.to_bits() as u64,
                c.unit.smooth_previous.to_bits() as u64, c.unit.previous_step as u64,
                c.unit.reset_pending as u64, c.unit.phase_parameter.to_bits() as u64]);
            bits.extend(c.controls.iter().map(|v| v.to_bits() as u64));
        }
        if let Some(seed) = graph.random_seeds.borrow().get(&node) { bits.push(*seed as u64); }
        bits
    }
    #[test]
    fn lfo_memo_preserves_repeat_reads_native_clocks_rng_and_voice_context() {
        for wave in [0, 1, 2, 6, 9] {
            let p = lfo_memo_fixture(wave);
            let node = p.nodes.iter().position(|n| n.kind == "LFO").unwrap();
            let mut graph = ModulationGraph::new(&p).unwrap();
            graph.random_seeds.borrow_mut().insert(node, 123);
            if wave == 6 { graph.update_live_parameter(node, "Smooth", 0.032567389).unwrap(); }
            for voice in [1, 2] {
                for frame in [0, 1, 31, 32, 63, 64, 255, 256, 257, 511, 512] {
                    let input = Inputs { voice: Some(voice), instance: Some(voice as u64),
                        key: 60 + voice as u8, velocity: 80 + voice as u8,
                        time_seconds: frame as f64 / 48000., voice_time_seconds: frame as f64 / 48000.,
                        ..Inputs::default() };
                    if wave != 1 { graph.update_live_parameter(node, "Freq", if frame < 64 { 2. } else { 4. }).unwrap(); }
                    let mut memo = graph.memo.borrow_mut();
                    memo.begin();
                    let first = graph.source(&Source::Node(node), &input, Overrides::Registered, &mut memo, 0).unwrap();
                    let before = lfo_memo_state(&graph, node, &input);
                    let repeated = graph.source(&Source::Node(node), &input, Overrides::Registered, &mut memo, 0).unwrap();
                    assert_eq!((first.0.to_bits(), first.1), (repeated.0.to_bits(), repeated.1));
                    assert_eq!(before, lfo_memo_state(&graph, node, &input));
                    memo.sources.clear();
                    let original = graph.source(&Source::Node(node), &input, Overrides::Registered, &mut memo, 0).unwrap();
                    assert_eq!((first.0.to_bits(), first.1), (original.0.to_bits(), original.1));
                    assert_eq!(before, lfo_memo_state(&graph, node, &input));
                    assert!(graph.source(&Source::Node(node), &input, Overrides::Registered, &mut memo, DEPTH - 2).is_err());
                    memo.sources.clear();
                    assert!(graph.source(&Source::Node(node), &input, Overrides::Registered, &mut memo, DEPTH - 2).is_err());
                    assert!(graph.source(&Source::Node(node), &input, Overrides::Registered, &mut memo, DEPTH - 1).is_err());
                }
            }
            graph.update_live_parameter(node, "WaveFormType", 3.).unwrap();
            let mut memo = graph.memo.borrow_mut();
            memo.begin();
            let input = Inputs { time_seconds: 512. / 48000., voice_time_seconds: 512. / 48000.,
                voice: Some(2), instance: Some(2), ..Inputs::default() };
            assert!(graph.source(&Source::Node(node), &input, Overrides::Registered, &mut memo, 0).is_err());
            assert!(!memo.sources.contains_key(&node));
        }
    }
    #[test]
    fn lfo_memo_preserves_derived_sync_overflow_for_each_clock_family() {
        for wave in [0, 1, 2, 6, 9] {
            let p = lfo_memo_fixture(wave);
            let node = p.nodes.iter().position(|n| n.kind == "LFO").unwrap();
            let mut graph = ModulationGraph::new(&p).unwrap();
            graph.random_seeds.borrow_mut().insert(node, 123);
            let input = Inputs { voice: Some(1), instance: Some(1), ..Inputs::default() };
            {
                let mut memo = graph.memo.borrow_mut();
                memo.begin();
                graph.source(&Source::Node(node), &input, Overrides::Registered, &mut memo, 0).unwrap();
            }
            // Finite serialized/override period underflows in the f32 sync law.
            graph.update_live_parameter(node, "Freq", 1e-300).unwrap();
            graph.update_live_parameter(node, "SyncToHost", 1.).unwrap();
            let mut memo = graph.memo.borrow_mut();
            memo.begin();
            let first = graph.source(&Source::Node(node), &input, Overrides::Registered, &mut memo, 0);
            if wave == 1 || wave == 2 {
                assert!(first.is_err());
                assert!(!memo.sources.contains_key(&node));
            } else if wave == 6 {
                let first = first.unwrap();
                let before = lfo_memo_state(&graph, node, &input);
                memo.sources.clear();
                let original = graph.source(&Source::Node(node), &input, Overrides::Registered, &mut memo, 0).unwrap();
                assert_eq!((first.0.to_bits(), first.1), (original.0.to_bits(), original.1));
                assert_eq!(before, lfo_memo_state(&graph, node, &input));
            } else {
                assert!(first.unwrap().0.is_finite());
                assert!(!memo.sources.contains_key(&node));
                assert!(graph.source(&Source::Node(node), &input, Overrides::Registered, &mut memo, 0).is_err());
            }
        }
    }
    #[test]
    fn lfo_memo_declines_f64_signed_zero_frequency_transition() {
        let p = lfo_memo_fixture(0);
        let node = p.nodes.iter().position(|n| n.kind == "LFO").unwrap();
        let mut graph = ModulationGraph::new(&p).unwrap();
        graph.update_live_parameter(node, "Freq", 0.).unwrap();
        let input = Inputs { voice: Some(1), instance: Some(1), ..Inputs::default() };
        graph.lfo_clocks.borrow_mut().insert((node, input.voice, input.instance),
            LfoClock { time: 0., frequency: -0., phase: -0. });
        let mut memo = graph.memo.borrow_mut();
        memo.begin();
        let first = graph.source(&Source::Node(node), &input, Overrides::Registered, &mut memo, 0).unwrap();
        assert_eq!(first.0.to_bits(), (-0f64).to_bits());
        assert!(!memo.sources.contains_key(&node));
        let second = graph.source(&Source::Node(node), &input, Overrides::Registered, &mut memo, 0).unwrap();
        assert_eq!(second.0.to_bits(), 0f64.to_bits());
        let third = graph.source(&Source::Node(node), &input, Overrides::Registered, &mut memo, 0).unwrap();
        assert_eq!(third.0.to_bits(), second.0.to_bits());
    }
    #[test]
    fn source_memo_resets_between_same_frame_voices_inputs_and_live_writes() {
        let p = parse_program(r#"<Program><ControlSignalSources><ConstantModulation Name="C" Value="0.4"/><ScriptEventModulation Name="S" EventId="7" Bipolar="1"/></ControlSignalSources><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a"><Connections><SignalConnection Source="$Program/C" Destination="Gain" Ratio="1"/><SignalConnection Source="$Program/S" Destination="Gain" Ratio="1"/></Connections></SamplePlayer></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let player = p.sample_zones[0].player;
        let constant = p.nodes.iter().position(|n| n.kind == "ConstantModulation").unwrap();
        let script = p.nodes.iter().position(|n| n.kind == "ScriptEventModulation").unwrap();
        let mut graph = ModulationGraph::new(&p).unwrap();
        let mut input = Inputs::default();
        let empty = HashMap::new();
        for (instance, value) in [(1, -0.5), (2, 0.75)] {
            input.instance = Some(instance);
            input.voice = Some(instance as u32);
            input.velocity = 80 + instance as u8;
            input.key = 60 + instance as u8;
            input.controllers[1] = instance as u8;
            input.script_values.insert(7, value);
            graph.evaluate(&input, &empty).unwrap();
            assert_eq!(graph.memo.borrow().sources[&script].0.to_bits(), value.to_bits());
            assert!(graph.constant_clocks.borrow().contains_key(&(constant, input.voice, input.instance)));
        }
        assert_eq!(graph.constant_clocks.borrow().len(), 2);
        let nodes = HashSet::from([player]);
        graph.update_live_parameter(script, "EventId", 8.).unwrap();
        input.script_values.insert(8, -0.25);
        graph.evaluate_registered_nodes_into(&input, &nodes, |_, _| {}).unwrap();
        assert_eq!(graph.memo.borrow().sources[&script].0.to_bits(), (-0.25f64).to_bits());
        // External overrides are a distinct pass at the same frame and voice.
        let external = HashMap::from([((script, "EventId".into()), 9.)]);
        input.script_values.insert(9, 0.5);
        graph.evaluate_nodes_into(&input, &external, &nodes, |_, _| {}).unwrap();
        assert_eq!(graph.memo.borrow().sources[&script].0.to_bits(), 0.5f64.to_bits());
        graph.update_live_parameter(script, "EventId", 128.).unwrap();
        assert!(graph.evaluate_registered_nodes_into(&input, &nodes, |_, _| {}).is_err());
    }
    #[test]
    fn source_memo_keeps_same_frame_state_and_resets_for_each_evaluation() {
        let p = fixture();
        let node = p.nodes.iter().position(|n| n.kind == "ConstantModulation").unwrap();
        let mut graph = ModulationGraph::new(&p).unwrap();
        let source = Source::Node(node);
        let mut input = Inputs::default();
        for frame in [0, 1, 31, 32, 63, 64, 255, 256] {
            input.time_seconds = frame as f64 / input.sample_rate;
            graph.update_live_parameter(node, "Value", if frame < 32 { 0.4 } else { 0.8 }).unwrap();
            let mut memo = graph.memo.borrow_mut();
            memo.begin();
            assert!(memo.sources.is_empty());
            let first = graph.source(&source, &input, Overrides::Registered, &mut memo, 0).unwrap();
            let repeated = graph.source(&source, &input, Overrides::Registered, &mut memo, 0).unwrap();
            memo.sources.clear();
            let original = graph.source(&source, &input, Overrides::Registered, &mut memo, 0).unwrap();
            assert_eq!((first.0.to_bits(), first.1), (repeated.0.to_bits(), repeated.1));
            assert_eq!((first.0.to_bits(), first.1), (original.0.to_bits(), original.1));
            assert!(graph.source(&source, &input, Overrides::Registered, &mut memo, DEPTH - 1).is_err());
        }
        graph.update_live_parameter(node, "Style", 2.).unwrap();
        let mut memo = graph.memo.borrow_mut();
        memo.begin();
        assert!(graph.source(&source, &input, Overrides::Registered, &mut memo, 0).is_err());
        assert!(memo.sources.is_empty());
    }
    #[test]
    fn registered_updates_are_atomic_isolated_and_keep_defaults() {
        let p = fixture();
        let player = p.sample_zones[0].player;
        let unused = p
            .nodes
            .iter()
            .position(|n| n.kind == "ConstantModulation")
            .unwrap();
        let filter = p.nodes.iter().position(|n| n.kind == "OnePole").unwrap();
        let nodes = HashSet::from([player]);
        let input = Inputs::default();
        let mut a = ModulationGraph::new(&p).unwrap();
        let mut b = ModulationGraph::new(&p).unwrap();
        let empty = HashMap::new();
        assert_eq!(
            a.base(&(player, "Gain".into()), Overrides::Registered)
                .unwrap()
                .to_bits(),
            1f64.to_bits()
        );
        assert_eq!(
            a.setting(unused, "CustomNumeric", 0.25, Overrides::Registered)
                .unwrap()
                .to_bits(),
            0.25f64.to_bits()
        );
        a.update_live_parameter(player, "Pitch", -0.).unwrap();
        let key = (player, "Pitch".into());
        let before = bits(&mut a, &input, &nodes).unwrap();
        for (node, value) in [
            (player, f64::NAN),
            (player, f64::INFINITY),
            (player, f64::NEG_INFINITY),
            (p.nodes.len(), 1.),
            (usize::MAX, 0.),
        ] {
            let count = a.memo.borrow().values.len();
            assert!(a.update_live_parameter(node, "Pitch", value).is_err());
            assert_eq!(a.memo.borrow().values.len(), count);
            assert_eq!(bits(&mut a, &input, &nodes).unwrap(), before);
        }
        assert_eq!(
            b.base(&key, Overrides::Registered).unwrap().to_bits(),
            3f64.to_bits()
        );
        assert_eq!(
            a.base(&key, Overrides::External(&empty)).unwrap().to_bits(),
            3f64.to_bits()
        );
        assert_eq!(
            a.base(&key, Overrides::Registered).unwrap().to_bits(),
            (-0f64).to_bits()
        );
        let n = a.memo.borrow().values.len();
        a.update_live_parameter(unused, "CustomNumeric", 0.75)
            .unwrap();
        assert_eq!(a.memo.borrow().values.len(), n + 1);
        let slot = a.cached_parameters[unused]["CustomNumeric"];
        let storage = a.memo.borrow().values.as_ptr();
        for value in [1., -0., 1e300] {
            a.update_live_parameter(unused, "CustomNumeric", value)
                .unwrap();
            assert_eq!(a.memo.borrow().values.len(), n + 1);
            assert_eq!(a.memo.borrow().values.as_ptr(), storage);
            assert_eq!(a.cached_parameters[unused]["CustomNumeric"], slot);
            assert_eq!(
                a.setting(unused, "CustomNumeric", 0.25, Overrides::Registered)
                    .unwrap()
                    .to_bits(),
                value.to_bits()
            );
        }
        assert_eq!(
            b.setting(unused, "CustomNumeric", 0.25, Overrides::Registered)
                .unwrap(),
            0.25
        );
        assert!(
            a.base(&(filter, "Freq".into()), Overrides::Registered)
                .is_err()
        );
        a.update_live_parameter(filter, "Freq", 1000.).unwrap();
        assert_eq!(
            a.base(&(filter, "Freq".into()), Overrides::Registered)
                .unwrap(),
            1000.
        );
        assert!(
            a.base(&(filter, "Freq".into()), Overrides::External(&empty))
                .is_err()
        );
        assert!(
            b.base(&(filter, "Freq".into()), Overrides::Registered)
                .is_err()
        );
        // External insertion/removal never replaces or clears the owned slot.
        let mut external = HashMap::from([(key.clone(), 9.)]);
        assert_eq!(a.base(&key, Overrides::External(&external)).unwrap(), 9.);
        external.remove(&key);
        assert_eq!(a.base(&key, Overrides::External(&external)).unwrap(), 3.);
        assert_eq!(
            a.base(&key, Overrides::Registered).unwrap().to_bits(),
            (-0f64).to_bits()
        );
        // Eager fallback remains an error even when an absolute audio base exists.
        b.absolute_audio_bases
            .insert((filter, "Freq".into()), 1000.);
        let filter_nodes = HashSet::from([filter]);
        let mut emitted = 0;
        assert!(
            b.evaluate_registered_nodes_into(&input, &filter_nodes, |_, _| emitted += 1)
                .is_err()
        );
        assert_eq!(emitted, 0);
        b.update_live_parameter(filter, "Freq", 1000.).unwrap();
        assert!(bits(&mut b, &input, &filter_nodes).is_ok());
    }
    #[test]
    fn registered_results_match_external_bits_for_nested_mapping_and_updates() {
        let p = fixture();
        let player = p.sample_zones[0].player;
        let outer = p
            .connections
            .iter()
            .find(|c| c.owner == player)
            .unwrap()
            .node;
        let external = ModulationGraph::new(&p).unwrap();
        let mut registered = ModulationGraph::new(&p).unwrap();
        let nodes = HashSet::from([player]);
        let mut live = HashMap::new();
        let mut input = Inputs::default();
        for cc in 0..128 {
            input.controllers[1] = cc;
            input.controllers[2] = 127 - cc;
            input.controllers[3] = cc / 2;
            for (node, name, value) in [
                (player, "Pitch", f64::from(cc) - 64.),
                (player, "Gain", f64::from(cc) / 127.),
                (outer, "Ratio", 2. + f64::from(cc) / 20.),
                (outer, "Inverted", f64::from(cc % 2)),
                (outer, "Bypass", f64::from(cc % 3 == 0)),
                (p.root, "AbsentNumeric", f64::from(cc)),
            ] {
                live.insert((node, name.into()), value);
                registered.update_live_parameter(node, name, value).unwrap();
            }
            let expected = external
                .evaluate_nodes(&input, &live, &nodes)
                .unwrap()
                .into_iter()
                .map(|(p, v)| (p, v.to_bits()))
                .collect::<BTreeMap<_, _>>();
            assert_eq!(bits(&mut registered, &input, &nodes).unwrap(), expected);
            let mut emitted = HashMap::new();
            registered
                .evaluate_nodes_into(&input, &live, &nodes, |p, v| {
                    emitted.insert(p.clone(), v.to_bits());
                })
                .unwrap();
            assert_eq!(emitted.into_iter().collect::<BTreeMap<_, _>>(), expected);
        }
        registered
            .update_live_parameter(outer, "Bypass", 0.5)
            .unwrap();
        live.insert((outer, "Bypass".into()), 0.5);
        let mut callbacks = 0;
        assert!(
            registered
                .evaluate_registered_nodes_into(&input, &nodes, |_, _| callbacks += 1)
                .is_err()
        );
        assert_eq!(callbacks, 0);
        assert!(external.evaluate_nodes(&input, &live, &nodes).is_err());
    }
    #[test]
    fn registered_keeps_full_inputs_and_external_map_validation() {
        let p = fixture();
        let player = p.sample_zones[0].player;
        let nodes = HashSet::from([player]);
        let mut graph = ModulationGraph::new(&p).unwrap();
        let valid = Inputs::default();
        let empty = HashMap::new();
        let cases: Vec<(&str, fn(&mut Inputs))> = vec![
            ("key", |i| i.key = 128),
            ("velocity", |i| i.velocity = 128),
            ("unrelated controller", |i| i.controllers[127] = 128),
            ("tune NaN", |i| i.tune_semitones = f64::NAN),
            ("tune infinity", |i| i.tune_semitones = f64::INFINITY),
            ("tune overflow", |i| {
                i.tune_semitones = f64::from(f32::MAX) * 2.
            }),
            ("bend NaN", |i| i.pitch_bend = f64::NAN),
            ("bend low", |i| i.pitch_bend = -1.1),
            ("bend high", |i| i.pitch_bend = 1.1),
            ("channel pressure NaN", |i| i.channel_pressure = f64::NAN),
            ("channel pressure low", |i| i.channel_pressure = -0.1),
            ("channel pressure high", |i| i.channel_pressure = 1.1),
            ("poly pressure NaN", |i| i.poly_pressure = f64::NAN),
            ("poly pressure low", |i| i.poly_pressure = -0.1),
            ("poly pressure high", |i| i.poly_pressure = 1.1),
            ("rate NaN", |i| i.sample_rate = f64::NAN),
            ("rate infinity", |i| i.sample_rate = f64::INFINITY),
            ("rate low", |i| i.sample_rate = 999.),
            ("rate high", |i| i.sample_rate = 768001.),
            ("tempo NaN", |i| i.host_tempo = f64::NAN),
            ("tempo infinity", |i| i.host_tempo = f64::INFINITY),
            ("tempo low", |i| i.host_tempo = -1.),
            ("tempo overflow", |i| {
                i.host_tempo = f64::from(f32::MAX) * 2.
            }),
            ("block low", |i| i.control_block_frames = 31),
            ("block high", |i| i.control_block_frames = 65537),
            ("block unaligned", |i| i.control_block_frames = 33),
            ("time NaN", |i| i.time_seconds = f64::NAN),
            ("time infinity", |i| i.time_seconds = f64::INFINITY),
            ("time negative", |i| i.time_seconds = -1.),
            ("clock overflow", |i| {
                i.time_seconds = u64::MAX as f64 / i.sample_rate
            }),
            ("voice time NaN", |i| i.voice_time_seconds = f64::NAN),
            ("voice time infinity", |i| {
                i.voice_time_seconds = f64::INFINITY
            }),
            ("voice time negative", |i| i.voice_time_seconds = -1.),
            ("off NaN", |i| i.note_off_time_seconds = Some(f64::NAN)),
            ("off infinity", |i| {
                i.note_off_time_seconds = Some(f64::INFINITY)
            }),
            ("off negative", |i| i.note_off_time_seconds = Some(-1.)),
            ("off beyond voice", |i| i.note_off_time_seconds = Some(1.)),
        ];
        for (name, mutate) in cases {
            let mut input = Inputs::default();
            mutate(&mut input);
            let mut external_calls = 0;
            let mut registered_calls = 0;
            let external = graph
                .evaluate_nodes_into(&input, &empty, &nodes, |_, _| external_calls += 1)
                .unwrap_err()
                .to_string();
            let registered = graph
                .evaluate_registered_nodes_into(&input, &nodes, |_, _| registered_calls += 1)
                .unwrap_err()
                .to_string();
            assert_eq!(external, registered, "{name}");
            assert_eq!((external_calls, registered_calls), (0, 0), "{name}");
            assert!(
                graph.release_finished(&input, &empty, &nodes).is_err(),
                "{name}"
            );
            assert!(
                graph.release_registered_finished(&input, &nodes).is_err(),
                "{name}"
            );
        }
        for map in [
            HashMap::from([((p.root, "Unrelated".into()), f64::NAN)]),
            HashMap::from([((p.root, "Unrelated".into()), f64::INFINITY)]),
            HashMap::from([((p.nodes.len(), "Unrelated".into()), 0.)]),
        ] {
            let mut calls = 0;
            assert!(graph.evaluate(&valid, &map).is_err());
            assert!(graph.evaluate_nodes(&valid, &map, &nodes).is_err());
            assert!(
                graph
                    .evaluate_nodes_into(&valid, &map, &nodes, |_, _| calls += 1)
                    .is_err()
            );
            assert!(graph.deltas(&valid, &map).is_err());
            assert!(graph.release_finished(&valid, &map, &nodes).is_err());
            assert_eq!(calls, 0);
            assert!(bits(&mut graph, &valid, &nodes).is_ok());
        }
        let mut count = 0;
        assert!(
            graph
                .evaluate_registered_nodes_into(&valid, &HashSet::from([p.nodes.len()]), |_, _| {
                    count += 1
                })
                .is_err()
        );
        assert_eq!(count, 0);
        assert!(
            graph
                .release_registered_finished(&valid, &HashSet::from([p.nodes.len()]))
                .is_err()
        );
        // Typed enum checks remain at source evaluation, after finite registration.
        let p=parse_program(r#"<Program><ControlSignalSources><LFO Name="L" Freq="1" Depth="1" WaveFormType="0"/></ControlSignalSources><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a"><Connections><SignalConnection Source="$Program/L" Destination="Pitch" Ratio="1"/></Connections></SamplePlayer></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let source = p.nodes.iter().position(|n| n.kind == "LFO").unwrap();
        let mut g = ModulationGraph::new(&p).unwrap();
        g.update_live_parameter(source, "WaveFormType", 0.5)
            .unwrap();
        let mut calls = 0;
        assert!(
            g.evaluate_registered_nodes_into(
                &valid,
                &HashSet::from([p.sample_zones[0].player]),
                |_, _| calls += 1
            )
            .is_err()
        );
        assert_eq!(calls, 0);
    }
}

#[cfg(test)]
mod source_setting_identity_proof {
    use super::*;
    use crate::uvi::program::parse_program;
    fn fixture(kind: &str, attributes: &str) -> Program {
        parse_program(&format!(r#"<Program><ControlSignalSources><{kind} Name="S" {attributes}/></ControlSignalSources><Connections><SignalConnection Source="$Program/S" Destination="Gain" Ratio="1"/></Connections></Program>"#)).unwrap()
    }
    #[test]
    fn typed_source_setting_keeps_exact_wording_value_and_first_gate() {
        for (kind, attributes, parameter, observed, prefix) in [
            ("LFO", "WaveFormType=\"3\" Smooth=\".1\"", "WaveFormType", 3., "Unverified UVI LFO waveform type"),
            ("LFO", "WaveFormType=\"1\" Retrigger=\"0\" Smooth=\".1\"", "Retrigger", 0., "Unverified UVI square LFO trigger mode"),
            ("LFO", "WaveFormType=\"0\" Smooth=\"5.2776863e-09\"", "Smooth", 5.2776863e-09, "Unimplemented UVI deterministic LFO smoothing"),
            ("StepEnvelope", "NumSteps=\"2\" Levels=\"0 1\" SyncToHost=\"0\" Retrigger=\"1\" Smooth=\".1\"", "SyncToHost", 0., "Unverified UVI StepEnvelope SyncToHost"),
            ("StepEnvelope", "NumSteps=\"2\" Levels=\"0 1\" SyncToHost=\"1\" Retrigger=\"0\" Smooth=\".1\"", "Smooth", 0.1, "Unverified UVI StepEnvelope Smooth"),
        ] {
            let program = fixture(kind, attributes);
            let node = program.nodes.iter().position(|n| n.kind == kind).unwrap();
            let error = ModulationGraph::new(&program).err().unwrap();
            assert_eq!(error.to_string(), format!("{prefix} at node {node}"));
            let setting = error.downcast_ref::<UnsupportedSourceSetting>().unwrap();
            assert_eq!(setting.node_in(&program), Some(node));
            assert_eq!(setting.parameter, parameter);
            assert_eq!(setting.observed, observed);
            assert!(setting.observed.is_finite());
        }
    }
    #[test]
    fn source_identity_requires_valid_node_kind_parameter_and_scalar() {
        let program = fixture("LFO", "Smooth=\"0\"");
        let node = program.nodes.iter().position(|n| n.kind == "LFO").unwrap();
        for (node, source_kind, parameter, observed) in [
            (program.nodes.len(), "LFO", "Smooth", 0.1),
            (program.root, "LFO", "Smooth", 0.1),
            (node, "StepEnvelope", "Smooth", 0.1),
            (node, "LFO", "Levels", 0.1),
            (node, "LFO", "Smooth", f64::NAN),
        ] {
            assert_eq!(UnsupportedSourceSetting { node, source_kind, parameter, observed }.node_in(&program), None);
        }
    }
    #[test]
    fn live_source_setting_failure_retains_the_same_scalar_identity() {
        for (kind, attributes) in [
            ("LFO", "WaveFormType=\"0\" Smooth=\"0\""),
            ("StepEnvelope", "SyncToHost=\"1\" Retrigger=\"0\" NumSteps=\"2\" Levels=\"0 1\" Smooth=\"0\""),
        ] {
            let program = fixture(kind, attributes);
            let node = program.nodes.iter().position(|n| n.kind == kind).unwrap();
            let graph = ModulationGraph::new(&program).unwrap();
            let error = graph.evaluate(&Inputs::default(), &HashMap::from([((node, "Smooth".into()), 0.1)])).unwrap_err();
            let setting = error.downcast_ref::<UnsupportedSourceSetting>().unwrap();
            assert_eq!((setting.node_in(&program), setting.parameter, setting.observed), (Some(node), "Smooth", 0.1));
        }
    }
    fn step_projection_fixture() -> (ModulationGraph, NodeId, NodeId) {
        let levels = (0..16).map(|i| ((i * 37 % 129) as f64 / 128.).to_string())
            .collect::<Vec<_>>().join(" ");
        let program = parse_program(&format!(r#"<Program><ControlSignalSources><StepEnvelope Name="Seq" SyncToHost="1" Retrigger="0" Freq=".25" NumSteps="16" Levels="{levels}"/></ControlSignalSources><Layers><Layer><Keygroups><Keygroup Gain="1"><Connections><SignalConnection Source="$Program/Seq" Destination="Gain" Ratio="1"/></Connections></Keygroup></Keygroups></Layer></Layers></Program>"#)).unwrap();
        let source = program.nodes.iter().position(|n| n.kind == "StepEnvelope").unwrap();
        let target = program.nodes.iter().position(|n| n.kind == "Keygroup").unwrap();
        (ModulationGraph::new(&program).unwrap(), source, target)
    }
    #[test]
    fn step_projection_reuses_prepared_native_points_across_queries_and_rewinds() {
        let (graph, source, target) = step_projection_fixture();
        // Existing authored native point/interpolation observations; no new native run.
        for (frame, expected) in [(5999, 0.135498046875), (6000, 0.14453125),
            (6001, 0.153564453125), (6015, 0.280029296875),
            (11999, 0.569091796875), (6000, 0.14453125), (0, 0.)] {
            for instance in [1, 2] {
                let input = Inputs { time_seconds: frame as f64 / 48000.,
                    voice: Some(instance as u32), instance: Some(instance), ..Default::default() };
                assert_eq!(graph.evaluate(&input, &HashMap::new()).unwrap()[&(target, "Gain".into())], expected);
                let cache = graph.step_projection.borrow();
                assert_eq!(cache.len(), 1);
                assert_eq!(cache[&source].key.start, frame / 256 * 256);
            }
        }
    }
    #[test]
    fn step_projection_host_keys_and_numeric_writes_cannot_hide_source_gates() {
        let (mut graph, source, target) = step_projection_fixture();
        let mut input = Inputs { host_position: Some(HostPosition { frame: 0, beat: 2., playing: true }),
            ..Default::default() };
        graph.evaluate(&input, &HashMap::new()).unwrap();
        let key = graph.step_projection.borrow()[&source].key;
        input.host_position.as_mut().unwrap().beat = 3.;
        graph.evaluate(&input, &HashMap::new()).unwrap();
        assert!(graph.step_projection.borrow()[&source].key != key);
        let key = graph.step_projection.borrow()[&source].key;
        input.host_tempo = 137.;
        graph.evaluate(&input, &HashMap::new()).unwrap();
        assert!(graph.step_projection.borrow()[&source].key != key);
        graph.update_live_parameter(source, "Depth", 0.5).unwrap();
        assert!(graph.step_projection.borrow().is_empty());
        let error = graph.evaluate_nodes_emit(&input, Overrides::Registered,
            &HashSet::from([target]), |_, _| {}).unwrap_err();
        assert!(error.to_string().contains("Unverified UVI StepEnvelope Depth"));
        // External overrides also run the existing gates before a prepared hit.
        assert!(graph.evaluate(&input, &HashMap::from([((source, "Smooth".into()), 0.1)])).is_err());
    }
    #[test]
    fn step_projection_does_not_move_future_phase_failure_to_the_first_query() {
        let (graph, source, _) = step_projection_fixture();
        let increment = 32. * (120_f64 * (1. / 60.) / 48000.) * (1. / 0.25);
        let beat = (f64::from(i32::MAX) - 2.5 * increment) * 0.25;
        let mut input = Inputs { host_position: Some(HostPosition { frame: 0, beat, playing: true }),
            ..Default::default() };
        assert!(graph.evaluate(&input, &HashMap::new()).is_ok());
        input.time_seconds = 32. / 48000.;
        assert!(graph.evaluate(&input, &HashMap::new()).is_ok());
        input.time_seconds = 64. / 48000.;
        let error = graph.evaluate(&input, &HashMap::new()).unwrap_err();
        assert_eq!(error.to_string(), "UVI StepEnvelope phase overflow");
        assert_eq!(graph.step_projection.borrow().len(), 1);
        // Non256 block widths retain the original uncached path.
        let (other, _, _) = step_projection_fixture();
        input.time_seconds = 0.;
        input.control_block_frames = 64;
        assert!(other.evaluate(&input, &HashMap::new()).is_ok());
        assert!(other.step_projection.borrow().is_empty());
        assert_eq!(graph.step_projection.borrow()[&source].key.count, 16);
    }

    // Explicit legacy scalar path for UNRUN cache-equivalence cases, not a native oracle.
    fn legacy_step_projection(graph: &ModulationGraph, n: NodeId, input: &Inputs,
        live: Overrides<'_>) -> Result<f64> {
        let values = graph.tables.get(&n).context("UVI StepEnvelope Levels are missing")?;
        let count = graph.setting(n, "NumSteps", 16., live)? as usize;
        let frequency = f64::from(graph.setting(n, "Freq", 1., live)? as f32);
        ensure!(input.time_seconds * input.sample_rate < (u64::MAX - 65536) as f64,
            "UVI StepEnvelope clock overflow");
        let frame = (input.time_seconds * input.sample_rate + 0.000001).floor() as u64;
        let block = u64::from(input.control_block_frames);
        let block_start = frame / block * block;
        let (start, beat) = if let Some(position) = input.host_position {
            let start = block_start.max(position.frame);
            (start, position.beat + (start - position.frame) as f64 / input.sample_rate * input.host_tempo / 60.)
        } else { (block_start, block_start as f64 / input.sample_rate * input.host_tempo / 60.) };
        let tick = (frame - start) / 32;
        let mut phase = beat / frequency;
        let increment = 32. * (f64::from(input.host_tempo as f32) * (1. / 60.)
            / f64::from(input.sample_rate as f32)) * (1. / frequency);
        for _ in 0..tick { phase += increment; }
        ensure!(phase.is_finite() && phase >= 0. && phase + increment < f64::from(i32::MAX),
            "UVI StepEnvelope phase overflow");
        let left = values[(phase.floor() as u64 % count as u64) as usize] as f32;
        let right = values[((phase + increment).floor() as u64 % count as u64) as usize] as f32;
        Ok(f64::from(left + (right - left) * ((frame - start - tick * 32) as f32 / 32.)))
    }
    #[test]
    fn step_prepared_projection_matches_explicit_legacy_for_every_frame_and_boundary() {
        for rate in [44100., 48000., 96000.] {
            for hosted in [false, true] {
                let (graph, source, _) = step_projection_fixture();
                let mut input = Inputs { sample_rate: rate, host_tempo: 137., ..Default::default() };
                for frame in 0..768 {
                    input.time_seconds = frame as f64 / rate;
                    if hosted && frame % 256 == 0 {
                        input.host_position = Some(HostPosition { frame,
                            beat: [0., 2.5, 0.125][frame as usize / 256], playing: true });
                        input.host_tempo = [137., 90., 120.][frame as usize / 256];
                    }
                    graph.step_gate(source, &input, Overrides::External(&HashMap::new())).unwrap();
                    let old = legacy_step_projection(&graph, source, &input, Overrides::External(&HashMap::new())).unwrap();
                    let new = graph.step(source, &input, Overrides::External(&HashMap::new())).unwrap();
                    assert_eq!(new.to_bits(), old.to_bits(), "frame={frame},rate={rate},hosted={hosted}");
                }
                // Rewind replaces the block, preserving the original global projection.
                input.time_seconds = 0.;
                input.host_position = hosted.then_some(HostPosition { frame: 0, beat: -0., playing: true });
                assert_eq!(graph.step(source, &input, Overrides::External(&HashMap::new())).unwrap().to_bits(),
                    legacy_step_projection(&graph, source, &input, Overrides::External(&HashMap::new())).unwrap().to_bits());
            }
        }
        let (graph, source, _) = step_projection_fixture();
        let increment = 32. * (120_f64 * (1. / 60.) / 48000.) * (1. / 0.25);
        let mut input = Inputs { host_position: Some(HostPosition { frame: 0,
            beat: (f64::from(i32::MAX) - 2.5 * increment) * 0.25, playing: true }), ..Default::default() };
        for frame in 0..96 {
            input.time_seconds = frame as f64 / 48000.;
            let old = legacy_step_projection(&graph, source, &input, Overrides::External(&HashMap::new()))
                .map(f64::to_bits).map_err(|e| e.to_string());
            let new = graph.step(source, &input, Overrides::External(&HashMap::new()))
                .map(f64::to_bits).map_err(|e| e.to_string());
            assert_eq!(new, old, "near phase guard frame={frame}");
        }
    }

}
