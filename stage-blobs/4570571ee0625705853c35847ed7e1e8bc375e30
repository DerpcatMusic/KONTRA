//! Prepared stereo bus DAGs. A bus owns summed-signal history, never note identity.
use crate::dsp::{BLOCK, ControlRamp, PreparedProcessor, ProcessorState, allocate};
use crate::{ControlRange, Error, Frame, Prepared, Processor};

/// One post-processing send. `None` targets the runtime's stereo output.
#[derive(Clone, Copy, Debug)]
pub struct BusSend {
    pub bus: Option<usize>,
    pub gain: f64,
}

/// How the host mixes one bus at run time, after its processors and before
/// its sends. The first send is the bus's own output (lowering's convention);
/// `output` redirects that send to one of the extra outputs given to
/// [`crate::Runtime::render_split`], leaving the remaining sends in place.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BusMix {
    /// Left and right gain: fader, pan and mute folded together.
    pub gain: [f32; 2],
    pub output: Option<usize>,
}
impl Default for BusMix {
    fn default() -> Self {
        Self {
            gain: [1.0; 2],
            output: None,
        }
    }
}

/// One summed-signal processing scope. Use separate nodes for pre/post-insert taps.
/// Tails are explicit maximum zero-input durations, in output sample frames.
pub struct Bus {
    pub processors: Vec<Processor>,
    pub sends: Vec<BusSend>,
    pub tail_frames: u32,
}

/// Where a group's fader lives (see [`crate::Prepared::with_group_faders`]).
#[derive(Clone, Debug, PartialEq)]
pub struct GroupFader {
    pub bus: usize,
    /// Indices into the bus's sends (the bus's own output is send 0) that the
    /// fader scales; later sends leave before it.
    pub follows: Vec<usize>,
    /// The fader's starting linear level.
    pub initial: f64,
}

struct PreparedBus {
    processors: Box<[PreparedProcessor]>,
    sends: Box<[BusSend]>,
    /// Which sends the runtime fader scales, and its starting level.
    follows: Box<[bool]>,
    fader: f64,
    states: std::ops::Range<usize>,
    tail_frames: u32,
}

