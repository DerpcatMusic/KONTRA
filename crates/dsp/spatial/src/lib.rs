//! Spatial and modulation units on stereo audio (03-DSP.md §5).

mod ringmod;
mod rotator;
mod stereo_detune;

pub use ringmod::{Ring, RingMod, Source as RingSource};
pub use rotator::{Rotary, RotarySettings};
pub use stereo_detune::{StereoDetune, StereoDetuneSettings};
