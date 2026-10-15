//! Bounded control/UI handoff. Owned payloads return to their producer for reuse
//! or destruction; the audio side never releases their backing storage.
use super::{ControlWrite, Error, PlanId, Runtime};
use rtrb::{Consumer, Producer, PushError, RingBuffer};

#[derive(Debug)]
pub enum ControlOperation {
    ArrayFileCapture(crate::ArrayFileCompletion),
    ArrayFileComplete(crate::ArrayFileCompletion),
    MidiComplete(crate::MidiCompletion),
    MidiCapture(crate::MidiCompletion),
    Invoke(super::ControlContext, ControlWrite),
    HostParameter(super::ControlContext, u16, f64),
    InvokeWidget(super::ControlContext, Vec<crate::WidgetEdit>),
    CaptureWidget(Vec<crate::WidgetEdit>),
    CaptureScriptState(crate::ScriptStateBuffer),
    RestoreScriptState(crate::ScriptStateBuffer),
    CaptureSnapshotFence(Option<super::SnapshotOverlayFence>),
    NativeSnapshotOverlay(super::NativeSnapshotOverlay),
    Edit(Box<[ControlWrite]>),
    Recall(Box<[ControlWrite]>),
    Capture(Box<[ControlWrite]>),
}
impl ControlOperation {
    fn len(&self) -> usize {
        match self {
            Self::ArrayFileCapture(..)
            | Self::ArrayFileComplete(..)
            | Self::MidiComplete(..)
            | Self::MidiCapture(..)
            | Self::Invoke(..)
            | Self::HostParameter(..)
            | Self::CaptureSnapshotFence(..) => 1,
            Self::InvokeWidget(_, v) | Self::CaptureWidget(v) => v.len(),
            Self::Edit(v) | Self::Recall(v) | Self::Capture(v) => v.len(),
            Self::CaptureScriptState(v) | Self::RestoreScriptState(v) => {
                v.values.len().saturating_add(v.callbacks.len())
            }
            Self::NativeSnapshotOverlay(v) => v.state.values.len()
                .saturating_add(v.state.callbacks.len()).saturating_add(v.native.len())
                .saturating_add(v.epochs.len()),
        }
    }
}
#[derive(Debug)]
pub struct ControlRequest {
    pub plan: PlanId,
    /// Capture ignores this field; edit/recall use it as optimistic concurrency.
    pub expected_revision: Option<u64>,
    pub operation: ControlOperation,
}
#[derive(Debug)]
pub struct ControlReply {
    pub request: u64,
    pub behavior: Option<crate::BehaviorId>,
    /// The first refused callback preflight in this rejected operation, captured
    /// before subsequent audio work can change its owners. Other Capacity causes
    /// and successful operations leave this empty.
    pub admission_failure: Option<super::BehaviorAdmissionFailure>,
    pub command: ControlRequest,
    /// (written/captured count, coherent revision), or an explicit rejection.
    /// Capture writes only the returned prefix. Error leaves its buffer untouched.
    pub result: Result<(usize, u64), Error>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlQueueError {
    Capacity,
    PayloadLimit,
    Disconnected,
    SequenceExhausted,
    Disabled,
}
#[derive(Debug)]
pub struct RejectedControls {
    pub reason: ControlQueueError,
    pub command: ControlRequest,
}

/// Single non-audio producer; receives every accepted payload back, including
/// rejected edits. Destroy both endpoints only off audio after processing stops.
pub struct ControlClient {
    pending: Producer<ControlReply>,
    replies: Consumer<ControlReply>,
    max_values: usize,
    sequence: u64,
}
pub(crate) struct ControlQueues {
    pending: Consumer<ControlReply>,
    replies: Producer<ControlReply>,
    retained: Option<ControlReply>,
    record_admission: bool,
    admission_failure: Option<super::BehaviorAdmissionFailure>,
}
impl ControlClient {
    #[allow(
        clippy::result_large_err,
        reason = "Return the bounded caller-owned command intact without allocating a rejection wrapper"
    )]
    pub fn submit(&mut self, command: ControlRequest) -> Result<u64, RejectedControls> {
        let reason = if self.pending.is_abandoned() {
            Some(ControlQueueError::Disconnected)
        } else if command.operation.len() > self.max_values {
            Some(ControlQueueError::PayloadLimit)
        } else if self.sequence == u64::MAX {
            Some(ControlQueueError::SequenceExhausted)
        } else if self.pending.is_full() {
            Some(ControlQueueError::Capacity)
        } else {
            None
        };
        if let Some(reason) = reason {
            return Err(RejectedControls { reason, command });
        }
        let request = self.sequence + 1;
        match self.pending.push(ControlReply {
            request,
            behavior: None,
            admission_failure: None,
            command,
            result: Err(Error::InvalidInput),
        }) {
            Ok(()) => {
                self.sequence = request;
                Ok(request)
            }
            Err(PushError::Full(reply)) => Err(RejectedControls {
                reason: ControlQueueError::Capacity,
                command: reply.command,
            }),
        }
    }
    pub fn reply(&mut self) -> Option<ControlReply> {
        self.replies.pop().ok()
    }
}
impl Runtime {
    /// Enable once, off audio. `max_values` bounds work per transaction independently
    /// of queue depth. Multiple UI/language producers must serialize on control.
    pub fn with_control_updates(
        mut self,
        queued: usize,
        max_values: usize,
    ) -> Result<(Self, ControlClient), Error> {
        if queued == 0 || self.control_queues.is_some() {
            return Err(Error::InvalidInput);
        }
        let (pending, incoming) = RingBuffer::new(queued);
        let (replies, outgoing) = RingBuffer::new(queued);
        self.control_queues = Some(ControlQueues {
            pending: incoming,
            replies,
            retained: None,
            record_admission: false,
            admission_failure: None,
        });
        Ok((
            self,
            ControlClient {
                pending,
                replies: outgoing,
                max_values,
                sequence: 0,
            },
        ))
    }

