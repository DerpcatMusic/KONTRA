//! Sample clock, frame counts, tempo and beat↔sample conversion.
//!
//! Tempo is quarter notes per minute and conversion truncates toward zero,
//! matching `sampler-core` (`beats * 60 / bpm * rate`, cast to an integer).

mod sample;
mod transport;

pub use sample::{Frames, SampleTime};
pub use transport::{Tempo, TimeError, Transport};
