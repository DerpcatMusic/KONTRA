//! Drive: nonlinear colour (03-DSP.md §3). [`Shaper`] is the one waveshaper
//! (a curve between optional tone filters, run at twice the rate); [`Crush`]
//! is the one sample-rate and bit-depth reducer. Formats load their knob laws
//! into these; a named unit is a law, not a class.

mod amp_stack;
mod cabinet;
mod clipper;
mod color_sat;
mod converter;
mod crush;
mod overdrive;
mod shaper;
mod tube;

pub use amp_stack::{AmpStack, AmpStackSettings, AmpVoicing, GEQ_BANDS_HZ};
pub use cabinet::{Cabinet, CabinetSettings};
pub use clipper::{Circuit, Clipper};
pub use color_sat::{ColorSat, ColorSatSettings};
pub use converter::Converter;
pub use crush::{Crush, Reduction};
pub use kontra_dsp_core::{Section, Sections, Stage};
pub use overdrive::{Overdrive, Voicing};
pub use shaper::{Core, Curve, Outer, Shaper, Tone};
