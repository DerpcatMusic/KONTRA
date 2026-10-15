//! Selection-time source witness for the existing opt-in signal trace.
//! Numeric identities only. Window rows predict the first read at selection;
//! they do not claim a worker completion time or an observed reader miss.
use crate::{AssetId, Error, Runtime, VoiceId};
use rtrb::{Consumer, Producer, RingBuffer};
use serde::Serialize;
use std::sync::{Arc, Mutex, atomic::{AtomicU64, Ordering}};

/// The onset reduction uses strict abs(sample) > this amplitude, separately
/// from exact nonzero. Neither reduction is a perceptual attack detector.
pub const THRESHOLD: f64 = 1e-5;
/// Independent of asset length and ring capacity; truncation invalidates capture.
pub const MAX_WINDOW_ROWS: usize = 64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct PhysicalVoice {
    pub runtime: u64,
    pub slot: usize,
    pub generation: u64,
}
impl From<VoiceId> for PhysicalVoice {
    fn from(value: VoiceId) -> Self {
        Self { runtime: value.0.runtime, slot: value.0.index, generation: value.0.generation }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct SignalOnset {
    /// Channel-local sample offsets from TraceRecord.at, not block timestamps.
    pub nonzero: [Option<u32>; 2],
    pub above_threshold: [Option<u32>; 2],
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Cursor {
    pub base_start: u64,
    pub traversal: u64,
    /// None when the cursor is outside its source view; never falls back to base.
    pub frame: Option<u64>,
    /// Fraction of traversal step; reverse/loop geometry retains its own law.
    pub fraction: f64,
    pub step: f64,
}
impl From<crate::source::Cursor> for Cursor {
    fn from(value: crate::source::Cursor) -> Self {
        let (traversal, fraction, frame) = value.trace_phase();
        Self { base_start: value.trace_start(), traversal, frame, fraction, step: value.step() }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Birth {
    pub at: u64,
    pub voice: PhysicalVoice,
    pub event: u64,
    pub parent_event: Option<u64>,
    pub family: [u64; 2],
    pub zone: u32,
    pub group: Option<u32>,
    pub sample: usize,
    pub asset: AssetId,
    pub source_rate: u32,
    pub key: u8,
    pub velocity: f64,
    pub control_revision: u64,
    /// Actual return from the onset consumer, including its dynamic inputs.
    pub used_start_offset: u32,
    pub source_offset_micros: u32,
    pub seed: u64,
    pub cursor: Cursor,
    pub cold: bool,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Window {
    pub at: u64,
    pub voice: PhysicalVoice,
    pub asset: AssetId,
    pub page: usize,
    /// Exact per-page subrange of the first interpolation demand, exclusive end.
    pub frames: [usize; 2],
    pub resident: bool,
    /// 0 missing, 1 pending, 2 ready, 3 unavailable, 4 invalid samples.
    pub page_status: u8,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Record { Birth(Birth), Window(Window) }

pub(super) struct Prepared {
    writer: Mutex<Option<Producer<Record>>>,
    pub(super) reader: Reader,
}
pub(super) struct Recorder {
    // get_mut on the sole audio writer never acquires this mutex.
    writer: Mutex<Producer<Record>>,
    dropped: Arc<AtomicU64>,
}
#[derive(Clone)]
pub struct Reader {
    consumer: Arc<Mutex<Consumer<Record>>>,
    dropped: Arc<AtomicU64>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrainError { Poisoned }
impl Reader {
    /// Off audio only. A poisoned consumer fails explicitly.
    pub fn drain(&self) -> Result<Vec<Record>, DrainError> {
        let mut consumer = self.consumer.lock().map_err(|_| DrainError::Poisoned)?;
        let mut rows = Vec::new();
        while let Ok(row) = consumer.pop() { rows.push(row); }
        Ok(rows)
    }
    pub fn dropped(&self) -> u64 { self.dropped.load(Ordering::Relaxed) }
}
impl Prepared {
    pub(super) fn new(capacity: usize) -> Self {
        let (writer, consumer) = RingBuffer::new(capacity);
        Self { writer: Mutex::new(Some(writer)), reader: Reader {
            consumer: Arc::new(Mutex::new(consumer)), dropped: Arc::new(AtomicU64::new(0)) } }
    }
    pub(super) fn recorder(&self) -> Result<Recorder, Error> {
        let writer = self.writer.lock().map_err(|_| Error::InvalidInput)?
            .take().ok_or(Error::InvalidInput)?;
        Ok(Recorder { writer: Mutex::new(writer), dropped: self.reader.dropped.clone() })
    }
}
impl Recorder {
    fn lose_row(&mut self) {
        self.dropped.fetch_add(1, Ordering::Relaxed);
    }
    fn push(&mut self, row: Record) -> bool {
        if self.writer.get_mut().unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(row).is_err() { self.lose_row(); false } else { true }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct VoiceSnapshot {
    pub at: u64,
    pub voice: PhysicalVoice,
    pub zone: u32,
    pub asset: AssetId,
    pub cursor: Cursor,
    pub cold: bool,
    pub starved: bool,
}
impl Runtime {
    /// Read-only off-audio observation between render calls. This is current
    /// state, never a replacement for the historical selection-time Birth row.
    pub fn visit_onset_voices(&self, mut accept: impl FnMut(VoiceSnapshot) -> bool) -> bool {
        for (index, slot) in self.voices.slots.iter().enumerate() {
            let Some(voice) = slot.value.as_ref() else { continue };
            let plan = self.plans.get(voice.plan.0).unwrap();
            if !accept(VoiceSnapshot { at: self.now,
                voice: VoiceId(self.voices.id(index)).into(), zone: voice.source_zone,
                asset: plan.asset(voice.sample).asset_id(), cursor: voice.cursor.into(),
                cold: voice.cursor.holding_onset(), starved: voice.cursor.starved() }) { return false; }
        }
        true
    }

    /// Only called on the existing trace path after selection has initialized
    /// this physical voice. Direct Runtime::start sources are outside this scope.
    pub(crate) fn trace_selected_birth(&mut self, id: VoiceId, used_start_offset: u32, seed: u64) {
        let Some(voice) = self.voices.get(id.0) else { return };
        let family = self.families.get(voice.family.0).unwrap();
        let note = self.notes.get(family.note.0).unwrap();
        let parent_event = note.parent.and_then(|id| self.notes.get(id.0)).map(|n| n.order);
        let plan = self.plans.get_mut(voice.plan.0).unwrap();
        let asset = plan.samples.asset(&plan.prepared, voice.sample).unwrap();
        let Some(trace) = plan.dsp.trace.as_mut() else { return };
        let physical = PhysicalVoice::from(id);
        trace.onset.push(Record::Birth(Birth { at: self.now, voice: physical,
            event: note.order, parent_event,
            family: [voice.family.0.index as u64, voice.family.0.generation],
            zone: voice.source_zone, group: voice.group, sample: voice.sample,
            asset: asset.asset_id(), source_rate: asset.sample_rate(),
            key: note.pitch.key(), velocity: note.velocity, control_revision: plan.controls.revision,
            used_start_offset, source_offset_micros: self.note_events[family.note.0.index].source_offset_micros,
            seed, cursor: voice.cursor.into(), cold: voice.cursor.holding_onset() }));
        // Same immutable cursor demand law as admission, one output frame only.
        // No worker polling, cache mutation, allocation, logging, or PCM retained.
        let mut window_rows = 0;
        voice.cursor.visit_demand(1, voice.envelope, |offset, frames| {
            if frames.is_empty() { return true; }
            for page in frames.start / crate::PAGE_FRAMES..=(frames.end - 1) / crate::PAGE_FRAMES {
                if window_rows == MAX_WINDOW_ROWS {
                    // Count the first omitted attempt, then stop predicting.
                    // Nonzero loss invalidates the capture; do not invent the
                    // number of further ranges that this visitor did not walk.
                    trace.onset.lose_row();
                    return false;
                }
                window_rows += 1;
                let start = frames.start.max(page * crate::PAGE_FRAMES);
                let end = frames.end.min((page + 1) * crate::PAGE_FRAMES);
                let key = crate::PageKey { asset: asset.asset_id(), index: page };
                let status = self.stream_cache.as_ref().map_or(crate::PageStatus::Missing, |c| c.status(key));
                if !trace.onset.push(Record::Window(Window { at: self.now + u64::from(offset),
                    voice: physical, asset: asset.asset_id(), page, frames: [start, end],
                    resident: asset.resident_covers(start..end), page_status: match status {
                        crate::PageStatus::Missing => 0, crate::PageStatus::Pending => 1,
                        crate::PageStatus::Ready => 2,
                        crate::PageStatus::Failed(crate::DecodeFailure::Unavailable) => 3,
                        crate::PageStatus::Failed(crate::DecodeFailure::InvalidSamples) => 4 } })) { return false; }
            }
            true
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cursor_witness_preserves_fractional_authored_offset_and_dynamic_skip() {
        let cursor = crate::Playback { start: 7, ..Default::default() }
            .cursor(256, 44100, 48000).unwrap().with_offset(10, 44100).skip(32);
        let witness = Cursor::from(cursor);
        assert_eq!((witness.base_start, witness.traversal, witness.frame), (7, 32, Some(39)));
        assert_eq!(witness.fraction, 0.441);
        assert_eq!(witness.step, 44100. / 48000.);
        let past_end = crate::Playback { start: 7, ..Default::default() }
            .cursor(256, 48000, 48000).unwrap().skip(1000);
        assert_eq!(Cursor::from(past_end).frame, None);
        assert_eq!(Cursor::from(past_end).traversal, 1000);
    }
    #[test]
    fn a_poisoned_drain_is_explicit() {
        let prepared = Prepared::new(1);
        let reader = prepared.reader.clone();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = reader.consumer.lock().unwrap();
            panic!("synthetic drain poison");
        }));
        assert!(matches!(reader.drain(), Err(DrainError::Poisoned)));
    }
}
