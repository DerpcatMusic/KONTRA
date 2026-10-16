//! Multicore voice rendering; see docs/architecture-v2/MULTICORE.md.
//!
//! A block's voices are grouped into runs exactly as the single-threaded
//! path groups them. The runs render on the pool, each voice into its own
//! zeroed scratch block; the audio thread then folds the scratch blocks into
//! the output and bus inputs in slot order, which is the order single-threaded
//! rendering adds them in, so the result is identical.
use super::{
    Frame, Runtime, Slot, Voice, VoiceId,
    dsp::{BLOCK, MAX_LANES, RenderContext, lanes, lanes::VOICES, svf::FilterContext},
    render::{Prelude, render_source},
};
use sampler_pool::{Claims, Disjoint, Pool, Slab};

#[cfg(test)]
mod worker_witness;

/// How many threads render voices, counting the audio thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Threads {
    /// Up to four, never more than the machine has cores.
    #[default]
    Auto,
    /// Exactly this many (1 renders on the audio thread alone).
    Fixed(usize),
}

impl Threads {
    pub fn count(self) -> usize {
        match self {
            Self::Auto => std::thread::available_parallelism().map_or(1, |n| n.get().min(4)),
            Self::Fixed(n) => n,
        }
        .clamp(1, MAX_LANES)
    }
}

/// Voices per thread below which a block renders on the audio thread alone:
/// at about 1.5 us a voice, waking workers costs more than it saves.
const MIN_VOICES_PER_THREAD: usize = 32;

#[derive(Clone, Copy, Default)]
struct Outcome {
    produced: usize,
    done: bool,
    faults: u64,
    underrun: bool,
}

#[derive(Clone, Copy)]
enum Run {
    Single(usize),
    Batch { voices: [usize; VOICES], count: usize, key: super::render::BatchKey },
}
impl Run {
    fn count(&self) -> usize { match self { Self::Single(_) => 1, Self::Batch { count, .. } => *count } }
    fn voices(&self) -> &[usize] {
        match self { Self::Single(voice) => std::slice::from_ref(voice), Self::Batch { voices, count, .. } => &voices[..*count] }
    }
    fn append(&mut self, voice: usize, key: super::render::BatchKey) {
        match self {
            Self::Single(first) => {
                let mut voices = [0; VOICES]; voices[0] = *first; voices[1] = voice;
                *self = Self::Batch { voices, count: 2, key };
            }
            Self::Batch { voices, count, .. } => { voices[*count] = voice; *count += 1; }
        }
    }
}

/// Per-voice-slot storage of the parallel path.
pub(super) struct Scratch {
    scratch: Slab<[Frame; BLOCK]>,
    outcomes: Slab<Outcome>,
    claims: Claims,
    runs: Vec<Run>,
    preps: Vec<Option<Prelude>>,
}

impl Scratch {
    pub(super) fn new(voices: usize) -> Self {
        Self {
            scratch: Slab::new(vec![[[0.; 2]; BLOCK]; voices].into_boxed_slice(), 1),
            outcomes: Slab::new(vec![Outcome::default(); voices].into_boxed_slice(), 1),
            claims: Claims::new(voices),
            runs: Vec::with_capacity(voices),
            preps: vec![None; voices],
        }
    }
}

pub(super) struct Parallel {
    pool: Pool,
    threads: usize,
    scratch: Slab<[Frame; BLOCK]>,
    outcomes: Slab<Outcome>,
    claims: Claims,
    runs: Vec<Run>,
    /// Per voice slot: modulation and script state prepared for this block.
    preps: Vec<Option<Prelude>>,
    blocks: u64,
    #[cfg(test)]
    worker_witness: Option<std::sync::Arc<worker_witness::Witness>>,
}

impl Parallel {
    fn new(threads: usize, voices: usize) -> Self {
        let Scratch {
            scratch,
            outcomes,
            claims,
            runs,
            preps,
        } = Scratch::new(voices);
        Self {
            pool: Pool::new(threads - 1),
            threads,
            scratch,
            outcomes,
            claims,
            runs,
            preps,
            blocks: 0,
            #[cfg(test)]
            worker_witness: None,
        }
    }
    /// Slots the scratch covers.
    pub(super) fn voices(&self) -> usize {
        self.outcomes.len()
    }
    pub(super) fn swap_scratch(&mut self, other: &mut Scratch) {
        std::mem::swap(&mut self.scratch, &mut other.scratch);
        std::mem::swap(&mut self.outcomes, &mut other.outcomes);
        std::mem::swap(&mut self.claims, &mut other.claims);
        std::mem::swap(&mut self.runs, &mut other.runs);
        std::mem::swap(&mut self.preps, &mut other.preps);
    }
}

