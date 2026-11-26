//! The script callback being run, preserved across its waits by typed
//! [`CallbackContext`].

use crate::NoteRef;

/// The script callback kind.
///
/// Every variant maps to the callback-type number scripts read via
/// [`Kind::callback_type`]. Wait resumption preserves the original [`Kind`] and
/// typed [`CallbackContext`] on the coroutine frame rather than degrading to a
/// synthetic resumed kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Init,
    PersistenceChanged,
    Note,
    Release,
    Controller,
    /// `on rpn` and `on nrpn`.
    Rpn,
    Nrpn,
    PolyAt,
    /// `on note_controller`: a per-note controller another script slot set.
    NoteController,
    /// `on midi_in`: any incoming MIDI message, before the engine acts on it.
    MidiIn,
    Listener,
    Pgs,
    UiControl,
    UiControls,
    UiUpdate,
    AsyncComplete,
}

impl Kind {
    /// The measured callback-type number scripts read (probes/callback_constants.lua).
    #[must_use]
    pub const fn callback_type(self) -> u8 {
        match self {
            Self::Note => 1,
            Self::Release => 2,
            Self::Controller => 3,
            // ponytail: unmeasured, the gap in the measured numbering (controller 3, init 5).
            Self::PolyAt => 4,
            Self::Init => 5,
            Self::UiControl => 6,
            // ponytail: unmeasured, the gap between ui_control (6) and pgs (8).
            Self::UiUpdate => 7,
            Self::Pgs => 8,
            Self::MidiIn => kontra_vm::midi::CB_MIDI_IN,
            Self::Rpn => 10,
            Self::Nrpn => 11,
            Self::Listener => 12,
            Self::AsyncComplete => 13,
            Self::PersistenceChanged => 14,
            Self::UiControls => 15,
            Self::NoteController => kontra_vm::midi::CB_NOTE_CONTROLLER,
        }
    }
}

/// Typed callback identity and payload preserved across `wait(...)` on the
/// coroutine frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallbackContext {
    Init,
    PersistenceChanged,
    Note {
        event: NoteRef,
    },
    Release {
        event: NoteRef,
    },
    Controller {
        cc: u8,
    },
    Rpn {
        address: u16,
        value: u16,
    },
    Nrpn {
        address: u16,
        value: u16,
    },
    PolyAt {
        key: u8,
    },
    NoteController {
        cc: u8,
        note: u8,
        value: i32,
    },
    /// `on midi_in`: `[status nibble (0x80..0xE0), channel (0..=63), data 1, data 2]`
    /// and its MIDI-queue event id (`1..=0xFFFF`, disjoint from `NoteRef` ids).
    MidiIn {
        bytes: [u8; 4],
        id: i32,
    },
    Listener {
        signal: u8,
    },
    Pgs,
    UiControl {
        id: i32,
    },
    UiControls {
        id: i32,
    },
    UiUpdate {
        id: i32,
    },
    AsyncComplete,
}

impl CallbackContext {
    /// The callback's [`Kind`].
    #[must_use]
    pub const fn kind(self) -> Kind {
        match self {
            Self::Init => Kind::Init,
            Self::PersistenceChanged => Kind::PersistenceChanged,
            Self::Note { .. } => Kind::Note,
            Self::Release { .. } => Kind::Release,
            Self::Controller { .. } => Kind::Controller,
            Self::Rpn { .. } => Kind::Rpn,
            Self::Nrpn { .. } => Kind::Nrpn,
            Self::PolyAt { .. } => Kind::PolyAt,
            Self::NoteController { .. } => Kind::NoteController,
            Self::MidiIn { .. } => Kind::MidiIn,
            Self::Listener { .. } => Kind::Listener,
            Self::Pgs => Kind::Pgs,
            Self::UiControl { .. } => Kind::UiControl,
            Self::UiControls { .. } => Kind::UiControls,
            Self::UiUpdate { .. } => Kind::UiUpdate,
            Self::AsyncComplete => Kind::AsyncComplete,
        }
    }

    /// The callback-type number for this context.
    #[must_use]
    pub const fn callback_type(self) -> u8 {
        self.kind().callback_type()
    }
}

/// Coroutine-frame metadata stored per active VM coroutine slot so `wait(...)`
/// resumes with the exact [`CallbackContext`] and callback ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallbackFrame {
    pub generation: u16,
    pub context: CallbackContext,
    pub callback_id: u32,
}

/// The callback currently executing on the engine and its immediate event
/// decisions.
#[derive(Clone, Copy, Debug)]
pub struct Running {
    pub context: Option<CallbackContext>,
    pub ignore_release: bool,
    pub ignore_controller: bool,
    /// The callback set the controller it runs for itself (or its key's
    /// polyphonic aftertouch): with `ignore_controller`, that value stands
    /// in for the one that arrived.
    pub set_own: bool,
    /// Unique per callback run (20-bit), kept across waits.
    pub callback_id: u32,
    /// The script slot running it.
    pub slot: u8,
}

/// Marks a non-note payload in the low bits of the VM `u64` context so
/// polyphonic-variable lookup (`take_poly`) never collides with note event IDs.
const NON_EVENT_BIT: u32 = 1 << 31;

impl Running {
    pub const IDLE: Self = Self {
        context: None,
        ignore_release: false,
        ignore_controller: false,
        set_own: false,
        callback_id: 0,
        slot: 0,
    };

    #[must_use]
    pub const fn new(context: CallbackContext) -> Self {
        Self {
            context: Some(context),
            ..Self::IDLE
        }
    }

    /// Reconstructs the running state when resuming a suspended coroutine frame.
    #[must_use]
    pub const fn from_frame(slot: u8, frame: CallbackFrame) -> Self {
        Self {
            context: Some(frame.context),
            callback_id: frame.callback_id,
            slot,
            ..Self::IDLE
        }
    }

