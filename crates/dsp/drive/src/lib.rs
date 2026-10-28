//! Drive: nonlinear colour (03-DSP.md §3). [`Shaper`] is the one waveshaper
//! (a curve between optional tone filters, run at twice the rate); [`Crush`]
//! is the one sample-rate and bit-depth reducer. Formats load their knob laws
//! into these; a named unit is a law, not a class.

mod crush;
mod overdrive;
mod shaper;

pub use crush::{Crush, Reduction};
pub use overdrive::{Overdrive, Voicing};
pub use shaper::{Curve, Shaper, Tone};
