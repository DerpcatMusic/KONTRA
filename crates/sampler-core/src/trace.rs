//! Optional signal measurements. Graph construction, draining and file I/O are
//! control-side; the single render writer uses fixed buffers and a bounded SPSC.
use crate::dsp::{BLOCK, ControlRamp, Planar, PreparedProcessor};
use crate::{EngineParameterAddress, EngineParameterLaw, Error, Frame, Prepared};
use rtrb::{Consumer, Producer, RingBuffer};
use serde::Serialize;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

#[derive(Clone, Debug, Serialize)]
pub struct TraceParameter {
    pub name: &'static str,
    pub initial: f64,
    pub address: Option<EngineParameterAddress>,
    pub law: Option<EngineParameterLaw>,
    #[serde(skip)]
    pub(crate) lane: Option<usize>,
    #[serde(skip)]
    native_range: Option<[f64; 3]>,
}
#[derive(Clone, Debug, Serialize)]
pub struct TraceNode {
    pub id: usize,
    pub kind: &'static str,
    pub processor: &'static str,
    pub zone: Option<u32>,
    pub group: Option<u32>,
    pub bus: Option<usize>,
    pub latency_samples: u32,
    pub gain_measurement: &'static str,
    pub parameters: Vec<TraceParameter>,
}
#[derive(Clone, Debug, Serialize)]
pub struct TraceEdge {
    pub from: usize,
    pub to: usize,
    pub kind: &'static str,
}
#[derive(Clone, Debug, Default)]
pub(crate) struct VoiceNodes {
    pub source: usize,
    pub amp: usize,
    pub output: usize,
    pub region: usize,
    pub pre: Vec<usize>,
    pub post: Vec<usize>,
    pub taps: Vec<usize>,
}
#[derive(Clone, Debug, Default)]
pub(crate) struct BusNodes {
    pub input: usize,
    pub tone: Option<usize>,
    pub output: usize,
    pub stages: Vec<usize>,
    pub sends: Vec<usize>,
}
#[derive(Debug, Serialize)]
pub struct TraceGraph {
    pub sample_rate: u32,
    pub nodes: Vec<TraceNode>,
    pub edges: Vec<TraceEdge>,
    pub order: Vec<usize>,
    #[serde(skip)]
    pub(crate) voices: std::collections::BTreeMap<u32, VoiceNodes>,
    #[serde(skip)]
    pub(crate) buses: Vec<BusNodes>,
    #[serde(skip)]
    pub(crate) master: usize,
    #[serde(skip)]
    pub(crate) host: Vec<usize>,
}
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct TraceIdentity {
    pub zone: u32,
    pub sample: usize,
    pub family: usize,
    pub generation: u64,
    pub ratio: f64,
    pub source_frame: u64,
    pub sample_start: u64,
    pub velocity: f64,
    pub key: u8,
    pub rr_sequence: Option<usize>,
    pub rr_take: Option<u32>,
    pub group: Option<u32>,
    pub layer: Option<usize>,
    pub external_port: Option<usize>,
    pub output_channels: Option<u8>,
    pub routed_to: Option<usize>,
    pub cc1: u8,
    pub cc7: u8,
    pub cc11: u8,
    pub voice_gain: f64,
    pub region_gain: f64,
    pub velocity_gain: f64,
    pub xfade_weight: f64,
    pub script_gain: [f64; 2],
    pub note_gain: [f64; 2],
    pub amplifier_control_gain: [f64; 2],
    pub envelope_level: f64,
    pub source_metrics: TraceMetrics,
}
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct TraceMetrics {
    pub peak: [f64; 2],
    pub rms: [f64; 2],
    pub dc: [f64; 2],
}
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct TraceRecord {
    pub node: usize,
    pub at: u64,
    pub frames: usize,
    pub input: TraceMetrics,
    pub output: TraceMetrics,
    /// Scalar/channel multiplier where one exists; signal RMS delta is separate.
    pub gain: [f64; 2],
    pub enabled: bool,
    pub bypassed: bool,
    pub latency_samples: u32,
    pub identity: TraceIdentity,
    pub contributors: u32,
    /// One voice at the zone→layer boundary; false denotes the coherent node sum.
    pub contribution: bool,
    pub values: [f64; 16],
    pub normalized: [Option<i32>; 16],
}
struct Scratch {
    input: Planar,
    output: Planar,
    record: TraceRecord,
    used: bool,
}
impl Default for Scratch {
    fn default() -> Self {
        Self {
            input: [[0.; BLOCK]; 2],
            output: [[0.; BLOCK]; 2],
            record: TraceRecord::default(),
            used: false,
        }
    }
}

