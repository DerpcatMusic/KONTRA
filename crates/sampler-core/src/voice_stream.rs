//! Port from v1 0cb7a8a0:src/engine/stream.rs: one source-time ring per voice.
use crate::{AssetId, DecodeFailure, Error, Frame, Packed, StreamError, source::Cursor};
use rtrb::{Consumer, Producer, RingBuffer};
use sampler_pool::{RingConsumer, RingProducer, RingRead, STREAM_RING_FRAMES, stream_rings};

pub(crate) const CHUNK: usize = 2048;
const URGENT: u64 = (2 * CHUNK) as u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Binding {
    pub slot: usize,
    pub generation: u64,
}
#[derive(Clone, Copy)]
struct Setup {
    binding: Binding,
    asset: AssetId,
    cursor: Cursor,
}
struct AudioSlot {
    ring: RingConsumer,
    setup: Option<Setup>,
}
struct WorkerSlot {
    ring: RingProducer,
    setup: Option<Setup>,
    launched: Option<u64>,
    retries: u8,
    retry_at: Option<std::time::Instant>,
    fault: Option<DecodeFailure>,
}

pub(crate) struct VoiceStreams {
    slots: Vec<AudioSlot>,
    free: Vec<usize>,
    requests: Producer<Setup>,
    faults: Consumer<(Binding, DecodeFailure)>,
}
pub(crate) struct VoiceStreamWorker {
    slots: Vec<WorkerSlot>,
    requests: Consumer<Setup>,
    faults: Producer<(Binding, DecodeFailure)>,
    free: Vec<Box<[Frame]>>,
    hand: usize,
    pending_faults: bool,
    next_retry: Option<std::time::Instant>,
}

/// A worker-owned source-time run. Geometry retains all native loop slots.
pub struct VoiceStreamJob {
    setup: Setup,
    from: u64,
    len: usize,
    samples: Box<[Frame]>,
}
impl VoiceStreamJob {
    pub fn asset(&self) -> AssetId {
        self.setup.asset
    }
    /// The worker may cache physical decode blocks; the audio side never reads storage.
    pub fn fill(
        &mut self,
        mut frame: impl FnMut(usize) -> Result<Frame, DecodeFailure>,
    ) -> Result<(), DecodeFailure> {
        for (i, out) in self.samples[..self.len].iter_mut().enumerate() {
            *out = self
                .setup
                .cursor
                .stream_frame(self.from + i as u64, &mut frame)?;
        }
        Ok(())
    }
}

impl VoiceStreams {
    pub(crate) fn new(slots: usize, jobs: usize) -> Result<(Self, VoiceStreamWorker), Error> {
        let (audio, workers) = stream_rings(slots).map_err(|_| Error::Capacity)?;
        let (requests, incoming) = RingBuffer::new(slots.checked_mul(2).ok_or(Error::Capacity)?);
        let (outgoing, faults) = RingBuffer::new(slots);
        let mut buffers = Vec::new();
        buffers
            .try_reserve_exact(jobs.max(1))
            .map_err(|_| Error::Capacity)?;
        for _ in 0..jobs.max(1) {
            let mut buffer = Vec::new();
            buffer
                .try_reserve_exact(CHUNK)
                .map_err(|_| Error::Capacity)?;
            buffer.resize(CHUNK, [0.; 2]);
            buffers.push(buffer.into_boxed_slice());
        }
        Ok((
            Self {
                slots: audio
                    .into_iter()
                    .map(|ring| AudioSlot { ring, setup: None })
                    .collect(),
                free: (0..slots).rev().collect(),
                requests,
                faults,
            },
            VoiceStreamWorker {
                slots: workers
                    .into_iter()
                    .map(|ring| WorkerSlot {
                        ring,
                        setup: None,
                        launched: None,
                        retries: 0,
                        retry_at: None,
                        fault: None,
                    })
                    .collect(),
                requests: incoming,
                faults: outgoing,
                free: buffers,
                hand: 0,
                pending_faults: false,
                next_retry: None,
            },
        ))
    }

