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
/// Hard bound on command rows plus provenance metadata inspected per query.
pub const MAX_COMMAND_SCAN_STEPS: usize = 4096;

/// Pure current-state evidence. Exempting a proven future timer from diagnostic
/// activation settlement never authorizes a snapshot overlay or cancels work.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct CommandProgress {
    pub at: u64,
    pub total: usize,
    pub observed: usize,
    pub exact_future_parked_listener_resumes: usize,
    pub blocking_observed: usize,
    pub reserved: usize,
    pub scan_steps: usize,
    pub scan_limit: usize,
    pub truncated: bool,
}
impl CommandProgress {
    pub fn non_background_complete(&self) -> bool {
        !self.truncated && self.blocking_observed == 0 && self.reserved == 0
    }
}
impl Runtime {
    /// Off audio only: no polling, callback admission, time advancement or queue
    /// mutation. Actual handles, current owner and validated driver bytecode
    /// establish provenance; matching a count of waiting callbacks is insufficient.
    pub fn onset_command_progress(
        &self,
        plan: crate::PlanId,
        limit: usize,
    ) -> Result<CommandProgress, Error> {
        if plan != self.active_plan || self.plans.get(plan.0).is_none() {
            return Err(Error::StaleHandle);
        }
        let limit = limit.min(MAX_COMMAND_SCAN_STEPS);
        let mut remaining = limit;
        let mut row = CommandProgress {
            at: self.now,
            total: self.commands.len(),
            observed: 0,
            exact_future_parked_listener_resumes: 0,
            blocking_observed: 0,
            reserved: self.reserved_commands,
            scan_steps: 0,
            scan_limit: limit,
            truncated: false,
        };
        for command in &self.commands {
            if remaining == 0 {
                row.truncated = true;
                break;
            }
            remaining -= 1;
            row.observed += 1;
            let parked = match command.action {
                crate::schedule::Action::Resume(id) if command.at > self.now => {
                    self.onset_parked_listener(id, plan, &mut remaining)
                }
                _ => Some(false),
            };
            match parked {
                Some(true) => row.exact_future_parked_listener_resumes += 1,
                Some(false) => row.blocking_observed += 1,
                None => {
                    row.blocking_observed += 1;
                    row.truncated = true;
                    break;
                }
            }
        }
        row.scan_steps = limit - remaining;
        row.truncated |= row.observed < row.total;
        Ok(row)
    }
    fn onset_parked_listener(
        &self,
        id: crate::BehaviorId,
        plan: crate::PlanId,
        remaining: &mut usize,
    ) -> Option<bool> {
        let Some(c) = self.behaviors.get(id.0) else {
            return Some(false);
        };
        if c.owner != crate::BehaviorOwner::Plan(plan)
            || c.outcome.is_some()
            || !c.waiting
            || c.yielded_at.is_some()
            || c.async_wait.is_some()
            || c.async_result.is_some()
        {
            return Some(false);
        }
        let generation = self.plans.get(plan.0)?;
        let Some(program) = generation.prepared.programs.get(c.program) else {
            return Some(false);
        };
        let Some(body) = program
            .snapshot_listener_driver
            .and_then(|body| generation.prepared.programs.get(body as usize))
        else {
            return Some(false);
        };
        // Deliberately independent of body.snapshot_overlay_safe: a listener
        // may change DSP while its timer driver is parked. This is not overlay admission.
        if body.script_instance != program.script_instance
            || body.source_slot != program.source_slot
            || !c
                .pc
                .checked_sub(1)
                .and_then(|pc| program.code.get(pc))
                .is_some_and(|op| {
                    matches!(
                        op,
                        crate::Instruction::Wait(_) | crate::Instruction::WaitLocal { .. }
                    )
                })
        {
            return Some(false);
        }
        for entry in generation.prepared.plan_programs.iter() {
            if *remaining == 0 {
                return None;
            }
            *remaining -= 1;
            if entry.program == c.program && c.context.stage() == Some(entry.stage) {
                return Some(true);
            }
        }
        Some(false)
    }
}


