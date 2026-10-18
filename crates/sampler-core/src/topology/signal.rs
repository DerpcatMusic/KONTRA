//! Compiled explicit channel matrices, sharing the existing control value owner.
use crate::dsp::control::{ControlRamp, PreparedParameter};
use crate::{ChannelCount, ControlDomain, ControlId, ControlRange, Error, Prepared};
use sampler_ir as ir;

pub(crate) struct Matrix {
    pub input: ChannelCount,
    pub output: ChannelCount,
    cells: Box<[PreparedParameter]>,
    law: ir::MatrixArithmetic,
}
pub(crate) struct UviLayout {
    pub source:ir::UviNativeLayout,pub destination:ir::UviNativeLayout,
    pan:super::stages::StageParameter,offset:f64,
}
impl UviLayout {
    pub(crate) fn pan_control_lane(&self)->Option<usize>{if let PreparedParameter::Control(lane)=self.pan.base{Some(lane)}else{None}}
    fn render(&self,input:&[f32],output:&mut[f32],frames:usize,stride:usize,parameters:&[ControlRamp],at:u64)->Result<(),Error>{
        let ni=self.source.channels();let no=self.destination.channels();
        if frames>stride || input.len()<(ni-1)*stride+frames || output.len()<(no-1)*stride+frames{return Err(Error::InvalidInput);}
        for frame in 0..frames {
            // Native wide converter does NOT read this Pan operand. It remains
            // an actual compiled typed home; enhanced expression Pan is separate.
            let gains=if self.source==ir::UviNativeLayout::Code2&&self.destination==ir::UviNativeLayout::Code2 {
                ir::PanLaw::RaisedCosineCoreF64.gains((self.pan.base.value(parameters,at+frame as u64,None)+self.offset).clamp(-1.,1.))
            }else{[1.;2]};
            for row in 0..no {
                let mut sum=0.0f32;
                for column in 0..ni {
                    let weight=self.source.weight(self.destination,column,row).ok_or(Error::UnsupportedSourceLayout)?;
                    if weight!=0. {sum+=input[column*stride+frame]*weight;}
                }
                if self.source==ir::UviNativeLayout::Code2&&self.destination==ir::UviNativeLayout::Code2{sum=(f64::from(sum)*gains[row])as f32;}
                if !sum.is_finite(){return Err(Error::InvalidInput);}output[row*stride+frame]=sum;
            }
        }Ok(())
    }
}
pub(crate) enum Node {
    Matrix(Matrix),
    UviLayout(UviLayout),
    Stage(super::stages::Stage),
    Mix { count: usize, lanes: [usize; 3] },
    BusChain,
}
pub(crate) struct Signal {
    pub input: ChannelCount,
    pub output: ChannelCount,
    pub max_channels: usize,
    pub nodes: Box<[Node]>,
    pub history_words: usize,
    pub mix_depth: usize,
    pub expression_pan: bool,
    pub bus_chain: bool,
    pub tail_frames: u32,
    pub domains: Box<[(ControlId, ControlDomain)]>,
}

impl Prepared {
    pub(crate) fn signal_channels(&self) -> usize {
        self.processing_signals
            .iter()
            .flatten()
            .map(|s| s.max_channels)
            .max()
            .unwrap_or(0)
    }
    pub(crate) fn signal_mix_depth(&self) -> usize {
        self.processing_signals.iter().flatten().map(|s| s.mix_depth).max().unwrap_or(0)
    }
    pub(crate) fn signal_history_words(&self) -> usize {
        self.processing_signals
            .iter()
            .flatten()
            .map(|s| s.history_words)
            .max()
            .unwrap_or(0)
    }
    /// Shape validation is separate from completed DSP lane compilation. Builders
    /// can safely validate incoming metadata before allocating processor state.
    pub(crate) fn validate_processing_controls(&self) -> Result<(), Error> {
        for &(id,domain) in self.processing_signals.iter().flatten().flat_map(|signal|signal.domains.iter())
            .chain(self.buses.signal_domains()) {
            if self.controls[self.control_index(id)?].domain!=domain{return Err(Error::InvalidInput);}
            match domain {
                ControlDomain::Real{..}=>{self.real_control(id)?;}
                ControlDomain::Toggle=>{},
                _=>return Err(Error::InvalidInput),
            }
        }
        Ok(())
    }
    pub(crate) fn validate_signal_execution(&self) -> Result<(), Error> {
        self.validate_source_ports()?;
        self.validate_bus_signal_edges()?;
        if self
            .processing_scopes
            .iter()
            .enumerate()
            .any(|(index, scope)| {
                scope.signal.is_some()
                    && self
                        .processing_signals
                        .get(index)
                        .is_none_or(Option::is_none)
            })
        {
            return Err(Error::UnsupportedSourceLayout);
        }
        for (index, scope) in self.processing_scopes.iter().enumerate() {
            let Some(signal)=self.processing_signals.get(index).and_then(Option::as_ref) else {continue;};
            if signal.output.get()!=2 && (scope.bus.is_none() || scope.chain.is_some() || !signal.expression_pan) {
                // A wide outlet cannot run a second stereo chain or defer a
                // note's Pan until after different notes have been summed by
                // a global bus. The signal must retain its actual expression
                // home before an explicit authored expansion.
                return Err(Error::UnsupportedSourceLayout);
            }
        }
        self.validate_processing_controls()
    }
}

