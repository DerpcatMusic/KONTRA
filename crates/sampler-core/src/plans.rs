//! Prepared generations move through bounded SPSC ownership transfers.
use super::{Arena, Error, Handle, Limits, NoteId, Prepared, Runtime};
use rtrb::{Consumer, Producer, PushError, RingBuffer};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlanId(pub(super) Handle);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanError {
    Capacity,
    SampleRate,
    LocalCapacity,
    NoteStateCapacity,
    Disconnected,
    SequenceExhausted,
    Disabled,
}

pub struct RejectedPlan {
    pub reason: PlanError,
    pub prepared: Box<Prepared>,
}
impl std::fmt::Debug for RejectedPlan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RejectedPlan")
            .field("reason", &self.reason)
            .finish_non_exhaustive()
    }
}

/// Owned plan, mutable sequence storage and control request identity. Drop on control.
pub struct PlanTransfer {
    pub request: u64,
    pub prepared: Box<Prepared>,
    sequences: super::variation::SequenceState,
    controls: super::control::ControlState,
    scripts: Box<[super::ops::ScriptBank]>,
    midi_object: super::MidiObject,
    dsp: super::dsp::DspState,
    groups: super::groups::GroupState,
    controllers: super::controller_event::ControllerState,
    projections: super::note_event::NoteProjections,
    modulation: super::voice_mod::VoiceModState,
    script: super::script_params::EngineLayers,
}

pub(super) struct Generation {    pub request: u64,
    pub script_revision: u64,
    pub prepared: Box<Prepared>,
    pub notes: usize,
    pub callbacks: usize,
    pub sequences: super::variation::SequenceState,
    pub native_cycle: u64,
    pub native_seed: u64,
    pub controls: super::control::ControlState,
    pub scripts: Box<[super::ops::ScriptBank]>,
    pub midi_object: super::MidiObject,
    pub dsp: super::dsp::DspState,
    pub groups: super::groups::GroupState,
    pub controllers: super::controller_event::ControllerState,
    pub projections: super::note_event::NoteProjections,
    pub modulation: super::voice_mod::VoiceModState,
    pub script: super::script_params::EngineLayers,
}

pub(super) struct PlanQueues {
    pending: Consumer<PlanTransfer>,
    retired: Producer<PlanTransfer>,
    /// Request of the newest adopted plan, for `PlanControl::grow_voices`.
    installed: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

/// Single control-side owner. Prepare, submit, consume retirements and destroy here.
/// Runtime and this endpoint must be destroyed off audio after processing stops.
pub struct PlanControl {
    pending: Producer<PlanTransfer>,
    retired: Consumer<PlanTransfer>,
    rate: u32,
    locals: usize,
    note_cells: usize,
    voices: usize,
    expressions: usize,
    notes: usize,
    note_params: usize,
    performances: usize,
    sequence: u64,
    /// Render lanes of the runtime (see `Runtime::set_threads`).
    lanes: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    installed: std::sync::Arc<std::sync::atomic::AtomicU64>,
    growth: Producer<super::grow::Growth>,
    grown: Consumer<super::grow::Growth>,
    pressure: super::grow::Pressure,
    note_pressure: super::grow::NotePressure,
    /// Sizes of the plans the runtime may still hold, for growth.
    live: Vec<super::grow::Dims>,
    growing: bool,
}

impl PlanControl {
    pub fn note_params_capacity(&self) -> usize { self.note_params }
    pub fn note_params_bytes(&self) -> usize { std::mem::size_of::<crate::script_params::NoteParams>() }
    pub fn note_pressure(&self) -> bool { self.note_pressure.load(std::sync::atomic::Ordering::Relaxed) >= self.note_params }

    /// Control side allocates new pages; audio adopts pointers and returns the
    /// emptied transfer for control-side destruction. Existing notes do not move.
    pub fn grow_note_params(&mut self, notes: usize) -> Result<usize, PlanError> {
        if self.growth.is_abandoned() { return Err(PlanError::Disconnected); }
        while self.grown.pop().is_ok() { self.growing = false; }
        let adopted = self.installed.load(std::sync::atomic::Ordering::Acquire) == self.sequence;
        if self.growing || !adopted { return Err(PlanError::Capacity); }
        let pages = crate::script_params::NoteParamsGrowth::build(self.note_params, notes, self.notes)
            .map_err(|_| PlanError::Capacity)?;
        let capacity = pages.capacity;
        self.growth.push(super::grow::Growth::note_params(pages)).map_err(|_| PlanError::Capacity)?;
        self.note_pressure.store(0, std::sync::atomic::Ordering::Relaxed);
        self.growing = true;
        self.note_params = capacity;
        Ok(capacity)
    }
    /// Voice slots the pool has, or is being grown to.
    pub fn voice_capacity(&self) -> usize {
        self.voices
    }

