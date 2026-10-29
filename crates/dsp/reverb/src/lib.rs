//! Zero-latency stereo convolver, its response shaping, the generic
//! algorithmic reverb and the network a translator describes; design in
//! docs/architecture-v3/03-DSP.md.

mod combs;
mod convolver;
mod fade;
mod fdn;
mod network;
mod resample;
pub mod shape;

pub use combs::Combs;
pub use convolver::Convolver;
pub use fdn::{Fdn, Settings};
pub use network::Network;
