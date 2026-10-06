//! Sample-time segmentation and deterministic voice rendering.
use super::{Error, Frame, Runtime, VoiceId};

impl Runtime {
    /// Events at the exclusive block end stay pending until the next render (including
    /// an empty block). Overflow is rejected before any output/state mutation.
    pub fn render(&mut self, output: &mut [Frame]) -> Result<(), Error> {
        let end = self
            .now
            .checked_add(output.len() as u64)
            .ok_or(Error::ClockOverflow)?;
        output.fill([0.0; 2]);
        self.apply_due();
        let mut offset = 0;
        while self.now < end {
            self.apply_due();
            let boundary = self.commands.first().map_or(end, |c| c.at.min(end));
            let len = (boundary - self.now) as usize;
            let segment = &mut output[offset..offset + len];
            self.render_segment(segment);
            for frame in segment {
                if !frame.iter().all(|x| x.is_finite()) {
                    *frame = [0.0; 2];
                    self.nonfinite_frames = self.nonfinite_frames.saturating_add(1);
                }
            }
            self.now = boundary;
            offset += len;
        }
        debug_assert_eq!(self.now, end);
        Ok(())
    }

    fn render_segment(&mut self, output: &mut [Frame]) {
        let chunked = self.plans.slots.iter().any(|s| {
            s.value
                .as_ref()
                .is_some_and(|g| g.prepared.buses.len() != 0 || !g.dsp.filters.is_empty())
        });
        if !chunked {
            self.render_voices(output, self.now);
            return;
        }
        for (chunk, output) in output.chunks_mut(super::dsp::BLOCK).enumerate() {
            let at = self.now + (chunk * super::dsp::BLOCK) as u64;
            for g in self.plans.slots.iter_mut().filter_map(|s| s.value.as_mut()) {
                g.dsp.buses.begin();
            }
            self.render_voices(output, at);
            for g in self.plans.slots.iter_mut().filter_map(|s| s.value.as_mut()) {
                let faults = g.dsp.buses.render(&g.prepared.buses, output, at);
                self.nonfinite_frames = self.nonfinite_frames.saturating_add(faults);
            }
        }
    }

    fn render_voices(&mut self, output: &mut [Frame], at: u64) {
        // Skip empty slots a word at a time. Ascending set bits preserve the
        // original slot-order sum even after holes and generational slot reuse.
        for word in 0..self.voice_activity.len() {
            let mut occupied = self.voice_activity[word];
            while occupied != 0 {
                let begin = occupied.trailing_zeros() as usize;
                let end = begin + (occupied >> begin).trailing_ones() as usize;
                occupied = if end == 64 {
                    0
                } else {
                    occupied & (u64::MAX << end)
                };
                // Dense runs retain the simple contiguous slot loop; sparse
                // pools skip the untouched Voice storage between those runs.
                for i in word * 64 + begin..word * 64 + end {
                    self.render_voice(i, output, at);
                }
            }
        }
    }

    #[inline]
    fn render_voice(&mut self, i: usize, segment: &mut [Frame], at: u64) {
        let Some(v) = &mut self.voices.slots[i].value else {
            return;
        };
        if !v.started {
            return;
        }
        // Retention invariant: live voice -> counted family -> counted note
        // -> expression owner. Each owner retires only after its dependents.
        let f = self.families.get(v.family.0).unwrap();
        let n = self.notes.get(f.note.0).unwrap();
        let expression = self.expressions.get(n.expression.0).unwrap();
        let gains = expression.rendered.gains;
        v.cursor = v.cursor.with_step(v.base_step * expression.rendered.ratio);
        // Prepared playback bounds and the cursor's contiguous spans stay
        // within immutable PCM; looping never changes asset ownership.
        let plan = self.plans.get_mut(n.plan.0).unwrap();
        let asset = &plan.prepared.pcm[v.sample];
        let bus = v.bus;
        let segment = if let Some(bus) = bus {
            plan.dsp.buses.input(bus, segment.len())
        } else {
            segment
        };
        let chain = v.chain.map(|index| &plan.prepared.voice_chains[index]);
        let begin = i * plan.dsp.stride;
        let states = &mut plan.dsp.cells[begin..begin + chain.map_or(0, |c| c.stages())];
        let delay_begin = i * plan.dsp.delay_stride;
        let context = super::dsp::RenderContext {
            delay: &mut plan.dsp.delay_samples
                [delay_begin..delay_begin + chain.map_or(0, |c| c.delay_frames)],
            expression: gains,
            parameters: &plan.dsp.parameters,
            filters: super::dsp::svf::FilterContext {
                bank: &mut plan.dsp.filters,
                expression: Some((n.expression, expression.value)),
            },
            at,
        };
        let (produced, done, faults, underrun) = if let Some(frames) = asset.resident_frames() {
            let pcm = super::source::Resident {
                frames,
                levels: asset.levels(),
            };
            render_source(v, &pcm, segment, chain, states, context, &self.kernel)
        } else {
            let source = super::source::PagedFrames {
                cache: self
                    .stream_cache
                    .as_ref()
                    .expect("preflighted stream cache"),
                asset: asset.asset_id(),
            };
            render_source(v, &source, segment, chain, states, context, &self.kernel)
        };
        self.nonfinite_frames = self.nonfinite_frames.saturating_add(faults);
        self.stream_underruns = self.stream_underruns.saturating_add(u64::from(underrun));
        if let Some(bus) = bus {
            plan.dsp.buses.fed(bus, produced);
        }
        if done {
            self.end_voice(VoiceId(self.voices.id(i)));
        }
    }
}

fn render_source(
    voice: &mut super::Voice,
    source: &(impl super::source::ReadFrames + ?Sized),
    output: &mut [Frame],
    chain: Option<&super::dsp::PreparedVoiceChain>,
    states: &mut [super::dsp::ProcessorState],
    context: super::dsp::RenderContext<'_>,
    kernel: &super::resample::Kernel,
) -> (usize, bool, u64, bool) {
    let was_starved = voice.cursor.starved();
    let (produced, done, faults) = if let Some(chain) = chain {
        let (produced, faults) = chain.render(voice, source, output, states, context, kernel);
        (produced, chain.done(voice), faults)
    } else {
        let produced = voice.cursor.render(
            source,
            output,
            &mut voice.envelope,
            voice.gain,
            context.expression,
            kernel,
        );
        (produced, voice.cursor.done() || voice.envelope.done(), 0)
    };
    (
        produced,
        done,
        faults,
        !was_starved && voice.cursor.starved(),
    )
}