    /// Whether the audio side saw the voice pool three quarters full since
    /// the last `grow_voices`.
    pub fn voice_pressure(&self) -> bool {
        self.pressure.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Grow the voice pool to `voices` slots without stopping audio: builds the
    /// larger per-voice state here and queues it for the audio thread, which
    /// adopts it at the start of its next block. Plans submitted afterwards
    /// are built for the new size. Fails (`Capacity`) when a growth or a plan
    /// is still in flight, or `voices` is not larger; call again later.
    /// `Runtime::stats().voice_capacity` shows the adopted size.
    pub fn grow_voices(&mut self, voices: usize) -> Result<usize, PlanError> {
        if self.growth.is_abandoned() {
            return Err(PlanError::Disconnected);
        }
        while self.grown.pop().is_ok() {
            self.growing = false;
        }
        let adopted = self.installed.load(std::sync::atomic::Ordering::Acquire) == self.sequence;
        if self.growing || !adopted || voices <= self.voices {
            return Err(PlanError::Capacity);
        }
        let parallel = self.lanes.load(std::sync::atomic::Ordering::Relaxed) > 1;
        let growth = super::grow::Growth::build(voices, &self.live, parallel)
            .map_err(|_| PlanError::Capacity)?;
        self.pressure
            .store(false, std::sync::atomic::Ordering::Relaxed);
        self.growth.push(growth).map_err(|_| PlanError::Capacity)?;
        self.growing = true;
        self.voices = voices;
        Ok(voices)
    }

    /// Rejection returns the exact owned plan to the caller; nothing is published.
    pub fn submit(&mut self, mut prepared: Box<Prepared>) -> Result<u64, RejectedPlan> {
        let reason = if self.pending.is_abandoned() {
            Some(PlanError::Disconnected)
        } else if prepared.rate != self.rate {
            Some(PlanError::SampleRate)
        } else if prepared.programs.iter().any(|p| p.locals > self.locals) {
            Some(PlanError::LocalCapacity)
        } else if prepared.note_cells > self.note_cells {
            Some(PlanError::NoteStateCapacity)
        } else if self.sequence == u64::MAX {
            Some(PlanError::SequenceExhausted)
        } else if self.pending.is_full() {
            Some(PlanError::Capacity)
        } else {
            None
        };
        if let Some(reason) = reason {
            return Err(RejectedPlan { reason, prepared });
        }
        if super::trace_report::configure(&mut prepared).is_err() { return Err(RejectedPlan {reason:PlanError::Capacity,prepared}); }
        let dsp = match super::dsp::DspState::new(
            &prepared,
            self.voices,
            self.expressions,
            self.lanes.load(std::sync::atomic::Ordering::Relaxed),
        ) {
            Ok(dsp) => dsp,
            Err(_) => {
                return Err(RejectedPlan {
                    reason: PlanError::Capacity,
                    prepared,
                });
            }
        };
        let groups = match super::groups::GroupState::new(
            prepared.group_count,
            self.notes,
            prepared.stages.len(),
        ) {
            Ok(groups) => groups,
            Err(_) => {
                return Err(RejectedPlan {
                    reason: PlanError::Capacity,
                    prepared,
                });
            }
        };
        let controllers =
            match super::controller_event::ControllerState::new(&prepared, self.performances) {
                Ok(state) => state,
                Err(_) => {
                    return Err(RejectedPlan {
                        reason: PlanError::Capacity,
                        prepared,
                    });
                }
            };
        let projections =
            match super::note_event::NoteProjections::new(prepared.stages.len(), self.notes) {
                Ok(state) => state,
                Err(_) => {
                    return Err(RejectedPlan {
                        reason: PlanError::Capacity,
                        prepared,
                    });
                }
            };
        let modulation =
            match super::voice_mod::VoiceModState::new(&prepared.voice_modulation, self.voices) {
                Ok(state) => state,
                Err(_) => {
                    return Err(RejectedPlan {
                        reason: PlanError::Capacity,
                        prepared,
                    });
                }
            };
        let request = self.sequence + 1;
        let script = super::script_params::EngineLayers::new(&prepared);
        let sequences = super::variation::SequenceState::new(&prepared);
        let controls = super::control::ControlState::new(&prepared);
        let scripts = prepared.script_initial.iter().map(super::ops::ScriptInitial::bank).collect();
        let midi_object = prepared.midi_object.clone();
        let dims = super::grow::Dims::of(request, &prepared);
        match self.pending.push(PlanTransfer {
            request,
            prepared,
            sequences,
            controls,
            scripts,
            midi_object,
            dsp,
            groups,
            controllers,
            projections,
            modulation,
            script,
        }) {
            Ok(()) => {
                self.sequence = request;
                self.live.push(dims);
                Ok(request)
            }
            Err(PushError::Full(plan)) => Err(RejectedPlan {
                reason: PlanError::Capacity,
                prepared: plan.prepared,
            }),
        }
    }

    pub fn retired(&mut self) -> Option<PlanTransfer> {
        let plan = self.retired.pop().ok()?;
        self.live.retain(|d| d.request != plan.request);
        Some(plan)
    }
}

impl Runtime {
    /// Control-side construction. `generations` includes the active generation;
    /// `queued` independently bounds each direction of the ownership transfer.
    pub fn with_plan_updates(
        plan: Prepared,
        limits: Limits,
        generations: usize,
        queued: usize,
    ) -> Result<(Self, PlanControl), Error> {
        Self::with_plan_updates_and_note_capacity(plan, limits, generations, queued, limits.notes)
    }

