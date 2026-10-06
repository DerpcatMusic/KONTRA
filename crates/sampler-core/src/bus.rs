//! Prepared stereo bus DAGs. A bus owns summed-signal history, never note identity.
use crate::dsp::{ControlRamp, PreparedProcessor, ProcessorState, allocate};
use crate::{ControlRange, Error, Frame, Prepared, Processor};

const BLOCK: usize = 64;

/// One post-processing send. `None` targets the runtime's stereo output.
#[derive(Clone, Copy, Debug)]
pub struct BusSend {
    pub bus: Option<usize>,
    pub gain: f64,
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
                    )?,
                    sends: bus.sends.into_boxed_slice(),
                    states: begin..cells,
                    tail_frames: bus.tail_frames,
                })
            })
            .collect::<Result<Box<[_]>, Error>>()?;
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
}

impl Default for Buffer {
    fn default() -> Self {
        Self {
            samples: [[0.; 2]; BLOCK],
            input_frames: 0,
            remaining: 0,
        }
    }
}

pub(super) struct BusState {
    buffers: Box<[Buffer]>,
    cells: Box<[ProcessorState]>,
    delay_samples: Box<[[f64; 2]]>,
    pub parameters: Box<[ControlRamp]>,
    filters: Box<[crate::dsp::svf::FilterCache]>,
}
impl BusState {
    pub fn new(plan: &Prepared) -> Result<Self, Error> {
        Ok(Self {
            buffers: allocate(plan.buses.len())?,
            cells: allocate(plan.buses.cells)?,
            delay_samples: allocate(plan.buses.delay_frames)?,
            filters: plan
                .buses
                .filters
                .iter()
                .copied()
                .map(crate::dsp::svf::FilterCache::new)
                .collect(),
            parameters: crate::dsp::control::initial_parameters(plan, &plan.buses.parameters),
        })
    }
    pub fn begin(&mut self) {
        for buffer in &mut self.buffers {
            buffer.samples.fill([0.; 2]);
            buffer.input_frames = 0;
        }
    }
    pub fn input(&mut self, bus: usize, frames: usize) -> &mut [Frame] {
        &mut self.buffers[bus].samples[..frames]
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
        for buffer in &mut self.buffers {
            buffer.remaining = 0;
        }
    }
    pub fn render(&mut self, graph: &PreparedBuses, output: &mut [Frame], at: u64) -> u64 {
        let mut faults = 0;
        for filter in &mut self.filters {
            filter.begin(at);
        }
        for &index in &graph.order {
            let node = &graph.nodes[index];
            let buffer = &mut self.buffers[index];
            let states = &mut self.cells[node.states.clone()];
            let mut produced = 0;
            for (frame_index, frame) in buffer.samples[..output.len()].iter_mut().enumerate() {
                if frame_index < buffer.input_frames {
                    buffer.remaining = node.tail_frames;
                } else if buffer.remaining != 0 {
                    buffer.remaining -= 1;
                } else {
                    states.fill(ProcessorState::default());
                    break;
                }
                let value = crate::dsp::process(
                    &node.processors,
                    states,
                    frame.map(f64::from),
                    &self.parameters,
                    at + frame_index as u64,
                    &mut self.delay_samples,
                    &mut self.filters,
                );
                let result = value.map(|v| v as f32);
                if result.iter().all(|v| v.is_finite()) && states.iter().all(ProcessorState::finite)
                {
                    *frame = result.map(|v| if v.is_subnormal() { 0. } else { v });
                } else {
                    *frame = [0.; 2];
                    states.fill(ProcessorState::default());
                    faults += 1;
                }
                produced += 1;
            }
            // Copy one bounded block so fan-out never aliases destination state.
            let samples = buffer.samples;
            for send in &node.sends {
                let target = if let Some(bus) = send.bus {
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