#[derive(Default)]
pub(super) struct PreparedBuses {
    nodes: Box<[PreparedBus]>,
    order: Box<[usize]>,
    cells: usize,
    delay_frames: usize,
    reverbs: Box<[(crate::dsp::ReverbSettings, u32)]>,
    /// Impulse index, dry and wet gain, by `PreparedProcessor::Convolution` index.
    convolutions: Box<[(usize, f64, f64)]>,
    /// The bus each convolution belongs to.
    convolution_bus: Box<[usize]>,
    impulses: Box<[std::sync::Arc<crate::dsp::Impulse>]>,
    rate: u32,
    filters: Box<[crate::dsp::svf::PreparedFilter]>,
    pub parameters: Box<[ControlRange]>,
    pub controls: Box<[(crate::ControlId, usize)]>,
}
impl PreparedBuses {
    pub(crate) fn trace_filter(&self, i: usize) -> [(&'static str, crate::dsp::control::PreparedParameter); 2] { self.filters[i].trace_parameters() }
    pub(crate) fn trace_reverb(&self, i: usize) -> [(&'static str, f64); 9] {
        let r=self.reverbs[i].0;
        [("decay_seconds",r.decay_seconds),("size",r.size),("damping_hz",r.damping_hz),("modulation_seconds",r.modulation_seconds),("diffusion",r.diffusion),("predelay_seconds",r.predelay_seconds),("input_cutoff_hz",r.input_cutoff_hz),("low_shelf_db",r.low_shelf_db),("width",r.width)]
    }
    pub(crate) fn trace_convolution(&self, i: usize) -> [(&'static str, f64); 3] { let (impulse,dry,wet)=self.convolutions[i]; [("impulse_index",impulse as f64),("dry",dry),("wet",wet)] }

    /// Convolution processors across the buses, in bus then processor order.
    pub(super) fn convolution_slots(&self) -> usize {
        self.convolutions.len()
    }
    pub(super) fn set_fader(&mut self, fader: &GroupFader) -> Result<(), Error> {
        let node = self.nodes.get_mut(fader.bus).ok_or(Error::InvalidInput)?;
        let mut follows = vec![false; node.sends.len()];
        for &n in &fader.follows {
            *follows.get_mut(n).ok_or(Error::InvalidInput)? = true;
        }
        if !(fader.initial.is_finite() && fader.initial >= 0.0) {
            return Err(Error::InvalidInput);
        }
        node.follows = follows.into_boxed_slice();
        node.fader = fader.initial;
        Ok(())
    }

    pub fn new(
        rate: u32,
        buses: Vec<Bus>,
        impulses: &[std::sync::Arc<crate::dsp::Impulse>],
    ) -> Result<Self, Error> {
        let mut indegree = vec![0usize; buses.len()];
        for bus in &buses {
            for send in &bus.sends {
                if !send.gain.is_finite() {
                    return Err(Error::InvalidInput);
                }
                if let Some(index) = send.bus {
                    let count = indegree.get_mut(index).ok_or(Error::InvalidInput)?;
                    *count = count.checked_add(1).ok_or(Error::Capacity)?;
                }
            }
        }
        let mut order = Vec::with_capacity(buses.len());
        let mut ready: std::collections::VecDeque<_> = indegree
            .iter()
            .enumerate()
            .filter_map(|(index, count)| (*count == 0).then_some(index))
            .collect();
        while let Some(index) = ready.pop_front() {
            order.push(index);
            for send in &buses[index].sends {
                if let Some(target) = send.bus {
                    indegree[target] -= 1;
                    if indegree[target] == 0 {
                        ready.push_back(target);
                    }
                }
            }
        }
        if order.len() != buses.len() {
            return Err(Error::InvalidInput);
        }
        let mut parameters = Vec::new();
        let mut cells = 0usize;
        let mut delay_frames = 0;
        let mut filters = Vec::new();
        let mut reverbs = Vec::new();
        let mut convolutions = Vec::new();
        let mut convolution_bus = Vec::new();
        let nodes = buses
            .into_iter()
            .enumerate()
            .map(|(index, bus)| {
                let begin = cells;
                cells = cells
                    .checked_add(bus.processors.len())
                    .ok_or(Error::Capacity)?;
                let processors = crate::dsp::compile_processors(
                    bus.processors.into_boxed_slice(),
                    rate,
                    &mut parameters,
                    &mut delay_frames,
                    &mut filters,
                    Some(&mut reverbs),
                    Some(&mut convolutions),
                )?;
                convolution_bus.resize(convolutions.len(), index);
                Ok(PreparedBus {
                    processors,
                    follows: Box::new([]),
                    fader: 1.0,
                    sends: bus.sends.into_boxed_slice(),
                    states: begin..cells,
                    tail_frames: bus.tail_frames,
                })
            })
            .collect::<Result<Box<[_]>, Error>>()?;
        if convolutions.iter().any(|(i, ..)| *i >= impulses.len()) {
            return Err(Error::InvalidInput);
        }
        if filters.iter().any(|filter| filter.requires_expression()) {
            return Err(Error::InvalidInput);
        }
        let mut controls: Vec<_> = parameters
            .iter()
            .enumerate()
            .map(|(i, binding)| (binding.control, i))
            .collect();
        controls.sort_unstable();
        Ok(Self {
            nodes,
            order: order.into_boxed_slice(),
            cells,
            delay_frames,
            reverbs: reverbs.into_boxed_slice(),
            convolutions: convolutions.into_boxed_slice(),
            convolution_bus: convolution_bus.into_boxed_slice(),
            impulses: impulses.into(),
            rate,
            filters: filters.into_boxed_slice(),
            parameters: parameters.into_boxed_slice(),
            controls: controls.into_boxed_slice(),
        })
    }
    pub fn len(&self) -> usize {
        self.nodes.len()
    }
}

struct Buffer {
    samples: [Frame; BLOCK],
    input_frames: usize,
    remaining: u32,
    /// Written since last cleared; idle buses cost nothing to begin.
    dirty: bool,
}

impl Default for Buffer {
    fn default() -> Self {
        Self {
            samples: [[0.; 2]; BLOCK],
            input_frames: 0,
            remaining: 0,
            dirty: false,
        }
    }
}

pub(super) struct BusState {
    buffers: Box<[Buffer]>,
    cells: Box<[ProcessorState]>,
    delay_samples: Box<[[f64; 2]]>,
    reverbs: Box<[crate::dsp::Reverb]>,
    convolutions: Box<[crate::dsp::Convolution]>,
    /// Per bus, the frames its tail rings after input stops; a swapped-in
    /// impulse may change it.
    tail_frames: Box<[u32]>,
    pub parameters: Box<[ControlRamp]>,
    filters: crate::dsp::svf::FilterBank,
    pub mix: Box<[BusMix]>,
    /// Per bus, the runtime level of the sends its fader scales.
    pub fader: Box<[f64]>,
    /// Per bus, its post-mix peak since last taken.
    pub peaks: Box<[[f32; 2]]>,
}
impl BusState {
    pub fn new(plan: &Prepared) -> Result<Self, Error> {
        Ok(Self {
            buffers: allocate(plan.buses.len())?,
            cells: allocate(plan.buses.cells)?,
            delay_samples: allocate(plan.buses.delay_frames)?,
            reverbs: plan
                .buses
                .reverbs
                .iter()
                .map(|(settings, _)| crate::dsp::Reverb::new(settings, plan.buses.rate))
                .collect::<Result<_, _>>()?,
            convolutions: plan
                .buses
                .convolutions
                .iter()
                .map(|&(impulse, dry, wet)| {
                    crate::dsp::Convolution::new(&plan.buses.impulses[impulse], dry, wet)
                })
                .collect(),
            tail_frames: plan.buses.nodes.iter().map(|n| n.tail_frames).collect(),
            filters: crate::dsp::svf::FilterBank::new(&plan.buses.filters, 0)?,
            parameters: crate::dsp::control::initial_parameters(plan, &plan.buses.parameters),
            mix: vec![BusMix::default(); plan.buses.len()].into_boxed_slice(),
            fader: plan.buses.nodes.iter().map(|n| n.fader).collect(),
            peaks: vec![[0.0; 2]; plan.buses.len()].into_boxed_slice(),
        })
    }
    /// Exchange convolution `slot` with `upload`: no allocation, state starts
    /// empty (the old tail stops), and the bus rings for the new impulse's tail.
    pub fn swap_convolution(
        &mut self,
        graph: &PreparedBuses,
        slot: usize,
        upload: &mut crate::ConvolutionUpload,
    ) -> Result<(), Error> {
        let bus = *graph.convolution_bus.get(slot).ok_or(Error::InvalidInput)?;
        std::mem::swap(&mut self.convolutions[slot], &mut upload.conv);
        self.tail_frames[bus] = upload.tail;
        Ok(())
    }
    pub fn begin(&mut self) {
        for buffer in &mut self.buffers {
            if buffer.dirty {
                buffer.samples.fill([0.; 2]);
                buffer.dirty = false;
            }
            buffer.input_frames = 0;
        }
    }
    pub fn input(&mut self, bus: usize, frames: usize) -> &mut [Frame] {
        let buffer = &mut self.buffers[bus];
        buffer.dirty = true;
        &mut buffer.samples[..frames]
    }
    pub fn fed(&mut self, bus: usize, frames: usize) {
        self.buffers[bus].input_frames = self.buffers[bus].input_frames.max(frames);
    }
    pub fn active(&self) -> bool {
        self.buffers.iter().any(|b| b.remaining != 0)
    }
    pub fn reset(&mut self) {
        self.begin();
        self.cells.fill(ProcessorState::default());
        self.reverbs.iter_mut().for_each(crate::dsp::Reverb::clear);
        self.convolutions
            .iter_mut()
            .for_each(crate::dsp::Convolution::clear);
        for buffer in &mut self.buffers {
            buffer.remaining = 0;
        }
    }
    /// `outs` are whole-render buffers; this block starts at `offset` in them.
    pub fn render<const TRACE: bool>(
        &mut self,
        graph: &PreparedBuses,
        output: &mut [Frame],
        outs: &mut [&mut [Frame]],
        offset: usize,
        at: u64,
        mut trace: Option<(&crate::trace::TraceGraph, &mut crate::trace::Recorder)>,
    ) -> u64 {
        let mut faults = 0;
        for &index in &graph.order {
            let node = &graph.nodes[index];
            let buffer = &mut self.buffers[index];
            let states = &mut self.cells[node.states.clone()];
            // Input frames hold the tail at its maximum; it then counts down
            // over zero input until it ends, which clears the bus's history.
            let len = output.len();
            let input = buffer.input_frames.min(len);
            let tail = if input > 0 {
                self.tail_frames[index]
            } else {
                buffer.remaining
            };
            let produced = input + (tail as usize).min(len - input);
            buffer.remaining = tail - (produced - input) as u32;
            if produced > 0 {
                let mut block = [[0.; BLOCK]; 2];
                for (i, frame) in buffer.samples[..produced].iter().enumerate() {
                    block[0][i] = f64::from(frame[0]);
                    block[1][i] = f64::from(frame[1]);
                }
                if TRACE { if let Some((g,r)) = trace.as_mut() {
                    let id = g.buses[index].input;
                    r.record(id, &block, &block, produced, [1.; 2], true, Default::default(), &self.parameters, &g.nodes[id]);
                } }
                let fault = crate::dsp::process::<TRACE>(
                    &node.processors,
                    states,
                    &mut block,
                    produced,
                    &self.parameters,
                    at,
                    &mut self.delay_samples,
                    &mut crate::dsp::svf::FilterContext {
                        bank: &mut self.filters,
                        expression: None,
                        reverbs: &mut self.reverbs,
                        convolutions: &mut self.convolutions,
                    },
                    if TRACE { trace.as_mut().map(|(g,r)| crate::trace::Section { recorder: &mut **r,
                        graph: g, nodes: &g.buses[index].stages, identity: Default::default() }) } else { None },
                );
                let finite = !fault
                    && block
                        .iter()
                        .all(|c| c[..produced].iter().all(|v| (*v as f32).is_finite()))
                    && states.iter().all(ProcessorState::finite);
                if finite {
                    for (i, frame) in buffer.samples[..produced].iter_mut().enumerate() {
                        *frame = [block[0][i], block[1][i]]
                            .map(|v| v as f32)
                            .map(|v| if v.is_subnormal() { 0. } else { v });
                    }
                } else {
                    buffer.samples[..produced].fill([0.; 2]);
                    states.fill(ProcessorState::default());
                    self.reverbs.iter_mut().for_each(crate::dsp::Reverb::clear);
                    self.convolutions
                        .iter_mut()
                        .for_each(crate::dsp::Convolution::clear);
                    faults += 1;
                }
            }
            // The tail ended inside this block: nothing carries over.
            if produced < len {
                states.fill(ProcessorState::default());
            }
            if produced == 0 {
                continue;
            }
            buffer.dirty = true;
            let before_mix = if TRACE { crate::trace::planar(&buffer.samples[..produced]) } else { [[0.; BLOCK]; 2] };
            let mix = self.mix[index];
            let fader = self.fader[index];
            if mix.gain != [1.0; 2] {
                for frame in &mut buffer.samples[..produced] {
                    frame[0] *= mix.gain[0];
                    frame[1] *= mix.gain[1];
                }
            }
            let peak = &mut self.peaks[index];
            for frame in &buffer.samples[..produced] {
                *peak = [peak[0].max(frame[0].abs()), peak[1].max(frame[1].abs())];
            }
            if TRACE { if let Some((g,r)) = trace.as_mut() {
                let id = g.buses[index].output; let block = crate::trace::planar(&buffer.samples[..produced]);
                r.record(id, &before_mix, &block, produced, mix.gain.map(f64::from), true, Default::default(), &self.parameters, &g.nodes[id]);
            } }
            // Copy one bounded block so fan-out never aliases destination state.
            let samples = buffer.samples;
            for (n, send) in node.sends.iter().enumerate() {
                // Out-of-range outputs fall back to the bus's own target.
                let direct = mix.output.filter(|&out| n == 0 && out < outs.len());
                let gain = if node.follows.get(n).copied().unwrap_or(false) {
                    send.gain * fader
                } else {
                    send.gain
                };
                if TRACE { if let Some((g,r)) = trace.as_mut() {
                    let input = crate::trace::planar(&samples[..produced]);
                    let mut sent = input; for channel in &mut sent { for value in &mut channel[..produced] { *value *= gain; } }
                    let id = g.buses[index].sends[n];
                    r.record(id, &input, &sent, produced, [gain; 2], gain != 0., crate::trace::TraceIdentity {external_port:direct,routed_to:direct.filter(|&p|p<crate::trace::HOST_PORTS).map(|p|g.host[1+p]).or_else(||send.bus.map(|b|g.buses[b].input)).or(Some(g.master)),..Default::default()}, &self.parameters, &g.nodes[id]);
                    if send.bus.is_none() && direct.is_none() {
                        r.record(g.master, &sent, &sent, produced, [1.; 2], true, Default::default(), &self.parameters, &g.nodes[g.master]);
                    }
                } }
                let target = if let Some(out) = direct {
                    &mut outs[out][offset..offset + produced]
                } else if let Some(bus) = send.bus {
                    self.fed(bus, produced);
                    self.input(bus, produced)
                } else {
                    &mut output[..produced]
                };
                for (input, frame) in samples.iter().zip(target) {
                    for channel in 0..2 {
                        frame[channel] += (f64::from(input[channel]) * gain) as f32;
                    }
                }
            }
        }
        faults
    }
}

impl PreparedBuses {
    pub(crate) fn trace_graph(&self, plan: &Prepared, graph: &mut crate::trace::TraceGraph) {
        let initial = crate::dsp::control::initial_parameters(plan, &self.parameters);
        for (bus, node) in self.nodes.iter().enumerate() {
            let group = plan.group_faders.iter().position(|f| f.as_ref().is_some_and(|f| f.bus == bus)).map(|n| n as u32);
            let input = graph.node(if group.is_some() { "group_bus" } else { "bus_input" }, "sum", None, group, Some(bus), vec![], 0);
            let stages: Vec<_> = node.processors.iter().map(|s| graph.stage(s, "bus_fx", None, group, Some(bus), plan, &self.parameters, &initial)).collect();
            let native = stages.iter().flat_map(|&id|graph.nodes[id].parameters.iter()).filter_map(|p|p.address).find(|a|a.group==-1);
            let role=if plan.bus_addresses.iter().any(|(a,b)|*b==bus && *a>=1000) { "instrument_bus" }
                else if native.is_some_and(|a|a.generic==0) { "send_return_rack" }
                else if native.is_some_and(|a|a.generic==1) { "instrument_inserts" }
                else if native.is_some_and(|a|a.generic==2) { "master_inserts" }
                else { "bus_output" };
            let output = graph.node(role, "fader_pan", None, group, Some(bus), vec![], 0);
            let parent = graph.connect(&node.processors, &stages, input);
            graph.edge(parent, output, "serial");
            let sends = node.sends.iter().enumerate().map(|(i,send)| graph.node("bus_send", "send_gain", None, group, Some(bus), vec![crate::trace::TraceParameter::constant("level",send.gain),crate::trace::TraceParameter::constant("post_fader",f64::from(node.follows.get(i).copied().unwrap_or(true)))], 0)).collect();
            graph.buses.push(crate::trace::BusNodes { input, output, stages, sends });
        }
        for (bus, node) in self.nodes.iter().enumerate() {
            for (n, send) in node.sends.iter().enumerate() {
                let id = graph.buses[bus].sends[n];
                graph.edge(graph.buses[bus].output, id, "tap");
                graph.edge(id, send.bus.map_or(graph.master, |b| graph.buses[b].input), "send");
                if n == 0 {
                    for port in 0..crate::trace::HOST_PORTS {
                        graph.edge(id, graph.host[1 + port], "possible_direct_route");
                    }
                }
            }
        }
    }
}
