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

struct PreparedBus {
    processors: Box<[PreparedProcessor]>,
    sends: Box<[BusSend]>,
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
    rate: u32,
    filters: Box<[crate::dsp::svf::PreparedFilter]>,
    pub parameters: Box<[ControlRange]>,
    pub controls: Box<[(crate::ControlId, usize)]>,
}
impl PreparedBuses {
    pub fn new(rate: u32, buses: Vec<Bus>) -> Result<Self, Error> {
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
        let nodes = buses
            .into_iter()
            .map(|bus| {
                let begin = cells;
                cells = cells
                    .checked_add(bus.processors.len())
                    .ok_or(Error::Capacity)?;
                Ok(PreparedBus {
                    processors: crate::dsp::compile_processors(
                        bus.processors.into_boxed_slice(),
                        rate,
                        &mut parameters,
                        &mut delay_frames,
                        &mut filters,
                        Some(&mut reverbs),
                    )?,
                    sends: bus.sends.into_boxed_slice(),
                    states: begin..cells,
                    tail_frames: bus.tail_frames,
                })
            })
            .collect::<Result<Box<[_]>, Error>>()?;
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
    pub parameters: Box<[ControlRamp]>,
    filters: crate::dsp::svf::FilterBank,
    pub mix: Box<[BusMix]>,
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
            filters: crate::dsp::svf::FilterBank::new(&plan.buses.filters, 0)?,
            parameters: crate::dsp::control::initial_parameters(plan, &plan.buses.parameters),
            mix: vec![BusMix::default(); plan.buses.len()].into_boxed_slice(),
            peaks: vec![[0.0; 2]; plan.buses.len()].into_boxed_slice(),
        })
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
        for buffer in &mut self.buffers {
            buffer.remaining = 0;
        }
    }
    /// `outs` are whole-render buffers; this block starts at `offset` in them.
    pub fn render(
        &mut self,
        graph: &PreparedBuses,
        output: &mut [Frame],
        outs: &mut [&mut [Frame]],
        offset: usize,
        at: u64,
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
                node.tail_frames
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
                let fault = crate::dsp::process(
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
                    },
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
            let mix = self.mix[index];
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
            // Copy one bounded block so fan-out never aliases destination state.
            let samples = buffer.samples;
            for (n, send) in node.sends.iter().enumerate() {
                // Out-of-range outputs fall back to the bus's own target.
                let direct = mix.output.filter(|&out| n == 0 && out < outs.len());
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
                        frame[channel] += (f64::from(input[channel]) * send.gain) as f32;
                    }
                }
            }
        }
        faults
    }
}
