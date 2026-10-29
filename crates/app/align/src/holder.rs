//! Holding a patch's events back on the audio thread.

/// A patch's held events, each due at a sample time. Fixed capacity, set at
/// creation: nothing allocates while it runs. Past it an event plays at
/// once, late rather than lost.
#[derive(Debug)]
pub struct Holder<T> {
    /// Sorted by due time, then by arrival.
    queue: Vec<(u64, T)>,
    /// What each held key (channel, key) waits, and whether a legato script
    /// plays it: its release and expression wait as much.
    keys: Box<[[(u32, bool); 128]; 16]>,
    /// The latest due time of an event a legato script plays.
    last: u64,
    pub overflows: u64,
}

impl<T> Holder<T> {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            queue: Vec::with_capacity(capacity),
            keys: Box::new([[(0, false); 128]; 16]),
            last: 0,
            overflows: 0,
        }
    }

    /// A note-on arriving at `now` that waits `frames`; `mono` when a legato
    /// script plays it. `Err` gives it back to play now: nothing to wait for,
    /// or the queue is full.
    ///
    /// # Errors
    /// See above.
    pub fn on(
        &mut self,
        now: u64,
        channel: u8,
        key: u8,
        frames: u32,
        mono: bool,
        event: T,
    ) -> Result<(), T> {
        if let Some(k) = self
            .keys
            .get_mut(usize::from(channel))
            .and_then(|c| c.get_mut(usize::from(key)))
        {
            *k = (frames, mono);
        }
        self.hold(now, frames, mono, event)
    }

    /// A release or expression of the note on (`channel`, `key`): it waits
    /// as long as its note-on did.
    ///
    /// # Errors
    /// As [`Holder::on`].
    pub fn follow(&mut self, now: u64, channel: u8, key: u8, event: T) -> Result<(), T> {
        let (frames, mono) = (self.keys.get(usize::from(channel)))
            .and_then(|c| c.get(usize::from(key)).copied())
            .unwrap_or_default();
        self.hold(now, frames, mono, event)
    }

    /// Any other event: due at `now + frames`, never before an earlier
    /// legato-script event when `mono`.
    ///
    /// # Errors
    /// As [`Holder::on`].
    pub fn hold(&mut self, now: u64, frames: u32, mono: bool, event: T) -> Result<(), T> {
        let mut due = now + u64::from(frames);
        if mono {
            due = due.max(self.last);
            self.last = due;
        }
        if due <= now && self.queue.is_empty() {
            return Err(event);
        }
        if self.queue.len() == self.queue.capacity() {
            self.overflows += 1;
            return Err(event);
        }
        let at = self.queue.partition_point(|(d, _)| *d <= due);
        self.queue.insert(at, (due, event));
        Ok(())
    }

    /// When the next held event is due.
    #[must_use]
    pub fn next_due(&self) -> Option<u64> {
        self.queue.first().map(|(due, _)| *due)
    }

    /// The next event due at or before `now`, earliest first.
    pub fn pop_due(&mut self, now: u64) -> Option<(u64, T)> {
        (self.next_due()? <= now).then(|| self.queue.remove(0))
    }

    /// Lets everything go (a panic or a reset); nothing held plays.
    pub fn clear(&mut self) {
        self.queue.clear();
        self.last = 0;
    }
}
