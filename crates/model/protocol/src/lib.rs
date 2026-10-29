//! The three channels between the audio thread and the rest of the process
//! (07-APP.md §2): commands in, publications out, retirements out. Each is
//! lock-free and never blocks the audio side.
//!
//! The payloads are the host's: a command carries a built engine, which is
//! an engine type this crate must not see (01-LAYOUT.md §2). What the UI
//! reads is defined here: [`Meters`] and [`Stats`].

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

/// The latest value one thread publishes for another to read. A slow reader
/// skips values and never builds a backlog; the writer never waits. For
/// plain data: the value a publish replaces is dropped by the writer.
pub struct Latest<T>(ArrayQueue<T>);

impl<T> Latest<T> {
    #[must_use]
    pub fn new() -> Self {
        Self(ArrayQueue::new(1))
    }

    /// Replaces whatever is unread.
    pub fn publish(&self, value: T) {
        drop(self.0.force_push(value));
    }

    /// The newest value since the last take, if one was published.
    pub fn take(&self) -> Option<T> {
        self.0.pop()
    }
}

impl<T> Default for Latest<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Parts a rack holds at most; publications carry one entry each.
pub const MAX_PARTS: usize = 16;

/// Peak levels since the last publication, 0..1 (above 1 clips).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Meters {
    pub master: [f32; 2],
    pub parts: [[f32; 2]; MAX_PARTS],
}

/// What the engine is doing, for the header and the report.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    /// Time spent rendering over the block's real time, 0..1 and above.
    pub load: f32,
    pub voices: u32,
    /// Since the plugin started, across parts.
    pub voices_stolen: u64,
    pub notes_refused: u64,
    pub missing_frames: u64,
    /// Commands the audio thread had to refuse or could not retire.
    pub dropped: u64,
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
    fn a_slow_reader_sees_only_the_latest_value() {
        let cell = Latest::new();
        assert_eq!(cell.take(), None::<u32>);
        for n in 0..10 {
            cell.publish(n);
        }
        assert_eq!(cell.take(), Some(9));
        assert_eq!(cell.take(), None);
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