impl Runtime {
    /// Read-only off-audio observation between render calls. This is current
    /// state, never a replacement for the historical selection-time Birth row.
    /// This legacy snapshot names one actual Pcm AssetId. If any live source
    /// requires a channel family, return incomplete before emitting any row;
    /// no physical pack is a substitute for that logical source identity.
    pub fn visit_onset_voices(&self, mut accept: impl FnMut(VoiceSnapshot) -> bool) -> bool {
        if self.voices.slots.iter().any(|slot| slot.value.as_ref().is_some_and(|voice| {
            self.plans.get(voice.plan.0).and_then(|plan| plan.source(voice.sample))
                .is_none_or(|source| source.stereo().is_none())
        })) { return false; }
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
        // Unsupported or disabled trace paths must not attempt a legacy Pcm
        // projection before discovering that no recorder exists.
        if plan.dsp.trace.is_none() { return; }
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
    fn parked_runtime() -> Runtime {
        use crate::{Instruction as I, Limits, PlanProgram, Prepared, Program, WaitLifetime};
        let driver = Program::new(vec![
            I::Wait(100),
            I::StartProgram { program: 1 },
            I::Jump { target: 0 },
        ])
        .unwrap()
        .with_snapshot_listener_driver()
        .unwrap()
        .with_wait_lifetime(WaitLifetime::Callback);
        let body = Program::new(vec![I::End])
            .unwrap()
            .without_snapshot_overlay()
            .with_wait_lifetime(WaitLifetime::Callback);
        let plan = Prepared::new(48000, vec![], vec![], 0)
            .unwrap()
            .with_programs(vec![driver, body], None)
            .unwrap()
            .with_plan_programs(vec![PlanProgram {
                program: 0,
                stage: 0,
            }])
            .unwrap();
        let limits = Limits::for_plan(&plan, 8, 16);
        let mut runtime = Runtime::new(plan, limits).unwrap();
        runtime.render(&mut [[0.; 2]; 1]).unwrap();
        runtime
    }
    #[test]
    fn command_query_proves_actual_parked_timer_without_overlay_or_queue_mutation() {
        let mut rt = parked_runtime();
        let plan = rt.active_plan();
        assert!(!rt.plans.get(plan.0).unwrap().prepared.programs[1].snapshot_overlay_safe);
        let before = (
            rt.now(),
            rt.pending_commands(),
            rt.voice_count(),
            rt.note_count(),
        );
        let row = rt.onset_command_progress(plan, usize::MAX).unwrap();
        assert_eq!(row.total, 1);
        assert_eq!(row.observed, 1);
        assert_eq!(row.exact_future_parked_listener_resumes, 1);
        assert_eq!(row.blocking_observed, 0);
        assert!(row.non_background_complete());
        assert_eq!(row.scan_limit, 4096);
        assert_eq!(
            before,
            (
                rt.now(),
                rt.pending_commands(),
                rt.voice_count(),
                rt.note_count()
            )
        );
        let short = rt.onset_command_progress(plan, 1).unwrap();
        assert!(short.truncated && !short.non_background_complete());
        assert_eq!(short.scan_steps, 1);
        rt.reserved_commands = 1;
        let reserved = rt.onset_command_progress(plan, 4096).unwrap();
        assert_eq!(reserved.reserved, 1);
        assert!(!reserved.non_background_complete());
        rt.reserved_commands = 0;
        let performance = rt.performance(0).unwrap();
        rt.schedule_event(
            rt.now() + 50,
            crate::Event::Controller(performance, 12, 123),
        )
        .unwrap();
        let scheduled = rt.onset_command_progress(plan, 4096).unwrap();
        assert_eq!(scheduled.total, 2);
        assert_eq!(scheduled.blocking_observed, 1);
        assert!(!scheduled.non_background_complete());
    }
    #[test]
    fn command_query_blocks_active_async_foreign_due_and_unvalidated_continuations() {
        let mut rt = parked_runtime();
        let plan = rt.active_plan();
        let other = parked_runtime().active_plan();
        assert_eq!(
            rt.onset_command_progress(other, 4096),
            Err(Error::StaleHandle)
        );
        let crate::schedule::Action::Resume(id) = rt.commands[0].action else {
            panic!()
        };
        let original = *rt.behaviors.get(id.0).unwrap();
        for variant in 0..9 {
            let mut changed = original;
            match variant {
                0 => changed.waiting = false,
                1 => changed.yielded_at = Some(rt.now()),
                2 => changed.async_wait = Some(1),
                3 => changed.async_result = Some((1, 1)),
                4 => changed.owner = crate::BehaviorOwner::Plan(other),
                5 => changed.outcome = Some(crate::Outcome::Finished),
                6 => changed.program = 1,
                7 => changed.pc = 2, // last instruction is StartProgram, not the validated wait.
                8 => changed.context = crate::behavior::PlanContext::Bare,
                _ => unreachable!(),
            }
            *rt.behaviors.get_mut(id.0).unwrap() = changed;
            let row = rt.onset_command_progress(plan, 4096).unwrap();
            assert_eq!(row.blocking_observed, 1, "variant {variant}");
            assert_eq!(row.exact_future_parked_listener_resumes, 0);
            assert!(!row.non_background_complete());
        }
        *rt.behaviors.get_mut(id.0).unwrap() = original;
        let action = rt.commands[0].action;
        let mut stale = id;
        stale.0.generation = stale.0.generation.checked_add(1).unwrap();
        rt.commands[0].action = crate::schedule::Action::Resume(stale);
        assert_eq!(
            rt.onset_command_progress(plan, 4096)
                .unwrap()
                .blocking_observed,
            1
        );
        rt.commands[0].action = action;
        let before = rt.now;
        rt.now = rt.commands[0].at;
        assert_eq!(
            rt.onset_command_progress(plan, 4096)
                .unwrap()
                .blocking_observed,
            1
        );
        rt.now = before;
        assert!(
            rt.onset_command_progress(plan, 4096)
                .unwrap()
                .non_background_complete()
        );
    }
    #[test]
    fn command_query_checks_real_body_instance_slot_and_plan_stage_metadata() {
        let mut rt = parked_runtime();
        let plan = rt.active_plan();
        {
            let prepared = rt.plans.get_mut(plan.0).unwrap().prepared.as_mut();
            prepared.programs[1].source_slot = 1;
        }
        assert_eq!(
            rt.onset_command_progress(plan, 4096)
                .unwrap()
                .blocking_observed,
            1
        );
        {
            let prepared = rt.plans.get_mut(plan.0).unwrap().prepared.as_mut();
            prepared.programs[1].source_slot = 0;
            prepared.programs[1].script_instance = Some(crate::ScriptInstanceId(1));
        }
        assert_eq!(
            rt.onset_command_progress(plan, 4096)
                .unwrap()
                .blocking_observed,
            1
        );
        {
            let prepared = rt.plans.get_mut(plan.0).unwrap().prepared.as_mut();
            prepared.programs[1].script_instance = None;
            prepared.plan_programs[0].stage = usize::MAX;
        }
        assert_eq!(
            rt.onset_command_progress(plan, 4096)
                .unwrap()
                .blocking_observed,
            1
        );
    }
}
