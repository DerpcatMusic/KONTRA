//! Bounded state ownership handoff shared by the format wrappers.
use crossbeam_queue::ArrayQueue;
use moose_core::state::DeserializedState;

pub(crate) struct StateLoadQueue {
    pending: ArrayQueue<DeserializedState>,
    retired: ArrayQueue<DeserializedState>,
}
impl StateLoadQueue {
    pub(crate) fn new(capacity: usize) -> Self {
        assert_eq!(capacity, 1);
        Self {
            pending: ArrayQueue::new(1),
            retired: ArrayQueue::new(2),
        }
    }
    /// Host/editor only; displaced and consumed blobs are destroyed here.
    pub(crate) fn force_push(&self, state: DeserializedState) -> Option<DeserializedState> {
        self.collect_retired();
        self.pending.force_push(state)
    }
    /// Inactive host only; preserve the existing deactivate/save drain.
    pub(crate) fn pop(&self) -> Option<DeserializedState> {
        self.collect_retired();
        self.pending.pop()
    }
    pub(crate) fn collect_retired(&self) -> usize {
        let mut count = 0;
        while let Some(state) = self.retired.pop() {
            drop(state);
            count += 1;
        }
        count
    }
    /// The wrapper's serialized audio consumer must never destroy the blob.
    pub(crate) fn apply_audio(&self, apply: impl FnOnce(&DeserializedState)) -> bool {
        // ponytail: two blobs can remain until the next main callback, recall or deactivate.
        if self.retired.is_full() {
            return false;
        }
        let Some(state) = self.pending.pop() else {
            return false;
        };
        struct Retire<'a> {
            queue: &'a StateLoadQueue,
            state: Option<DeserializedState>,
        }
        impl Drop for Retire<'_> {
            fn drop(&mut self) {
                // Only this serialized consumer pushes; control can only make room.
                assert!(self.queue.retired.push(self.state.take().unwrap()).is_ok());
            }
        }
        let state = Retire {
            queue: self,
            state: Some(state),
        };
        apply(state.state.as_ref().unwrap());
        true
    }
}