    /// Prepare a smaller initial note-parameter pool, growable from PlanControl.
    /// Other Limits::notes storage retains the full ceiling.
    pub fn with_plan_updates_and_note_capacity(
        plan: Prepared, limits: Limits, generations: usize, queued: usize, initial_notes: usize,
    ) -> Result<(Self, PlanControl), Error> {
        if generations < 2 || queued == 0 {
            return Err(Error::InvalidInput);
        }
        let mut runtime = Self::new_with_note_params(plan, limits, initial_notes)?;
        // Move the initial generation into the larger control-side arena. No live
        // notes exist yet, and the emptied old arena owns no prepared assets.
        let mut slots = Arena::new(runtime.plans.runtime, generations);
        let initial = runtime
            .plans
            .take(runtime.active_plan.0)
            .ok_or(Error::StaleHandle)?;
        runtime.active_plan = PlanId(slots.insert(initial)?);
        runtime.plans = slots;
        let (pending, incoming) = RingBuffer::new(queued);
        let (retired, outgoing) = RingBuffer::new(queued);
        let (growth, incoming_growth) = RingBuffer::new(1);
        let (outgoing_growth, grown) = RingBuffer::new(1);
        let installed = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let live = vec![super::grow::Dims::of(
            0,
            &runtime.plans.get(runtime.active_plan.0).unwrap().prepared,
        )];
        runtime.growth = Some(super::grow::GrowthQueues {
            incoming: incoming_growth,
            outgoing: outgoing_growth,
            waker: None,
        });
        let control = PlanControl {
            pending,
            retired: outgoing,
            rate: runtime.rate,
            locals: runtime.behavior_stride,
            note_cells: runtime.note_stride,
            voices: limits.voices,
            expressions: limits.expressions,
            notes: limits.notes,
            note_params: runtime.note_params.capacity(),
            performances: limits.performances,
            sequence: 0,
            lanes: runtime.lanes.clone(),
            installed: installed.clone(),
            growth,
            grown,
            pressure: runtime.voice_pressure.clone(),
            note_pressure: runtime.note_pressure.clone(),
            live,
            growing: false,
        };
        runtime.plan_queues = Some(PlanQueues {
            pending: incoming,
            retired,
            installed,
        });
        Ok((runtime, control))
    }

    pub fn active_plan(&self) -> PlanId {
        self.active_plan
    }
    pub fn plan_count(&self) -> usize {
        self.plans.count()
    }
    pub fn note_plan(&self, note: NoteId) -> Result<PlanId, Error> {
        Ok(self.notes.get(note.0).ok_or(Error::StaleHandle)?.plan)
    }
    pub fn plan_request(&self, plan: PlanId) -> Result<u64, Error> {
        Ok(self.plans.get(plan.0).ok_or(Error::StaleHandle)?.request)
    }

