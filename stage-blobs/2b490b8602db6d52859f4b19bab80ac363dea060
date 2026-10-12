//! One dry, host-rate Mapping cursor. Source admission and decoding stay off audio.
use crate::audio::Frame;
use anyhow::{Result, ensure};
use crossbeam_queue::ArrayQueue;
use std::sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU64, Ordering}};

pub(crate) const MAX_BYTES: usize = 32 << 20;
pub(crate) const MAX_FRAMES: usize = MAX_BYTES / size_of::<Frame>();
// The final ID is a permanent cancellation tombstone, never a Start ticket.
const LAST_ID: u64 = u64::MAX >> 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Status { Preparing = 1, Ready, Playing, Stopped, Failed, Unsupported }

/// Owned by the persisted Selection boundary. Audio never takes `owner`.
#[derive(Default)]
pub(crate) struct Control {
    wanted: AtomicU64,
    status: AtomicU64,
    owner: Mutex<Option<(u64, usize)>>,
}
impl Control {
    /// Call under the validated Selection read guard, before releasing admission.
    pub(crate) fn begin(&self, slot: usize) -> Option<u64> {
        let mut owner = self.owner.lock().unwrap();
        let id = self.advance()?;
        if id == LAST_ID { self.publish_status(id, Status::Stopped); return None; }
        *owner = Some((id, slot));
        self.publish_status(id, Status::Preparing);
        Some(id)
    }
    fn advance(&self) -> Option<u64> {
        self.wanted.fetch_update(Ordering::AcqRel, Ordering::Acquire,
            |id| (id < LAST_ID).then(|| id + 1)).ok().map(|id| id + 1)
    }
    /// Global host reset/panic/restore cancellation: bounded POD only.
    pub(crate) fn stop(&self) {
        let id = self.wanted.load(Ordering::Acquire);
        if id < LAST_ID && self.wanted.compare_exchange(id, id + 1, Ordering::AcqRel, Ordering::Acquire).is_ok() {
            self.publish_status(id + 1, Status::Stopped);
        }
        // On CAS failure a concurrent issuance/cancellation already invalidated
        // this observed ID. A genuinely newer Start follows this Stop boundary.
    }
    /// An old editor completion cannot stop a newer request.
    pub(crate) fn stop_id(&self, id: u64) -> bool {
        if id == 0 || id >= LAST_ID { return false; }
        if self.wanted.compare_exchange(id, id + 1, Ordering::AcqRel, Ordering::Acquire).is_err() {
            return false;
        }
        self.publish_status(id + 1, Status::Stopped);
        true
    }
    /// Off-audio owner query. Source commit keeps Selection write held through
    /// this query and stop_slot, excluding any newly admitted Start.
    pub(crate) fn owner_slot(&self) -> Option<usize> {
        self.owner.lock().unwrap().as_ref()
            .and_then(|&(id, slot)| self.current(id).then_some(slot))
    }
    /// Call at the owner commit under its Selection write guard.
    pub(crate) fn stop_slot(&self, slot: usize) -> bool {
        let owner = self.owner.lock().unwrap();
        owner.is_some_and(|(id, owner)| owner == slot && self.stop_id(id))
    }
    pub(crate) fn current(&self, id: u64) -> bool {
        id > 0 && id < LAST_ID && self.wanted.load(Ordering::Acquire) == id
    }
    pub(crate) fn status_key(&self) -> u64 { self.status.load(Ordering::Acquire) }
    pub(crate) fn status(&self, id: u64) -> Option<Status> {
        if !self.current(id) { return None; }
        let word = self.status.load(Ordering::Acquire);
        if word >> 3 != id || !self.current(id) { return None; }
        match word & 7 {
            1 => Some(Status::Preparing), 2 => Some(Status::Ready), 3 => Some(Status::Playing),
            4 => Some(Status::Stopped), 5 => Some(Status::Failed), 6 => Some(Status::Unsupported),
            _ => None,
        }
    }
    pub(crate) fn set_status(&self, id: u64, status: Status) {
        if self.current(id) { self.publish_status(id, status); }
    }
    fn publish_status(&self, id: u64, status: Status) {
        let next = (id << 3) | status as u64;
        // One tagged atomic: late Ready cannot regress Playing; an older
        // producer cannot overwrite a newer request, including concurrent Stop.
        let _ = self.status.fetch_update(Ordering::AcqRel, Ordering::Acquire,
            |word| (word < next).then_some(next));
    }
    fn audible(&self, id: u64) -> bool {
        matches!(self.status(id), Some(Status::Preparing | Status::Ready | Status::Playing))
    }
}

