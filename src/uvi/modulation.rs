//! Native UVI control graph. No legacy fixed-size modulation-slot lowering.
//!
//! Format observations: locally owned Program XML; native reference access was
//! UVI Workstation 3.1.16 executable (static code inspection, not clean-room).
//! Mapper interpolation, integer rounding, polarity and source inversion follow
//! observed arithmetic. Script ramps and parameter units are documented at
//! https://lua.uvi.net/group___voice.html and https://lua.uvi.net/_elements.html.
//! No preset tables, commercial scripts or executable code are included here.
use super::{
    playback::resolve_path,
    program::{NodeId, Program},
};
use anyhow::{Context, Result, bail, ensure};
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet},
};

pub type Parameter = (NodeId, String);
type SourceStateKey = (NodeId, Option<u32>, Option<u64>);
pub const FIDELITY_DIAGNOSTIC: &str = "Native UVI control graph uses measured Mode0 gain, matrix, Ratio, Depth, Value, EQ GainScale, OnePole and LFO frequency laws; LFO sample scheduling, nonaligned target interpolation, host block segmentation, live envelope-control smoothing and float-rounding parity remain unverified against reference audio; random LFO clock seeds and cross-voice RNG ordering cannot be reconstructed from serialized programs";
const LIMIT: usize = 100_000;
const DEPTH: usize = 128;

