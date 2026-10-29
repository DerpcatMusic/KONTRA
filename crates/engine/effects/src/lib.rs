//! The effect processors racks run: an IR effect slot as a stereo matrix
//! (matrix), as summed EQ and filter stages (summed), or as a processor
//! with its settings in DSP terms (processors); convolution responses
//! decoded off the audio thread (response).

pub mod matrix;
pub mod processors;
mod response;
pub mod summed;

pub use processors::stem;
pub use response::{Response, response};

/// Frames rendered per inner span; sizes the bus buffers.
pub const MAX_SPAN: u64 = 256;
