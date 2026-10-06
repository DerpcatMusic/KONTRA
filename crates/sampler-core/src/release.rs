//! Note-owned release context survives source completion and prepared-plan changes.
use super::{Error, Note, NoteId, Runtime};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReleaseCause {
    KeyUp,
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
    pub release_behavior: bool,
}

pub(super) fn validate_velocity(velocity: Option<f64>) -> Result<(), Error> {
    if velocity.is_some_and(|v| !v.is_finite() || !(0.0..=1.0).contains(&v)) {
        Err(Error::InvalidInput)
    } else {
        Ok(())
    }
}

impl Runtime {
    pub(super) fn release_key(&mut self, id: NoteId, cause: ReleaseCause, velocity: Option<f64>) {
        let note = self.notes.get_mut(id.0).unwrap();
        if note.key_down() {
            let times = &mut self.release_times[id.0.index];
            times.key_at = self.now;
            times.velocity = velocity;
            note.key_release = Some(cause);
            self.run_release_behavior(
                id,
                matches!(cause, ReleaseCause::KeyUp | ReleaseCause::AllNotesOff),
            );
            if !self.release_times[id.0.index].held {
                self.run_release(
                    id,
                    Trigger::KeyRelease,
                    matches!(cause, ReleaseCause::KeyUp | ReleaseCause::AllNotesOff),
                );
            }
        }
    }

    /// Stop a pending release before its script forwarding boundary. Physical key
    /// state/velocity stay recorded; pedal release cannot bypass this hold.
    pub fn suppress_release(&mut self, note: NoteId) -> Result<bool, Error> {
        self.apply_due();
        let n = self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        if n.key_down() {
            return Err(Error::InvalidInput);
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
        self.release_times[note.0.index].held = false;
        self.forward_release_groups(note)?;
        self.run_release(note, Trigger::KeyRelease, true);
        self.key_up_now(note, None)?;
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
    pub(super) fn musical(self) -> bool {
        matches!(
            self,
            Self::KeyUp | Self::Pedal | Self::Explicit | Self::Parent | Self::AllNotesOff
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