pub(crate) struct Prepared {
    pub(crate) id: u64,
    pub(crate) rate: u32,
    frames: Option<Box<[Frame]>>,
    credit: Option<Arc<AtomicBool>>,
    reserved: bool,
}
impl Prepared {
    /// All validation and payload allocation occur on the preparation thread.
    pub(crate) fn new(id: u64, rate: u32, frames: Box<[Frame]>) -> Result<Box<Self>> {
        ensure!(id > 0 && id < LAST_ID, "Invalid sample preview request");
        ensure!((8000..=192000).contains(&rate), "Unsupported sample preview host rate");
        ensure!(!frames.is_empty() && frames.len() <= MAX_FRAMES, "Sample preview exceeds PCM budget");
        ensure!(frames.iter().flatten().all(|x| x.is_finite()), "Nonfinite sample preview PCM");
        Ok(Box::new(Self { id, rate, frames: Some(frames), credit: None, reserved: false }))
    }
}

impl Drop for Prepared {
    fn drop(&mut self) {
        // Return credit only after the PCM allocation is actually freed.
        // This destructor is reached only by the off-audio service/teardown.
        drop(self.frames.take());
        // Only an accepted retirement owns this credit. Ready/pending/current
        // teardown is off audio too, but must not release another owner's slot.
        if self.reserved {
            self.credit.as_ref().unwrap().store(false, Ordering::Release);
            self.reserved = false;
        }
    }
}

