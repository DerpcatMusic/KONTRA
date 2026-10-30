//! Every future thing, ordered by (time, arrival). Fixed capacity.

use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;

use kontra_events::Event;
use kontra_time::SampleTime;
use kontra_vm::CoId;

use crate::NoteRef;

/// Something that happens at a sample time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Timed<T> {
    pub at: SampleTime,
    pub body: T,
}

/// How an asynchronous command ended: `$NI_ASYNC_EXIT_STATUS` 1 or 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Exit {
    Done,
    Failed,
}

/// What the queue holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Work {
    Input(Event),
    /// A waiting callback of a script slot continues, if still that wait.
    Resume(u8, CoId, u32),
    /// A script note's duration is up.
    Release(NoteRef),
    /// Script slots run `on pgs_changed`.
    PgsChanged,
    /// A script slot's listener timer fires, if it is still that generation.
    Listen {
        slot: u8,
        signal: u8,
        generation: u16,
    },
    /// A script slot's asynchronous command `id` has finished.
    Async {
        slot: u8,
        id: i32,
        exit: Exit,
    },
}

struct Entry {
    at: SampleTime,
    seq: u64,
    work: Work,
}

impl PartialEq for Entry {
    fn eq(&self, other: &Self) -> bool {
        (self.at, self.seq) == (other.at, other.seq)
    }
}
impl Eq for Entry {}
impl PartialOrd for Entry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Entry {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.at, self.seq).cmp(&(other.at, other.seq))
    }
}

pub(crate) struct EventQueue {
    heap: BinaryHeap<Reverse<Entry>>,
    capacity: usize,
    seq: u64,
}

impl EventQueue {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            heap: BinaryHeap::with_capacity(capacity),
            capacity,
            seq: 0,
        }
    }

    /// Admits `work` at `at`; false (refused, never reordered) when full.
    pub(crate) fn push(&mut self, at: SampleTime, work: Work) -> bool {
        if self.heap.len() >= self.capacity {
            return false;
        }
        self.seq += 1;
        self.heap.push(Reverse(Entry {
            at,
            seq: self.seq,
            work,
        }));
        true
    }

    /// The earliest work due at or before `now`.
    pub(crate) fn pop_due(&mut self, now: SampleTime) -> Option<Work> {
        if self.heap.peek()?.0.at > now {
            return None;
        }
        self.heap.pop().map(|Reverse(e)| e.work)
    }

    pub(crate) fn next_at(&self) -> Option<SampleTime> {
        self.heap.peek().map(|Reverse(e)| e.at)
    }
}