/// Called whenever the ordinary typed chain builder rebuilds parameter lanes.
/// Matrix lanes join that SAME final lane vector before its inverse index forms.
pub(crate) fn compile(
    plan: &Prepared,
    parameters: &mut Vec<ControlRange>,
) -> Result<Box<[Option<Signal>]>, Error> {
    if plan.processing_scopes.iter().all(|s| s.signal.is_none()) {
        return Ok(Box::new([]));
    }
    plan.validate_processing_controls()?;
    plan.processing_scopes
        .iter()
        .map(|scope| {
            let Some(definition) = &scope.signal else {
                return Ok(None);
            };
            compile_program(plan,definition,parameters,if scope.bus.is_some(){None}else{Some(ChannelCount::new(2)?)},true).map(Some)
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Vec::into_boxed_slice)
}

/// Same compiler for per-activation source scopes and global bus scopes.
/// A global sum has no canonical note owner: expression operands refuse.
pub(crate) fn compile_program(plan:&Prepared,definition:&ir::ProcessingSignal,
    parameters:&mut Vec<ControlRange>,output:Option<ChannelCount>,allow_expression:bool)->Result<Signal,Error> {
    match definition.source_law {
        ir::ProcessingSourceLaw::CoreF64GainAmplitudeF32Outlet | ir::ProcessingSourceLaw::UviC997Layout{..} => {}
    }
    // Shared normal lowering and public Core builders admit every binding
    // before lane compilation; ignored rows and last-writer-wins are invalid.
    let mut bound_cells=Vec::new();
    bound_cells.try_reserve_exact(definition.controls.len()).map_err(|_|Error::Capacity)?;
    for binding in &definition.controls {
        let Some(ir::ProcessingSignalNode::Matrix(matrix))=definition.nodes.get(binding.node) else {
            return Err(Error::InvalidInput);
        };
        if binding.input>=matrix.input.channels || binding.output>=matrix.output.channels {
            return Err(Error::InvalidInput);
        }
        bound_cells.push((binding.node,binding.input,binding.output));
    }
    bound_cells.sort_unstable();
    if bound_cells.windows(2).any(|pair|pair[0]==pair[1]) {return Err(Error::InvalidInput);}
    let input = ChannelCount::new(definition.input.channels)?;
    let mut previous = input;
    let mut maximum = input.get();
    let mut nodes = Vec::new();
    let mut histories = 0usize;
    let mut tail_frames = 0u32;
    let mut domains = Vec::new();
    let mut spans: Vec<(usize, ChannelCount)> = Vec::new();
    let mut mix_depth = 0;
    let mut expression_pan=false;
    let mut bus_chain=false;
    for (node, operation) in definition.nodes.iter().enumerate() {
        if !allow_expression && match operation {
            ir::ProcessingSignalNode::Gain(gain)=>gain.gain.expression.is_some(),
            ir::ProcessingSignalNode::OnePole(filter)=>filter.cutoff_hz.expression.is_some(),
            ir::ProcessingSignalNode::UviOnePole(filter)=>filter.frequency.expression.is_some() || filter.tracking!=0.,
            ir::ProcessingSignalNode::Pan(pan)=>pan.expression || pan.position.expression.is_some(),
            _=>false,
        } {return Err(Error::InvalidInput);}
        while spans.last().is_some_and(|&(end, _)| end == node) {
            let (_, expected) = spans.pop().ok_or(Error::InvalidInput)?;
            if previous != expected { return Err(Error::InvalidInput); }
        }
        if let ir::ProcessingSignalNode::UviLayout(layout)=operation {
            if previous.get()!=layout.source.channels() || !layout.pan_offset.is_finite() || layout.pan.expression.is_some()
                || layout.source.weight(layout.destination,0,0).is_none() {return Err(Error::InvalidInput);}
            let pan=super::stages::parameter(plan,&layout.pan,parameters,&mut domains,|v|v.is_finite()&&(-1. ..=1.).contains(&v),false)?;
            nodes.push(Node::UviLayout(UviLayout{source:layout.source,destination:layout.destination,pan,offset:layout.pan_offset}));
            previous=ChannelCount::new(layout.destination.channels())?;maximum=maximum.max(previous.get());continue;
        }
        if matches!(operation,ir::ProcessingSignalNode::BusChain) {
            if allow_expression || bus_chain || previous.get()!=2 || !spans.is_empty() {
                return Err(Error::InvalidInput);
            }
            bus_chain=true;
            nodes.push(Node::BusChain);
            continue;
        }
        if let ir::ProcessingSignalNode::Mix(mix) = operation {
            let count = usize::from(mix.count);
            let end = node.checked_add(1).and_then(|index| index.checked_add(count)).ok_or(Error::Capacity)?;
            if end > definition.nodes.len() || spans.len() == 16
                || spans.last().is_some_and(|&(parent,_)| end > parent) {
                return Err(Error::InvalidInput);
            }
            let mut lanes = [0; 3];
            for (index, kind) in [crate::SlotKind::Dry, crate::SlotKind::Output, crate::SlotKind::Bypass].into_iter().enumerate() {
                let id = crate::slot_control(kind, mix.address.group, mix.address.slot, mix.address.generic);
                let domain = plan.controls[plan.control_index(id)?].domain;
                if domain != (ControlDomain::Real { min: 0., max: kind.max() }) { return Err(Error::InvalidInput); }
                plan.real_control(id)?;
                lanes[index] = parameters.len();
                parameters.push(ControlRange { control:id, low:0., high:kind.max(), ramp_frames:plan.rate/100 });
                domains.push((id,domain));
            }
            spans.push((end, previous));
            mix_depth = mix_depth.max(spans.len());
            nodes.push(Node::Mix { count, lanes });
            continue;
        }
        if matches!(operation,ir::ProcessingSignalNode::Pan(ir::ChannelPan{expression:true,..})) {
            if expression_pan || !spans.is_empty() {return Err(Error::InvalidInput);}
            expression_pan=true;
        }
        let ir::ProcessingSignalNode::Matrix(matrix) = operation else {
            let (stage, tail) = super::stages::Stage::compile(
                plan,
                operation,
                previous.get(),
                &mut histories,
                parameters,
                &mut domains,
            )?;
            tail_frames = tail_frames.checked_add(tail).ok_or(Error::Capacity)?;
            nodes.push(Node::Stage(stage));
            continue;
        };
        let width = ChannelCount::new(matrix.input.channels)?;
        let output = ChannelCount::new(matrix.output.channels)?;
        if width != previous
            || width.get().checked_mul(output.get()) != Some(matrix.cells.len())
        {
            return Err(Error::InvalidInput);
        }
        let mut cells = Vec::new();
        cells
            .try_reserve_exact(matrix.cells.len())
            .map_err(|_| Error::Capacity)?;
        for &value in &matrix.cells {
            if !value.is_finite() || value.abs() > f64::from(f32::MAX) {
                return Err(Error::InvalidInput);
            }
            cells.push(PreparedParameter::Constant(value));
        }
        for binding in definition.controls.iter().filter(|b| b.node == node) {
            let index = binding
                .output
                .checked_mul(width.get())
                .and_then(|i| i.checked_add(binding.input))
                .ok_or(Error::Capacity)?;
            if binding.input >= width.get() || binding.output >= output.get() {
                return Err(Error::InvalidInput);
            }
            let id = crate::lower::ir_control_id(&binding.control);
            let logical = plan.control_index(id)?;
            let domain = plan.controls[logical].domain;
            let ControlDomain::Real { min, max } = domain else {
                return Err(Error::InvalidInput);
            };
            if min.abs() > f64::from(f32::MAX) || max.abs() > f64::from(f32::MAX) {
                return Err(Error::InvalidInput);
            }
            plan.real_control(id)?;
            let lane = parameters.len();
            parameters.push(ControlRange {
                control: id,
                low: min,
                high: max,
                ramp_frames: 0,
            });
            *cells.get_mut(index).ok_or(Error::InvalidInput)? =
                PreparedParameter::Control(lane);
            domains.push((id, domain));
        }
        maximum = maximum.max(output.get());
        nodes.push(Node::Matrix(Matrix {
            input: width,
            output,
            cells: cells.into_boxed_slice(),
            law: matrix.arithmetic,
        }));
        previous = output;
    }
    while let Some((end, expected)) = spans.pop() {
        if end != nodes.len() || previous != expected { return Err(Error::InvalidInput); }
    }
    if nodes.is_empty() || output.is_some_and(|output|previous != output) {
        return Err(Error::UnsupportedSourceLayout);
    }
    // All scratch/last count multiplication is admitted before DSP allocation.
    maximum
        .checked_mul(3 * crate::dsp::BLOCK + 2)
        .and_then(|n| n.checked_mul(size_of::<f32>()))
        .and_then(|bytes| {
            maximum
                .checked_mul(size_of::<[f64; 2]>())
                .and_then(|tone| bytes.checked_add(tone))
        })
        .ok_or(Error::Capacity)?;
    maximum.checked_mul(crate::dsp::BLOCK).and_then(|n| n.checked_mul(mix_depth))
        .and_then(|n| n.checked_mul(size_of::<f32>())).ok_or(Error::Capacity)?;
    histories
        .checked_mul(size_of::<f64>())
        .ok_or(Error::Capacity)?;
    Ok(Signal {
        input,
        output:previous,
        max_channels: maximum,
        nodes: nodes.into_boxed_slice(),
        history_words: histories,
        mix_depth,
        expression_pan,
        bus_chain,
        tail_frames,
        domains: domains.into_boxed_slice(),
    })
}

impl Matrix {
    pub(crate) fn render(
        &self,
        input: &[f32],
        output: &mut [f32],
        frames: usize,
        stride: usize,
        parameters: &[ControlRamp],
        at: u64,
    ) -> Result<(), Error> {
        let input_len = (self.input.get() - 1)
            .checked_mul(stride)
            .and_then(|n| n.checked_add(frames))
            .ok_or(Error::Capacity)?;
        let output_len = (self.output.get() - 1)
            .checked_mul(stride)
            .and_then(|n| n.checked_add(frames))
            .ok_or(Error::Capacity)?;
        if frames > stride || input.len() < input_len || output.len() < output_len {
            return Err(Error::InvalidInput);
        }
        at.checked_add(frames.saturating_sub(1) as u64)
            .ok_or(Error::ClockOverflow)?;
        match self.law {
            ir::MatrixArithmetic::SeparateMulAddF32 => {}
        }
        for row in 0..self.output.get() {
            for frame in 0..frames {
                let mut sum = 0.0f32;
                for column in 0..self.input.get() {
                    let coefficient = self.cells[row * self.input.get() + column].value(
                        parameters,
                        at + frame as u64,
                        None,
                    ) as f32;
                    let product = input[column * stride + frame] * coefficient;
                    sum += product;
                }
                if !sum.is_finite() {
                    return Err(Error::InvalidInput);
                }
                output[row * stride + frame] = sum;
            }
        }
        Ok(())
    }
}

impl crate::Runtime {
    /// One source writes its own preallocated member outlet before any owner
    /// sum scratch is borrowed. Source retirement cannot free that generation.
    fn render_signal_member(
        &mut self,
        plan_id: crate::PlanId,
        voice: crate::VoiceId,
        frames: usize,
        at: u64,
    ) -> Result<usize, Error> {
        let Some(value) = self.voices.get(voice.0) else {
            return Ok(0);
        };
        let source = self
            .plans
            .get(plan_id.0)
            .ok_or(Error::StaleHandle)?
            .source(value.sample)
            .ok_or(Error::NotReady)?;
        let stereo = source.stereo().is_some();
        let channel_count = source.channels().get();
        if !stereo && value.cursor.pending_start_runway() > 1 {
            // Ordinary render_source observes the same cohort decision. A
            // current complete row is not admission of the required future
            // runway or a missing sibling in the note's startup cohort.
            return Ok(0);
        }
        if stereo {
            let mut output = [[0.; 2]; crate::dsp::BLOCK];
            let produced = self
                .render_source_voice::<false>(voice.0.index, &mut output[..frames], at, true)
                .unwrap_or(0);
            let generation = self.plans.get(plan_id.0).ok_or(Error::StaleHandle)?;
            let mut block = generation.dsp.channel_sources.claim(voice.0.index);
            block.fill(0.);
            for (i, frame) in output.iter().take(frames).enumerate() {
                block[i] = frame[0];
                block[crate::dsp::BLOCK + i] = frame[1];
            }
            return Ok(produced);
        }
        let Some(prelude) = self.prepare_voice(voice.0.index, at, frames) else {
            return Ok(0);
        };
        // Legacy source pan is a two-channel operation. Its mutation entrypoints
        // must reject an unmatched wide layout before changing retained values.
        if channel_count != 2
            && prelude.points.is_some_and(|r| {
                r.from.gains[0] != r.from.gains[1] || r.to.gains[0] != r.to.gains[1]
            })
        {
            return Err(Error::UnsupportedSourceLayout);
        }
        let (produced, done) = {
            let v = self.voices.get_mut(voice.0).ok_or(Error::StaleHandle)?;
            let generation = self.plans.get_mut(plan_id.0).ok_or(Error::StaleHandle)?;
            let source = generation.source(v.sample).ok_or(Error::NotReady)?;
            let channels = source.channels();
            let mut heads = generation.dsp.channel_heads.claim(voice.0.index);
            let count = frames.min(v.envelope.remaining());
            let mut block = generation.dsp.channel_sources.claim(voice.0.index);
            let mut row = generation.dsp.channel_row.claim(0);
            let mut last = generation.dsp.channel_last.claim(voice.0.index);
            let mut tone = generation.dsp.channel_tone.claim(voice.0.index);
            block.fill(0.);
            let mut output = crate::source::channels::ChannelBlock::new(
                &mut block,
                channels,
                count,
                crate::dsp::BLOCK,
            )?;
            let was_starved = v.cursor.starved();
            let produced = match source {
                crate::SourceStorageRef::Channels(family) if family.is_paged() => {
                    let cache = self.stream_cache.as_ref().ok_or(Error::NotReady)?;
                    if heads.len() < family.packs().len() { return Err(Error::Capacity); }
                    if heads[..family.packs().len()].iter().any(Option::is_none) {
                        // No retained cohort prepass for this source. Pin all
                        // current heads together; never mix an admitted old
                        // member publication with a newly acquired sibling.
                        heads.fill_with(|| None);
                        for (slot, pack) in heads.iter_mut().zip(family.packs()) { *slot = Some(pack.pin_head()); }
                    }
                    let view = crate::source::channels::PagedChannels {
                        family, heads: &heads[..family.packs().len()], cache: cache.reader(),
                    };
                    v.cursor.render_channels_raw(&view, &mut output, &mut row[..channels.get()],
                        &mut last[..channels.get()], &self.kernel)?
                }
                _ => {
                    let view = source.resident_view().ok_or(Error::NotReady)?;
                    v.cursor.render_channels_raw(&view, &mut output, &mut row[..channels.get()],
                        &mut last[..channels.get()], &self.kernel)?
                }
            };
            heads.fill_with(|| None);
            self.stream_underruns = self.stream_underruns.saturating_add(u64::from(!was_starved && v.cursor.starved()));
            let levels = crate::dsp::levels(v, produced, 0);
            let ramp = prelude.points.map(|r| r.prepare_gains());
            let gains = v
                .chain
                .map(|chain| {
                    generation
                        .prepared
                        .wide_gain_sections
                        .get(chain)
                        .and_then(Option::as_ref)
                        .ok_or(Error::UnsupportedSourceLayout)
                })
                .transpose()?;
            for channel in 0..channels.get() {
                for frame in 0..produced {
                    let mut value = f64::from(output.values[channel * output.stride + frame]);
                    if let Some(gains) = gains {
                        for gain in &gains.pre {
                            value *= gain;
                        }
                    }
                    let modifier = ramp.as_ref().map_or(1., |r| {
                        r.at(at + frame as u64 + 1)[if channels.get() == 2 { channel } else { 0 }]
                    });
                    value *= levels[frame] * f64::from(modifier);
                    if let Some(gains) = gains {
                        for gain in &gains.post {
                            value *= gain;
                        }
                    }
                    let value = value as f32;
                    if !value.is_finite() {
                        return Err(Error::InvalidInput);
                    }
                    output.values[channel * output.stride + frame] = value;
                }
                if prelude.modulated {
                    let amount = prelude
                        .points
                        .map_or(0., |r| (r.from.tone + r.to.tone) * 0.5);
                    // Match the legacy source outlet store before Tone. Tone
                    // runs across the complete segment, including a zero suffix.
                    crate::voice_mod::process_channel_tone(
                        &mut output.values
                            [channel * output.stride..channel * output.stride + frames],
                        &mut tone[channel],
                        amount,
                        f64::from(self.rate),
                    );
                }
            }
            v.last_gains = [v.gain; 2];
            (
                produced,
                v.cursor.done() || v.envelope.done() || prelude.stop,
            )
        };
        if done {
            self.end_voice(voice);
        }
        Ok(produced)
    }

    pub(crate) fn render_signal_sources(
        &mut self,
        owner: &super::ProcessingOwner,
        frames: usize,
        at: u64,
    ) -> Result<usize, Error> {
        let mut produced = [0usize; super::MAX_MEMBERS];
        for (ordinal, voice) in owner.members.iter().enumerate() {
            if let Some(voice) = voice {
                produced[ordinal] = self.render_signal_member(owner.plan, *voice, frames, at)?;
            }
        }
        let generation = self.plans.get(owner.plan.0).ok_or(Error::StaleHandle)?;
        let signal = generation
            .prepared
            .processing_signals
            .get(owner.template)
            .and_then(Option::as_ref)
            .ok_or(Error::UnsupportedSourceLayout)?;
        let width = signal
            .max_channels
            .checked_mul(crate::dsp::BLOCK)
            .ok_or(Error::Capacity)?;
        let mut work = generation.dsp.channel_work.claim(0);
        let (input, output) = work.split_at_mut(width);
        input.fill(0.);
        output.fill(0.);
        for (ordinal, voice) in owner.members.iter().enumerate() {
            if produced[ordinal] == 0 {
                continue;
            }
            let Some(voice) = voice else {
                continue;
            };
            let member = generation.dsp.channel_sources.claim(voice.0.index);
            let scope=&generation.prepared.processing_scopes[owner.template];
            let native=match scope.signal.as_ref().map(|definition|&definition.source_law){
                Some(ir::ProcessingSourceLaw::UviC997Layout{members})=>Some(members.get(ordinal).copied().flatten().ok_or(Error::UnsupportedSourceLayout)?),
                _=>None,
            };
            for channel in 0..signal.input.get() {
                for frame in 0..frames {
                    let i = channel * crate::dsp::BLOCK + frame;
                    if let Some(native)=native {
                        let destination=ir::UviNativeLayout::plain_header(signal.input.get()).ok_or(Error::UnsupportedSourceLayout)?;
                        for original in 0..native.layout.channels(){
                            let weight=native.layout.weight(destination,original,channel).ok_or(Error::UnsupportedSourceLayout)?;
                            if weight!=0.{input[i]+=member[original*crate::dsp::BLOCK+frame]*weight;}
                        }
                    }else{match scope.sum {ir::ProcessingSumLaw::OrderedF32=>input[i]+=member[i]}}
                }
            }
        }
        Ok(produced.into_iter().max().unwrap_or(0))
    }
}

/// Runs only after the owner has decided the exact source/tail length. Source
/// gathering zero-fills both halves even with no members; no cursor advances here.
pub(crate) fn render_stages(
    generation: &crate::plans::Generation,
    owner: &super::ProcessingOwner,
    index: usize,
    frames: usize,
    at: u64,
    stereo: &mut [crate::Frame],
    expression: &crate::Expression,
    native_note:f64,
) -> Result<(), Error> {
    let signal = generation
        .prepared
        .processing_signals
        .get(owner.template)
        .and_then(Option::as_ref)
        .ok_or(Error::UnsupportedSourceLayout)?;
    let width = signal
        .max_channels
        .checked_mul(crate::dsp::BLOCK)
        .ok_or(Error::Capacity)?;
    let mut work = generation.dsp.channel_work.claim(0);
    let mut histories = generation.dsp.owner_signal_history.claim(index);
    let mut dry = generation.dsp.channel_mix.claim(0);
    for node in &signal.nodes{if let Node::Stage(stage)=node{stage.prepare_native_note(&mut histories,native_note)?;}}
    if signal.output.get()!=2 {return Err(Error::UnsupportedSourceLayout);}
    let projected=generation.dsp.modulated_parameters.claim(0);
    let offset=render_program(signal,&mut work,&mut histories,&mut dry,
        if generation.dsp.owner_modulation.program(index).is_some(){&projected}else{&generation.dsp.parameters},generation.prepared.rate,at,frames,expression)?;
    for (i, frame) in stereo.iter_mut().take(frames).enumerate() {
        *frame = [work[offset+i], work[offset+crate::dsp::BLOCK+i]];
    }
    Ok(())
}


/// Deliver a compiled wide activation into its actual global bus. The source
/// clock has already advanced once; only this activation's envelope advances
/// here. No global owner evaluates a per-note expression after note summation.
pub(crate) fn render_wide_stages(
    generation:&mut crate::plans::Generation,
    owner:&mut super::ProcessingOwner,index:usize,frames:usize,at:u64,
    expression:&crate::Expression,expression_gain:f32,
)->Result<(),Error>{
    let scope=&generation.prepared.processing_scopes[owner.template];
    let bus=scope.bus.ok_or(Error::UnsupportedSourceLayout)?;
    let signal=generation.prepared.processing_signals.get(owner.template)
        .and_then(Option::as_ref).ok_or(Error::UnsupportedSourceLayout)?;
    if signal.output.get()==2 || scope.chain.is_some() || !signal.expression_pan {
        return Err(Error::UnsupportedSourceLayout);
    }
    let mut work=generation.dsp.channel_work.claim(0);
    let mut histories=generation.dsp.owner_signal_history.claim(index);
    let mut dry=generation.dsp.channel_mix.claim(0);
    let projected=generation.dsp.modulated_parameters.claim(0);
    let offset=render_program(signal,&mut work,&mut histories,&mut dry,
        if generation.dsp.owner_modulation.program(index).is_some(){&projected}else{&generation.dsp.parameters},generation.prepared.rate,at,frames,expression)?;
    for frame in 0..frames {
        let level=owner.envelope.constant_level().unwrap_or_else(||owner.envelope.next());
        let gain=level*expression_gain;
        for channel in 0..signal.output.get(){
            let value=&mut work[offset+channel*crate::dsp::BLOCK+frame];
            *value*=gain;
            if !value.is_finite(){return Err(Error::InvalidInput);}
        }
    }
    generation.dsp.buses.add_channels(&generation.prepared.buses,bus,&work[offset..],
        signal.output.get(),crate::dsp::BLOCK,frames,1.)
}

pub(crate) fn render_program(signal:&Signal,work:&mut[f32],histories:&mut[f64],
    dry:&mut[f32],parameters:&[ControlRamp],rate:u32,at:u64,frames:usize,
    expression:&crate::Expression)->Result<usize,Error> {
    let mut reject=|_:&mut[f32],_:usize,_:usize|Err(Error::UnsupportedSourceLayout);
    render_program_with_chain(signal,work,histories,dry,parameters,rate,at,frames,expression,&mut reject)
}

pub(crate) fn render_program_with_chain<F>(signal:&Signal,work:&mut[f32],histories:&mut[f64],
    dry:&mut[f32],parameters:&[ControlRamp],rate:u32,at:u64,frames:usize,
    expression:&crate::Expression,chain:&mut F)->Result<usize,Error>
where F:FnMut(&mut[f32],usize,usize)->Result<(),Error> {
    let width=signal.max_channels.checked_mul(crate::dsp::BLOCK).ok_or(Error::Capacity)?;
    if frames>crate::dsp::BLOCK || work.len()<width.checked_mul(2).ok_or(Error::Capacity)?
        || histories.len()<signal.history_words || dry.len()<width.checked_mul(signal.mix_depth).ok_or(Error::Capacity)? {
        return Err(Error::InvalidInput);
    }
    let mut alternate=false;
    let channels=SignalRenderer{signal,work,histories,parameters,rate,at,expression,frames,width,chain}
        .range(0,signal.nodes.len(),&mut alternate,signal.input.get(),dry)?;
    if channels!=signal.output.get(){return Err(Error::InvalidInput);}
    Ok(if alternate{width}else{0})
}


/// At most sixteen nested spans, admitted at compile time. Scratch belongs to
/// the existing serial signal render owner; no per-block allocation or cache.
struct SignalRenderer<'a,F> {
    signal:&'a Signal, work:&'a mut [f32], histories:&'a mut [f64],
    parameters:&'a [ControlRamp], rate:u32, at:u64, expression:&'a crate::Expression,
    frames:usize, width:usize,chain:&'a mut F,
}
impl<F> SignalRenderer<'_,F> where F:FnMut(&mut[f32],usize,usize)->Result<(),Error> {
fn range(&mut self, begin:usize, end:usize, alternate:&mut bool,
    mut channels:usize, scratch:&mut [f32]) -> Result<usize,Error> {
    let stride=crate::dsp::BLOCK;
    let mut index=begin;
    while index < end {
        match &self.signal.nodes[index] {
            Node::Matrix(matrix) => {
                let (first,second)=self.work.split_at_mut(self.width);
                if *alternate { matrix.render(second,first,self.frames,stride,self.parameters,self.at)?; }
                else { matrix.render(first,second,self.frames,stride,self.parameters,self.at)?; }
                *alternate=!*alternate;
                channels=matrix.output.get();
                index+=1;
            }
            Node::UviLayout(layout)=>{
                if channels!=layout.source.channels(){return Err(Error::InvalidInput);}
                let(first,second)=self.work.split_at_mut(self.width);
                if *alternate{layout.render(second,first,self.frames,stride,self.parameters,self.at)?;}
                else{layout.render(first,second,self.frames,stride,self.parameters,self.at)?;}
                *alternate=!*alternate;channels=layout.destination.channels();index+=1;
            }
            Node::BusChain => {
                if channels!=2{return Err(Error::InvalidInput);}
                let offset=if *alternate{self.width}else{0};
                (self.chain)(&mut self.work[offset..offset+self.width],self.frames,stride)?;
                index+=1;
            }
            Node::Stage(stage) => {
                let offset=if *alternate {self.width}else{0};
                stage.render(&mut self.work[offset..offset+self.width],channels,self.frames,stride,self.histories,self.parameters,self.rate,self.at,self.expression)?;
                index+=1;
            }
            Node::Mix {count,lanes} => {
                let count=*count;
                let lanes=*lanes;
                let inner=index+1;
                let after=inner.checked_add(count).ok_or(Error::Capacity)?;
                if after > end || scratch.len() < self.width {return Err(Error::InvalidInput);}
                let (dry,remaining)=scratch.split_at_mut(self.width);
                let offset=if *alternate {self.width}else{0};
                dry.copy_from_slice(&self.work[offset..offset+self.width]);
                let [direct,wet,bypass]=lanes.map(|lane| self.parameters[lane]);
                let last=self.at+self.frames.saturating_sub(1) as u64;
                let off=bypass.value(self.at)>=1. && bypass.value(last)>=1.;
                if !off && self.range(inner,after,alternate,channels,remaining)? != channels {
                    return Err(Error::InvalidInput);
                }
                let offset=if *alternate {self.width}else{0};
                for frame in 0..self.frames {
                    let time=self.at+frame as u64;
                    let b=bypass.value(time);
                    let d=direct.value(time)*(1.-b)+b;
                    let w=wet.value(time)*(1.-b);
                    for channel in 0..channels {
                        let i=channel*stride+frame;
                        let processed=if off {0.}else{w*f64::from(self.work[offset+i])};
                        let value=(d*f64::from(dry[i])+processed) as f32;
                        if !value.is_finite(){return Err(Error::InvalidInput);}
                        self.work[offset+i]=value;
                    }
                }
                index=after;
            }
        }
    }
    Ok(channels)
}
}