pub(crate) struct TracePrepared {
    pub graph: Arc<TraceGraph>,
    writer: Mutex<Option<Producer<TraceRecord>>>,
    reader: TraceReader,
}
#[derive(Clone)]
pub struct TraceReader {
    pub graph: Arc<TraceGraph>,
    consumer: Arc<Mutex<Consumer<TraceRecord>>>,
    dropped: Arc<AtomicU64>,
}
impl TraceReader {
    pub(crate) fn abandoned(&self) -> bool {
        self.consumer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_abandoned()
    }
    /// Non-RT only. No library names, scripts, paths or PCM exist in these rows.
    pub fn drain(&self) -> Vec<TraceRecord> {
        let mut reader = self
            .consumer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut rows = Vec::new();
        while let Ok(row) = reader.pop() {
            rows.push(row);
        }
        rows
    }
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}
pub(crate) struct Recorder {
    writer: Mutex<Producer<TraceRecord>>,
    dropped: Arc<AtomicU64>,
    scratch: Box<[Scratch]>,
    touched: Vec<usize>,
    at: u64,
    frames: usize,
}
impl TracePrepared {
    pub(crate) fn new(graph: TraceGraph, capacity: usize) -> Result<Self, Error> {
        if capacity == 0 {
            return Err(Error::InvalidInput);
        }
        let graph = Arc::new(graph);
        let (writer, consumer) = RingBuffer::new(capacity);
        let reader = TraceReader {
            graph: graph.clone(),
            consumer: Arc::new(Mutex::new(consumer)),
            dropped: Arc::new(AtomicU64::new(0)),
        };
        Ok(Self {
            graph,
            writer: Mutex::new(Some(writer)),
            reader,
        })
    }
    pub(crate) fn recorder(&self) -> Result<Recorder, Error> {
        let writer = self
            .writer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .ok_or(Error::InvalidInput)?;
        Ok(Recorder {
            writer: Mutex::new(writer),
            dropped: self.reader.dropped.clone(),
            scratch: std::iter::repeat_with(Scratch::default)
                .take(self.graph.nodes.len())
                .collect(),
            touched: Vec::with_capacity(self.graph.nodes.len()),
            at: 0,
            frames: 0,
        })
    }
}
impl Prepared {
    pub fn signal_trace_reader(&self) -> Option<TraceReader> {
        self.signal_trace.as_ref().map(|t| t.reader.clone())
    }
}
impl Recorder {
    pub(crate) fn begin(&mut self, at: u64, frames: usize) {
        self.at = at;
        self.frames = frames;
    }
    pub(crate) fn record(
        &mut self,
        id: usize,
        input: &Planar,
        output: &Planar,
        len: usize,
        gain: [f64; 2],
        enabled: bool,
        identity: TraceIdentity,
        parameters: &[ControlRamp],
        node: &TraceNode,
    ) {
        let cell = &mut self.scratch[id];
        if !cell.used {
            cell.used = true;
            self.touched.push(id);
        }
        for c in 0..2 {
            for i in 0..len {
                cell.input[c][i] += input[c][i];
                cell.output[c][i] += output[c][i];
            }
        }
        cell.record = TraceRecord {
            node: id,
            at: self.at,
            frames: self.frames,
            gain,
            enabled,
            bypassed: !enabled,
            latency_samples: node.latency_samples,
            identity,
            contributors: cell.record.contributors + 1,
            values: std::array::from_fn(|i| {
                node.parameters.get(i).map_or(0., |p| {
                    p.lane.map_or(p.initial, |n| parameters[n].value(self.at))
                })
            }),
            normalized: std::array::from_fn(|i| {
                node.parameters.get(i).and_then(|p| {
                    p.native(p.lane.map_or(p.initial, |n| parameters[n].value(self.at)))
                })
            }),
            ..Default::default()
        };
        if node.kind == "voice_output" {
            let row = TraceRecord {
                input: metrics(input, len),
                output: metrics(output, len),
                frames: len,
                contributors: 1,
                contribution: true,
                ..cell.record
            };
            if self
                .writer
                .get_mut()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(row)
                .is_err()
            {
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    pub(crate) fn end(&mut self) {
        for &id in &self.touched {
            let s = &mut self.scratch[id];
            s.record.input = metrics(&s.input, self.frames);
            s.record.output = metrics(&s.output, self.frames);
            if self
                .writer
                .get_mut()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(s.record)
                .is_err()
            {
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
            *s = Scratch::default();
        }
        self.touched.clear();
    }
}
pub(crate) fn metrics(block: &Planar, len: usize) -> TraceMetrics {
    if len == 0 {
        return TraceMetrics::default();
    }
    let mut result = TraceMetrics::default();
    for c in 0..2 {
        for &v in &block[c][..len] {
            result.peak[c] = result.peak[c].max(v.abs());
            result.rms[c] += v * v;
            result.dc[c] += v;
        }
        result.rms[c] = (result.rms[c] / len as f64).sqrt();
        result.dc[c] /= len as f64;
    }
    result
}
pub(crate) fn planar(frames: &[Frame]) -> Planar {
    let mut block = [[0.; BLOCK]; 2];
    for (i, frame) in frames.iter().enumerate() {
        for c in 0..2 {
            block[c][i] = f64::from(frame[c]);
        }
    }
    block
}
pub(crate) struct Section<'a> {
    pub recorder: &'a mut Recorder,
    pub graph: &'a TraceGraph,
    pub nodes: &'a [usize],
    pub identity: TraceIdentity,
}
impl Section<'_> {
    pub fn record(
        &mut self,
        index: usize,
        input: &Planar,
        output: &Planar,
        len: usize,
        gain: [f64; 2],
        enabled: bool,
        parameters: &[ControlRamp],
    ) {
        let id = self.nodes[index];
        self.recorder.record(
            id,
            input,
            output,
            len,
            gain,
            enabled,
            self.identity,
            parameters,
            &self.graph.nodes[id],
        );
    }
}
pub(crate) struct VoiceTrace<'a> {
    pub recorder: &'a mut Recorder,
    pub graph: &'a TraceGraph,
    pub nodes: &'a VoiceNodes,
    pub identity: TraceIdentity,
    pub envelope_parameters: [f64; 10],
}
impl VoiceTrace<'_> {
    pub fn record(
        &mut self,
        id: usize,
        input: &Planar,
        output: &Planar,
        len: usize,
        gain: [f64; 2],
        parameters: &[ControlRamp],
    ) {
        let enabled = self.graph.nodes[id]
            .parameters
            .iter()
            .position(|p| p.name == "bypass")
            .is_none_or(|i| {
                let p = &self.graph.nodes[id].parameters[i];
                p.lane
                    .map_or(p.initial, |n| parameters[n].value(self.recorder.at))
                    < 1.
            });
        self.recorder.record(
            id,
            input,
            output,
            len,
            gain,
            enabled,
            self.identity,
            parameters,
            &self.graph.nodes[id],
        );
        if id == self.nodes.amp {
            let record = &mut self.recorder.scratch[id].record;
            for (i, value) in self.envelope_parameters.into_iter().enumerate() {
                record.values[i + 2] = value;
                record.normalized[i + 2] = self.graph.nodes[id].parameters[i + 2].native(value);
            }
        }
    }
}

