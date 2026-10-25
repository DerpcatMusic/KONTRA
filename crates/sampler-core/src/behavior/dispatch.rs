//! Event dispatch, independent-event ordering and bounded execution policy.
//! The ready stack is one synchronous transaction. The FIFO is for genuine
//! preemption and later independent events, never nested stage continuations.
use super::{BehaviorId, Instruction, Outcome, Ready};
use crate::{Error, Runtime};

/// What happens when runnable script work exhausts its execution allowance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BehaviorOverload {
    /// Explicit cooperative execution: resume in a later block, in event order.
    #[default]
    Defer,
    /// Real-time playback: fail and release the callback's owned work now.
    /// Never replay ordinary note processing at a later musical timestamp.
    /// The one exception is engine-effect outbox back-pressure, which waits for the
    /// host drain under every policy (UI-only writes never wait or fault; see `OutboxMerge`).
    Abort,
}

/// One allowance belongs to a host block, never to a render fragment.
#[derive(Clone, Copy)]
pub(crate) struct DispatchBudget {
    pub(crate) limit: usize,
    pub(crate) remaining: usize,
    pub(crate) host_block: bool,
    pub(crate) time_left: Option<std::time::Duration>,
    pub(crate) overload: BehaviorOverload,
}
impl Default for DispatchBudget {
    fn default() -> Self {
        Self {
            limit: usize::MAX,
            remaining: usize::MAX,
            host_block: false,
            time_left: None,
            overload: BehaviorOverload::Defer,
        }
    }
}

impl Runtime {
    pub fn set_behavior_overload(&mut self, policy: BehaviorOverload) {
        self.behavior_budget.overload = policy;
    }

    /// Loader-only preparation at the current sample time. Completes runnable
    /// initialization, not authored waits/listeners or asynchronous operations.
    /// Total instructions remain bounded even when a callback never terminates.
    /// Restores the caller's execution policy and allowance on every result.
    pub fn prepare_behaviors(
        &mut self,
        fuel: usize,
        canceled: &(dyn Fn() -> bool + Sync),
        service: &mut dyn FnMut(&mut Runtime) -> Result<(), Error>,
    ) -> Result<(), Error> {
        if self.dispatching_behavior {
            return Err(Error::Busy);
        }
        let saved = self.behavior_budget;
        self.behavior_budget.overload = BehaviorOverload::Defer;
        self.begin_behavior_block(fuel, None);
        let result = (|| {
            if canceled() {
                return Err(Error::Canceled);
            }
            self.start_plan_programs();
            self.drain_behavior();
            service(self)?;
            while !self.yielded.is_empty() {
                if canceled() {
                    return Err(Error::Canceled);
                }
                if self.behavior_budget.remaining == 0 {
                    return Err(Error::Capacity);
                }
                self.resume_yielded();
                service(self)?;
            }
            if self.started_plan != Some(self.active_plan) {
                return Err(Error::Capacity);
            }
            let mut failure = None;
            self.visit_behavior_progress(|callback| match callback.outcome {
                Some(Outcome::Fault(error)) => failure = Some(error),
                Some(Outcome::FuelExhausted) => failure = Some(Error::Capacity),
                _ => {}
            });
            failure.map_or(Ok(()), Err)
        })();
        self.behavior_budget = saved;
        result
    }

    /// Set the shared callback instruction allowance (default unlimited).
    /// Exhaustion follows the explicitly selected overload policy.
    pub fn set_behavior_block_fuel(&mut self, fuel: usize) {
        self.behavior_budget.limit = fuel;
        self.behavior_budget.remaining = fuel;
        self.behavior_budget.host_block = false;
        self.behavior_budget.time_left = None;
    }

    /// One host block's shared callback allowance, installed before its events.
    /// MIDI, empty renders and render fragments consume the same allowance.
    pub fn begin_behavior_block(&mut self, fuel: usize, time: Option<std::time::Duration>) {
        self.behavior_budget.limit = fuel;
        self.behavior_budget.remaining = fuel;
        self.behavior_budget.time_left = time;
        self.behavior_budget.host_block = true;
    }