pub(crate) struct Mailbox {
    ready: ArrayQueue<Box<Prepared>>,
    retirement: Arc<AtomicBool>,
}
impl Default for Mailbox {
    fn default() -> Self { Self { ready: ArrayQueue::new(1), retirement: Arc::new(AtomicBool::new(false)) } }
}
impl Mailbox {
    /// Metadata must already be published; callback acceptance may be immediate.
    /// Full/stale returns ownership to the sole serialized off-audio producer.
    pub(crate) fn publish(&self, control: &Control, mut prepared: Box<Prepared>) -> std::result::Result<(), Box<Prepared>> {
        if !control.audible(prepared.id) || prepared.reserved { return Err(prepared); }
        // The producer alone attaches/clones this tiny shared credit. Callback
        // never clones an Arc, and only one preview can wait in discard.
        if prepared.credit.as_ref().is_none_or(|credit| !Arc::ptr_eq(credit, &self.retirement)) {
            prepared.credit = Some(self.retirement.clone());
        }
        let id = prepared.id;
        self.ready.push(prepared)?;
        control.set_status(id, Status::Ready);
        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct Progress { pub(crate) retired: bool, pub(crate) ready_taken: bool }

#[derive(Default)]
pub(crate) struct Cursor {
    current: Option<Box<Prepared>>,
    at: usize,
    playing: bool,
}
impl Cursor {
    fn valid(&self, control: &Control, rate: f64) -> bool {
        self.current.as_ref().is_some_and(|p| control.audible(p.id) && f64::from(p.rate) == rate)
    }
    /// At most one incoming owner per boundary; no pop without a disposal slot.
    /// Stop/EOF/full discard keeps the silent payload here until it can retire.
    pub(super) fn poll(&mut self, control: &Control, mailbox: &Mailbox,
        discard: &ArrayQueue<super::Retired>, rate: f64) -> Progress {
        let mut progress = Progress::default();
        if !self.valid(control, rate) {
            self.playing = false;
            if let Some(p) = &self.current { control.set_status(p.id, Status::Stopped); }
        }
        if !self.playing && self.current.is_some() && !discard.is_full() {
            progress.retired |= self.retire(discard);
        }
        if self.current.is_none() && !discard.is_full() {
            if let Some(next) = mailbox.ready.pop() {
                progress.ready_taken = true;
                self.current = Some(next);
                self.at = 0;
                self.playing = self.valid(control, rate);
                if self.playing { control.set_status(self.current.as_ref().unwrap().id, Status::Playing); }
                else {
                    control.set_status(self.current.as_ref().unwrap().id, Status::Stopped);
                    progress.retired |= self.retire(discard);
                }
            }
        }
        progress
    }
    fn retire(&mut self, discard: &ArrayQueue<super::Retired>) -> bool {
        let p = self.current.as_mut().unwrap();
        // One CAS, no wait: a previous preview may still await loader disposal.
        if p.credit.as_ref().unwrap().compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
            return false;
        }
        p.reserved = true;
        let retired = super::Retired { preview: self.current.take(), ..Default::default() };
        match discard.push(retired) {
            Ok(()) => true,
            // Do not rely on an is_full check to authorize callback destruction.
            Err(mut retired) => {
                let mut p = retired.preview.take().unwrap();
                p.reserved = false;
                p.credit.as_ref().unwrap().store(false, Ordering::Release);
                self.current = Some(p);
                false
            }
        }
    }
    pub(crate) fn fail(&mut self, control: &Control) {
        self.playing = false;
        if let Some(p) = &self.current { control.set_status(p.id, Status::Failed); }
    }
    pub(crate) fn render(&mut self, control: &Control, rate: f64, out: &mut [Frame]) -> bool {
        out.fill([0.; 2]);
        if !self.playing || !self.valid(control, rate) { self.playing = false; return false; }
        let prepared = self.current.as_ref().unwrap();
        let frames = prepared.frames.as_ref().unwrap();
        let len = out.len().min(frames.len() - self.at);
        out[..len].copy_from_slice(&frames[self.at..self.at + len]);
        self.at += len;
        // This boundary can observe a commit while the frames were copied.
        if !control.audible(prepared.id) { out.fill([0.; 2]); self.playing = false; return false; }
        else if self.at == frames.len() {
            self.playing = false;
            control.set_status(prepared.id, Status::Stopped);
        }
        len != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::allocations;

    fn payload(id: u64) -> Box<Prepared> {
        Prepared::new(id, 48000, vec![[0.25, -0.5], [0.5, -0.25]].into_boxed_slice()).unwrap()
    }
    #[test]
    fn delayed_start_status_cannot_revive_after_lockfree_stop() {
        let c = Control::default();
        // Exact begin interleaving: ID issued under owner lock, then reset
        // before that producer can publish Preparing or its slot metadata.
        let mut owner = c.owner.lock().unwrap();
        let issued = c.advance().unwrap();
        assert_eq!(allocations(|| c.stop()), 0); // Owner mutex is still held.
        *owner = Some((issued, 1));
        c.publish_status(issued, Status::Preparing);
        assert!(!c.current(issued));
        assert!(c.status_key() >> 3 > issued);
        drop(owner);
        let fresh = c.begin(2).unwrap();
        assert!(fresh > issued);
        assert_eq!(c.status(fresh), Some(Status::Preparing));
    }

    #[test]
    fn scoped_stop_and_status_do_not_cancel_or_regress_new_owner() {
        let c = Control::default();
        let first = c.begin(2).unwrap();
        c.set_status(first, Status::Playing);
        assert_eq!(c.owner_slot(), Some(2));
        c.set_status(first, Status::Ready);
        assert_eq!(c.status(first), Some(Status::Playing));
        let second = c.begin(3).unwrap();
        assert_eq!(c.owner_slot(), Some(3));
        assert!(!c.stop_slot(2));
        assert!(!c.stop_id(first));
        c.set_status(first, Status::Failed);
        assert_eq!(c.status(second), Some(Status::Preparing));
        assert!(c.stop_slot(3));
        assert!(!c.current(second));
        assert_eq!(c.owner_slot(), None);
        let third = c.begin(3).unwrap();
        c.stop();
        assert!(!c.current(third));
        c.wanted.store(LAST_ID - 1, Ordering::Release);
        assert!(c.begin(3).is_none());
        assert!(c.begin(3).is_none());
        assert!(!c.current(third));
    }
    #[test]
    fn full_retirement_eof_stale_and_rate_mismatch_keep_audio_owners() {
        let c = Control::default();
        let mailbox = Mailbox::default();
        let discard = ArrayQueue::new(1);
        let mut cursor = Cursor::default();
        let id = c.begin(1).unwrap();
        mailbox.publish(&c, payload(id)).ok().unwrap();
        let mut out = [[0.; 2]; 4];
        assert_eq!(allocations(|| {
            cursor.poll(&c, &mailbox, &discard, 48000.);
            cursor.render(&c, 48000., &mut out);
        }), 0);
        assert_eq!(out, [[0.25, -0.5], [0.5, -0.25], [0.; 2], [0.; 2]]);
        assert_eq!(c.status(id), Some(Status::Stopped));
        discard.push(super::super::Retired::default()).ok().unwrap();
        assert_eq!(allocations(|| {
            cursor.poll(&c, &mailbox, &discard, 48000.);
            cursor.render(&c, 48000., &mut out);
        }), 0);
        assert!(cursor.current.is_some());
        assert_eq!(out, [[0.; 2]; 4]);
        drop(discard.pop()); // Service thread.
        assert_eq!(allocations(|| { assert!(cursor.poll(&c, &mailbox, &discard, 48000.).retired); }), 0);
        assert!(cursor.current.is_none());
        assert!(discard.pop().unwrap().preview.is_some()); // Service thread frees payload.
        let stale = c.begin(1).unwrap();
        mailbox.publish(&c, payload(stale)).ok().unwrap();
        c.stop_id(stale);
        discard.push(super::super::Retired::default()).ok().unwrap();
        assert_eq!(allocations(|| { cursor.poll(&c, &mailbox, &discard, 48000.); }), 0);
        assert_eq!(mailbox.ready.len(), 1); // No disposal space: do not take stale ready.
        drop(discard.pop());
        assert_eq!(allocations(|| { cursor.poll(&c, &mailbox, &discard, 48000.); }), 0);
        assert!(discard.pop().unwrap().preview.is_some());
        let next = c.begin(1).unwrap();
        mailbox.publish(&c, payload(next)).ok().unwrap();
        assert_eq!(allocations(|| {
            cursor.poll(&c, &mailbox, &discard, 44100.);
            cursor.render(&c, 44100., &mut out);
        }), 0);
        assert_eq!(out, [[0.; 2]; 4]);
        assert!(discard.pop().unwrap().preview.is_some());
    }
    #[test]
    fn failed_discard_push_restores_same_owner_and_its_own_credit() {
        let c = Control::default();
        let mailbox = Mailbox::default();
        let discard = ArrayQueue::new(1);
        let mut cursor = Cursor::default();
        let id = c.begin(0).unwrap();
        mailbox.publish(&c, payload(id)).ok().unwrap();
        cursor.poll(&c, &mailbox, &discard, 48000.);
        let address = &**cursor.current.as_ref().unwrap() as *const Prepared;
        c.stop_id(id);
        discard.push(super::super::Retired::default()).ok().unwrap();
        // Exercise the exact fallback after the callback's capacity observation
        // loses its slot, without fabricating an alternate disposal routine.
        assert_eq!(allocations(|| { assert!(!cursor.retire(&discard)); }), 0);
        assert_eq!(&**cursor.current.as_ref().unwrap() as *const Prepared, address);
        assert!(!cursor.current.as_ref().unwrap().reserved);
        assert!(!mailbox.retirement.load(Ordering::Acquire));
        drop(discard.pop());
        assert_eq!(allocations(|| { assert!(cursor.poll(&c, &mailbox, &discard, 48000.).retired); }), 0);
        let retired = discard.pop().unwrap();
        assert_eq!(&**retired.preview.as_ref().unwrap() as *const Prepared, address);
        assert!(retired.preview.as_ref().unwrap().reserved);
        drop(retired);
        assert!(!mailbox.retirement.load(Ordering::Acquire));
    }

    #[test]
    fn one_retirement_credit_is_released_only_by_its_reserved_owner() {
        let c = Control::default();
        let mailbox = Mailbox::default();
        let discard = ArrayQueue::new(4);
        let mut cursor = Cursor::default();
        let first = c.begin(0).unwrap();
        mailbox.publish(&c, payload(first)).ok().unwrap();
        let mut out = [[0.; 2]; 2];
        assert_eq!(allocations(|| {
            cursor.poll(&c, &mailbox, &discard, 48000.);
            cursor.render(&c, 48000., &mut out);
            cursor.poll(&c, &mailbox, &discard, 48000.);
        }), 0);
        let retired = discard.pop().unwrap(); // Loader retains, has not dropped it.
        assert!(mailbox.retirement.load(Ordering::Acquire));
        let second = c.begin(0).unwrap();
        mailbox.publish(&c, payload(second)).ok().unwrap();
        drop(mailbox.publish(&c, payload(second)).err().unwrap()); // Unreserved pending.
        assert!(mailbox.retirement.load(Ordering::Acquire));
        assert_eq!(allocations(|| {
            cursor.poll(&c, &mailbox, &discard, 48000.);
            cursor.render(&c, 48000., &mut out);
            cursor.poll(&c, &mailbox, &discard, 48000.);
        }), 0);
        assert!(cursor.current.is_some());
        assert!(discard.is_empty());
        assert!(!cursor.current.as_ref().unwrap().reserved);
        drop(retired); // Actual off-audio disposal releases exactly that credit.
        assert!(!mailbox.retirement.load(Ordering::Acquire));
        assert_eq!(allocations(|| { cursor.poll(&c, &mailbox, &discard, 48000.); }), 0);
        let second_retired = discard.pop().unwrap();
        assert!(second_retired.preview.as_ref().unwrap().reserved);
        assert!(mailbox.retirement.load(Ordering::Acquire));
        drop(second_retired);
        assert!(!mailbox.retirement.load(Ordering::Acquire));
    }

    #[test]
    fn ready_full_returns_payload_and_new_stop_silences_old_cursor() {
        let c = Control::default();
        let mailbox = Mailbox::default();
        let discard = ArrayQueue::new(1);
        let mut cursor = Cursor::default();
        let first = c.begin(0).unwrap();
        mailbox.publish(&c, payload(first)).ok().unwrap();
        let second = c.begin(0).unwrap();
        let held = mailbox.publish(&c, payload(second)).err().unwrap();
        assert_eq!(held.id, second);
        assert_eq!(allocations(|| { cursor.poll(&c, &mailbox, &discard, 48000.); }), 0);
        assert!(cursor.current.is_none());
        drop(discard.pop());
        mailbox.publish(&c, held).ok().unwrap();
        assert_eq!(allocations(|| { cursor.poll(&c, &mailbox, &discard, 48000.); }), 0);
        assert_eq!(c.status(second), Some(Status::Playing));
        c.stop_id(second);
        let mut out = [[1.; 2]; 1];
        assert_eq!(allocations(|| { cursor.render(&c, 48000., &mut out); }), 0);
        assert_eq!(out, [[0.; 2]]);
        assert_eq!(allocations(|| { cursor.poll(&c, &mailbox, &discard, 48000.); }), 0);
        assert!(discard.pop().unwrap().preview.is_some());
    }
}