impl TraceParameter {
    fn native(&self, value: f64) -> Option<i32> {
        let value = self.native_range.map_or(value, |[low, high, max]| {
            if high == low {
                0.
            } else {
                (value - low) / (high - low) * max
            }
        });
        if self
            .address
            .and_then(|a| crate::engine_parameter_name(a.parameter))
            .is_some_and(|n| n.ends_with("BYPASS"))
        {
            Some(value.round() as i32)
        } else {
            self.law.map(|l| l.encode(value))
        }
    }

    pub(crate) fn envelope(name: &'static str, initial: f64, binding: Option<&crate::EngineParameterBinding>) -> Self {
        let mut p = Self::constant(name, initial);
        if let Some(b) = binding { p.address = Some(b.address); p.law = Some(b.law); }
        p
    }
    pub(crate) fn constant(name: &'static str, initial: f64) -> Self {
        Self {
            name,
            initial,
            lane: None,
            native_range: None,
            address: None,
            law: None,
        }
    }
}

impl TraceGraph {
    pub(crate) fn new(rate: u32) -> Self {
        let mut graph = Self {
            sample_rate: rate,
            nodes: Vec::new(),
            edges: Vec::new(),
            order: Vec::new(),
            voices: Default::default(),
            buses: Vec::new(),
            master: 0,
            host: Vec::new(),
        };
        graph.master = graph.node("master", "sum", None, None, None, Vec::new(), 0);
        let part = graph.node(
            "host_part_fader",
            "fader_pan",
            None,
            None,
            None,
            vec![
                TraceParameter::constant("left_gain", 1.),
                TraceParameter::constant("right_gain", 1.),
            ],
            0,
        );
        graph.host.push(part);
        graph.edge(graph.master, part, "host_part");
        for port in 0..HOST_PORTS {
            let id = graph.node(
                "host_rack_bus",
                "sum_fader_pan",
                None,
                None,
                Some(port),
                vec![],
                0,
            );
            graph.host.push(id);
            graph.edge(part, id, "possible_host_route");
        }
        for port in 0..HOST_PORTS {
            let id = graph.node(
                "host_master",
                "master_gain",
                None,
                None,
                Some(port),
                vec![],
                0,
            );
            graph.host.push(id);
            graph.edge(graph.host[1 + port], id, "host_master");
        }

        let aux = graph.node("host_aux_send", "send_gain", None, None, None, vec![], 0);
        graph.host.push(aux);
        graph.edge(part, aux, "aux_send");
        for port in 0..HOST_PORTS {
            graph.edge(aux, graph.host[1 + port], "possible_host_route");
        }
        for port in 0..HOST_PORTS {
            let output = graph.node("host_output", "physical_channel_sum", None, None, Some(port), vec![], 0);
            graph.host.push(output);
            for rack in 0..HOST_PORTS {
                graph.edge(graph.host[1 + HOST_PORTS + rack], output, "possible_physical_route");
            }
        }

        for &id in &graph.host {
            graph.nodes[id].parameters = vec![
                TraceParameter::constant("left_gain", 1.),
                TraceParameter::constant("right_gain", 1.),
            ];
        }
        graph
    }
    pub(crate) fn node(
        &mut self,
        kind: &'static str,
        processor: &'static str,
        zone: Option<u32>,
        group: Option<u32>,
        bus: Option<usize>,
        parameters: Vec<TraceParameter>,
        latency: u32,
    ) -> usize {
        let id = self.nodes.len();
        self.nodes.push(TraceNode {
            id,
            kind,
            processor,
            zone,
            group,
            bus,
            parameters,
            latency_samples: latency,
            gain_measurement: "scalar_multiplier",
        });
        id
    }
    pub(crate) fn edge(&mut self, from: usize, to: usize, kind: &'static str) {
        self.edges.push(TraceEdge { from, to, kind });
    }
    pub(crate) fn topological_order(&self) -> Vec<usize> {
        let mut degree = vec![0usize; self.nodes.len()];
        let mut children = vec![Vec::new(); self.nodes.len()];
        for e in &self.edges {
            degree[e.to] += 1;
            children[e.from].push(e.to);
        }
        let mut ready: std::collections::VecDeque<_> = degree
            .iter()
            .enumerate()
            .filter_map(|(n, d)| (*d == 0).then_some(n))
            .collect();
        let mut order = Vec::with_capacity(self.nodes.len());
        while let Some(n) = ready.pop_front() {
            order.push(n);
            for &child in &children[n] {
                degree[child] -= 1;
                if degree[child] == 0 {
                    ready.push_back(child);
                }
            }
        }
        order
    }
    pub(crate) fn connect(
        &mut self,
        stages: &[PreparedProcessor],
        ids: &[usize],
        mut parent: usize,
    ) -> usize {
        let mut n = 0;
        let mut fork = None;
        let mut accumulator = None;
        while n < stages.len() {
            let id = ids[n];
            match stages[n] {
                PreparedProcessor::Mix { count, .. } => {
                    let end = n + 1 + usize::from(count);
                    let wet = self.connect(&stages[n + 1..end], &ids[n + 1..end], parent);
                    self.edge(parent, id, "dry_or_bypass");
                    self.edge(wet, id, "wet");
                    parent = id;
                    n = end;
                }
                PreparedProcessor::Branch {
                    count, first, last, ..
                } => {
                    if first || fork.is_none() {
                        fork = Some(parent);
                    }
                    let end = n + 1 + usize::from(count);
                    let child = self.connect(&stages[n + 1..end], &ids[n + 1..end], fork.unwrap());
                    self.edge(child, id, "branch_sum");
                    if let Some(previous) = accumulator {
                        self.edge(previous, id, "accumulate");
                    }
                    accumulator = Some(id);
                    if last {
                        parent = id;
                        fork = None;
                        accumulator = None;
                    }
                    n = end;
                }
                _ => {
                    self.edge(parent, id, "serial");
                    parent = id;
                    n += 1;
                }
            }
        }
        parent
    }
    pub(crate) fn parameter(
        &self,
        name: &'static str,
        p: crate::dsp::control::PreparedParameter,
        plan: &Prepared,
        bindings: &[crate::ControlRange],
        initial: &[ControlRamp],
    ) -> TraceParameter {
        use crate::dsp::control::PreparedParameter;
        match p {
            PreparedParameter::Constant(value) => TraceParameter {
                name,
                initial: value,
                lane: None,
                native_range: None,
                address: None,
                law: None,
            },
            PreparedParameter::Expression { low, high: _, .. } => TraceParameter {
                name,
                initial: low,
                lane: None,
                native_range: None,
                address: None,
                law: None,
            },
            PreparedParameter::Control(lane) => {
                let control = bindings[lane].control;
                let native = plan.engine_parameters.iter().find(|b| b.control == control);
                let slot = crate::is_slot_control(control);
                let physical_slot =
                    slot && (control.0 >> 32) as u32 as i32 != crate::BUS_VOLUME_SLOT;
                let address = native.map(|b| b.address).or_else(|| {
                    physical_slot.then(|| EngineParameterAddress {
                        parameter: crate::engine_parameter_id(match (control.0 >> 96) as u8 {
                            0 => {
                                if control.0 as u32 as i32 == 0 {
                                    "ENGINE_PAR_SEND_EFFECT_BYPASS"
                                } else {
                                    "ENGINE_PAR_EFFECT_BYPASS"
                                }
                            }
                            1 => {
                                if control.0 as u32 as i32 == 0 {
                                    "ENGINE_PAR_SEND_EFFECT_OUTPUT_GAIN"
                                } else {
                                    "ENGINE_PAR_INSERT_EFFECT_OUTPUT_GAIN"
                                }
                            }
                            _ => "ENGINE_PAR_SEND_EFFECT_DRY_LEVEL",
                        })
                        .unwrap(),
                        group: (control.0 >> 64) as u32 as i32,
                        slot: (control.0 >> 32) as u32 as i32,
                        generic: control.0 as u32 as i32,
                    })
                });
                let law = native.map(|b| b.law).or_else(|| {
                    physical_slot.then(|| {
                        if (control.0 >> 96) as u8 == 0 {
                            EngineParameterLaw::Linear { low: 0., high: 1. }
                        } else {
                            EngineParameterLaw::CubicGain { unity: 396851. }
                        }
                    })
                });
                TraceParameter {
                    name,
                    initial: initial[lane].value(0),
                    lane: Some(lane),
                    address,
                    law,
                    native_range: slot.then(|| {
                        [
                            bindings[lane].low,
                            bindings[lane].high,
                            if (control.0 >> 96) as u8 == 0 {
                                1.
                            } else {
                                16.
                            },
                        ]
                    }),
                }
            }
        }
    }
    pub(crate) fn stage(
        &mut self,
        stage: &PreparedProcessor,
        kind: &'static str,
        zone: Option<u32>,
        group: Option<u32>,
        bus: Option<usize>,
        plan: &Prepared,
        bindings: &[crate::ControlRange],
        initial: &[ControlRamp],
    ) -> usize {
        use crate::dsp::control::PreparedParameter;
        let parameter = |name, p| self.parameter(name, p, plan, bindings, initial);
        let constant = |name, value| parameter(name, PreparedParameter::Constant(value));
        let (name, parameters, latency) = match stage {
            PreparedProcessor::Gain(g) => ("gain", vec![constant("linear_gain", *g)], 0),
            PreparedProcessor::ControlGain(lane) => (
                "control_gain",
                vec![parameter("linear_gain", PreparedParameter::Control(*lane))],
                0,
            ),
            PreparedProcessor::Mix { lanes, .. } => (
                "slot_mix",
                ["dry", "output_gain", "bypass"]
                    .into_iter()
                    .zip(lanes)
                    .map(|(name, lane)| parameter(name, PreparedParameter::Control(*lane)))
                    .collect(),
                0,
            ),
            PreparedProcessor::Gainer { gain, dry, .. } => (
                "gainer",
                vec![parameter("gain", *gain), constant("dry", *dry)],
                0,
            ),
            PreparedProcessor::StereoMatrix(m) => (
                "stereo_matrix",
                vec![
                    constant("ll", m[0][0]),
                    constant("lr", m[0][1]),
                    constant("rl", m[1][0]),
                    constant("rr", m[1][1]),
                ],
                0,
            ),
            PreparedProcessor::Compressor(c) => (
                "compressor",
                c.trace_parameters()
                    .into_iter()
                    .map(|(n, v)| parameter(n, v))
                    .collect(),
                0,
            ),
            PreparedProcessor::Branch { gain, .. } => ("branch", vec![constant("gain", *gain)], 0),
            PreparedProcessor::Delay { delay, .. } => (
                "delay",
                delay
                    .trace_parameters()
                    .into_iter()
                    .map(|(n, v)| constant(n, v))
                    .collect(),
                delay.trace_frames(),
            ),
            PreparedProcessor::PeakingEq(eq) => (
                "v1_peaking_eq", eq.trace_parameters().into_iter().map(|(n, p)| parameter(n, p)).collect(), 0,
            ),
            PreparedProcessor::Biquad(b) => (
                "biquad",
                ["b0", "b1", "b2", "a1", "a2"]
                    .into_iter()
                    .zip(b.trace_coefficients())
                    .map(|(n, v)| constant(n, v))
                    .collect(),
                0,
            ),
            PreparedProcessor::StateVariable(i) => (
                "state_variable_filter",
                if bus.is_some() && zone.is_none() {
                    plan.buses.trace_filter(*i)
                } else {
                    plan.filters[*i].trace_parameters()
                }
                .into_iter()
                .map(|(n, p)| parameter(n, p))
                .collect(),
                0,
            ),
            PreparedProcessor::Daft(d) => (
                "daft",
                d.trace_parameters()
                    .into_iter()
                    .map(|(n, p)| parameter(n, p))
                    .collect(),
                0,
            ),
            PreparedProcessor::LadderLP4 { ladder, .. } => (
                "ladder_lp4",
                ladder
                    .trace_parameters()
                    .into_iter()
                    .map(|(n, p)| parameter(n, p))
                    .collect(),
                0,
            ),
            PreparedProcessor::StereoModeller { stereo, .. } => (
                "stereo_modeller",
                stereo
                    .trace_parameters()
                    .into_iter()
                    .map(|(n, p)| parameter(n, p))
                    .collect(),
                0,
            ),
            PreparedProcessor::Decimate(d) => (
                "decimator",
                vec![constant("period", d.period), constant("blend", d.blend)],
                0,
            ),
            PreparedProcessor::LoFi(d) => ("lofi", d.trace_parameters().into_iter()
                .map(|(n, v)| constant(n, v)).collect(), 0),
            PreparedProcessor::Rectify(_) => ("rectifier", vec![], 0),
            PreparedProcessor::Reverb(i) => (
                "reverb",
                plan.buses
                    .trace_reverb(*i)
                    .into_iter()
                    .map(|(n, v)| constant(n, v))
                    .collect(),
                0,
            ),
            PreparedProcessor::Convolution(i) => (
                "convolution",
                plan.buses
                    .trace_convolution(*i)
                    .into_iter()
                    .map(|(n, v)| constant(n, v))
                    .collect(),
                0,
            ),
        };
        let id = self.node(kind, name, zone, group, bus, parameters, latency);
        self.nodes[id].gain_measurement = match stage {
            PreparedProcessor::Gain(_) | PreparedProcessor::ControlGain(_) | PreparedProcessor::Branch { .. } => "scalar_multiplier",
            PreparedProcessor::Mix { .. } => "mix_coefficients",
            _ => "effective_energy_ratio",
        };
        id
    }
}

