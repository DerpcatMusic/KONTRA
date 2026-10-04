//! Offline native Program graph playback. No VM work runs in the audio callback.
//!
//! Parameter units follow https://lua.uvi.net/_elements.html. Linear sample
//! interpolation uses an original law. Authored native probes
//! established matrix width, output layouts, mono pan and gain control timing;
//! this renderer does not assert overall numerical parity with Falcon.
use super::{
    dsp::{self, Frame, Gain, GainMatrix, OnePole, TrackDelay},
    effects::{self, EffectProcessor},
    filter::{self, XpanderFilter},
    generator::{self, Generator},
    host::{self, ParameterValue},
    maximizer::{self, Maximizer},
    modulation::{self, Inputs, ModulationGraph, Parameter},
    phasor::{self, Phasor},
    program::{NodeId, Program},
    sample::{Sample, SampleLoop},
    script,
    sparkverb::{self, SparkVerb},
    time_effects::{self, TimeEffect},
    waveshaper::{self, WaveShaper},
};
use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use std::{
    cell::Cell,
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
};

pub const FIDELITY_DIAGNOSTIC: &str = "Native Program playback uses original sample resampling and file ping-pong traversal, crossfade, oldest-note stealing, gain/insert ordering; voices with supported envelope routes retain their release phase and honor looped-release markers; nearest/linear/Catmull-Rom samples with physical zero padding and forward-loop neighbors, matrix width, mono/stereo pan, output layouts and aligned SamplePlayer gain control were checked against authored native fixtures; TriggerRule2 is admitted only for measured ordinary note-on scope, KG pan is ignored as measured for6/10/12-channel sources; SIMD phase rounding, overlapping keygroup selection and nonaligned control timing remain unverified; Falcon numerical parity is unverified";
const LIMIT: usize = 65_536;
const VOICE_LIMIT: usize = 4096;
const PROCESSOR_MEMORY_LIMIT: usize = 256 << 20;
const SAMPLE_MEMORY_LIMIT: usize = 512 << 20;

#[derive(Debug, Serialize)]
pub struct Unsupported {
    pub node: NodeId,
    pub kind: String,
    pub reason: String,
}

fn wrapper(kind: &str) -> bool {
    matches!(
        kind,
        "Layers"
            | "Keygroups"
            | "Oscillators"
            | "Inserts"
            | "Auxs"
            | "Connections"
            | "ControlSignalSources"
            | "Steps"
            | "Mappers"
            | "EventProcessors"
            | "BusRouters"
            | "Chains"
    )
}
fn parent(program: &Program, node: NodeId) -> Option<NodeId> {
    let mut at = program.nodes.get(node)?.parent;
    while let Some(id) = at {
        if !wrapper(&program.nodes[id].kind) {
            return Some(id);
        }
        at = program.nodes[id].parent;
    }
    None
}
/// Resolve a serialized element path through collection wrappers. Ambiguous
/// names are errors; no basename/global-name guess may select the wrong bus.
pub fn resolve_path(program: &Program, node: NodeId, path: &str) -> Result<NodeId> {
    ensure!(node < program.nodes.len(), "Invalid UVI path owner");
    let mut at = node;
    let mut path = path;
    if let Some(rest) = path.strip_prefix("/uvi/Part 0/Program/") {
        at = program.root;
        path = rest;
    } else {
        ensure!(!path.starts_with('/'), "Unverified absolute UVI graph path");
    }
    if let Some((scope, rest)) = path
        .split_once('/')
        .or_else(|| path.starts_with('$').then_some((path, "")))
    {
        if scope.starts_with('$') {
            let kind = match scope {
                "$Program" => "Program",
                "$Layer" => "Layer",
                "$Keygroup" => "Keygroup",
                _ => bail!("Unknown UVI path scope {scope}"),
            };
            let mut ancestor = Some(node);
            while ancestor.is_some_and(|id| program.nodes[id].kind != kind) {
                ancestor = ancestor.and_then(|id| program.nodes[id].parent);
            }
            at = ancestor.with_context(|| format!("UVI path has no enclosing {kind}"))?;
            path = rest;
        }
    }
    for part in path.split('/').filter(|p| !p.is_empty() && *p != ".") {
        if part == ".." {
            at = parent(program, at).context("UVI path ascends above Program")?;
            continue;
        }
        let matches: Vec<_> = program
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(id, n)| {
                (parent(program, id) == Some(at) && n.name.as_deref() == Some(part)).then_some(id)
            })
            .collect();
        ensure!(
            matches.len() == 1,
            "UVI graph path segment cannot be resolved uniquely"
        );
        at = matches[0];
    }
    Ok(at)
}

pub fn preflight(program: &Program) -> Vec<Unsupported> {
    let mut unsupported = Vec::new();
    for (id, node) in program.nodes.iter().enumerate() {
        let implemented = matches!(
            node.kind.as_str(),
            "ControlSignalMapper"
                | "ScriptEventModulation"
                | "ConstantModulation"
                | "LFO"
                | "AnalogADSR"
                | "DAHDSR"
                | "AHD"
                | "AttackDecayEnv"
                | "StdRandom"
                | "Drunk"
                | "MultiEnvelope"
                | "Step"
                | "SignalConnection"
        ) || effects::supports(&node.kind)
            || filter::supports(&node.kind)
            || time_effects::supports(&node.kind)
            || waveshaper::supports(&node.kind)
            || maximizer::supports(&node.kind)
            || sparkverb::supports(&node.kind)
            || phasor::supports(&node.kind)
            || generator::supports(&node.kind)
            || wrapper(&node.kind)
            || matches!(
                node.kind.as_str(),
                "Program"
                    | "Layer"
                    | "Keygroup"
                    | "SamplePlayer"
                    | "PlaybackOptions"
                    | "Loop"
                    | "GainMatrix"
                    | "Gain"
                    | "OnePole"
                    | "TrackDelay"
                    | "EffectRack"
                    | "AuxEffect"
                    | "BusRouter"
                    | "Properties"
                    | "ScriptProcessor"
                    | "script"
                    | "state"
                    | "ScriptData"
                    | "UserTable"
            );
        if !implemented {
            unsupported.push(Unsupported { node:id, kind:node.kind.clone(), reason:"Processor or control source is not executable by this renderer (including if later enabled)".into() });
        }
        if node.kind == "MultiEnvelope"
            && node
                .attributes
                .get("Retrigger")
                .is_some_and(|v| v.parse::<f64>() != Ok(1.))
        {
            unsupported.push(Unsupported {
                node: id,
                kind: node.kind.clone(),
                reason: "MultiEnvelope modes that ignore note-off have no verified exhausted-source/insert-tail cleanup law".into(),
            });
        }
        if node.kind == "Step"
            && !node.parent.is_some_and(|steps| {
                program.nodes[steps].kind == "Steps"
                    && program.nodes[steps]
                        .parent
                        .is_some_and(|owner| program.nodes[owner].kind == "MultiEnvelope")
            })
        {
            unsupported.push(Unsupported {
                node: id,
                kind: node.kind.clone(),
                reason: "Envelope Step must belong to MultiEnvelope Steps".into(),
            });
        }
        if node.kind == "Loop" {
            let valid_owner = node
                .parent
                .is_some_and(|p| program.nodes[p].kind == "PlaybackOptions");
            let forward = node.attributes.get("Type").is_none_or(|v| v == "0");
            let supported_attributes = node
                .attributes
                .keys()
                .all(|name| matches!(name.as_str(), "Start" | "End" | "Type" | "Name"));
            if !valid_owner || !forward || !supported_attributes {
                unsupported.push(Unsupported { node:id,kind:node.kind.clone(),reason:"Only a serialized forward loop without crossfade has a verified endpoint law".into() });
            }
        }
        if generator::supports(&node.kind) {
            if let Err(error) = generator::validate(node) {
                unsupported.push(Unsupported {
                    node: id,
                    kind: node.kind.clone(),
                    reason: error.to_string(),
                });
            }
        }
        if time_effects::supports(&node.kind) {
            if let Err(error) = time_effects::validate(node) {
                unsupported.push(Unsupported {
                    node: id,
                    kind: node.kind.clone(),
                    reason: error.to_string(),
                });
            }
        }
        if phasor::supports(&node.kind) {
            if let Err(error) = phasor::validate(node) {
                unsupported.push(Unsupported {
                    node: id,
                    kind: node.kind.clone(),
                    reason: error.to_string(),
                });
            }
        }
        if sparkverb::supports(&node.kind) {
            if let Err(error) = sparkverb::validate(node) {
                unsupported.push(Unsupported {
                    node: id,
                    kind: node.kind.clone(),
                    reason: error.to_string(),
                });
            }
        }
        if maximizer::supports(&node.kind) {
            if let Err(error) = maximizer::validate(node) {
                unsupported.push(Unsupported {
                    node: id,
                    kind: node.kind.clone(),
                    reason: error.to_string(),
                });
            }
        }
        if waveshaper::supports(&node.kind) {
            if let Err(error) = waveshaper::validate(node) {
                unsupported.push(Unsupported {
                    node: id,
                    kind: node.kind.clone(),
                    reason: error.to_string(),
                });
            }
        }
        if filter::supports(&node.kind) {
            if let Err(error) = filter::validate(node) {
                unsupported.push(Unsupported {
                    node: id,
                    kind: node.kind.clone(),
                    reason: error.to_string(),
                });
            }
        }
        if effects::supports(&node.kind) {
            if let Err(error) = effects::validate(node) {
                unsupported.push(Unsupported {
                    node: id,
                    kind: node.kind.clone(),
                    reason: error.to_string(),
                });
            }
        }
        for parameter in [
            "TriggerMode",
            "TriggerSync",
            "LatchTrigger",
            "ExclusiveGroup",
            "PlayMode",
            "VelocityCurve",
            "PortamentoMode",
            "NotePolyphony",
        ] {
            if parameter == "TriggerMode"
                && matches!(node.kind.as_str(), "StdRandom" | "Drunk")
                && node
                    .attributes
                    .get(parameter)
                    .is_some_and(|value| value.parse::<f64>() == Ok(1.))
            {
                continue;
            }
            if node.attributes.get(parameter).is_some_and(|s| s != "0") {
                unsupported.push(Unsupported {
                    node: id,
                    kind: node.kind.clone(),
                    reason: format!("Unsupported nondefault {parameter}"),
                });
            }
        }
        if node.attributes.get("TriggerRule").is_some_and(|v| v != "0") {
            let mut scope = Some(id);
            let mut ordinary = true;
            while let Some(at) = scope {
                ordinary &= ["TriggerMode", "TriggerSync", "PlayMode"].iter().all(|p| {
                    program.nodes[at]
                        .attributes
                        .get(*p)
                        .is_none_or(|v| v == "0")
                });
                scope = parent(program, at);
            }
            if !ordinary || node.attributes.get("TriggerRule").is_some_and(|v| v != "2") {
                unsupported.push(Unsupported {
                    node: id,
                    kind: node.kind.clone(),
                    reason: "Unverified trigger rule outside ordinary note-on scope".into(),
                });
            }
        }
        for parameter in ["OutputName", "LoopProgram"] {
            if node
                .attributes
                .get(parameter)
                .is_some_and(|value| !value.is_empty() && value != "0")
            {
                unsupported.push(Unsupported {
                    node: id,
                    kind: node.kind.clone(),
                    reason: format!("Unsupported nondefault {parameter}"),
                });
            }
        }
        if node
            .attributes
            .get("NumVoicesPerNote")
            .is_some_and(|s| s != "1")
        {
            unsupported.push(Unsupported {
                node: id,
                kind: node.kind.clone(),
                reason: "Multiple unison voices are not implemented".into(),
            });
        }
        if node.kind == "OnePole" && node.attributes.get("KeyTracking").is_some_and(|s| s != "0") {
            unsupported.push(Unsupported {
                node: id,
                kind: node.kind.clone(),
                reason: "Filter key tracking is not implemented".into(),
            });
        }
    }
    match ModulationGraph::new(program) {
        Ok(graph) => {
            for (node, entry) in program.nodes.iter().enumerate() {
                let mut scope = Some(node);
                while scope.is_some_and(|id| program.nodes[id].kind != "Keygroup") {
                    scope = scope.and_then(|id| program.nodes[id].parent);
                }
                let rendered_target = matches!(
                    entry.kind.as_str(),
                    "Program"
                        | "Layer"
                        | "AuxEffect"
                        | "BusRouter"
                        | "GainMatrix"
                        | "Gain"
                        | "OnePole"
                        | "TrackDelay"
                ) || effects::supports(&entry.kind)
                    || filter::supports(&entry.kind)
                    || time_effects::supports(&entry.kind)
                    || waveshaper::supports(&entry.kind)
                    || maximizer::supports(&entry.kind)
                    || sparkverb::supports(&entry.kind)
                    || phasor::supports(&entry.kind);
                if rendered_target
                    && scope.is_none()
                    && graph.has_release_envelopes(&HashSet::from([node]))
                {
                    unsupported.push(Unsupported { node, kind: entry.kind.clone(), reason: "Envelope-dependent targets outside keygroup voice context have no verified gate law".into() });
                }
            }
            for (node, parameter) in graph.unsupported_targets() {
                unsupported.push(Unsupported {
                    node,
                    kind: program.nodes[node].kind.clone(),
                    reason: format!("Unimplemented native modulation conversion for {parameter}"),
                });
            }
        }
        Err(error) => unsupported.push(Unsupported {
            node: program.root,
            kind: "ControlGraph".into(),
            reason: error.to_string(),
        }),
    }
    unsupported
}

enum Processor {
    Matrix(GainMatrix),
    Gain(Gain),
    Pole(OnePole),
    Delay(TrackDelay),
    Effect(EffectProcessor),
    Filter(Box<XpanderFilter>),
    Time(TimeEffect),
    Wave(Box<WaveShaper>),
    Max(Maximizer),
    Spark(Box<SparkVerb>),
    Phasor(Box<Phasor>),
}
impl Processor {
    fn new(
        node: &super::program::ProgramNode,
        rate: f64,
        channel_count: usize,
        samples: &Arc<HashMap<String, Arc<Sample>>>,
    ) -> Result<Option<Self>> {
        let kind = node.kind.as_str();
        if phasor::supports(kind) {
            return Ok(Some(Self::Phasor(Box::new(Phasor::new(
                node,
                channel_count,
                rate,
            )?))));
        }
        if sparkverb::supports(kind) {
            return Ok(Some(Self::Spark(Box::new(SparkVerb::new(
                node,
                channel_count,
                rate,
            )?))));
        }
        if maximizer::supports(kind) {
            return Ok(Some(Self::Max(Maximizer::new(node, channel_count, rate)?)));
        }
        if waveshaper::supports(kind) {
            return Ok(Some(Self::Wave(Box::new(WaveShaper::new(
                node,
                channel_count,
                rate,
            )?))));
        }
        if time_effects::supports(kind) {
            ensure!(
                channel_count == 2,
                "Time-effect placement on this source width has no verified native routing law"
            );
            return Ok(Some(Self::Time(TimeEffect::new(
                node,
                channel_count,
                rate,
            )?)));
        }
        if filter::supports(kind) {
            return Ok(Some(Self::Filter(Box::new(XpanderFilter::new(
                node,
                channel_count,
                rate,
            )?))));
        }
        if effects::supports(kind) {
            return Ok(Some(Self::Effect(EffectProcessor::new(
                node,
                channel_count,
                rate,
                1,
                Arc::clone(samples),
            )?)));
        }
        Ok(Some(match kind {
            "GainMatrix" => Self::Matrix(GainMatrix::new(channel_count, channel_count)?),
            "Gain" => Self::Gain(Gain::new(channel_count)?),
            "OnePole" => Self::Pole(OnePole::new(channel_count, rate)?),
            "TrackDelay" => Self::Delay(TrackDelay::new(channel_count, rate)?),
            _ => return Ok(None),
        }))
    }
    fn memory_bytes(&self) -> usize {
        // Every entry owns the full enum allocation. Leaf accessors differ in
        // whether they include inline state; count that state exactly once.
        std::mem::size_of::<Self>()
            + match self {
                Self::Effect(p) => p.memory_bytes() - std::mem::size_of::<EffectProcessor>(),
                Self::Time(p) => p.memory_bytes(),
                Self::Wave(p) => p.memory_bytes(),
                Self::Filter(_) => std::mem::size_of::<XpanderFilter>(),
                Self::Max(p) => p.memory_bytes() - std::mem::size_of::<Maximizer>(),
                Self::Delay(p) => p.memory_bytes() - std::mem::size_of::<TrackDelay>(),
                Self::Spark(p) => std::mem::size_of::<SparkVerb>() + p.memory_bytes(),
                Self::Phasor(p) => p.memory_bytes(),
                _ => 0,
            }
    }
    fn set(&mut self, name: &str, value: f64) -> Result<()> {
        match self {
            Self::Matrix(p) => p.set_parameter(name, value),
            Self::Gain(p) => p.set_parameter(name, value),
            Self::Pole(p) => p.set_parameter(name, value),
            Self::Delay(p) => p.set_parameter(name, value),
            Self::Effect(p) => p.set_parameter(name, &ParameterValue::Number(value)),
            Self::Filter(p) => p.set_parameter(
                name,
                &if name == "Bypass" {
                    ParameterValue::Boolean(value >= 0.5)
                } else {
                    ParameterValue::Number(value)
                },
            ),
            Self::Time(p) => p.set_parameter(name, &ParameterValue::Number(value)),
            Self::Wave(p) => p.set_parameter(name, &ParameterValue::Number(value)),
            Self::Max(p) => p.set_parameter(name, &ParameterValue::Number(value)),
            Self::Spark(p) => p.set_parameter(name, &ParameterValue::Number(value)),
            Self::Phasor(p) => p.set_parameter(
                name,
                &if matches!(name, "Bypass" | "SyncToHost") {
                    ParameterValue::Boolean(value >= 0.5)
                } else {
                    ParameterValue::Number(value)
                },
            ),
        }
    }
    fn set_value(&mut self, name: &str, value: &ParameterValue) -> Result<()> {
        match self {
            Self::Effect(p) => return p.set_parameter(name, value),
            Self::Filter(p) => return p.set_parameter(name, value),
            Self::Time(p) => return p.set_parameter(name, value),
            Self::Wave(p) => return p.set_parameter(name, value),
            Self::Max(p) => return p.set_parameter(name, value),
            Self::Spark(p) => return p.set_parameter(name, value),
            Self::Phasor(p) => return p.set_parameter(name, value),
            _ => {}
        }
        let value = match value {
            ParameterValue::Number(n) => *n,
            ParameterValue::Boolean(b) => f64::from(u8::from(*b)),
            _ => bail!("Numeric UVI processor parameter required"),
        };
        self.set(name, value)
    }
    fn set_value_bounded(
        &mut self,
        name: &str,
        value: &ParameterValue,
        bytes: &mut usize,
    ) -> Result<()> {
        let before = self.memory_bytes();
        self.set_value(name, value)?;
        *bytes = bytes
            .saturating_sub(before)
            .saturating_add(self.memory_bytes());
        ensure!(
            *bytes <= PROCESSOR_MEMORY_LIMIT,
            "UVI processing buffers exceed memory budget"
        );
        Ok(())
    }
    fn bypassed(&self) -> Result<bool> {
        let value = match self {
            Self::Matrix(p) => return Ok(p.parameter("Bypass")? != 0.),
            Self::Gain(p) => return Ok(p.parameter("Bypass")? != 0.),
            Self::Pole(p) => return Ok(p.parameter("Bypass")? != 0.),
            Self::Delay(p) => return Ok(p.parameter("Bypass")? != 0.),
            Self::Effect(p) => p.parameter("Bypass")?,
            Self::Filter(p) => p.parameter("Bypass")?,
            Self::Time(p) => p.parameter("Bypass")?,
            Self::Wave(p) => p.parameter("Bypass")?,
            Self::Max(p) => p.parameter("Bypass")?,
            Self::Spark(p) => p.parameter("Bypass")?,
            Self::Phasor(p) => p.parameter("Bypass")?,
        };
        match value {
            ParameterValue::Boolean(value) => Ok(value),
            ParameterValue::Number(value) => Ok(value != 0.),
            _ => bail!("Invalid UVI processor bypass state"),
        }
    }
    fn process(&mut self, frame: &mut Frame) -> Result<()> {
        match self {
            Self::Matrix(p) => {
                let mut output = [[0.; dsp::MAX_CHANNELS]];
                p.process(&[*frame], &mut output)?;
                *frame = output[0];
            }
            Self::Gain(p) => p.process(std::slice::from_mut(frame)),
            Self::Pole(p) => p.process(std::slice::from_mut(frame))?,
            Self::Delay(p) => p.process(std::slice::from_mut(frame))?,
            Self::Effect(p) => p.process(std::slice::from_mut(frame))?,
            Self::Filter(p) => p.process(std::slice::from_mut(frame))?,
            Self::Time(p) => p.process(std::slice::from_mut(frame))?,
            Self::Wave(p) => p.process(std::slice::from_mut(frame))?,
            Self::Max(p) => p.process(std::slice::from_mut(frame))?,
            Self::Spark(p) => p.process(std::slice::from_mut(frame))?,
            Self::Phasor(p) => p.process(std::slice::from_mut(frame))?,
        }
        Ok(())
    }
}

struct Oscillator {
    player: NodeId,
    path: String,
    position: f64,
    start: usize,
    end: usize,
    loop_data: Option<SampleLoop>,
    play_release: bool,
    direction: f64,
    loops_completed: u32,
    done: bool,
    generator: Option<Generator>,
    gain: Option<GainClock>,
}
struct SamplePlayback {
    start: usize,
    end: usize,
    marker_span: usize,
    reverse: bool,
    silent: bool,
    play_release: bool,
    loop_data: Option<SampleLoop>,
}
/// Native SamplePlayer gain has 32-frame, f32 exponential control points
/// with linear interpolation. Nonaligned changes cannot rewrite already emitted
/// frames, so that lookahead difference remains in the fidelity diagnostic.
struct GainClock {
    endpoint: f32,
    point: f32,
    current: f32,
    target: f32,
    integrated: u64,
    point_frame: u64,
}
impl GainClock {
    fn new(frame: u64, target: f32) -> Self {
        Self {
            endpoint: target,
            point: target,
            current: target,
            target,
            integrated: frame,
            point_frame: frame / 32 * 32,
        }
    }
    fn integrate(value: f32, target: f32, frames: u64, rate: f64) -> f32 {
        if frames == 32 {
            let alpha = (1. - 0.330000013113f64.powf(3200. / rate)) as f32;
            value + (target - value) * alpha
        } else {
            let alpha = (1. - 0.330000013113f64.powf(100. / rate)) as f32;
            let mut value = value;
            for _ in 0..frames {
                value += (target - value) * alpha;
            }
            value
        }
    }
    fn value(&mut self, frame: u64, target: f32, rate: f64) -> f32 {
        let mut changed = false;
        while frame >= self.point_frame + 32 {
            let end = self.point_frame + 32;
            self.current = Self::integrate(self.current, self.target, end - self.integrated, rate);
            self.integrated = end;
            self.point_frame = end;
            self.point = self.current;
            changed = true;
        }
        if target != self.target {
            self.current =
                Self::integrate(self.current, self.target, frame - self.integrated, rate);
            self.integrated = frame;
            self.target = target;
            changed = true;
        }
        if changed {
            self.endpoint = Self::integrate(
                self.current,
                self.target,
                self.point_frame + 32 - self.integrated,
                rate,
            );
        }
        self.point + (self.endpoint - self.point) * ((frame - self.point_frame) as f32 / 32.)
    }
}
#[derive(Clone, Copy)]
struct VoiceFade {
    start: f32,
    target: f32,
    begin: u64,
    duration: u64,
    kill: bool,
}
impl VoiceFade {
    fn value(self, frame: u64) -> f32 {
        if self.duration == 0 {
            self.target
        } else {
            self.start
                + (self.target - self.start)
                    * ((frame.saturating_sub(self.begin) as f64 / self.duration as f64)
                        .clamp(0., 1.) as f32)
        }
    }
}
struct Voice {
    root: Option<script::HostRoot>,
    note: script::Note,
    started: u64,
    instance: u64,
    launch: u64,
    key_released: bool,
    note_off: Option<u64>,
    channels: usize,
    fade: Option<VoiceFade>,
    gain: GainClock,
    keygroup: NodeId,
    layer: NodeId,
    oscillators: Vec<Oscillator>,
    processors: HashMap<NodeId, Processor>,
}

/// Worker-owned observations, read only at an explicit diagnostic request.
#[derive(Serialize)]
pub struct NodeRuntimeEvidence {
    pub node: NodeId,
    /// Native 256-frame intervals with at least one successful source/insert call
    /// past the node's bypass gate, including calls starting mid-interval.
    /// This is execution evidence, not an audibility or fidelity measurement.
    pub processed_blocks: u64,
    /// Current authored/live base Bypass; ancestor and modulation gates differ.
    pub currently_bypassed: Option<bool>,
    /// Membership in retained voices, including silent/releasing/done sources.
    pub retained_voice_instances: usize,
}
pub struct Renderer<'a> {
    program: &'a Program,
    processed_blocks: Vec<Cell<u64>>,
    last_processed_block: Vec<Cell<u64>>,
    // Existing successful setters keep this in sync with the insert bypass gate.
    insert_bypassed: Vec<Cell<bool>>,
    runtime_nodes: Vec<NodeId>,
    parameters: Vec<BTreeMap<String, String>>,
    numbers: Vec<BTreeMap<String, usize>>,
    number_slots: Vec<CachedNumber>,
    active_numbers: Vec<usize>,
    effective_generation: u64,
    samples: Arc<HashMap<String, Arc<Sample>>>,
    rate: f64,
    frame: u64,
    next_instance: u64,
    next_launch: u64,
    snapshots: HashMap<u32, script::Note>,
    voices: Vec<Voice>,
    processors: HashMap<NodeId, Processor>,
    children: Vec<Vec<NodeId>>,
    processor_ids: HashMap<Option<NodeId>, Vec<NodeId>>,
    scope_nodes: HashMap<Option<NodeId>, HashSet<NodeId>>,
    source_channels: HashMap<NodeId, usize>,
    global_channels: usize,
    buses: HashMap<NodeId, Frame>,
    routes: HashMap<NodeId, NodeId>,
    modulation: ModulationGraph,
    live: HashMap<Parameter, f64>,
    registered_live_valid: bool,
    controllers: [[u8; 128]; 16],
    bends: [f64; 16],
    pressures: [f64; 16],
    poly_pressures: [[u8; 128]; 16],
    tempo: f64,
}

