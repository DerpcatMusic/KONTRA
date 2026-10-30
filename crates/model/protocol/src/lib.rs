//! The three channels between the audio thread and the rest of the process
//! (07-APP.md §2): commands in, publications out, retirements out. Each is
//! lock-free and never blocks the audio side.
//!
//! The payloads are the host's: a command carries a built engine, which is
//! an engine type this crate must not see (01-LAYOUT.md §2). What the UI
//! reads is defined here: [`Meters`] and [`Stats`], published through
//! [`MeterCells`] and [`StatCells`].

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use crossbeam_queue::ArrayQueue;

/// A fixed-capacity queue from one thread to another: commands into the
/// audio thread, retirements out of it. Nothing is dropped silently: a full
/// ring hands the value back.
pub struct Ring<T>(ArrayQueue<T>);

impl<T> Ring<T> {
    /// A ring holding up to `capacity` values (at least one).
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self(ArrayQueue::new(capacity.max(1)))
    }

    /// Queues `value`, or gives it back when the ring is full so the sender
    /// can retry later.
    ///
    /// # Errors
    /// The value itself, when the ring is full.
    pub fn send(&self, value: T) -> Result<(), T> {
        self.0.push(value)
    }

    /// The oldest value queued.
    pub fn recv(&self) -> Option<T> {
        self.0.pop()
    }

    /// Whether a send would fail now; the audio side checks this before it
    /// takes on something it must retire.
    #[must_use]
    pub fn is_full(&self) -> bool {
        self.0.is_full()
    }
}

/// Parts a rack holds at most; publications carry one entry each.
pub const MAX_PARTS: usize = 16;

/// Instrument buses a part's meters can read.
pub const BUSES: usize = 16;

/// Peak levels since the last publication, 0..1 (above 1 clips): the
/// master's, each part's output's and each part's instrument buses'.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Meters {
    pub master: [f32; 2],
    pub parts: [[f32; 2]; MAX_PARTS],
    pub buses: [[[f32; 2]; BUSES]; MAX_PARTS],
}

/// What the engine is doing, for the header and the report.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    /// Time spent rendering, and the real time of what it rendered, in
    /// nanoseconds since the plugin started: their growth between two
    /// looks is the audio thread's load over that span.
    pub busy_ns: u64,
    pub span_ns: u64,
    pub voices: u32,
    /// Since the plugin started, across parts.
    pub voices_stolen: u64,
    pub notes_refused: u64,
    pub missing_frames: u64,
    /// Commands the audio thread had to refuse or could not retire.
    pub dropped: u64,
}

/// The audio thread's [`Meters`] for the UI: each level is the highest
/// published since the UI last took it. Plain atomics, so publishing never
/// waits on a reader, however it is scheduled.
#[derive(Default)]
pub struct MeterCells {
    master: [AtomicU32; 2],
    parts: [[AtomicU32; 2]; MAX_PARTS],
    buses: [[[AtomicU32; 2]; BUSES]; MAX_PARTS],
}

impl MeterCells {
    /// Raises each level to at least `m`'s. Levels are never negative, and
    /// a non-negative `f32` orders as its bits do.
    pub fn publish(&self, m: &Meters) {
        let pairs = std::iter::once((&self.master, &m.master))
            .chain(self.parts.iter().zip(&m.parts))
            .chain(self.buses.iter().flatten().zip(m.buses.iter().flatten()));
        for (cells, levels) in pairs {
            for (cell, level) in cells.iter().zip(levels) {
                cell.fetch_max(level.max(0.0).to_bits(), Ordering::Relaxed);
            }
        }
    }

    /// The levels since the last take, and starts again from silence.
    pub fn take(&self) -> Meters {
        let pair = |c: &[AtomicU32; 2]| {
            c.each_ref()
                .map(|c| f32::from_bits(c.swap(0, Ordering::Relaxed)))
        };
        Meters {
            master: pair(&self.master),
            parts: self.parts.each_ref().map(pair),
            buses: self.buses.each_ref().map(|b| b.each_ref().map(pair)),
        }
    }
}

