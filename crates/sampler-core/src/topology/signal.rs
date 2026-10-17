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
pub(crate) enum Node {
    Matrix(Matrix),
    Stage(super::stages::Stage),
}
pub(crate) struct Signal {
    pub input: ChannelCount,
    pub max_channels: usize,
    pub nodes: Box<[Node]>,
    pub history_words: usize,
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
        for signal in self.processing_signals.iter().flatten() {
            for &(id, domain) in &signal.domains {
                if self.controls[self.control_index(id)?].domain != domain {
                    return Err(Error::InvalidInput);
                }
                self.real_control(id)?;
            }
        }
        Ok(())
    }
    pub(crate) fn validate_signal_execution(&self) -> Result<(), Error> {
        self.validate_source_ports()?;
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
            match definition.source_law {
                ir::ProcessingSourceLaw::CoreF64GainAmplitudeF32Outlet => {}
            }
            let input = ChannelCount::new(definition.input.channels)?;
            let mut previous = input;
            let mut maximum = input.get();
            let mut nodes = Vec::new();
            let mut histories = 0usize;
            let mut tail_frames = 0u32;
            let mut domains = Vec::new();
            for (node, operation) in definition.nodes.iter().enumerate() {
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
            if nodes.is_empty() || previous.get() != 2 {
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
            histories
                .checked_mul(size_of::<f64>())
                .ok_or(Error::Capacity)?;
            Ok(Some(Signal {
                input,
                max_channels: maximum,
                nodes: nodes.into_boxed_slice(),
                history_words: histories,
                tail_frames,
                domains: domains.into_boxed_slice(),
            }))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Vec::into_boxed_slice)
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
            let view = source.resident_view().ok_or(Error::NotReady)?;
            let channels = view.channels();
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
            let produced = v.cursor.render_channels_raw(
                &view,
                &mut output,
                &mut row[..channels.get()],
                &mut last[..channels.get()],
                &self.kernel,
            )?;
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
            for channel in 0..signal.input.get() {
                for frame in 0..frames {
                    let i = channel * crate::dsp::BLOCK + frame;
                    match generation.prepared.processing_scopes[owner.template].sum {
                        ir::ProcessingSumLaw::OrderedF32 => input[i] += member[i],
                    }
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
    let (mut input, mut output) = work.split_at_mut(width);
    let mut histories = generation.dsp.owner_signal_history.claim(index);
    let mut channels = signal.input.get();
    for node in &signal.nodes {
        match node {
            Node::Matrix(matrix) => {
                matrix.render(
                    input,
                    output,
                    frames,
                    crate::dsp::BLOCK,
                    &generation.dsp.parameters,
                    at,
                )?;
                channels = matrix.output.get();
                std::mem::swap(&mut input, &mut output);
            }
            Node::Stage(stage) => stage.render(
                input,
                channels,
                frames,
                crate::dsp::BLOCK,
                &mut histories,
                &generation.dsp.parameters,
                generation.prepared.rate,
                at,
                expression,
            )?,
        }
    }
    for (i, frame) in stereo.iter_mut().take(frames).enumerate() {
        *frame = [input[i], input[crate::dsp::BLOCK + i]];
    }
    Ok(())
}
