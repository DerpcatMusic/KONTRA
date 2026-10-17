//! Prepared stereo bus DAGs. A bus owns summed-signal history, never note identity.
use crate::dsp::{BLOCK, ControlRamp, PreparedProcessor, ProcessorState, allocate};
use crate::{ChannelCount, ControlRange, Error, Frame, Prepared, Processor};

#[cfg(test)]
std::thread_local! {
    static STATE_CLEARS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

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

/// Raw stereo sample peaks of a summed runtime bus. These are linear peaks,
/// not a native meter's ballistics, mono summation or display transfer law.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BusPeakTap {
    /// Sum entering the bus, before its tone, inserts and mix gain.
    Input,
    /// Signal after the bus's inserts and mix gain, before sends.
    Output,
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

/// A declared channel program on one EXISTING global bus owner. Its input and
/// output are explicit layouts; this is never a per-note/expression owner.
/// Legacy Bus and Frame APIs remain stereo when no such definition is attached.
pub struct ChannelBus {
    pub bus:usize,
    pub owner:Option<sampler_ir::SourceOwnerId>,
    pub signal:sampler_ir::ProcessingSignal,
    pub output:ChannelCount,
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
    pre_sends:Box<[bool]>,
    /// Which sends the runtime fader scales, and its starting level.
    follows: Box<[bool]>,
    fader: f64,
    states: std::ops::Range<usize>,
    tail_frames: u32,
    signal:Option<crate::topology::signal::Signal>,
    source_owner:Option<crate::prepare::PreparedSourceOwnerIndex>,
}

#[derive(Default)]
pub(super) struct PreparedBuses {
    nodes: Box<[PreparedBus]>,
    order: Box<[usize]>,
    cells: usize,
    delay_frames: usize,
    /// Every reverb and convolution, by `PreparedProcessor::Space` index.
    spaces: Box<[sampler_ir::fx::space::Config]>,
    /// The bus each space belongs to.
    space_bus: Box<[usize]>,
    /// The spaces that hold a convolution, in order: the slots
    /// [`crate::Runtime::swap_convolution`] addresses.
    convolutions: Box<[usize]>,
    impulses: Box<[std::sync::Arc<crate::dsp::Impulse>]>,
    rate: u32,
    pub(super) input: Option<usize>,
    filters: crate::dsp::svf::PreparedFilters,
    channel_workspace_bytes:usize,
    /// The last caller-declared channel workspace ceiling is retained so
    /// later input-bus selection cannot silently change the admitted price.
    channel_workspace_limit:Option<usize>,
    pub parameters: Box<[ControlRange]>,
    pub controls: Box<[(crate::ControlId, usize)]>,
}
impl PreparedBuses {
    pub(crate) fn trace_filter(
        &self,
        i: crate::dsp::svf::SvfHandle,
    ) -> [(&'static str, crate::dsp::control::PreparedParameter); 2] {
        self.filters.trace_parameters(i)
    }
    pub(crate) fn trace_space(&self, i: usize) -> Vec<(&'static str, f64)> {
        let c = self.spaces[i];
        let mut out = vec![
            ("impulse_index", c.impulse.map_or(-1.0, |i| i.0 as f64)),
            ("dry", c.dry),
            ("wet", c.wet),
        ];
        if let Some(n) = c.network {
            out.extend([
                ("topology", n.topology as u8 as f64),
                ("decay_seconds", n.decay_seconds),
                ("size", n.size),
                ("damping_hz", n.damping_hz),
                ("modulation_seconds", n.modulation_seconds),
                ("diffusion", n.diffusion),
                ("predelay_seconds", n.predelay_seconds),
                ("input_cutoff_hz", n.input_cutoff_hz),
                ("low_shelf_db", n.low_shelf_db),
                ("width", n.width),
            ]);
        }
        out
    }

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
        let mut filters = crate::dsp::svf::FilterRegistryBuilder::default();
        let mut spaces = Vec::new();
        let mut space_bus = Vec::new();
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
                    Some(&mut spaces),
                )?;
                space_bus.resize(spaces.len(), index);
                Ok(PreparedBus {
                    processors,
                    follows: Box::new([]),
                    fader: 1.0,
                    sends: bus.sends.into_boxed_slice(),
                    pre_sends:Box::new([]),
                    states: begin..cells,
                    tail_frames: bus.tail_frames,
                    signal:None,source_owner:None,
                })
            })
            .collect::<Result<Box<[_]>, Error>>()?;
        if spaces.iter().any(|c| c.impulse.is_some_and(|i| i.0 >= impulses.len())) {
            return Err(Error::InvalidInput);
        }
        let convolutions: Vec<usize> = (0..spaces.len()).filter(|&i| spaces[i].impulse.is_some()).collect();
        if filters.requires_expression() {
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
            spaces: spaces.into_boxed_slice(),
            space_bus: space_bus.into_boxed_slice(),
            convolutions: convolutions.into_boxed_slice(),
            impulses: impulses.into(),
            rate,
            input: None,
            filters: filters.finish(),
            channel_workspace_bytes:0,channel_workspace_limit:None,
            parameters: parameters.into_boxed_slice(),
            controls: controls.into_boxed_slice(),
        })
    }
    /// Actual immutable domains retained by the compiled global programs.
    /// Schema replacement validates these same bindings, never a mirror bank.
    pub(super) fn signal_domains(&self)->impl Iterator<Item=&(crate::ControlId,crate::ControlDomain)> {
        self.nodes.iter().filter_map(|node|node.signal.as_ref()).flat_map(|signal|signal.domains.iter())
    }
    pub(crate) fn input_channels(&self,index:usize)->Result<usize,Error> {
        Ok(self.nodes.get(index).ok_or(Error::InvalidInput)?.signal.as_ref().map_or(2,|signal|signal.input.get()))
    }
    pub(crate) fn output_channels(&self,index:usize)->Result<usize,Error> {
        Ok(self.nodes.get(index).ok_or(Error::InvalidInput)?.signal.as_ref().map_or(2,|signal|signal.output.get()))
    }
    /// Host redirection addresses send0's tap, independently of the post-chain
    /// width where the per-channel mix gain is applied.
    pub(crate) fn main_send_channels(&self,index:usize)->Result<usize,Error> {
        let node=self.nodes.get(index).ok_or(Error::InvalidInput)?;
        if node.pre_sends.first().copied().unwrap_or(false) {self.input_channels(index)}
        else {self.output_channels(index)}
    }

    /// Pure offaudio candidate configuration; complete scope/host geometry is
    /// checked by the enclosing transaction after typed buses are attached.
    fn select_pre_sends(&mut self,taps:Vec<(usize,usize)>)->Result<(),Error>{
        // Unchanged legacy graphs retain their empty representation and incur
        // no per-node tap allocation. An explicit empty replacement still
        // clears previously installed taps.
        if taps.is_empty() && self.nodes.iter().all(|node|node.pre_sends.is_empty()) {return Ok(());}
        let mut selected=self.nodes.iter().map(|node|vec![false;node.sends.len()]).collect::<Vec<_>>();
        for(bus,send)in taps{let flag=selected.get_mut(bus).and_then(|row|row.get_mut(send)).ok_or(Error::InvalidInput)?;
            if *flag{return Err(Error::InvalidInput);}*flag=true;}
        // Pre/post selection changes the actual source side of every edge.
        // Check the complete replacement roster before publishing any mask.
        for(index,node)in self.nodes.iter().enumerate(){for(send_index,send)in node.sends.iter().enumerate(){
            let source=if selected[index][send_index]{self.input_channels(index)?}else{self.output_channels(index)?};
            let target=send.bus.map_or(Ok(2),|bus|self.input_channels(bus))?;
            if source!=target{return Err(Error::UnsupportedSourceLayout);}
        }}
        for(node,selected)in self.nodes.iter_mut().zip(selected){node.pre_sends=selected.into_boxed_slice();}
        Ok(())
    }
    fn validate_edges(&self)->Result<(),Error> {
        if let Some(input)=self.input {
            if self.input_channels(input)?!=2 {return Err(Error::UnsupportedSourceLayout);}
        }
        for(index,node)in self.nodes.iter().enumerate(){
            let output=self.output_channels(index)?;
            for(n,send)in node.sends.iter().enumerate(){let input=send.bus.map_or(Ok(2),|bus|self.input_channels(bus))?;
                let source=if node.pre_sends.get(n).copied().unwrap_or(false){self.input_channels(index)?}else{output};
                if input!=source{return Err(Error::UnsupportedSourceLayout);}}
        }
        Ok(())
    }
    fn priced_channel_workspace(&self,input:Option<usize>)->Result<usize,Error>{
        if let Some(input)=input {
            // The ordinary source/host input path supplies Frame stereo. An
            // N-width bus requires an explicit N source edge, not zero-padding.
            if self.input_channels(input)?!=2 {return Err(Error::UnsupportedSourceLayout);}
        }
        if self.nodes.iter().all(|node|node.signal.is_none()){return Ok(0);}
        let mut bytes=self.nodes.len().checked_mul(size_of::<Option<ChannelBuffer>>()).ok_or(Error::Capacity)?;
        for(index,node)in self.nodes.iter().enumerate(){if let Some(signal)=&node.signal {
            bytes=bytes.checked_add(ChannelBuffer::workspace_bytes(signal,input==Some(index))?).ok_or(Error::Capacity)?;
        }}
        Ok(bytes)
    }
    pub(super) fn select_input_bus(&mut self,input:usize)->Result<(),Error>{
        let bytes=self.priced_channel_workspace(Some(input))?;
        if self.channel_workspace_limit.is_some_and(|limit|bytes>limit){return Err(Error::Capacity);}
        // Publish the selected input and its exact price together, after all
        // geometry and caller-ceiling checks. No existing runtime is mutated.
        self.input=Some(input);self.channel_workspace_bytes=bytes;Ok(())
    }
    fn validate_channel_configuration(&self)->Result<(),Error>{
        self.validate_edges()?;
        let bytes=self.priced_channel_workspace(self.input)?;
        if bytes!=self.channel_workspace_bytes{return Err(Error::InvalidInput);}
        if bytes!=0 && self.channel_workspace_limit.is_none(){return Err(Error::InvalidInput);}
        if self.channel_workspace_limit.is_some_and(|limit|bytes>limit){return Err(Error::Capacity);}
        Ok(())
    }
    pub fn len(&self) -> usize {
        self.nodes.len()
    }
}

