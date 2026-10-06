//! Note-owned release context survives source completion and prepared-plan changes.
use super::{Error, Note, NoteId, Runtime};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReleaseCause {
    KeyUp,
    /// A downstream script event ended; its external input may still be held.
    Script,
    Pedal,
    Explicit,
    BehaviorCancelled,
    BehaviorFault,
    Parent,
    AllNotesOff,
    AllSoundOff,
    Panic,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KeyRelease {
    pub at: u64,
    /// Normalized release velocity; absent is distinct from an explicit zero.
    pub velocity: Option<f64>,
    pub cause: ReleaseCause,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GateRelease {
    pub at: u64,
    pub cause: ReleaseCause,
}

/// First key and gate transitions on the engine's monotonic sample clock.
/// `admitted_at` is logical admission, not a delayed source's audible onset.
/// A hard-silenced physical note may close its gate before receiving key-up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReleaseContext {
    pub admitted_at: u64,
    pub key: Option<KeyRelease>,
    pub gate: Option<GateRelease>,
}

impl Note {
    pub(super) fn key_down(&self) -> bool {
        self.key_release.is_none()
    }

    pub(super) fn gate(&self) -> bool {
        self.gate_release.is_none()
    }
}

/// Cold payload indexed by the owning note slot. Cause markers in Note are the
/// only key/gate state; payload fields are meaningful only after their transition.
#[derive(Clone, Copy, Default)]
pub(super) struct ReleaseTimes {
    pub groups_forwarded: bool,
    pub held: bool,
    pub admitted_at: u64,
    pub key_at: u64,
    pub gate_at: u64,
    pub velocity: Option<f64>,
    pub selection: [super::ReleaseStatus; 2],
    pub release_stage: Option<usize>,
    pub cleanup: Option<ReleaseCause>,
    pub cleanup_hard: bool,
}

pub(super) fn validate_velocity(velocity: Option<f64>) -> Result<(), Error> {
    if velocity.is_some_and(|v| !v.is_finite() || !(0.0..=1.0).contains(&v)) {
        Err(Error::InvalidInput)
    } else {
        Ok(())
    }
}

impl Runtime {
    pub(super) fn release_key(
        &mut self,
        id: NoteId,
        cause: ReleaseCause,
        velocity: Option<f64>,
    ) -> bool {
        let note = self.notes.get_mut(id.0).unwrap();
        if matches!(
            cause,
            ReleaseCause::KeyUp
                | ReleaseCause::AllNotesOff
                | ReleaseCause::Explicit
                | ReleaseCause::Panic
        ) {
            note.input_down = false;
        }
        if note.key_down() {
            // A source stop before attack forwarding consumes that pending attack.
            // Physical key-up retains the native deferred-attack rejection policy.
            if cause == ReleaseCause::Script && note.attack == super::AttackStatus::Pending {
                note.attack = super::AttackStatus::Suppressed;
            }
            self.release_note_callbacks(id);
            self.trim_release_callbacks(id, false);
            let note = self.notes.get_mut(id.0).unwrap();
            let times = &mut self.release_times[id.0.index];
            times.key_at = self.now;
            times.velocity = velocity;
            note.key_release = Some(cause);
            let musical = cause.triggers_key_release();
            if musical && self.note_events[id.0.index].release_routed {
                times.held = true;
                self.run_release_behavior(id, true);
                return true;
            }
            self.run_release_behavior(id, musical);
            if !self.release_times[id.0.index].held {
                self.run_release(id, Trigger::KeyRelease, musical);
            }
        }
        false
    }

    /// Stop a pending release before its script forwarding boundary. Physical key
    /// state/velocity stay recorded; pedal release cannot bypass this hold.
    pub fn suppress_release(&mut self, note: NoteId) -> Result<bool, Error> {
        self.apply_due();
        let n = self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        if n.key_down() {
            return Err(Error::InvalidInput);
        }
        if let Some(stage) = self.release_times[note.0.index].release_stage {
            return self.suppress_release_stage(note, stage);
        }
        let state = &mut self.release_times[note.0.index];
        if !n.gate() || state.groups_forwarded {
            return Ok(false);
        }
        state.held = true;
        Ok(true)
    }

    /// Forward a suppressed release once, preserving the original key-up context.
    /// Release layers use the current group draft; the gate still obeys pedals.
    pub fn resume_release(&mut self, note: NoteId) -> Result<bool, Error> {
        self.apply_due();
        let n = self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        if !n.gate() || !self.release_times[note.0.index].held {
            return Ok(false);
        }
        if let Some(stage) = self.release_times[note.0.index].release_stage {
            let plan = n.plan;
            let state = &mut self
                .plans
                .get_mut(plan.0)
                .unwrap()
                .projections
                .get_mut(note.0.index, stage)?
                .release;
            if *state != super::note_event::ReleaseStage::Suppressed {
                return Ok(false);
            }
            *state = super::note_event::ReleaseStage::Pending;
            return self.forward_release_stage(note, stage);
        }
        self.release_times[note.0.index].held = false;
        self.forward_release_groups(note)?;
        self.finish_key_release(note, ReleaseCause::Script);
        Ok(true)
    }

    /// Valid until the logical note retires, including rejected terminal delivery.
    pub fn release_context(&self, id: NoteId) -> Result<ReleaseContext, Error> {
        let note = self.notes.get(id.0).ok_or(Error::StaleHandle)?;
        let times = &self.release_times[id.0.index];
        Ok(ReleaseContext {
            admitted_at: times.admitted_at,
            key: note.key_release.map(|cause| KeyRelease {
                at: times.key_at,
                velocity: times.velocity,
                cause,
            }),
            gate: note.gate_release.map(|cause| GateRelease {
                at: times.gate_at,
                cause,
            }),
        })
    }
}

/// Selection phase of a prepared region and its retained take decision.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Trigger {
    #[default]
    Attack,
    KeyRelease,
    GateRelease,
}

