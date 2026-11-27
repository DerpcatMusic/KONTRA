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
mod stage;

pub use amp_stack::{AmpStack, AmpStackSettings, AmpVoicing, GEQ_BANDS_HZ};
pub use cabinet::{Cabinet, CabinetSettings};
pub use clipper::{Circuit, Clipper};
pub use color_sat::{ColorSat, ColorSatSettings};
pub use converter::Converter;
pub use crush::{Crush, Reduction};
pub use kontra_dsp_core::{Section, Sections, Stage};
pub use overdrive::{Overdrive, Voicing};
pub use shaper::{Core, Curve, Outer, Shaper, Tone};
pub use stage::{DcBlocker, ToneStack3};

/// Unified static-dispatch nonlinear drive family covering waveshapers,
/// overdrives, diode clippers, amp stacks, harmonic saturators, speaker
/// cabinets, and bit/rate reducers.
pub enum Drive {
    Shaper(Box<Shaper>, u32),
    Overdrive(Box<Overdrive>, u32),
    Clipper(Box<Clipper>),
    AmpStack(Box<AmpStack>),
    ColorSat(Box<ColorSat>),
    Cabinet(Box<Cabinet>),
    Crush(Box<Crush>, u32),
}

impl Drive {
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        match self {
            Self::Shaper(u, _) => u.process(io),
            Self::Overdrive(u, _) => u.process(io),
            Self::Clipper(u) => u.process(io),
            Self::AmpStack(u) => u.process(io),
            Self::ColorSat(u) => u.process(io),
            Self::Cabinet(u) => u.process(io),
            Self::Crush(u, _) => u.process(io),
        }
    }
}

kontra_dsp_core::impl_dsp_core!(
    Shaper, Overdrive, Clipper, AmpStack, ColorSat, Cabinet, Crush
);
