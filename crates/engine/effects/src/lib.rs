//! The effect processors racks run: an IR effect slot as a stereo matrix
//! (matrix), as summed EQ and filter stages (summed), or as a processor
//! with its settings in DSP terms (processors); convolution responses
//! decoded off the audio thread (response).

mod ensemble;
pub mod matrix;
/// Standalone, allocation-free Kontakt L4 DSP cores (`EnhancedCompressor`,
/// `SolidBusComp`, `SolidPassFilters`, `StereoModeller`). Rack/translator
/// integration for `StereoModeller` pseudo-stereo lands with the Wave 2A
/// `Module` table; presets continue emitting the diagnostic until wired.
pub mod measured_dsp;
pub mod processors;
mod response;
pub mod summed;

pub use processors::stem;
pub use response::{Response, response};

/// Frames rendered per inner control/audio span (~1.33 ms / 750 Hz at 48 kHz,
/// 4× finer than 256 frames); sizes the bus buffers.
pub const MAX_SPAN: u64 = 64;