    /// Move unused, inactive generations to the control thread. A full or abandoned
    /// return queue leaves ownership in its existing arena slot, never on the stack.
    pub fn collect_retired_plans(&mut self) -> usize {
        let Some(queues) = &mut self.plan_queues else {
            return 0;
        };
        if queues.retired.is_abandoned() {
            return 0;
        }
        let mut count = 0;
        for index in 0..self.plans.slots.len() {
            let slot = &self.plans.slots[index];
            if index == self.active_plan.0.index
                || !slot.value.as_ref().is_some_and(|g| {
                    g.notes == 0
                        && g.callbacks == 0
                        && g.controls.pending == 0
                        && !g.dsp.buses.active()
                })
            {
                continue;
            }
            if queues.retired.is_full() {
                break;
            }
            let id = self.plans.id(index);
            let Some(generation) = self.plans.take(id) else {
                continue;
            };
            match queues.retired.push(PlanTransfer {
                request: generation.request,
                prepared: generation.prepared,
                sequences: generation.sequences,
                controls: generation.controls,
                scripts: generation.scripts,
                midi_object: generation.midi_object,
                dsp: generation.dsp,
                groups: generation.groups,
                controllers: generation.controllers,
                projections: generation.projections,
                modulation: generation.modulation,
                script: generation.script,
            }) {
                Ok(()) => count += 1,
                Err(PushError::Full(plan)) => {
                    self.plans.restore(
                        id,
                        Generation {
                            script_revision: 0,
                            native_cycle: 0,
                            native_seed: 0,                            request: plan.request,
                            prepared: plan.prepared,
                            sequences: plan.sequences,
                            controls: plan.controls,
                            scripts: plan.scripts,
                            midi_object: plan.midi_object,
                            dsp: plan.dsp,
                            groups: plan.groups,
                            controllers: plan.controllers,
                            projections: plan.projections,
                            modulation: plan.modulation,
                            script: plan.script,
                            notes: 0,
                            callbacks: 0,
                        },
                    );
                    break;
                }
            }
        }
        if self.signal_trace { self.signal_trace = self.plans.slots.iter().any(|s| s.value.as_ref().is_some_and(|g| g.prepared.signal_trace.is_some())); }
        count
    }

    /// Apply at most one queued plan at the current sample boundary. Existing notes
    /// retain their generation. Backpressure leaves the pending plan and active plan
    /// untouched; call again after retirements/terminals are consumed.
    pub fn poll_plan_update(&mut self) -> Result<Option<u64>, PlanError> {
        self.apply_due();
        self.collect_retired_plans();
        let queues = self.plan_queues.as_mut().ok_or(PlanError::Disabled)?;
        if queues.retired.is_abandoned() {
            return Err(PlanError::Disconnected);
        }
        if queues.pending.is_empty() {
            return Ok(None);
        }
        if queues.retired.is_full() || self.plans.available() == 0 {
            return Err(PlanError::Capacity);
        }
        let Ok(plan) = queues.pending.pop() else {
            return Ok(None);
        };
        let request = plan.request;
        self.signal_trace |= plan.prepared.signal_trace.is_some();
        // The single audio writer preflighted a non-quarantined slot before popping.
        self.active_plan = PlanId(
            self.plans
                .insert(Generation {
                            script_revision: 0,
                            native_cycle: 0,
                            native_seed: 0,                    request,
                    prepared: plan.prepared,
                    sequences: plan.sequences,
                    controls: plan.controls,
                    scripts: plan.scripts,
                    midi_object: plan.midi_object,
                    dsp: plan.dsp,
                    groups: plan.groups,
                    controllers: plan.controllers,
                    projections: plan.projections,
                    modulation: plan.modulation,
                    script: plan.script,
                    notes: 0,
                    callbacks: 0,
                })
                .expect("reserved plan generation slot"),
        );
        self.collect_retired_plans();
        self.start_plan_programs();
        if let Some(queues) = &self.plan_queues {
            queues
                .installed
                .store(request, std::sync::atomic::Ordering::Release);
        }
        Ok(Some(request))
    }
}