/// The audio thread's latest [`Stats`] for the UI, as plain atomics. A
/// reader may see one publication's fields mixed with the next's; each
/// field is whole.
#[derive(Default)]
pub struct StatCells {
    busy_ns: AtomicU64,
    span_ns: AtomicU64,
    voices: AtomicU32,
    voices_stolen: AtomicU64,
    notes_refused: AtomicU64,
    missing_frames: AtomicU64,
    dropped: AtomicU64,
    fresh: AtomicBool,
}

impl StatCells {
    pub fn publish(&self, s: &Stats) {
        self.busy_ns.store(s.busy_ns, Ordering::Relaxed);
        self.span_ns.store(s.span_ns, Ordering::Relaxed);
        self.voices.store(s.voices, Ordering::Relaxed);
        self.voices_stolen.store(s.voices_stolen, Ordering::Relaxed);
        self.notes_refused.store(s.notes_refused, Ordering::Relaxed);
        self.missing_frames
            .store(s.missing_frames, Ordering::Relaxed);
        self.dropped.store(s.dropped, Ordering::Relaxed);
        self.fresh.store(true, Ordering::Release);
    }

    /// The stats, if published since the last take.
    pub fn take(&self) -> Option<Stats> {
        self.fresh.swap(false, Ordering::Acquire).then(|| Stats {
            busy_ns: self.busy_ns.load(Ordering::Relaxed),
            span_ns: self.span_ns.load(Ordering::Relaxed),
            voices: self.voices.load(Ordering::Relaxed),
            voices_stolen: self.voices_stolen.load(Ordering::Relaxed),
            notes_refused: self.notes_refused.load(Ordering::Relaxed),
            missing_frames: self.missing_frames.load(Ordering::Relaxed),
            dropped: self.dropped.load(Ordering::Relaxed),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_ring_hands_the_value_back_without_blocking() {
        let ring = Ring::new(2);
        assert_eq!(ring.send(1), Ok(()));
        assert_eq!(ring.send(2), Ok(()));
        assert!(ring.is_full());
        assert_eq!(ring.send(3), Err(3));
        assert_eq!(ring.recv(), Some(1));
        assert_eq!(ring.send(3), Ok(()));
        assert_eq!(
            (ring.recv(), ring.recv(), ring.recv()),
            (Some(2), Some(3), None)
        );
    }

    #[test]
    fn meters_hold_the_peak_until_taken_and_stats_the_latest() {
        let cells = MeterCells::default();
        let mut m = Meters {
            master: [0.5, 0.25],
            ..Meters::default()
        };
        m.parts[3] = [0.1, 0.9];
        cells.publish(&m);
        m.master = [0.2, 0.75];
        cells.publish(&m);
        let got = cells.take();
        assert_eq!((got.master, got.parts[3]), ([0.5, 0.75], [0.1, 0.9]));
        assert_eq!(cells.take(), Meters::default());
        let stats = StatCells::default();
        assert_eq!(stats.take(), None);
        for voices in 1..4 {
            stats.publish(&Stats {
                voices,
                ..Stats::default()
            });
        }
        assert_eq!(stats.take().map(|s| s.voices), Some(3));
        assert_eq!(stats.take(), None);
    }

    #[test]
    fn values_cross_threads() {
        let ring = std::sync::Arc::new(Ring::new(64));
        let tx = std::sync::Arc::clone(&ring);
        let sender = std::thread::spawn(move || {
            for n in 0..1000u32 {
                let mut v = n;
                while let Err(back) = tx.send(v) {
                    v = back;
                    std::thread::yield_now();
                }
            }
        });
        let mut next = 0;
        while next < 1000 {
            if let Some(v) = ring.recv() {
                assert_eq!(v, next);
                next += 1;
            }
        }
        sender.join().unwrap();
    }
}