impl Runtime {
    /// Choose how many threads render voices. Control side: allocates and
    /// starts the worker threads, so call it before processing starts or
    /// between blocks off the audio thread. One thread (the default) renders
    /// on the audio thread alone.
    pub fn set_threads(&mut self, threads: Threads) {
        let n = threads.count();
        // Plans prepared from now on size for `n`; adopted ones grow here.
        self.lanes.store(n, std::sync::atomic::Ordering::Relaxed);
        let expressions = self.expressions.slots.len();
        for generation in self.plans.slots.iter_mut().filter_map(|s| s.value.as_mut()) {
            // An allocation failure leaves that plan short; blocks using it render on one thread.
            let _ = generation
                .dsp
                .ensure_lanes(&generation.prepared, expressions, n);
        }
        self.parallel = (n > 1).then(|| Parallel::new(n, self.voices.slots.len()));
    }

    pub fn with_threads(mut self, threads: Threads) -> Self {
        self.set_threads(threads);
        self
    }

    /// Blocks (of at most 64 frames) whose voices rendered on the pool.
    pub fn parallel_blocks(&self) -> u64 {
        self.parallel.as_ref().map_or(0, |p| p.blocks)
    }

    /// Threads rendering voices, counting the audio thread.
    pub fn threads(&self) -> usize {
        self.parallel.as_ref().map_or(1, |p| p.threads)
    }

    /// Render the block's voices on the pool. False, having changed nothing,
    /// when the block is not eligible and must render on this thread.
    pub(super) fn render_voices_parallel(&mut self, output: &mut [Frame], at: u64) -> bool {
        let frames = output.len();
        if self.parallel.is_none() || frames > BLOCK {
            return false;
        }
        let mut par = self.parallel.take().expect("checked");
        let eligible = self.plan_runs(&mut par.runs, frames, par.threads);
        let voices: usize = par.runs.iter().map(|r| r.count()).sum();
        if !eligible || voices < MIN_VOICES_PER_THREAD * par.threads {
            self.parallel = Some(par);
            return false;
        }
        let tasks = par.runs.len().min(par.threads * 4);
        // Modulation and script state advance per voice on this thread, in
        // slot order, ahead of the workers.
        for run in &par.runs {
            for &i in run.voices() {
                let needs = self.script_params
                    || self
                        .families
                        .get(self.voices.slots[i].value.as_ref().unwrap().family.0)
                        .is_some_and(|f| {
                            let plan = self.notes.get(f.note.0).unwrap().plan.0;
                            self.plans
                                .get(plan)
                                .unwrap()
                                .modulation
                                .program(i)
                                .is_some()
                        });
                par.preps[i] = if needs && run.count() == 1 {
                    self.prepare_voice(i, at, frames)
                } else {
                    None
                };
            }
        }
        {
            let Parallel {
                pool,
                scratch,
                outcomes,
                claims,
                runs,
                preps,
                #[cfg(test)]
                worker_witness,
                ..
            } = &mut par;
            let view = View {
                plans: &self.plans,
                families: &self.families,
                notes: &self.notes,
                expressions: &self.expressions,
                kernel: &self.kernel,
                cache: self.stream_cache.as_ref().map(|c| c.reader()),
                onset_heads: &self.onsets.heads,
                onset_residents: &self.onsets.residents,
                voices: Disjoint::new(&mut self.voices.slots, 1, claims),
                scratch,
                outcomes,
                preps,
                frames,
                at,
                tempo: self.tempo,
            };
            let runs = &runs[..];
            #[cfg(test)]
            if let Some(witness) = worker_witness.as_deref() {
                witness.reset();
            }
            pool.run(tasks, &|task, lane| {
                #[cfg(test)]
                if let Some(witness) = worker_witness.as_deref() {
                    witness.before(lane);
                }
                let (first, last) = (task * runs.len() / tasks, (task + 1) * runs.len() / tasks);
                for run in &runs[first..last] {
                    view.exec(run, lane);
                }
                #[cfg(test)]
                if let Some(witness) = worker_witness.as_deref() {
                    witness.after(lane);
                }
            });
        }
        par.blocks += 1;
        self.fold_runs(&par, output, at);
        self.parallel = Some(par);
        true
    }