    /// Apply at most one command at this sample boundary. Response pressure leaves
    /// commands untouched; no accepted edit loses its acknowledgement. No coalescing.
    pub fn poll_control_update(&mut self) -> Result<Option<u64>, ControlQueueError> {
        self.poll_control_update_with_snapshot_gate(|| true)
    }

    /// Inspect host-owned audio state only for a snapshot overlay whose complete
    /// core preflight succeeded. Other operations never invoke `snapshot_gate`.
    pub fn poll_control_update_with_snapshot_gate(
        &mut self,
        snapshot_gate: impl FnOnce() -> bool,
    ) -> Result<Option<u64>, ControlQueueError> {
        let queues = self
            .control_queues
            .as_mut()
            .ok_or(ControlQueueError::Disabled)?;
        if queues.replies.is_abandoned() {
            return Err(ControlQueueError::Disconnected);
        }
        if queues.replies.is_full() {
            return Err(ControlQueueError::Capacity);
        }
        if let Some(reply) = queues.retained.take() {
            let request = reply.request;
            if let Err(PushError::Full(reply)) = queues.replies.push(reply) {
                queues.retained = Some(reply);
                return Err(ControlQueueError::Capacity);
            }
            return Ok(Some(request));
        }
        let Ok(mut reply) = queues.pending.pop() else {
            return Ok(None);
        };
        let command = &mut reply.command;
        // Share ordering with direct controls and the native musical timeline.
        if !matches!(command.operation, ControlOperation::NativeSnapshotOverlay(_) | ControlOperation::CaptureSnapshotFence(_)) {
            self.apply_due();
        }
        let queues = self.control_queues.as_mut().unwrap();
        queues.admission_failure = None;
        queues.record_admission = true;
        reply.result = match &mut command.operation {
            ControlOperation::ArrayFileCapture(output) => self
                .capture_array_file(command.plan, output)
                .map(|_| (1, self.control_revision(command.plan).unwrap_or(0))),
            ControlOperation::ArrayFileComplete(output) => self
                .complete_array_file(command.plan, output)
                .map(|_| (1, self.control_revision(command.plan).unwrap_or(0))),
            ControlOperation::MidiComplete(output) => self
                .complete_midi(command.plan, output)
                .map(|_| (1, self.control_revision(command.plan).unwrap_or(0))),
            ControlOperation::MidiCapture(output) => self
                .capture_midi(command.plan, output)
                .map(|_| (1, self.control_revision(command.plan).unwrap_or(0))),
            ControlOperation::Invoke(context, write) => self
                .invoke_control(*context, command.plan, command.expected_revision, *write)
                .map(|(revision, behavior)| {
                    reply.behavior = behavior;
                    (1, revision)
                }),
            ControlOperation::InvokeWidget(context, edits) => self
                .invoke_widget(*context, command.plan, command.expected_revision, edits)
                .map(|(revision, behavior)| {
                    reply.behavior = behavior;
                    (edits.len(), revision)
                }),
            ControlOperation::HostParameter(context, address, value) => self
                .dispatch_host_parameter_in(
                    *context,
                    command.plan,
                    command.expected_revision,
                    *address,
                    *value,
                )
                .map(|revision| (1, revision)),
            ControlOperation::CaptureWidget(output) => self.capture_widgets(command.plan, output),
            ControlOperation::CaptureScriptState(output) => {
                self.capture_script_state(command.plan, output)
            }
            ControlOperation::RestoreScriptState(state) => {
                self.restore_script_state(command.plan, command.expected_revision, state)
            }
            ControlOperation::CaptureSnapshotFence(output) => self.snapshot_overlay_fence(command.plan).map(|fence| {
                *output = Some(fence);
                (1, fence.control_revision)
            }),
            ControlOperation::NativeSnapshotOverlay(overlay) => {
                if command.plan != overlay.expected.plan
                    || (overlay.linearized_ingress() && command.expected_revision.is_some())
                    || command.expected_revision.is_some_and(|revision| revision != overlay.expected.control_revision) {
                    overlay.refusal = Some(super::SnapshotOverlayRefusal::InvalidInput);
                    Err(Error::InvalidInput)
                } else {
                    self.apply_snapshot_overlay_with_gate(overlay, snapshot_gate)
                }
            }
            ControlOperation::Edit(writes) => self
                .edit_controls_now(command.plan, command.expected_revision, writes)
                .map(|rev| (writes.len(), rev)),
            ControlOperation::Recall(writes) => self
                .recall_controls(command.plan, command.expected_revision, writes)
                .map(|rev| (writes.len(), rev)),
            ControlOperation::Capture(output) => self.capture_controls(command.plan, output),
        };
        let request = reply.request;
        let queues = self.control_queues.as_mut().unwrap();
        queues.record_admission = false;
        let failure = queues.admission_failure.take();
        if reply.result == Err(Error::Capacity) {
            reply.admission_failure = failure;
        }
        // A consumer only frees slots. Retain even on an unexpected queue failure
        // so neither a rejected edit nor a successful one is freed/repeated here.
        if let Err(PushError::Full(reply)) = queues.replies.push(reply) {
            queues.retained = Some(reply);
            return Err(ControlQueueError::Capacity);
        }
        Ok(Some(request))
    }

