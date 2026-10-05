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
    pub admitted_at: u64,
    pub key_at: u64,
    pub gate_at: u64,
    pub velocity: Option<f64>,
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
        }
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
