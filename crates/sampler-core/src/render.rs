//! Sample-time segmentation and deterministic voice rendering.
use super::{Error, Frame, Runtime, VoiceId, dsp::lanes::VOICES};

/// Runtime health counters as plain data, for load reports and meters.
/// Counters are cumulative since construction; timings cover `render` calls.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RuntimeStats {
    /// Voices that ran out of resident stream pages and faded out.
    pub stream_underruns: u64,
    /// Voice starts rejected because every voice was busy and audible.
    pub voice_drops: u64,
    /// Voice starts refused after preflight (source not resident, pool full).
    pub refused_starts: u64,
    /// Voices started silent because their first frames were not resident
    /// (`set_cold_starts`); they fade in when their pages arrive.
    pub cold_starts: u64,
    /// Output frames silenced because they were not finite.
    pub nonfinite_frames: u64,
    pub voices: usize,
    /// Voice slots now, how often the pool grew, and growths refused.
    pub voice_capacity: usize,
    pub voice_growths: u64,
    pub growth_failures: u64,
    /// Wall time of the last `render` call, and the peak since `reset_peak`.
    pub render_nanos_last: u64,
    pub render_nanos_peak: u64,
    /// Frames of the last `render` call (its real-time budget is
    /// frames / sample rate).
    pub render_frames_last: u32,
    /// Bytes of stream cache pages, allocated whether resident or not.
    pub stream_cache_bytes: usize,
}

impl Runtime {
    /// Cap the callback instructions run per rendered block across all
    /// callbacks (default unlimited). Callbacks over it resume next block in
    /// order, so notes they play start a few blocks later but no block runs
    /// long.
    pub fn set_behavior_block_fuel(&mut self, fuel: usize) {
        self.block_fuel = fuel;
        self.block_fuel_left = fuel;
    }

    pub fn behavior_block_fuel(&self) -> usize {
        self.block_fuel
    }

    pub fn stats(&self) -> RuntimeStats {
        RuntimeStats {
            stream_underruns: self.stream_underruns,
            voice_drops: self.voice_drops,
            refused_starts: self.refused_starts,
            cold_starts: self.cold_started,
            nonfinite_frames: self.nonfinite_frames,
            voices: self.voices.count(),
            voice_capacity: self.voices.slots.len(),
            voice_growths: self.voice_growths,
            growth_failures: self.growth_failures,
            render_nanos_last: self.render_time[0],
            render_nanos_peak: self.render_time[1],
            render_frames_last: self.render_time[2] as u32,
            stream_cache_bytes: self.stream_cache.as_ref().map_or(0, |c| c.bytes()),
        }
    }

    pub fn reset_peak(&mut self) {
        self.render_time[1] = 0;
    }

    /// Resident PCM bytes (frames and octave levels) of every live plan, each
    /// asset once. Control side: linear in assets per plan.
    pub fn resident_bytes(&self) -> usize {
        let plans: Vec<_> = self
            .plans
            .slots
            .iter()
            .filter_map(|s| s.value.as_ref())
            .collect();
        let mut bytes = 0;
        for (i, plan) in plans.iter().enumerate() {
            for pcm in &plan.prepared.pcm {
                // ponytail: quadratic dedupe, only paid while plans overlap.
                let shared = plans[..i].iter().any(|p| {
                    p.prepared
                        .pcm
                        .iter()
                        .any(|q| q.asset_id() == pcm.asset_id())
                });
                if !shared {
                    bytes += pcm.resident_bytes();
                }
            }
        }
        bytes
    }
}

/// A voice's per-chunk modulation and script state, from `prepare_voice`.
#[derive(Clone, Copy)]
pub(super) struct Prelude {
    pub points: Option<super::voice_mod::Ramp>,
    pub modulated: bool,
    pub stop: bool,
    /// Filter-cutoff modulation for the chunk.
    pub filter: Option<[f64; 2]>,
}

/// A releasing voice whose output cannot exceed this (about -120 dBFS) for
/// `QUIET_FRAMES` in a row is ended: it costs CPU and nobody hears it.
const INAUDIBLE: f32 = 1e-6;
const QUIET_FRAMES: u32 = 512;