/// Numeric mixer stages supplied by a host adapter after the instrument runtime.
#[derive(Clone, Copy)]
pub enum HostStage {
    PartFader,
    AuxSend,
    Output(usize, u8),
    RackBus(usize),
    Master(usize),
}
pub const HOST_PORTS: usize = 16;
impl crate::Runtime {
    pub fn signal_trace_enabled(&self) -> bool {
        self.signal_trace
    }
    /// Adapter hook. The inputs are observed only while tracing; no allocation,
    /// locking or file work occurs here. `master` supports per-frame host gain.
    pub fn trace_host_frames(
        &mut self,
        stage: HostStage,
        frames: &[Frame],
        gain: [f32; 2],
        enabled: bool,
        port: usize,
    ) {
        if !self.signal_trace {
            return;
        }
        for (n, chunk) in frames.chunks(BLOCK).enumerate() {
            let input = planar(chunk);
            self.trace_host_block(
                stage,
                &input,
                chunk.len(),
                gain,
                None,
                enabled,
                port,
                self.now.saturating_sub(frames.len() as u64) + (n * BLOCK) as u64,
            );
        }
    }
    pub fn trace_host_planar(
        &mut self,
        stage: HostStage,
        left: &[f32],
        right: &[f32],
        gain: [f32; 2],
        master: Option<&[f32]>,
        enabled: bool,
        port: usize,
    ) {
        if !self.signal_trace {
            return;
        }
        let len = left.len().min(right.len());
        if master.is_some_and(|m| m.len() < len) {
            return;
        }
        for first in (0..len).step_by(BLOCK) {
            let count = BLOCK.min(len - first);
            let mut input = [[0.; BLOCK]; 2];
            for i in 0..count {
                input[0][i] = f64::from(left[first + i]);
                input[1][i] = f64::from(right[first + i]);
            }
            self.trace_host_block(
                stage,
                &input,
                count,
                gain,
                master.map(|m| &m[first..first + count]),
                enabled,
                port,
                self.now.saturating_sub(len as u64) + first as u64,
            );
        }
    }
    fn trace_host_block(
        &mut self,
        stage: HostStage,
        input: &Planar,
        len: usize,
        gain: [f32; 2],
        master: Option<&[f32]>,
        enabled: bool,
        port: usize,
        at: u64,
    ) {
        let Some(plan) = self.plans.get_mut(self.active_plan.0) else {
            return;
        };
        let Some((trace, recorder)) = plan
            .prepared
            .signal_trace
            .as_ref()
            .zip(plan.dsp.trace.as_mut())
        else {
            return;
        };
        let id = match stage {
            HostStage::PartFader => trace.graph.host[0],
            HostStage::AuxSend => trace.graph.host[1 + 2 * HOST_PORTS],
            HostStage::Output(n, _) if n < HOST_PORTS => trace.graph.host[2 + 2 * HOST_PORTS + n],
            HostStage::RackBus(n) if n < HOST_PORTS => trace.graph.host[1 + n],
            HostStage::Master(n) if n < HOST_PORTS => trace.graph.host[1 + HOST_PORTS + n],
            _ => return,
        };
        let mut output = *input;
        let mut applied = [0.; 2];
        for c in 0..2 {
            for i in 0..len {
                let g = if enabled {
                    gain[c] * master.map_or(1., |m| m[i])
                } else {
                    0.
                };
                output[c][i] = f64::from(input[c][i] as f32 * g);
                applied[c] += f64::from(g) / len.max(1) as f64;
            }
        }
        recorder.begin(at, len);
        recorder.record(
            id,
            input,
            &output,
            len,
            applied,
            enabled,
            TraceIdentity {
                external_port: (!matches!(stage, HostStage::PartFader | HostStage::AuxSend)).then_some(port),
                output_channels: match stage {HostStage::Output(_, channels) => Some(channels), _ => None},
                routed_to: match stage {
                    HostStage::PartFader | HostStage::AuxSend if port < HOST_PORTS => Some(trace.graph.host[1 + port]),
                    HostStage::RackBus(p) if p < HOST_PORTS => {
                        Some(trace.graph.host[1 + HOST_PORTS + p])
                    }
                    HostStage::Master(_) if port < HOST_PORTS => Some(trace.graph.host[2 + 2 * HOST_PORTS + port]),
                    _ => None,
                },
                ..Default::default()
            },
            &[],
            &trace.graph.nodes[id],
        );
        recorder.scratch[id].record.values[0] = applied[0];
        recorder.scratch[id].record.values[1] = applied[1];
        recorder.end();
    }
}
