//! Event model: host notes, host patterns, note expression and the Universal
//! MIDI Packet event, plus the receiver wire value types. Plain data only; no
//! KONTRA crate is a dependency.

mod event;
mod midi2;
mod wire;

pub use event::*;
pub use midi2::PER_NOTE_BEND_SEMITONES;
pub use wire::*;