    /// Called only at a refused behavior-room preflight. There is no periodic
    /// scan and no diagnostic retention outside an in-flight queued operation.
    pub(crate) fn record_control_admission_failure(&mut self, needed: usize) {
        if !self
            .control_queues
            .as_ref()
            .is_some_and(|queues| queues.record_admission && queues.admission_failure.is_none())
        {
            return;
        }
        const SCAN_LIMIT: usize = 4096;
        let mut failure = super::BehaviorAdmissionFailure {
            required_callbacks: needed,
            used: self.behaviors.count(),
            capacity: self.behaviors.slots.len(),
            available: self.behaviors.available(),
            reserved: self.behaviors.reserved,
            scanned: 0,
            scan_truncated: false,
            note_owned: 0,
            plan_owned: 0,
            waiting: 0,
            preempted: 0,
            terminal: 0,
            finished: 0,
            yielded_queue: self.yielded.len(),
            pending_commands: self.commands.len(),
            reserved_commands: self.reserved_commands,
            stalled_effects: self.ops.stalled_effects,
            outbox_used: self.ops.effects.len(),
            outbox_capacity: crate::ops::EFFECT_CAPACITY,
            owners: [None; 8],
            owners_omitted: 0,
        };
        let mut next = self.behaviors.first;
        while let Some(index) = next {
            if failure.scanned == SCAN_LIMIT {
                failure.scan_truncated = true;
                break;
            }
            let slot = &self.behaviors.slots[index];
            next = slot.next;
            let c = slot.value.as_ref().expect("occupied callback list");
            match c.owner {
                crate::BehaviorOwner::Note(_) => failure.note_owned += 1,
                crate::BehaviorOwner::Plan(_) => failure.plan_owned += 1,
            }
            failure.waiting += usize::from(c.waiting);
            failure.preempted += usize::from(c.yielded_at.is_some());
            failure.terminal += usize::from(c.outcome.is_some());
            failure.finished += usize::from(c.outcome == Some(crate::Outcome::Finished));
            if failure.scanned < failure.owners.len() {
                let plan = self.behavior_plan(c.owner).expect("retained callback plan");
                failure.owners[failure.scanned] = Some(super::BehaviorAdmissionOwner {
                    owner: c.owner,
                    plan,
                    program: c.program,
                    source_slot: self.plans.get(plan.0).unwrap().prepared.programs[c.program]
                        .source_slot,
                    stage: self
                        .behavior_stage(crate::BehaviorId(self.behaviors.id(index)))
                        .expect("retained callback stage"),
                    pc: c.pc,
                    waiting: c.waiting,
                    preempted: c.yielded_at.is_some(),
                    outcome: c.outcome,
                });
            }
            failure.scanned += 1;
        }
        failure.owners_omitted = failure.used.saturating_sub(failure.owners.len());
        self.control_queues.as_mut().unwrap().admission_failure = Some(failure);
    }
}
