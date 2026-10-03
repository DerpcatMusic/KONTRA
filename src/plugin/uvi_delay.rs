//! Loader-prepared source latency for the shared rack mixer. Native buffering
//! and MIDI ownership stay with their existing endpoint/core owners.
use crate::engine::{HostNote, SourceDelay};
use std::mem::size_of;

const MEMORY_LIMIT: usize = 256 << 20;
const END_CAPACITY: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Error {
    InvalidLayout,
    MemoryBudget,
    Allocation,
}

/// Exactly one bank per rack slot; prepare and destroy the container off audio.
/// A disabled/native-free configuration may use Default's empty slice.
pub(super) struct Prepared {
    delays: Vec<SourceDelay>,
    capacity: usize,
    latency: u32,
    bytes: usize,
}
impl Default for Prepared {
    fn default() -> Self {
        Self {
            delays: Vec::new(),
            capacity: 0,
            latency: 0,
            bytes: size_of::<Self>(),
        }
    }
}
impl Prepared {
    /// Allocate all main/direct-output history on the loader. The budget counts
    /// the container, SourceDelay scratch, vector capacity and actual histories.
    pub fn new(slots: usize, capacity_frames: usize) -> Result<Self, Error> {
        if slots == 0 && capacity_frames != 0 {
            return Err(Error::InvalidLayout);
        }
        let frame_bytes = (crate::fx::OUTS + 1) * 2 * size_of::<f32>();
        let per_slot = capacity_frames
            .checked_mul(frame_bytes)
            .and_then(|bytes| bytes.checked_add(size_of::<SourceDelay>()))
            .ok_or(Error::MemoryBudget)?;
        let estimate = slots
            .checked_mul(per_slot)
            .and_then(|bytes| bytes.checked_add(size_of::<Self>()))
            .ok_or(Error::MemoryBudget)?;
        if estimate > MEMORY_LIMIT {
            return Err(Error::MemoryBudget);
        }
        let mut delays = Vec::new();
        delays
            .try_reserve_exact(slots)
            .map_err(|_| Error::Allocation)?;
        for _ in 0..slots {
            delays.push(SourceDelay::new(capacity_frames).map_err(|_| Error::Allocation)?);
        }
        let mut result = Self {
            delays,
            capacity: capacity_frames,
            latency: 0,
            bytes: 0,
        };
        result.refresh_bytes();
        if result.bytes > MEMORY_LIMIT {
            return Err(Error::MemoryBudget);
        }
        Ok(result)
    }
    fn refresh_bytes(&mut self) {
        self.bytes = size_of::<Self>()
            + self.delays.capacity() * size_of::<SourceDelay>()
            + self
                .delays
                .iter()
                .map(|delay| delay.memory_bytes() - size_of::<SourceDelay>())
                .sum::<usize>();
    }
    pub fn slots(&self) -> usize {
        self.delays.len()
    }
    pub fn capacity_frames(&self) -> usize {
        self.capacity
    }
    pub fn latency_frames(&self) -> u32 {
        self.latency
    }
    pub fn memory_bytes(&self) -> usize {
        self.bytes
    }
    pub fn as_mut_slice(&mut self) -> &mut [SourceDelay] {
        &mut self.delays
    }

    /// Validate the complete set before changing any bank. A repeated setting
    /// preserves queued tails; a changed delay clears old PCM as SourceDelay
    /// specifies. Prepare initial settings off audio. Callback clears/changes
    /// are bounded by the prepared memory budget, not a proven CPU deadline.
    pub fn set_all(&mut self, frames: u32) -> bool {
        let frames_usize = frames as usize;
        if frames_usize > self.capacity
            || (self.delays.is_empty() && frames != 0)
            || self
                .delays
                .iter()
                .any(|delay| frames_usize > delay.max_delay())
        {
            return false;
        }
        for delay in &mut self.delays {
            let valid = delay.set_delay(frames_usize);
            debug_assert!(valid);
        }
        self.latency = frames;
        true
    }
    pub fn clear(&mut self) {
        for delay in &mut self.delays {
            delay.clear();
        }
    }
    pub fn clear_slot(&mut self, slot: usize) -> bool {
        let Some(delay) = self.delays.get_mut(slot) else {
            return false;
        };
        delay.clear();
        true
    }

    /// Preserve existing history during rack growth only when its capacity and
    /// current latency agree. Return emptied/replaced storage in `prepared` for
    /// loader disposal, matching Rack's existing growth ownership pattern.
    pub fn adopt_growth(&mut self, prepared: &mut Self) -> bool {
        if prepared.slots() < self.slots()
            || prepared.capacity != self.capacity
            || prepared.latency != self.latency
        {
            return false;
        }
        for (old, new) in self.delays.iter_mut().zip(&mut prepared.delays) {
            std::mem::swap(old, new);
        }
        std::mem::swap(&mut self.delays, &mut prepared.delays);
        self.refresh_bytes();
        prepared.refresh_bytes();
        true
    }
}

