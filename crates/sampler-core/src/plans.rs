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

/// Owned plan and its control request identity. Dropping it destroys the assets.
pub struct PlanTransfer {
    pub request: u64,
    pub prepared: Box<Prepared>,
}

pub(super) struct Generation {
    pub request: u64,
    pub prepared: Box<Prepared>,
    pub notes: usize,
}

pub(super) struct PlanQueues {
    pending: Consumer<PlanTransfer>,
    retired: Producer<PlanTransfer>,
}

/// Single control-side owner. Prepare, submit, consume retirements and destroy here.
/// Runtime and this endpoint must be destroyed off audio after processing stops.
pub struct PlanControl {
    pending: Producer<PlanTransfer>,
    retired: Consumer<PlanTransfer>,
    rate: u32,
    locals: usize,
    sequence: u64,
}

impl PlanControl {
    /// Rejection returns the exact owned plan to the caller; nothing is published.
    pub fn submit(&mut self, prepared: Box<Prepared>) -> Result<u64, RejectedPlan> {
        let reason = if self.pending.is_abandoned() {
            Some(PlanError::Disconnected)
        } else if prepared.rate != self.rate {
            Some(PlanError::SampleRate)
        } else if prepared.programs.iter().any(|p| p.locals > self.locals) {
            Some(PlanError::LocalCapacity)
        } else if self.sequence == u64::MAX {
            Some(PlanError::SequenceExhausted)
        } else {
            None
        };
        if let Some(reason) = reason {
            return Err(RejectedPlan { reason, prepared });
        }
        let request = self.sequence + 1;
        match self.pending.push(PlanTransfer { request, prepared }) {
            Ok(()) => {
                self.sequence = request;
                Ok(request)
            }
            Err(PushError::Full(plan)) => Err(RejectedPlan {
                reason: PlanError::Capacity,
                prepared: plan.prepared,
            }),
        }
    }

    pub fn retired(&mut self) -> Option<PlanTransfer> {
        self.retired.pop().ok()
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
        if generations < 2 || queued == 0 {
            return Err(Error::InvalidInput);
        }
        let mut runtime = Self::new(plan, limits)?;
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
        let control = PlanControl {
            pending,
            retired: outgoing,
            rate: runtime.rate,
            locals: runtime.behavior_stride,
            sequence: 0,
        };
        runtime.plan_queues = Some(PlanQueues {
            pending: incoming,
            retired,
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
                || !slot.value.as_ref().is_some_and(|g| g.notes == 0)
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
            }) {
                Ok(()) => count += 1,
                Err(PushError::Full(plan)) => {
                    self.plans.restore(
                        id,
                        Generation {
                            request: plan.request,
                            prepared: plan.prepared,
                            notes: 0,
                        },
                    );
                    break;
                }
            }
        }
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
        // The single audio writer preflighted a non-quarantined slot before popping.
        self.active_plan = PlanId(
            self.plans
                .insert(Generation {
                    request,
                    prepared: plan.prepared,
                    notes: 0,
                })
                .expect("reserved plan generation slot"),
        );
        self.collect_retired_plans();
        Ok(Some(request))
    }
}