    pub fn behavior_block_fuel(&self) -> usize {
        self.behavior_budget.limit
    }

    pub(crate) fn queue_behavior(&mut self, id: BehaviorId) {
        // Only independent events wait behind an earlier preemption. Work
        // admitted by the active dispatcher belongs to its synchronous event
        // transaction (stage forwarding, receivers, generated notes). Putting
        // it in the FIFO would make a queue pass advance musical time.
        if self
            .behaviors
            .get(id.0)
            .is_some_and(|c| c.external.is_some_and(|state| state.blocks_dispatch()))
        {
            return;
        }
        let plan = self.behaviors.get(id.0).map(|c| c.owner);
        let plan = plan.and_then(|o| self.behavior_plan(o).ok());
        if plan.is_some_and(|plan| self.queues_deferred(plan)) {
            self.defer_behavior(id);
            return;
        }
        self.push_behavior_work(Ready::Resume {
            id,
            fuel: self.behavior_fuel,
        });
    }

    pub(crate) fn drain_behavior(&mut self) {
        if self.publishing_receiver
            || self.dispatching_behavior
            || self.behavior_ready.is_empty() && !self.has_ready_zone_completions()
        {
            return;
        }
        let time = self.behavior_budget.time_left;
        let started = time.map(|_| std::time::Instant::now());
        let mut time_check_steps = 64usize;
        self.dispatching_behavior = true;
        // A bounded explicit stack preserves synchronous nested callback ordering.
        // Each suspended caller retains its own remaining fuel, never a Rust frame.
        loop {
            while let Some(ready) = self.behavior_ready.last().copied() {
                let index = self.behavior_ready.len() - 1;
                let (id, fuel) = match ready {
                    Ready::Resume { id, fuel } => (id, fuel),
                    Ready::Release { note, stage } => {
                        self.behavior_ready.pop();
                        self.note_events[note.0.index].release_queued = false;
                        if self.notes.get(note.0).unwrap().gate() {
                            if let Some(stage) = stage {
                                self.advance_release_stage(note, stage);
                            } else {
                                self.key_up_with_cause(note, None, crate::ReleaseCause::Script)
                                    .expect("retained generated note release");
                            }
                        }
                        self.notes.get_mut(note.0).unwrap().work -= 1;
                        continue;
                    }
                };
                // A finished callback retires at once; a stale entry is skipped.
                let Some(c) = self.behaviors.get(id.0).copied() else {
                    self.behavior_ready.pop();
                    continue;
                };
                if c.outcome.is_some() {
                    self.release_controller_reserve(id);
                    self.behavior_ready.pop();
                    continue;
                }
                let plan = &self
                    .plans
                    .get(self.behavior_plan(c.owner).unwrap().0)
                    .unwrap()
                    .prepared;
                let Some(op) = plan.programs[c.program].code.get(c.pc).copied() else {
                    self.flush_zone_parameters(id);
                    self.flush_deferred(id);
                    self.behaviors.get_mut(id.0).unwrap().outcome = Some(Outcome::Finished);
                    self.release_controller_reserve(id);
                    self.behavior_ready.remove(index);
                    continue;
                };
                if time_check_steps >= 64 {
                    time_check_steps = 0;
                    if started
                        .zip(time)
                        .is_some_and(|(start, left)| start.elapsed() >= left)
                    {
                        self.behavior_budget.remaining = 0;
                    }
                }
                if fuel == 0 || self.behavior_budget.remaining == 0 {
                    self.behavior_ready.remove(index);
                    self.yield_behavior(id);
                    continue;
                }
                // Bound the interval between clock checks without timing every instruction.
                let slice = if started.is_some() { 256 } else { usize::MAX };
                let ran =
                    self.run_straight(id, fuel.min(self.behavior_budget.remaining).min(slice));
                if ran > 0 {
                    time_check_steps = time_check_steps.saturating_add(ran);
                    self.behavior_budget.remaining =
                        self.behavior_budget.remaining.saturating_sub(ran);
                    self.behavior_ready[index] = Ready::Resume {
                        id,
                        fuel: fuel - ran,
                    };
                    continue;
                }
                self.behaviors.get_mut(id.0).unwrap().last_pc = c.pc;
                let cost = if matches!(op, Instruction::AdvanceSourceNote) {
                    match c
                        .owner
                        .note()
                        .and_then(|note| self.source_note_work_cost(note))
                    {
                        Ok(cost) if cost <= self.behavior_fuel => cost,
                        Ok(_) => {
                            self.fail_behavior(id, Outcome::Fault(Error::Capacity));
                            self.behavior_ready.remove(index);
                            continue;
                        }
                        Err(error) => {
                            self.fail_behavior(id, Outcome::Fault(error));
                            self.behavior_ready.remove(index);
                            continue;
                        }
                    }
                } else {
                    1
                };
                if cost > fuel || cost > self.behavior_budget.remaining {
                    self.behavior_ready.remove(index);
                    self.yield_behavior(id);
                    continue;
                }
                time_check_steps = time_check_steps.saturating_add(cost);
                self.behavior_ready[index] = Ready::Resume {
                    id,
                    fuel: fuel - cost,
                };
                self.behavior_budget.remaining =
                    self.behavior_budget.remaining.saturating_sub(cost);
                self.behaviors.get_mut(id.0).unwrap().pc += 1;
                let stepped = self.behavior_step(id, c.owner, op);
                if !matches!(stepped, Ok(false) | Err(Error::ClosedNote)) {
                    // Zone commit precedes deferred attacks at actual wait/end.
                    // This ordering is KONTRA policy; no native ordering claim.
                    if stepped == Ok(true) {
                        self.flush_zone_parameters(id);
                    } else if stepped.is_err() {
                        self.cancel_zone_parameters(id);
                    }
                    self.flush_deferred(id);
                }
                match stepped {
                    Ok(true) => {
                        if self.behaviors.get(id.0).unwrap().outcome.is_some() {
                            self.release_controller_reserve(id);
                        }
                        self.behavior_ready.remove(index);
                    }
                    Ok(false) => {
                        // Host-service backpressure is not exhausted script fuel, and
                        // UI work can be replayed. The host drains the outbox at block
                        // end, so the callback waits for the next block under any
                        // overload policy. Failing it would drop the callback of any
                        // note that follows an outbox-filling note in the same block.
                        if std::mem::take(&mut self.ops.outbox_full) {
                            self.behavior_ready.remove(index);
                            let plan = self.behavior_plan(c.owner).unwrap();
                            self.plans.get_mut(plan.0).unwrap().dispatch_preempted = true;
                            self.defer_behavior(id);
                        }
                    }
                    // Kontakt ignores a script's call on a note that already ended
                    // (play_note on a released parent, forwarding a dropped note).
                    Err(Error::ClosedNote) => {}
                    Err(error) => {
                        self.fail_behavior(id, Outcome::Fault(error));
                        self.behavior_ready.remove(index);
                    }
                }
            }
            // Entries have settled before completion admissions can push work.
            // One completion at a time preserves callback/waiter order with LIFO
            // dispatch and keeps callback execution on this bounded explicit stack.
            let serviced = match self.service_zone_completions() {
                Ok(serviced) => serviced,
                Err(error) => {
                    self.fault = Some((0, error));
                    false
                }
            };
            if self.behavior_ready.is_empty() && !serviced {
                break;
            }
        }
        if let Some((start, left)) = started.zip(time) {
            self.behavior_budget.time_left = Some(left.saturating_sub(start.elapsed()));
        }
        self.dispatching_behavior = false;
    }