    pub(crate) fn bytes(&self) -> usize {
        self.slots.len() * STREAM_RING_FRAMES * size_of::<Frame>()
    }
    pub(crate) fn available(&self) -> bool {
        self.available_count() != 0
    }
    pub(crate) fn available_count(&self) -> usize {
        self.free.len().min(self.requests.slots())
    }
    pub(crate) fn stop(&mut self, binding: Binding) {
        let slot = &mut self.slots[binding.slot];
        if slot
            .setup
            .is_some_and(|s| s.binding.generation == binding.generation)
        {
            slot.ring.stop();
            slot.setup = None;
            self.free.push(binding.slot);
        }
    }
    pub(crate) fn update(
        &mut self,
        binding: &mut Option<Binding>,
        asset: AssetId,
        cursor: Cursor,
        head: &[(usize, Packed)],
    ) -> Result<(), StreamError> {
        if self.requests.is_abandoned() {
            return Err(StreamError::Disconnected);
        }
        let read = cursor.stream_read();
        if let Some(b) = *binding {
            let slot = &mut self.slots[b.slot];
            if slot.setup.is_some_and(|s| {
                s.binding.generation == b.generation
                    && s.asset == asset
                    && s.cursor.same_stream_path(&cursor)
                    && (read >= slot.ring.start_boundary()
                        || cursor.resident_stream_end(head, read) >= slot.ring.start_boundary())
            }) {
                slot.ring
                    .release_below(read)
                    .map_err(|_| StreamError::InvalidRange)?;
                return Ok(());
            }
            self.stop(b);
            *binding = None;
        }
        if self.requests.slots() == 0 {
            return Err(StreamError::Capacity);
        }
        let index = self.free.pop().ok_or(StreamError::Capacity)?;
        let from = cursor
            .resident_stream_end(head, read)
            .min(read.saturating_add(STREAM_RING_FRAMES as u64));
        let slot = &mut self.slots[index];
        let generation = match slot.ring.configure(from, read) {
            Ok(g) => g,
            Err(_) => {
                self.free.push(index);
                return Err(StreamError::SequenceExhausted);
            }
        };
        let b = Binding {
            slot: index,
            generation,
        };
        let setup = Setup {
            binding: b,
            asset,
            cursor,
        };
        self.requests
            .push(setup)
            .expect("reserved setup queue slot");
        slot.setup = Some(setup);
        *binding = Some(b);
        Ok(())
    }
    pub(crate) fn ready(
        &self,
        b: Binding,
        cursor: Cursor,
        head: &[(usize, Packed)],
        frames: u32,
    ) -> bool {
        let read = cursor.stream_read();
        let needed = cursor
            .stream_end(frames.min(256))
            .min(cursor.stream_limit());
        let resident = cursor.resident_stream_end(head, read);
        needed <= resident
            || self.slots[b.slot]
                .ring
                .published(b.generation)
                .is_some_and(|end| {
                    let from = self.slots[b.slot].ring.start_boundary();
                    from <= resident.max(read) && needed <= end
                })
    }
    pub(crate) fn read(&self, b: Binding) -> Option<RingRead<'_>> {
        let slot = &self.slots[b.slot];
        let start = slot.ring.read_boundary().max(slot.ring.start_boundary());
        let end = slot.ring.published(b.generation)?;
        slot.ring.claim(b.generation, start..end)
    }
    pub(crate) fn fault(&mut self) -> Option<(Binding, AssetId, StreamError)> {
        while let Ok((b, failure)) = self.faults.pop() {
            if self.slots[b.slot]
                .setup
                .is_some_and(|s| s.binding.generation == b.generation)
            {
                return Some((
                    b,
                    self.slots[b.slot].setup.unwrap().asset,
                    StreamError::DecodeFailed(failure),
                ));
            }
        }
        None
    }
    pub(crate) fn reader(&self) -> RingReader<'_> {
        RingReader { slots: &self.slots }
    }
    pub(crate) fn needs_work(&self, b: Binding) -> bool {
        let ring = &self.slots[b.slot].ring;
        ring.published(b.generation).is_none_or(|end| {
            end.saturating_sub(ring.read_boundary()) < (STREAM_RING_FRAMES - CHUNK) as u64
        })
    }
}

