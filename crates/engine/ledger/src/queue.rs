//! Every future thing, ordered by (due time, arrival). Fixed capacity.

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

/// When queued work is due: a frame and how far into it, in millionths of
/// a frame. A script wait shorter than a frame lands inside one, and work
/// due in the same frame runs in this order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Due {
    pub frame: SampleTime,
    pub part: u32,
}

impl Due {
    /// Millionths of a frame in one.
    pub const FRAME: u32 = 1_000_000;

    #[must_use]
    pub fn at(frame: SampleTime) -> Self {
        Self { frame, part: 0 }
    }
}

/// How an asynchronous command ended: `$NI_ASYNC_EXIT_STATUS` 1 or 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    Done,
    Failed,
}

impl From<bool> for Exit {
    fn from(done: bool) -> Self {
        if done { Self::Done } else { Self::Failed }
    }
}

/// What the queue holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Work {
    Input(Event),
    /// A waiting callback of a script slot continues, if still that wait.
    Resume(u8, CoId, u32),
    /// A script note's duration is up.
    Release(NoteRef),
    /// The note a `play_note(.., -1)` child was played for is released: its
    /// release callbacks may hold it back.
    ParentUp(NoteRef),
    /// Script slots run `on pgs_changed`.
    PgsChanged,
    /// A script's controller (128 the bend, 129 the pressure), sent later.
    Controller {
        controller: u8,
        value: i16,
    },
    /// A script's polyphonic aftertouch on a key, sent later.
    PolyAt {
        channel: u8,
        key: u8,
        value: u8,
    },
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
    at: Due,
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

pub struct EventQueue {
    heap: BinaryHeap<Reverse<Entry>>,
    capacity: usize,
    seq: u64,
}

impl EventQueue {
    pub fn new(capacity: usize) -> Self {
        Self {
            heap: BinaryHeap::with_capacity(capacity),
            capacity,
            seq: 0,
        }
    }

    /// Admits `work` at the start of frame `at`; false (refused, never
    /// reordered) when full.
    pub fn push(&mut self, at: SampleTime, work: Work) -> bool {
        self.push_due(Due::at(at), work)
    }

    /// Admits `work` due at `at`; false (refused, never reordered) when full.
    pub fn push_due(&mut self, at: Due, work: Work) -> bool {
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

    /// Keeps only the work `keep` admits, in its order.
    pub fn retain(&mut self, mut keep: impl FnMut(&Work) -> bool) {
        self.heap.retain(|Reverse(e)| keep(&e.work));
    }

    /// The earliest work due in or before frame `now`, and when it was due.
    pub fn pop_due(&mut self, now: SampleTime) -> Option<(Due, Work)> {
        if self.heap.peek()?.0.at.frame > now {
            return None;
        }
        self.heap.pop().map(|Reverse(e)| (e.at, e.work))
    }

    pub fn next_at(&self) -> Option<SampleTime> {
        self.heap.peek().map(|Reverse(e)| e.at.frame)
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }
}