    /// Whether `queue_behavior` sends `plan`'s new work to the FIFO (true) or
    /// the LIFO ready stack (false). Fanouts order their admissions by this.
    pub(crate) fn queues_deferred(&self, plan: crate::PlanId) -> bool {
        !self.dispatching_behavior
            && self.yielded.iter().any(|&y| self.yielded_plan(y) == Some(plan))
    }

    pub(crate) fn yielded_plan(&self, id: BehaviorId) -> Option<crate::PlanId> {
        let c = self.behaviors.get(id.0)?;
        if c.outcome.is_some() {
            return None;
        }
        self.behavior_plan(c.owner).ok()
    }

    /// Out of fuel for this block: continue next block, unless it has been
    /// continuously preempted for a second without an intentional wait.
    fn yield_behavior(&mut self, id: BehaviorId) {
        if self.behavior_budget.overload == BehaviorOverload::Abort {
            self.behavior_budget_failures = self.behavior_budget_failures.saturating_add(1);
            self.fail_behavior(id, Outcome::FuelExhausted);
            return;
        }
        let now = self.now;
        let c = self.behaviors.get_mut(id.0).unwrap();
        let since = *c.yielded_at.get_or_insert(now);
        if now - since >= u64::from(self.rate) {
            self.behavior_budget_failures = self.behavior_budget_failures.saturating_add(1);
            self.fail_behavior(id, Outcome::FuelExhausted);
            return;
        }
        let owner = c.owner;
        let plan = self
            .behavior_plan(owner)
            .expect("live callback pins its plan");
        self.plans.get_mut(plan.0).unwrap().dispatch_preempted = true;
        self.preemptions = self.preemptions.saturating_add(1);
        self.longest_preempted = self.longest_preempted.max(now - since);
        self.defer_behavior(id);
    }