impl Prepared {
    /// Offaudio channel attachment. Actual owner/control/edge identities are
    /// verified before any runtime exists. Multiple definitions cannot alias.
    pub fn with_channel_buses(mut self,definitions:Vec<ChannelBus>,max_workspace_bytes:usize)->Result<Self,Error> {
        if definitions.is_empty(){return Ok(self);}
        let mut seen=vec![false;self.buses.nodes.len()];
        let mut parameters=self.buses.parameters.to_vec();
        let mut compiled=Vec::with_capacity(definitions.len());
        for definition in definitions {
            let node=self.buses.nodes.get(definition.bus).ok_or(Error::InvalidInput)?;
            if seen[definition.bus] || node.signal.is_some(){return Err(Error::InvalidInput);}
            seen[definition.bus]=true;
            let owner=definition.owner.map(|id|self.source_owner_index(id).ok_or(Error::InvalidInput)).transpose()?;
            let signal=crate::topology::signal::compile_program(&self,&definition.signal,&mut parameters,Some(definition.output),false)?;
            if signal.bus_chain != !node.processors.is_empty(){return Err(Error::InvalidInput);}
            compiled.push((definition.bus,owner,signal));
        }
        // Self is still an offaudio owned candidate. After installing this
        // batch, admit its final actual input/tap/edge geometry and price.
        for(index,owner,signal)in compiled {let node=&mut self.buses.nodes[index];
            node.tail_frames=node.tail_frames.checked_add(signal.tail_frames).ok_or(Error::Capacity)?;
            node.signal=Some(signal);node.source_owner=owner;}
        self.buses.validate_edges()?;
        let workspace=self.buses.priced_channel_workspace(self.buses.input)?;
        if workspace>max_workspace_bytes{return Err(Error::Capacity);}
        self.buses.channel_workspace_bytes=workspace;
        self.buses.channel_workspace_limit=Some(max_workspace_bytes);
        self.buses.controls=parameters.iter().enumerate().map(|(lane,range)|(range.control,lane)).collect::<Vec<_>>().into_boxed_slice();
        self.buses.controls.sort_unstable();self.buses.parameters=parameters.into_boxed_slice();
        self.validate_bus_signal_edges()?;
        Ok(self)
    }
    /// Select existing send rows that tap the raw bus sum before its chain.
    /// Indices address the actual compiled sends (own output remains row0).
    pub fn with_bus_pre_sends(mut self,taps:Vec<(usize,usize)>)->Result<Self,Error>{
        if taps.is_empty() && self.buses.nodes.iter().all(|node|node.pre_sends.is_empty()) {return Ok(self);}
        self.buses.select_pre_sends(taps)?;
        self.validate_bus_signal_edges()?;
        Ok(self)
    }
    /// Normal lowering owns the entire offaudio candidate. Retained source
    /// scopes can have an N outlet before actual typed bus inputs are attached;
    /// configure input/taps and typed programs before one complete validation.
    pub(crate) fn with_channel_bus_graph(mut self,definitions:Vec<ChannelBus>,
        max_workspace_bytes:usize,taps:Vec<(usize,usize)>,input:Option<usize>)->Result<Self,Error>{
        self.buses.select_pre_sends(taps)?;
        if let Some(input)=input {self.buses.select_input_bus(input)?;}
        if definitions.is_empty(){self.validate_bus_signal_edges()?;Ok(self)}
        else {self.with_channel_buses(definitions,max_workspace_bytes)}
    }
    pub fn channel_bus_workspace_bytes(&self)->usize{self.buses.channel_workspace_bytes}
    pub(crate) fn validate_bus_signal_edges(&self)->Result<(),Error> {
        self.buses.validate_channel_configuration()?;
        for(index,scope)in self.processing_scopes.iter().enumerate(){
            let output=self.processing_signals.get(index).and_then(Option::as_ref).map_or(2,|signal|signal.output.get());
            let input=scope.bus.map_or(Ok(2),|bus|self.buses.input_channels(bus))?;
            if output!=input{return Err(Error::UnsupportedSourceLayout);}
        }
        for region_index in 0..self.region_count(){if let Some(bus)=self.region_bus(region_index) {
            if self.buses.input_channels(bus)?!=2 && !self.processing_scopes.iter().any(|scope|scope.members.iter().any(|member|*member==region_index)) {
                return Err(Error::UnsupportedSourceLayout);
            }
        }}
        Ok(())
    }
}