#[derive(Clone, Copy)]
pub(crate) struct RingReader<'a> {
    slots: &'a [AudioSlot],
}
impl<'a> RingReader<'a> {
    pub(crate) fn read(self, b: Binding) -> Option<RingRead<'a>> {
        let slot = &self.slots[b.slot];
        let start = slot.ring.read_boundary().max(slot.ring.start_boundary());
        let end = slot.ring.published(b.generation)?;
        slot.ring.claim(b.generation, start..end)
    }
}

impl VoiceStreamWorker {
    pub(crate) fn retry_delay(&self) -> Option<std::time::Duration> {
        if self.pending_faults {
            return Some(std::time::Duration::from_millis(10));
        }
        self.next_retry
            .map(|at| at.saturating_duration_since(std::time::Instant::now()))
    }
    pub(crate) fn next_job(&mut self) -> Option<VoiceStreamJob> {
        self.next_retry = None;
        while let Ok(setup) = self.requests.pop() {
            let slot = &mut self.slots[setup.binding.slot];
            slot.setup = Some(setup);
            slot.retries = 0;
            slot.retry_at = None;
            slot.fault = None;
        }
        if self.pending_faults {
            self.pending_faults = false;
            for slot in &mut self.slots {
                if let (Some(setup), Some(failure)) = (slot.setup, slot.fault) {
                    if self.faults.push((setup.binding, failure)).is_ok() {
                        slot.fault = None;
                    } else {
                        self.pending_faults = true;
                        break;
                    }
                }
            }
        }
        if self.free.is_empty() {
            return None;
        }
        // v1's two passes: every urgent voice before any speculative top-up.
        for lead in [URGENT, STREAM_RING_FRAMES as u64] {
            for offset in 0..self.slots.len() {
                let index = (self.hand + offset) % self.slots.len();
                let slot = &mut self.slots[index];
                if slot.launched.is_some() || slot.retries >= 3 {
                    continue;
                }
                let Some(setup) = slot.setup else {
                    continue;
                };
                let Some((generation, room)) = slot.ring.room() else {
                    continue;
                };
                if generation != setup.binding.generation {
                    continue;
                }
                if let Some(at) = slot.retry_at
                    && std::time::Instant::now() < at
                {
                    self.next_retry = Some(self.next_retry.map_or(at, |old| old.min(at)));
                    continue;
                }
                let read = slot.ring.read_boundary();
                let end = room
                    .end
                    .min(read.saturating_add(lead))
                    .min(setup.cursor.stream_limit());
                if room.start >= end
                    || lead != URGENT
                        && end - room.start < CHUNK as u64
                        && end < setup.cursor.stream_limit()
                {
                    continue;
                }
                let len = (end - room.start).min(CHUNK as u64) as usize;
                slot.launched = Some(generation);
                self.hand = (index + 1) % self.slots.len();
                return Some(VoiceStreamJob {
                    setup,
                    from: room.start,
                    len,
                    samples: self.free.pop().unwrap(),
                });
            }
        }
        None
    }
    pub(crate) fn complete(&mut self, job: VoiceStreamJob, result: Result<(), DecodeFailure>) {
        let result = result.and_then(|()| {
            job.samples[..job.len]
                .iter()
                .flatten()
                .all(|f| f.is_finite())
                .then_some(())
                .ok_or(DecodeFailure::InvalidSamples)
        });
        let slot = &mut self.slots[job.setup.binding.slot];
        slot.launched = None;
        if slot
            .setup
            .is_some_and(|s| s.binding.generation == job.setup.binding.generation)
        {
            match result {
                Ok(()) => {
                    let _ = slot.ring.write(
                        job.setup.binding.generation,
                        job.from,
                        &job.samples[..job.len],
                    );
                    slot.retries = 0;
                    slot.retry_at = None;
                }
                Err(failure) => {
                    if failure == DecodeFailure::Unavailable {
                        slot.retries += 1;
                    } else {
                        slot.retries = 3;
                    }
                    slot.retry_at =
                        Some(std::time::Instant::now() + std::time::Duration::from_millis(10));
                    if failure != DecodeFailure::Unavailable || slot.retries == 3 {
                        if self.faults.push((job.setup.binding, failure)).is_err() {
                            slot.fault = Some(failure);
                            self.pending_faults = true;
                        }
                    }
                }
            }
        }
        self.free.push(job.samples);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Pcm, Playback};
    fn cursor(frames: usize) -> Cursor {
        Playback::default().cursor(frames, 48000, 48000).unwrap()
    }
    #[test]
    fn urgent_pass_is_fair_and_speculative_fill_keeps_the_final_partial_chunk() {
        let (mut audio, mut worker) = VoiceStreams::new(3, 1).unwrap();
        let asset = Pcm::streamed(48000, 4600).unwrap().asset_id();
        let mut bindings = [None; 3];
        for b in &mut bindings {
            audio.update(b, asset, cursor(4600), &[]).unwrap();
        }
        let mut slots = Vec::new();
        while let Some(mut job) = worker.next_job() {
            slots.push((job.setup.binding.slot, job.from, job.len));
            job.fill(|at| Ok([at as f32; 2])).unwrap();
            worker.complete(job, Ok(()));
        }
        assert_eq!(slots.len(), 9);
        for pass in slots.chunks(3) {
            assert_eq!(
                pass.iter().map(|&(slot, _, _)| slot).collect::<Vec<_>>(),
                vec![0, 1, 2]
            );
        }
        assert!(
            slots[6..]
                .iter()
                .all(|&(_, from, len)| from == 4096 && len == 504)
        );
        for b in bindings.into_iter().flatten() {
            assert_eq!(audio.read(b).unwrap().frame(4599), Some([4599.; 2]));
        }
    }
    #[test]
    fn stale_completion_cannot_publish_into_a_reused_voice() {
        let (mut audio, mut worker) = VoiceStreams::new(1, 1).unwrap();
        let asset = Pcm::streamed(48000, 20000).unwrap().asset_id();
        let mut b = None;
        audio.update(&mut b, asset, cursor(20000), &[]).unwrap();
        let mut stale = worker.next_job().unwrap();
        audio.stop(b.take().unwrap());
        let changed = Playback {
            start: 8000,
            ..Default::default()
        }
        .cursor(20000, 48000, 48000)
        .unwrap();
        audio.update(&mut b, asset, changed, &[]).unwrap();
        assert!(worker.next_job().is_none());
        stale.fill(|_| Ok([-1.; 2])).unwrap();
        worker.complete(stale, Ok(()));
        let mut current = worker.next_job().unwrap();
        current.fill(|at| Ok([at as f32; 2])).unwrap();
        worker.complete(current, Ok(()));
        assert_eq!(audio.read(b.unwrap()).unwrap().frame(0), Some([8000.; 2]));
    }
    #[test]
    fn terminal_decode_fault_is_generation_scoped_and_does_not_repeat() {
        let (mut audio, mut worker) = VoiceStreams::new(1, 1).unwrap();
        let asset = Pcm::streamed(48000, 20000).unwrap().asset_id();
        let mut b = None;
        audio.update(&mut b, asset, cursor(20000), &[]).unwrap();
        let job = worker.next_job().unwrap();
        worker.complete(job, Err(DecodeFailure::InvalidSamples));
        assert_eq!(
            audio.fault(),
            Some((
                b.unwrap(),
                asset,
                StreamError::DecodeFailed(DecodeFailure::InvalidSamples)
            ))
        );
        assert!(audio.fault().is_none() && worker.next_job().is_none());
        audio.stop(b.take().unwrap());
        audio.update(&mut b, asset, cursor(20000), &[]).unwrap();
        assert!(worker.next_job().is_some());
    }

