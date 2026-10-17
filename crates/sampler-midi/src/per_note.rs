//! The prepared typed PN bank is shared with Core receiver ownership.
pub(crate) use sampler_core::{ReceiverPerNoteState as PerNoteState,
    ReceiverKeyState as KeyState,ReceiverSnapshot as Snapshot};
pub(crate) const CENTER:u32=sampler_core::RECEIVER_PER_NOTE_CENTER;