pub struct Inputs {
    /// Render rate used by native 32-sample control clocks.
    pub sample_rate: f64,
    /// Host tempo for synchronized sources, in beats per minute.
    pub host_tempo: f64,
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
    Node(NodeId),
}
#[derive(Debug)]
struct Connection {
    mode: u32,
    node: NodeId,
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
    amplitude: f64,
}
#[derive(Clone)]
struct DahClock {
    rate: f64,
    origin: u64,
    block_frames: u32,
    frame: u64,
    stage: i8,
    remaining: f32,
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
    fn next_stage(&mut self, mut actual: u64, mut budget: u64, settings: DahSettings) {
        loop {
            self.stage += 1;
            if self.stage >= 4 {
                if self.pending_release {
                    self.release_with_budget(settings.sustain, actual, budget, settings);
                }
                return;
            }
            let duration = settings.durations[self.stage as usize];
            if duration.floor() == 0. {
                continue;
            }
            if duration <= budget as f32 {
                actual = actual.saturating_sub(duration.ceil() as u64);
                budget = budget.saturating_sub(duration.floor() as u64);
                continue;
            }
            self.remaining = duration - budget as f32;
            self.denominator = self.remaining.floor() as u64;
            self.elapsed = actual;
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
        self.release_with_budget(level, 0, 0, settings);
    }
    fn release_with_budget(&mut self, level: f64, actual: u64, budget: u64, settings: DahSettings) {
        self.pending_release = false;
        self.release_level = level;
        if settings.release <= budget as f32 || settings.release.floor() == 0. {
            self.stage = 6;
            return;
        }
        self.remaining = settings.release - budget as f32;
        self.denominator = self.remaining.floor() as u64;
        self.elapsed = actual;
        self.stage = 5;
    }
    fn step(&mut self, frames: u64, settings: DahSettings) {
        if self.stage < 4 || self.stage == 5 {
            if self.remaining > frames as f32 {
                self.remaining -= frames as f32;
                self.elapsed += frames;
            } else if self.stage == 5 {
                self.stage = 6;
            } else {
                self.next_stage(
                    frames.saturating_sub(self.remaining.ceil() as u64),
                    frames.saturating_sub(self.remaining.floor() as u64),
                    settings,
                );
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
                if settings.note_off_retrigger && self.stage < 4 {
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
struct AbsoluteClock {
    producer: ConstantClock,
    filtered: HashMap<NodeId, f32>,
    published: HashMap<NodeId, f32>,
}
pub struct ModulationGraph {
    kinds: Vec<String>,
    bases: Vec<BTreeMap<String, String>>,
    connections: HashMap<Parameter, Vec<Connection>>,
    node_targets: HashMap<NodeId, Vec<Parameter>>,
    absolute_order: Vec<NodeId>,
    absolute_clocks: HashMap<NodeId, AbsoluteClock>,
    target_sources: HashMap<Parameter, HashSet<NodeId>>,
    mappers: HashMap<NodeId, Mapper>,
    tables: HashMap<NodeId, Vec<f64>>,
    ramps: HashMap<(u8, Option<u32>), Ramp>,
    script_ranges: HashMap<u8, bool>,
    event_order: u64,
    analog_clocks: RefCell<HashMap<SourceStateKey, AnalogClock>>,
    dah_clocks: RefCell<HashMap<SourceStateKey, DahClock>>,
    random_seeds: RefCell<HashMap<NodeId, u32>>,
    random_lfo_clocks: RefCell<HashMap<SourceStateKey, RandomLfoClock>>,
    lfo_clocks: RefCell<HashMap<SourceStateKey, LfoClock>>,
    constant_clocks: RefCell<HashMap<SourceStateKey, ConstantClock>>,
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
fn mapper_path(program: &Program, owner: NodeId, path: &str) -> Result<NodeId> {
    if path.contains('/') || path.starts_with('$') {
        return resolve_path(program, owner, path);
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
        let matches: Vec<_> = program
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(n, node)| {
                (node.kind == "ControlSignalMapper"
                    && node.name.as_deref() == Some(path)
                    && scope(program, n) == Some(id))
                .then_some(n)
            })
            .collect();
        ensure!(matches.len() <= 1, "Ambiguous UVI mapper in scope");
        if let Some(n) = matches.first() {
            return Ok(*n);
        }
        at = scope(program, id);
    }
    bail!("Unresolved UVI mapper reference")
}
pub fn supports_target(kind: &str, name: &str) -> bool {
    matches!(
        (kind, name),
        (
            "SamplePlayer" | "MinBlepGenerator" | "WaveTableOscillator",
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
            | (
                "Gain"
                    | "OnePole"
                    | "XpanderFilter"
                    | "WhiteChorus"
                    | "DualDelay"
                    | "GainMatrix"
                    | "SparkVerb"
                    | "WaveShaper",
                "Bypass"
            )
    ) || (kind == "GainMatrix" && supports_target(kind, name))
}
fn absolute_value(kind: &str, name: &str, normalized: f32) -> Result<f64> {
    let normalized = normalized.clamp(0., 1.);
    let value = match (kind, name) {
        ("ConstantModulation", "Value")
        | ("DualDelay" | "WhiteChorus" | "SparkVerb" | "WaveShaper", "Mix")
        | ("XpanderFilter", "Q") => normalized,
        ("OnePole" | "XpanderFilter", "Freq") => 20_f32 * 1000_f32.powf(normalized),
        ("XpanderFilter", "Drive") => 40. * normalized - 20.,
        ("WaveShaper", "Knee") => 20. * normalized - 10.,
        ("WhiteChorus", "Speed") => 0.1_f32 * 10_f32.powf(normalized),
        ("WhiteChorus", "Depth") => 1. + 39. * normalized,
        ("WhiteChorus", "Crossover") => 20_f32 * 250_f32.powf(normalized),
        ("Layer", "Mute")
        | (
            "Gain" | "OnePole" | "XpanderFilter" | "WhiteChorus" | "DualDelay" | "GainMatrix"
            | "SparkVerb" | "WaveShaper",
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
    pub fn new(program: &Program) -> Result<Self> {
        ensure!(
            program.nodes.len() <= LIMIT && program.connections.len() <= LIMIT,
            "UVI modulation graph exceeds limit"
        );
        let mut graph = Self {
            kinds: program.nodes.iter().map(|n| n.kind.clone()).collect(),
            bases: program.nodes.iter().map(|n| n.attributes.clone()).collect(),
            connections: HashMap::new(),
            node_targets: HashMap::new(),
            absolute_order: Vec::new(),
            absolute_clocks: HashMap::new(),
            target_sources: HashMap::new(),
            mappers: HashMap::new(),
            tables: HashMap::new(),
            ramps: HashMap::new(),
            script_ranges: HashMap::new(),
            event_order: 0,
            analog_clocks: RefCell::new(HashMap::new()),
            dah_clocks: RefCell::new(HashMap::new()),
            random_seeds: RefCell::new(HashMap::new()),
            random_lfo_clocks: RefCell::new(HashMap::new()),
            lfo_clocks: RefCell::new(HashMap::new()),
            constant_clocks: RefCell::new(HashMap::new()),
        };
        for (id, n) in program.nodes.iter().enumerate() {
            match n.kind.as_str() {
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
                "@ChannelPressure" | "@Aftertouch" => Source::Pressure,
                "@PolyPressure" | "@PolyAftertouch" => Source::PolyPressure,
                s if s.starts_with("@MIDI CC ") => {
                    let cc = s[9..].parse::<u8>().context("Invalid UVI MIDI CC source")?;
                    ensure!(cc < 128, "Invalid UVI MIDI CC source");
                    Source::Controller(cc)
                }
                s if s.starts_with('@') => {
                    bail!("Unsupported UVI control source at node {}", c.node)
                }
                s => {
                    let id = resolve_path(program, c.owner, s)?;
                    ensure!(
                        matches!(
                            program.nodes[id].kind.as_str(),
                            "ConstantModulation"
                                | "ScriptEventModulation"
                                | "LFO"
                                | "AnalogADSR"
                                | "DAHDSR"
                        ),
                        "Unsupported UVI source kind at node {id}"
                    );
                    Source::Node(id)
                }
            };
            let mapper = if c.mapper.is_empty() {
                None
            } else {
                Some(mapper_path(program, c.owner, &c.mapper)?)
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
                    source,
                    mapper,
                });
        }
        for p in graph.connections.keys() {
            graph.node_targets.entry(p.0).or_default().push(p.clone());
        }
        // Validate the full graph, including currently bypassed connections:
        // a script can enable those, so bypass cannot hide a dependency cycle.
        let mut finished = HashSet::new();
        let mut active = HashSet::new();
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
        absolute_sources.sort_unstable();
        absolute_sources.dedup();
        let mut ordered = HashSet::new();
        for node in absolute_sources {
            graph.order_absolute(node, &mut ordered);
        }
        Ok(graph)
    }
    pub fn is_absolute_source_parameter(&self, node: NodeId, name: &str) -> bool {
        name == "Value" && self.absolute_order.contains(&node)
    }
    fn order_absolute(&mut self, node: NodeId, visited: &mut HashSet<NodeId>) {
        if !visited.insert(node) {
            return;
        }
        let upstream = self
            .connections
            .get(&(node, "Value".into()))
            .into_iter()
            .flatten()
            .filter_map(|c| match (c.mode, &c.source) {
                (1, Source::Node(n)) => Some(*n),
                _ => None,
            })
            .collect::<Vec<_>>();
        for source in upstream {
            self.order_absolute(source, visited);
        }
        self.absolute_order.push(node);
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
            sources
                .iter()
                .any(|node| matches!(self.kinds[*node].as_str(), "LFO" | "DAHDSR" | "AnalogADSR"))
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
            .ramp(id, voice)
            .map(|r| r.value(time_seconds))
            .unwrap_or(0.);
        let start = start.unwrap_or(previous);
        ensure!(
            start.is_finite() && (low..=1.).contains(&start),
            "Invalid UVI script modulation start"
        );
        ensure!(
            self.ramps.len() < LIMIT || self.ramps.contains_key(&(id, voice)),
            "UVI script modulation state exceeds limit"
        );
        self.event_order = self
            .event_order
            .checked_add(1)
            .context("UVI modulation event sequence exhausted")?;
        self.ramps.insert(
            (id, voice),
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
    fn ramp(&self, id: u8, voice: Option<u32>) -> Option<&Ramp> {
        [self.ramps.get(&(id, voice)), self.ramps.get(&(id, None))]
            .into_iter()
            .flatten()
            .max_by_key(|r| r.order)
    }
    /// Retire one renderer instance without removing shared script ramps.
    pub fn remove_instance(&mut self, voice: u32, instance: u64) {
        let retained = |key: &SourceStateKey| key.1 != Some(voice) || key.2 != Some(instance);
        self.analog_clocks.get_mut().retain(|key, _| retained(key));
        self.dah_clocks.get_mut().retain(|key, _| retained(key));
        self.constant_clocks
            .get_mut()
            .retain(|key, _| retained(key));
        self.lfo_clocks.get_mut().retain(|key, _| retained(key));
        self.random_lfo_clocks
            .get_mut()
            .retain(|key, _| retained(key));
    }
    pub fn remove_voice(&mut self, voice: u32) {
        self.ramps.retain(|(_, v), _| *v != Some(voice));
        self.analog_clocks
            .get_mut()
            .retain(|(_, v, _), _| *v != Some(voice));
        self.dah_clocks
            .get_mut()
            .retain(|(_, v, _), _| *v != Some(voice));
        self.constant_clocks
            .get_mut()
            .retain(|(_, v, _), _| *v != Some(voice));
        self.lfo_clocks
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
        let mut current_live = HashMap::new();
        let mut updates = Vec::new();
        for node in self.absolute_order.clone() {
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
                if let Some(clock) = self.absolute_clocks.get(&node) {
                    ensure!(
                        clock.producer.target == target,
                        "Unverified nonaligned UVI Mode1 producer change at node {node}"
                    );
                }
                continue;
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
            for (_, edge) in &edges {
                for name in ["Ratio", "Offset", "Inverted", "Bypass"] {
                    if let Some(value) = live.get(&(edge.node, name.into())) {
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
        Ok(updates)
    }
    /// Evaluate native parameter values. Unverified audio-target conversions
    /// fail explicitly; use `deltas` to inspect their graph contributions.
    pub fn evaluate(
        &self,
        input: &Inputs,
        live: &HashMap<Parameter, f64>,
    ) -> Result<HashMap<Parameter, f64>> {
        self.validate(input, live)?;
        let mut memo = HashMap::new();
        for p in self.connections.keys() {
            self.value(p, input, live, &mut memo, 0)?;
        }
        Ok(memo
            .into_iter()
            .filter(|(p, _)| self.connections.contains_key(p))
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
        self.validate(input, live)?;
        ensure!(
            nodes.iter().all(|id| *id < self.bases.len()),
            "Invalid UVI modulation target node"
        );
        let mut memo = HashMap::new();
        let mut result = HashMap::new();
        for node in nodes {
            for p in self.node_targets.get(node).into_iter().flatten() {
                result.insert(p.clone(), self.value(p, input, live, &mut memo, 0)?);
            }
        }
        Ok(result)
    }
    /// Inspect routed ratio-times-source sums before target conversion.
    /// These are control-domain signals, not physical-unit parameter deltas.
    pub fn deltas(
        &self,
        input: &Inputs,
        live: &HashMap<Parameter, f64>,
    ) -> Result<HashMap<Parameter, f64>> {
        self.validate(input, live)?;
        let mut memo = HashMap::new();
        let mut result = HashMap::new();
        for p in self.connections.keys() {
            result.insert(p.clone(), self.delta(p, input, live, &mut memo, 0)?);
        }
        Ok(result)
    }
    fn validate(&self, input: &Inputs, live: &HashMap<Parameter, f64>) -> Result<()> {
        ensure!(
            input.key < 128
                && input.tune_semitones.is_finite()
                && input.tune_semitones.abs() <= f64::from(f32::MAX)
                && input.velocity < 128
                && input.controllers.iter().all(|cc| *cc < 128),
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
        ensure!(
            live.iter()
                .all(|((n, _), v)| *n < self.bases.len() && v.is_finite()),
            "Invalid UVI live parameter override"
        );
        Ok(())
    }
    fn setting(
        &self,
        node: NodeId,
        name: &str,
        default: f64,
        live: &HashMap<Parameter, f64>,
    ) -> Result<f64> {
        if let Some(value) = live.get(&(node, name.into())) {
            Ok(*value)
        } else {
            number(&self.bases[node], name, default)
        }
    }
    fn boolean(
        &self,
        node: NodeId,
        name: &str,
        default: bool,
        live: &HashMap<Parameter, f64>,
    ) -> Result<bool> {
        let value = self.setting(node, name, f64::from(default), live)?;
        ensure!(
            value == 0. || value == 1.,
            "Invalid live UVI modulation Boolean {name}"
        );
        Ok(value == 1.)
    }
    fn base(&self, p: &Parameter, live: &HashMap<Parameter, f64>) -> Result<f64> {
        if let Some(v) = live.get(p) {
            return Ok(*v);
        }
        let default = match (self.kinds[p.0].as_str(), p.1.as_str()) {
            (
                "SamplePlayer"
                | "MinBlepGenerator"
                | "WaveTableOscillator"
                | "Program"
                | "Layer"
                | "Keygroup",
                "Gain",
            )
            | ("Gain", "Volume")
            | ("DigitalEq", "GainScale")
            | ("SignalConnection", "Ratio")
            | ("LFO", "Depth") => 1.,
            ("LFO", "Freq") => 0.5,
            ("OnePole" | "XpanderFilter", "Freq") => 1000.,
            ("XpanderFilter", "Fat") => 1.,
            ("DualDelay", "Feedback") => 0.3,
            ("DualDelay", "Mix") => 0.5,
            ("WhiteChorus", "Mix") => 1.,
            ("WhiteChorus", "Speed") => 0.2,
            ("WhiteChorus", "Depth") => 5.,
            ("WhiteChorus", "Crossover") => 20.,
            ("AnalogADSR", "AttackTime") => 0.001,
            ("AnalogADSR", "DecayTime") => 0.05,
            ("AnalogADSR", "ReleaseTime") => 0.01,
            ("AnalogADSR" | "DAHDSR", "SustainLevel") => 1.,
            ("DAHDSR", "ReleaseTime") => 0.05,
            ("GainMatrix", name) if name.starts_with("Gain_") => {
                let parts = name[5..].split('_').collect::<Vec<_>>();
                f64::from(parts.len() == 2 && parts[0] == parts[1])
            }
            _ => 0.,
        };
        number(&self.bases[p.0], &p.1, default)
    }
    fn value(
        &self,
        p: &Parameter,
        input: &Inputs,
        live: &HashMap<Parameter, f64>,
        memo: &mut HashMap<Parameter, f64>,
        depth: usize,
    ) -> Result<f64> {
        ensure!(
            depth < DEPTH,
            "UVI modulation evaluation depth exceeds limit"
        );
        if let Some(v) = memo.get(p) {
            return Ok(*v);
        }
        ensure!(
            !self
                .connections
                .get(p)
                .is_some_and(|edges| edges.iter().any(|c| c.mode == 0))
                || supports_target(&self.kinds[p.0], &p.1),
            "Unverified UVI modulation target conversion at node {} parameter {}",
            p.0,
            p.1
        );
        let base = self.base(p, live)?;
        let value = if !self
            .connections
            .get(p)
            .is_some_and(|edges| edges.iter().any(|c| c.mode == 0))
        {
            base
        } else if (matches!(p.1.as_str(), "Gain" | "Volume" | "Ratio" | "Depth")
            && self.kinds[p.0] != "WhiteChorus")
            || (self.kinds[p.0] == "GainMatrix" && p.1.starts_with("Gain_"))
            || (self.kinds[p.0] == "DAHDSR" && matches!(p.1.as_str(), "AttackTime" | "DecayTime"))
        {
            let mut factor = 1.;
            for c in &self.connections[p] {
                if c.mode != 0 {
                    continue;
                }
                if self.boolean(c.node, "Bypass", false, live)? {
                    continue;
                }
                let ratio = self.value(&(c.node, "Ratio".into()), input, live, memo, depth + 1)?;
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
            if self.kinds[p.0] == "GainMatrix" {
                (base + 1.) * factor - 1.
            } else {
                base * factor
            }
        } else if self.kinds[p.0] == "AnalogADSR"
            && matches!(p.1.as_str(), "AttackTime" | "DecayTime" | "ReleaseTime")
        {
            // Native logarithmic time converter has an offset; its physical
            // range alone does not determine the modulation span.
            let delta = self.delta(p, input, live, memo, depth + 1)?;
            let offset = f64::from(0.001_f32);
            let min = f64::from(0.0001_f32);
            let max = 10.;
            let span = ((max + offset) / (min + offset)).ln();
            let shifted = ((base.clamp(min, max) + offset).ln() + delta * span).exp() as f32;
            f64::from((shifted - offset as f32).clamp(min as f32, max as f32))
        } else if p.1 == "Freq" && self.kinds[p.0] == "LFO" {
            // Workstation original renders: base1 + ratio.1*source1 =>3Hz,
            // ratio.25 =>6Hz; base2 + ratio.25*source.5 =>4.5Hz.
            (base + 20. * self.delta(p, input, live, memo, depth + 1)?).clamp(0., 20.)
        } else if p.1 == "Freq" && matches!(self.kinds[p.0].as_str(), "OnePole" | "XpanderFilter") {
            ensure!(base > 0., "Invalid UVI filter frequency base");
            let delta = self.delta(p, input, live, memo, depth + 1)?;
            (base * 1000_f64.powf(delta)).clamp(20., 20000.)
        } else if self.kinds[p.0] == "XpanderFilter" && matches!(p.1.as_str(), "Q" | "Fat") {
            (base + self.delta(p, input, live, memo, depth + 1)?).clamp(0., 1.)
        } else if self.kinds[p.0] == "XpanderFilter" && p.1 == "Drive" {
            (base + 40. * self.delta(p, input, live, memo, depth + 1)?).clamp(-20., 20.)
        } else if self.kinds[p.0] == "DAHDSR" && p.1 == "DelayTime" {
            (base + 10. * self.delta(p, input, live, memo, depth + 1)?).clamp(0., 10.)
        } else if self.kinds[p.0] == "XpanderFilter" && p.1 == "Bypass" {
            f64::from((base + self.delta(p, input, live, memo, depth + 1)?).clamp(0., 1.) >= 0.5)
        } else if self.kinds[p.0] == "DualDelay" && matches!(p.1.as_str(), "Feedback" | "Mix") {
            (base + self.delta(p, input, live, memo, depth + 1)?).clamp(0., 1.)
        } else if self.kinds[p.0] == "WhiteChorus" {
            let delta = self.delta(p, input, live, memo, depth + 1)?;
            // Authored original CC renders verify each physical converter;
            // the processor owns its target smoothing, not this graph.
            match p.1.as_str() {
                "Mix" => (base + delta).clamp(0., 1.),
                "Speed" => (base * 10_f64.powf(delta)).clamp(0.1, 1.),
                "Crossover" => (base * 250_f64.powf(delta)).clamp(20., 5000.),
                "Depth" => (base + 39. * delta).clamp(1., 40.),
                _ => unreachable!("target support checked above"),
            }
        } else if self.kinds[p.0] == "DigitalEq" && p.1 == "GainScale" {
            (base + 4. * self.delta(p, input, live, memo, depth + 1)?).clamp(-2., 2.)
        } else {
            base + self.delta(p, input, live, memo, depth + 1)?
        };
        ensure!(value.is_finite(), "Nonfinite UVI modulation result");
        memo.insert(p.clone(), value);
        Ok(value)
    }
    fn delta(
        &self,
        p: &Parameter,
        input: &Inputs,
        live: &HashMap<Parameter, f64>,
        memo: &mut HashMap<Parameter, f64>,
        depth: usize,
    ) -> Result<f64> {
        let mut sum = 0.;
        for c in self.connections.get(p).into_iter().flatten() {
            if c.mode != 0 {
                continue;
            }
            if self.boolean(c.node, "Bypass", false, live)? {
                continue;
            }
            let ratio = self.value(&(c.node, "Ratio".into()), input, live, memo, depth + 1)?;
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
    fn source(
        &self,
        s: &Source,
        input: &Inputs,
        live: &HashMap<Parameter, f64>,
        memo: &mut HashMap<Parameter, f64>,
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
            Source::PolyPressure => (input.poly_pressure, false),
            Source::Node(n) => {
                let bipolar = self.boolean(
                    n,
                    "Bipolar",
                    matches!(self.kinds[n].as_str(), "LFO" | "ScriptEventModulation"),
                    live,
                )?;
                let bypass = self.value(&(n, "Bypass".into()), input, live, memo, depth + 1)?;
                ensure!(
                    bypass == 0. || bypass == 1.,
                    "Invalid UVI modulation source Bypass"
                );
                if bypass != 0. {
                    return Ok((0., bipolar));
                }
                let v = match self.kinds[n].as_str() {
                    "ConstantModulation" => {
                        let v = self
                            .value(&(n, "Value".into()), input, live, memo, depth + 1)?
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
                        let id =
                            self.value(&(n, "EventId".into()), input, live, memo, depth + 1)?;
                        ensure!(
                            (0. ..=127.).contains(&id) && id.fract() == 0.,
                            "Invalid live UVI script EventId"
                        );
                        let id = id as u8;
                        let v = input.script_values.get(&id).copied().unwrap_or_else(|| {
                            self.ramp(id, input.voice)
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
                    "DAHDSR" => {
                        let value = self.dah(n, input, live, memo, depth + 1)?;
                        if bipolar { 2. * value - 1. } else { value }
                    }
                    "AnalogADSR" => {
                        let value = self.analog(n, input, live, memo, depth + 1)?;
                        if bipolar { 2. * value - 1. } else { value }
                    }
                    _ => bail!("Unsupported UVI modulation source"),
                };
                (v, bipolar)
            }
        };
        ensure!(value.is_finite(), "Nonfinite UVI modulation source");
        Ok((value, bipolar))
    }
    fn dah(
        &self,
        n: NodeId,
        input: &Inputs,
        live: &HashMap<Parameter, f64>,
        memo: &mut HashMap<Parameter, f64>,
        depth: usize,
    ) -> Result<f64> {
        ensure!(
            self.setting(n, "Retrigger", 1., live)? == 1.,
            "Unverified shared UVI DAHDSR trigger mode at node {n}"
        );
        let mut durations = [0.; 4];
        for (index, name) in ["DelayTime", "AttackTime", "HoldTime", "DecayTime"]
            .into_iter()
            .enumerate()
        {
            let max = if index == 3 { 30. } else { 10. };
            durations[index] = self
                .value(&(n, name.into()), input, live, memo, depth + 1)?
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
            sustain: self
                .value(&(n, "SustainLevel".into()), input, live, memo, depth + 1)?
                .clamp(0., 1.),
            release: self
                .value(&(n, "ReleaseTime".into()), input, live, memo, depth + 1)?
                .clamp(0., 20.) as f32
                * input.sample_rate as f32,
            note_off_retrigger: self.boolean(n, "NoteOffRetrigger", false, live)?,
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
            clock.next_stage(0, 0, settings);
            clock
        });
        ensure!(
            clock.rate == input.sample_rate && clock.block_frames == input.control_block_frames,
            "UVI DAHDSR clock configuration changed during playback"
        );
        clock.advance(frame, off, settings)
    }
    fn analog(
        &self,
        n: NodeId,
        input: &Inputs,
        live: &HashMap<Parameter, f64>,
        memo: &mut HashMap<Parameter, f64>,
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
                    memo: &mut HashMap<Parameter, f64>|
         -> Result<f64> {
            let base = self
                .value(&(n, name.into()), input, live, memo, depth + 1)?
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
                self.value(&(n, "ReleaseTime".into()), input, live, memo, depth + 1)?
                    .clamp(0.0001, 10.),
            ),
            sustain: self
                .value(&(n, "SustainLevel".into()), input, live, memo, depth + 1)?
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
            .flat_map(|node| self.node_targets.get(node).into_iter().flatten())
            .flat_map(|target| self.target_sources.get(target).into_iter().flatten())
            .any(|node| matches!(self.kinds[*node].as_str(), "AnalogADSR" | "DAHDSR"))
    }
    /// Release completion belongs to the render instance, not logical script ID.
    pub fn release_finished(
        &self,
        input: &Inputs,
        live: &HashMap<Parameter, f64>,
        nodes: &HashSet<NodeId>,
    ) -> Result<bool> {
        self.validate(input, live)?;
        let mut memo = HashMap::new();
        let mut envelopes = HashSet::new();
        for target in nodes
            .iter()
            .flat_map(|node| self.node_targets.get(node).into_iter().flatten())
        {
            for node in self.target_sources.get(target).into_iter().flatten() {
                if matches!(self.kinds[*node].as_str(), "AnalogADSR" | "DAHDSR") {
                    envelopes.insert(*node);
                }
            }
        }
        for node in envelopes {
            if self.value(&(node, "Bypass".into()), input, live, &mut memo, 0)? != 0. {
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
                "DAHDSR" => {
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
        live: &HashMap<Parameter, f64>,
        memo: &mut HashMap<Parameter, f64>,
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
        let delay = self.value(&(n, "DelayTime".into()), input, live, memo, depth + 1)?;
        let rise = self.value(&(n, "RiseTime".into()), input, live, memo, depth + 1)?;
        let frequency = self.value(&(n, "Freq".into()), input, live, memo, depth + 1)?;
        let freq = if self.boolean(n, "SyncToHost", false, live)? {
            ensure!(frequency > 0., "Invalid synchronized UVI LFO beat period");
            f64::from((input.host_tempo as f32 * (1_f32 / 60.)) / frequency as f32)
        } else {
            frequency
        };
        let phase = self.value(&(n, "Phase".into()), input, live, memo, depth + 1)?;
        let amplitude = self.value(&(n, "Depth".into()), input, live, memo, depth + 1)?;
        ensure!(
            delay >= 0. && rise >= 0. && freq >= 0. && (0. ..=1.).contains(&amplitude),
            "Invalid UVI LFO parameters"
        );
        let wave = self.setting(n, "WaveFormType", 0., live)?;
        ensure!(
            (0. ..=9.).contains(&wave) && wave.fract() == 0.,
            "Invalid UVI LFO waveform type"
        );
        if wave == 6. {
            ensure!(
                retrigger == 1. && delay == 0. && rise == 0.,
                "Unverified UVI random LFO delay/rise/trigger mode at node {n}"
            );
            let raw = self.random_lfo(n, input, freq, phase, smooth)?;
            return Ok((if bipolar { raw } else { (raw + 1.) * 0.5 }) * amplitude);
        }
        ensure!(
            smooth == 0.,
            "Unimplemented UVI deterministic LFO smoothing at node {n}"
        );
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
    use super::*;
    use crate::uvi::program::parse_program;
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
        assert_eq!(graph.evaluate(&input, &live).unwrap()[&parameter], 0.8);
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
        ] {
            assert!(supports_absolute_target(kind, name));
            assert!((absolute_value(kind, name, normalized).unwrap() - expected).abs() < 0.00001);
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
