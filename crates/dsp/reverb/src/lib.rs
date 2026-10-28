//! Zero-latency stereo convolver and one algorithmic reverb; design in
//! docs/architecture-v3/03-DSP.md.

mod convolver;
mod fdn;

pub use convolver::Convolver;
pub use fdn::{Fdn, Settings};
