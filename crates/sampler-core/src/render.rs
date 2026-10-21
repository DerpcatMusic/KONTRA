//! Sample-time segmentation and deterministic voice rendering.
use super::{Error, Frame, Runtime, VoiceId, dsp::lanes::VOICES};
#[cfg(test)]
#[path = "render/traversal_experiment.rs"]
mod traversal_experiment;
#[cfg(test)]
std::thread_local! { static SUM_CELLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

/// Runtime health counters as plain data, for load reports and meters.
/// Counters are cumulative since construction; timings cover `render` calls.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RuntimeStats {
    /// Voices that ran out of resident stream pages and faded out.
    pub stream_underruns: u64,
    /// `while` loops left at Kontakt's iteration limit (the callback continued).
    pub loop_limits: u64,
    /// Starts and rendered chunks using the admitted pinned whole-voice controls.
    pub v1_control_starts: u64,
    pub v1_control_blocks: u64,
    /// Aggregate rejected voice starts: stream admission and unavailable slots.
    /// Preserved for compatibility; use the reason counters below for diagnosis.
    pub voice_drops: u64,
    /// Starts rejected by unavailable voice slots after no silent waiting
    /// victim could be replaced. Separate from page-pool admission failures.
    pub voice_pool_refusals: u64,
    /// A full voice pool replaced a started voice already waiting silently for
    /// source data. Includes cold onset holds and faded-out stream starvation.
    pub waiting_voice_replacements: u64,
    /// Voice starts refused after preflight (source not resident, pool full).
    pub refused_starts: u64,
    /// Time-stretch voices that found no free engine and played as a sampler.
    pub stretch_fallbacks: u64,
    /// Voices started silent because their admitted startup runway was not
    /// resident (`set_cold_starts`; one frame unless explicitly configured);
    /// they fade in when the runway is available.
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
    /// Starts rejected by page admission, separate from audible underruns.
    pub stream_capacity_refusals: u64,
    pub stream_disconnected_refusals: u64,
}

/// Dense batch identity retained from admission through dispatch. Plan identity
/// keeps the existing slot-order grouping law; live notes pin its generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct BatchKey {
    plan: usize,
    pub(super) batch: super::dsp::BatchIndex,
}
struct PendingBatch {
    key: BatchKey,
    voices: [usize; VOICES],
    count: usize,
}

impl Runtime {
    /// Cap the callback instructions run per rendered block across all
    /// callbacks (default unlimited). Callbacks over it resume next block in
    /// order, so notes they play start a few blocks later but no block runs
    /// long.
    pub fn set_behavior_block_fuel(&mut self, fuel: usize) {
        self.block_fuel = fuel;
        self.block_fuel_left = fuel;
        self.behavior_block_explicit = false;
        self.behavior_time_left = None;
    }

    /// One host block's shared callback allowance, installed before its events.
    /// MIDI, empty renders and render fragments consume the same allowance.
    pub fn begin_behavior_block(&mut self, fuel: usize, time: Option<std::time::Duration>) {
        self.block_fuel = fuel;
        self.block_fuel_left = fuel;
        self.behavior_time_left = time;
        self.behavior_block_explicit = true;
    }

    pub fn behavior_block_fuel(&self) -> usize {
        self.block_fuel
    }