impl Trigger {
    pub(super) fn release_index(self) -> Option<usize> {
        match self {
            Self::Attack => None,
            Self::KeyRelease => Some(0),
            Self::GateRelease => Some(1),
        }
    }
}

/// Velocity used for both release-region eligibility and source gain.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum ReleaseVelocity {
    #[default]
    Onset,
    KeyUp {
        fallback: f64,
    },
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ReleaseOptions {
    pub velocity: ReleaseVelocity,
    /// Frames until the family's own envelope/loop release. None requires finite,
    /// sources (including counted loops). Zero releases immediately using each source's envelope.
    pub duration: Option<u32>,
}

/// Note-owned phase outcome, retained through source EOF and terminal retry.
/// Selected includes valid zero-source selections; it does not imply active audio.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReleaseStatus {
    #[default]
    Unarmed,
    Pending,
    Selected,
    Suppressed,
    Failed(Error),
}

/// Outstanding capacity owned by pending release phases, separate from live counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReleaseReserve {
    pub voices: usize,
    pub families: usize,
    pub decisions: usize,
    pub commands: usize,
}

impl ReleaseReserve {
    pub(super) fn plus(self, other: Self) -> Self {
        Self {
            voices: self.voices + other.voices,
            families: self.families + other.families,
            decisions: self.decisions + other.decisions,
            commands: self.commands + other.commands,
        }
    }
    pub(super) fn maximum(self, other: Self) -> Self {
        Self {
            voices: self.voices.max(other.voices),
            families: self.families.max(other.families),
            decisions: self.decisions.max(other.decisions),
            commands: self.commands.max(other.commands),
        }
    }
}

impl ReleaseCause {
    pub(super) fn triggers_key_release(self) -> bool {
        matches!(self, Self::KeyUp | Self::Script | Self::AllNotesOff)
    }
    pub(super) fn musical(self) -> bool {
        matches!(
            self,
            Self::KeyUp
                | Self::Script
                | Self::Pedal
                | Self::Explicit
                | Self::Parent
                | Self::AllNotesOff
        )
    }
}

impl Runtime {
    pub fn release_reserve(&self) -> ReleaseReserve {
        ReleaseReserve {
            voices: self.voices.reserved,
            families: self.families.reserved,
            decisions: self.decisions.reserved,
            commands: self.reserved_commands,
        }
    }

    /// Let an attack that only outstanding release reservations keep from
    /// fitting suppress the pending release phases of the oldest notes (their
    /// release samples will not sound) instead of failing `Capacity`. Off by
    /// default: reservations are then strictly owned.
    pub fn set_release_stealing(&mut self, on: bool) {
        self.steal_releases = on;
    }

    pub fn release_status(&self, note: NoteId, trigger: Trigger) -> Result<ReleaseStatus, Error> {
        self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        let index = trigger.release_index().ok_or(Error::InvalidInput)?;
        Ok(self.release_times[note.0.index].selection[index])
    }

    pub(super) fn reserve_release(&mut self, reserve: ReleaseReserve) {
        self.voices.reserve(reserve.voices);
        self.families.reserve(reserve.families);
        self.decisions.reserve(reserve.decisions);
        assert!(reserve.commands <= self.available_commands());
        self.reserved_commands += reserve.commands;
    }

    pub(super) fn unreserve_release(&mut self, reserve: ReleaseReserve) {
        self.voices.unreserve(reserve.voices);
        self.families.unreserve(reserve.families);
        self.decisions.unreserve(reserve.decisions);
        assert!(reserve.commands <= self.reserved_commands);
        self.reserved_commands -= reserve.commands;
    }
}

impl Runtime {
    pub(super) fn reserve_release_callbacks(&mut self, note: NoteId, entry: usize) {
        let plan = self.notes.get(note.0).unwrap().plan;
        let generation = self.plans.get_mut(plan.0).unwrap();
        let mut count = 0;
        for (stage, binding) in generation.prepared.stages.iter().enumerate().skip(entry) {
            if binding.release.is_some() {
                generation
                    .projections
                    .get_mut(note.0.index, stage)
                    .unwrap()
                    .release_reserved = true;
                count += 1;
            }
        }
        self.note_events[note.0.index].release_start = entry;
        self.note_events[note.0.index].release_routed = true;
        self.note_events[note.0.index].pending_releases = count;
        self.behaviors.reserve(count);
    }