    #[test]
    fn stale_fault_queue_pressure_does_not_drop_a_new_terminal_failure() {
        let (mut audio, mut worker) = VoiceStreams::new(1, 1).unwrap();
        let asset = Pcm::streamed(48000, 20000).unwrap().asset_id();
        let mut b = None;
        for _ in 0..2 {
            audio.update(&mut b, asset, cursor(20000), &[]).unwrap();
            let job = worker.next_job().unwrap();
            worker.complete(job, Err(DecodeFailure::InvalidSamples));
            if !worker.pending_faults {
                audio.stop(b.take().unwrap());
            }
        }
        assert!(worker.pending_faults);
        assert!(
            audio.fault().is_none(),
            "the queued previous generation is stale"
        );
        assert!(worker.next_job().is_none());
        assert_eq!(
            audio.fault(),
            Some((
                b.unwrap(),
                asset,
                StreamError::DecodeFailed(DecodeFailure::InvalidSamples)
            ))
        );
    }

    #[test]
    fn admission_accounts_for_queued_setups_before_publishing_a_voice() {
        let (mut audio, mut worker) = VoiceStreams::new(1, 1).unwrap();
        let asset = Pcm::streamed(48000, 20000).unwrap().asset_id();
        let mut b = None;
        for _ in 0..2 {
            audio.update(&mut b, asset, cursor(20000), &[]).unwrap();
            audio.stop(b.take().unwrap());
        }
        assert_eq!(audio.available_count(), 0);
        assert_eq!(
            audio.update(&mut b, asset, cursor(20000), &[]),
            Err(StreamError::Capacity)
        );
        assert!(b.is_none());
        assert!(worker.next_job().is_none());
        assert_eq!(audio.available_count(), 1);
    }