    /// One FIFO entry per live continuation. Finished/cancelled handles can
    /// remain until a render; reclaim them before capacity can force allocation.
    fn defer_behavior(&mut self, id: BehaviorId) {
        if self.yielded.contains(&id) {
            return;
        }
        if self.yielded.len() == self.yielded.capacity() {
            let behaviors = &self.behaviors;
            self.yielded
                .retain(|id| behaviors.get(id.0).is_some_and(|c| c.outcome.is_none()));
        }
        if self.yielded.len() == self.yielded.capacity() {
            self.fail_behavior(id, Outcome::Fault(Error::Capacity));
            return;
        }
        self.yielded.push_back(id);
    }

    /// Resume preempted callbacks with fresh fuel, oldest first. A callback
    /// stays queued while an earlier one of its instrument preempted again
    /// in this pass.
    pub(crate) fn resume_yielded(&mut self) {
        // Plan state records actual preemption. Queue length arithmetic cannot
        // distinguish a continuation from a budget failure, and becomes invalid
        // when cancelled/stale entries are reclaimed under capacity pressure.
        for slot in &mut self.plans.slots {
            if let Some(plan) = &mut slot.value {
                plan.dispatch_preempted = false;
            }
        }
        let pending = self.yielded.len();
        for _ in 0..pending {
            let Some(id) = self.yielded.pop_front() else {
                break;
            };
            let Some(plan) = self.yielded_plan(id) else {
                continue;
            };
            let blocked = self.plans.get(plan.0).unwrap().dispatch_preempted;
            if blocked {
                self.yielded.push_back(id);
                continue;
            }
            self.push_behavior_work(Ready::Resume {
                id,
                fuel: self.behavior_fuel,
            });
            self.drain_behavior();
        }
    }

    pub(crate) fn push_behavior_work(&mut self, ready: Ready) {
        assert!(
            self.behavior_ready.len() < self.behavior_ready.capacity(),
            "reserved callback dispatch capacity"
        );
        self.behavior_ready.push(ready);
    }
}