#[derive(Default)]
struct CachedNumber {
    parameter: Parameter,
    base: Option<f64>,
    effective: Option<(u64, f64)>,
    dynamic: Option<bool>,
}

impl CachedNumber {
    fn new(node: NodeId, name: String, text: &str) -> Self {
        Self {
            parameter: (node, name),
            base: text.parse::<f64>().ok().filter(|n| n.is_finite()),
            effective: None,
            dynamic: None,
        }
    }
}

fn numeric(
    parameters: &[BTreeMap<String, String>],
    node: NodeId,
    name: &str,
    default: f64,
) -> Result<f64> {
    let n = parameters[node]
        .get(name)
        .map(|s| s.parse::<f64>())
        .transpose()
        .with_context(|| format!("Invalid UVI numeric parameter {name}"))?
        .unwrap_or(default);
    ensure!(n.is_finite(), "Nonfinite UVI parameter {name}");
    Ok(n)
}
fn validate_sample(sample: &Sample) -> Result<()> {
    ensure!(
        sample.rate > 0
            && sample.frames > 0
            && sample.channels > 0
            && sample.channels <= 256
            && sample.frames.checked_mul(sample.channels) == Some(sample.interleaved.len())
            && sample.interleaved.iter().all(|value| value.is_finite()),
        "Invalid UVI sample dimensions or values"
    );
    Ok(())
}
fn source_layout(channels: usize) -> Result<()> {
    ensure!(
        [1, 2, 3, 4, 6, 8, 10, 11, 12].contains(&channels),
        "Unverified UVI source output layout"
    );
    Ok(())
}
fn add(to: &mut Frame, from: Frame) {
    for (to, from) in to.iter_mut().zip(from) {
        *to += from;
    }
}
fn balance(frame: &mut Frame, gain: f64, pan: f64) -> Result<()> {
    ensure!(
        gain >= 0. && gain.is_finite() && (-1. ..=1.).contains(&pan),
        "Invalid UVI gain/pan"
    );
    if gain == 1. && pan == 0. {
        return Ok(());
    }
    let left = if pan > 0. {
        gain * (pan * std::f64::consts::FRAC_PI_2).cos().powi(2)
    } else {
        gain
    } as f32;
    let right = if pan < 0. {
        gain * (pan * std::f64::consts::FRAC_PI_2).cos().powi(2)
    } else {
        gain
    } as f32;
    for pair in frame.chunks_exact_mut(2) {
        pair[0] *= left;
        pair[1] *= right;
    }
    Ok(())
}

/// Native KG output layouts measured with positive, channel-isolated authored
/// impulses. Matrices retain the source width; this conversion happens after KG inserts.
fn downmix(input: Frame, channels: usize, pan: f64, pan_law: f64) -> Result<Frame> {
    ensure!([0., 1.].contains(&pan_law), "Invalid UVI pan law");
    ensure!(
        [1, 2, 3, 4, 6, 8, 10, 11, 12].contains(&channels),
        "Unverified UVI output channel layout"
    );
    let mut out = [0.; dsp::MAX_CHANNELS];
    if channels == 1 {
        ensure!((-1. ..=1.).contains(&pan), "Invalid UVI mono pan");
        let theta = (pan + 1.) * std::f64::consts::FRAC_PI_4;
        let exponent = if pan_law == 0. { 2 } else { 1 };
        out[0] = input[0] * theta.cos().powi(exponent) as f32;
        out[1] = input[0] * theta.sin().powi(exponent) as f32;
        return Ok(out);
    }
    ensure!(
        pan_law == 0. || channels == 2,
        "Multichannel PanLaw1 requires native law verification"
    );
    let center = std::f32::consts::FRAC_1_SQRT_2;
    let (l, r, scale) = match channels {
        3 => (
            input[0] + input[2] * center,
            input[1] + input[2] * center,
            1.,
        ),
        4 => (input[0] + input[2], input[1] + input[3], center),
        6 | 8 => {
            let shared = (input[2] + input[3]) * center;
            let mut l = input[0] + shared + input[4];
            let mut r = input[1] + shared + input[5];
            if channels == 8 {
                l += input[6];
                r += input[7];
            }
            (l, r, if channels == 6 { 1. / 3f32.sqrt() } else { 0.5 })
        }
        12 => {
            let shared = (input[2] + input[3] + input[8] + input[9]) * center;
            (
                input[0] + shared + input[4] + input[6] + input[10],
                input[1] + shared + input[5] + input[7] + input[11],
                1. / 6f32.sqrt(),
            )
        }
        _ => (input[0], input[1], 1.),
    };
    out[0] = l * scale;
    out[1] = r * scale;
    balance(&mut out, 1., pan)?;
    Ok(out)
}