    #[test]
    fn failed_onset_retries_at_its_deadline_and_idle_workers_do_not_poll() {
        let (mut audio, mut worker) = VoiceStreams::new(1, 1).unwrap();
        let asset = Pcm::streamed(48000, 20000).unwrap().asset_id();
        let mut b = None;
        assert!(worker.next_job().is_none() && worker.retry_delay().is_none());
        audio.update(&mut b, asset, cursor(20000), &[]).unwrap();
        let job = worker.next_job().unwrap();
        worker.complete(job, Err(DecodeFailure::Unavailable));
        assert!(worker.next_job().is_none());
        assert!(worker.retry_delay().is_some());
        assert!(audio.fault().is_none());
        worker.slots[0].retry_at = Some(std::time::Instant::now());
        let mut retry = worker.next_job().unwrap();
        retry.fill(|at| Ok([at as f32; 2])).unwrap();
        worker.complete(retry, Ok(()));
        while let Some(mut job) = worker.next_job() {
            job.fill(|at| Ok([at as f32; 2])).unwrap();
            worker.complete(job, Ok(()));
        }
        assert!(worker.retry_delay().is_none());
        assert_eq!(audio.read(b.unwrap()).unwrap().frame(0), Some([0.; 2]));
    }

    #[test]
    fn purged_resident_prefix_is_backfilled_without_leaving_a_ring_hole() {
        let (mut audio, mut worker) = VoiceStreams::new(1, 1).unwrap();
        let pcm = Pcm::streamed(48000, 20000).unwrap();
        pcm.set_ranges(vec![(0, vec![[0.5; 2]; 4048].into_boxed_slice())])
            .unwrap();
        let mut b = None;
        audio
            .update(
                &mut b,
                pcm.asset_id(),
                cursor(20000),
                &pcm.try_head().unwrap(),
            )
            .unwrap();
        let original = b;
        while let Some(mut job) = worker.next_job() {
            job.fill(|at| Ok([at as f32; 2])).unwrap();
            worker.complete(job, Ok(()));
        }
        pcm.set_ranges(vec![]).unwrap();
        audio
            .update(&mut b, pcm.asset_id(), cursor(20000), &[])
            .unwrap();
        assert_ne!(
            b, original,
            "missing resident prefix must reconfigure the ring"
        );
        let mut job = worker.next_job().unwrap();
        assert_eq!(job.from, 0);
        job.fill(|at| Ok([at as f32; 2])).unwrap();
        worker.complete(job, Ok(()));
        assert_eq!(audio.read(b.unwrap()).unwrap().frame(0), Some([0.; 2]));
        assert!(audio.ready(b.unwrap(), cursor(20000), &[], 128));
    }
}