/// Track how long a releasing voice has been inaudible; true once it can end.
/// The bound is its envelope level times its gain and the gains applied
/// after it (`gains`: expression, script volume, modulation), for samples up
/// to full scale. Chain filters could add resonance, so a voice that is
/// merely quiet is never cut: only one far below the threshold is.
pub(super) fn inaudible(v: &mut super::Voice, frames: usize, gains: [f32; 2]) -> bool {
    if !v.envelope.releasing() {
        v.quiet = 0;
        return false;
    }
    let bound = v.envelope.current().abs() * v.gain.abs() * gains[0].abs().max(gains[1].abs());
    if bound < INAUDIBLE {
        v.quiet = v.quiet.saturating_add(frames as u32);
    } else {
        v.quiet = 0;
    }
    v.quiet >= QUIET_FRAMES
}

impl Runtime {
    /// Events at the exclusive block end stay pending until the next render (including
    /// an empty block). Overflow is rejected before any output/state mutation.
    pub fn render(&mut self, output: &mut [Frame]) -> Result<(), Error> {
        self.render_split(output, &mut [])
    }

    /// [`Self::render`], with buses whose [`crate::BusMix::output`] names one
    /// of `outs` summed there instead of into `output`. Each of `outs` must be
    /// at least `output.len()` long; they are added to, not cleared.
    pub fn render_split(
        &mut self,
        output: &mut [Frame],
        outs: &mut [&mut [Frame]],
    ) -> Result<(), Error> {
        if outs.iter().any(|o| o.len() < output.len()) {
            return Err(Error::InvalidInput);
        }
        let start = std::time::Instant::now();
        self.stream_fault = None;
        if self.signal_trace { self.render_inner::<true>(output, outs)?; }
        else { self.render_inner::<false>(output, outs)?; }
        let nanos = u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX);
        self.render_time = [
            nanos,
            self.render_time[1].max(nanos),
            output.len().min(u32::MAX as usize) as u64,
        ];
        Ok(())
    }

    fn render_inner<const TRACE: bool>(
        &mut self,
        output: &mut [Frame],
        outs: &mut [&mut [Frame]],
    ) -> Result<(), Error> {
        let end = self
            .now
            .checked_add(output.len() as u64)
            .ok_or(Error::ClockOverflow)?;
        output.fill([0.0; 2]);
        self.start_plan_programs();
        self.apply_growth();
        self.resume_yielded();
        self.apply_due();
        let mut offset = 0;
        while self.now < end {
            self.apply_due();
            let boundary = self.commands.first().map_or(end, |c| c.at.min(end));
            let len = (boundary - self.now) as usize;
            let segment = &mut output[offset..offset + len];
            self.render_segment::<TRACE>(segment, outs, offset);
            if self.stream_fault.is_some() { return Err(Error::NotReady); }
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
        // The next block's events draw on a full allowance; the empty renders
        // a host makes between events are not blocks.
        if !output.is_empty() {
            self.block_fuel_left = self.block_fuel;
        }
        Ok(())
    }

    fn render_segment<const TRACE: bool>(&mut self, output: &mut [Frame], outs: &mut [&mut [Frame]], offset: usize) {
        let chunked = TRACE || self.script_params
            || self.parallel.is_some()
            || self.plans.slots.iter().any(|s| {
                s.value.as_ref().is_some_and(|g| {
                    g.prepared.buses.len() != 0
                        || !g.prepared.voice_chains.is_empty()
                        || !g.prepared.voice_modulation.is_empty()
                })
            });
        if !chunked {
            self.render_voices::<TRACE>(output, self.now);
            return;
        }
        // Chunks end on the absolute BLOCK grid, where voice modulation
        // evaluates, so host block sizes do not move control points.
        let mut at = self.now;
        let mut rest = output;
        while !rest.is_empty() {
            let len = (super::voice_mod::CELL - at % super::voice_mod::CELL) as usize;
            let (output, tail) = rest.split_at_mut(len.min(rest.len()));
            rest = tail;
            for g in self.plans.slots.iter_mut().filter_map(|s| s.value.as_mut()) {
                g.dsp.buses.begin();
                if TRACE { if let Some(trace) = &mut g.dsp.trace { trace.begin(at, output.len()); } }
            }
            self.render_voices::<TRACE>(output, at);
            if self.stream_fault.is_some() { return; }
            for g in self.plans.slots.iter_mut().filter_map(|s| s.value.as_mut()) {
                let start = offset + (at - self.now) as usize;
                let faults = g
                    .dsp
                    .buses
                    .render::<TRACE>(&g.prepared.buses, output, outs, start, at,
                        if TRACE { g.dsp.trace.as_mut().zip(g.prepared.signal_trace.as_ref()).map(|(r,t)| (&*t.graph,r)) } else { None });
                self.nonfinite_frames = self.nonfinite_frames.saturating_add(faults);
                if TRACE { if let Some(trace) = &mut g.dsp.trace { trace.end(); } }
            }
            at += output.len() as u64;
        }
    }

    fn render_voices<const TRACE: bool>(&mut self, output: &mut [Frame], at: u64) {
        if TRACE {
            for word in 0..self.voice_activity.len() {
                let mut occupied = self.voice_activity[word];
                while occupied != 0 { let bit = occupied.trailing_zeros() as usize; occupied &= occupied - 1;
                    self.render_voice::<true>(word * 64 + bit, output, at); }
            }
            return;
        }
        if self.offline {
            // Modulation advance is cached on the absolute control grid. Prepare
            // the exact steps before requesting pages; rendering at the same
            // clock reads that ramp without advancing its sources again.
            let mut next = self.voices.first;
            while let Some(i) = next {
                next = self.voices.slots[i].next;
                self.prepare_voice(i, at, output.len());
            }
            if let Err(error) = self.wait_streaming(output.len().min(u32::MAX as usize) as u32, std::time::Duration::from_secs(5)) {
                self.stream_fault = Some(error);
                return;
            }
        }
        if self.render_voices_parallel(output, at) {
            return;
        }
        let mut batch: (Option<(usize, usize)>, [usize; VOICES], usize) = (None, [0; VOICES], 0);
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
                    if self.stream_fault.is_some() { return; }
                    let key = self.batch_key(i, output.len());
                    if key.is_none() || key != batch.0 || batch.2 == VOICES {
                        self.render_run(&batch.1[..batch.2], output, at);
                        batch = (key, [0; VOICES], 0);
                    }
                    if key.is_none() {
                        self.render_voice::<false>(i, output, at);
                    } else {
                        batch.1[batch.2] = i;
                        batch.2 += 1;
                    }
                }
            }
        }
        self.render_run(&batch.1[..batch.2], output, at);
    }

    /// Voices batch when consecutive in slot order, started, and running the
    /// same delay-free chain of the same plan over at most one block.
    pub(super) fn batch_key(&self, i: usize, frames: usize) -> Option<(usize, usize)> {
        let v = self.voices.slots[i].value.as_ref()?;
        let chain = v.chain?;
        if !v.started || frames > super::dsp::BLOCK {
            return None;
        }
        let f = self.families.get(v.family.0).unwrap();
        let plan = self.notes.get(f.note.0).unwrap().plan.0;
        let generation = self.plans.get(plan).unwrap();
        // A modulated or script-layered voice mixes through its own ramp.
        if self.script_params || generation.modulation.program(i).is_some() {
            return None;
        }
        let prepared = &generation.prepared;
        prepared.voice_chains[chain]
            .batches()
            .then_some((plan.index, chain))
    }

    fn render_run(&mut self, voices: &[usize], output: &mut [Frame], at: u64) {
        match voices {
            [] => {}
            [i] => self.render_voice::<false>(*i, output, at),
            _ => self.render_batch(voices, output, at),
        }
    }

    /// [`Self::render_voice`] for a batch: sources and the epilogue per voice
    /// in slot order, every stage across the batch's lanes.
    fn render_batch(&mut self, voices: &[usize], output: &mut [Frame], at: u64) {
        use super::dsp::lanes;
        let mut block = [[0.; lanes::LANES]; super::dsp::BLOCK];
        let mut batch = lanes::Batch {
            count: voices.len(),
            expressions: [None; VOICES],
            ends: [0; lanes::LANES],
            len: 0,
        };
        let mut begun = [None; VOICES];
        let mut gains = [[0.; 2]; VOICES];
        let mut starved = [false; VOICES];
        let mut plan_id = None;
        for (lane, &i) in voices.iter().enumerate() {
            let v = self.voices.slots[i].value.as_mut().unwrap();
            let f = self.families.get(v.family.0).unwrap();
            let n = self.notes.get(f.note.0).unwrap();
            let expression = self.expressions.get(n.expression.0).unwrap();
            gains[lane] = expression.rendered.gains;
            v.last_gains = gains[lane].map(|g| g * v.gain);
            v.cursor = v.cursor.with_step(v.base_step * expression.rendered.ratio);
            let plan = self.plans.get(n.plan.0).unwrap();
            plan_id = Some(n.plan.0);
            let chain = &plan.prepared.voice_chains[v.chain.unwrap()];
            let asset = &plan.prepared.pcm[v.sample];
            starved[lane] = v.cursor.starved();
            batch.expressions[lane] = Some((n.expression, expression.value));
            let mut planar = [[0.; super::dsp::BLOCK]; 2];
            begun[lane] = if let Some(frames) = asset.resident_frames() {
                asset.want_levels(v.cursor.step(), at);
                let guard = asset.try_levels();
                let pcm = super::source::Resident {
                    frames,
                    levels: guard.as_deref().map_or(&[], |l| l),
                };
                chain.begin(v, &pcm, output.len(), &self.kernel, &mut planar)
            } else {
                asset.touch(at + output.len() as u64);
                let head = asset.try_head();
                let pcm = super::source::PagedFrames {
                    cache: self
                        .stream_cache
                        .as_ref()
                        .expect("preflighted stream cache")
                        .reader(),
                    asset: asset.asset_id(),
                    head: head.as_deref().map_or(&[], |h| h),
                };
                chain.begin(v, &pcm, output.len(), &self.kernel, &mut planar)
            };
            let len = begun[lane].map_or(0, |b: super::dsp::Begun| b.len);
            for (frame, x) in block[..len].iter_mut().enumerate() {
                (x[2 * lane], x[2 * lane + 1]) = (planar[0][frame], planar[1][frame]);
            }
            batch.ends[2 * lane] = len;
            batch.ends[2 * lane + 1] = len;
        }
        batch.len = batch.ends.iter().copied().max().unwrap_or(0);
        for end in &mut batch.ends[2 * voices.len()..] {
            *end = batch.len;
        }
        let plan = self
            .plans
            .get_mut(plan_id.expect("a batch has a voice"))
            .unwrap();
        let first = self.voices.slots[voices[0]].value.as_ref().unwrap();
        let chain = &plan.prepared.voice_chains[first.chain.unwrap()];
        let dsp = &mut plan.dsp;
        let mut cells: lanes::Cells<'_> =
            std::array::from_fn(|lane| voices.get(lane).map(|&i| dsp.cells.claim(i)));
        let bank = &mut dsp.filters.as_mut_slice()[0];
        sampler_simd::dispatch(
            #[inline(always)]
            || {
                lanes::process(
                    chain.pre(),
                    0,
                    &mut cells,
                    &batch,
                    &mut block,
                    &dsp.parameters,
                    at,
                    bank,
                )
            },
        );
        for (lane, &i) in voices.iter().enumerate() {
            let v = self.voices.slots[i].value.as_mut().unwrap();
            let len = batch.ends[2 * lane];
            lanes::scale(&mut block, lane, &super::dsp::levels(v, len), len);
        }
        sampler_simd::dispatch(
            #[inline(always)]
            || {
                lanes::process(
                    chain.post(),
                    chain.pre().len(),
                    &mut cells,
                    &batch,
                    &mut block,
                    &dsp.parameters,
                    at,
                    bank,
                )
            },
        );
        let mut done = [false; VOICES];
        for (lane, &i) in voices.iter().enumerate() {
            let v = self.voices.slots[i].value.as_mut().unwrap();
            let mut produced = 0;
            if let Some(b) = begun[lane] {
                let mut planar = [[0.; super::dsp::BLOCK]; 2];
                for (frame, x) in block[..b.len].iter().enumerate() {
                    (planar[0][frame], planar[1][frame]) = (x[2 * lane], x[2 * lane + 1]);
                }
                let destination = match v.bus {
                    Some(bus) => dsp.buses.input(bus, output.len()),
                    None => &mut *output,
                };
                let states = &mut cells[lane].as_mut().expect("batch voice")[..chain.stages()];
                let fault = chain.finish(v, b, &planar, false, states, gains[lane], destination);
                self.nonfinite_frames = self.nonfinite_frames.saturating_add(u64::from(fault));
                produced = b.len;
            }
            self.stream_underruns = self
                .stream_underruns
                .saturating_add(u64::from(!starved[lane] && v.cursor.starved()));
            if let Some(bus) = v.bus {
                dsp.buses.fed(bus, produced);
            }
            done[lane] = chain.done(v) || inaudible(v, produced, gains[lane]);
        }
        drop(cells);
        for (lane, &i) in voices.iter().enumerate() {
            if done[lane] {
                self.end_voice(VoiceId(self.voices.id(i)));
            }
        }
    }

    /// What a voice's chunk needs before it renders: its modulation ramp and
    /// script layers, and its cursor's step. None for a voice not yet started.
    /// Evaluated per voice, so the voices may be prepared ahead of rendering.
    pub(super) fn prepare_voice(&mut self, i: usize, at: u64, frames: usize) -> Option<Prelude> {
        let v = self.voices.slots[i].value.as_mut()?;
        if !v.started {
            return None;
        }
        let f = self.families.get(v.family.0).unwrap();
        let n = self.notes.get(f.note.0).unwrap();
        let expression = self.expressions.get(n.expression.0).unwrap();
        let plan = self.plans.get_mut(n.plan.0).unwrap();
        let modulated = plan.modulation.program(i).is_some();
        let mut filter = None;
        let points = plan.modulation.program(i).map(|_| {
            let performance = self.selections[f.note.0.index].performance;
            let inputs = super::voice_mod::Inputs::new(
                n,
                expression.value,
                &self.performance_state.current(performance).controllers,
                self.release_times[f.note.0.index].counter(n.key_down(), at),
                &self.note_params[f.note.0.index].mods,
            );
            let clock = super::voice_mod::Clock {
                rate: f64::from(self.rate),
                tempo: self.tempo,
                now: at,
            };
            let ramp = plan
                .modulation
                .advance(&plan.prepared.voice_modulation, i, &inputs, clock);
            let (from, to) = (ramp.from, ramp.to);
            filter = Some([
                (from.filter[0] + to.filter[0]) * 0.5,
                (from.filter[1] + to.filter[1]) * 0.5,
            ]);
            ramp
        });
        // Script layers (render_segment chunks once a script writes one).
        let mut stop = false;
        let points = if self.script_params {
            let params = self.note_params[f.note.0.index];
            let group = plan.script.layer(v.group);
            let end = at + frames as u64;
            let fade = |t| {
                params
                    .fade
                    .map_or(1.0, |f: super::script_params::Fade| f.at(t))
                    * v.script_fade.map_or(1.0, |f| f.at(t))
            };
            // Gains sit on the absolute control grid, like voice modulation, so
            // host block sizes and event splits cannot move them.
            let begin = at - at % super::voice_mod::CELL;
            let grid_end = begin + super::voice_mod::CELL;
            let first = group.stack(params.layer_at(begin));
            let last = group.stack(params.layer_at(grid_end));
            let from = first.gains(fade(begin));
            let to = last.gains(fade(grid_end));
            stop = end == grid_end
                && (params.fade.is_some_and(|f| f.stop && f.done(grid_end))
                    || v.script_fade.is_some_and(|f| f.stop && f.done(grid_end)));
            let mut ramp = points.unwrap_or(super::voice_mod::Ramp {
                from: Default::default(),
                to: Default::default(),
                begin,
                end: grid_end,
            });
            for (o, g, layer) in [(&mut ramp.from, from, first), (&mut ramp.to, to, last)] {
                o.gains = [o.gains[0] * g[0], o.gains[1] * g[1]];
                o.pitch += layer.semitones();
            }
            Some(ramp)
        } else {
            points
        };
        v.cursor = v.cursor.with_step(match points {
            None => v.base_step * expression.rendered.ratio,
            Some(ramp) => (v.base_step
                * expression.rendered.ratio
                * ((ramp.from.pitch + ramp.to.pitch) / 24.0).exp2())
            .clamp(super::resample::MIN_STEP, super::resample::MAX_STEP),
        });
        Some(Prelude {
            points,
            modulated,
            stop,
            filter,
        })
    }

    #[inline]
    fn render_voice<const TRACE: bool>(&mut self, i: usize, segment: &mut [Frame], at: u64) {
        let Some(Prelude {
            points,
            modulated,
            stop,
            filter,
        }) = self.prepare_voice(i, at, segment.len())
        else {
            return;
        };
        let v = self.voices.slots[i].value.as_mut().unwrap();
        // Retention invariant: live voice -> counted family -> counted note
        // -> expression owner. Each owner retires only after its dependents.
        let f = self.families.get(v.family.0).unwrap();
        let n = self.notes.get(f.note.0).unwrap();
        let expression = self.expressions.get(n.expression.0).unwrap();
        let gains = expression.rendered.gains;
        let cc = |controller:usize| if TRACE {
            ((u64::from(self.performance_state.current(self.selections[f.note.0.index].performance).controllers[controller])*127+u64::from(u32::MAX)/2)/u64::from(u32::MAX)) as u8
        } else {0};
        let controllers=if TRACE {[cc(1),cc(7),cc(11)]} else {[0;3]};
        // Prepared playback bounds and the cursor's contiguous spans stay
        // within immutable PCM; looping never changes asset ownership.
        let plan = self.plans.get_mut(n.plan.0).unwrap();
        // Modulated voices render at most one BLOCK chunk per call (render_segment
        // chunks whenever a plan has programs) into scratch, then mix with ramps.
        let chain = v.chain.map(|index| &plan.prepared.voice_chains[index]);
        if let Some(chain) = chain {
            for &bus in &chain.tap_buses {
                plan.dsp.feeds[bus].samples = [[0.0; super::dsp::BLOCK]; 2];
            }
        }
        let mut scratch = [[0.0; 2]; super::dsp::BLOCK];
        let bank = &mut plan.dsp.filters.as_mut_slice()[0];
        bank.modulation = filter.unwrap_or([1.0; 2]);
        bank.set_addressed_modulation(&plan.prepared.voice_modulation, &plan.modulation, i);
        let asset = &plan.prepared.pcm[v.sample];
        let bus = v.bus;
        let target = if let Some(bus) = bus {
            plan.dsp.buses.input(bus, segment.len())
        } else {
            segment
        };
        let (segment, mixed) = if points.is_some() {
            (&mut scratch[..target.len()], Some(target))
        } else {
            (target, None)
        };
        let mut claimed = plan.dsp.cells.claim(i);
        let states = &mut claimed[..chain.map_or(0, |c| c.stages())];
        let mut delay = plan.dsp.delay_samples.claim(i);
        let trace = if TRACE {
            plan.prepared.signal_trace.as_ref().zip(plan.dsp.trace.as_mut()).and_then(|(t, recorder)|
                t.graph.voices.get(&v.source_zone).map(|nodes| crate::trace::VoiceTrace { recorder,
                    graph: &t.graph, nodes, identity: crate::trace::TraceIdentity { zone: v.source_zone,
                        sample: v.sample, family: v.family.0.index, generation: v.family.0.generation,
                        ratio: v.cursor.step(), source_frame: v.cursor.trace_position(), sample_start: v.cursor.trace_start(), velocity: n.velocity, key:n.pitch.key(),
                        rr_sequence:plan.prepared.trace_region_take(nodes.region).map(|t|t.sequence),
                        rr_take:plan.prepared.trace_region_take(nodes.region).map(|t|t.index),
                        group:v.group, layer:v.bus, routed_to:v.bus.map(|b|t.graph.buses[b].input).or(Some(t.graph.master)), cc1:controllers[0], cc7:controllers[1], cc11:controllers[2], voice_gain:f64::from(v.gain),
                        region_gain:plan.prepared.trace_region_gains(nodes.region,n.pitch.key(),n.velocity)[0],
                        velocity_gain:plan.prepared.trace_region_gains(nodes.region,n.pitch.key(),n.velocity)[1],
                        xfade_weight:plan.prepared.trace_region_gains(nodes.region,n.pitch.key(),n.velocity)[2],
                        script_gain:plan.script.layer(v.group).stack(self.note_params[f.note.0.index].layer_at(at+1)).gains(1.).map(f64::from),
                        note_gain:self.note_params[f.note.0.index].layer_at(at+1).gains(1.).map(f64::from),
                        amplifier_control_gain:points.map_or([1.;2],|r|r.gains_at(at+1)).map(f64::from),
                        envelope_level:f64::from(v.envelope.current()), ..Default::default() } }))
        } else { None };
        let context = super::dsp::RenderContext {
            trace,
            amplifier: chain.and(points),
            delay: &mut delay[..chain.map_or(0, |c| c.delay_frames)],
            expression: gains,
            parameters: &plan.dsp.parameters,
            filters: super::dsp::svf::FilterContext {
                bank: &mut plan.dsp.filters.as_mut_slice()[0],
                expression: Some((n.expression, expression.value)),
                reverbs: &mut [],
                convolutions: &mut [],
            },
            at,
            feeds: &mut plan.dsp.feeds,
        };
        let (produced, done, faults, underrun) = if let Some(frames) = asset.resident_frames() {
            asset.want_levels(v.cursor.step(), at);
            let guard = asset.try_levels();
            let pcm = super::source::Resident {
                frames,
                levels: guard.as_deref().map_or(&[], |l| l),
            };
            render_source::<TRACE>(v, &pcm, segment, chain, states, context, &self.kernel)
        } else {
            asset.touch(at + segment.len() as u64);
            let head = asset.try_head();
            let source = super::source::PagedFrames {
                cache: self
                    .stream_cache
                    .as_ref()
                    .expect("preflighted stream cache")
                    .reader(),
                asset: asset.asset_id(),
                head: head.as_deref().map_or(&[], |h| h),
            };
            render_source::<TRACE>(v, &source, segment, chain, states, context, &self.kernel)
        };
        drop((claimed, delay));
        if let (Some(ramp), Some(target)) = (points, mixed) {
            let ramp = if chain.is_some() { ramp.without_gains() } else { ramp };
            plan.dsp.filters.as_mut_slice()[0].modulation = [1.0; 2];
            if modulated {
                plan.modulation
                    .mix(i, segment, target, ramp, at, f64::from(self.rate));
            } else {
                ramp_mix(segment, target, ramp, at);
            }
        }
        if faults == 0 {
            if let Some(chain) = chain {
                for &bus in &chain.tap_buses {
                    let feed = &plan.dsp.feeds[bus].samples;
                    let target = plan.dsp.buses.input(bus, produced);
                    for (i, sample) in target.iter_mut().enumerate() {
                        for c in 0..2 {
                            sample[c] += (feed[c][i] * f64::from(gains[c])) as f32;
                        }
                    }
                    plan.dsp.buses.fed(bus, produced);
                }
            }
        }
        let applied = points.map_or(gains, |r| {
            let m = |c: usize| r.from.gains[c].abs().max(r.to.gains[c].abs());
            [gains[0] * m(0), gains[1] * m(1)]
        });
        v.last_gains = applied.map(|g| g * v.gain);
        let done = done || stop || inaudible(v, produced, applied);
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

/// Mix `chunk` into `output`, ramping per-channel gains from `from` to `to`.
pub(super) fn ramp_mix(
    chunk: &[Frame],
    output: &mut [Frame],
    ramp: super::voice_mod::Ramp,
    now: u64,
) {
    let (from, to) = (ramp.from.gains, ramp.to.gains);
    let len = ramp.end.saturating_sub(ramp.begin).max(1) as f32;
    let step = [(to[0] - from[0]) / len, (to[1] - from[1]) / len];
    let offset = now.saturating_sub(ramp.begin) as f32;
    for (i, (out, frame)) in output.iter_mut().zip(chunk).enumerate() {
        let at = offset + (i + 1) as f32;
        out[0] += frame[0] * (from[0] + step[0] * at);
        out[1] += frame[1] * (from[1] + step[1] * at);
    }
}

pub(super) fn render_source<const TRACE: bool>(
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
        let (produced, faults) = chain.render::<TRACE>(voice, source, output, states, context, kernel);
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

impl Runtime {
    /// Clone the control-side drain endpoint; never call file I/O from render.
    pub fn signal_trace_reader(&self) -> Option<crate::trace::TraceReader> {
        self.plans.get(self.active_plan.0)?.prepared.signal_trace_reader()
    }
}