struct Buffer {
    samples: [Frame; BLOCK],
    input_frames: usize,
    remaining: u32,
    /// Written since last cleared; idle buses cost nothing to begin.
    dirty: bool,
    /// Processor cells need a reset after processing, never while already asleep.
    state_dirty: bool,
}

impl Default for Buffer {
    fn default() -> Self {
        Self {
            samples: [[0.; 2]; BLOCK],
            input_frames: 0,
            remaining: 0,
            dirty: false,
            state_dirty: false,
        }
    }
}

/// Storage belongs to the existing bus generation, never a note/activation.
/// Every plane/history/scratch word is allocated off audio from compiled geometry.
struct ChannelBuffer {
    samples:Box<[f32]>,pre:Box<[f32]>,work:Box<[f32]>,histories:Box<[f64]>,dry:Box<[f32]>,
    tones:Box<[crate::OutputLowPass]>,tone_history:Box<[[[f64;2];2]]>,
}
impl ChannelBuffer {
    fn workspace_bytes(signal:&crate::topology::signal::Signal,tone:bool)->Result<usize,Error>{
        let plane=signal.max_channels.checked_mul(BLOCK).ok_or(Error::Capacity)?;
        let pairs=if tone{signal.input.get().checked_add(1).ok_or(Error::Capacity)?/2}else{0};
        plane.checked_mul(4).and_then(|n|signal.mix_depth.checked_mul(plane).and_then(|dry|n.checked_add(dry)))
            .and_then(|n|n.checked_mul(size_of::<f32>()))
            .and_then(|bytes|signal.history_words.checked_mul(size_of::<f64>()).and_then(|history|bytes.checked_add(history)))
            .and_then(|bytes|pairs.checked_mul(size_of::<crate::OutputLowPass>()+size_of::<[[f64;2];2]>()).and_then(|tone|bytes.checked_add(tone)))
            .and_then(|bytes|if signal.bus_chain{bytes.checked_add(size_of::<[[f64;BLOCK];2]>())}else{Some(bytes)}).ok_or(Error::Capacity)
    }
    fn new(signal:&crate::topology::signal::Signal,rate:u32,tone:bool)->Result<Self,Error>{
        let plane=signal.max_channels.checked_mul(BLOCK).ok_or(Error::Capacity)?;
        let work=plane.checked_mul(2).ok_or(Error::Capacity)?;
        let dry=plane.checked_mul(signal.mix_depth).ok_or(Error::Capacity)?;
        let pairs=if tone{signal.input.get().checked_add(1).ok_or(Error::Capacity)?/2}else{0};
        let _bytes=plane.checked_add(plane).and_then(|v|v.checked_add(work)).and_then(|v|v.checked_add(dry)).and_then(|v|v.checked_mul(size_of::<f32>()))
            .and_then(|v|signal.history_words.checked_mul(size_of::<f64>()).and_then(|h|v.checked_add(h)))
            .and_then(|v|pairs.checked_mul(size_of::<crate::OutputLowPass>()+size_of::<[[f64;2];2]>()).and_then(|h|v.checked_add(h))).ok_or(Error::Capacity)?;
        Ok(Self{samples:allocate(plane)?,pre:allocate(plane)?,work:allocate(work)?,histories:allocate(signal.history_words)?,dry:allocate(dry)?,
            tones:(0..pairs).map(|_|crate::OutputLowPass::new(rate)).collect::<Result<Vec<_>,_>>()?.into_boxed_slice(),tone_history:allocate(pairs)?})
    }
    fn reset(&mut self){self.samples.fill(0.);self.pre.fill(0.);self.work.fill(0.);self.histories.fill(0.);self.dry.fill(0.);self.tone_history.fill([[0.;2];2]);}
}

pub(super) struct BusState {
    buffers: Box<[Buffer]>,
    channels:Box<[Option<ChannelBuffer>]>,
    tone: Option<crate::OutputLowPass>,
    tone_history: [[f64; 2]; 2],
    tone_ringing: bool,
    cells: Box<[ProcessorState]>,
    delay_samples: Box<[[f64; 2]]>,
    spaces: Box<[crate::dsp::Space]>,
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
    /// Input measurement is opt-in: unobserved buses do no extra sample scan.
    input_peaks: Box<[[f32; 2]]>,
    input_peak_enabled: Box<[bool]>,
}
/// Reset only the failing/expired actual bus's stateful Space owners. The
/// immutable compilation roster is shared by legacy and typed signal paths.
/// No allocation, owner discovery by name, or new history cache is involved.
fn clear_owned_bus_spaces(spaces:&mut[crate::dsp::Space],owners:&[usize],bus:usize){
    for (space,&owner)in spaces.iter_mut().zip(owners){if owner==bus{space.clear();}}
}

