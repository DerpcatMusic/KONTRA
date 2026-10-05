//! Bounded control/UI handoff. Owned payloads return to their producer for reuse
//! or destruction; the audio side never releases their backing storage.
use super::{ControlWrite, Error, PlanId, Runtime};
use rtrb::{Consumer, Producer, PushError, RingBuffer};

#[derive(Debug)]
pub enum ControlOperation {
    Invoke(ControlWrite),
    Edit(Box<[ControlWrite]>),
    Recall(Box<[ControlWrite]>),
    Capture(Box<[ControlWrite]>),
}
impl ControlOperation {
    fn len(&self) -> usize {
        match self {
            Self::Invoke(_) => 1,
            Self::Edit(v) | Self::Recall(v) | Self::Capture(v) => v.len(),
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
}
impl ControlClient {
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
        self.apply_due();
        reply.result = match &mut command.operation {
            ControlOperation::Invoke(write) => self
                .invoke_control(command.plan, command.expected_revision, *write)
                .map(|(revision, behavior)| {
                    reply.behavior = behavior;
                    (1, revision)
                }),
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
        // A consumer only frees slots. Retain even on an unexpected queue failure
        // so neither a rejected edit nor a successful one is freed/repeated here.
        if let Err(PushError::Full(reply)) = queues.replies.push(reply) {
            queues.retained = Some(reply);
            return Err(ControlQueueError::Capacity);
        }
        Ok(Some(request))
    }
}