    pub fn stats(&self) -> RuntimeStats {
        RuntimeStats {
            stream_underruns: self.stream_underruns,
            loop_limits: self.loop_limits,
            v1_control_starts: self.plans.slots.iter().filter_map(|s| s.value.as_ref()).map(|g| g.modulation.native_starts).sum(),
            v1_control_blocks: self.plans.slots.iter().filter_map(|s| s.value.as_ref()).map(|g| g.modulation.native_blocks.load(std::sync::atomic::Ordering::Relaxed)).sum(),
            voice_drops: self.voice_drops,
            voice_pool_refusals: self.voice_pool_refusals,
            waiting_voice_replacements: self.waiting_voice_replacements,
            refused_starts: self.refused_starts,
            stretch_fallbacks: self.stretch_fallbacks,
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
            stream_capacity_refusals: self.stream_admission_errors[0],
            stream_disconnected_refusals: self.stream_admission_errors[1],
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
            bytes += plan.samples.resident_bytes();
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
        // Source installation is explicit. Missing real input bindings refuse
        // before due work, output writes, cursor advance or callback dispatch.
        for generation in self.plans.slots.iter().filter_map(|slot|slot.value.as_ref()){
            generation.dsp.buses.validate_element_readiness(&generation.prepared.buses)?;
            if generation.prepared.source_output_modulation_storage_bytes()!=0&&!output.is_empty(){
                let last=self.now.checked_add(u64::try_from(output.len()).map_err(|_|Error::ClockOverflow)?).and_then(|end|end.checked_sub(1)).ok_or(Error::ClockOverflow)?;
                last.checked_div(super::voice_mod::CELL).and_then(|cell|cell.checked_add(1)).and_then(|cell|cell.checked_mul(super::voice_mod::CELL)).ok_or(Error::ClockOverflow)?;
            }
        }
        self.validate_external_render_prefix(output.len())?;
        let start = std::time::Instant::now();
        self.stream_fault = None;
        if self.signal_trace {
            self.render_inner::<true>(output, outs)?;
        } else {
            self.render_inner::<false>(output, outs)?;
        }
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
        let external=self.has_external_programs();
        // Preserve the old no-external output/error order exactly. An external
        // prefix services current-frame work before touching PCM, so a newly
        // discovered turn can return Busy at the SAME sample frame.
        if !external {output.fill([0.0; 2]);}
        let _ = self.poll_sample_update();
        self.start_plan_programs();
        self.service_residency()?;
        self.drain_behavior();
        self.apply_growth();
        self.resume_yielded();
        self.apply_due();
        if external {
            self.validate_external_render_prefix(output.len())?;
            output.fill([0.0;2]);
        }
        let mut offset = 0;
        while self.now < end {
            self.apply_due();
            if external && self.external_clock_blocked(){return Err(Error::Busy);}
            let boundary = self.commands.first().map_or(end, |c| c.at.min(end));
            let len = (boundary - self.now) as usize;
            let segment = &mut output[offset..offset + len];
            self.render_segment::<TRACE>(segment, outs, offset);
            if self.stream_fault.is_some() {
                return Err(Error::NotReady);
            }
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
        // Legacy render-driven callers refill per nonempty render. Explicit host
        // blocks refill only at begin_behavior_block, never between fragments.
        if !output.is_empty() && !self.behavior_block_explicit {
            self.block_fuel_left = self.block_fuel;
        }
        Ok(())
    }

    fn render_segment<const TRACE: bool>(
        &mut self,
        output: &mut [Frame],
        outs: &mut [&mut [Frame]],
        offset: usize,
    ) {
        // T21: due callbacks/events are handled by render_inner regardless of
        // audio activity. No voices or tails means no sum kernels/tiles to run.
        // Keep tracing and the one pending bus-history reset on their old path.
        // NI engine activity contract / topology B9-B10; no control-rate change.
        if !TRACE && self.voices.count() == 0 && self.processing_owners.count() == 0
            && self.plans.slots.iter().filter_map(|s| s.value.as_ref()).all(|g| !g.dsp.buses.needs_render())
        {
            return;
        }
        let chunked = self.processing_owners.count() != 0 || TRACE
            || self.script_params
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
        let tone_cutoff = self.has_input_tone().then_some(self.part_tone_cutoff);
        let mut at = self.now;
        let mut rest = output;
        while !rest.is_empty() {
            #[cfg(test)]
            SUM_CELLS.set(SUM_CELLS.get() + 1);
            let len = (super::voice_mod::CELL - at % super::voice_mod::CELL) as usize;
            let (output, tail) = rest.split_at_mut(len.min(rest.len()));
            rest = tail;
            for g in self.plans.slots.iter_mut().filter_map(|s| s.value.as_mut()) {
                g.dsp.buses.begin();
                if TRACE {
                    if let Some(trace) = &mut g.dsp.trace {
                        trace.begin(at, output.len());
                    }
                }
            }
            self.render_voices::<TRACE>(output, at);
            if self.stream_fault.is_some() {
                return;
            }
            for g in self.plans.slots.iter_mut().filter_map(|s| s.value.as_mut()) {
                let start = offset + (at - self.now) as usize;
                if g.dsp.buses.prepare_element_outputs(&g.prepared.buses,at,self.tempo,g.controls.base_reals()).is_err(){
                    self.nonfinite_frames=self.nonfinite_frames.saturating_add(1);continue;
                }
                let faults = g.dsp.buses.render::<TRACE>(
                    &g.prepared.buses,
                    output,
                    outs,
                    start,
                    at,
                    tone_cutoff,
                    self.tempo,
                    if TRACE {
                        g.dsp
                            .trace
                            .as_mut()
                            .zip(g.prepared.signal_trace.as_ref())
                            .map(|(r, t)| (&*t.graph, r))
                    } else {
                        None
                    },
                );
                self.nonfinite_frames = self.nonfinite_frames.saturating_add(faults);
                if TRACE {
                    if let Some(trace) = &mut g.dsp.trace {
                        trace.end();
                    }
                }
            }
            at += output.len() as u64;
        }
    }

    fn render_voices<const TRACE: bool>(&mut self, output: &mut [Frame], at: u64) {
        if !self.offline { self.prepare_onsets(at, output.len()); }
        self.render_prepared_voices::<TRACE>(output, at);
        // Every worker has joined. Source owners remain in live/retired plans;
        // releasing the reader count cannot destroy PCM on this callback.
        self.clear_onsets();
    }

    pub(super) fn render_prepared_voices<const TRACE: bool>(&mut self, output: &mut [Frame], at: u64) {
        if self.offline {
            // Modulation advance is cached on the absolute control grid. Prepare
            // the exact steps before requesting pages; rendering at the same
            // clock reads that ramp without advancing its sources again.
            let mut next = self.voices.first;
            while let Some(i) = next {
                next = self.voices.slots[i].next;
                self.prepare_voice(i, at, output.len());
            }
            if let Err(error) = self.wait_streaming(
                output.len().min(u32::MAX as usize) as u32,
                std::time::Duration::from_secs(5),
            ) {
                self.stream_fault = Some(error);
                return;
            }
            // Voices admitted during realtime can still carry a pending cohort
            // when the host switches to offline. Service has now completed; use
            // the same pinned gate before dispatching their first source read.
            self.prepare_onsets(at, output.len());
        }
        if TRACE {
            for word in 0..self.voice_activity.len() {
                let mut occupied = self.voice_activity[word];
                while occupied != 0 {
                    let bit = occupied.trailing_zeros() as usize;
                    occupied &= occupied - 1;
                    self.render_voice::<true>(word * 64 + bit, output, at);
                }
            }
            self.render_processing_owners(output, at);
            return;
        }
        if self.render_voices_parallel(output, at) {
            return;
        }
        let mut batch: Option<PendingBatch> = None;
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
                    if self.stream_fault.is_some() {
                        return;
                    }
                    let key = self.batch_key(i, output.len());
                    if batch.as_ref().is_some_and(|run| Some(run.key) != key || run.count == VOICES) {
                        if let Some(run) = batch.take() {
                            self.render_run(run.key, &run.voices[..run.count], output, at);
                        }
                    }
                    match key {
                        None => self.render_voice::<false>(i, output, at),
                        Some(key) => match batch.as_mut() {
                            Some(run) => { run.voices[run.count] = i; run.count += 1; }
                            None => {
                                let mut voices = [0; VOICES]; voices[0] = i;
                                batch = Some(PendingBatch { key, voices, count: 1 });
                            }
                        },
                    }
                }
            }
        }
        if let Some(run) = batch {
            self.render_run(run.key, &run.voices[..run.count], output, at);
        }
        self.render_processing_owners(output, at);
    }

    /// Voices batch when consecutive in slot order, started, and running the
    /// same delay-free chain of the same plan over at most one block.
    pub(super) fn batch_key(&self, i: usize, frames: usize) -> Option<BatchKey> {
        let v = self.voices.slots[i].value.as_ref()?;
        if v.processing.is_some() { return None; }
        let chain = v.chain?;
        if !v.started || frames > super::dsp::BLOCK || v.cursor.waiting()
            || v.cursor.pending_start_runway() > 1 {
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
        prepared.voice_chains[chain].batch_index()
            .map(|batch| BatchKey { plan: plan.index, batch })
    }

    fn render_run(&mut self, key: BatchKey, voices: &[usize], output: &mut [Frame], at: u64) {
        match voices {
            [] => {}
            [i] => self.render_voice::<false>(*i, output, at),
            _ => self.render_batch(key, voices, output, at),
        }
    }

    /// [`Self::render_voice`] for a batch: sources and the epilogue per voice
    /// in slot order, every stage across the batch's lanes.
    fn render_batch(&mut self, key: BatchKey, voices: &[usize], output: &mut [Frame], at: u64) {
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
        let reuse_mask = super::source::reuse::candidates(
            voices.iter().map(|&i| self.voices.slots[i].value.as_ref().unwrap()),
        );
        let mut source_reuse = None;
        for (lane, &i) in voices.iter().enumerate() {
            let v = self.voices.slots[i].value.as_mut().unwrap();
            let f = self.families.get(v.family.0).unwrap();
            let n = self.notes.get(f.note.0).unwrap();
            let expression = self.expressions.get(n.expression.0).unwrap();
            gains[lane] = expression.rendered.gains;
            v.last_gains = gains[lane].map(|g| g * v.gain);
            v.set_step(v.base_step * expression.rendered.ratio);
            let plan = self.plans.get(n.plan.0).unwrap();
            plan.project_wavetable(&mut v.cursor, v.group, i);
            plan_id = Some(n.plan.0);
            let chain = &plan.prepared.voice_chains[v.chain.unwrap()];
            let asset = plan.samples.asset(&plan.prepared, v.sample).expect("a live voice pins its immutable source");
            let mut stretch = v.stretch.map(|l| plan.dsp.stretch.claim(usize::from(l.engine)));
            let engine = stretch.as_mut().map(|c| &mut c[0]);
            starved[lane] = v.cursor.starved();
            batch.expressions[lane] = Some((n.expression, expression.value));
            let mut planar = [[0.; super::dsp::BLOCK]; 2];
            begun[lane] = if let Some(head) = &self.onsets.heads[i] {
                let pcm = super::source::PagedFrames {
                    cache: self.stream_cache.as_ref().expect("pinned streamed onset").reader(),
                    asset: asset.asset_id(), head,
                };
                chain.begin(v, &pcm, output.len(), &self.kernel, &mut planar, engine)
            } else if let Some(frames) = &self.onsets.residents[i] {
                asset.want_levels(v.cursor.step(), at);
                let guard = asset.try_levels();
                let pcm = super::source::Resident {
                    frames, levels: guard.as_deref().map_or(&[], |l| l),
                };
                chain.begin(v, &pcm, output.len(), &self.kernel, &mut planar, engine)
            } else if let Some(frames) = asset.resident_frames() {
                asset.want_levels(v.cursor.step(), at);
                let guard = asset.try_levels();
                if reuse_mask & (1 << lane) != 0 {
                    source_reuse
                        .get_or_insert_with(super::source::reuse::ResidentReuse::default)
                        .begin(chain, v, frames, guard, output.len(), &self.kernel, &mut planar, engine)
                } else {
                    let pcm = super::source::Resident {
                        frames: &frames,
                        levels: guard.as_deref().map_or(&[], |l| l),
                    };
                    chain.begin(v, &pcm, output.len(), &self.kernel, &mut planar, engine)
                }
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
                chain.begin(v, &pcm, output.len(), &self.kernel, &mut planar, engine)
            };
            let len = begun[lane].map_or(0, |b: super::dsp::Begun| b.len);
            for (frame, x) in block[..len].iter_mut().enumerate() {
                (x[2 * lane], x[2 * lane + 1]) = (planar[0][frame], planar[1][frame]);
            }
            batch.ends[2 * lane] = len;
            batch.ends[2 * lane + 1] = len;
        }
        // Readers retire before any plan/voice lifecycle mutation. Their drop
        // only releases snapshot counts; storage reclamation remains control-side.
        drop(source_reuse);
        batch.len = batch.ends.iter().copied().max().unwrap_or(0);
        for end in &mut batch.ends[2 * voices.len()..] {
            *end = batch.len;
        }
        let plan = self
            .plans
            .get_mut(plan_id.expect("a batch has a voice"))
            .unwrap();
        let chain = plan.prepared.voice_chains.batch(key.batch);
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
            lanes::scale(&mut block, lane, &super::dsp::levels(v, len, 0), len);
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
            } else {
                let step = self.voices.slots[i].value.as_ref().unwrap().cursor.step();
                self.refresh_stream_reservation(i, step);
            }
        }
    }

    /// What a voice's chunk needs before it renders: its modulation ramp and
    /// script layers, and its cursor's step. None for a voice not yet started.
    /// Evaluated per voice, so the voices may be prepared ahead of rendering.
    pub(super) fn prepare_voice(&mut self, i: usize, at: u64, frames: usize) -> Option<Prelude> {
        if !self.voices.slots[i].value.as_ref()?.started{return None;}
        let voice_id=VoiceId(self.voices.id(i));
        let provider_owner=self.voices.slots[i].value.as_ref().and_then(|voice|{
            let (owner,_)=voice.processing?;
            let state=self.processing_owners.get(owner.0)?;
            let generation=self.plans.get(state.plan.0)?;
            generation.modulation.provider_scope(&generation.prepared.voice_modulation,i).map(|_|owner)
        });
        if let Some(owner)=provider_owner&&self.prepare_processing_modulation(owner,at,crate::topology::ProcessingModulationPhase::Forecast).is_err(){
            self.fail_processing_owner(owner);return None;
        }
        let v = self.voices.slots[i].value.as_mut()?;
        if !v.started {
            return None;
        }
        let processing = v.processing;
        let chained_owner = processing.and_then(|(owner, _)| {
            let state = self.processing_owners.get(owner.0)?;
            let generation = self.plans.get(state.plan.0)?;
            Some(
                generation
                    .prepared
                    .processing_scopes
                    .get(state.template)
                    .is_some_and(|scope| scope.chain.is_some()),
            )
        });
        let f = self.families.get(v.family.0).unwrap();
        let n = self.notes.get(f.note.0).unwrap();
        let expression = self.expressions.get(n.expression.0).unwrap();
        let plan = self.plans.get_mut(n.plan.0).unwrap();
        let modulated = plan.modulation.program(i).is_some();
        let mut filter = None;
        let points = if plan.modulation.program(i).is_some() {
            let performance = self.selections[f.note.0.index].performance;
            let follow_len = v.follow_len;
            let mut follow = [0f32; super::dsp::BLOCK];
            follow[..follow_len].copy_from_slice(&v.follow_audio[..follow_len]);
            let inputs = super::voice_mod::Inputs {
                audio: &follow[..follow_len],
                ..super::voice_mod::Inputs::new(
                    n,
                    expression.value,
                    &self.performance_state.channel_current(performance, n.address).controllers,
                    self.release_times[f.note.0.index].counter(n.key_down(), at),
                    &self.note_params[f.note.0.index].mods,
                    plan.controls.base_reals(),
                )
            };
            let clock = super::voice_mod::Clock {
                rate: f64::from(self.rate),
                tempo: self.tempo,
                now: at,
            };
            // v1 freezes loop edits throughout release, including continuous loops.
            if !v.envelope.releasing()
                && let Some(offsets) = plan.modulation.loop_offsets(
                    &plan.prepared.voice_modulation,
                    i,
                    inputs.controls,
                )
            {
                v.cursor = v.cursor.with_loop_offsets(offsets);
            }
            let ramp=if let Some(scope)=plan.modulation.provider_scope(&plan.prepared.voice_modulation,i){
                let (id,ordinal)=v.processing?;
                let owner=self.processing_owners.get(id.0)?;
                if owner.plan!=n.plan||owner.template!=scope||owner.members.get(ordinal).copied().flatten()!=Some(voice_id){return None;}
                let provider=plan.dsp.owner_modulation.provider_view(&plan.prepared.processing_modulation,id.0.index,scope).ok()?;
                plan.modulation.advance_provided(&plan.prepared.voice_modulation,i,&inputs,clock,provider,v.envelope.releasing()).ok()?
            }else{plan.modulation.advance(&plan.prepared.voice_modulation,i,&inputs,clock)};
            let (from, to) = (ramp.from, ramp.to);
            filter = Some([
                (from.filter[0] + to.filter[0]) * 0.5,
                (from.filter[1] + to.filter[1]) * 0.5,
            ]);
            Some(ramp)
        }else{None};
        plan.project_wavetable(&mut v.cursor, v.group, i);
        // Script layers (render_segment chunks once a script writes one).
        let mut stop = false;
        let points = if self.script_params {
            let params = self.note_params[f.note.0.index];
            let group = plan.script.layer(v.group);
            let end = at + frames as u64;
            let curved = params.fade.filter(|f| f.nonlinear() && !f.done(at));
            let fade = |t| {
                params.fade.map_or(1.0, |f| {
                    if !f.nonlinear() {
                        f.at(t)
                    } else if f.done(at) {
                        f.at(at)
                    } else {
                        // The active curve is applied per sample, not between grid endpoints.
                        1.0
                    }
                }) * v.script_fade.map_or(1.0, |f| f.at(t))
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
                script_fade: None,
            });
            ramp.script_fade = curved;
            for (o, g, layer) in [(&mut ramp.from, from, first), (&mut ramp.to, to, last)] {
                o.gains = [o.gains[0] * g[0], o.gains[1] * g[1]];
                o.pitch += layer.semitones();
            }
            Some(ramp)
        } else {
            points
        };
        let mut points = points;
        if chained_owner == Some(true) {
            // A shared processing chain owns the audible stage order; Tone is
            // applied by the activation after that chain, not per member.
            let tone = points.map_or(0., |r| (r.from.tone + r.to.tone) * 0.5);
            if let Some((owner, _)) = processing
                && let Some(state) = self.processing_owners.get_mut(owner.0)
            {
                state.tone = tone;
            }
            if let Some(ramp) = points.as_mut() {
                ramp.from.tone = 0.0;
                ramp.to.tone = 0.0;
            }
        }
        let pitch = match points {
            None => v.base_step * expression.rendered.ratio,
            Some(ramp) => {
                let exponent = (ramp.from.pitch + ramp.to.pitch) / 24.0;
                if exponent != v.mod_pitch.0 {
                    v.mod_pitch = (exponent, exponent.exp2());
                }
                let step = v.base_step * expression.rendered.ratio * v.mod_pitch.1;
                if v.cursor.wavetable().is_some() {
                    step
                } else {
                    step.clamp(super::resample::MIN_STEP, super::resample::MAX_STEP)
                }
            }
        };
        v.set_step(pitch);
        Some(Prelude {
            points,
            modulated,
            stop,
            filter,
        })
    }

    #[inline]
    fn render_voice<const TRACE: bool>(&mut self, i: usize, segment: &mut [Frame], at: u64) {
        if self.voices.slots[i].value.as_ref().is_some_and(|v| v.processing.is_some()) { return; }
        let _ = self.render_source_voice::<TRACE>(i, segment, at, false);
    }

    pub(super) fn render_source_voice<const TRACE: bool>(
        &mut self, i: usize, segment: &mut [Frame], at: u64, owner_capture: bool,
    ) -> Option<usize> {
        let prelude = self.prepare_voice(i, at, segment.len());
        let Some(prelude) = prelude.as_ref() else { return None; };
        let points = prelude.points.as_ref();
        let modulated = prelude.modulated;
        let stop = prelude.stop;
        let filter = prelude.filter;
        let trace_voice = if TRACE {
            Some(crate::trace::onset::PhysicalVoice::from(crate::VoiceId(self.voices.id(i))))
        } else { None };
        let v = self.voices.slots[i].value.as_mut().unwrap();
        // Retention invariant: live voice -> counted family -> counted note
        // -> expression owner. Each owner retires only after its dependents.
        let f = self.families.get(v.family.0).unwrap();
        let n = self.notes.get(f.note.0).unwrap();
        let expression = self.expressions.get(n.expression.0).unwrap();
        let gains = if owner_capture { [1.; 2] } else { expression.rendered.gains };
        let tap_gains = expression.rendered.gains;
        let cc = |controller: usize| {
            if TRACE {
                ((u64::from(
                    self.performance_state
                        .channel_current(self.selections[f.note.0.index].performance, n.address)
                        .controllers[controller],
                ) * 127
                    + u64::from(u32::MAX) / 2)
                    / u64::from(u32::MAX)) as u8
            } else {
                0
            }
        };
        let controllers = if TRACE {
            [cc(1), cc(7), cc(11)]
        } else {
            [0; 3]
        };
        let stream_step = v.base_step * expression.rendered.ratio;
        // Prepared playback bounds and the cursor's contiguous spans stay
        // within immutable PCM; looping never changes asset ownership.
        let plan = self.plans.get_mut(n.plan.0).unwrap();
        // Modulated voices render at most one BLOCK chunk per call (render_segment
        // chunks whenever a plan has programs) into scratch, then mix with ramps.
        let chain = v.chain.map(|index| plan.prepared.voice_chains.view(index));
        if let Some(chain) = chain {
            for &bus in &chain.tap_buses {
                plan.dsp.feeds[bus].samples = [[0.0; super::dsp::BLOCK]; 2];
            }
        }
        // Only ramped voices render through scratch; direct voices use target.
        let mut scratch;
        let bank = &mut plan.dsp.filters.as_mut_slice()[0];
        bank.modulation = filter.unwrap_or([1.0; 2]);
        bank.set_addressed_modulation(&plan.prepared.voice_modulation, &plan.modulation, i);
        let asset = plan.samples.asset(&plan.prepared, v.sample).expect("a live voice pins its immutable source");
        let bus = if owner_capture { None } else { v.bus };
        let target = if let Some(bus) = bus {
            plan.dsp.buses.input(bus, segment.len())
        } else {
            segment
        };
        let (segment, mixed) = if points.is_some() {
            scratch = [[0.0; 2]; super::dsp::BLOCK];
            (&mut scratch[..target.len()], Some(target))
        } else {
            (target, None)
        };
        let mut claimed = plan.dsp.cells.claim(i);
        let states = &mut claimed[..chain.map_or(0, |c| c.stages())];
        let mut delay = plan.dsp.delay_samples.claim(i);
        let trace = if TRACE {
            plan.prepared
                .signal_trace
                .as_ref()
                .zip(plan.dsp.trace.as_mut())
                .and_then(|(t, recorder)| {
                    t.graph
                        .voices
                        .get(&v.source_zone)
                        .map(|nodes| crate::trace::VoiceTrace {
                            recorder,
                            graph: &t.graph,
                            nodes,
                            envelope_parameters: v.envelope.trace_parameters(),
                            identity: crate::trace::TraceIdentity {
                                voice: trace_voice,
                                zone: v.source_zone,
                                sample: v.sample,
                                family: v.family.0.index,
                                generation: v.family.0.generation,
                                ratio: v.cursor.step(),
                                source_frame: v.cursor.trace_position(),
                                sample_start: v.cursor.trace_start(),
                                velocity: n.velocity,
                                key: n.pitch.key(),
                                rr_sequence: plan
                                    .prepared
                                    .trace_region_take(nodes.region)
                                    .map(|t| t.sequence),
                                rr_take: plan
                                    .prepared
                                    .trace_region_take(nodes.region)
                                    .map(|t| t.index),
                                group: v.group,
                                layer: v.bus,
                                routed_to: v
                                    .bus
                                    .map(|b| t.graph.buses[b].input)
                                    .or(Some(t.graph.master)),
                                cc1: controllers[0],
                                cc7: controllers[1],
                                cc11: controllers[2],
                                voice_gain: f64::from(v.gain),
                                region_gain: plan.prepared.trace_region_gains(
                                    nodes.region,
                                    n.pitch.key(),
                                    n.velocity,
                                )[0],
                                velocity_gain: plan.prepared.trace_region_gains(
                                    nodes.region,
                                    n.pitch.key(),
                                    n.velocity,
                                )[1],
                                xfade_weight: plan.prepared.trace_region_gains(
                                    nodes.region,
                                    n.pitch.key(),
                                    n.velocity,
                                )[2],
                                script_gain: plan
                                    .script
                                    .layer(v.group)
                                    .stack(self.note_params[f.note.0.index].layer_at(at + 1))
                                    .gains(1.)
                                    .map(f64::from),
                                note_gain: self.note_params[f.note.0.index]
                                    .layer_at(at + 1)
                                    .gains(1.)
                                    .map(f64::from),
                                amplifier_control_gain: points
                                    .map_or([1.; 2], |r| r.gains_at(at + 1))
                                    .map(f64::from),
                                envelope_level: f64::from(v.envelope.current()),
                                ..Default::default()
                            },
                        })
                })
        } else {
            None
        };
        let mut parameter_scratch = (!plan.dsp.modulated_parameters.is_empty())
            .then(|| plan.dsp.modulated_parameters.claim(0));
        let parameters = if let Some(scratch) = parameter_scratch.as_mut() {
            if plan.modulation.project_parameters(
                &plan.prepared.voice_modulation,
                i,
                &plan.dsp.parameters,
                &plan.prepared.dsp_bindings,
                chain.map_or(0..0, |c| c.parameter_span.clone()),
                scratch,
            ) {
                &scratch[..]
            } else {
                &plan.dsp.parameters[..]
            }
        } else {
            &plan.dsp.parameters[..]
        };
        let mut native = plan.modulation.native.claim(i);
        let controls = native.get_mut(0).and_then(|state| plan.modulation.native_render(&plan.prepared.voice_modulation, i, state, plan.controls.base_reals()));
        let mut stretch = v.stretch.map(|l| plan.dsp.stretch.claim(usize::from(l.engine)));
        let context = super::dsp::RenderContext {
            controls,
            trace,
            stretch: stretch.as_mut().map(|c| &mut c[0]),
            amplifier: chain.and_then(|_| points.copied()),
            delay: &mut delay[..chain.map_or(0, |c| c.delay_frames)],
            expression: gains,
            parameters,
            filters: super::dsp::svf::FilterContext {
                tempo: self.tempo,
                bank: &mut plan.dsp.filters.as_mut_slice()[0],
                expression: Some((n.expression, expression.value)),
                spaces: &mut [], stereo_tunes: &mut [], replikas: &mut [],
            },
            at,
            feeds: &mut plan.dsp.feeds,
        };
        let (produced, done, faults, underrun) = if let Some(head) = &self.onsets.heads[i] {
            let source = super::source::PagedFrames {
                cache: self.stream_cache.as_ref().expect("pinned streamed onset").reader(),
                asset: asset.asset_id(), head,
            };
            render_source::<TRACE>(v, &source, segment, chain, states, context, &self.kernel)
        } else if let Some(frames) = &self.onsets.residents[i] {
            asset.want_levels(v.cursor.step(), at);
            let guard = asset.try_levels();
            let pcm = super::source::Resident {
                frames, levels: guard.as_deref().map_or(&[], |l| l),
            };
            render_source::<TRACE>(v, &pcm, segment, chain, states, context, &self.kernel)
        } else if let Some(frames) = asset.resident_frames() {
            asset.want_levels(v.cursor.step(), at);
            let guard = asset.try_levels();
            let pcm = super::source::Resident {
                frames: &frames,
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
        drop((claimed, delay, parameter_scratch, native, stretch));
        if let (Some(ramp), Some(target)) = (points, mixed) {
            let ramp = if chain.is_some() {
                (*ramp).without_gains()
            } else {
                *ramp
            };
            plan.dsp.filters.as_mut_slice()[0].modulation = [1.0; 2];
            if modulated {
                plan.modulation
                    .mix(i, segment, target, ramp, at, f64::from(self.rate));
            } else {
                ramp_mix(segment, target, ramp, at);
            }
        }
        let follow_len = segment.len().min(super::dsp::BLOCK);
        for (dst, frame) in v.follow_audio.iter_mut().zip(segment.iter()) {
            *dst = (frame[0] + frame[1]) * 0.5;
        }
        v.follow_len = follow_len;
        if faults == 0 {
            if let Some(chain) = chain {
                for &bus in &chain.tap_buses {
                    let feed = &plan.dsp.feeds[bus].samples;
                    let target = plan.dsp.buses.input(bus, produced);
                    for (i, sample) in target.iter_mut().enumerate() {
                        for c in 0..2 {
                            sample[c] += (feed[c][i] * f64::from(tap_gains[c])) as f32;
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
        } else {
            self.refresh_stream_reservation(i, stream_step);
        }
        Some(produced)
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
    if let Some(fade) = ramp.script_fade {
        for (i, (out, frame)) in output.iter_mut().zip(chunk).enumerate() {
            let at = offset + (i + 1) as f32;
            let gain = fade.at(now + i as u64 + 1) as f32;
            out[0] += frame[0] * (from[0] + step[0] * at) * gain;
            out[1] += frame[1] * (from[1] + step[1] * at) * gain;
        }
    } else {
        for (i, (out, frame)) in output.iter_mut().zip(chunk).enumerate() {
            let at = offset + (i + 1) as f32;
            out[0] += frame[0] * (from[0] + step[0] * at);
            out[1] += frame[1] * (from[1] + step[1] * at);
        }
    }
}

pub(super) fn render_source<const TRACE: bool>(
    voice: &mut super::Voice,
    source: &(impl super::source::ReadFrames + ?Sized),
    output: &mut [Frame],
    chain: Option<super::dsp::PreparedChainRef<'_>>,
    states: &mut [super::dsp::ProcessorState],
    mut context: super::dsp::RenderContext<'_>,
    kernel: &super::resample::Kernel,
) -> (usize, bool, u64, bool) {
    // A cohort decision happens before any source, envelope or voice DSP work.
    if voice.cursor.pending_start_runway() > 1 {
        return (0, voice.envelope.done(), 0, false);
    }
    let was_starved = voice.cursor.starved();
    let (produced, done, faults) = if let Some(chain) = chain {
        let (produced, faults) =
            chain.render::<TRACE>(voice, source, output, states, context, kernel);
        (produced, chain.done(voice), faults)
    } else if let Some(controls) = context.controls.as_mut() {
        let waiting = voice.cursor.waiting();
        let active = voice.envelope.remaining();
        let mut unity = crate::blocks::envelope::EnvelopeState::new(if waiting {
            crate::Envelope::one_shot(0, active.min(super::dsp::BLOCK + 1) as u32, 0)
        } else { crate::Envelope::default() });
        let remaining = unity.remaining();
        let count = if waiting { output.len() } else { output.len().min(active) };
        let produced = match (context.stretch.as_deref_mut(), waiting) {
            (Some(engine), false) => super::source::stretch_mix(engine, voice, source, &mut output[..count], 1., [1.; 2], false),
            _ => voice.cursor.render(source, &mut output[..count], &mut unity, 1., [1.; 2], kernel),
        };
        let held = if waiting { produced - (remaining - unity.remaining()) } else { 0 };
        let levels = controls.levels(voice, produced, held);
        for (i, frame) in output[..produced].iter_mut().enumerate() {
            for c in 0..2 { frame[c] = (f64::from(frame[c]) * levels[i] * f64::from(context.expression[c])) as f32; }
        }
        (produced, voice.cursor.done() || voice.envelope.done() || controls.done(), 0)
    } else {
        let produced = match (context.stretch.as_deref_mut(), voice.cursor.waiting()) {
            (Some(engine), false) => super::source::stretch_mix(
                engine,
                voice,
                source,
                output,
                voice.gain,
                context.expression,
                true,
            ),
            _ => voice.cursor.render(
                source,
                output,
                &mut voice.envelope,
                voice.gain,
                context.expression,
                kernel,
            ),
        };
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
        self.plans
            .get(self.active_plan.0)?
            .prepared
            .signal_trace_reader()
    }
}

#[cfg(test)]
mod cpu_tests {
    use super::*;
    use crate::*;
    #[test]
    fn dormant_sum_graph_skips_cells_but_keeps_events_clock_and_wake_pcm() {
        let plan = Prepared::new(48000, vec![Pcm::new(48000, vec![[1., -0.5]].into_boxed_slice()).unwrap()],
            vec![Region { sample:0, key_low:60, key_high:60, root_key:None, velocity_low:0., velocity_high:1.,
                gain:1., envelope:Envelope::default(), playback:Playback::default() }], 1).unwrap()
            .with_buses((0..100).map(|_| Bus { processors:vec![Processor::Gain(1.)],
                sends:vec![BusSend { bus:None, gain:1. }], tail_frames:0 }).collect(), vec![Some(97)]).unwrap();
        let mut rt = Runtime::new(plan, Limits { notes:4, channels:0, performances:1, families:4,
            voices:4, expressions:4, decisions:4, commands:8, behaviors:0, behavior_cells:0, behavior_fuel:0, note_cells:0 }).unwrap();
        let performance = rt.performance(0).unwrap();
        rt.schedule_event(512, Event::Controller(performance, 1, 12345)).unwrap();
        SUM_CELLS.set(0);
        let mut idle = [[1.;2];2048];
        rt.render(&mut idle).unwrap();
        assert_eq!(idle, [[0.;2];2048]);
        assert_eq!(rt.now(), 2048);
        assert_eq!(rt.controller(performance, 1).unwrap(), 12345);
        assert_eq!(SUM_CELLS.get(), 0, "dormant sums must not traverse every control tile");
        let input = Input { protocol:Protocol::Native, port:0, group:0, channel:0, key:60, external_id:None };
        rt.trigger(input, 60, 1.).unwrap();
        let mut output = [[0.;2];64];
        rt.render(&mut output).unwrap();
        assert_eq!(output[0], [1., -0.5]);
        assert_eq!(output[1..], [[0.;2];63]);
        SUM_CELLS.set(0);
        rt.render(&mut idle).unwrap();
        assert_eq!(idle, [[0.;2];2048]);
        assert_eq!(SUM_CELLS.get(), 0);
        rt.trigger(input, 60, 1.).unwrap();
        let mut woke = [[0.;2];64];
        rt.render(&mut woke).unwrap();
        assert_eq!(woke, output, "wake after silent clock advancement is exact");
    }
}