    /// Extracts the 20-bit callback ID encoded in a VM `u64` context.
    #[must_use]
    pub const fn callback_id_of(vm_context: u64) -> u32 {
        ((vm_context >> 40) as u32) & 0xF_FFFF
    }

    #[must_use]
    pub const fn kind(&self) -> Option<Kind> {
        match self.context {
            Some(c) => Some(c.kind()),
            None => None,
        }
    }

    #[must_use]
    pub fn is(&self, kind: Kind) -> bool {
        self.kind() == Some(kind)
    }

    #[must_use]
    pub const fn callback_type(&self) -> u8 {
        match self.context {
            Some(c) => c.callback_type(),
            None => 0,
        }
    }

    #[must_use]
    pub const fn event(&self) -> Option<NoteRef> {
        match self.context {
            Some(CallbackContext::Note { event } | CallbackContext::Release { event }) => {
                Some(event)
            }
            _ => None,
        }
    }

    #[must_use]
    pub const fn controller(&self) -> u8 {
        match self.context {
            Some(
                CallbackContext::Controller { cc } | CallbackContext::NoteController { cc, .. },
            ) => cc,
            Some(CallbackContext::PolyAt { key }) => key,
            _ => 0,
        }
    }

    #[must_use]
    pub const fn ui(&self) -> i32 {
        match self.context {
            Some(
                CallbackContext::UiControl { id }
                | CallbackContext::UiControls { id }
                | CallbackContext::UiUpdate { id },
            ) => id,
            _ => 0,
        }
    }

    #[must_use]
    pub const fn signal(&self) -> u8 {
        match self.context {
            Some(CallbackContext::Listener { signal }) => signal,
            _ => 0,
        }
    }

    #[must_use]
    pub const fn midi(&self) -> [u8; 4] {
        match self.context {
            Some(CallbackContext::MidiIn { bytes, .. }) => bytes,
            _ => [0; 4],
        }
    }

    pub fn midi_mut(&mut self) -> Option<&mut [u8; 4]> {
        match &mut self.context {
            Some(CallbackContext::MidiIn { bytes, .. }) => Some(bytes),
            _ => None,
        }
    }

    /// The active MIDI-queue event id while `on midi_in` runs.
    #[must_use]
    pub const fn midi_id(&self) -> Option<i32> {
        match self.context {
            Some(CallbackContext::MidiIn { id, .. }) => Some(id),
            _ => None,
        }
    }

    #[must_use]
    pub const fn note_controller(&self) -> Option<(u8, u8, i32)> {
        match self.context {
            Some(CallbackContext::NoteController { cc, note, value }) => Some((cc, note, value)),
            _ => None,
        }
    }

    #[must_use]
    pub const fn rpn(&self) -> Option<(u16, u16)> {
        match self.context {
            Some(
                CallbackContext::Rpn { address, value } | CallbackContext::Nrpn { address, value },
            ) => Some((address, value)),
            _ => None,
        }
    }

    /// Compact `u64` tag passed to `kontra_vm` so polyphonic variable lookup
    /// (`take_poly`), `has_callback`, and `stop_wait` keep their zero-alloc
    /// `u64` ABI while full typed payload lives in [`CallbackFrame`].
    #[must_use]
    pub fn vm_context(&self) -> u64 {
        let low = match self.event() {
            Some(n) => n.event_id() as u32,
            None => (self.ui() as u32) | NON_EVENT_BIT,
        };
        u64::from(low)
            | u64::from(self.callback_type()) << 32
            | u64::from(self.callback_id & 0xF_FFFF) << 40
            | u64::from(self.signal() & 0xF) << 60
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persistence_changed_reports_cb_type_14() {
        let r = Running::new(CallbackContext::PersistenceChanged);
        assert_eq!(r.kind(), Some(Kind::PersistenceChanged));
        assert_eq!(r.callback_type(), 14);
    }

    #[test]
    fn every_callback_context_round_trips_through_callback_frame() {
        let note = NoteRef::from_event_id(0x0003_0002).unwrap();
        let cases = [
            CallbackContext::Init,
            CallbackContext::PersistenceChanged,
            CallbackContext::Note { event: note },
            CallbackContext::Release { event: note },
            CallbackContext::Controller { cc: 74 },
            CallbackContext::Rpn {
                address: 1,
                value: 8202,
            },
            CallbackContext::Nrpn {
                address: 133,
                value: 391,
            },
            CallbackContext::PolyAt { key: 61 },
            CallbackContext::NoteController {
                cc: 12,
                note: 64,
                value: -500_000,
            },
            CallbackContext::MidiIn {
                bytes: [0x90, 17, 60, 100],
                id: 7,
            },
            CallbackContext::Listener { signal: 3 },
            CallbackContext::Pgs,
            CallbackContext::UiControl { id: 77 },
            CallbackContext::UiControls { id: 78 },
            CallbackContext::UiUpdate { id: 79 },
            CallbackContext::AsyncComplete,
        ];
        for (i, ctx) in cases.into_iter().enumerate() {
            let mut run = Running::new(ctx);
            run.callback_id = (i as u32) + 1;
            run.slot = 2;
            let frame = CallbackFrame {
                generation: 9,
                context: ctx,
                callback_id: run.callback_id,
            };
            let resumed = Running::from_frame(run.slot, frame);
            assert_eq!(resumed.context, Some(ctx));
            assert_eq!(resumed.kind(), Some(ctx.kind()));
            assert_eq!(resumed.callback_type(), ctx.callback_type());
            assert_eq!(resumed.callback_id, run.callback_id);
            assert_eq!(Running::callback_id_of(run.vm_context()), run.callback_id);
        }
    }
}