    /// Group the active voices into runs as `render_voices` does. False when
    /// a voice needs state this path does not parallelize.
    fn plan_runs(&self, runs: &mut Vec<Run>, frames: usize, lanes: usize) -> bool {
        runs.clear();
        let mut open: Option<(super::render::BatchKey, usize)> = None;
        for word in 0..self.voice_activity.len() {
            let mut bits = self.voice_activity[word];
            while bits != 0 {
                let i = word * 64 + bits.trailing_zeros() as usize;
                bits &= bits - 1;
                let Some(v) = self.voices.slots[i].value.as_ref() else {
                    continue;
                };
                let key = self.batch_key(i, frames);
                if !v.started {
                    open = None;
                    continue;
                }
                let f = self.families.get(v.family.0).unwrap();
                let plan = self
                    .plans
                    .get(self.notes.get(f.note.0).unwrap().plan.0)
                    .unwrap();
                if v.chain
                    .is_some_and(|index| !plan.prepared.voice_chains[index].tap_buses.is_empty())
                {
                    return false;
                }
                // A plan adopted before the thread count rose may lack lane caches.
                if plan.dsp.filters.len() < lanes {
                    return false;
                }
                match (key, open) {
                    (Some(k), Some((o, r))) if k == o && runs[r].count() < VOICES => {
                        runs[r].append(i, k);
                    }
                    _ => {
                        runs.push(Run::Single(i));
                        open = key.map(|k| (k, runs.len() - 1));
                    }
                }
            }
        }
        true
    }

    /// Add every voice's scratch block to its destination in slot order and
    /// settle what the render decided: counters, bus feeds, ended voices.
    fn fold_runs(&mut self, par: &Parallel, output: &mut [Frame], at: u64) {
        let frames = output.len();
        for run in &par.runs {
            for &i in run.voices() {
                let outcome = par.outcomes.claim(i)[0];
                let mut scratch = par.scratch.claim(i);
                let v = self.voices.slots[i].value.as_ref().unwrap();
                let bus = v.bus;
                let f = self.families.get(v.family.0).unwrap();
                let note = self.notes.get(f.note.0).unwrap();
                let plan = note.plan.0;
                let stream_step = v.base_step
                    * self
                        .expressions
                        .get(note.expression.0)
                        .unwrap()
                        .rendered
                        .ratio;
                let generation = self.plans.get_mut(plan).unwrap();
                let (dsp, modulation) = (&mut generation.dsp, &mut generation.modulation);
                let target = match bus {
                    Some(bus) => dsp.buses.input(bus, frames),
                    None => &mut *output,
                };
                match par.preps[i].and_then(|p| {
                    p.points.map(|r| {
                        (
                            p.modulated,
                            if v.chain.is_some() {
                                r.without_gains()
                            } else {
                                r
                            },
                        )
                    })
                }) {
                    Some((true, ramp)) => {
                        modulation.mix(
                            i,
                            &mut scratch[0][..frames],
                            target,
                            ramp,
                            at,
                            f64::from(self.rate),
                        );
                    }
                    Some((false, ramp)) => {
                        super::render::ramp_mix(&scratch[0][..frames], target, ramp, at);
                    }
                    None => {
                        for (out, x) in target.iter_mut().zip(&scratch[0][..frames]) {
                            out[0] += x[0];
                            out[1] += x[1];
                        }
                    }
                }
                if let Some(bus) = bus {
                    dsp.buses.fed(bus, outcome.produced);
                }
                self.nonfinite_frames = self.nonfinite_frames.saturating_add(outcome.faults);
                self.stream_underruns = self
                    .stream_underruns
                    .saturating_add(u64::from(outcome.underrun));
                if !outcome.done {
                    self.refresh_stream_reservation(i, stream_step);
                }
            }
        }
        for run in &par.runs {
            for &i in run.voices() {
                if par.outcomes.claim(i)[0].done {
                    self.end_voice(VoiceId(self.voices.id(i)));
                }
            }
        }
    }
}

/// What a render task reads: shared state, and claims on the rest.
struct View<'a> {
    plans: &'a super::Arena<super::Generation>,
    families: &'a super::Arena<super::Family>,
    notes: &'a super::Arena<super::Note>,
    expressions: &'a super::Arena<super::ExpressionOwner>,
    kernel: &'a super::resample::Kernel,
    cache: Option<super::stream::PageReader<'a>>,
    onset_heads: &'a [Option<sampler_pool::SnapshotPin<crate::prepare::Ranges>>],
    onset_residents: &'a [Option<crate::prepare::ResidentPin>],
    voices: Disjoint<'a, Slot<Voice>>,
    scratch: &'a Slab<[Frame; BLOCK]>,
    outcomes: &'a Slab<Outcome>,
    preps: &'a [Option<Prelude>],
    frames: usize,
    at: u64,
    tempo: f64,
}

