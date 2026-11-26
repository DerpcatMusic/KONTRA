//! Spatial and modulation units on stereo audio (03-DSP.md §5).

mod ringmod;
mod rotator;

pub use ringmod::{Ring, RingMod, Source as RingSource};
pub use rotator::{Rotary, RotarySettings};