impl<'a> Renderer<'a> {
    pub fn new(
        program: &'a Program,
        samples: HashMap<String, Arc<Sample>>,
        rate: u32,
    ) -> Result<Self> {
        ensure!((8_000..=192_000).contains(&rate), "Invalid UVI render rate");
        let unsupported = preflight(program);
        ensure!(
            unsupported.is_empty(),
            "Native UVI graph preflight failed: {}",
            serde_json::to_string(&unsupported)?
        );
        let mut validated_samples = HashSet::new();
        let mut sample_bytes = 0usize;
        for sample in samples.values() {
            if validated_samples.insert(Arc::as_ptr(sample)) {
                validate_sample(sample)?;
                sample_bytes = sample_bytes
                    .checked_add(sample.interleaved.bytes())
                    .context("UVI sample memory accounting overflow")?;
            }
        }
        ensure!(
            sample_bytes <= SAMPLE_MEMORY_LIMIT,
            "UVI decoded samples exceed 512-MiB budget"
        );
        let mut children = vec![Vec::new(); program.nodes.len()];
        for (id, n) in program.nodes.iter().enumerate() {
            if let Some(parent) = n.parent {
                children[parent].push(id);
            }
        }
        let parameters: Vec<_> = program.nodes.iter().map(|n| n.attributes.clone()).collect();
        let mut number_slots = Vec::new();
        let numbers = parameters
            .iter()
            .enumerate()
            .map(|(node, attributes)| {
                attributes
                    .iter()
                    .map(|(name, text)| {
                        let index = number_slots.len();
                        number_slots.push(CachedNumber::new(node, name.clone(), text));
                        (name.clone(), index)
                    })
                    .collect()
            })
            .collect();
        let mut renderer = Self {
            program,
            processed_blocks: (0..program.nodes.len()).map(|_| Cell::new(0)).collect(),
            last_processed_block: (0..program.nodes.len()).map(|_| Cell::new(u64::MAX)).collect(),
            insert_bypassed: (0..program.nodes.len()).map(|_| Cell::new(false)).collect(),
            runtime_nodes: Vec::new(),
            parameters,
            numbers,
            number_slots,
            active_numbers: Vec::new(),
            effective_generation: 0,
            samples: Arc::new(samples),
            rate: f64::from(rate),
            frame: 0,
            next_instance: 0,
            next_launch: 0,
            snapshots: HashMap::new(),
            voices: Vec::new(),
            processors: HashMap::new(),
            children,
            processor_ids: HashMap::new(),
            scope_nodes: HashMap::new(),
            source_channels: HashMap::new(),
            global_channels: 2,
            buses: HashMap::new(),
            routes: HashMap::new(),
            modulation: ModulationGraph::new(program)?,
            live: HashMap::new(),
            registered_live_valid: true,
            controllers: [[0; 128]; 16],
            bends: [0.; 16],
            pressures: [0.; 16],
            poly_pressures: [[0; 128]; 16],
            tempo: 120.,
        };
        for zone in &program.sample_zones {
            let sample = renderer
                .samples
                .get(&zone.sample_path)
                .context("Missing resolved UVI sample resource")?;
            source_layout(sample.channels)?;
            let count = renderer.source_channels.entry(zone.keygroup).or_default();
            *count = (*count).max(sample.channels);

            renderer.sample_playback(zone.player, sample)?;
        }
        for (id, node) in program
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| generator::supports(&n.kind))
        {
            let group = parent(program, id).context("Generator has no Keygroup")?;
            ensure!(
                program.nodes[group].kind == "Keygroup",
                "Generator must belong to a Keygroup"
            );
            let table = node
                .attributes
                .get("WavetablePath")
                .map(|path| {
                    renderer
                        .samples
                        .get(path)
                        .cloned()
                        .context("Unresolved UVI wavetable")
                })
                .transpose()?;
            let generator = Generator::new(node, renderer.rate, table)?;
            let width = renderer.source_channels.entry(group).or_insert(1);
            *width = (*width).max(generator.channels());
        }
        for (id, node) in program.nodes.iter().enumerate() {
            let processor = effects::supports(&node.kind)
                || filter::supports(&node.kind)
                || time_effects::supports(&node.kind)
                || waveshaper::supports(&node.kind)
                || maximizer::supports(&node.kind)
                || sparkverb::supports(&node.kind)
                || phasor::supports(&node.kind)
                || matches!(
                    node.kind.as_str(),
                    "GainMatrix" | "Gain" | "OnePole" | "TrackDelay"
                );
            if processor
                || generator::supports(&node.kind)
                || matches!(
                    node.kind.as_str(),
                    "Program"
                        | "Layer"
                        | "Keygroup"
                        | "SamplePlayer"
                        | "AuxEffect"
                        | "BusRouter"
                        | "EffectRack"
                )
            {
                let mut scope = Some(id);
                while scope.is_some_and(|n| program.nodes[n].kind != "Keygroup") {
                    scope = program.nodes[scope.unwrap()].parent;
                }
                renderer.scope_nodes.entry(scope).or_default().insert(id);
                if processor {
                    renderer.processor_ids.entry(scope).or_default().push(id);
                }
            }
        }
        renderer.runtime_nodes = program.nodes.iter().enumerate().filter_map(|(id, node)| {
            (node.kind == "SamplePlayer" || generator::supports(&node.kind)
                || effects::supports(&node.kind) || filter::supports(&node.kind)
                || time_effects::supports(&node.kind) || waveshaper::supports(&node.kind)
                || maximizer::supports(&node.kind) || sparkverb::supports(&node.kind)
                || phasor::supports(&node.kind)
                || matches!(node.kind.as_str(), "GainMatrix" | "Gain" | "OnePole" | "TrackDelay"))
                .then_some(id)
        }).collect();
        ensure!(
            renderer.delay_bytes(None) <= PROCESSOR_MEMORY_LIMIT,
            "Native UVI delay state exceeds memory budget"
        );
        renderer.processors = renderer.make_processors(None, renderer.global_channels)?;
        for (id, n) in program
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.kind == "BusRouter")
        {
            let dest = n
                .attributes
                .get("Destination")
                .context("UVI bus send has no Destination")?;
            let target = resolve_path(program, id, dest)?;
            ensure!(
                program.nodes[target].kind == "AuxEffect",
                "UVI send must target an AuxEffect"
            );
            // A destination aux must belong to an ancestor mixer, never itself
            // or a descendant: this makes traversal deterministic and acyclic.
            let owner = parent(program, id).context("UVI bus send has no owner")?;
            let mixer = parent(program, target).context("UVI auxiliary has no owner")?;
            let mut scope = Some(owner);
            while scope.is_some_and(|s| s != mixer) {
                scope = parent(program, scope.unwrap());
            }
            ensure!(
                scope.is_some(),
                "Feedback or cross-branch UVI auxiliary routing is unsupported"
            );
            renderer.routes.insert(id, target);
        }
        Ok(renderer)
    }
    /// Install caller-prepared PCM off the rendering worker. Existing aliases
    /// are immutable so active oscillators keep their original resource.
    /// Effect maps change here; their current convolution changes only on load.
    pub fn install_prepared_samples(
        &mut self,
        additions: HashMap<String, Arc<Sample>>,
    ) -> Result<()> {
        let mut unique = HashSet::new();
        let mut bytes = 0usize;
        for sample in self.samples.values() {
            if unique.insert(Arc::as_ptr(sample)) {
                bytes = bytes
                    .checked_add(sample.interleaved.bytes())
                    .context("UVI sample memory accounting overflow")?;
            }
        }
        let mut changed = false;
        for (path, sample) in &additions {
            if let Some(existing) = self.samples.get(path) {
                ensure!(
                    Arc::ptr_eq(existing, sample),
                    "UVI prepared resource alias already refers to different PCM"
                );
            } else {
                changed = true;
            }
            if unique.insert(Arc::as_ptr(sample)) {
                validate_sample(sample)?;
                bytes = bytes
                    .checked_add(sample.interleaved.bytes())
                    .context("UVI sample memory accounting overflow")?;
            }
        }
        ensure!(
            bytes <= SAMPLE_MEMORY_LIMIT,
            "UVI decoded samples exceed 512-MiB budget"
        );
        if !changed {
            return Ok(());
        }
        let mut resources = (*self.samples).clone();
        resources.extend(additions);
        let resources = Arc::new(resources);
        for processor in self.processors.values_mut().chain(
            self.voices
                .iter_mut()
                .flat_map(|voice| voice.processors.values_mut()),
        ) {
            if let Processor::Effect(effect) = processor {
                effect.replace_resources(Arc::clone(&resources));
            }
        }
        self.samples = resources;
        Ok(())
    }
    /// Absolute output position of the next rendered frame.
    pub fn current_frame(&self) -> u64 {
        self.frame
    }
    fn record_processing(&self, node: NodeId) {
        let block = self.frame / 256;
        if self.last_processed_block[node].get() != block {
            self.last_processed_block[node].set(block);
            let blocks = &self.processed_blocks[node];
            blocks.set(blocks.get().saturating_add(1));
        }
    }
    pub fn runtime_evidence(&self) -> Vec<NodeRuntimeEvidence> {
        self.runtime_nodes.iter().map(|&node| NodeRuntimeEvidence {
            node,
            processed_blocks: self.processed_blocks[node].get(),
            currently_bypassed: self.base_number(node, "Bypass", 0.).ok().and_then(|value| {
                if value == 0. { Some(false) } else if value == 1. { Some(true) } else { None }
            }),
            retained_voice_instances: self.voices.iter().filter(|voice| {
                voice.processors.contains_key(&node) || voice.oscillators.iter().any(|osc| osc.player == node)
            }).count(),
        }).collect()
    }
    /// Planned native sources need complete musical-event lookahead within
    /// fixed 256-frame blocks, independent of the requested output length.
    pub fn requires_planned_segments(&self) -> bool {
        self.modulation.requires_planned_segments()
    }
    pub fn diagnostics(&self) -> Vec<&'static str> {
        super::diagnostics::fidelity_diagnostics()
    }
    fn base_number(&self, node: NodeId, name: &str, default: f64) -> Result<f64> {
        if let Some(&index) = self.numbers[node].get(name) {
            let number = &self.number_slots[index];
            if let Some(value) = number.base {
                return Ok(value);
            }
            // Preserve the original error for a nonnumeric or nonfinite attribute.
            if self.parameters[node].contains_key(name) {
                return numeric(&self.parameters, node, name, default);
            }
        }
        ensure!(default.is_finite(), "Nonfinite UVI parameter {name}");
        Ok(default)
    }
    fn number(&self, node: NodeId, name: &str, default: f64) -> Result<f64> {
        if let Some(&index) = self.numbers[node].get(name) {
            let number = &self.number_slots[index];
            if let Some((generation, value)) = number.effective {
                if generation == self.effective_generation {
                    return Ok(value);
                }
            }
            if let Some(value) = number.base {
                return Ok(value);
            }
            if self.parameters[node].contains_key(name) {
                return numeric(&self.parameters, node, name, default);
            }
        }
        ensure!(default.is_finite(), "Nonfinite UVI parameter {name}");
        Ok(default)
    }
    fn release_finished(&self, input: &Inputs, nodes: &HashSet<NodeId>) -> Result<bool> {
        if self.registered_live_valid {
            self.modulation.release_registered_finished(input, nodes)
        } else {
            self.modulation.release_finished(input, &self.live, nodes)
        }
    }
    fn evaluate_scope(&mut self, inputs: Inputs, scope: Option<NodeId>) -> Result<()> {
        // Persistent slots retain their keys and storage. An epoch expires the
        // prior voice/scope; only active slot indices are rebuilt each sample.
        let generation = self
            .effective_generation
            .checked_add(1)
            .context("UVI control evaluation identity overflow")?;
        self.active_numbers.clear();
        let numbers = &mut self.numbers;
        let slots = &mut self.number_slots;
        let active = &mut self.active_numbers;
        let emit = |parameter: &Parameter, value| {
            let (node, name) = parameter;
            let index = if let Some(&index) = numbers[*node].get(name.as_str()) {
                index
            } else {
                let index = slots.len();
                slots.push(CachedNumber {
                    parameter: parameter.clone(),
                    ..Default::default()
                });
                numbers[*node].insert(name.clone(), index);
                index
            };
            slots[index].effective = Some((generation, value));
            active.push(index);
        };
        if self.registered_live_valid {
            self.modulation.evaluate_registered_nodes_into(
                &inputs,
                &self.scope_nodes[&scope],
                emit,
            )?;
        } else {
            self.modulation.evaluate_nodes_into(
                &inputs,
                &self.live,
                &self.scope_nodes[&scope],
                emit,
            )?;
        }
        self.effective_generation = generation;
        for &index in &self.active_numbers {
            if self.number_slots[index].dynamic.is_none() {
                self.number_slots[index].dynamic = Some(
                    self.modulation
                        .target_has_dynamic_source(&self.number_slots[index].parameter),
                );
            }
        }
        Ok(())
    }
    fn dynamic_source(&self, node: NodeId, name: &str) -> bool {
        self.numbers[node]
            .get(name)
            .and_then(|&index| self.number_slots[index].dynamic)
            .unwrap_or(false)
    }
    fn cache_effective_value(&mut self, node: NodeId, name: &str, value: f64) {
        let index = if let Some(&index) = self.numbers[node].get(name) {
            index
        } else {
            let index = self.number_slots.len();
            self.number_slots.push(CachedNumber {
                parameter: (node, name.into()),
                dynamic: Some(false),
                ..Default::default()
            });
            self.numbers[node].insert(name.into(), index);
            index
        };
        self.number_slots[index].effective = Some((self.effective_generation, value));
    }
    fn set_parameter_text(&mut self, node: NodeId, name: String, text: String) {
        if let Some(&index) = self.numbers[node].get(name.as_str()) {
            self.number_slots[index].base = text.parse::<f64>().ok().filter(|n| n.is_finite());
        } else {
            let index = self.number_slots.len();
            self.number_slots
                .push(CachedNumber::new(node, name.clone(), &text));
            self.numbers[node].insert(name.clone(), index);
        }
        self.parameters[node].insert(name, text);
    }
    fn boolean(&self, node: NodeId, name: &str) -> Result<bool> {
        let value = self.number(node, name, 0.)?;
        ensure!(
            value == 0. || value == 1.,
            "Invalid UVI Boolean parameter {name}"
        );
        Ok(value == 1.)
    }
    /// UVI async API uses sample-frame marker coordinates. Authored native
    /// fixtures established forward Stop exclusive, reverse Stop inclusive;
    /// FLAC foreign-RIFF sampler ends clamp to the last frame without scaling.
    fn sample_playback(&self, player: NodeId, sample: &Sample) -> Result<SamplePlayback> {
        let options: Vec<_> = self.children[player]
            .iter()
            .copied()
            .filter(|&id| self.program.nodes[id].kind == "PlaybackOptions")
            .collect();
        ensure!(options.len() <= 1, "Multiple UVI playback-options nodes");
        let reverse = self.boolean(player, "Reverse")?;
        let play_release = if let Some(&id) = options.first() {
            let value = self.base_number(id, "PlayRelease", 1.)?;
            ensure!([0., 1.].contains(&value), "Invalid UVI looped-release flag");
            value != 0.
        } else {
            true
        };
        let (start, end, marker_span, reverse, silent, loop_data) =
            if let Some(&id) = options.first() {
                let marker = |node, name, default| -> Result<usize> {
                    let value = self.base_number(node, name, default)?;
                    ensure!(
                        value >= 0. && value.fract() == 0. && value <= u32::MAX as f64,
                        "Invalid UVI sample {name} marker"
                    );
                    Ok(value as usize)
                };
                let start = marker(id, "Start", 0.)?;
                let stop = marker(id, "Stop", sample.frames as f64)?;
                ensure!(
                    start <= stop && stop <= sample.frames,
                    "Invalid or inverted UVI sample playback markers"
                );
                let direction = self.base_number(id, "PlayDirection", 0.)?;
                ensure!(
                    [0., 1.].contains(&direction),
                    "Unverified UVI serialized playback direction"
                );
                let reverse = reverse || direction == 1.;
                let play_release = self.base_number(id, "PlayRelease", 1.)?;
                ensure!(
                    [0., 1.].contains(&play_release),
                    "Invalid UVI looped-release flag"
                );
                ensure!(
                    self.children[id].len() <= 1
                        && self.children[id]
                            .iter()
                            .all(|&n| self.program.nodes[n].kind == "Loop"),
                    "Unsupported UVI playback-options child"
                );
                let silent = reverse && stop == sample.frames;
                ensure!(
                    !silent || self.children[id].is_empty(),
                    "Reverse physical-EOF loop behavior is unverified"
                );
                let loop_data = if let Some(&loop_id) = self.children[id].first() {
                    ensure!(
                        self.base_number(loop_id, "Type", 0.)? == 0.,
                        "Serialized alternate/one-shot loop law is unverified"
                    );
                    let loop_start = marker(loop_id, "Start", 0.)?;
                    let loop_end = marker(loop_id, "End", sample.frames as f64)?.min(sample.frames);
                    ensure!(loop_start < loop_end, "Invalid UVI forward loop markers");
                    Some(SampleLoop {
                        id: loop_id as u32,
                        kind: 0,
                        start: loop_start as u32,
                        end: (loop_end - 1) as u32,
                        fraction: 0,
                        play_count: 0,
                    })
                } else {
                    None
                };
                (
                    start,
                    (stop + usize::from(reverse)).min(sample.frames),
                    stop - start,
                    reverse,
                    silent,
                    loop_data,
                )
            } else {
                ensure!(
                    sample.loops.len() <= 1,
                    "Multiple sample loops are not implemented"
                );
                // WAV smpl markers are retained by the decoder but native loadSample
                // ignores them. Only FLAC APPLICATION riff imports file loops.
                let mut loop_data = sample
                    .riff_metadata
                    .iter()
                    .any(|block| block.starts_with(b"riff"))
                    .then(|| sample.loops.first().cloned())
                    .flatten();
                if let Some(l) = &mut loop_data {
                    ensure!(
                        (l.start as usize) < sample.frames,
                        "Invalid UVI file loop start"
                    );
                    l.end = l.end.min((sample.frames - 1) as u32);
                }
                (0, sample.frames, sample.frames, reverse, false, loop_data)
            };
        if let Some(l) = &loop_data {
            ensure!(
                l.kind <= 2
                    && l.fraction == 0
                    && l.start <= l.end
                    && l.start as usize >= start
                    && (l.end as usize) < end,
                "Unsupported or invalid effective sample loop bounds"
            );
        }
        Ok(SamplePlayback {
            start,
            end,
            marker_span,
            reverse,
            silent,
            play_release,
            loop_data,
        })
    }
    fn inputs(&self, voice: Option<&Voice>) -> Inputs {
        let channel = voice.map_or(0, |v| v.note.channel) as usize;
        Inputs {
            sample_rate: self.rate,
            control_block_frames: 256,
            host_tempo: self.tempo,
            key: voice.map_or(60, |v| v.note.note),
            tune_semitones: voice.map_or(0., |v| v.note.tune),
            velocity: voice.map_or(127, |v| v.note.velocity),
            controllers: self.controllers[channel],
            pitch_bend: self.bends[channel],
            channel_pressure: self.pressures[channel],
            poly_pressure: voice.map_or(0., |v| {
                f64::from(self.poly_pressures[channel][v.note.note as usize]) / 127.
            }),
            time_seconds: self.frame as f64 / self.rate,
            voice_time_seconds: voice.map_or(0., |v| (self.frame - v.started) as f64 / self.rate),
            voice: voice.map(|v| v.note.id),
            instance: voice.map(|v| v.instance),
            note_off_time_seconds: voice.and_then(|v| {
                v.note_off
                    .map(|frame| (frame - v.started) as f64 / self.rate)
            }),
            ..Default::default()
        }
    }
    fn update_processors(&self, processors: &mut HashMap<NodeId, Processor>) -> Result<()> {
        for &index in &self.active_numbers {
            let number = &self.number_slots[index];
            let (id, name) = &number.parameter;
            let value = &number
                .effective
                .context("Missing evaluated UVI parameter")?
                .1;
            if let Some(processor) = processors.get_mut(id) {
                if let Processor::Gain(gain) = processor
                    && name == "Volume"
                {
                    gain.set_effective_volume(*value)?;
                } else if let Processor::Filter(filter) = processor {
                    let typed = if name == "Bypass" {
                        ParameterValue::Boolean(*value >= 0.5)
                    } else {
                        ParameterValue::Number(*value)
                    };
                    filter.set_effective_parameter(name, &typed, number.dynamic.unwrap_or(false))
                        .with_context(|| format!("Modulated UVI filter parameter {name}={value} at node {id}, frame {}", self.frame))?;
                } else if let Processor::Time(effect) = processor {
                    effect.set_effective(name, &ParameterValue::Number(*value))
                        .with_context(|| format!("Modulated UVI time parameter {name}={value} at node {id}, frame {}",self.frame))?;
                } else {
                    processor.set(name, *value).with_context(|| {
                        format!(
                            "Modulated UVI parameter {name}={value} at node {id}, frame {}",
                            self.frame
                        )
                    })?;
                }
                if name == "Bypass" {
                    self.insert_bypassed[*id].set(*value >= 0.5);
                }
            }
        }
        Ok(())
    }
    fn processor_bytes(&self) -> usize {
        self.processors
            .values()
            .chain(self.voices.iter().flat_map(|v| v.processors.values()))
            .fold(0usize, |bytes, p| bytes.saturating_add(p.memory_bytes()))
    }
    fn check_processor_memory(&self) -> Result<()> {
        ensure!(
            self.processor_bytes() <= PROCESSOR_MEMORY_LIMIT,
            "UVI processing buffers exceed memory budget"
        );
        Ok(())
    }
    fn delay_bytes(&self, group: Option<NodeId>) -> usize {
        self.processor_ids
            .get(&group)
            .into_iter()
            .flatten()
            .map(|&id| {
                if self.program.nodes[id].kind == "TrackDelay" {
                    (5. * self.rate + 2.) as usize
                        * group.map_or(self.global_channels, |id| self.source_channels[&id])
                        * 4
                } else if time_effects::supports(&self.program.nodes[id].kind) {
                    (5. * self.rate + 4.) as usize * 8
                } else {
                    0
                }
            })
            .sum()
    }
    fn make_processors(
        &self,
        keygroup: Option<NodeId>,
        channels: usize,
    ) -> Result<HashMap<NodeId, Processor>> {
        let mut result = HashMap::new();
        let mut bytes = self.processor_bytes();
        for &id in self
            .processor_ids
            .get(&keygroup)
            .map(Vec::as_slice)
            .unwrap_or(&[])
        {
            let n = &self.program.nodes[id];
            let live_node = super::program::ProgramNode {
                parent: n.parent,
                kind: n.kind.clone(),
                name: n.name.clone(),
                attributes: self.parameters[id].clone(),
                text: String::new(),
            };
            if let Some(mut p) = Processor::new(&live_node, self.rate, channels, &self.samples)
                .with_context(|| {
                    format!(
                        "UVI {} constructor at node {id}, channels {channels}, rate {}, frame {}",
                        n.kind, self.rate, self.frame
                    )
                })?
            {
                for (name, value) in &self.parameters[id] {
                    if matches!(
                        p,
                        Processor::Effect(_)
                            | Processor::Filter(_)
                            | Processor::Time(_)
                            | Processor::Wave(_)
                            | Processor::Max(_)
                            | Processor::Spark(_)
                            | Processor::Phasor(_)
                    ) {
                        continue;
                    }
                    if name == "Bypass"
                        || name == "Volume"
                        || name == "Freq"
                        || name == "Mode"
                        || name == "KeyTracking"
                        || name == "DelayTime"
                        || name == "SyncToHost"
                        || name.starts_with("Gain_")
                    {
                        p.set(name, value.parse()?)?;
                    }
                }
                match &mut p {
                    Processor::Delay(delay) => delay.set_tempo(self.tempo)?,
                    Processor::Time(effect) => effect.set_tempo(self.tempo)?,
                    Processor::Phasor(effect) => effect.set_tempo(self.tempo)?,
                    _ => {}
                }
                bytes = bytes.saturating_add(p.memory_bytes());
                ensure!(
                    bytes <= PROCESSOR_MEMORY_LIMIT,
                    "UVI processing buffers exceed memory budget"
                );
                self.insert_bypassed[id].set(p.bypassed()?);
                result.insert(id, p);
            }
        }
        Ok(result)
    }
    fn collection(&self, owner: NodeId, kind: &str) -> &[NodeId] {
        self.children[owner]
            .iter()
            .find(|&&id| self.program.nodes[id].kind == kind)
            .map(|&id| self.children[id].as_slice())
            .unwrap_or(&[])
    }
    fn permitted(&self, id: NodeId, note: &script::Note) -> Result<bool> {
        if self.boolean(id, "Bypass")? || self.boolean(id, "MidiMute")? {
            return Ok(false);
        }
        Ok(f64::from(note.note) >= self.number(id, "LowKey", 0.)?
            && f64::from(note.note) <= self.number(id, "HighKey", 127.)?
            && f64::from(note.velocity) >= self.number(id, "LowVelocity", 1.)?
            && f64::from(note.velocity) <= self.number(id, "HighVelocity", 127.)?)
    }
    fn start(&mut self, note: &script::Note, root: Option<script::HostRoot>) -> Result<()> {
        ensure!(
            note.id > 0
                && note.note < 128
                && note.velocity > 0
                && note.velocity < 128
                && note.channel < 16
                && note.volume.is_finite()
                && note.volume >= 0.
                && note.tune.is_finite()
                && (-1. ..=1.).contains(&note.pan),
            "Invalid UVI playback note"
        );
        let posted_note = note.clone();
        let mut transposed = note.clone();
        let transpose = self.number(self.program.root, "TransposeOctaves", 0.)? * 12.
            + self.number(self.program.root, "TransposeSemiTones", 0.)?;
        ensure!(transpose.fract() == 0., "Noninteger UVI transposition");
        let key = f64::from(note.note) + transpose;
        if !(0. ..=127.).contains(&key) {
            return Ok(());
        }
        transposed.note = key as u8;
        let note = &transposed;
        ensure!(
            note.dim1 == 0 && note.dim2.is_none(),
            "SampleMapping dimensions cannot route a native SamplePlayer graph"
        );
        if let Some(layers) = &note.layers {
            if layers.is_empty() {
                return Ok(());
            }
            ensure!(
                layers.iter().all(|id| self.program.layers.contains(id)),
                "Invalid UVI note layer routing"
            );
        }
        ensure!(
            note.oscillator.is_none_or(|index| index > 0),
            "UVI oscillator routing is one-based"
        );
        if !self.permitted(self.program.root, note)? {
            return Ok(());
        }
        let solo = self
            .program
            .layers
            .iter()
            .any(|&id| self.boolean(id, "Solo").unwrap_or(false));
        let groups: Vec<_> = self
            .program
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.kind == "Keygroup")
            .map(|(id, _)| {
                Ok((
                    id,
                    parent(self.program, id).context("Keygroup has no Layer")?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        self.next_launch = self
            .next_launch
            .checked_add(1)
            .context("UVI note launch identity overflow")?;
        let launch = self.next_launch;
        self.snapshots.insert(note.id, posted_note);
        let mut program_started = false;
        for (group, layer) in groups {
            if note
                .layers
                .as_ref()
                .is_some_and(|layers| !layers.contains(&layer))
            {
                continue;
            }
            if !self.permitted(layer, note)?
                || !self.permitted(group, note)?
                || (solo && !self.boolean(layer, "Solo")?)
            {
                continue;
            }
            let mut oscillators = Vec::new();
            for (index, &player) in self.collection(group, "Oscillators").iter().enumerate() {
                if note
                    .oscillator
                    .is_some_and(|selected| selected != index + 1)
                {
                    continue;
                }
                if self.boolean(player, "Bypass")? {
                    continue;
                }
                let node = &self.program.nodes[player];
                if generator::supports(&node.kind) {
                    let live_node = super::program::ProgramNode {
                        parent: node.parent,
                        kind: node.kind.clone(),
                        name: node.name.clone(),
                        attributes: self.parameters[player].clone(),
                        text: String::new(),
                    };
                    let table = self.parameters[player]
                        .get("WavetablePath")
                        .map(|path| {
                            self.samples
                                .get(path)
                                .cloned()
                                .context("Unresolved live UVI wavetable")
                        })
                        .transpose()?;
                    oscillators.push(Oscillator {
                        player,
                        path: String::new(),
                        position: 0.,
                        start: 0,
                        end: 0,
                        loop_data: None,
                        play_release: true,
                        direction: 1.,
                        loops_completed: 0,
                        done: false,
                        generator: Some(Generator::new_seeded(
                            &live_node,
                            self.rate,
                            table,
                            launch ^ (player as u64).rotate_left(32),
                        )?),
                        gain: None,
                    });
                    continue;
                }
                ensure!(
                    node.kind == "SamplePlayer",
                    "Unsupported Keygroup oscillator"
                );
                if self.boolean(player, "SamplePurged")? {
                    continue;
                }
                let path = self.parameters[player]
                    .get("SamplePath")
                    .context("Missing UVI sample path")?
                    .clone();
                let sample = self
                    .samples
                    .get(&path)
                    .context("Unresolved live UVI sample resource")?;
                let start = self.number(player, "SampleStart", 0.)?;
                ensure!(
                    (0. ..=1.).contains(&start),
                    "Invalid normalized UVI sample start"
                );
                let offset = self.number(player, "SampleStartMillisecond", 0.)?
                    * f64::from(sample.rate)
                    / 1000.
                    + note.offset_us as f64 * f64::from(sample.rate) / 1_000_000.;
                ensure!(offset >= 0., "Negative UVI sample offset");
                let playback = self.sample_playback(player, sample)?;
                let reverse = playback.reverse;
                let position = if reverse {
                    playback.end.saturating_sub(1) as f64
                        - start * playback.marker_span as f64
                        - offset
                } else {
                    playback.start as f64 + start * playback.marker_span as f64 + offset
                };
                oscillators.push(Oscillator {
                    player,
                    path,
                    position,
                    start: playback.start,
                    end: playback.end,
                    loop_data: playback.loop_data,
                    play_release: playback.play_release,
                    direction: if reverse { -1. } else { 1. },
                    loops_completed: 0,
                    done: playback.silent,
                    generator: None,
                    gain: None,
                });
            }
            if oscillators.is_empty() {
                continue;
            }
            let polyphony = self.number(layer, "CustomPolyphony", 0.)? as usize;
            let ids = self
                .voices
                .iter()
                .filter(|v| v.layer == layer)
                .map(|v| v.note.id)
                .collect::<std::collections::BTreeSet<_>>();
            if polyphony > 0 && !ids.contains(&note.id) && ids.len() >= polyphony {
                if let Some(old) = self
                    .voices
                    .iter()
                    .find(|v| v.layer == layer)
                    .map(|v| v.note.id)
                {
                    self.voices.retain(|v| {
                        let keep = v.layer != layer || v.note.id != old;
                        if !keep {
                            self.modulation.remove_instance(v.note.id, v.instance);
                        }
                        keep
                    });
                    if !self.voices.iter().any(|v| v.note.id == old) {
                        self.modulation.remove_voice(old);
                        self.snapshots.remove(&old);
                    }
                }
            }
            if !program_started {
                let polyphony = self.number(self.program.root, "Polyphony", 0.)? as usize;
                let ids = self
                    .voices
                    .iter()
                    .map(|v| v.note.id)
                    .collect::<std::collections::BTreeSet<_>>();
                if polyphony > 0 && !ids.contains(&note.id) && ids.len() >= polyphony {
                    if let Some(old) = self.voices.first().map(|v| v.note.id) {
                        self.voices.retain(|v| v.note.id != old);
                        self.modulation.remove_voice(old);
                        self.snapshots.remove(&old);
                    }
                }
                program_started = true;
            }
            ensure!(
                self.voices.len() < VOICE_LIMIT,
                "UVI playback voice limit exceeded"
            );
            let delay_bytes = self.delay_bytes(None)
                + self
                    .voices
                    .iter()
                    .map(|v| self.delay_bytes(Some(v.keygroup)))
                    .sum::<usize>()
                + self.delay_bytes(Some(group));
            ensure!(
                delay_bytes <= PROCESSOR_MEMORY_LIMIT,
                "Native UVI voice delay state exceeds memory budget"
            );
            let channels = oscillators
                .iter()
                .map(|osc| {
                    osc.generator
                        .as_ref()
                        .map_or_else(|| self.samples[&osc.path].channels, Generator::channels)
                })
                .max()
                .unwrap_or(1);
            let mut processors = self.make_processors(Some(group), channels)?;
            for processor in processors.values_mut() {
                if let Processor::Filter(filter) = processor {
                    filter.set_note(note.note)?;
                }
            }
            self.next_instance = self
                .next_instance
                .checked_add(1)
                .context("UVI voice instance identity overflow")?;
            let mut voice = Voice {
                root,
                note: note.clone(),
                started: self.frame,
                instance: self.next_instance,
                launch,
                key_released: false,
                note_off: None,
                channels,
                fade: None,
                gain: GainClock::new(self.frame, 0.),
                keygroup: group,
                layer,
                oscillators,
                processors,
            };
            let targets = self.modulation.evaluate_nodes(
                &self.inputs(Some(&voice)),
                &self.live,
                &self.scope_nodes[&Some(group)],
            )?;
            let target = targets
                .get(&(group, "Gain".into()))
                .copied()
                .unwrap_or(self.base_number(group, "Gain", 1.)?);
            voice.gain = GainClock::new(self.frame, target as f32);
            for osc in &mut voice.oscillators {
                if osc.generator.is_none() {
                    let target = targets
                        .get(&(osc.player, "Gain".into()))
                        .copied()
                        .unwrap_or(self.base_number(osc.player, "Gain", 1.)?);
                    osc.gain = Some(GainClock::new(self.frame, target as f32));
                }
            }
            self.voices.push(voice);
        }
        if !self.voices.iter().any(|voice| voice.note.id == note.id) {
            self.snapshots.remove(&note.id);
        }
        Ok(())
    }
    fn release_pending(&mut self) -> Result<()> {
        let voices = std::mem::take(&mut self.voices);
        let mut removed = HashSet::new();
        for mut voice in voices {
            if voice.key_released
                && voice.note_off.is_none()
                && self.controllers[voice.note.channel as usize][64] < 64
            {
                voice.note_off = Some(self.frame);
                for oscillator in &mut voice.oscillators {
                    if !oscillator.play_release {
                        oscillator.loop_data = None;
                    }
                }
                let nodes = &self.scope_nodes[&Some(voice.keygroup)];
                if !self.modulation.has_release_envelopes(nodes)
                    || self.release_finished(&self.inputs(Some(&voice)), nodes)?
                {
                    removed.insert(voice.note.id);
                    self.modulation
                        .remove_instance(voice.note.id, voice.instance);
                    continue;
                }
            }
            self.voices.push(voice);
        }
        for id in removed {
            if !self.voices.iter().any(|voice| voice.note.id == id) {
                self.modulation.remove_voice(id);
                self.snapshots.remove(&id);
            }
        }
        Ok(())
    }
    fn release_key(&mut self, id: u32, note: u8, channel: u8, layer: Option<NodeId>) -> Result<()> {
        ensure!(
            note < 128 && channel < 16 && layer.is_none_or(|id| self.program.layers.contains(&id)),
            "Invalid UVI key-release routing"
        );
        let key = f64::from(note)
            + self.base_number(self.program.root, "TransposeOctaves", 0.)? * 12.
            + self.base_number(self.program.root, "TransposeSemiTones", 0.)?;
        let mut targets = HashSet::new();
        for &owner in &self.program.layers {
            if layer.is_some_and(|layer| owner != layer) {
                continue;
            }
            if let Some(voice) = self.voices.iter().find(|voice| {
                voice.note.id == id
                    && f64::from(voice.note.note) == key
                    && voice.layer == owner
                    && !voice.key_released
            }) {
                targets.insert((owner, voice.launch));
            }
        }
        // Native terminal NoteOff is FIFO within each Layer. All mic/keygroup
        // voices of the same terminal launch share that gate. MIDI channel only
        // routes Parts and does not reject a release within this Program.
        for voice in &mut self.voices {
            if targets.contains(&(voice.layer, voice.launch)) {
                voice.key_released = true;
            }
        }
        self.release_pending()
    }
    #[cfg(test)]
    fn apply_note(&mut self, action: &script::Action) -> Result<()> {
        self.apply_note_rooted(action, None)
    }
    fn apply_note_rooted(
        &mut self,
        action: &script::Action,
        root: Option<script::HostRoot>,
    ) -> Result<()> {
        match action {
            script::Action::ChokeRoot => {
                let root = root.context("Hosted choke has no ancestry")?;
                let mut killed = HashSet::new();
                self.voices.retain(|voice| {
                    if voice.root == Some(root) {
                        killed.insert(voice.note.id);
                        self.modulation.remove_instance(voice.note.id, voice.instance);
                        false
                    } else {
                        true
                    }
                });
                for id in killed {
                    if !self.voices.iter().any(|voice| voice.note.id == id) {
                        self.modulation.remove_voice(id);
                        self.snapshots.remove(&id);
                    }
                }
            }
            script::Action::Start(note) => self.start(note, root)?,
            script::Action::ReleaseNote {
                id,
                note,
                channel,
                layer,
            } => {
                self.release_key(*id, *note, *channel, *layer)?;
            }
            script::Action::Release(id) => {
                if let Some(note) = self.snapshots.remove(id) {
                    self.release_key(*id, note.note, note.channel, None)?;
                }
            }
            script::Action::Change {
                id,
                gain,
                tune,
                pan,
                layer,
                relative,
            } => {
                for voice in self
                    .voices
                    .iter_mut()
                    .filter(|v| v.note.id == *id && layer.is_none_or(|id| v.layer == id))
                {
                    if let Some(v) = gain {
                        let value = if *relative {
                            voice.note.volume * *v
                        } else {
                            *v
                        };
                        ensure!(
                            v.is_finite() && *v >= 0. && value.is_finite() && value >= 0.,
                            "Invalid UVI note gain"
                        );
                        voice.note.volume = value;
                    }
                    if let Some(v) = tune {
                        let value = if *relative { voice.note.tune + *v } else { *v };
                        ensure!(
                            v.is_finite() && value.is_finite() && value.abs() <= 120.,
                            "Invalid UVI note tuning"
                        );
                        voice.note.tune = value;
                    }
                    if let Some(v) = pan {
                        let value = if *relative { voice.note.pan + *v } else { *v };
                        ensure!(
                            (-1. ..=1.).contains(v) && (-1. ..=1.).contains(&value),
                            "Invalid UVI note pan"
                        );
                        voice.note.pan = value;
                    }
                }
            }
            script::Action::Fade {
                id,
                start,
                target,
                duration_frames,
                kill,
                layer,
            } => {
                ensure!(
                    (0. ..=1.).contains(target)
                        && start.is_none_or(|value| (0. ..=1.).contains(&value)),
                    "Invalid UVI voice fade"
                );
                for voice in self
                    .voices
                    .iter_mut()
                    .filter(|v| v.note.id == *id && layer.is_none_or(|id| v.layer == id))
                {
                    let initial = start
                        .unwrap_or_else(|| voice.fade.map_or(1., |fade| fade.value(self.frame)));
                    voice.fade = Some(VoiceFade {
                        start: initial,
                        target: *target,
                        begin: self.frame,
                        duration: *duration_frames,
                        kill: *kill,
                    });
                }
            }
            script::Action::ControllerAll { controller, value } => {
                ensure!(
                    *controller < 128 && *value < 128,
                    "Invalid UVI omnichannel controller"
                );
                for channel in &mut self.controllers {
                    channel[*controller as usize] = *value;
                }
                if *controller == 64 {
                    self.release_pending()?;
                }
            }
            script::Action::Controller {
                channel,
                controller,
                value,
            } => {
                ensure!(
                    *channel < 16 && *controller < 128 && *value < 128,
                    "Invalid UVI controller"
                );
                self.controllers[*channel as usize][*controller as usize] = *value;
                if *controller == 64 {
                    self.release_pending()?;
                }
            }
            script::Action::PitchBend { channel, bend } => {
                ensure!(
                    *channel < 16 && (-1. ..=1.).contains(bend),
                    "Invalid UVI pitch bend"
                );
                self.bends[*channel as usize] = *bend;
            }
            script::Action::AfterTouch { channel, value } => {
                ensure!(*channel < 16 && *value < 128, "Invalid UVI pressure");
                self.pressures[*channel as usize] = f64::from(*value) / 127.;
            }
            script::Action::PolyAfterTouch {
                channel,
                note,
                value,
            } => {
                ensure!(
                    *channel < 16 && *note < 128 && *value < 128,
                    "Invalid UVI polyphonic pressure"
                );
                self.poly_pressures[*channel as usize][*note as usize] = *value;
            }
            script::Action::PolyAfterTouchAll { note, value } => {
                ensure!(
                    *note < 128 && *value < 128,
                    "Invalid UVI omnichannel polyphonic pressure"
                );
                for channel in &mut self.poly_pressures {
                    channel[*note as usize] = *value;
                }
            }
            script::Action::Transport {
                playing: _,
                beat: _,
                tempo,
            } => {
                ensure!(tempo.is_finite() && *tempo > 0., "Invalid UVI tempo");
                self.tempo = *tempo;
                for p in self.processors.values_mut() {
                    match p {
                        Processor::Delay(p) => p.set_tempo(*tempo)?,
                        Processor::Time(p) => p.set_tempo(*tempo)?,
                        Processor::Phasor(p) => p.set_tempo(*tempo)?,
                        _ => {}
                    }
                }
                for v in &mut self.voices {
                    for p in v.processors.values_mut() {
                        match p {
                            Processor::Delay(p) => p.set_tempo(*tempo)?,
                            Processor::Time(p) => p.set_tempo(*tempo)?,
                            Processor::Phasor(p) => p.set_tempo(*tempo)?,
                            _ => {}
                        }
                    }
                }
            }
        }
        Ok(())
    }
    fn apply_host(&mut self, action: &host::Action) -> Result<()> {
        match action {
            host::Action::Parameter {
                node,
                parameter,
                value,
            } => {
                ensure!(
                    *node < self.parameters.len(),
                    "Invalid UVI parameter target"
                );
                let stochastic_trigger = parameter == "TriggerMode"
                    && matches!(
                        self.program.nodes[*node].kind.as_str(),
                        "StdRandom" | "Drunk"
                    );
                if stochastic_trigger {
                    ensure!(
                        matches!(value,ParameterValue::Number(n) if *n == self.base_number(*node,"TriggerMode",1.)?),
                        "Changing stochastic clock trigger ownership requires graph rebuild"
                    );
                }
                if !stochastic_trigger
                    && matches!(
                        parameter.as_str(),
                        "TriggerMode"
                            | "TriggerSync"
                            | "LatchTrigger"
                            | "ExclusiveGroup"
                            | "PlayMode"
                            | "VelocityCurve"
                            | "PortamentoMode"
                            | "NotePolyphony"
                    )
                {
                    ensure!(
                        matches!(value,ParameterValue::Number(n) if *n==0.)
                            || matches!(value, ParameterValue::Boolean(false)),
                        "Unsupported live UVI trigger/play mode"
                    );
                }
                if parameter == "TriggerRule" {
                    let mut at = Some(*node);
                    let mut ordinary = true;
                    while let Some(id) = at {
                        ordinary &= ["TriggerMode", "TriggerSync", "PlayMode"]
                            .iter()
                            .all(|name| self.number(id, name, 0.).is_ok_and(|v| v == 0.));
                        at = parent(self.program, id);
                    }
                    ensure!(
                        matches!(value,ParameterValue::Number(v) if *v == 0. || (*v == 2. && ordinary)),
                        "Unverified live UVI trigger rule"
                    );
                }
                if self.program.nodes[*node].kind == "MultiEnvelope" && parameter == "Retrigger" {
                    ensure!(
                        matches!(value, ParameterValue::Number(v) if *v == 1.),
                        "MultiEnvelope note-off-ignoring modes have no verified voice cleanup law"
                    );
                }
                if parameter == "NumVoicesPerNote" {
                    ensure!(
                        matches!(value,ParameterValue::Number(n) if *n==1.),
                        "Unsupported live UVI unison voice count"
                    );
                }
                let text = match value {
                    ParameterValue::Number(n) => {
                        ensure!(n.is_finite(), "Nonfinite UVI parameter");
                        n.to_string()
                    }
                    ParameterValue::Boolean(v) => u8::from(*v).to_string(),
                    ParameterValue::Text(s) => s.clone(),
                };
                let kind = self.program.nodes[*node].kind.as_str();
                if (kind == "SignalConnection"
                    && matches!(
                        parameter.as_str(),
                        "Source" | "Destination" | "Mapper" | "ConnectionMode"
                    ))
                    || (kind == "ScriptEventModulation"
                        && matches!(parameter.as_str(), "EventId" | "Bipolar"))
                {
                    ensure!(
                        self.parameters[*node].get(parameter) == Some(&text),
                        "Changing compiled UVI control identity/endpoint/mode requires graph rebuild"
                    );
                }
                let mut bytes = self.processor_bytes();
                if let Some(p) = self.processors.get_mut(node) {
                    p.set_value_bounded(parameter, value, &mut bytes)
                        .with_context(|| {
                            format!(
                                "UVI parameter {parameter}={value:?} at node {node}, frame {}",
                                self.frame
                            )
                        })?;
                    if parameter == "Bypass" {
                        self.insert_bypassed[*node].set(p.bypassed()?);
                    }
                }
                for v in &mut self.voices {
                    if let Some(p) = v.processors.get_mut(node) {
                        p.set_value_bounded(parameter, value, &mut bytes)
                            .with_context(|| {
                                format!(
                                    "UVI parameter {parameter}={value:?} at node {node}, frame {}",
                                    self.frame
                                )
                            })?;
                        if parameter == "Bypass" {
                            self.insert_bypassed[*node].set(p.bypassed()?);
                        }
                    }
                }
                if self.program.nodes[*node].kind == "BusRouter" && parameter == "Destination" {
                    let target = resolve_path(self.program, *node, &text)?;
                    ensure!(
                        self.program.nodes[target].kind == "AuxEffect",
                        "UVI send must target an auxiliary"
                    );
                    let owner = parent(self.program, *node).context("Invalid send owner")?;
                    let mixer = parent(self.program, target).context("Invalid auxiliary owner")?;
                    let mut at = Some(owner);
                    while at.is_some_and(|id| id != mixer) {
                        at = parent(self.program, at.unwrap());
                    }
                    ensure!(
                        at.is_some(),
                        "Feedback or cross-branch UVI routing is unsupported"
                    );
                    self.routes.insert(*node, target);
                }
                if let Ok(value) = text.parse::<f64>() {
                    if value.is_finite() {
                        self.modulation
                            .update_live_parameter(*node, parameter, value)?;
                    }
                    self.live.insert((*node, parameter.clone()), value);
                    // Nonfinite Text writes retain the public-map error contract.
                    // Only repairing an invalid write needs a setter-time rescan.
                    if !value.is_finite() {
                        self.registered_live_valid = false;
                    } else if !self.registered_live_valid {
                        self.registered_live_valid = self.live.values().all(|v| v.is_finite());
                    }
                }
                self.check_processor_memory()?;
                self.set_parameter_text(*node, parameter.clone(), text);
            }
            host::Action::LoadResource {
                node,
                kind: host::ResourceKind::Sample,
                path,
            } => {
                ensure!(
                    *node < self.parameters.len()
                        && self.program.nodes[*node].kind == "SamplePlayer",
                    "Invalid UVI sample load target"
                );
                ensure!(
                    self.samples.contains_key(path),
                    "UVI resource was not resolved by the caller"
                );
                let group = self
                    .program
                    .sample_zones
                    .iter()
                    .find(|zone| zone.player == *node)
                    .context("Invalid UVI sample player")?
                    .keygroup;
                ensure!(
                    self.samples[path].channels <= self.source_channels[&group],
                    "UVI resource changes bus channel width; rebuild graph required"
                );
                source_layout(self.samples[path].channels)?;
                self.sample_playback(*node, &self.samples[path])?;
                self.set_parameter_text(*node, "SamplePath".into(), path.clone());
                self.set_parameter_text(*node, "SamplePurged".into(), "0".into());
            }
            host::Action::LoadResource {
                node,
                kind: host::ResourceKind::Impulse,
                path,
            } => {
                ensure!(
                    self.program.nodes.get(*node).is_some_and(|node| matches!(
                        node.kind.as_str(),
                        "Convolver" | "SampledReverb"
                    )),
                    "Invalid UVI impulse load target"
                );
                ensure!(
                    self.samples.contains_key(path),
                    "UVI impulse was not resolved by caller"
                );
                let value = ParameterValue::Text(path.clone());
                let mut bytes = self.processor_bytes();
                if let Some(p) = self.processors.get_mut(node) {
                    p.set_value_bounded("SamplePath", &value, &mut bytes)?;
                }
                for v in &mut self.voices {
                    if let Some(p) = v.processors.get_mut(node) {
                        p.set_value_bounded("SamplePath", &value, &mut bytes)?;
                    }
                }
                self.check_processor_memory()?;
                self.set_parameter_text(*node, "SamplePath".into(), path.clone());
            }
            host::Action::ScriptModulation {
                id,
                start,
                target,
                ramp_ms,
                voice,
                layer,
            } => self.modulation.set_script_modulation_scoped(
                *layer,
                *id,
                *start,
                *target,
                *ramp_ms,
                *voice,
                self.frame as f64 / self.rate,
            )?,
        }
        Ok(())
    }
    fn inserts(
        &self,
        node: NodeId,
        frame: &mut Frame,
        processors: &mut HashMap<NodeId, Processor>,
        buses: &mut HashMap<NodeId, Frame>,
        channels: usize,
    ) -> Result<()> {
        if self.boolean(node, "BypassInsertFX")? {
            return Ok(());
        }
        for &id in self.collection(node, "Inserts") {
            if self.program.nodes[id].kind == "EffectRack" {
                if self.boolean(id, "Bypass")? {
                    continue;
                }
                let source = *frame;
                *frame = [0.; dsp::MAX_CHANNELS];
                for &chain in self.collection(id, "Chains") {
                    if self.boolean(chain, "Bypass")? {
                        continue;
                    }
                    let mut branch = source;
                    self.bus(chain, &mut branch, processors, buses, channels)?;
                    add(frame, branch);
                }
            } else {
                processors
                    .get_mut(&id)
                    .context("UVI insert has no executable processor")
                    .and_then(|processor| processor.process(frame))
                    .with_context(|| {
                        format!(
                            "UVI {} insert at node {id}, frame {}",
                            self.program.nodes[id].kind, self.frame
                        )
                    })?;
                if !self.insert_bypassed[id].get() {
                    self.record_processing(id);
                }
            }
        }
        Ok(())
    }
    fn sends(
        &self,
        node: NodeId,
        pre: bool,
        frame: Frame,
        buses: &mut HashMap<NodeId, Frame>,
        channels: usize,
    ) -> Result<()> {
        for &id in self.collection(node, "BusRouters") {
            if self.boolean(id, "Bypass")? || self.boolean(id, "PreFader")? != pre {
                continue;
            }
            let target = *self
                .routes
                .get(&id)
                .context("Missing UVI send destination")?;
            let mut send = if self.program.nodes[node].kind == "Keygroup"
                && parent(self.program, target) != Some(node)
            {
                downmix(frame, channels, 0., 0.)?
            } else {
                frame
            };
            balance(&mut send, self.number(id, "Gain", 1.)?, 0.)?;
            add(buses.entry(target).or_insert([0.; dsp::MAX_CHANNELS]), send);
        }
        Ok(())
    }
    fn bus(
        &self,
        node: NodeId,
        frame: &mut Frame,
        processors: &mut HashMap<NodeId, Processor>,
        buses: &mut HashMap<NodeId, Frame>,
        channels: usize,
    ) -> Result<()> {
        if self.boolean(node, "Bypass")? || self.boolean(node, "Mute")? {
            *frame = [0.; dsp::MAX_CHANNELS];
            return Ok(());
        }
        if self.program.nodes[node].kind != "Keygroup" {
            ensure!(
                [0., 1.].contains(&self.number(node, "PanLaw", 0.)?),
                "Invalid UVI stereo pan law"
            );
        }
        self.sends(node, true, *frame, buses, channels)?;
        let fx_post_gain =
            self.program.nodes[node].kind != "Keygroup" || self.boolean(node, "FXPostGain")?;
        if fx_post_gain {
            balance(
                frame,
                self.number(node, "Gain", 1.)?,
                if self.program.nodes[node].kind == "Keygroup" && channels != 2 {
                    0.
                } else {
                    self.number(node, "Pan", 0.)?
                },
            )?;
        }
        let auxs = self.collection(node, "Auxs");
        for pre in [true, false] {
            if !pre {
                self.inserts(node, frame, processors, buses, channels)?;
            }
            for &id in auxs {
                if self.boolean(id, "PreInsert")? != pre {
                    continue;
                }
                let mut aux = buses.remove(&id).unwrap_or([0.; dsp::MAX_CHANNELS]);
                if !self.boolean(id, "Bypass")? {
                    self.bus(id, &mut aux, processors, buses, channels)?;
                    add(frame, aux);
                }
            }
        }
        if !fx_post_gain {
            balance(
                frame,
                self.number(node, "Gain", 1.)?,
                if self.program.nodes[node].kind == "Keygroup" && channels != 2 {
                    0.
                } else {
                    self.number(node, "Pan", 0.)?
                },
            )?;
        }
        self.sends(node, false, *frame, buses, channels)?;
        Ok(())
    }
    fn oscillator(
        &self,
        osc: &mut Oscillator,
        note: &script::Note,
        bus_channels: usize,
    ) -> Result<Frame> {
        let mut frame = [0.; dsp::MAX_CHANNELS];
        if osc.done
            || self.boolean(osc.player, "Bypass")?
            || self.boolean(osc.player, "SamplePurged")?
        {
            return Ok(frame);
        }
        if let Some(generator) = &mut osc.generator {
            let player = osc.player;
            let pitch = (f64::from(note.note) - self.number(player, "BaseNote", 60.)?)
                * self.number(player, "NoteTracking", 1.)?
                + self.number(player, "CoarseTune", 0.)?
                + self.number(player, "FineTune", 0.)? / 100.
                + self.number(player, "Pitch", 0.)?
                + note.tune;
            ensure!(
                pitch.abs() <= 240.,
                "UVI generator pitch outside bounded range"
            );
            let mut frame = generator.next(
                |name, default| self.number(player, name, default),
                261.6255653005986 * 2f64.powf(pitch / 12.),
            )?;
            if generator.channels() == 1 && bus_channels > 1 {
                frame[0] *= 0.5;
                frame[1] = frame[0];
            }
            balance(
                &mut frame,
                self.number(player, "Gain", 1.)?,
                self.number(player, "Pan", 0.)?,
            )?;
            self.record_processing(osc.player);
            return Ok(frame);
        }
        let sample = &self.samples[&osc.path];
        if osc.position < osc.start as f64 || osc.position >= osc.end as f64 {
            osc.done = true;
            return Ok(frame);
        }
        let lo = osc.position.floor() as usize;
        let mut hi = lo + 1;
        let loop_data = osc
            .loop_data
            .as_ref()
            .filter(|l| l.play_count == 0 || osc.loops_completed < l.play_count);
        if let Some(l) = loop_data {
            if lo == l.end as usize && l.kind == 0 {
                hi = l.start as usize;
            }
        }
        let fraction = osc.position.fract() as f32;
        let mode = self.number(osc.player, "InterpolationMode", 1.)?;
        ensure!(mode.fract() == 0., "Noninteger UVI interpolation mode");
        let mode = mode.clamp(0., 2.) as u8;
        let neighbor = |index: isize, channel| -> Result<f32> {
            let mut index = index;
            if let Some(marker) = loop_data.filter(|l| l.kind == 0) {
                let start = marker.start as isize;
                let end = marker.end as isize;
                if lo >= marker.start as usize
                    && lo <= marker.end as usize
                    && (index > end || (index < start && osc.loops_completed > 0))
                {
                    index = start + (index - start).rem_euclid(end - start + 1);
                }
            }
            if index < 0 || index as usize >= sample.frames {
                return Ok(0.);
            }
            sample
                .interleaved
                .value(index as usize * sample.channels + channel)
                .context("Missing UVI PCM frame")
        };
        for (ch, out) in frame[..sample.channels].iter_mut().enumerate() {
            let a = neighbor(lo as isize, ch)?;
            let b = neighbor(hi as isize, ch)?;
            *out = match mode {
                0 => {
                    if fraction >= 0.5 {
                        b
                    } else {
                        a
                    }
                }
                1 => a + (b - a) * fraction,
                _ => {
                    let prior = neighbor(lo as isize - 1, ch)?;
                    let following = neighbor(lo as isize + 2, ch)?;
                    0.5 * (2. * a
                        + fraction
                            * ((b - prior)
                                + fraction
                                    * (2. * prior - 5. * a + 4. * b - following
                                        + fraction * (-prior + 3. * a - 3. * b + following))))
                }
            };
        }

        if sample.channels == 1 && bus_channels > 1 {
            frame[0] *= 0.5;
            frame[1] = frame[0];
        }
        let pitch = (f64::from(note.note) - self.number(osc.player, "BaseNote", 60.)?)
            * self.number(osc.player, "NoteTracking", 1.)?
            + self.number(osc.player, "CoarseTune", 0.)?
            + self.number(osc.player, "FineTune", 0.)? / 100.
            + self.number(osc.player, "Pitch", 0.)?
            + note.tune;
        ensure!(
            pitch.abs() <= 240.,
            "UVI playback pitch outside bounded range"
        );
        let speed = f64::from(sample.rate) / self.rate * 2f64.powf(pitch / 12.);
        osc.position += speed * osc.direction;
        if let Some(l) = loop_data {
            let start = f64::from(l.start);
            let end = f64::from(l.end) + 1.;
            let length = end - start;
            if l.kind == 1 {
                let span = length - 1.;
                if span == 0. {
                    osc.position = start;
                    osc.loops_completed = osc.loops_completed.saturating_add(1);
                } else if (osc.direction > 0. && osc.position >= end - 1.)
                    || (osc.direction < 0. && osc.position <= start)
                {
                    let old = osc.position - speed * osc.direction;
                    let phase = if osc.direction > 0. {
                        osc.position - start
                    } else {
                        2. * span - (osc.position - start)
                    };
                    let old_phase = if osc.direction > 0. {
                        old - start
                    } else {
                        2. * span - (old - start)
                    };
                    let crossings =
                        ((phase / span).floor() - (old_phase / span).floor()).max(1.) as u32;
                    osc.loops_completed = osc.loops_completed.saturating_add(crossings);
                    let phase = phase.rem_euclid(2. * span);
                    if phase < span {
                        osc.position = start + phase;
                        osc.direction = 1.;
                    } else {
                        osc.position = start + 2. * span - phase;
                        osc.direction = -1.;
                    }
                }
            } else if osc.direction > 0. && osc.position >= end {
                let crossings = ((osc.position - start) / length).floor().max(1.) as u32;
                osc.loops_completed = osc.loops_completed.saturating_add(crossings);
                if l.kind == 2 {
                    osc.direction = -1.;
                    osc.position =
                        start + ((end - 1. - (osc.position - end)) - start).rem_euclid(length);
                } else {
                    osc.position = start + (osc.position - start).rem_euclid(length);
                }
            } else if osc.direction < 0. && osc.position < start {
                let crossings = ((start - osc.position) / length).ceil().max(1.) as u32;
                osc.loops_completed = osc.loops_completed.saturating_add(crossings);
                osc.position = start + (osc.position - start).rem_euclid(length);
            }
        }
        let target = self.number(osc.player, "Gain", 1.)?;
        let gain = if self.dynamic_source(osc.player, "Gain") {
            target
        } else {
            f64::from(
                osc.gain
                    .as_mut()
                    .context("Uninitialized UVI gain clock")?
                    .value(self.frame, target as f32, self.rate),
            )
        };
        balance(&mut frame, gain, self.number(osc.player, "Pan", 0.)?)?;
        self.record_processing(osc.player);
        Ok(frame)
    }
    fn next_frame(&mut self) -> Result<[f32; 2]> {
        let mut voices = std::mem::take(&mut self.voices);
        let mut killed = HashSet::new();
        voices.retain(|voice| {
            let done = voice.fade.is_some_and(|fade| {
                fade.kill && self.frame.saturating_sub(fade.begin) >= fade.duration
            });
            if done {
                killed.insert(voice.note.id);
                self.modulation
                    .remove_instance(voice.note.id, voice.instance);
            }
            !done
        });
        for id in killed {
            if !voices.iter().any(|voice| voice.note.id == id) {
                self.modulation.remove_voice(id);
                self.snapshots.remove(&id);
            }
        }
        let mut processors = std::mem::take(&mut self.processors);
        let mut buses = std::mem::take(&mut self.buses);
        let mut layers = HashMap::<NodeId, Frame>::new();
        let mut released = HashSet::new();
        for voice in &mut voices {
            if voice.note_off.is_some()
                && self.release_finished(
                    &self.inputs(Some(voice)),
                    &self.scope_nodes[&Some(voice.keygroup)],
                )?
            {
                released.insert(voice.instance);
                continue;
            }
            self.evaluate_scope(self.inputs(Some(voice)), Some(voice.keygroup))?;
            self.update_processors(&mut voice.processors)?;
            let mut frame = [0.; dsp::MAX_CHANNELS];
            for osc in &mut voice.oscillators {
                add(
                    &mut frame,
                    self.oscillator(osc, &voice.note, voice.channels)
                        .with_context(|| {
                            format!(
                                "UVI {} oscillator at node {}, frame {}, voice {}, instance {}",
                                self.program.nodes[osc.player].kind,
                                osc.player,
                                self.frame,
                                voice.note.id,
                                voice.instance
                            )
                        })?,
                );
            }
            let mut fade = 1f64;
            for (low, high, lofade, hifade, key) in [
                (
                    "LowKey",
                    "HighKey",
                    "LowKeyFade",
                    "HighKeyFade",
                    voice.note.note,
                ),
                (
                    "LowVelocity",
                    "HighVelocity",
                    "LowVelocityFade",
                    "HighVelocityFade",
                    voice.note.velocity,
                ),
            ] {
                let lo = self.number(voice.keygroup, lofade, 0.)?;
                let hi = self.number(voice.keygroup, hifade, 0.)?;
                if lo > 0. {
                    fade *= ((f64::from(key) - self.number(voice.keygroup, low, 0.)?) / lo)
                        .clamp(0., 1.);
                }
                if hi > 0. {
                    fade *= ((self.number(voice.keygroup, high, 127.)? - f64::from(key)) / hi)
                        .clamp(0., 1.);
                }
            }
            if self.number(voice.keygroup, "FadeCurve", 2.)? == 2. {
                fade = (fade * std::f64::consts::FRAC_PI_2).sin();
            }
            balance(
                &mut frame,
                f64::from(voice.note.volume)
                    * fade
                    * f64::from(voice.fade.map_or(1., |fade| fade.value(self.frame))),
                if voice.channels == 1 {
                    0.
                } else {
                    f64::from(voice.note.pan)
                },
            )?;
            if self.boolean(voice.layer, "Mute")? {
                frame = [0.; dsp::MAX_CHANNELS];
            }
            ensure!(
                [1, 2, 6, 10, 12].contains(&voice.channels)
                    || self.number(voice.keygroup, "Pan", 0.)? == 0.,
                "Nonzero pan for this multichannel layout is unverified"
            );
            if !self.dynamic_source(voice.keygroup, "Gain") {
                let target = self.number(voice.keygroup, "Gain", 1.)? as f32;
                let value = f64::from(voice.gain.value(self.frame, target, self.rate));
                self.cache_effective_value(voice.keygroup, "Gain", value);
                // Bus gain reads its persistent slot, so the clock needs no owned key.
            }
            self.bus(
                voice.keygroup,
                &mut frame,
                &mut voice.processors,
                &mut buses,
                voice.channels,
            )?;
            frame = downmix(
                frame,
                voice.channels,
                if voice.channels == 1 {
                    self.number(voice.keygroup, "Pan", 0.)?
                } else {
                    0.
                },
                self.number(voice.keygroup, "PanLaw", 0.)?,
            )?;
            if voice.channels == 1 {
                balance(&mut frame, 1., f64::from(voice.note.pan))?;
            }
            add(
                layers.entry(voice.layer).or_insert([0.; dsp::MAX_CHANNELS]),
                frame,
            );
        }
        let mut retired = HashSet::new();
        voices.retain(|voice| {
            if released.contains(&voice.instance) {
                retired.insert(voice.note.id);
                self.modulation
                    .remove_instance(voice.note.id, voice.instance);
                false
            } else {
                true
            }
        });
        for id in retired {
            if !voices.iter().any(|voice| voice.note.id == id) {
                self.modulation.remove_voice(id);
                self.snapshots.remove(&id);
            }
        }
        // Keep held voices after source exhaustion so per-note filter/delay state can drain.
        self.evaluate_scope(self.inputs(None), None)?;
        self.update_processors(&mut processors)?;
        let mut output = [0.; dsp::MAX_CHANNELS];
        for &layer in &self.program.layers {
            let mut frame = layers.remove(&layer).unwrap_or([0.; dsp::MAX_CHANNELS]);
            self.bus(layer, &mut frame, &mut processors, &mut buses, 2)?;
            add(&mut output, frame);
        }
        self.bus(
            self.program.root,
            &mut output,
            &mut processors,
            &mut buses,
            2,
        )?;
        ensure!(
            buses.is_empty(),
            "UVI routed auxiliary was not consumed by its graph owner"
        );
        ensure!(
            output.iter().all(|s| s.is_finite()),
            "UVI graph produced nonfinite audio"
        );
        self.voices = voices;
        self.processors = processors;
        self.buses = buses;
        self.frame += 1;
        Ok([output[0], output[1]])
    }
    /// Retained DSP voice instances, including held/releasing silent voices.
    /// This is not an audibility estimate or a count of shared effect tails.
    pub fn active_voices(&self) -> usize {
        self.voices.len()
    }
    /// Active per-launch instances, including sustain and owned DSP lifetime.
    /// Shared Layer/Program/Aux processors do not belong to one backend token.
    pub fn sounding_roots(&self) -> Vec<script::HostRoot> {
        let mut roots = self
            .voices
            .iter()
            .filter_map(|voice| voice.root)
            .collect::<Vec<_>>();
        roots.sort_unstable_by_key(|root| (root.epoch, root.generation, root.token));
        roots.dedup();
        roots
    }
    /// Apply owning-worker commands at the current boundary without advancing PCM.
    pub(crate) fn apply_boundary(
        &mut self,
        notes: &[script::Command],
        roots: Option<&[Option<script::HostRoot>]>,
        host: &[host::Command],
    ) -> Result<()> {
        ensure!(
            notes.len() <= LIMIT && host.len() <= LIMIT,
            "UVI boundary command limit exceeded"
        );
        ensure!(
            notes.iter().all(|c| c.frame == self.frame)
                && host.iter().all(|c| c.frame == self.frame),
            "UVI commands are outside the saved boundary"
        );
        ensure!(
            roots.is_none_or(|r| r.len() == notes.len()),
            "Hosted boundary ancestry length mismatch"
        );
        for command in host {
            self.apply_host(&command.action)?;
        }
        for (index, command) in notes.iter().enumerate() {
            self.apply_note_rooted(&command.action, roots.and_then(|r| r[index]))?;
        }
        self.check_processor_memory()
    }

    /// Events are at absolute output frame positions. Host parameter commands
    /// at a frame precede note commands, so initialization affects the attack.
    pub fn render(
        &mut self,
        notes: &[script::Command],
        host: &[host::Command],
        frames: usize,
    ) -> Result<Vec<[f32; 2]>> {
        self.render_inner(notes, None, host, frames)
    }
    /// Activation-local ancestry annotates terminal launches; it never changes
    /// opaque Lua ID/key/layer release matching or core note identity.
    pub fn render_with_roots(
        &mut self,
        notes: &[script::Command],
        roots: &[Option<script::HostRoot>],
        host: &[host::Command],
        frames: usize,
    ) -> Result<Vec<[f32; 2]>> {
        ensure!(
            roots.len() == notes.len(),
            "Hosted command ancestry length mismatch"
        );
        ensure!(
            roots
                .iter()
                .flatten()
                .all(|r| r.epoch > 0 && r.generation > 0 && r.token > 0),
            "Invalid hosted ancestry"
        );
        ensure!(
            notes.iter().zip(roots).all(|(command, root)|
                !matches!(command.action, script::Action::ChokeRoot) || root.is_some()),
            "Hosted choke has no ancestry"
        );
        self.render_inner(notes, Some(roots), host, frames)
    }
    fn render_inner(
        &mut self,
        notes: &[script::Command],
        roots: Option<&[Option<script::HostRoot>]>,
        host: &[host::Command],
        frames: usize,
    ) -> Result<Vec<[f32; 2]>> {
        ensure!(
            roots.is_some() || notes.iter().all(|command|
                !matches!(command.action, script::Action::ChokeRoot)),
            "Hosted choke has no ancestry"
        );
        self.check_processor_memory()?;
        ensure!(
            frames <= self.rate as usize * 60,
            "UVI render exceeds 60 seconds"
        );
        ensure!(
            notes.len() <= LIMIT && host.len() <= LIMIT,
            "UVI command limit exceeded"
        );
        ensure!(
            notes.windows(2).all(|w| w[0].frame <= w[1].frame)
                && host.windows(2).all(|w| w[0].frame <= w[1].frame),
            "UVI commands must be time ordered"
        );
        ensure!(
            notes.first().is_none_or(|c| c.frame >= self.frame)
                && host.first().is_none_or(|c| c.frame >= self.frame),
            "UVI command precedes current render position"
        );
        let end = self
            .frame
            .checked_add(frames as u64)
            .context("UVI render position overflow")?;
        ensure!(
            notes.last().is_none_or(|c| c.frame < end) && host.last().is_none_or(|c| c.frame < end),
            "UVI command is outside the requested render interval; retain it for a later call"
        );
        ensure!(
            !self.requires_planned_segments()
                || (self.frame.is_multiple_of(256) && frames.is_multiple_of(256)),
            "This UVI control source requires 256-frame aligned render partitions with all upcoming musical events queued"
        );
        ensure!(
            !self.requires_planned_segments()
                || host.iter().all(|command| {
                    command.frame.is_multiple_of(256)
                        || notes
                            .binary_search_by_key(&command.frame, |note| note.frame)
                            .is_ok()
                }),
            "Unverified host-only control mutation inside a planned UVI processing segment"
        );
        let (mut ni, mut hi) = (0, 0);
        let mut output = Vec::with_capacity(frames);
        for _ in 0..frames {
            let boundary = (self.frame / 256 + 1)
                .checked_mul(256)
                .context("UVI control block position overflow")?;
            let segment_end = notes[ni..]
                .iter()
                .find(|command| command.frame > self.frame)
                .map_or(boundary, |command| boundary.min(command.frame));
            self.modulation
                .set_control_segment_end_frame(Some(segment_end));
            while host.get(hi).is_some_and(|c| c.frame == self.frame) {
                self.apply_host(&host[hi].action)?;
                hi += 1;
            }
            let updates = self
                .modulation
                .control_updates(&self.inputs(None), &self.live)?;
            ensure!(
                updates.len() <= LIMIT,
                "UVI absolute control update limit exceeded"
            );
            for ((node, parameter), value) in updates {
                let value = if parameter == "Bypass" {
                    ParameterValue::Boolean(value >= 0.5)
                } else {
                    ParameterValue::Number(value)
                };
                self.apply_host(&host::Action::Parameter {
                    node,
                    parameter,
                    value,
                })?;
            }
            while notes.get(ni).is_some_and(|c| c.frame == self.frame) {
                self.apply_note_rooted(&notes[ni].action, roots.and_then(|roots| roots[ni]))?;
                ni += 1;
            }
            output.push(self.next_frame()?);
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uvi::{program::parse_program, sample::SampleLoop, storage::Storage};
    fn sample(channels: usize) -> Sample {
        Sample {
            rate: 48000,
            channels,
            frames: 8,
            interleaved: Storage::from_f32(
                (0..8)
                    .flat_map(|f| (0..channels).map(move |c| f as f32 + c as f32 + 1.))
                    .collect(),
            )
            .unwrap(),
            loops: vec![SampleLoop {
                id: 0,
                kind: 0,
                start: 2,
                end: 4,
                fraction: 0,
                play_count: 0,
            }],
            unity_note: None,
            wavetable_cycle_frames: None,
            wavetable_image: false,
            riff_metadata: vec![b"riff".to_vec()],
        }
    }
    fn note(id: u32) -> script::Note {
        script::Note {
            id,
            note: 60,
            velocity: 100,
            channel: 0,
            dim1: 0,
            dim2: None,
            layers: None,
            oscillator: None,
            volume: 1.,
            pan: 0.,
            tune: 0.,
            offset_us: 0,
        }
    }
    #[test]
    fn runtime_evidence_counts_executed_leaves_and_preserves_passive_snapshots() {
        let program = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer Name="Active" SamplePath="a" NoteTracking="0"/><SamplePlayer Name="Bypassed" SamplePath="a" Bypass="1"/></Oscillators><Inserts><Gain Name="ActiveInsert"/><Gain Name="BypassedInsert" Bypass="1"/></Inserts></Keygroup><Keygroup LowKey="100"><Oscillators><SamplePlayer Name="Unvisited" SamplePath="a"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let id = |name: &str| program.nodes.iter().position(|n| n.name.as_deref() == Some(name)).unwrap();
        let mut renderer = Renderer::new(&program, HashMap::from([("a".into(), Arc::new(sample(1)))]), 48000).unwrap();
        assert!(renderer.runtime_evidence().iter().all(|row| row.processed_blocks == 0));
        renderer.render(&[script::Command { frame: 0, action: script::Action::Start(note(7)) }], &[], 8).unwrap();
        let evidence = renderer.runtime_evidence();
        let row = |name| evidence.iter().find(|row| row.node == id(name)).unwrap();
        assert_eq!(row("Active").processed_blocks, 1);
        assert_eq!(row("ActiveInsert").processed_blocks, 1);
        assert_eq!(row("Active").retained_voice_instances, 1);
        assert_eq!(row("Bypassed").processed_blocks, 0);
        assert_eq!(row("BypassedInsert").processed_blocks, 0);
        assert_eq!(row("BypassedInsert").currently_bypassed, Some(true));
        assert_eq!(row("Unvisited").processed_blocks, 0);
        assert_eq!(row("Unvisited").retained_voice_instances, 0);
        assert_eq!(renderer.current_frame(), 8);
        let before = serde_json::to_value(&evidence).unwrap();
        assert_eq!(serde_json::to_value(renderer.runtime_evidence()).unwrap(), before);
        assert_eq!(renderer.current_frame(), 8);
        renderer.render(&[], &[], 248).unwrap();
        assert_eq!(renderer.runtime_evidence().iter().find(|row| row.node == id("Active")).unwrap().processed_blocks, 1);
        renderer.render(&[], &[], 1).unwrap();
        assert_eq!(renderer.runtime_evidence().iter().find(|row| row.node == id("Active")).unwrap().processed_blocks, 2);
    }
    #[test]
    fn runtime_evidence_counts_mid_interval_start_and_enable_only_after_execution() {
        let program = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer Name="Source" SamplePath="a" NoteTracking="0"/></Oscillators><Inserts><Gain Name="Insert" Bypass="1"/></Inserts></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let id = |name: &str| program.nodes.iter().position(|n| n.name.as_deref() == Some(name)).unwrap();
        let mut renderer = Renderer::new(&program, HashMap::from([("a".into(), Arc::new(sample(1)))]), 48000).unwrap();
        renderer.render(&[script::Command { frame: 17, action: script::Action::Start(note(7)) }], &[], 32).unwrap();
        let evidence = renderer.runtime_evidence();
        assert_eq!(evidence.iter().find(|row| row.node == id("Source")).unwrap().processed_blocks, 1);
        assert_eq!(evidence.iter().find(|row| row.node == id("Insert")).unwrap().processed_blocks, 0);
        renderer.render(&[], &[host::Command { frame: 32, action: host::Action::Parameter { node: id("Insert"), parameter: "Bypass".into(), value: ParameterValue::Number(0.) } }], 1).unwrap();
        assert_eq!(renderer.runtime_evidence().iter().find(|row| row.node == id("Insert")).unwrap().processed_blocks, 1);
    }
    #[test]
    fn runtime_evidence_uses_each_voice_effective_bypass_gate() {
        let program = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a" NoteTracking="0"/></Oscillators><Inserts><XpanderFilter Name="Insert" Algorithm="1" Oversampling="0"><Connections><SignalConnection Source="@MIDI CC 1" Destination="Bypass" Ratio="1"/></Connections></XpanderFilter></Inserts></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let insert = program.nodes.iter().position(|n| n.name.as_deref() == Some("Insert")).unwrap();
        for enabled_first in [false, true] {
            let mut renderer = Renderer::new(&program, HashMap::from([("a".into(), Arc::new(sample(1)))]), 48000).unwrap();
            let mut first = note(7);
            first.channel = u8::from(enabled_first);
            let mut second = note(8);
            second.channel = u8::from(!enabled_first);
            let bypass = script::Command { frame: 0, action: script::Action::Controller { channel: 0, controller: 1, value: 127 } };
            renderer.render(&[bypass, script::Command { frame: 0, action: script::Action::Start(first) }], &[], 256).unwrap();
            let count = |renderer: &Renderer| renderer.runtime_evidence().iter().find(|row| row.node == insert).unwrap().processed_blocks;
            assert_eq!(count(&renderer), u64::from(enabled_first));
            renderer.render(&[script::Command { frame: 256, action: script::Action::Start(second) }], &[], 256).unwrap();
            assert_eq!(count(&renderer), u64::from(enabled_first) + 1);
            assert_eq!(renderer.runtime_evidence().iter().find(|row| row.node == insert).unwrap().retained_voice_instances, 2);
            assert_eq!(renderer.voices[0].processors[&insert].bypassed().unwrap(), !enabled_first);
            assert_eq!(renderer.voices[1].processors[&insert].bypassed().unwrap(), enabled_first);
            renderer.render(&[], &[], 256).unwrap();
            assert_eq!(count(&renderer), u64::from(enabled_first) + 2);
        }
    }
    #[test]
    fn first_note_processor_constructor_error_identifies_node_and_audio_configuration() {
        let program = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a" NoteTracking="0"/></Oscillators><Inserts><Exciter Amount="2" Mode="1" Oversampling="0"/></Inserts></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let exciter = program.nodes.iter().position(|n| n.kind == "Exciter").unwrap();
        let mut renderer = Renderer::new(
            &program,
            HashMap::from([("a".into(), Arc::new(sample(1)))]),
            48000,
        ).unwrap();
        let error = renderer.render(
            &[script::Command { frame: 17, action: script::Action::Start(note(7)) }],
            &[],
            18,
        ).unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains(&format!("Exciter constructor at node {exciter}")), "{message}");
        assert!(message.contains("channels 1, rate 48000, frame 17"), "{message}");
        assert!(message.contains("Exciter requires a measured 48-kHz stereo bus"), "{message}");
    }
    #[test]
    fn registered_numeric_text_fallback_waits_for_every_finite_repair() {
        let program = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a" Gain="1" Pitch="0"><Connections><SignalConnection Source="@MIDI CC 1" Destination="Gain" Ratio=".5"/><SignalConnection Source="@MIDI CC 2" Destination="Pitch" Ratio="1"/></Connections></SamplePlayer></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut renderer = Renderer::new(
            &program,
            HashMap::from([("a".into(), Arc::new(sample(1)))]),
            48000,
        )
        .unwrap();
        let player = program.sample_zones[0].player;
        let change = |name: &str, value| host::Action::Parameter {
            node: player,
            parameter: name.into(),
            value,
        };
        assert!(renderer.registered_live_valid);
        renderer
            .apply_host(&change("Gain", ParameterValue::Number(0.5)))
            .unwrap();
        for (name, text) in [("Gain", "NaN"), ("Pitch", "inf")] {
            renderer
                .apply_host(&change(name, ParameterValue::Text(text.into())))
                .unwrap();
            assert!(!renderer.registered_live_valid);
            assert!(
                renderer
                    .evaluate_scope(renderer.inputs(None), None)
                    .is_err()
            );
        }
        renderer
            .apply_host(&change("Gain", ParameterValue::Text("not-a-number".into())))
            .unwrap();
        assert!(renderer.live[&(player, "Gain".into())].is_nan());
        assert!(!renderer.registered_live_valid);
        renderer
            .apply_host(&change("UnrelatedNumeric", ParameterValue::Number(7.)))
            .unwrap();
        assert!(!renderer.registered_live_valid);
        assert!(
            renderer
                .evaluate_scope(renderer.inputs(None), None)
                .is_err()
        );
        renderer
            .apply_host(&change("Gain", ParameterValue::Number(1.25)))
            .unwrap();
        assert!(!renderer.registered_live_valid);
        assert!(renderer.live[&(player, "Pitch".into())].is_infinite());
        assert!(
            renderer
                .evaluate_scope(renderer.inputs(None), None)
                .is_err()
        );
        assert!(
            renderer
                .apply_host(&change("Pitch", ParameterValue::Number(f64::NAN)))
                .is_err()
        );
        assert!(!renderer.registered_live_valid);
        assert!(renderer.live[&(player, "Pitch".into())].is_infinite());
        renderer
            .apply_host(&change("Pitch", ParameterValue::Text("0.75".into())))
            .unwrap();
        assert!(renderer.registered_live_valid);
        let input = renderer.inputs(None);
        let nodes = HashSet::from([player]);
        let expected = renderer
            .modulation
            .evaluate_nodes(&input, &renderer.live, &nodes)
            .unwrap();
        let mut actual = HashMap::new();
        renderer
            .modulation
            .evaluate_registered_nodes_into(&input, &nodes, |p, v| {
                actual.insert(p.clone(), v.to_bits());
            })
            .unwrap();
        assert_eq!(
            actual,
            expected
                .into_iter()
                .map(|(p, v)| (p, v.to_bits()))
                .collect()
        );
        renderer
            .evaluate_scope(renderer.inputs(None), None)
            .unwrap();
        assert_eq!(
            renderer.base_number(player, "Gain", 0.).unwrap().to_bits(),
            1.25f64.to_bits()
        );
        assert_eq!(
            renderer.base_number(player, "Pitch", 0.).unwrap().to_bits(),
            0.75f64.to_bits()
        );
    }
    #[test]
    fn numeric_cache_tracks_mutations_scope_defaults_and_invalid_text() {
        let program = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a" Gain="0.25"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut renderer = Renderer::new(
            &program,
            HashMap::from([("a".into(), Arc::new(sample(1)))]),
            48000,
        )
        .unwrap();
        let player = program.sample_zones[0].player;
        let change = |name: &str, value| host::Action::Parameter {
            node: player,
            parameter: name.into(),
            value,
        };
        assert_eq!(renderer.number(player, "Gain", 1.).unwrap(), 0.25);
        renderer
            .apply_host(&change("Gain", ParameterValue::Number(0.5)))
            .unwrap();
        assert_eq!(renderer.base_number(player, "Gain", 1.).unwrap(), 0.5);
        renderer.cache_effective_value(player, "Gain", 0.125);
        assert_eq!(renderer.number(player, "Gain", 1.).unwrap(), 0.125);
        assert_eq!(renderer.base_number(player, "Gain", 1.).unwrap(), 0.5);
        renderer
            .apply_host(&change("Gain", ParameterValue::Number(0.75)))
            .unwrap();
        // A live base mutation does not overwrite this frame's evaluated scope.
        assert_eq!(renderer.number(player, "Gain", 1.).unwrap(), 0.125);
        renderer
            .evaluate_scope(renderer.inputs(None), None)
            .unwrap();
        assert_eq!(renderer.number(player, "Gain", 1.).unwrap(), 0.75);
        assert_eq!(renderer.number(player, "Absent", 3.).unwrap(), 3.);
        renderer
            .apply_host(&change("SamplePurged", ParameterValue::Boolean(true)))
            .unwrap();
        assert!(renderer.boolean(player, "SamplePurged").unwrap());
        renderer
            .apply_host(&host::Action::LoadResource {
                node: player,
                kind: host::ResourceKind::Sample,
                path: "a".into(),
            })
            .unwrap();
        assert!(!renderer.boolean(player, "SamplePurged").unwrap());
        for invalid in ["not-a-number", "NaN", "inf"] {
            renderer
                .apply_host(&change("Gain", ParameterValue::Text(invalid.into())))
                .unwrap();
            assert_eq!(
                renderer.number(player, "Gain", 1.).unwrap_err().to_string(),
                numeric(&renderer.parameters, player, "Gain", 1.)
                    .unwrap_err()
                    .to_string()
            );
        }
        assert!(renderer.number(player, "Absent", f64::NAN).is_err());
        assert!(
            renderer
                .apply_host(&change("Gain", ParameterValue::Number(f64::NAN)))
                .is_err()
        );
    }
    #[test]
    fn oscillators_sum_before_matrix_and_release_keeps_other_note_owned_voice() {
        let xml = r#"<Program><Layers><Layer Name="A"><Keygroups><Keygroup Name="K"><Oscillators>
          <SamplePlayer Name="One" SamplePath="a"/><SamplePlayer Name="Two" SamplePath="b"/>
          </Oscillators><Inserts><GainMatrix Name="Mic" Gain_1_1="0" Gain_2_2="0" Gain_3_3="0" Gain_4_4="0" Gain_5_5="0" Gain_6_6="0" Gain_6_1="0.5" Gain_3_2="1"/></Inserts>
          </Keygroup></Keygroups></Layer></Layers></Program>"#;
        let p = parse_program(xml).unwrap();
        let mut renderer = Renderer::new(
            &p,
            HashMap::from([
                ("a".into(), Arc::new(sample(6))),
                ("b".into(), Arc::new(sample(6))),
            ]),
            48000,
        )
        .unwrap();
        let commands = vec![
            script::Command {
                frame: 0,
                action: script::Action::Start(note(1)),
            },
            script::Command {
                frame: 0,
                action: script::Action::Start(note(2)),
            },
            script::Command {
                frame: 3,
                action: script::Action::Release(1),
            },
            script::Command {
                frame: 6,
                action: script::Action::Release(2),
            },
        ];
        let player = p.sample_zones[0].player;
        let changes = vec![host::Command {
            frame: 0,
            action: host::Action::Parameter {
                node: player,
                parameter: "Gain".into(),
                value: ParameterValue::Number(0.),
            },
        }];
        let rendered = renderer.render(&commands, &changes, 8).unwrap();
        for (index, expected) in [
            (0, [6., 6.]),
            (2, [8., 10.]),
            (3, [4.5, 6.]),
            (4, [5., 7.]),
            (5, [4., 5.]),
        ] {
            for (actual, expected) in rendered[index].iter().zip(expected) {
                assert!((actual - expected / 3f32.sqrt()).abs() < 2e-6);
            }
        }
        assert_eq!(rendered[6], [0., 0.]);
        assert_eq!(rendered[7], [0., 0.]);
        let unsupported =
            parse_program(&xml.replace("<Inserts>", "<Inserts><UnknownFX Bypass=\"1\"/>")).unwrap();
        assert!(!preflight(&unsupported).is_empty());
        assert!(Renderer::new(&unsupported, HashMap::new(), 48000).is_err());
    }
    #[test]
    fn auxiliary_relative_path_and_prefader_routing_follow_graph() {
        let p=parse_program(r#"<Program><Auxs><AuxEffect Name="Aux0"><Inserts><Gain Volume="0.5"/></Inserts></AuxEffect></Auxs>
          <Layers><Layer Name="L" Gain="0"><BusRouters><BusRouter Name="Send" Destination="../../Aux0" PreFader="1" Gain="1"/></BusRouters>
          <Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut renderer = Renderer::new(
            &p,
            HashMap::from([("a".into(), Arc::new(sample(2)))]),
            48000,
        )
        .unwrap();
        let rendered = renderer
            .render(
                &[script::Command {
                    frame: 0,
                    action: script::Action::Start(note(1)),
                }],
                &[],
                1,
            )
            .unwrap();
        assert_eq!(rendered[0], [0.5, 1.]);
    }
    #[test]
    fn per_voice_modulation_ramp_and_live_parameter_share_native_target() {
        let p=parse_program(r#"<Program><ControlSignalSources><ScriptEventModulation Name="Ramp" EventId="1" Bipolar="0"/></ControlSignalSources><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a" Gain="1"><Connections><SignalConnection Source="$Program/Ramp" Destination="Gain" Ratio="1"/></Connections></SamplePlayer></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut source = sample(1);
        source.interleaved = Storage::from_f32(vec![1.; source.interleaved.len()]).unwrap();
        let mut renderer =
            Renderer::new(&p, HashMap::from([("a".into(), Arc::new(source))]), 48000).unwrap();
        let notes = vec![
            script::Command {
                frame: 0,
                action: script::Action::Start(note(1)),
            },
            script::Command {
                frame: 0,
                action: script::Action::Start(note(2)),
            },
            script::Command {
                frame: 4,
                action: script::Action::Release(1),
            },
        ];
        let controls = vec![
            host::Command {
                frame: 0,
                action: host::Action::ScriptModulation {
                    id: 1,
                    start: Some(0.),
                    target: 1.,
                    ramp_ms: 4. * 1000. / 48000.,
                    voice: Some(1),
                    layer: None,
                },
            },
            host::Command {
                frame: 3,
                action: host::Action::Parameter {
                    node: p.sample_zones[0].player,
                    parameter: "Gain".into(),
                    value: ParameterValue::Number(0.5),
                },
            },
        ];
        let output = renderer.render(&notes, &controls, 5).unwrap();
        assert_eq!(output[0], [0., 0.]);
        assert!(output[2][0] > 0. && output[2][0] < 0.25);
        assert!(output[3][0] > 0. && output[3][0] < 0.1875);
        assert_eq!(output[4], [0., 0.]);
    }
    #[test]
    fn reverse_pingpong_and_source_rate_keep_inclusive_loop_coordinates() {
        let p=parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a" Reverse="1"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut reverse = Renderer::new(
            &p,
            HashMap::from([("a".into(), Arc::new(sample(1)))]),
            48000,
        )
        .unwrap();
        let command = script::Command {
            frame: 0,
            action: script::Action::Start(note(1)),
        };
        let result = reverse
            .render(std::slice::from_ref(&command), &[], 8)
            .unwrap();
        assert_eq!(
            result.iter().map(|f| f[0]).collect::<Vec<_>>(),
            vec![4., 3.5, 3., 2.5, 2., 1.5, 2.5, 2.]
        );
        let p=parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut source = sample(1);
        source.loops[0].kind = 1;
        let mut pingpong =
            Renderer::new(&p, HashMap::from([("a".into(), Arc::new(source))]), 48000).unwrap();
        let result = pingpong
            .render(std::slice::from_ref(&command), &[], 8)
            .unwrap();
        assert_eq!(
            result.iter().map(|f| f[0]).collect::<Vec<_>>(),
            vec![0.5, 1., 1.5, 2., 2.5, 2., 1.5, 2.]
        );
        let mut source = sample(1);
        source.rate = 24000;
        source.loops.clear();
        let mut slow =
            Renderer::new(&p, HashMap::from([("a".into(), Arc::new(source))]), 48000).unwrap();
        let result = slow.render(std::slice::from_ref(&command), &[], 4).unwrap();
        assert_eq!(
            result.iter().map(|f| f[0]).collect::<Vec<_>>(),
            vec![0.5, 0.75, 1., 1.25]
        );
    }
    #[test]
    fn note_routes_keep_layer_identity_and_bypassed_oscillator_index() {
        let p=parse_program(r#"<Program><Layers><Layer Name="A"><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a" Bypass="1"/><SamplePlayer SamplePath="b"/></Oscillators></Keygroup></Keygroups></Layer><Layer Name="B"><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut a = sample(1);
        a.interleaved = Storage::from_f32(vec![1.; a.interleaved.len()]).unwrap();
        let mut b = sample(1);
        b.interleaved = Storage::from_f32(vec![2.; b.interleaved.len()]).unwrap();
        let mut renderer = Renderer::new(
            &p,
            HashMap::from([("a".into(), Arc::new(a)), ("b".into(), Arc::new(b))]),
            48000,
        )
        .unwrap();
        let mut one = note(1);
        one.layers = Some(vec![p.layers[0]]);
        one.oscillator = Some(2);
        let mut empty = note(2);
        empty.layers = Some(Vec::new());
        let mut both = note(3);
        both.layers = Some(p.layers.clone());
        let commands = vec![
            script::Command {
                frame: 0,
                action: script::Action::Start(one),
            },
            script::Command {
                frame: 1,
                action: script::Action::Start(empty),
            },
            script::Command {
                frame: 2,
                action: script::Action::Start(both),
            },
            script::Command {
                frame: 3,
                action: script::Action::Release(1),
            },
            script::Command {
                frame: 4,
                action: script::Action::Release(3),
            },
        ];
        let output = renderer.render(&commands, &[], 5).unwrap();
        assert_eq!(
            output,
            vec![[1., 1.], [1., 1.], [2.5, 2.5], [1.5, 1.5], [0., 0.]]
        );
    }
    #[test]
    fn mono_effects_and_matrix_keep_native_bus_width_before_hardware_upmix() {
        let mut xml = String::from(
            r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a"/></Oscillators><Inserts><GainMatrix Gain_1_2="1"/><DigitalEq StereoMode="1" Type1="6" Gain1="-12" Freq1="3000" Channels1="2""#,
        );
        for i in 2..=16 {
            xml.push_str(&format!(" Enabled{i}=\"0\""));
        }
        xml.push_str("/></Inserts></Keygroup></Keygroups></Layer></Layers></Program>");
        let p = parse_program(&xml).unwrap();
        let mut source = sample(1);
        source.interleaved = Storage::from_f32(vec![0.25; source.interleaved.len()]).unwrap();
        let mut renderer =
            Renderer::new(&p, HashMap::from([("a".into(), Arc::new(source))]), 48000).unwrap();
        let output = renderer
            .render(
                &[script::Command {
                    frame: 0,
                    action: script::Action::Start(note(1)),
                }],
                &[],
                4,
            )
            .unwrap();
        assert_eq!(output, vec![[0.125, 0.125]; 4]);
        let p =
            parse_program(&xml.replace("Gain_1_2=\"1\"", "Gain_1_2=\"1\" Gain_1_1=\"0\"")).unwrap();
        let mut source = sample(1);
        source.interleaved = Storage::from_f32(vec![0.25; source.interleaved.len()]).unwrap();
        let mut renderer =
            Renderer::new(&p, HashMap::from([("a".into(), Arc::new(source))]), 48000).unwrap();
        let output = renderer
            .render(
                &[script::Command {
                    frame: 0,
                    action: script::Action::Start(note(1)),
                }],
                &[],
                4,
            )
            .unwrap();
        assert_eq!(output, vec![[0., 0.]; 4]);
    }
    #[test]
    fn native_multichannel_output_layout_and_mono_pan() {
        let mut frame = [0.; 12];
        frame[0] = 1.;
        assert!((downmix(frame, 6, 0., 0.).unwrap()[0] - 1. / 3f32.sqrt()).abs() < 1e-7);
        assert!((downmix(frame, 12, 0., 0.).unwrap()[0] - 1. / 6f32.sqrt()).abs() < 1e-7);
        frame = [1.; 12];
        let six = downmix(frame, 6, 0., 0.).unwrap();
        let expected = (2. + 2f32.sqrt()) / 3f32.sqrt();
        assert!((six[0] - expected).abs() < 3e-7 && six[0] == six[1]);
        assert_eq!(&downmix(frame, 10, 0., 0.).unwrap()[..2], &[1., 1.]);
        let mono = downmix(frame, 1, -0.5, 0.).unwrap();
        assert!((mono[0] - 0.8535534).abs() < 1e-7);
        assert!((mono[1] - 0.1464466).abs() < 1e-7);
        let mut stereo = [0.; 12];
        stereo[0] = 0.25;
        stereo[1] = 0.125;
        balance(&mut stereo, 1., 0.25).unwrap();
        assert!((stereo[0] - 0.21338835).abs() < 1e-7 && stereo[1] == 0.125);
        let equal_power = downmix(frame, 1, 0., 1.).unwrap();
        assert!((equal_power[0] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-7);
        assert_eq!(equal_power[0], equal_power[1]);
        assert!(downmix(frame, 5, 0., 0.).is_err());
    }
    #[test]
    fn fades_are_linear_layer_owned_and_replace_pending_kill() {
        let p=parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a"/></Oscillators></Keygroup></Keygroups></Layer><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut source = sample(1);
        source.interleaved = Storage::from_f32(vec![1.; source.interleaved.len()]).unwrap();
        let mut renderer =
            Renderer::new(&p, HashMap::from([("a".into(), Arc::new(source))]), 48000).unwrap();
        let fade = |frame, target, duration_frames, kill, layer| script::Command {
            frame,
            action: script::Action::Fade {
                id: 1,
                start: None,
                target,
                duration_frames,
                kill,
                layer,
            },
        };
        let notes = [
            script::Command {
                frame: 0,
                action: script::Action::Start(note(1)),
            },
            fade(1, 0., 4, true, Some(p.layers[0])),
            fade(3, 1., 2, false, Some(p.layers[0])),
            fade(6, 0., 2, true, Some(p.layers[0])),
            fade(9, 0., 0, true, Some(p.layers[1])),
        ];
        let output = renderer.render(&notes, &[], 10).unwrap();
        let expected = [1., 1., 0.875, 0.75, 0.875, 1., 1., 0.75, 0.5, 0.];
        for (actual, expected) in output.iter().zip(expected) {
            assert!((actual[0] - expected).abs() < 1e-7);
        }
        assert!(renderer.voices.is_empty());
    }
    #[test]
    fn authored_analog_generator_has_note_phase_and_native_c4_tuning() {
        let p=parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><MinBlepGenerator Waveform="4" StartPhase="0.25" BaseNote="60"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut renderer = Renderer::new(&p, HashMap::new(), 48000).unwrap();
        let output = renderer
            .render(
                &[script::Command {
                    frame: 0,
                    action: script::Action::Start(note(1)),
                }],
                &[],
                4,
            )
            .unwrap();
        for (index, frame) in output.iter().enumerate() {
            let expected = -0.25
                * (std::f64::consts::TAU * (0.25 + index as f64 * 261.6255653005986 / 48000.)).sin()
                    as f32;
            assert!((frame[0] - expected).abs() < 1e-6);
            assert_eq!(frame[0], frame[1]);
        }
        let stereo=parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><MinBlepGenerator Waveform="4" Stereo="1" StartPhase="0.25"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut renderer = Renderer::new(&stereo, HashMap::new(), 48000).unwrap();
        let frame = renderer
            .render(
                &[script::Command {
                    frame: 0,
                    action: script::Action::Start(note(1)),
                }],
                &[],
                1,
            )
            .unwrap()[0];
        assert_eq!(frame, [-0.25, -0.25]);
    }
    #[test]
    fn sample_gain_control_matches_independent_native_aligned_step() {
        let mut clock = GainClock::new(0, 0.);
        for frame in 0..20480 {
            assert_eq!(clock.value(frame, 0., 48000.), 0.);
        }
        let checkpoints = [
            (0, 0.),
            (1, 0.002226421),
            (32, 0.071245493),
            (128, 0.255947237),
            (512, 0.693510939),
        ];
        for offset in 0..=512 {
            let value = clock.value(20480 + offset, 1., 48000.);
            if let Some((_, expected)) = checkpoints.iter().find(|(frame, _)| *frame == offset) {
                assert!((value - expected).abs() < 4e-7, "at {offset}: {value}");
            }
        }
    }
    #[test]
    fn rack_parallel_chains_obey_their_bus_gain_and_mute() {
        let p=parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup TriggerRule="2"><Oscillators><SamplePlayer SamplePath="a"/></Oscillators><Inserts><EffectRack><Chains><AuxEffect Gain="0.5"><Inserts><Gain Volume="1"/></Inserts></AuxEffect><AuxEffect Mute="1"><Inserts><Gain Volume="0.25"/></Inserts></AuxEffect></Chains></EffectRack></Inserts></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        assert!(preflight(&p).is_empty());
        let mut renderer = Renderer::new(
            &p,
            HashMap::from([("a".into(), Arc::new(sample(2)))]),
            48000,
        )
        .unwrap();
        let output = renderer
            .render(
                &[script::Command {
                    frame: 0,
                    action: script::Action::Start(note(1)),
                }],
                &[],
                1,
            )
            .unwrap();
        assert_eq!(output[0], [0.5, 1.]);
    }
    #[test]
    fn controller_after_same_frame_note_on_uses_captured_initial_gain() {
        let p=parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a"><Connections><SignalConnection Source="@MIDI CC 1" Destination="Gain" Ratio="1"/></Connections></SamplePlayer></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut source = sample(1);
        source.interleaved = Storage::from_f32(vec![1.; source.interleaved.len()]).unwrap();
        let mut renderer =
            Renderer::new(&p, HashMap::from([("a".into(), Arc::new(source))]), 48000).unwrap();
        let output = renderer
            .render(
                &[
                    script::Command {
                        frame: 0,
                        action: script::Action::Start(note(1)),
                    },
                    script::Command {
                        frame: 0,
                        action: script::Action::Controller {
                            channel: 0,
                            controller: 1,
                            value: 127,
                        },
                    },
                ],
                &[],
                2,
            )
            .unwrap();
        assert_eq!(output[0], [0., 0.]);
        assert!(
            (output[1][0] - 0.0011132105).abs() < 1e-8,
            "got {}",
            output[1][0]
        );
    }
    #[test]
    fn stale_foreign_riff_loop_uses_frame_start_and_clamps_only_effective_end() {
        let p=parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut source = sample(2);
        source.loops[0].end = 15;
        let source = Arc::new(source);
        let mut renderer =
            Renderer::new(&p, HashMap::from([("a".into(), source.clone())]), 48000).unwrap();
        let output = renderer
            .render(
                &[script::Command {
                    frame: 0,
                    action: script::Action::Start(note(1)),
                }],
                &[],
                10,
            )
            .unwrap();
        assert_eq!(output[7], [8., 9.]);
        assert_eq!(output[8], [3., 4.]);
        assert_eq!(output[9], [4., 5.]);
        assert_eq!(source.loops[0].end, 15);
        let mut wave = sample(2);
        wave.riff_metadata.clear();
        let mut renderer =
            Renderer::new(&p, HashMap::from([("a".into(), Arc::new(wave))]), 48000).unwrap();
        let output = renderer
            .render(
                &[script::Command {
                    frame: 0,
                    action: script::Action::Start(note(1)),
                }],
                &[],
                9,
            )
            .unwrap();
        assert_eq!(output[8], [0., 0.]);
    }
    #[test]
    fn serialized_playback_bounds_and_forward_loop_match_native_frames() {
        let make_program = |direction, reverse, stop, loop_xml: &str| {
            parse_program(&format!(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a" Reverse="{reverse}"><PlaybackOptions Start="16" Stop="{stop}" PlayDirection="{direction}" PlayRelease="1">{loop_xml}</PlaybackOptions></SamplePlayer></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#)).unwrap()
        };
        let source = Arc::new(Sample {
            rate: 48000,
            channels: 1,
            frames: 128,
            interleaved: Storage::from_f32((0..128).map(|i| (i + 1) as f32 / 2048.).collect())
                .unwrap(),
            loops: Vec::new(),
            unity_note: None,
            wavetable_cycle_frames: None,
            wavetable_image: false,
            riff_metadata: Vec::new(),
        });
        for (direction, reverse) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let p = make_program(direction, reverse, 64, "");
            let mut renderer =
                Renderer::new(&p, HashMap::from([("a".into(), source.clone())]), 48000).unwrap();
            let output = renderer
                .render(
                    &[script::Command {
                        frame: 0,
                        action: script::Action::Start(note(1)),
                    }],
                    &[],
                    50,
                )
                .unwrap();
            let backwards = direction == 1 || reverse == 1;
            let count = if backwards { 49 } else { 48 };
            for (i, frame) in output.iter().enumerate() {
                let expected = if i >= count {
                    0.
                } else if backwards {
                    (65 - i) as f32 / 4096.
                } else {
                    (17 + i) as f32 / 4096.
                };
                assert_eq!(*frame, [expected, expected]);
            }
        }
        let p = make_program(1, 0, 128, "");
        let mut renderer =
            Renderer::new(&p, HashMap::from([("a".into(), source.clone())]), 48000).unwrap();
        assert!(
            renderer
                .render(
                    &[script::Command {
                        frame: 0,
                        action: script::Action::Start(note(1))
                    }],
                    &[],
                    10
                )
                .unwrap()
                .iter()
                .all(|frame| *frame == [0., 0.])
        );
        let p = make_program(0, 0, 127, r#"<Loop Start="20" End="24" Type="0"/>"#);
        let mut renderer = Renderer::new(&p, HashMap::from([("a".into(), source)]), 48000).unwrap();
        let output = renderer
            .render(
                &[script::Command {
                    frame: 0,
                    action: script::Action::Start(note(1)),
                }],
                &[],
                16,
            )
            .unwrap();
        for (i, frame) in output.iter().enumerate() {
            let index = if i < 8 { 16 + i } else { 20 + (i - 8) % 4 };
            assert_eq!(*frame, [(index + 1) as f32 / 4096.; 2]);
        }
        assert!(
            !preflight(&make_program(
                0,
                0,
                127,
                r#"<Loop Start="20" End="24" Type="1"/>"#
            ))
            .is_empty()
        );
    }
    #[test]
    fn omnichannel_controller_reaches_voice_channel_without_changing_note_identity() {
        let p=parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a"><Connections><SignalConnection Source="@MIDI CC 1" Destination="Gain" Ratio="1"/></Connections></SamplePlayer></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut source = sample(1);
        source.interleaved = Storage::from_f32(vec![1.; source.interleaved.len()]).unwrap();
        let mut renderer =
            Renderer::new(&p, HashMap::from([("a".into(), Arc::new(source))]), 48000).unwrap();
        let mut voiced = note(1);
        voiced.channel = 9;
        let notes = [
            script::Command {
                frame: 0,
                action: script::Action::ControllerAll {
                    controller: 1,
                    value: 127,
                },
            },
            script::Command {
                frame: 0,
                action: script::Action::Start(voiced),
            },
            script::Command {
                frame: 1,
                action: script::Action::Controller {
                    channel: 0,
                    controller: 1,
                    value: 0,
                },
            },
            script::Command {
                frame: 32,
                action: script::Action::ControllerAll {
                    controller: 1,
                    value: 0,
                },
            },
        ];
        let output = renderer.render(&notes, &[], 34).unwrap();
        assert_eq!(output[0], [0.5, 0.5]);
        assert_eq!(output[1], [0.5, 0.5]);
        assert_eq!(output[32], [0.5, 0.5]);
        assert!((output[33][0] - 0.4988868).abs() < 1e-7);
        assert_eq!(renderer.voices[0].note.channel, 9);
    }
    #[test]
    fn dynamic_resource_assets_validate_before_the_renderer_can_swap_them() {
        let p=parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        for bad in [0, 1, 2] {
            let mut extra = sample(1);
            match bad {
                0 => extra.rate = 0,
                1 => extra.frames = 9,
                _ => extra.channels = 0,
            };
            assert!(
                Renderer::new(
                    &p,
                    HashMap::from([
                        ("a".into(), Arc::new(sample(1))),
                        ("extra".into(), Arc::new(extra))
                    ]),
                    48000
                )
                .is_err()
            );
        }
    }
    #[test]
    fn event_tune_reaches_native_key_source_without_changing_keygroup_selection() {
        let p=parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup LowKey="60" HighKey="60"><Oscillators><SamplePlayer SamplePath="a"><Connections><SignalConnection Source="@VoiceParam Key" Destination="Gain" Ratio="1"/></Connections></SamplePlayer></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut source = sample(1);
        source.interleaved = Storage::from_f32(vec![1.; source.interleaved.len()]).unwrap();
        let mut renderer =
            Renderer::new(&p, HashMap::from([("a".into(), Arc::new(source))]), 48000).unwrap();
        let mut tuned = note(1);
        tuned.tune = 0.5;
        let output = renderer
            .render(
                &[script::Command {
                    frame: 0,
                    action: script::Action::Start(tuned),
                }],
                &[],
                1,
            )
            .unwrap();
        assert!((output[0][0] - 0.23818898).abs() < 1e-8);
        assert_eq!(renderer.voices[0].note.note, 60);
    }
    #[test]
    fn duplicate_logical_note_ids_share_controls_and_release_fifo_once() {
        let p = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut source = sample(1);
        source.interleaved = Storage::from_f32(vec![1.; source.interleaved.len()]).unwrap();
        let resources = HashMap::from([("a".into(), Arc::new(source))]);
        let mut renderer = Renderer::new(&p, resources, 48000).unwrap();
        let commands = [
            script::Command {
                frame: 0,
                action: script::Action::Start(note(1)),
            },
            script::Command {
                frame: 0,
                action: script::Action::Start(note(1)),
            },
            script::Command {
                frame: 2,
                action: script::Action::Change {
                    id: 1,
                    gain: Some(0.5),
                    tune: None,
                    pan: None,
                    layer: None,
                    relative: false,
                },
            },
            script::Command {
                frame: 4,
                action: script::Action::Release(1),
            },
        ];
        let output = renderer.render(&commands[..3], &[], 4).unwrap();
        assert_eq!(output, vec![[1., 1.], [1., 1.], [0.5, 0.5], [0.5, 0.5]]);
        assert_eq!(renderer.voices.len(), 2);
        assert_ne!(renderer.voices[0].instance, renderer.voices[1].instance);
        assert_eq!(
            renderer.render(&commands[3..], &[], 1).unwrap(),
            vec![[0.25, 0.25]]
        );
        assert_eq!(renderer.voices.len(), 1);
        assert_eq!(
            renderer
                .render(
                    &[script::Command {
                        frame: 5,
                        action: script::Action::Release(1)
                    }],
                    &[],
                    1
                )
                .unwrap(),
            vec![[0.25, 0.25]]
        );
        assert_eq!(
            renderer
                .render(
                    &[script::Command {
                        frame: 6,
                        action: script::Action::ReleaseNote {
                            id: 1,
                            note: 60,
                            channel: 0,
                            layer: None
                        }
                    }],
                    &[],
                    1
                )
                .unwrap(),
            vec![[0., 0.]]
        );
        assert!(renderer.voices.is_empty());
    }
    #[test]
    fn analog_envelope_keeps_owned_release_and_bypasses_scalar_gain_smoothing() {
        let p = parse_program(r#"<Program><ControlSignalSources><AnalogADSR Name="Env" AttackTime="0.002" DecayTime="0.2" SustainLevel="0.4" ReleaseTime="0.002"/></ControlSignalSources><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a"><Connections><SignalConnection Source="$Program/Env" Destination="Gain" Ratio="1"/></Connections></SamplePlayer></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut source = sample(1);
        source.interleaved = Storage::from_f32(vec![1.; source.interleaved.len()]).unwrap();
        let mut renderer =
            Renderer::new(&p, HashMap::from([("a".into(), Arc::new(source))]), 48000).unwrap();
        let output = renderer
            .render(
                &[
                    script::Command {
                        frame: 0,
                        action: script::Action::Start(note(1)),
                    },
                    script::Command {
                        frame: 113,
                        action: script::Action::Release(1),
                    },
                ],
                &[],
                146,
            )
            .unwrap();
        for (frame, expected) in [
            (0, 0.),
            (96, 0.249999985),
            (112, 0.281604865),
            (113, 0.284641981125),
            (145, 0.248657675),
        ] {
            assert!(
                (output[frame][0] - expected).abs() < 0.000002,
                "frame {frame}: {:?}",
                output[frame]
            );
        }
        assert_eq!(renderer.voices.len(), 1);
        assert_eq!(renderer.voices[0].note_off, Some(113));
        let tail = renderer.render(&[], &[], 48000).unwrap();
        assert!(renderer.voices.is_empty());
        assert_eq!(tail.last().unwrap(), &[0., 0.]);
    }
    #[test]
    fn looped_release_flag_keeps_or_exits_sample_loop_during_envelope_release() {
        for (flag, done) in [(0, true), (1, false)] {
            let p = parse_program(&format!(r#"<Program><ControlSignalSources><AnalogADSR Name="Env" AttackTime="0.002" DecayTime="0.2" SustainLevel="0.4" ReleaseTime="0.2"/></ControlSignalSources><Layers><Layer><Keygroups><Keygroup><Connections><SignalConnection Source="$Program/Env" Destination="Gain" Ratio="1"/></Connections><Oscillators><SamplePlayer SamplePath="a"><PlaybackOptions Start="0" Stop="8" PlayRelease="{flag}"><Loop Start="2" End="5" Type="0"/></PlaybackOptions></SamplePlayer></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#)).unwrap();
            let mut renderer = Renderer::new(
                &p,
                HashMap::from([("a".into(), Arc::new(sample(1)))]),
                48000,
            )
            .unwrap();
            renderer
                .render(
                    &[
                        script::Command {
                            frame: 0,
                            action: script::Action::Start(note(1)),
                        },
                        script::Command {
                            frame: 3,
                            action: script::Action::Release(1),
                        },
                    ],
                    &[],
                    16,
                )
                .unwrap();
            assert_eq!(renderer.voices.len(), 1);
            assert_eq!(renderer.voices[0].oscillators[0].done, done);
            assert_eq!(
                renderer.voices[0].oscillators[0].loop_data.is_some(),
                flag == 1
            );
        }
    }
    #[test]
    fn program_delay_receives_stereo_projection_and_preserves_native_tail() {
        let p = parse_program(r#"<Program><Inserts><DualDelay DelayTime="0.01" Feedback="0.5" Mix="1"/></Inserts><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a" NoteTracking="0"><PlaybackOptions Stop="8"/></SamplePlayer></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut source = sample(1);
        let mut pcm = vec![0.; 8];
        pcm[0] = 0.5;
        source.interleaved = Storage::from_f32(pcm).unwrap();
        let mut renderer =
            Renderer::new(&p, HashMap::from([("a".into(), Arc::new(source))]), 48000).unwrap();
        let output = renderer
            .render(
                &[
                    script::Command {
                        frame: 0,
                        action: script::Action::Start(note(1)),
                    },
                    script::Command {
                        frame: 10,
                        action: script::Action::Release(1),
                    },
                ],
                &[],
                1000,
            )
            .unwrap();
        assert!(output[..480].iter().all(|frame| *frame == [0., 0.]));
        for (frame, expected) in [
            (480, 0.11557837575674057),
            (481, 0.008129146881401539),
            (960, 0.05334042),
        ] {
            assert!((output[frame][0] - expected).abs() < 3e-8);
            assert_eq!(output[frame][0], output[frame][1]);
        }
        let mono = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a"/></Oscillators><Inserts><DualDelay/></Inserts></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut renderer = Renderer::new(
            &mono,
            HashMap::from([("a".into(), Arc::new(sample(1)))]),
            48000,
        )
        .unwrap();
        assert!(
            renderer
                .render(
                    &[script::Command {
                        frame: 0,
                        action: script::Action::Start(note(1))
                    }],
                    &[],
                    1
                )
                .unwrap_err()
                .chain()
                .any(|cause| cause.to_string().contains("source width"))
        );
    }
    #[test]
    fn keygroup_xpander_executes_before_projection_with_actual_note_tracking() {
        let p = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a" NoteTracking="0"><PlaybackOptions Stop="264"/></SamplePlayer></Oscillators><Inserts><XpanderFilter Freq="1000" Q="0.75" DistortionType="0"/></Inserts></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut source = sample(1);
        // These native impulse observations were captured after silence. Warm
        // the serialized scalar solver through its initial 256-frame ramp;
        // retain the original observations and tolerance, not a cold substitute.
        let mut pcm = vec![0.; 264];
        pcm[256] = 32767. / 32768.;
        source.frames = 264;
        source.loops.clear();
        source.interleaved = Storage::from_f32(pcm).unwrap();
        let resources = HashMap::from([("a".into(), Arc::new(source))]);
        let mut renderer = Renderer::new(&p, resources.clone(), 48000).unwrap();
        let output = renderer
            .render(
                &[script::Command {
                    frame: 0,
                    action: script::Action::Start(note(1)),
                }],
                &[],
                288,
            )
            .unwrap();
        for (frame, expected) in [
            (3, 1.893573499e-04),
            (7, 7.173242047e-03),
            (15, 4.259800911e-02),
            (31, 2.153839171e-02),
        ] {
            assert!((output[256 + frame][0] - expected * 0.5).abs() < 1e-6);
        }
        assert!(output[..256].iter().all(|frame| *frame == [0., 0.]));
        // Keep the note-tracking comparison audible during its original short
        // interval rather than comparing only the new fixture's silent prefix.
        let resources = HashMap::from([("a".into(), Arc::new(sample(1)))]);
        let tracked = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a" NoteTracking="0"><PlaybackOptions Stop="8"/></SamplePlayer></Oscillators><Inserts><XpanderFilter Freq="1000" KeyTracking="1"/></Inserts></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let fixed = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a" NoteTracking="0"><PlaybackOptions Stop="8"/></SamplePlayer></Oscillators><Inserts><XpanderFilter Freq="2000" KeyTracking="0"/></Inserts></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut key = note(1);
        key.note = 72;
        let commands = [script::Command {
            frame: 0,
            action: script::Action::Start(key),
        }];
        let a = Renderer::new(&tracked, resources.clone(), 48000)
            .unwrap()
            .render(&commands, &[], 32)
            .unwrap();
        let b = Renderer::new(&fixed, resources, 48000)
            .unwrap()
            .render(&commands, &[], 32)
            .unwrap();
        assert_eq!(a, b);
    }
    #[test]
    fn shared_voice_gate_waits_for_its_channel_sustain_pedal() {
        let p = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut renderer = Renderer::new(
            &p,
            HashMap::from([("a".into(), Arc::new(sample(1)))]),
            48000,
        )
        .unwrap();
        let mut key = note(1);
        key.channel = 9;
        let output = renderer
            .render(
                &[
                    script::Command {
                        frame: 0,
                        action: script::Action::Controller {
                            channel: 9,
                            controller: 64,
                            value: 127,
                        },
                    },
                    script::Command {
                        frame: 0,
                        action: script::Action::Start(key.clone()),
                    },
                    script::Command {
                        frame: 0,
                        action: script::Action::Start(key),
                    },
                    script::Command {
                        frame: 2,
                        action: script::Action::ReleaseNote {
                            id: 1,
                            note: 60,
                            channel: 9,
                            layer: None,
                        },
                    },
                    script::Command {
                        frame: 2,
                        action: script::Action::ReleaseNote {
                            id: 1,
                            note: 60,
                            channel: 9,
                            layer: None,
                        },
                    },
                    script::Command {
                        frame: 3,
                        action: script::Action::Controller {
                            channel: 0,
                            controller: 64,
                            value: 0,
                        },
                    },
                    script::Command {
                        frame: 5,
                        action: script::Action::ControllerAll {
                            controller: 64,
                            value: 0,
                        },
                    },
                ],
                &[],
                6,
            )
            .unwrap();
        assert_eq!(output[2], [3., 3.]);
        assert_eq!(output[3], [4., 4.]);
        assert_eq!(output[4], [5., 5.]);
        assert_eq!(output[5], [0., 0.]);
        assert!(renderer.voices.is_empty());
    }
    #[test]
    fn global_envelope_target_requires_measured_gate_context() {
        let p = parse_program(r#"<Program><ControlSignalSources><AnalogADSR Name="Env"/></ControlSignalSources><Connections><SignalConnection Source="$Program/Env" Destination="Gain" Ratio="1"/></Connections></Program>"#).unwrap();
        assert!(
            preflight(&p)
                .iter()
                .any(|entry| entry.reason.contains("gate law"))
        );
    }
    #[test]
    fn polyphonic_pressure_routes_per_key_and_channel_with_omni_broadcast() {
        let p = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a" NoteTracking="0"><Connections><SignalConnection Source="@PolyAfterTouch" Destination="Gain" Ratio="1"/></Connections></SamplePlayer></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut source = sample(1);
        source.interleaved = Storage::from_f32(vec![1.; 8]).unwrap();
        let mut renderer =
            Renderer::new(&p, HashMap::from([("a".into(), Arc::new(source))]), 48000).unwrap();
        let mut key = note(1);
        key.channel = 9;
        let output = renderer
            .render(
                &[
                    script::Command {
                        frame: 0,
                        action: script::Action::PolyAfterTouch {
                            channel: 0,
                            note: 60,
                            value: 127,
                        },
                    },
                    script::Command {
                        frame: 0,
                        action: script::Action::PolyAfterTouch {
                            channel: 9,
                            note: 61,
                            value: 127,
                        },
                    },
                    script::Command {
                        frame: 0,
                        action: script::Action::Start(key),
                    },
                    script::Command {
                        frame: 32,
                        action: script::Action::PolyAfterTouchAll {
                            note: 60,
                            value: 127,
                        },
                    },
                ],
                &[],
                34,
            )
            .unwrap();
        assert_eq!(output[0], [0., 0.]);
        assert_eq!(output[31], [0., 0.]);
        assert!((output[33][0] - 0.0011132105).abs() < 1e-8);
    }
    #[test]
    fn sample_interpolation_modes_match_authored_native_fractional_points() {
        for (mode, expected) in [
            (0, [0., 0.0625, 0.0625, -0.125, -0.125, 0.0625]),
            (1, [0., 0.03125, 0.0625, -0.03125, -0.125, -0.03125]),
            (2, [0., 0.04296875, 0.0625, -0.0390625, -0.125, -0.04296875]),
            (3, [0., 0.04296875, 0.0625, -0.0390625, -0.125, -0.04296875]),
        ] {
            let p = parse_program(&format!(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a" NoteTracking="0" InterpolationMode="{mode}"><PlaybackOptions Stop="8"/></SamplePlayer></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#)).unwrap();
            let mut source = sample(1);
            source.rate = 24000;
            source.interleaved =
                Storage::from_f32(vec![0., 0.125, -0.25, 0.125, 0.125, 0.5, 0.25, 0.]).unwrap();
            let mut renderer =
                Renderer::new(&p, HashMap::from([("a".into(), Arc::new(source))]), 48000).unwrap();
            let output = renderer
                .render(
                    &[script::Command {
                        frame: 0,
                        action: script::Action::Start(note(1)),
                    }],
                    &[],
                    6,
                )
                .unwrap();
            for (actual, expected) in output.iter().zip(expected) {
                assert_eq!(actual, &[expected, expected]);
            }
        }
    }
    #[test]
    fn cubic_neighbors_have_physical_zero_padding_and_cross_playhead_markers() {
        for (start, stop, expected) in [(0, 8, 0.0869140625), (1, 2, 0.14111328125)] {
            let p = parse_program(&format!(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a" NoteTracking="0" InterpolationMode="2"><PlaybackOptions Start="{start}" Stop="{stop}"/></SamplePlayer></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#)).unwrap();
            let mut source = sample(1);
            source.rate = 24000;
            source.interleaved = Storage::from_f32(vec![
                0.125, 0.21875, 0.3125, 0.140625, 0.234375, 0.328125, 0.15625, 0.25,
            ])
            .unwrap();
            let mut renderer =
                Renderer::new(&p, HashMap::from([("a".into(), Arc::new(source))]), 48000).unwrap();
            let output = renderer
                .render(
                    &[script::Command {
                        frame: 0,
                        action: script::Action::Start(note(1)),
                    }],
                    &[],
                    2,
                )
                .unwrap();
            assert_eq!(output[1], [expected, expected]);
        }
    }
    #[test]
    fn dahdsr_release_uses_native_stage_clock_and_keeps_layer_owned_gate() {
        let p = parse_program(r#"<Program><ControlSignalSources><DAHDSR Name="Env" DelayTime="0.001" AttackTime="0.001" HoldTime="0.001" DecayTime="0.001" SustainLevel="0.25" ReleaseTime="0.002"/></ControlSignalSources><Layers><Layer Name="A"><Keygroups><Keygroup><Connections><SignalConnection Source="$Program/Env" Destination="Gain" Ratio="1"/></Connections><Oscillators><SamplePlayer SamplePath="a"/></Oscillators></Keygroup></Keygroups></Layer><Layer Name="B"><Keygroups><Keygroup><Connections><SignalConnection Source="$Program/Env" Destination="Gain" Ratio="1"/></Connections><Oscillators><SamplePlayer SamplePath="a"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut source = sample(1);
        source.interleaved = Storage::from_f32(vec![1.; 8]).unwrap();
        let mut renderer =
            Renderer::new(&p, HashMap::from([("a".into(), Arc::new(source))]), 48000).unwrap();
        let output = renderer
            .render(
                &[
                    script::Command {
                        frame: 0,
                        action: script::Action::Start(note(1)),
                    },
                    script::Command {
                        frame: 192,
                        action: script::Action::ReleaseNote {
                            id: 1,
                            note: 60,
                            channel: 0,
                            layer: Some(p.layers[0]),
                        },
                    },
                ],
                &[],
                321,
            )
            .unwrap();
        for (frame, expected) in [
            (32, 0.),
            (64, 0.46875),
            (96, 1.),
            (128, 1.),
            (160, 0.6484375),
            (192, 0.25),
        ] {
            assert!(
                (output[frame][0] - expected).abs() < 1e-6,
                "{frame}: {:?}",
                output[frame]
            );
        }
        assert!((output[320][0] - 0.125).abs() < 1e-6);
        assert_eq!(renderer.voices.len(), 1);
        assert_eq!(renderer.voices[0].layer, p.layers[1]);
        assert_eq!(renderer.voices[0].note_off, None);
    }
    #[test]
    fn effective_gain_volume_uses_modulation_domain_above_static_gui_limit() {
        let p = parse_program(r#"<Program><Mappers><ControlSignalMapper Name="Boost" Min="-3" Max="1">-1 -1</ControlSignalMapper></Mappers><Inserts><Gain Volume="1"><Connections><SignalConnection Source="@MIDI CC 1" Destination="Volume" Mapper="$Program/Boost" Ratio="-1"/><SignalConnection Source="@MIDI CC 2" Destination="Volume" Mapper="$Program/Boost" Ratio="-1"/></Connections></Gain></Inserts><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut source = sample(1);
        source.interleaved = Storage::from_f32(vec![0.25; 8]).unwrap();
        let mut renderer =
            Renderer::new(&p, HashMap::from([("a".into(), Arc::new(source))]), 48000).unwrap();
        assert_eq!(
            renderer
                .render(
                    &[script::Command {
                        frame: 0,
                        action: script::Action::Start(note(1))
                    }],
                    &[],
                    1
                )
                .unwrap(),
            vec![[2., 2.]]
        );
    }
    #[test]
    fn aggregate_processor_storage_counts_globals_and_each_voice_instance() {
        let p = parse_program(r#"<Program><Inserts><TrackDelay/></Inserts><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a"/></Oscillators><Inserts><TrackDelay/></Inserts></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut renderer = Renderer::new(
            &p,
            HashMap::from([("a".into(), Arc::new(sample(2)))]),
            48000,
        )
        .unwrap();
        let global = renderer.processor_bytes();
        renderer
            .render(
                &[
                    script::Command {
                        frame: 0,
                        action: script::Action::Start(note(1)),
                    },
                    script::Command {
                        frame: 0,
                        action: script::Action::Start(note(2)),
                    },
                ],
                &[],
                1,
            )
            .unwrap();
        assert_eq!(renderer.processor_bytes(), global * 3);
        renderer
            .render(
                &[script::Command {
                    frame: 1,
                    action: script::Action::Release(1),
                }],
                &[],
                1,
            )
            .unwrap();
        assert_eq!(renderer.processor_bytes(), global * 2);
    }
    #[test]
    fn forwarded_key_release_matches_identity_key_layer_and_ignores_channel() {
        let p = parse_program(r#"<Program><Layers><Layer Name="A"><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a" NoteTracking="0"/></Oscillators></Keygroup></Keygroups></Layer><Layer Name="B"><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a" NoteTracking="0"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut source = sample(1);
        source.interleaved = Storage::from_f32(vec![1.; 8]).unwrap();
        let mut renderer =
            Renderer::new(&p, HashMap::from([("a".into(), Arc::new(source))]), 48000).unwrap();
        let mut alternate = note(1);
        alternate.note = 72;
        alternate.channel = 1;
        let commands = [
            script::Command {
                frame: 0,
                action: script::Action::Start(note(1)),
            },
            script::Command {
                frame: 0,
                action: script::Action::Start(alternate),
            },
            script::Command {
                frame: 0,
                action: script::Action::Start(note(2)),
            },
            script::Command {
                frame: 1,
                action: script::Action::ReleaseNote {
                    id: 999,
                    note: 60,
                    channel: 0,
                    layer: None,
                },
            },
            script::Command {
                frame: 2,
                action: script::Action::ReleaseNote {
                    id: 1,
                    note: 61,
                    channel: 1,
                    layer: None,
                },
            },
            script::Command {
                frame: 3,
                action: script::Action::ReleaseNote {
                    id: 1,
                    note: 60,
                    channel: 0,
                    layer: Some(p.layers[0]),
                },
            },
            script::Command {
                frame: 4,
                action: script::Action::ReleaseNote {
                    id: 1,
                    note: 72,
                    channel: 1,
                    layer: None,
                },
            },
            script::Command {
                frame: 5,
                action: script::Action::ReleaseNote {
                    id: 1,
                    note: 60,
                    channel: 14,
                    layer: None,
                },
            },
            script::Command {
                frame: 6,
                action: script::Action::ReleaseNote {
                    id: 2,
                    note: 60,
                    channel: 0,
                    layer: None,
                },
            },
        ];
        assert_eq!(
            renderer.render(&commands, &[], 7).unwrap(),
            vec![
                [3., 3.],
                [3., 3.],
                [3., 3.],
                [2.5, 2.5],
                [1.5, 1.5],
                [1., 1.],
                [0., 0.]
            ]
        );
        assert!(renderer.voices.is_empty());
    }
    #[test]
    fn rack_filter_uses_voice_note_while_shared_aux_filter_stays_neutral() {
        let fixture = |frequency, tracking| {
            parse_program(&format!(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a" NoteTracking="0"><PlaybackOptions Stop="8"/></SamplePlayer></Oscillators><Inserts><EffectRack><Chains><AuxEffect><Inserts><XpanderFilter Freq="{frequency}" KeyTracking="{tracking}"/></Inserts></AuxEffect></Chains></EffectRack></Inserts></Keygroup></Keygroups></Layer></Layers></Program>"#)).unwrap()
        };
        let mut source = sample(1);
        let mut pcm = vec![0.; 8];
        pcm[0] = 0.25;
        source.interleaved = Storage::from_f32(pcm).unwrap();
        let resources = HashMap::from([("a".into(), Arc::new(source))]);
        let mut key = note(1);
        key.note = 72;
        let commands = [script::Command {
            frame: 0,
            action: script::Action::Start(key),
        }];
        let a = fixture(1000, 1);
        let b = fixture(2000, 0);
        let a = Renderer::new(&a, resources.clone(), 48000)
            .unwrap()
            .render(&commands, &[], 32)
            .unwrap();
        let b = Renderer::new(&b, resources.clone(), 48000)
            .unwrap()
            .render(&commands, &[], 32)
            .unwrap();
        assert_eq!(a, b);
        let fixture = |tracking| {
            parse_program(&format!(r#"<Program><Auxs><AuxEffect Name="A"><Inserts><XpanderFilter Freq="1000" KeyTracking="{tracking}"/></Inserts></AuxEffect></Auxs><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a" NoteTracking="0"><PlaybackOptions Stop="8"/></SamplePlayer></Oscillators><BusRouters><BusRouter Destination="$Program/A" Gain="1"/></BusRouters></Keygroup></Keygroups></Layer></Layers></Program>"#)).unwrap()
        };
        let a = fixture(1);
        let b = fixture(0);
        assert_eq!(
            Renderer::new(&a, resources.clone(), 48000)
                .unwrap()
                .render(&commands, &[], 32)
                .unwrap(),
            Renderer::new(&b, resources, 48000)
                .unwrap()
                .render(&commands, &[], 32)
                .unwrap()
        );
    }
    #[test]
    fn wave_shaper_executes_native_oversampling_history_before_mono_projection() {
        let p = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a"><PlaybackOptions Stop="8"/></SamplePlayer></Oscillators><Inserts><WaveShaper PreFreq="22000" PostFreq="2" Oversampling="1"/></Inserts></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut source = sample(1);
        let mut pcm = vec![0.; 8];
        pcm[0] = 0.25;
        source.interleaved = Storage::from_f32(pcm).unwrap();
        let mut renderer =
            Renderer::new(&p, HashMap::from([("a".into(), Arc::new(source))]), 48000).unwrap();
        let output = renderer
            .render(
                &[script::Command {
                    frame: 0,
                    action: script::Action::Start(note(1)),
                }],
                &[],
                8,
            )
            .unwrap();
        for (actual, expected) in output.iter().zip([
            0.00011817346967291087,
            0.004105924628674984,
            0.03593229502439499,
            0.10866370797157288,
            0.08956358581781387,
            -0.05086439847946167,
            -0.008961625397205353,
            0.03720388561487198,
        ]) {
            assert!((f64::from(actual[0]) - expected).abs() < 1e-7);
            assert_eq!(actual[0], actual[1]);
        }
    }
    #[test]
    fn fifo_release_gates_one_post_with_every_microphone_keygroup() {
        let p = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a"/></Oscillators></Keygroup><Keygroup><Oscillators><SamplePlayer SamplePath="a"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut source = sample(1);
        source.interleaved = Storage::from_f32(vec![0.25; 8]).unwrap();
        let mut renderer =
            Renderer::new(&p, HashMap::from([("a".into(), Arc::new(source))]), 48000).unwrap();
        let mut second = note(1);
        second.volume = 2.;
        second.tune = 12.;
        assert_eq!(
            renderer
                .render(
                    &[
                        script::Command {
                            frame: 0,
                            action: script::Action::Start(note(1))
                        },
                        script::Command {
                            frame: 0,
                            action: script::Action::Start(second)
                        }
                    ],
                    &[],
                    1
                )
                .unwrap(),
            vec![[0.75, 0.75]]
        );
        assert_eq!(renderer.voices[0].launch, renderer.voices[1].launch);
        assert_eq!(renderer.voices[2].launch, renderer.voices[3].launch);
        assert_ne!(renderer.voices[0].launch, renderer.voices[2].launch);
        assert_eq!(
            renderer
                .render(
                    &[script::Command {
                        frame: 1,
                        action: script::Action::ReleaseNote {
                            id: 1,
                            note: 60,
                            channel: 15,
                            layer: None
                        }
                    }],
                    &[],
                    1
                )
                .unwrap(),
            vec![[0.5, 0.5]]
        );
        assert_eq!(renderer.voices.len(), 2);
        assert!(renderer.voices.iter().all(|voice| voice.note.tune == 12.));
        assert_eq!(
            renderer
                .render(
                    &[script::Command {
                        frame: 2,
                        action: script::Action::ReleaseNote {
                            id: 1,
                            note: 60,
                            channel: 0,
                            layer: None
                        }
                    }],
                    &[],
                    1
                )
                .unwrap(),
            vec![[0., 0.]]
        );
    }
    #[test]
    fn program_maximizer_has_native_lookahead_and_gain_after_sample_projection() {
        let p = parse_program(r#"<Program><Inserts><Maximizer/></Inserts><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut source = sample(2);
        source.interleaved =
            Storage::from_f32((0..8).flat_map(|_| [0.25, 0.125]).collect()).unwrap();
        let mut renderer =
            Renderer::new(&p, HashMap::from([("a".into(), Arc::new(source))]), 48000).unwrap();
        let output = renderer
            .render(
                &[script::Command {
                    frame: 0,
                    action: script::Action::Start(note(1)),
                }],
                &[],
                97,
            )
            .unwrap();
        assert!(output[..96].iter().all(|frame| *frame == [0., 0.]));
        assert!((output[96][0] - 0.49310568).abs() < 1e-7);
        assert_eq!(output[96][0], 2. * output[96][1]);
    }
    #[test]
    fn processor_memory_includes_enum_padding_and_boxed_state() {
        let scalar = Processor::Gain(Gain::new(2).unwrap());
        let matrix = Processor::Matrix(GainMatrix::new(2, 2).unwrap());
        assert_eq!(scalar.memory_bytes(), std::mem::size_of::<Processor>());
        assert_eq!(matrix.memory_bytes(), std::mem::size_of::<Processor>());
        let p =
            parse_program(r#"<Program><Inserts><XpanderFilter/><WaveShaper/></Inserts></Program>"#)
                .unwrap();
        for node in p
            .nodes
            .iter()
            .filter(|node| filter::supports(&node.kind) || waveshaper::supports(&node.kind))
        {
            let processor = Processor::new(node, 48000., 2, &Arc::new(HashMap::new()))
                .unwrap()
                .unwrap();
            let boxed_bytes = if filter::supports(&node.kind) {
                std::mem::size_of::<XpanderFilter>()
            } else {
                std::mem::size_of::<WaveShaper>()
            };
            assert_eq!(
                processor.memory_bytes(),
                std::mem::size_of::<Processor>() + boxed_bytes
            );
        }
        assert!(std::mem::size_of::<Processor>() < std::mem::size_of::<XpanderFilter>());
    }
    #[test]
    fn program_sparkverb_keeps_native_stereo_projection_and_static_echo() {
        let p = parse_program(r#"<Program><Inserts><SparkVerb ModDepth="0" Mix="1"/></Inserts><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut source = sample(1);
        source.interleaved = Storage::from_f32(vec![0.25, 0., 0., 0., 0., 0., 0., 0.]).unwrap();
        let mut renderer =
            Renderer::new(&p, HashMap::from([("a".into(), Arc::new(source))]), 48000).unwrap();
        let output = renderer
            .render(
                &[script::Command {
                    frame: 0,
                    action: script::Action::Start(note(1)),
                }],
                &[],
                3805,
            )
            .unwrap();
        assert!(output[..1811].iter().all(|frame| *frame == [0., 0.]));
        assert!((output[1811][0] - 0.033346776).abs() < 1e-7);
        assert!((output[3804][0] - 0.020425495).abs() < 1e-7);
        assert!(renderer.processor_bytes() > std::mem::size_of::<Processor>());
        let unsupported = parse_program(
            r#"<Program><Inserts><SparkVerb ModDepth="1" Mode="0"/></Inserts></Program>"#,
        )
        .unwrap();
        assert!(
            preflight(&unsupported)
                .iter()
                .any(|entry| entry.reason.contains("moving Mode0"))
        );
    }
    #[test]
    fn typed_source_scopes_choose_nearest_ancestor_without_basename_fallback() {
        let p = parse_program(r#"<Program><ControlSignalSources><ConstantModulation Name="Env" Value="0.9"/></ControlSignalSources><Layers><Layer><ControlSignalSources><ConstantModulation Name="Env" Value="0.6"/></ControlSignalSources><Keygroups><Keygroup><ControlSignalSources><ConstantModulation Name="Env" Value="0.2"/></ControlSignalSources><Oscillators><SamplePlayer SamplePath="a"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let player = p.sample_zones[0].player;
        for (path, value) in [
            ("$Program/Env", "0.9"),
            ("$Layer/Env", "0.6"),
            ("$Keygroup/Env", "0.2"),
        ] {
            let id = resolve_path(&p, player, path).unwrap();
            assert_eq!(p.nodes[id].attributes["Value"], value);
        }
        assert_eq!(
            resolve_path(&p, p.sample_zones[0].keygroup, "$Keygroup").unwrap(),
            p.sample_zones[0].keygroup
        );
        assert!(resolve_path(&p, player, "Env").is_err());
        assert!(resolve_path(&p, p.root, "$Keygroup/Env").is_err());
        assert!(resolve_path(&p, player, "$ProgramInvalid/Env").is_err());
    }
    #[test]
    fn layer_controls_change_only_started_siblings_in_the_issuing_layer() {
        let p = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a"/></Oscillators></Keygroup></Keygroups></Layer><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut source = sample(1);
        source.interleaved = Storage::from_f32(vec![0.25; 8]).unwrap();
        let mut renderer =
            Renderer::new(&p, HashMap::from([("a".into(), Arc::new(source))]), 48000).unwrap();
        let output = renderer
            .render(
                &[
                    script::Command {
                        frame: 0,
                        action: script::Action::Change {
                            id: 1,
                            gain: Some(0.1),
                            tune: Some(12.),
                            pan: None,
                            layer: Some(p.layers[0]),
                            relative: false,
                        },
                    },
                    script::Command {
                        frame: 0,
                        action: script::Action::Start(note(1)),
                    },
                    script::Command {
                        frame: 1,
                        action: script::Action::Change {
                            id: 1,
                            gain: Some(0.5),
                            tune: Some(12.),
                            pan: None,
                            layer: Some(p.layers[0]),
                            relative: false,
                        },
                    },
                ],
                &[],
                2,
            )
            .unwrap();
        assert_eq!(output, vec![[0.25, 0.25], [0.1875, 0.1875]]);
        assert_eq!(renderer.voices[0].note.tune, 12.);
        assert_eq!(renderer.voices[1].note.tune, 0.);
    }
    #[test]
    fn absolute_producer_writes_use_native_block_endpoint_before_audio() {
        let p = parse_program(r#"<Program><ControlSignalSources><ConstantModulation Name="Src" Value="0"/><ConstantModulation Name="Target" Value="0.8"><Connections><SignalConnection Source="$Program/Src" Destination="Value" Ratio="1" ConnectionMode="1" SignalConnectionVersion="1"/></Connections></ConstantModulation></ControlSignalSources><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let src = p
            .nodes
            .iter()
            .position(|node| node.name.as_deref() == Some("Src"))
            .unwrap();
        let target = p
            .nodes
            .iter()
            .position(|node| node.name.as_deref() == Some("Target"))
            .unwrap();
        let mut source = sample(1);
        source.interleaved = Storage::from_f32(vec![0.25; 8]).unwrap();
        let mut renderer =
            Renderer::new(&p, HashMap::from([("a".into(), Arc::new(source))]), 48000).unwrap();
        renderer
            .render(
                &[script::Command {
                    frame: 0,
                    action: script::Action::Start(note(1)),
                }],
                &[],
                256,
            )
            .unwrap();
        assert_eq!(renderer.parameters[target]["Value"], "0.8");
        for (block, expected) in [
            0.10663980990648,
            0.31733468174934,
            0.51793825626373,
            0.67547661066055,
        ]
        .into_iter()
        .enumerate()
        {
            let commands = if block == 0 {
                vec![host::Command {
                    frame: 256,
                    action: host::Action::Parameter {
                        node: src,
                        parameter: "Value".into(),
                        value: ParameterValue::Number(1.),
                    },
                }]
            } else {
                vec![]
            };
            let audio = renderer.render(&[], &commands, 256).unwrap();
            let value = renderer.parameters[target]["Value"].parse::<f64>().unwrap();
            assert!((value - expected).abs() < 8e-8, "{value} != {expected}");
            assert!(audio.iter().all(|frame| *frame == [0.125, 0.125]));
        }
    }
    #[test]
    fn relative_controls_apply_independently_to_each_live_duplicate_in_scope() {
        let p = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a"/></Oscillators></Keygroup></Keygroups></Layer><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut source = sample(1);
        source.interleaved = Storage::from_f32(vec![0.25; 8]).unwrap();
        let mut renderer =
            Renderer::new(&p, HashMap::from([("a".into(), Arc::new(source))]), 48000).unwrap();
        let mut first = note(1);
        first.volume = 0.25;
        first.pan = -0.5;
        let mut second = note(1);
        second.tune = 12.;
        second.pan = 0.5;
        renderer
            .render(
                &[
                    script::Command {
                        frame: 0,
                        action: script::Action::Start(first),
                    },
                    script::Command {
                        frame: 0,
                        action: script::Action::Start(second),
                    },
                ],
                &[],
                1,
            )
            .unwrap();
        renderer
            .apply_note(&script::Action::Change {
                id: 1,
                gain: Some(0.5),
                tune: Some(12.),
                pan: Some(0.25),
                layer: Some(p.layers[0]),
                relative: true,
            })
            .unwrap();
        let values = renderer
            .voices
            .iter()
            .map(|voice| (voice.note.volume, voice.note.tune, voice.note.pan))
            .collect::<Vec<_>>();
        assert_eq!(
            values,
            vec![
                (0.125, 12., -0.25),
                (0.25, 0., -0.5),
                (0.5, 24., 0.75),
                (1., 12., 0.5)
            ]
        );
        renderer
            .apply_note(&script::Action::Change {
                id: 1,
                gain: Some(0.5),
                tune: Some(12.),
                pan: Some(0.25),
                layer: None,
                relative: false,
            })
            .unwrap();
        assert!(renderer.voices.iter().all(|voice| (
            voice.note.volume,
            voice.note.tune,
            voice.note.pan
        ) == (0.5, 12., 0.25)));
    }
    #[test]
    fn layer_script_modulation_filters_source_ancestry_not_receiving_voice() {
        let p = parse_program(r#"<Program><ControlSignalSources><ScriptEventModulation Name="Shared" EventId="1" Bipolar="0"/></ControlSignalSources><Layers><Layer><ControlSignalSources><ScriptEventModulation Name="Local" EventId="1" Bipolar="0"/></ControlSignalSources><Keygroups><Keygroup><Connections><SignalConnection Source="$Layer/Local" Destination="Gain" Ratio="1"/></Connections><Oscillators><SamplePlayer SamplePath="a"/></Oscillators></Keygroup><Keygroup><Connections><SignalConnection Source="$Program/Shared" Destination="Gain" Ratio="1"/></Connections><Oscillators><SamplePlayer SamplePath="a"/></Oscillators></Keygroup></Keygroups></Layer><Layer><ControlSignalSources><ScriptEventModulation Name="Local" EventId="1" Bipolar="0"/></ControlSignalSources><Keygroups><Keygroup><Connections><SignalConnection Source="$Layer/Local" Destination="Gain" Ratio="1"/></Connections><Oscillators><SamplePlayer SamplePath="a"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        for voice in [None, Some(1)] {
            let mut source = sample(1);
            source.interleaved = Storage::from_f32(vec![0.25; 8]).unwrap();
            let mut renderer =
                Renderer::new(&p, HashMap::from([("a".into(), Arc::new(source))]), 48000).unwrap();
            let command = |frame, layer, target, voice| host::Command {
                frame,
                action: host::Action::ScriptModulation {
                    id: 1,
                    start: None,
                    target,
                    ramp_ms: 0.,
                    voice,
                    layer,
                },
            };
            assert_eq!(
                renderer
                    .render(
                        &[script::Command {
                            frame: 0,
                            action: script::Action::Start(note(1))
                        }],
                        &[command(0, Some(p.layers[0]), 0.5, voice)],
                        1
                    )
                    .unwrap(),
                vec![[0.0625, 0.0625]]
            );
            let settled = renderer
                .render(&[], &[command(1, None, 0.75, None)], 8191)
                .unwrap();
            assert!(
                settled
                    .last()
                    .unwrap()
                    .iter()
                    .all(|value| (*value - 0.28125).abs() < 1e-6)
            );
            let settled = renderer
                .render(
                    &[],
                    &[command(8192, Some(p.layers[1]), 0.25, Some(1))],
                    8192,
                )
                .unwrap();
            assert!(
                settled
                    .last()
                    .unwrap()
                    .iter()
                    .all(|value| (*value - 0.21875).abs() < 1e-6)
            );
        }
    }
    #[test]
    fn rendering_rejects_future_commands_without_mutation_and_preserves_partitions() {
        let p = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut source = sample(1);
        source.interleaved = Storage::from_f32(vec![0.25; 8]).unwrap();
        let resources = HashMap::from([("a".into(), Arc::new(source))]);
        let commands = [
            script::Command {
                frame: 0,
                action: script::Action::Start(note(1)),
            },
            script::Command {
                frame: 2,
                action: script::Action::Release(1),
            },
        ];
        let mut renderer = Renderer::new(&p, resources.clone(), 48000).unwrap();
        let parameter = host::Command {
            frame: 0,
            action: host::Action::Parameter {
                node: p.root,
                parameter: "Gain".into(),
                value: ParameterValue::Number(0.5),
            },
        };
        assert!(
            renderer
                .render(&commands, &[parameter], 2)
                .unwrap_err()
                .to_string()
                .contains("outside")
        );
        assert_eq!(renderer.frame, 0);
        assert!(renderer.voices.is_empty());
        assert_eq!(renderer.number(p.root, "Gain", 1.).unwrap(), 1.);
        assert!(
            renderer
                .render(
                    &[],
                    &[host::Command {
                        frame: 2,
                        action: host::Action::Parameter {
                            node: p.root,
                            parameter: "Gain".into(),
                            value: ParameterValue::Number(0.5)
                        }
                    }],
                    2
                )
                .is_err()
        );
        let mut partitioned = renderer.render(&commands[..1], &[], 2).unwrap();
        partitioned.extend(renderer.render(&commands[1..], &[], 1).unwrap());
        let mut whole = Renderer::new(&p, resources, 48000).unwrap();
        assert_eq!(partitioned, whole.render(&commands, &[], 3).unwrap());
        renderer.frame = u64::MAX;
        assert!(
            renderer
                .render(&[], &[], 1)
                .unwrap_err()
                .to_string()
                .contains("overflow")
        );
    }
    #[test]
    fn prepared_resources_keep_active_pcm_and_update_global_and_voice_impulse_maps() {
        let p = parse_program(r#"<Program><Inserts><Convolver Dry="1" Wet="0"/></Inserts><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a"/></Oscillators><Inserts><Convolver Dry="1" Wet="0"/></Inserts></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut source = sample(1);
        source.interleaved = Storage::from_f32(vec![0.25; 8]).unwrap();
        let a = Arc::new(source);
        let mut renderer =
            Renderer::new(&p, HashMap::from([("a".into(), Arc::clone(&a))]), 48000).unwrap();
        renderer
            .render(
                &[script::Command {
                    frame: 0,
                    action: script::Action::Start(note(1)),
                }],
                &[],
                1,
            )
            .unwrap();
        let mut prepared = sample(1);
        prepared.interleaved = Storage::from_f32(vec![0.5; 8]).unwrap();
        let b = Arc::new(prepared);
        renderer
            .install_prepared_samples(HashMap::from([
                ("b".into(), Arc::clone(&b)),
                ("alias_b".into(), Arc::clone(&b)),
            ]))
            .unwrap();
        assert!(Arc::ptr_eq(
            &renderer.samples["b"],
            &renderer.samples["alias_b"]
        ));
        renderer
            .apply_host(&host::Action::LoadResource {
                node: p.sample_zones[0].player,
                kind: host::ResourceKind::Sample,
                path: "b".into(),
            })
            .unwrap();
        for (node, _) in p
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.kind == "Convolver")
        {
            renderer
                .apply_host(&host::Action::LoadResource {
                    node,
                    kind: host::ResourceKind::Impulse,
                    path: "b".into(),
                })
                .unwrap();
        }
        assert_eq!(renderer.render(&[], &[], 1).unwrap(), vec![[0.125, 0.125]]);
        assert_eq!(
            renderer
                .render(
                    &[script::Command {
                        frame: 2,
                        action: script::Action::Start(note(2))
                    }],
                    &[],
                    1
                )
                .unwrap(),
            vec![[0.375, 0.375]]
        );
        assert!(
            renderer
                .install_prepared_samples(HashMap::from([("a".into(), Arc::clone(&b))]))
                .is_err()
        );
        assert!(Arc::ptr_eq(&renderer.samples["a"], &a));
        let mut invalid = sample(1);
        invalid.rate = 0;
        assert!(
            renderer
                .install_prepared_samples(HashMap::from([("invalid".into(), Arc::new(invalid))]))
                .is_err()
        );
        assert!(!renderer.samples.contains_key("invalid"));
        renderer
            .install_prepared_samples(HashMap::from([("a".into(), a)]))
            .unwrap();
    }
    #[test]
    fn wavetable_storage_width_does_not_expand_audio_source_layouts() {
        let p = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let table = Arc::new(sample(65));
        let resources = HashMap::from([
            ("a".into(), Arc::new(sample(1))),
            ("table".into(), Arc::clone(&table)),
        ]);
        let mut renderer = Renderer::new(&p, resources, 48000).unwrap();
        assert!(
            renderer
                .apply_host(&host::Action::LoadResource {
                    node: p.sample_zones[0].player,
                    kind: host::ResourceKind::Sample,
                    path: "table".into()
                })
                .is_err()
        );
        renderer
            .install_prepared_samples(HashMap::from([("table_alias".into(), table)]))
            .unwrap();
        assert_eq!(renderer.source_channels[&p.sample_zones[0].keygroup], 1);
    }
    #[test]
    fn ahd_and_multi_envelope_keep_proven_release_and_loop_metadata() {
        for (kind, attributes, steps) in [
            (
                "AHD",
                r#"AttackTime=".0011" HoldTime=".0013" DecayTime=".0017" AttackCurve="-.5" DecayCurve=".5""#,
                "",
            ),
            (
                "MultiEnvelope",
                r#"Retrigger="1" LoopStart="0" LoopEnd="0" ReleaseStep="3""#,
                r#"<Steps><Step Time=".1" DestLevel="1" Curve=".5"/><Step Time=".1" DestLevel=".25"/><Step Time=".1" DestLevel=".75"/><Step Time=".1" DestLevel="0"/></Steps>"#,
            ),
        ] {
            let xml = format!(
                r#"<Program><Layers><Layer><Keygroups><Keygroup><ControlSignalSources><{kind} Name="Env" {attributes}>{steps}</{kind}></ControlSignalSources><Connections><SignalConnection Source="$Keygroup/Env" Destination="Gain" Ratio="1"/></Connections><Oscillators><SamplePlayer SamplePath="a"><PlaybackOptions Stop="8" PlayRelease="1"><Loop Start="2" End="5"/></PlaybackOptions></SamplePlayer></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#
            );
            let p = parse_program(&xml).unwrap();
            let mut source = sample(1);
            source.interleaved = Storage::from_f32(vec![0.25; 8]).unwrap();
            let mut renderer =
                Renderer::new(&p, HashMap::from([("a".into(), Arc::new(source))]), 48000).unwrap();
            let output = renderer
                .render(
                    &[
                        script::Command {
                            frame: 0,
                            action: script::Action::Start(note(1)),
                        },
                        script::Command {
                            frame: 113,
                            action: script::Action::Release(1),
                        },
                    ],
                    &[],
                    5121,
                )
                .unwrap();
            if kind == "AHD" {
                assert!((f64::from(output[32][0]) - 0.125 * 0.8339776397).abs() < 1e-7);
                // NoteOff ignores the one-shot gate but splits its native control
                // segment; its post-off interpolation differs from a held note.
                assert!(output[128][0] > 0.1);
                assert!(output[256..].iter().all(|frame| *frame == [0., 0.]));
            } else {
                assert!((f64::from(output[113][0]) - 0.125 * 0.006635937839).abs() < 1e-8);
                assert!((f64::from(output[2513][0]) - 0.125 * 0.003317968221).abs() < 1e-8);
                let unknown =
                    parse_program(&xml.replace("Retrigger=\"1\"", "Retrigger=\"2\"")).unwrap();
                assert!(
                    preflight(&unknown)
                        .iter()
                        .any(|u| u.reason.contains("cleanup"))
                );
            }
            assert!(renderer.voices.is_empty());
        }
    }
    #[test]
    fn lfo_filter_frequency_uses_array_updates_without_scalar_rc() {
        let p=parse_program(r#"<Program><ControlSignalSources><LFO Name="Cutoff" Freq="11" Depth="0.1" Bipolar="1" Type="0"/></ControlSignalSources><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a"/></Oscillators><Inserts><XpanderFilter Freq="1000" Algorithm="1" Oversampling="0" DistortionType="2"><Connections><SignalConnection Source="$Program/Cutoff" Destination="Freq" Ratio="1"/></Connections></XpanderFilter></Inserts></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let filter_id = p
            .nodes
            .iter()
            .position(|node| node.kind == "XpanderFilter")
            .unwrap();
        let mut source = sample(1);
        source.interleaved = Storage::from_f32(vec![0.25; 8]).unwrap();
        let mut renderer =
            Renderer::new(&p, HashMap::from([("a".into(), Arc::new(source))]), 48000).unwrap();
        let output = renderer
            .render(
                &[script::Command {
                    frame: 0,
                    action: script::Action::Start(note(1)),
                }],
                &[],
                1024,
            )
            .unwrap();
        let graph = ModulationGraph::new(&p).unwrap();
        let mut reference = XpanderFilter::new(&p.nodes[filter_id], 1, 48000.).unwrap();
        let mut scalar = XpanderFilter::new(&p.nodes[filter_id], 1, 48000.).unwrap();
        let mut different = false;
        for (frame, actual) in output.iter().enumerate() {
            let input = Inputs {
                voice: Some(1),
                instance: Some(1),
                velocity: 100,
                time_seconds: frame as f64 / 48000.,
                voice_time_seconds: frame as f64 / 48000.,
                ..Default::default()
            };
            let values = graph
                .evaluate_nodes(&input, &HashMap::new(), &HashSet::from([filter_id]))
                .unwrap();
            let value = ParameterValue::Number(values[&(filter_id, "Freq".into())]);
            reference
                .set_effective_parameter("Freq", &value, true)
                .unwrap();
            scalar
                .set_effective_parameter("Freq", &value, false)
                .unwrap();
            let mut a = [0.; dsp::MAX_CHANNELS];
            a[0] = 0.25;
            let mut b = a;
            reference.process(std::slice::from_mut(&mut a)).unwrap();
            scalar.process(std::slice::from_mut(&mut b)).unwrap();
            assert_eq!(*actual, [a[0] * 0.5, a[0] * 0.5]);
            different |= (a[0] - b[0]).abs() > 1e-5;
        }
        assert!(different);
    }
    #[test]
    fn attack_decay_uses_planned_note_gate_and_rejects_unseen_partition_events() {
        let p=parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><ControlSignalSources><AttackDecayEnv Name="Env" Attack="0" DecayTime=".2"/></ControlSignalSources><Connections><SignalConnection Source="$Keygroup/Env" Destination="Gain" Ratio="1"/></Connections><Oscillators><SamplePlayer SamplePath="a"><PlaybackOptions Stop="8" PlayRelease="1"><Loop Start="2" End="5"/></PlaybackOptions></SamplePlayer></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut source = sample(1);
        source.interleaved = Storage::from_f32(vec![0.25; 8]).unwrap();
        let mut renderer =
            Renderer::new(&p, HashMap::from([("a".into(), Arc::new(source))]), 48000).unwrap();
        assert!(renderer.requires_planned_segments());
        assert!(
            renderer
                .render(&[], &[], 113)
                .unwrap_err()
                .to_string()
                .contains("aligned")
        );
        assert_eq!(renderer.frame, 0);
        assert!(
            renderer
                .render(
                    &[],
                    &[host::Command {
                        frame: 13,
                        action: host::Action::Parameter {
                            node: p.root,
                            parameter: "Gain".into(),
                            value: ParameterValue::Number(0.5)
                        }
                    }],
                    256
                )
                .unwrap_err()
                .to_string()
                .contains("host-only")
        );
        assert_eq!(renderer.number(p.root, "Gain", 1.).unwrap(), 1.);
        let output = renderer
            .render(
                &[
                    script::Command {
                        frame: 0,
                        action: script::Action::Start(note(1)),
                    },
                    script::Command {
                        frame: 113,
                        action: script::Action::Release(1),
                    },
                ],
                &[],
                512,
            )
            .unwrap();
        for (frame, expected) in [
            (96, 0.9742403030),
            (112, 0.9895552397),
            (113, 0.9905124307),
            (128, 0.9949254990),
            (145, 0.9999269843),
            (256, 0.9764893055),
        ] {
            assert!(
                (f64::from(output[frame][0]) - 0.125 * expected).abs() < 3e-8,
                "frame {frame}: {}",
                output[frame][0]
            );
        }
    }
    #[test]
    fn keygroup_scalar_pressure_gain_matches_native_control_points() {
        let p=parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Connections><SignalConnection Source="@ChanAfterTouch" Destination="Gain" Ratio="1"/></Connections><Oscillators><SamplePlayer SamplePath="a"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut source = sample(1);
        source.interleaved = Storage::from_f32(vec![0.25; 8]).unwrap();
        let mut renderer =
            Renderer::new(&p, HashMap::from([("a".into(), Arc::new(source))]), 48000).unwrap();
        let output = renderer
            .render(
                &[
                    script::Command {
                        frame: 0,
                        action: script::Action::Start(note(1)),
                    },
                    script::Command {
                        frame: 512,
                        action: script::Action::AfterTouch {
                            channel: 0,
                            value: 64,
                        },
                    },
                ],
                &[],
                8193,
            )
            .unwrap();
        for (frame, gain) in [(512, 0.), (1024, 0.3494858146), (8192, 0.503937006)] {
            let expected = 0.125 * gain;
            assert!(
                (f64::from(output[frame][0]) - expected).abs() < 1e-7,
                "frame {frame}: {:?}",
                output[frame]
            );
            assert_eq!(output[frame][0], output[frame][1]);
        }
        assert_eq!(renderer.current_frame(), 8193);
    }
    #[test]
    fn mono_phasor_matches_native_impulse_before_keygroup_pan() {
        let p=parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a"/></Oscillators><Inserts><Phasor MinFreq="1000" MaxFreq="1000" Feedback="0" Order="3"/></Inserts></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        assert!(preflight(&p).is_empty());
        let mut source = sample(1);
        source.frames = 12;
        source.loops.clear();
        let mut pulse = vec![0.; 12];
        pulse[0] = 0.25;
        source.interleaved = Storage::from_f32(pulse).unwrap();
        let mut renderer =
            Renderer::new(&p, HashMap::from([("a".into(), Arc::new(source))]), 48000).unwrap();
        let output = renderer
            .render(
                &[script::Command {
                    frame: 0,
                    action: script::Action::Start(note(1)),
                }],
                &[],
                12,
            )
            .unwrap();
        for (actual, native) in output.iter().zip([
            0.2601984143257141,
            -0.12562325596809387,
            -0.03202161192893982,
            0.014917660504579544,
            0.03294673189520836,
            0.03425484523177147,
            0.026977399364113808,
            0.01633840799331665,
            0.005507950205355883,
            -0.003760534105822444,
            -0.010662119835615158,
            -0.015001287683844566,
        ]) {
            assert!((f64::from(actual[0]) - native * 0.5).abs() < 1e-7);
            assert_eq!(actual[0], actual[1]);
        }
    }
    #[test]
    fn verified_legacy_program_path_keeps_other_parts_strict() {
        let p=parse_program(r#"<Program><ControlSignalSources><ConstantModulation Name="Src" Value=".25"/></ControlSignalSources><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let owner = p.sample_zones[0].player;
        assert_eq!(
            resolve_path(&p, owner, "/uvi/Part 0/Program/Src").unwrap(),
            resolve_path(&p, owner, "$Program/Src").unwrap()
        );
        for path in [
            "/uvi/Part 1/Program/Src",
            "/uvi/Part 0/Arbitrary/Src",
            "Src",
        ] {
            assert!(resolve_path(&p, owner, path).is_err());
        }
    }
    #[test]
    fn drunk_native_points_use_planned_array_gain_without_scalar_rc() {
        let p=parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><ControlSignalSources><Drunk Name="Src" InitialValue="0" Rate="100" Step="100" Bias="1" Bipolar="1" TriggerMode="1"/></ControlSignalSources><Connections><SignalConnection Source="$Keygroup/Src" Destination="Gain" Ratio="1"/></Connections><Oscillators><SamplePlayer SamplePath="a"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        assert!(preflight(&p).is_empty(), "{:?}", preflight(&p));
        let mut source = sample(1);
        source.interleaved = Storage::from_f32(vec![0.25; 8]).unwrap();
        let mut renderer =
            Renderer::new(&p, HashMap::from([("a".into(), Arc::new(source))]), 48000).unwrap();
        assert!(renderer.requires_planned_segments());
        let output = renderer
            .render(
                &[script::Command {
                    frame: 0,
                    action: script::Action::Start(note(1)),
                }],
                &[],
                1280,
            )
            .unwrap();
        for (frame, native) in [
            (0, 0.5),
            (32, 0.5022222399711609),
            (64, 0.506518542766571),
            (96, 0.5127506256103516),
            (128, 0.5207894444465637),
            (256, 0.5687206387519836),
            (512, 0.7169595956802368),
            (1024, 0.6624647378921509),
        ] {
            assert!(
                (f64::from(output[frame][0]) - 0.125 * native).abs() < 3e-8,
                "frame {frame}: {:?}",
                output[frame]
            );
        }
        let p=parse_program(r#"<Program><ControlSignalSources><StdRandom Name="Src" Rate="300" Depth=".7" Bipolar="1" TriggerMode="0"/></ControlSignalSources><Layers><Layer><Keygroups><Keygroup><Connections><SignalConnection Source="$Program/Src" Destination="Gain" Ratio="1"/></Connections><Oscillators><SamplePlayer SamplePath="a"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        assert!(preflight(&p).is_empty(), "{:?}", preflight(&p));
        let mut source = sample(1);
        source.interleaved = Storage::from_f32(vec![0.25; 8]).unwrap();
        let resources = HashMap::from([("a".into(), Arc::new(source))]);
        let mut split = Renderer::new(&p, resources.clone(), 48000).unwrap();
        assert_eq!(split.render(&[], &[], 256).unwrap(), vec![[0.; 2]; 256]);
        let commands = [script::Command {
            frame: 256,
            action: script::Action::Start(note(1)),
        }];
        let split = split.render(&commands, &[], 512).unwrap();
        let mut whole = Renderer::new(&p, resources, 48000).unwrap();
        let whole = whole.render(&commands, &[], 768).unwrap();
        assert_eq!(split, whole[256..]);
        assert!(split.windows(2).any(|frames| frames[0] != frames[1]));
    }
    #[test]
    fn delay_graph_mix_smooths_raw_target_before_clamping_audio_gain() {
        let p=parse_program(r#"<Program><Inserts><DualDelay DelayTime="0.125" Feedback="0" Mix="0.2"><Connections><SignalConnection Source="@MIDI CC 1" Destination="Mix" Ratio="-0.5"/></Connections></DualDelay></Inserts><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let delay = p
            .nodes
            .iter()
            .position(|node| node.kind == "DualDelay")
            .unwrap();
        let mut source = sample(2);
        source.interleaved = Storage::from_f32((0..8).flat_map(|_| [0.25, 0.]).collect()).unwrap();
        let mut renderer =
            Renderer::new(&p, HashMap::from([("a".into(), Arc::new(source))]), 48000).unwrap();
        let output = renderer
            .render(
                &[
                    script::Command {
                        frame: 0,
                        action: script::Action::Start(note(1)),
                    },
                    script::Command {
                        frame: 16384,
                        action: script::Action::Controller {
                            channel: 0,
                            controller: 1,
                            value: 127,
                        },
                    },
                ],
                &[],
                16900,
            )
            .unwrap();
        for (offset, native) in [
            (0, 0.2236067951),
            (31, 0.2236067951),
            (32, 0.2285310030),
            (64, 0.2330112010),
            (128, 0.2408284694),
            (512, 0.25),
        ] {
            assert!(
                (f64::from(output[16384 + offset][0]) - native).abs() < 3e-8,
                "offset{offset}: {:?}",
                output[16384 + offset]
            );
            assert_eq!(output[16384 + offset][1], 0.);
        }
        assert_eq!(
            numeric(&renderer.parameters, delay, "Mix", 0.).unwrap(),
            0.2
        );
        assert!(
            renderer
                .render(
                    &[],
                    &[host::Command {
                        frame: 16900,
                        action: host::Action::Parameter {
                            node: delay,
                            parameter: "Mix".into(),
                            value: ParameterValue::Number(-0.3)
                        }
                    }],
                    1
                )
                .is_err()
        );
        assert_eq!(renderer.current_frame(), 16900);
    }
    #[test]
    fn rooted_choke_discards_owned_delay_but_shared_tail_can_continue() {
        let root = script::HostRoot {
            epoch: 7,
            generation: 9,
            token: 1,
        };
        for shared in [false, true] {
            let delay = r#"<Inserts><TrackDelay DelayTime="0.001"/></Inserts>"#;
            let program=parse_program(&format!(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a" NoteTracking="0"/></Oscillators>{}</Keygroup></Keygroups>{}</Layer></Layers></Program>"#,
                if shared{""}else{delay},if shared{delay}else{""})).unwrap();
            let mut renderer = Renderer::new(
                &program,
                HashMap::from([("a".into(), Arc::new(sample(1)))]),
                48000,
            )
            .unwrap();
            renderer
                .render_with_roots(
                    &[script::Command {
                        frame: 0,
                        action: script::Action::Start(note(7)),
                    }],
                    &[Some(root)],
                    &[],
                    8,
                )
                .unwrap();
            let silent = renderer
                .render_with_roots(
                    &[script::Command {
                        frame: 8,
                        action: script::Action::ChokeRoot,
                    }],
                    &[Some(root)],
                    &[],
                    8,
                )
                .unwrap();
            assert!(silent.iter().all(|frame| *frame == [0., 0.]));
            assert!(renderer.sounding_roots().is_empty());
            let tail = renderer.render_with_roots(&[], &[], &[], 64).unwrap();
            assert_eq!(tail.iter().any(|frame| frame[0].abs() > 0.), shared);
        }
    }
    #[test]
    fn rooted_choke_stops_same_id_siblings_only_at_ordered_frame() {
        let program=parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a" NoteTracking="0"/></Oscillators></Keygroup><Keygroup><Oscillators><SamplePlayer SamplePath="a" NoteTracking="0"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let samples = HashMap::from([("a".into(), Arc::new(sample(1)))]);
        let mut rooted = Renderer::new(&program, samples.clone(), 48000).unwrap();
        let mut survivor = Renderer::new(&program, samples, 48000).unwrap();
        let root = |token| script::HostRoot {
            epoch: 7,
            generation: 9,
            token,
        };
        let starts = [
            script::Command {
                frame: 0,
                action: script::Action::Start(note(7)),
            },
            script::Command {
                frame: 0,
                action: script::Action::Start(note(7)),
            },
            script::Command {
                frame: 1,
                action: script::Action::ChokeRoot,
            },
        ];
        let output = rooted
            .render_with_roots(
                &starts,
                &[Some(root(1)), Some(root(2)), Some(root(1))],
                &[],
                3,
            )
            .unwrap();
        let expected = survivor.render(&starts[..1], &[], 3).unwrap();
        assert_eq!(output[0], [expected[0][0] * 2., expected[0][1] * 2.]);
        assert_eq!(output[1..], expected[1..]);
        assert_eq!(rooted.sounding_roots(), [root(2)]);
        assert_eq!(rooted.active_voices(), 2);
        // Root metadata is essential; never fall back to an opaque-ID stop.
        assert!(
            rooted
                .render_with_roots(
                    &[script::Command {
                        frame: 3,
                        action: script::Action::ChokeRoot
                    }],
                    &[None],
                    &[],
                    1
                )
                .is_err()
        );
    }
    #[test]
    fn backend_launch_ancestry_preserves_fifo_and_legacy_pcm() {
        let program = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="a" NoteTracking="0"/></Oscillators></Keygroup><Keygroup><Oscillators><SamplePlayer SamplePath="a" NoteTracking="0"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let samples = HashMap::from([("a".into(), Arc::new(sample(1)))]);
        let mut rooted = Renderer::new(&program, samples.clone(), 48000).unwrap();
        let mut legacy = Renderer::new(&program, samples, 48000).unwrap();
        let root = |token| script::HostRoot {
            epoch: 7,
            generation: 9,
            token,
        };
        let starts = [
            script::Command {
                frame: 0,
                action: script::Action::Start(note(7)),
            },
            script::Command {
                frame: 0,
                action: script::Action::Start(note(7)),
            },
        ];
        let audio = rooted
            .render_with_roots(&starts, &[Some(root(1)), Some(root(2))], &[], 1)
            .unwrap();
        assert_eq!(audio, legacy.render(&starts, &[], 1).unwrap());
        assert_eq!(audio, [[2., 2.]]);
        assert_eq!(rooted.sounding_roots(), [root(1), root(2)]);
        let off = [script::Command {
            frame: 1,
            action: script::Action::ReleaseNote {
                id: 7,
                note: 60,
                channel: 0,
                layer: None,
            },
        }];
        // A release's ancestry is advisory: actual terminal matching stays FIFO,
        // and all keygroup siblings of the oldest launch retire together.
        let audio = rooted
            .render_with_roots(&off, &[Some(root(2))], &[], 1)
            .unwrap();
        assert_eq!(audio, legacy.render(&off, &[], 1).unwrap());
        assert_eq!(audio, [[2., 2.]]);
        assert_eq!(rooted.sounding_roots(), [root(2)]);
        let frame = rooted.current_frame();
        assert!(
            rooted
                .render_with_roots(&[], &[Some(root(1))], &[], 1)
                .is_err()
        );
        assert_eq!(rooted.current_frame(), frame);
    }
}