impl View<'_> {
    fn exec(&self, run: &Run, lane: usize) {
        match run {
            Run::Single(voice) => self.single(*voice, lane),
            Run::Batch { key, .. } => self.batch(*key, run.voices(), lane),
        }
    }

    /// The unmodulated, unscripted case of `Runtime::render_voice`.
    fn single(&self, i: usize, lane: usize) {
        let mut slot = self.voices.claim(i);
        let v = slot[0].value.as_mut().unwrap();
        let f = self.families.get(v.family.0).unwrap();
        let n = self.notes.get(f.note.0).unwrap();
        let expression = self.expressions.get(n.expression.0).unwrap();
        let plan = self.plans.get(n.plan.0).unwrap();
        plan.project_wavetable(&mut v.cursor, v.group, i);
        let prelude = self.preps[i];
        // A prepared voice had its step set with its modulation.
        if prelude.is_none() {
            v.set_step(v.base_step * expression.rendered.ratio);
        }
        let asset = plan
            .samples
            .asset(&plan.prepared, v.sample)
            .expect("a live voice pins its immutable source");
        let chain = v.chain.map(|index| plan.prepared.voice_chains.view(index));
        let mut cells = plan.dsp.cells.claim(i);
        let mut delay = plan.dsp.delay_samples.claim(i);
        let mut bank = plan.dsp.filters.claim(lane);
        bank[0].modulation = prelude.and_then(|p| p.filter).unwrap_or([1.0; 2]);
        bank[0].set_addressed_modulation(&plan.prepared.voice_modulation, &plan.modulation, i);
        let mut scratch = self.scratch.claim(i);
        let segment = &mut scratch[0][..self.frames];
        segment.fill([0.; 2]);
        let mut parameter_scratch = (!plan.dsp.modulated_parameters.is_empty())
            .then(|| plan.dsp.modulated_parameters.claim(lane));
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
        let controls = native.get_mut(0).and_then(|state| {
            plan.modulation.native_render(
                &plan.prepared.voice_modulation,
                i,
                state,
                plan.controls.base_reals(),
            )
        });
        let mut stretch = v
            .stretch
            .map(|l| plan.dsp.stretch.claim(usize::from(l.engine)));
        let context = RenderContext {
            controls,
            trace: None,
            stretch: stretch.as_mut().map(|c| &mut c[0]),
            amplifier: chain.and(prelude.and_then(|p| p.points)),
            delay: &mut delay[..chain.map_or(0, |c| c.delay_frames)],
            expression: expression.rendered.gains,
            parameters,
            filters: FilterContext {
                tempo: self.tempo,
                bank: &mut bank[0],
                expression: Some((n.expression, expression.value)),
                spaces: &mut [],
            },
            at: self.at,
            feeds: &mut [],
        };
        let applied = prelude
            .and_then(|p| p.points)
            .map_or(expression.rendered.gains, |r| {
                let m = |c: usize| r.from.gains[c].abs().max(r.to.gains[c].abs());
                [
                    expression.rendered.gains[0] * m(0),
                    expression.rendered.gains[1] * m(1),
                ]
            });
        let states = &mut cells[..chain.map_or(0, |c| c.stages())];
        let (produced, done, faults, underrun) = if let Some(head) = &self.onset_heads[i] {
            let source = super::source::PagedFrames {
                cache: self.cache.expect("pinned streamed onset"), asset: asset.asset_id(), head,
            };
            render_source::<false>(v, &source, segment, chain, states, context, self.kernel)
        } else if let Some(frames) = &self.onset_residents[i] {
            asset.want_levels(v.cursor.step(), self.at);
            let guard = asset.try_levels();
            let pcm = super::source::Resident {
                frames, levels: guard.as_deref().map_or(&[], |l| l),
            };
            render_source::<false>(v, &pcm, segment, chain, states, context, self.kernel)
        } else if let Some(frames) = asset.resident_frames() {
            asset.want_levels(v.cursor.step(), self.at);
            let guard = asset.try_levels();
            let pcm = super::source::Resident {
                frames: &frames,
                levels: guard.as_deref().map_or(&[], |l| l),
            };
            render_source::<false>(v, &pcm, segment, chain, states, context, self.kernel)
        } else {
            asset.touch(self.at + self.frames as u64);
            let head = asset.try_head();
            let source = super::source::PagedFrames {
                cache: self.cache.expect("preflighted stream cache"),
                asset: asset.asset_id(),
                head: head.as_deref().map_or(&[], |h| h),
            };
            render_source::<false>(v, &source, segment, chain, states, context, self.kernel)
        };
        if prelude.is_some_and(|p| p.points.is_some()) {
            bank[0].modulation = [1.0; 2];
        }
        v.last_gains = applied.map(|g| g * v.gain);
        let done = done
            || prelude.is_some_and(|p| p.stop)
            || super::render::inaudible(v, produced, applied);
        self.outcomes.claim(i)[0] = Outcome {
            produced,
            done,
            faults,
            underrun,
        };
    }

    /// `Runtime::render_batch`, into scratch.
    fn batch(&self, key: super::render::BatchKey, voices: &[usize], lane: usize) {
        let frames = self.frames;
        let mut block = [[0.; lanes::LANES]; BLOCK];
        let mut batch = lanes::Batch {
            count: voices.len(),
            expressions: [None; VOICES],
            ends: [0; lanes::LANES],
            len: 0,
        };
        let mut slots: [Option<sampler_pool::Claim<'_, Slot<Voice>>>; VOICES] =
            std::array::from_fn(|k| voices.get(k).map(|&i| self.voices.claim(i)));
        let mut begun = [None; VOICES];
        let mut gains = [[0.; 2]; VOICES];
        let mut starved = [false; VOICES];
        let mut plan_id = None;
        let reuse_mask = super::source::reuse::candidates(
            slots[..voices.len()]
                .iter()
                .map(|slot| slot.as_ref().unwrap()[0].value.as_ref().unwrap()),
        );
        let mut source_reuse = None;
        for k in 0..voices.len() {
            let v = slots[k].as_mut().unwrap()[0].value.as_mut().unwrap();
            let f = self.families.get(v.family.0).unwrap();
            let n = self.notes.get(f.note.0).unwrap();
            let expression = self.expressions.get(n.expression.0).unwrap();
            gains[k] = expression.rendered.gains;
            v.last_gains = gains[k].map(|g| g * v.gain);
            v.set_step(v.base_step * expression.rendered.ratio);
            let plan = self.plans.get(n.plan.0).unwrap();
            plan.project_wavetable(&mut v.cursor, v.group, voices[k]);
            plan_id = Some(n.plan.0);
            let chain = &plan.prepared.voice_chains[v.chain.unwrap()];
            let asset = plan
                .samples
                .asset(&plan.prepared, v.sample)
                .expect("a live voice pins its immutable source");
            let mut stretch = v
                .stretch
                .map(|l| plan.dsp.stretch.claim(usize::from(l.engine)));
            let engine = stretch.as_mut().map(|c| &mut c[0]);
            starved[k] = v.cursor.starved();
            batch.expressions[k] = Some((n.expression, expression.value));
            let mut planar = [[0.; BLOCK]; 2];
            begun[k] = if let Some(head) = &self.onset_heads[voices[k]] {
                let pcm = super::source::PagedFrames {
                    cache: self.cache.expect("pinned streamed onset"), asset: asset.asset_id(), head,
                };
                chain.begin(v, &pcm, frames, self.kernel, &mut planar, engine)
            } else if let Some(resident) = &self.onset_residents[voices[k]] {
                asset.want_levels(v.cursor.step(), self.at);
                let guard = asset.try_levels();
                let pcm = super::source::Resident {
                    frames: resident, levels: guard.as_deref().map_or(&[], |l| l),
                };
                chain.begin(v, &pcm, frames, self.kernel, &mut planar, engine)
            } else if let Some(resident) = asset.resident_frames() {
                asset.want_levels(v.cursor.step(), self.at);
                let guard = asset.try_levels();
                if reuse_mask & (1 << k) != 0 {
                    source_reuse
                        .get_or_insert_with(super::source::reuse::ResidentReuse::default)
                        .begin(
                            chain,
                            v,
                            resident,
                            guard,
                            frames,
                            self.kernel,
                            &mut planar,
                            engine,
                        )
                } else {
                    let pcm = super::source::Resident {
                        frames: &resident,
                        levels: guard.as_deref().map_or(&[], |l| l),
                    };
                    chain.begin(v, &pcm, frames, self.kernel, &mut planar, engine)
                }
            } else {
                asset.touch(self.at + frames as u64);
                let head = asset.try_head();
                let pcm = super::source::PagedFrames {
                    cache: self.cache.expect("preflighted stream cache"),
                    asset: asset.asset_id(),
                    head: head.as_deref().map_or(&[], |h| h),
                };
                chain.begin(v, &pcm, frames, self.kernel, &mut planar, engine)
            };
            let len = begun[k].map_or(0, |b: crate::dsp::Begun| b.len);
            for (frame, x) in block[..len].iter_mut().enumerate() {
                (x[2 * k], x[2 * k + 1]) = (planar[0][frame], planar[1][frame]);
            }
            batch.ends[2 * k] = len;
            batch.ends[2 * k + 1] = len;
        }
        drop(source_reuse);
        batch.len = batch.ends.iter().copied().max().unwrap_or(0);
        for end in &mut batch.ends[2 * voices.len()..] {
            *end = batch.len;
        }
        let plan = self
            .plans
            .get(plan_id.expect("a batch has a voice"))
            .unwrap();
        let chain = plan.prepared.voice_chains.batch(key.batch);
        let mut cells: lanes::Cells<'_> =
            std::array::from_fn(|k| voices.get(k).map(|&i| plan.dsp.cells.claim(i)));
        let mut bank = plan.dsp.filters.claim(lane);
        let at = self.at;
        sampler_simd::dispatch(
            #[inline(always)]
            || {
                lanes::process(
                    chain.pre(),
                    0,
                    &mut cells,
                    &batch,
                    &mut block,
                    &plan.dsp.parameters,
                    at,
                    &mut bank[0],
                )
            },
        );
        for (k, slot) in slots.iter_mut().enumerate().take(voices.len()) {
            let v = slot.as_mut().unwrap()[0].value.as_mut().unwrap();
            let len = batch.ends[2 * k];
            lanes::scale(&mut block, k, &crate::dsp::levels(v, len, 0), len);
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
                    &plan.dsp.parameters,
                    at,
                    &mut bank[0],
                )
            },
        );
        for (k, &i) in voices.iter().enumerate() {
            let v = slots[k].as_mut().unwrap()[0].value.as_mut().unwrap();
            let mut produced = 0;
            let mut fault = false;
            if let Some(b) = begun[k] {
                let mut planar = [[0.; BLOCK]; 2];
                for (frame, x) in block[..b.len].iter().enumerate() {
                    (planar[0][frame], planar[1][frame]) = (x[2 * k], x[2 * k + 1]);
                }
                let mut scratch = self.scratch.claim(i);
                let destination = &mut scratch[0][..frames];
                destination.fill([0.; 2]);
                let states = &mut cells[k].as_mut().expect("batch voice")[..chain.stages()];
                fault = chain.finish(v, b, &planar, false, states, gains[k], destination);
                produced = b.len;
            } else {
                self.scratch.claim(i)[0][..frames].fill([0.; 2]);
            }
            self.outcomes.claim(i)[0] = Outcome {
                produced,
                done: chain.done(v) || super::render::inaudible(v, produced, gains[k]),
                faults: u64::from(fault),
                underrun: !starved[k] && v.cursor.starved(),
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

    const NOTES: usize = 40;
    const LAYERS: usize = 4;

    fn runtime(threads: usize) -> Runtime {
        runtime_with_chain(threads, false)
    }

    fn runtime_with_chain(threads: usize, chained: bool) -> Runtime {
        let samples: Vec<Pcm> = (0..LAYERS)
            .map(|layer| {
                let mut x = 0x9E37_79B9u32.wrapping_mul(layer as u32 + 1);
                let frames = (0..6000)
                    .map(|_| {
                        x ^= x << 13;
                        x ^= x >> 17;
                        x ^= x << 5;
                        let v = (x as f32 / u32::MAX as f32 - 0.5) * 0.2;
                        [v, -v * 0.5]
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                Pcm::new(48000, frames).unwrap()
            })
            .collect();
        let regions = (0..LAYERS)
            .map(|sample| Region {
                sample,
                key_low: 48,
                key_high: 48 + NOTES as u8,
                root_key: Some(60),
                velocity_low: 0.,
                velocity_high: 1.,
                gain: if chained {
                    [0., 0.75, 0.25, 1.][sample]
                } else {
                    1.
                },
                envelope: Envelope::new(4, 2, 8, 0.5, 8).unwrap(),
                playback: Playback {
                    transpose_semitones: 7.,
                    ..Playback::default()
                },
            })
            .collect();
        let plan = Prepared::new(48000, samples, regions, LAYERS * (NOTES + 1)).unwrap();
        let plan = if chained {
            plan.with_voice_chains(
                vec![VoiceChain::new(vec![], vec![Processor::Gain(0.5)], 0).unwrap()],
                vec![Some(0); LAYERS],
            )
            .unwrap()
        } else {
            plan
        };
        Runtime::new(
            plan,
            Limits {
                notes: NOTES,
                channels: 0,
                performances: 1,
                families: NOTES,
                decisions: 0,
                expressions: NOTES,
                voices: NOTES * LAYERS,
                commands: 0,
                behaviors: 0,
                behavior_fuel: 0,
                behavior_cells: 0,
                note_cells: 0,
            },
        )
        .unwrap()
        .with_threads(Threads::Fixed(threads))
    }

    /// Script volume and fades (some stopping their voices) on every note, as
    /// a script's `change_vol` and `fade_out` would write them.
    fn play(rt: &mut Runtime, curve: crate::FadeCurve) -> Vec<Frame> {
        let plan = rt.active_plan();
        for id in 0..NOTES {
            let input = Input {
                protocol: Protocol::Native,
                port: 0,
                group: 0,
                channel: 0,
                key: 48 + id as u8,
                external_id: Some(id as i32),
            };
            let note = rt.trigger(input, 48 + id as u8, 1.).unwrap();
            rt.note_params[note.0.index]
                .layer
                .write(ModTarget::Decibels, -250 * (id as i64 % 5), false)
                .unwrap();
            if id % 3 == 0 {
                // Host IDs are not script aliases; export and witness the actual note target.
                let event = rt.source_event_id(note).unwrap();
                assert_eq!(rt.resolve_source_event(plan, event), Ok(Some(note)));
                rt.fade_event(
                    plan,
                    i64::from(event),
                    700 + 40 * id as u32,
                    true,
                    id % 2 == 0,
                    curve,
                )
                .unwrap();
            }
        }
        let mut out = Vec::new();
        for len in [64, 37, 128, 64, 200, 1].into_iter().cycle().take(60) {
            let mut block = vec![[0.; 2]; len];
            rt.render(&mut block).unwrap();
            out.extend(block);
        }
        out
    }

    #[test]
    fn empty_filter_workers_finish_real_tasks_with_exact_pcm_and_voice_history() {
        let start = |rt: &mut Runtime| {
            std::array::from_fn::<_, NOTES, _>(|id| {
                let input = Input {
                    protocol: Protocol::Native,
                    port: 0,
                    group: 0,
                    channel: 0,
                    key: 48 + id as u8,
                    external_id: Some(id as i32),
                };
                rt.trigger(input, input.key, 1.).unwrap()
            })
        };
        for chained in [false, true] {
            for threads in [2, 4] {
                let mut one = runtime_with_chain(1, chained);
                let mut many = runtime_with_chain(threads, chained);
                for generation in many.plans.slots.iter().filter_map(|s| s.value.as_ref()) {
                    assert!(generation.prepared.filters.is_empty());
                }
                let witness = std::sync::Arc::new(super::worker_witness::Witness::default());
                many.parallel.as_mut().unwrap().worker_witness = Some(witness.clone());
                let one_notes = start(&mut one);
                let many_notes = start(&mut many);
                assert_eq!(many.audible_voice_count(), 0);
                for step in 0..8 {
                    for (rt, notes) in [(&mut one, &one_notes), (&mut many, &many_notes)] {
                        if step == 1 || step == 5 {
                            for (id, &note) in notes.iter().enumerate() {
                                let expression = rt.expression_id(note).unwrap();
                                rt.set_expression(
                                    expression,
                                    Expression {
                                        gain: if step == 1 {
                                            [0., 0.125, 0.5, 2.][id % 4]
                                        } else {
                                            0.25
                                        },
                                        pan: [-0.75, -0.25, 0.25, 0.75][id % 4],
                                        ..Expression::default()
                                    },
                                )
                                .unwrap();
                            }
                        }
                        if step == 2 {
                            for (id, &note) in notes.iter().enumerate() {
                                if id % 3 == 0 {
                                    rt.fade_note(note, Some(2.), 0., 96, false).unwrap();
                                }
                            }
                        }
                        if step == 6 {
                            rt.release_all_notes();
                        }
                    }
                    if step == 3 {
                        many.set_threads(Threads::Fixed(1));
                    }
                    if step == 4 {
                        many.set_threads(Threads::Fixed(threads));
                        many.parallel.as_mut().unwrap().worker_witness = Some(witness.clone());
                    }
                    let mut expected = [[0.; 2]; 64];
                    let mut actual = expected;
                    let before = witness.snapshot().runs;
                    one.render(&mut expected).unwrap();
                    many.render(&mut actual).unwrap();
                    let observed = witness.snapshot();
                    assert!(
                        !observed.timed_out,
                        "worker failed to complete within the test bound"
                    );
                    if step != 3 && step != 7 {
                        assert!(observed.runs > before);
                        assert!(observed.completed_lane.is_some_and(|lane| lane > 0));
                    }
                    if step < 2 {
                        assert_eq!(
                            many.parallel
                                .as_ref()
                                .unwrap()
                                .runs
                                .iter()
                                .any(|run| run.count() > 1),
                            chained
                        );
                    }
                    assert_eq!(
                        actual.map(|f| f.map(f32::to_bits)),
                        expected.map(|f| f.map(f32::to_bits))
                    );
                    assert_eq!(
                        (many.now(), many.voice_count(), many.audible_voice_count()),
                        (one.now(), one.voice_count(), one.audible_voice_count())
                    );
                    if step == 0 {
                        assert!(many.audible_voice_count() > 0);
                    }
                    for (a, b) in many.voices.slots.iter().zip(&one.voices.slots) {
                        match (a.value.as_ref(), b.value.as_ref()) {
                            (Some(a), Some(b)) => {
                                assert_eq!(format!("{:?}", a.cursor), format!("{:?}", b.cursor));
                                assert_eq!(
                                    format!("{:?}", a.envelope),
                                    format!("{:?}", b.envelope)
                                );
                                assert_eq!(
                                    a.last_gains.map(f32::to_bits),
                                    b.last_gains.map(f32::to_bits)
                                );
                            }
                            (None, None) => {}
                            _ => panic!("serial/parallel voice lifetime diverged"),
                        }
                    }
                }
                assert_eq!((many.voice_count(), many.audible_voice_count()), (0, 0));
                assert_eq!((one.voice_count(), one.audible_voice_count()), (0, 0));
                one.flush_ended(|_| true);
                many.flush_ended(|_| true);
                assert_eq!((one.note_count(), many.note_count()), (0, 0));
                assert!(witness.snapshot().seen_lanes & !1 != 0);
                drop(many); // joins the owned worker pool before the witness owner leaves
                drop(witness);
            }
        }
    }

    #[test]
    fn fade_curve_parallel_plain_mix_matches_serial_and_differs_from_linear() {
        let linear = play(&mut runtime(1), crate::FadeCurve::Linear);
        for curve in [
            crate::FadeCurve::EqualPower,
            crate::FadeCurve::SCurve,
            crate::FadeCurve::Exponential,
            crate::FadeCurve::Logarithmic,
        ] {
            let mut one = runtime(1);
            let expected = play(&mut one, curve);
            assert!(expected.iter().flatten().all(|sample| sample.is_finite()));
            assert!(
                expected[..64]
                    .iter()
                    .zip(&linear[..64])
                    .any(|(faded, control)| {
                        faded.iter().any(|sample| *sample != 0.) && faded != control
                    }),
                "{curve:?}: no nonzero changed audio during the active fade"
            );
            assert_ne!(
                expected, linear,
                "{curve:?}: no nonlinear fade reached audio"
            );
            for threads in [2, 4] {
                let mut rt = runtime(threads);
                let actual = play(&mut rt, curve);
                assert!(rt.parallel_blocks() > 10, "{curve:?}, threads {threads}");
                assert_eq!(rt.voice_count(), one.voice_count());
                assert_eq!(actual, expected, "{curve:?}, threads {threads}");
            }
        }
    }

    #[test]
    fn script_layered_voices_render_the_single_threaded_output_exactly() {
        let mut one = runtime(1);
        let expected = play(&mut one, crate::FadeCurve::Linear);
        assert!(expected.iter().any(|f| f[0] != 0.));
        for threads in [2, 4] {
            let mut rt = runtime(threads);
            let actual = play(&mut rt, crate::FadeCurve::Linear);
            assert!(
                rt.parallel_blocks() > 10,
                "{threads}: {} parallel blocks",
                rt.parallel_blocks()
            );
            assert_eq!(rt.voice_count(), one.voice_count());
            assert!(
                actual
                    .iter()
                    .zip(&expected)
                    .all(|(a, e)| a.map(f32::to_bits) == e.map(f32::to_bits)),
                "{threads} threads differ from one"
            );
        }
    }
}

#[cfg(test)]
mod typed_run_layout {
    #[test]
    fn retained_batch_run_layout_receipt() {
        use std::mem::size_of;
        let baseline_field_layout = size_of::<([usize; super::VOICES], usize)>();
        let candidate = size_of::<super::Run>();
        eprintln!("retained batch run: baseline_field_layout={baseline_field_layout} candidate={candidate} key={}",
            size_of::<crate::render::BatchKey>());
        // The proposal's finite bound is 32 extra bytes per preallocated run.
        // Root's compiler measures this; no child layout probe was run.
        assert!(candidate <= baseline_field_layout + 32);
    }
}