/// Conservative integer PCM-end fence for already terminal Kontakt owners.
/// Call only after every core/script/alignment gate is closed. Native endpoints
/// already gate their completions and do not acquire a second fence here.
/// Retain a row through host End-output backpressure, then retire it only after
/// the core accepts that End. This is not another host identity registry.
pub(super) struct Ends {
    ready: Box<[Option<(HostNote, u64)>]>,
}
impl Default for Ends {
    fn default() -> Self {
        Self {
            ready: vec![None; END_CAPACITY].into_boxed_slice(),
        }
    }
}
impl Ends {
    pub fn permits(&mut self, note: HostNote, now: u64, delay: u32) -> bool {
        if delay == 0 {
            return true;
        }
        let ready = if let Some((_, first)) = self
            .ready
            .iter()
            .flatten()
            .find(|(owner, _)| *owner == note)
        {
            *first
        } else {
            let Some(row) = self.ready.iter_mut().find(|row| row.is_none()) else {
                return false;
            };
            *row = Some((note, now));
            now
        };
        ready
            .checked_add(u64::from(delay))
            .is_some_and(|end| now >= end)
    }
    pub fn retire(&mut self, note: HostNote) {
        for row in &mut self.ready {
            if row.is_some_and(|(owner, _)| owner == note) {
                *row = None;
            }
        }
    }
    /// A renewed descendant/anonymous retrigger invalidates prior readiness;
    /// this clears only the PCM fence, never the authoritative core owner.
    pub fn mark_pending(&mut self, note: HostNote) {
        self.retire(note);
    }
    pub fn clear(&mut self) {
        self.ready.fill(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn note(id: i32) -> HostNote {
        HostNote {
            port: 0,
            channel: 0,
            key: 60,
            id,
            clap: true,
        }
    }
    #[test]
    fn source_sets_validate_atomically_and_adopt_only_compatible_growth() {
        let mut current = Prepared::new(2, 513).unwrap();
        assert!(current.set_all(257));
        let bytes = current.memory_bytes();
        assert!(!current.set_all(514));
        assert_eq!(current.latency_frames(), 257);
        assert!(current
            .as_mut_slice()
            .iter()
            .all(|delay| delay.delay() == 257));
        assert!(!current.clear_slot(2));
        let mut wrong = Prepared::new(3, 514).unwrap();
        wrong.set_all(257);
        assert!(!current.adopt_growth(&mut wrong));
        assert_eq!(current.slots(), 2);
        assert_eq!(current.memory_bytes(), bytes);
        let mut larger = Prepared::new(3, 513).unwrap();
        larger.set_all(257);
        assert!(current.adopt_growth(&mut larger));
        assert_eq!(current.slots(), 3);
        assert_eq!(larger.slots(), 2);
        assert!(current
            .as_mut_slice()
            .iter()
            .all(|delay| delay.max_delay() == 513 && delay.delay() == 257));
        assert!(Prepared::default().set_all(0));
        assert!(!Prepared::default().set_all(1));
        assert!(matches!(
            Prepared::new(usize::MAX, 1),
            Err(Error::MemoryBudget)
        ));
        assert!(matches!(
            Prepared::new(1, usize::MAX),
            Err(Error::MemoryBudget)
        ));
        assert!(matches!(Prepared::new(0, 1), Err(Error::InvalidLayout)));
    }
    #[test]
    fn terminal_end_fence_retains_first_boundary_until_ack_and_handles_retrigger_capacity() {
        let mut ends = Ends::default();
        assert!(!ends.permits(note(1), 100, 17));
        assert!(!ends.permits(note(1), 116, 17));
        assert!(ends.permits(note(1), 117, 17));
        assert!(ends.permits(note(1), 118, 17)); // output rejection retains first boundary
        ends.retire(note(1));
        assert!(!ends.permits(note(1), 200, 17));
        ends.mark_pending(note(1));
        assert!(!ends.permits(note(1), 300, 17));
        assert!(!ends.permits(note(1), 316, 17));
        assert!(ends.permits(note(1), 317, 17));
        ends.clear();
        for id in 0..END_CAPACITY {
            assert!(!ends.permits(note(id as i32), 100, 1));
        }
        assert!(!ends.permits(note(END_CAPACITY as i32), 500, 1));
        ends.retire(note(0));
        assert!(!ends.permits(note(END_CAPACITY as i32), 501, 1));
        assert!(ends.permits(note(END_CAPACITY as i32), 502, 1));
        ends.clear();
        assert!(!ends.permits(note(1), u64::MAX - 1, 2));
        assert!(!ends.permits(note(1), u64::MAX, 2));
        assert!(ends.permits(note(2), 0, 0));
    }
    #[test]
    fn prepared_source_settings_and_pcm_end_fences_do_not_allocate_or_free() {
        let mut prepared = Prepared::new(2, 513).unwrap();
        let mut growth = Prepared::new(3, 513).unwrap();
        let mut ends = Ends::default();
        let calls = crate::plugin::tests::allocations(|| {
            assert!(prepared.set_all(513));
            assert!(prepared.set_all(513));
            assert!(growth.set_all(513));
            assert!(prepared.adopt_growth(&mut growth));
            assert!(prepared.clear_slot(0));
            prepared.clear();
            assert!(!ends.permits(note(1), 1, 513));
            assert!(ends.permits(note(1), 514, 513));
            ends.retire(note(1));
            ends.mark_pending(note(2));
            ends.clear();
        });
        assert_eq!(calls, 0);
    }
}