impl BusState {
    pub fn new(plan: &Prepared) -> Result<Self, Error> {
        Ok(Self {
            buffers: allocate(plan.buses.len())?,
            channels:if plan.buses.nodes.iter().all(|node|node.signal.is_none()){Box::new([])}else{
                plan.buses.nodes.iter().enumerate().map(|(index,node)|node.signal.as_ref().map(|signal|
                ChannelBuffer::new(signal,plan.buses.rate,plan.buses.input==Some(index))).transpose()).collect::<Result<Vec<_>,_>>()?.into_boxed_slice()},
            tone: plan
                .buses
                .input.filter(|index|plan.buses.nodes[*index].signal.is_none())
                .map(|_| crate::OutputLowPass::new(plan.buses.rate))
                .transpose()?,
            tone_history: [[0.; 2]; 2],
            tone_ringing: false,
            cells: allocate(plan.buses.cells)?,
            delay_samples: allocate(plan.buses.delay_frames)?,
            spaces: plan
                .buses
                .spaces
                .iter()
                .map(|config| crate::dsp::Space::new(config, plan.buses.rate, &plan.buses.impulses))
                .collect::<Result<_, _>>()?,
            tail_frames: plan.buses.nodes.iter().map(|n| n.tail_frames).collect(),
            filters: crate::dsp::svf::FilterBank::new(&plan.buses.filters, 0)?,
            parameters: crate::dsp::control::initial_parameters(plan, &plan.buses.parameters),
            mix: vec![BusMix::default(); plan.buses.len()].into_boxed_slice(),
            fader: plan.buses.nodes.iter().map(|n| n.fader).collect(),
            peaks: vec![[0.0; 2]; plan.buses.len()].into_boxed_slice(),
            input_peaks: vec![[0.0; 2]; plan.buses.len()].into_boxed_slice(),
            input_peak_enabled: vec![false; plan.buses.len()].into_boxed_slice(),
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
        let space = *graph.convolutions.get(slot).ok_or(Error::InvalidInput)?;
        let bus=graph.space_bus[space];
        let extra=graph.nodes[bus].signal.as_ref().map_or(0,|signal|signal.tail_frames);
        // Keep every admitted same-bus processor bound and admit the new
        // convolution plus this typed program's tail. Conservative high-water
        // scheduling does not recursively add tails or claim a shrinking bound.
        let tail=upload.tail.checked_add(extra).ok_or(Error::Capacity)?
            .max(self.tail_frames[bus]);
        self.spaces[space].swap_convolution(upload)?;
        self.tail_frames[bus] = tail;
        let buffer=&mut self.buffers[bus];
        if buffer.input_frames!=0 || buffer.remaining!=0 || buffer.state_dirty {
            buffer.remaining=buffer.remaining.max(tail);
        }
        Ok(())
    }
    pub fn begin(&mut self) {
        for (index,buffer) in self.buffers.iter_mut().enumerate() {
            if buffer.dirty {
                if let Some(channel)=self.channels.get_mut(index).and_then(Option::as_mut){channel.samples.fill(0.);}
                buffer.samples.fill([0.; 2]);
                buffer.dirty = false;
            }
            buffer.input_frames = 0;
        }
    }
    pub fn enable_input_peak(&mut self, bus: usize, enabled: bool) -> Result<(), Error> {
        let interest = self.input_peak_enabled.get_mut(bus).ok_or(Error::InvalidInput)?;
        if *interest != enabled {
            self.input_peaks[bus] = [0.; 2];
            *interest = enabled;
        }
        Ok(())
    }
    pub fn peak(&self, bus: usize, tap: BusPeakTap) -> Result<[f32; 2], Error> {
        match tap {
            BusPeakTap::Input => {
                if self.input_peak_enabled.get(bus) != Some(&true) {
                    return Err(Error::InvalidInput);
                }
                self.input_peaks.get(bus)
            }
            BusPeakTap::Output => self.peaks.get(bus),
        }.copied().ok_or(Error::InvalidInput)
    }
    pub fn take_input_peaks(&mut self, mut each: impl FnMut(usize, [f32; 2])) {
        for (bus, peak) in self.input_peaks.iter_mut().enumerate() {
            if self.input_peak_enabled[bus] {
                each(bus, std::mem::take(peak));
            }
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
        self.tone_ringing || self.buffers.iter().any(|b| b.remaining != 0)
    }
    /// Render scheduling includes one pending history reset; ownership still
    /// uses `active`, so a diagnostic reset does not extend a plan's tail.
    pub fn needs_render(&self) -> bool {
        self.tone_ringing || self.buffers.iter().any(|b| b.remaining != 0 || b.state_dirty)
    }
    pub fn reset(&mut self) {
        self.begin();
        self.cells.fill(ProcessorState::default());
        for channel in self.channels.iter_mut().flatten(){channel.reset();}
        self.tone_history = [[0.; 2]; 2];
        self.tone_ringing = false;
        self.spaces.iter_mut().for_each(crate::dsp::Space::clear);
        for buffer in &mut self.buffers {
            buffer.remaining = 0;
            buffer.state_dirty = false;
        }
    }
    pub(crate) fn add_channels(&mut self,graph:&PreparedBuses,bus:usize,input:&[f32],channels:usize,
        stride:usize,frames:usize,gain:f64)->Result<(),Error>{
        if frames>BLOCK || channels==0 || stride<frames || !gain.is_finite()
            || graph.input_channels(bus)?!=channels
            || channels.checked_sub(1).and_then(|n|n.checked_mul(stride)).and_then(|n|n.checked_add(frames)).is_none_or(|n|n>input.len()) {return Err(Error::InvalidInput);}
        if let Some(target)=self.channels.get_mut(bus).and_then(Option::as_mut){
            for channel in 0..channels{for frame in 0..frames{target.samples[channel*BLOCK+frame]+=(f64::from(input[channel*stride+frame])*gain)as f32;}}
        }else{
            if channels!=2{return Err(Error::UnsupportedSourceLayout);}
            let target=self.buffers.get_mut(bus).ok_or(Error::InvalidInput)?;
            for frame in 0..frames{for channel in 0..2{target.samples[frame][channel]+=(f64::from(input[channel*stride+frame])*gain)as f32;}}
        }
        self.fed(bus,frames);self.buffers[bus].dirty=true;Ok(())
    }

    fn send_channel_bus(&mut self,graph:&PreparedBuses,source:usize,target:usize,frames:usize,gain:f64,pre:bool)->Result<(),Error>{
        if source==target || !gain.is_finite() || frames>BLOCK{return Err(Error::InvalidInput);}
        let channels=if pre{graph.input_channels(source)?}else{graph.output_channels(source)?};
        if graph.input_channels(target)?!=channels{return Err(Error::UnsupportedSourceLayout);}
        let (source_channel,target_channel)=if source<target{let(a,b)=self.channels.split_at_mut(target);(a.get(source).and_then(Option::as_ref),b.first_mut().and_then(Option::as_mut))}
            else{let(a,b)=self.channels.split_at_mut(source);(b.first().and_then(Option::as_ref),a.get_mut(target).and_then(Option::as_mut))};
        let source_channel=source_channel.ok_or(Error::InvalidInput)?;
        let source_planes=if pre{&source_channel.pre}else{&source_channel.samples};
        if let Some(target_channel)=target_channel{for c in 0..channels{for f in 0..frames{target_channel.samples[c*BLOCK+f]+=(f64::from(source_planes[c*BLOCK+f])*gain)as f32;}}}
        else{if channels!=2{return Err(Error::UnsupportedSourceLayout);}let buffer=self.buffers.get_mut(target).ok_or(Error::InvalidInput)?;
            for f in 0..frames{for c in 0..2{buffer.samples[f][c]+=(f64::from(source_planes[c*BLOCK+f])*gain)as f32;}}}
        self.fed(target,frames);self.buffers[target].dirty=true;Ok(())
    }

    /// Runs the same compiled node kernels once on this global bus sum. Global
    /// tails/history survive individual NOTE_END and member-channel reuse.
    fn render_channel_bus<const TRACE:bool>(&mut self,graph:&PreparedBuses,index:usize,
        output:&mut[Frame],outs:&mut[&mut[Frame]],offset:usize,at:u64,tone_cutoff:Option<f64>,tempo:f64,
        mut trace:Option<(&crate::trace::TraceGraph,&mut crate::trace::Recorder)>)->u64{
        let node=&graph.nodes[index];let Some(signal)=node.signal.as_ref()else{return 1;};
        let buffer=&mut self.buffers[index];
        if buffer.input_frames==0 && buffer.remaining==0 && !buffer.state_dirty && graph.input!=Some(index){return 0;}
        let Some(channel)=self.channels[index].as_mut()else{return 1;};
        let len=output.len();let input=buffer.input_frames.min(len);
        if input>0{buffer.remaining=self.tail_frames[index];}
        let tail=buffer.remaining as usize;let mut produced=len.min(input.saturating_add(tail));
        buffer.remaining=tail.saturating_sub(produced.saturating_sub(input))as u32;
        channel.work.fill(0.);
        for c in 0..signal.input.get(){for f in 0..input{channel.work[c*BLOCK+f]=channel.samples[c*BLOCK+f];}}
        if signal.input.get()==2 {for f in 0..input{for c in 0..2{channel.work[c*BLOCK+f]+=buffer.samples[f][c];}}}
        channel.pre.fill(0.);for c in 0..signal.input.get(){channel.pre[c*BLOCK..c*BLOCK+input].copy_from_slice(&channel.work[c*BLOCK..c*BLOCK+input]);}
        if self.input_peak_enabled[index] && signal.input.get()==2 {
            for frame in 0..input{for c in 0..2{self.input_peaks[index][c]=self.input_peaks[index][c].max(channel.work[c*BLOCK+frame].abs());}}
        }
        if graph.input==Some(index){
            let cutoff=tone_cutoff.unwrap_or(20_000.);let ringing=channel.tone_history.iter().any(|pair|pair[0].iter().any(|v|v.abs()>1e-12));
            if cutoff<20_000. && (input>0||ringing){produced=len;}
            for pair in 0..channel.tones.len(){let mut frames=[[0.;2];BLOCK];
                for c in 0..2{let source=pair*2+c;if source<signal.input.get(){for f in 0..len{frames[f][c]=channel.work[source*BLOCK+f];}}}
                if channel.tones[pair].process(&mut frames[..len],&mut channel.tone_history[pair],cutoff,at).is_err(){self.cells[node.states.clone()].fill(ProcessorState::default());clear_owned_bus_spaces(&mut self.spaces,&graph.space_bus,index);channel.reset();buffer.remaining=0;buffer.state_dirty=false;return 1;}
                for c in 0..2{let target=pair*2+c;if target<signal.input.get(){for f in 0..len{channel.work[target*BLOCK+f]=frames[f][c];}}}
            }
            if produced==0{channel.tone_history.fill([[0.;2];2]);}
            self.tone_ringing=cutoff<20_000. && channel.tone_history.iter().any(|pair|pair[0].iter().any(|v|v.abs()>1e-12));
        }
        if produced==0{if buffer.state_dirty{channel.histories.fill(0.);self.cells[node.states.clone()].fill(ProcessorState::default());clear_owned_bus_spaces(&mut self.spaces,&graph.space_bus,index);buffer.state_dirty=false;}return 0;}
        let states=&mut self.cells[node.states.clone()];
        let mut chain=|planes:&mut[f32],frames:usize,stride:usize|->Result<(),Error>{
            if frames>BLOCK || stride<frames || stride.checked_add(frames).is_none_or(|end|end>planes.len()) {return Err(Error::InvalidInput);}
            // Same f32 -> f64 -> existing-kernel -> f32 boundary as the old
            // stereo bus loop, using this actual bus's sole state/Space owner.
            let mut block=[[0.;BLOCK];2];
            for c in 0..2{for frame in 0..frames{block[c][frame]=f64::from(planes[c*stride+frame]);}}
            let fault=crate::dsp::process::<TRACE>(node.processors.as_ref(),states,&mut block,frames,
                &self.parameters,at,&mut self.delay_samples,
                &mut crate::dsp::svf::FilterContext{tempo,bank:&mut self.filters,expression:None,spaces:&mut self.spaces},
                if TRACE{trace.as_mut().map(|(graph,recorder)|crate::trace::Section{recorder:&mut **recorder,graph,nodes:&graph.buses[index].stages,identity:Default::default()})}else{None});
            if fault || !crate::dsp::finite_states(&node.processors,states)
                || block.iter().any(|values|values[..frames].iter().any(|value|!(*value as f32).is_finite())) {
                states.fill(ProcessorState::default());clear_owned_bus_spaces(&mut self.spaces,&graph.space_bus,index);
                return Err(Error::InvalidInput);
            }
            for c in 0..2{for frame in 0..frames{let value=block[c][frame]as f32;planes[c*stride+frame]=if value.is_subnormal(){0.}else{value};}}
            Ok(())
        };
        let rendered=crate::topology::signal::render_program_with_chain(signal,&mut channel.work,&mut channel.histories,&mut channel.dry,
            &self.parameters,graph.rate,at,produced,&Default::default(),&mut chain);
        let position=match rendered {Ok(position)=>position,Err(_)=>{states.fill(ProcessorState::default());clear_owned_bus_spaces(&mut self.spaces,&graph.space_bus,index);channel.reset();buffer.remaining=0;buffer.state_dirty=false;return 1;}};
        if channel.work[position..].iter().take(signal.output.get()*BLOCK).any(|v|!v.is_finite())
            || channel.histories.iter().any(|v|!v.is_finite()){states.fill(ProcessorState::default());clear_owned_bus_spaces(&mut self.spaces,&graph.space_bus,index);channel.reset();buffer.remaining=0;buffer.state_dirty=false;return 1;}
        let mix=self.mix[index];
        if signal.output.get()!=2 && mix.gain[0]!=mix.gain[1]{states.fill(ProcessorState::default());clear_owned_bus_spaces(&mut self.spaces,&graph.space_bus,index);channel.reset();buffer.remaining=0;buffer.state_dirty=false;return 1;}
        for c in 0..signal.output.get(){let gain=mix.gain[if signal.output.get()==2{c}else{0}];for f in 0..produced{channel.samples[c*BLOCK+f]=channel.work[position+c*BLOCK+f]*gain;}}
        if signal.output.get()==2{
            for f in 0..produced{self.peaks[index][0]=self.peaks[index][0].max(channel.samples[f].abs());self.peaks[index][1]=self.peaks[index][1].max(channel.samples[BLOCK+f].abs());}
            if TRACE{if let Some((g,r))=trace.as_mut(){let mut block=[[0.;BLOCK];2];for c in 0..2{for f in 0..produced{block[c][f]=f64::from(channel.samples[c*BLOCK+f]);}}
                let id=g.buses[index].output;r.record(id,&block,&block,produced,[1.;2],true,Default::default(),&self.parameters,&g.nodes[id]);}}
        }
        buffer.state_dirty=true;buffer.dirty=true;
        if produced<len{channel.histories.fill(0.);states.fill(ProcessorState::default());clear_owned_bus_spaces(&mut self.spaces,&graph.space_bus,index);buffer.state_dirty=false;}
        // Only actual stereo host outputs need a fixed stereo copy. N-bus
        // fan-out borrows distinct existing generation-owned source/target planes.
        let width=signal.output.get();let mut emitted=[[0f32;2];BLOCK];let mut pre_emitted=[[0f32;2];BLOCK];
        if width==2{for f in 0..produced{emitted[f]=[channel.samples[f],channel.samples[BLOCK+f]];}}
        if signal.input.get()==2{for f in 0..input{pre_emitted[f]=[channel.pre[f],channel.pre[BLOCK+f]];}}
        let mut faults=0;
        for(n,send)in node.sends.iter().enumerate(){let pre=node.pre_sends.get(n).copied().unwrap_or(false);
            let produced=if pre{input}else{produced};let width=if pre{signal.input.get()}else{width};let emitted=if pre{&pre_emitted}else{&emitted};
            if produced==0{continue;}
            let gain=send.gain*if node.follows.get(n).copied().unwrap_or(false){self.fader[index]}else{1.};
            if let Some(direct)=mix.output.filter(|port|n==0&&*port<outs.len()){
                for f in 0..produced{for c in 0..2{outs[direct][offset+f][c]+=(f64::from(emitted[f][c])*gain)as f32;}}
            }else if let Some(target)=send.bus{if self.send_channel_bus(graph,index,target,produced,gain,pre).is_err(){faults+=1;}}
            else{if width!=2{faults+=1;continue;}for f in 0..produced{for c in 0..2{output[f][c]+=(f64::from(emitted[f][c])*gain)as f32;}}}
        }
        faults
    }

    /// `outs` are whole-render buffers; this block starts at `offset` in them.
    pub fn render<const TRACE: bool>(
        &mut self,
        graph: &PreparedBuses,
        output: &mut [Frame],
        outs: &mut [&mut [Frame]],
        offset: usize,
        at: u64,
        tone_cutoff: Option<f64>,
        tempo: f64,
        mut trace: Option<(&crate::trace::TraceGraph, &mut crate::trace::Recorder)>,
    ) -> u64 {
        let mut faults = 0;
        for &index in &graph.order {
            let node = &graph.nodes[index];
            if node.signal.is_some(){faults+=self.render_channel_bus::<TRACE>(graph,index,output,outs,offset,at,tone_cutoff,tempo,trace.as_mut().map(|(g,r)|(*g,&mut **r)));continue;}
            let buffer = &mut self.buffers[index];
            // Port v1 0cb7a8a0:src/fx/processor.rs::mix_buses sleeping early exit.
            // Retain v2's exact declared tail and the independent input-tone history;
            // no threshold, DSP law or control cadence is changed (NI signal-flow
            // manual; Kontakt 0x1408fa270 cadence remains owned by each processor).
            if buffer.input_frames == 0
                && buffer.remaining == 0
                && !buffer.state_dirty
                && graph.input != Some(index)
            {
                continue;
            }
            let states = &mut self.cells[node.states.clone()];
            // Input frames hold the tail at its maximum; it then counts down
            // over zero input until it ends, which clears the bus's history.
            let len = output.len();
            let input = buffer.input_frames.min(len);
            // Preserve the legacy path's copy cost when no pre-chain taps
            // are declared; the optional extra block is only for real taps.
            let pre_samples=if node.pre_sends.contains(&true){Some(buffer.samples)}else{None};
            if self.input_peak_enabled[index] {
                let peak = &mut self.input_peaks[index];
                for frame in &buffer.samples[..input] {
                    *peak = [peak[0].max(frame[0].abs()), peak[1].max(frame[1].abs())];
                }
            }
            let tail = if input > 0 {
                self.tail_frames[index]
            } else {
                buffer.remaining
            };
            let mut produced = input + (tail as usize).min(len - input);
            buffer.remaining = tail - (produced - input) as u32;
            if graph.input == Some(index) {
                let cutoff = tone_cutoff.unwrap_or(20_000.);
                // End IIR-only ringing below -240dB; never keep native FX awake indefinitely.
                if cutoff < 20_000.
                    && (input > 0 || self.tone_history[0].iter().any(|v| v.abs() > 1e-12))
                {
                    produced = len;
                }
                let before = if TRACE {
                    crate::trace::planar(&buffer.samples[..len])
                } else {
                    [[0.; BLOCK]; 2]
                };
                let _ = self.tone.as_mut().unwrap().process(
                    &mut buffer.samples[..len],
                    &mut self.tone_history,
                    cutoff,
                    at,
                );
                if cutoff < 20_000. {
                    buffer.dirty = true;
                }
                if produced == 0 {
                    self.tone_history = [[0.; 2]; 2];
                }
                self.tone_ringing =
                    cutoff < 20_000. && self.tone_history[0].iter().any(|v| v.abs() > 1e-12);
                if TRACE {
                    if let Some((g, r)) = trace.as_mut() {
                        let input_id = g.buses[index].input;
                        r.record(
                            input_id,
                            &before,
                            &before,
                            len,
                            [1.; 2],
                            true,
                            Default::default(),
                            &self.parameters,
                            &g.nodes[input_id],
                        );
                        if let Some(id) = g.buses[index].tone {
                            let after = crate::trace::planar(&buffer.samples[..len]);
                            r.record(
                                id,
                                &before,
                                &after,
                                len,
                                [1.; 2],
                                cutoff < 20_000.,
                                Default::default(),
                                &self.parameters,
                                &g.nodes[id],
                            );
                        }
                    }
                }
            }
            if produced > 0 {
                buffer.state_dirty = true;
                let mut block = [[0.; BLOCK]; 2];
                for (i, frame) in buffer.samples[..produced].iter().enumerate() {
                    block[0][i] = f64::from(frame[0]);
                    block[1][i] = f64::from(frame[1]);
                }
                if TRACE && graph.input != Some(index) {
                    if let Some((g, r)) = trace.as_mut() {
                        let id = g.buses[index].input;
                        r.record(
                            id,
                            &block,
                            &block,
                            produced,
                            [1.; 2],
                            true,
                            Default::default(),
                            &self.parameters,
                            &g.nodes[id],
                        );
                    }
                }
                let fault = crate::dsp::process::<TRACE>(
                    node.processors.as_ref(),
                    states,
                    &mut block,
                    produced,
                    &self.parameters,
                    at,
                    &mut self.delay_samples,
                    &mut crate::dsp::svf::FilterContext {
                        tempo: tempo,
                        bank: &mut self.filters,
                        expression: None,
                        spaces: &mut self.spaces,
                    },
                    if TRACE {
                        trace.as_mut().map(|(g, r)| crate::trace::Section {
                            recorder: &mut **r,
                            graph: g,
                            nodes: &g.buses[index].stages,
                            identity: Default::default(),
                        })
                    } else {
                        None
                    },
                );
                let finite = !fault
                    && block
                        .iter()
                        .all(|c| c[..produced].iter().all(|v| (*v as f32).is_finite()))
                    && crate::dsp::finite_states(&node.processors, states);
                if finite {
                    for (i, frame) in buffer.samples[..produced].iter_mut().enumerate() {
                        *frame = [block[0][i], block[1][i]]
                            .map(|v| v as f32)
                            .map(|v| if v.is_subnormal() { 0. } else { v });
                    }
                } else {
                    buffer.samples[..produced].fill([0.; 2]);
                    states.fill(ProcessorState::default());
                    clear_owned_bus_spaces(&mut self.spaces,&graph.space_bus,index);
                    faults += 1;
                }
            }
            // The tail ended inside this block: nothing carries over.
            if produced < len && buffer.state_dirty {
                buffer.state_dirty = false;
                #[cfg(test)]
                STATE_CLEARS.set(STATE_CLEARS.get() + states.len());
                states.fill(ProcessorState::default());
            }
            if produced == 0 {
                continue;
            }
            buffer.dirty = true;
            let before_mix = if TRACE {
                crate::trace::planar(&buffer.samples[..produced])
            } else {
                [[0.; BLOCK]; 2]
            };
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
            if TRACE {
                if let Some((g, r)) = trace.as_mut() {
                    let id = g.buses[index].output;
                    let block = crate::trace::planar(&buffer.samples[..produced]);
                    r.record(
                        id,
                        &before_mix,
                        &block,
                        produced,
                        mix.gain.map(f64::from),
                        true,
                        Default::default(),
                        &self.parameters,
                        &g.nodes[id],
                    );
                }
            }
            // Copy one bounded block so fan-out never aliases destination state.
            let samples = buffer.samples;
            for (n, send) in node.sends.iter().enumerate() {
                let pre=node.pre_sends.get(n).copied().unwrap_or(false);
                let samples=if pre{match pre_samples.as_ref(){Some(samples)=>samples,None=>{faults+=1;continue;}}}else{&samples};
                let produced=if pre{input}else{produced};
                if produced==0{continue;}
                // Out-of-range outputs fall back to the bus's own target.
                let direct = mix.output.filter(|&out| n == 0 && out < outs.len());
                let gain = if node.follows.get(n).copied().unwrap_or(false) {
                    send.gain * fader
                } else {
                    send.gain
                };
                if TRACE {
                    if let Some((g, r)) = trace.as_mut() {
                        let input = crate::trace::planar(&samples[..produced]);
                        let mut sent = input;
                        for channel in &mut sent {
                            for value in &mut channel[..produced] {
                                *value *= gain;
                            }
                        }
                        let id = g.buses[index].sends[n];
                        r.record(
                            id,
                            &input,
                            &sent,
                            produced,
                            [gain; 2],
                            gain != 0.,
                            crate::trace::TraceIdentity {
                                external_port: direct,
                                routed_to: direct
                                    .filter(|&p| p < crate::trace::HOST_PORTS)
                                    .map(|p| g.host[1 + p])
                                    .or_else(|| send.bus.map(|b| g.buses[b].input))
                                    .or(Some(g.master)),
                                ..Default::default()
                            },
                            &self.parameters,
                            &g.nodes[id],
                        );
                        if send.bus.is_none() && direct.is_none() {
                            r.record(
                                g.master,
                                &sent,
                                &sent,
                                produced,
                                [1.; 2],
                                true,
                                Default::default(),
                                &self.parameters,
                                &g.nodes[g.master],
                            );
                        }
                    }
                }
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
    /// Exact current builder count, no graph/storage allocation. All buses stay
    /// present even when no region feeding them is in diagnostic graph scope.
    pub(crate) fn trace_node_count(&self) -> Option<usize> {
        self.nodes.iter().enumerate().try_fold(0usize, |count, (bus, node)| {
            count.checked_add(2 + usize::from(self.input == Some(bus)))?
                .checked_add(node.processors.len())?.checked_add(node.sends.len())
        })
    }
    pub(crate) fn trace_graph(&self, plan: &Prepared, graph: &mut crate::trace::TraceGraph) {
        let initial = if graph.scope.is_some() { Box::new([]) as Box<[crate::dsp::ControlRamp]> }
            else { crate::dsp::control::initial_parameters(plan, &self.parameters) };
        for (bus, node) in self.nodes.iter().enumerate() {
            let group = plan
                .group_faders
                .iter()
                .position(|f| f.as_ref().is_some_and(|f| f.bus == bus))
                .map(|n| n as u32);
            let input = graph.node(
                if group.is_some() {
                    "group_bus"
                } else {
                    "bus_input"
                },
                "sum",
                None,
                group,
                Some(bus),
                vec![],
                0,
            );
            let stages: Vec<_> = node
                .processors
                .iter()
                .map(|s| {
                    graph.stage(
                        s.view(),
                        "bus_fx",
                        None,
                        group,
                        Some(bus),
                        plan,
                        &self.parameters,
                        &initial,
                    )
                })
                .collect();
            let native = stages
                .iter()
                .flat_map(|&id| graph.nodes[id].parameters.iter())
                .filter_map(|p| p.address)
                .find(|a| a.group == -1);
            let role = if plan
                .bus_addresses
                .iter()
                .any(|(a, b)| *b == bus && *a >= 1000)
            {
                "instrument_bus"
            } else if native.is_some_and(|a| a.generic == 0) {
                "send_return_rack"
            } else if native.is_some_and(|a| a.generic == 1) {
                "instrument_inserts"
            } else if native.is_some_and(|a| a.generic == 2) {
                "master_inserts"
            } else {
                "bus_output"
            };
            let output = graph.node(role, "fader_pan", None, group, Some(bus), vec![], 0);
            let tone = (self.input == Some(bus)).then(|| {
                let id = graph.node(
                    "part_tone",
                    "pre_insert_one_pole",
                    None,
                    group,
                    Some(bus),
                    vec![],
                    0,
                );
                graph.edge(input, id, "serial");
                id
            });
            let parent = graph.connect(node.processors.as_ref(), &stages, tone.unwrap_or(input));
            graph.edge(parent, output, "serial");
            let sends = node
                .sends
                .iter()
                .enumerate()
                .map(|(i, send)| {
                    graph.node(
                        "bus_send",
                        "send_gain",
                        None,
                        group,
                        Some(bus),
                        vec![
                            crate::trace::TraceParameter::constant("level", send.gain),
                            crate::trace::TraceParameter::constant(
                                "post_fader",
                                f64::from(node.follows.get(i).copied().unwrap_or(true)),
                            ),
                        ],
                        0,
                    )
                })
                .collect();
            graph.buses.push(crate::trace::BusNodes {
                input,
                output,
                tone,
                stages,
                sends,
            });
        }
        for (bus, node) in self.nodes.iter().enumerate() {
            for (n, send) in node.sends.iter().enumerate() {
                let id = graph.buses[bus].sends[n];
                graph.edge(graph.buses[bus].output, id, "tap");
                graph.edge(
                    id,
                    send.bus.map_or(graph.master, |b| graph.buses[b].input),
                    "send",
                );
                if n == 0 {
                    for port in 0..crate::trace::HOST_PORTS {
                        graph.edge(id, graph.host[1 + port], "possible_direct_route");
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod cpu_tests {
    use super::*;

    #[test]
    fn sleeping_bus_clears_once_and_wakes_with_identical_pcm() {
        let filter = crate::Biquad::new(48000, crate::FilterKind::LowPass, 12000., 1.).unwrap();
        let coefficients = filter.trace_coefficients();
        let plan = Prepared::new(48000, vec![], vec![], 0)
            .unwrap()
            .with_buses(
                vec![Bus {
                    processors: vec![Processor::Biquad(filter)],
                    sends: vec![BusSend {
                        bus: None,
                        gain: 1.,
                    }],
                    tail_frames: 3,
                }],
                vec![],
            )
            .unwrap();
        let mut state = BusState::new(&plan).unwrap();
        let step = |state: &mut BusState, input: bool, at: u64| {
            state.begin();
            if input {
                state.input(0, 1)[0] = [1., -0.5];
                state.fed(0, 1);
            }
            let mut output = [[0.; 2]; 64];
            assert_eq!(
                state.render::<false>(&plan.buses, &mut output, &mut [], 0, at, None, 120., None),
                0
            );
            output
        };
        STATE_CLEARS.set(0);
        let first = step(&mut state, true, 0);
        // Independently evaluate the same retained coefficients; bus output is f32.
        let [b0, b1, b2, a1, a2] = coefficients;
        let mut history = [0.; 2];
        for (i, frame) in first.iter().enumerate() {
            let expected = if i < 4 {
                let input = if i == 0 { 1. } else { 0. };
                let y = b0 * input + history[0];
                history = [b1 * input - a1 * y + history[1], b2 * input - a2 * y];
                [y as f32, (y * -0.5) as f32]
            } else {
                [0.; 2]
            };
            assert_eq!(*frame, expected, "frame {i}");
        }
        assert_eq!(STATE_CLEARS.get(), 1, "one reset when the tail ends");
        for i in 1..101 {
            assert_eq!(step(&mut state, false, i * 64), [[0.; 2]; 64]);
        }
        assert_eq!(
            STATE_CLEARS.get(),
            1,
            "sleeping buses must not repeatedly clear zero state"
        );
        assert_eq!(step(&mut state, true, 101 * 64), first, "wake is bit-exact");
        assert_eq!(STATE_CLEARS.get(), 2);
    }
}


#[cfg(test)]
mod channel_swap_scheduling_tests {
    use super::*;
    fn runtime()->crate::Runtime {
        let pcm=crate::Pcm::new(48000,vec![[0.25;2]].into_boxed_slice()).unwrap();let region=crate::Region{sample:0,key_low:60,key_high:60,root_key:None,velocity_low:0.,velocity_high:1.,gain:1.,envelope:Default::default(),playback:Default::default()};
        let mut h=vec![0.;513];h[400]=1.;let impulse=crate::Impulse::new(h.clone(),h).unwrap();let plan=crate::Prepared::new(48000,vec![pcm],vec![region],1).unwrap().with_impulses(vec![impulse]).with_buses(vec![Bus{processors:vec![crate::Processor::Space(sampler_ir::fx::space::Config::convolution(sampler_ir::ImpulseRef(0),0.,1.))],sends:vec![BusSend{bus:None,gain:1.}],tail_frames:513}],vec![Some(0)]).unwrap();let limits=crate::Limits::for_plan(&plan,4,8);crate::Runtime::new(plan,limits).unwrap()
    }
    #[test]
    fn idle_impulse_swap_does_not_create_remaining_work_or_owned_notes(){
        let mut rt=runtime();let impulse=crate::Impulse::new(vec![1.;2049],vec![1.;2049]).unwrap();let mut upload=crate::ConvolutionUpload::new(&impulse,0.,1.);let plan=rt.active_plan();
        rt.swap_convolution(0,&mut upload).unwrap();let state=&rt.plans.get(plan.0).unwrap().dsp.buses;assert_eq!(state.buffers[0].remaining,0);assert!(!state.active());
        let mut output=[[0.;2];256];rt.render(&mut output).unwrap();assert_eq!(output,[[0.;2];256]);let state=&rt.plans.get(plan.0).unwrap().dsp.buses;assert_eq!(state.buffers[0].remaining,0);assert!(!state.active());assert_eq!(rt.voice_count(),0);
    }
    #[test]
    fn shorter_impulse_swap_stops_old_space_history_without_replaying_it(){
        let mut rt=runtime();let input=crate::Input{protocol:crate::Protocol::Native,port:0,group:0,channel:0,key:60,external_id:None};rt.trigger(input,60,1.).unwrap();let mut first=[[0.;2];256];rt.render(&mut first).unwrap();assert_eq!(first,[[0.;2];256]);
        let impulse=crate::Impulse::new(vec![1.],vec![1.]).unwrap();let mut upload=crate::ConvolutionUpload::new(&impulse,0.,1.);let plan=rt.active_plan();let before=rt.plans.get(plan.0).unwrap().dsp.buses.buffers[0].remaining;assert!(before>0);
        // The existing partitioned convolver reserves two maximum stage blocks
        // after the impulse, even for a one-frame replacement. Drain that
        // independent declared bound rather than assuming a 1024-frame tail.
        let expected_tail = 1 + 2 * 8_192; // Existing MAX_STAGE_BLOCK policy.
        assert_eq!(upload.tail as usize, expected_tail);
        rt.swap_convolution(0, &mut upload).unwrap();
        assert_eq!(rt.plans.get(plan.0).unwrap().dsp.buses.buffers[0].remaining as usize, expected_tail.max(before as usize));
        let mut left = expected_tail.max(before as usize);
        while left > 0 {
            let count = left.min(crate::dsp::BLOCK);
            let mut output = [[0.; 2]; crate::dsp::BLOCK];
            rt.render(&mut output[..count]).unwrap();
            assert_eq!(output, [[0.; 2]; crate::dsp::BLOCK], "old impulse history must stay discarded");
            left -= count;
            assert_eq!(rt.plans.get(plan.0).unwrap().dsp.buses.buffers[0].remaining as usize, left);
        }
        assert!(!rt.plans.get(plan.0).unwrap().dsp.buses.active());
        assert_eq!(rt.voice_count(), 0);
    }
}