    /// Return unreached quotas after consumption, or every remaining quota at closure.
    pub(super) fn trim_release_callbacks(&mut self, note: NoteId, all: bool) {
        use super::note_event::ReleaseStage;
        let plan = self.notes.get(note.0).unwrap().plan;
        let generation = self.plans.get_mut(plan.0).unwrap();
        let mut count = 0;
        for stage in 0..=generation.prepared.stages.len() {
            let view = generation.projections.get_mut(note.0.index, stage).unwrap();
            if view.release_reserved && (all || view.properties.is_none()) {
                view.release_reserved = false;
                count += 1;
            }
            if all && view.properties.is_some() {
                view.release = ReleaseStage::Forwarded;
            }
        }
        self.note_events[note.0.index].pending_releases -= count;
        self.behaviors.unreserve(count);
    }

    pub(super) fn run_release_behavior(&mut self, note: NoteId, musical: bool) {
        if !musical || !self.note_events[note.0.index].release_routed {
            self.trim_release_callbacks(note, true);
            return;
        }
        let entry = self.note_events[note.0.index].release_start;
        self.advance_release_stage(note, entry);
    }

    pub(super) fn queue_note_release(&mut self, note: NoteId, stage: Option<usize>, at: usize) {
        assert!(
            !self.note_events[note.0.index].release_queued,
            "one native release frame per note"
        );
        assert!(
            self.behavior_ready.len() < self.behavior_ready.capacity(),
            "reserved native release capacity"
        );
        self.note_events[note.0.index].release_queued = true;
        let n = self.notes.get_mut(note.0).unwrap();
        n.work = n.work.checked_add(1).expect("bounded native release pin");
        self.behavior_ready
            .insert(at, super::behavior::Ready::Release { note, stage });
    }

    pub(super) fn advance_release_stage(&mut self, note: NoteId, from: usize) {
        use super::{behavior::NoteStage, note_event::ReleaseStage};
        let n = self.notes.get(note.0).unwrap();
        let plan = n.plan;
        let first_child = n.first_child;
        let end = self.plans.get(plan.0).unwrap().prepared.stages.len();
        for stage in from..=end {
            let generation = self.plans.get_mut(plan.0).unwrap();
            let view = generation.projections.get_mut(note.0.index, stage).unwrap();
            if view.properties.is_none() {
                break;
            }
            let callback = if view.release_reserved {
                view.release_reserved = false;
                view.release = ReleaseStage::Pending;
                let program = generation.prepared.stages[stage].release.unwrap();
                generation.groups.begin_release(note.0.index, stage);
                self.note_events[note.0.index].pending_releases -= 1;
                self.behaviors.unreserve(1);
                self.release_times[note.0.index].release_stage = Some(stage);
                Some(
                    self.admit_note_context(note, program, Some(NoteStage::Release(stage)))
                        .expect("owned reached-stage release callback"),
                )
            } else {
                view.release = ReleaseStage::Forwarded;
                None
            };
            if let Some(id) = callback {
                self.queue_behavior(id);
            }
            let mut child = first_child;
            let mut queued = false;
            while let Some(index) = child {
                let state = self.notes.at_mut(index);
                child = state.siblings.next;
                if state.release_link == super::ReleaseLink::Stage(stage)
                    && state.key_down()
                    && state.gate()
                {
                    if !queued && callback.is_none() {
                        self.queue_note_release(note, Some(stage + 1), self.behavior_ready.len());
                    }
                    let child = NoteId(self.notes.id(index.get()));
                    self.queue_note_release(child, None, self.behavior_ready.len());
                    queued = true;
                }
            }
            if callback.is_some() || queued {
                self.drain_behavior();
                return;
            }
        }
        self.release_times[note.0.index].held = false;
        self.release_times[note.0.index].groups_forwarded = true;
        let cause = self.notes.get(note.0).unwrap().key_release.unwrap();
        self.finish_key_release(note, cause);
    }

    pub(super) fn forward_release_stage(
        &mut self,
        note: NoteId,
        stage: usize,
    ) -> Result<bool, Error> {
        use super::note_event::ReleaseStage;
        let n = self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        let generation = self.plans.get_mut(n.plan.0).unwrap();
        let view = generation.projections.get_mut(note.0.index, stage)?;
        if view.release != ReleaseStage::Pending {
            return Ok(false);
        }
        view.release = ReleaseStage::Forwarded;
        generation.groups.commit_release(note.0.index, stage);
        self.advance_release_stage(note, stage + 1);
        Ok(true)
    }

    pub(super) fn suppress_release_stage(
        &mut self,
        note: NoteId,
        stage: usize,
    ) -> Result<bool, Error> {
        use super::note_event::ReleaseStage;
        let n = self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        let view = self
            .plans
            .get_mut(n.plan.0)
            .unwrap()
            .projections
            .get_mut(note.0.index, stage)?;
        if view.release != ReleaseStage::Pending {
            return Ok(false);
        }
        view.release = ReleaseStage::Suppressed;
        self.release_times[note.0.index].held = true;
        Ok(true)
    }
}
