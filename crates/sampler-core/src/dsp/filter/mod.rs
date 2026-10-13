//! Universal `filter` DSP (wave 4): ONE filter engine for the group/bus filter family.
//!
//! [`Settings`] is the engine's configuration and [`Engine`] its prepared form: one processor
//! variant, one native-knob layer (stored 0..=1 knobs plus knob-domain modulation, the way
//! Kontakt modulates its filters), one state store (`cells`) and one modulation slot. The core
//! that runs the audio is chosen at prepare time and dispatched once per block, so every core
//! is monomorphic in its inner loop:
//!
//! - [`cascade`]: first- and second-order direct-form sections whose coefficients a native law
//!   derives from the knobs, chained or parallel, with an optional in-loop knee clip and
//!   output gain. Kontakt's legacy BFilter classes are its configurations.
//! - [`svf`]: the zero-delay state-variable section cascade with hertz/Q laws (generic and
//!   format filters whose laws are hertz and Q, SV family, UVI biquad shapes); it lives in the
//!   voice/bus `FilterBank` because it batches across voices.
//! - [`ladder`], [`ar`], [`daft`]: structurally different native recurrences (a four-stage
//!   ladder with global feedback and per-stage saturation; the adaptive-resonance detector loop;
//!   a 2x saturating SVF). No section configuration expresses their feedback nonlinearities, so
//!   they are cores of the engine, each checked against native vectors in its own module.
//! - [`formant`]: the parallel band-pass bank of Formant I/II.
pub(crate) mod ar;
pub(crate) mod ar_kernel;
pub(crate) mod cascade;
pub(crate) mod daft;
pub(crate) mod formant;
pub(crate) mod ladder;
pub(crate) mod ladder_kernel;
pub(crate) mod svf;

use super::control::{ControlRamp, ControlRange, Parameter, PreparedParameter};
use super::{Planar, ProcessorState};

/// The engine's configuration: which core and its native knobs.
#[derive(Clone, Copy, Debug)]
pub enum Settings {
    Cascade(cascade::CascadeSettings),
    Ladder(ladder::LadderSettings),
    Ar(ar::ArSettings),
    Daft(daft::DaftSettings),
}

/// A knob of the engine a control can bind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Knob {
    Cutoff,
    Resonance,
    Gain,
    Response,
}

impl Settings {
    pub(crate) fn valid(&self) -> bool {
        match self {
            Self::Cascade(s) => s.valid(),
            Self::Ladder(s) => s.valid(),
            Self::Ar(s) => s.valid(),
            Self::Daft(s) => s.valid(),
        }
    }

    /// Bind `knob` to `parameter`; false when this core has no such knob.
    pub(crate) fn bind(&mut self, knob: Knob, parameter: Parameter) -> bool {
        let slot = match (self, knob) {
            (Self::Cascade(s), Knob::Cutoff) => &mut s.cutoff,
            (Self::Cascade(s), Knob::Resonance) => &mut s.resonance,
            (Self::Ladder(s), Knob::Cutoff) => &mut s.cutoff,
            (Self::Ladder(s), Knob::Resonance) => &mut s.resonance,
            (Self::Ladder(s), Knob::Gain) => &mut s.gain,
            (Self::Ar(s), Knob::Cutoff) => &mut s.cutoff,
            (Self::Ar(s), Knob::Resonance) => &mut s.resonance,
            (Self::Daft(s), Knob::Cutoff) => &mut s.cutoff,
            (Self::Daft(s), Knob::Resonance) => &mut s.resonance,
            (Self::Daft(s), Knob::Gain) => &mut s.gain,
            (Self::Daft(s), Knob::Response) => &mut s.response,
            _ => return false,
        };
        *slot = parameter;
        true
    }

    /// `[f64; 2]` state cells the prepared engine owns in the chain's delay storage.
    pub(crate) fn cells(&self) -> usize {
        match self {
            Self::Cascade(_) => cascade::CELLS,
            Self::Ladder(_) => ladder::CELLS,
            Self::Ar(_) => ar::CELLS,
            Self::Daft(_) => 0,
        }
    }

    pub(crate) fn compile(self, rate: u32, bindings: &mut Vec<ControlRange>) -> Engine {
        let core = match self {
            Self::Cascade(s) => Core::Cascade(s.compile(rate, bindings)),
            Self::Ladder(s) => Core::Ladder(s.compile(rate, bindings)),
            Self::Ar(s) => Core::Ar(s.compile(rate, bindings)),
            Self::Daft(s) => Core::Daft(s.compile(rate, bindings)),
        };
        Engine { core, modulation_index: usize::MAX }
    }
}

enum Core {
    Cascade(cascade::Cascade),
    Ladder(ladder::Ladder),
    Ar(ar::Ar),
    Daft(daft::Daft),
}

/// The prepared engine.
pub(crate) struct Engine {
    core: Core,
    /// This instance's slot in the voice's native-knob modulation table.
    pub(crate) modulation_index: usize,
}

impl Engine {
    pub(crate) fn cells(&self) -> usize {
        match &self.core {
            Core::Cascade(_) => cascade::CELLS,
            Core::Ladder(_) => ladder::CELLS,
            Core::Ar(_) => ar::CELLS,
            Core::Daft(_) => 0,
        }
    }

    /// The trace name of the core and its lanes.
    pub(crate) fn trace(&self) -> (&'static str, Vec<(&'static str, PreparedParameter)>) {
        match &self.core {
            Core::Cascade(c) => ("cascade", c.trace_parameters().to_vec()),
            Core::Ladder(c) => ("ladder_lp4", c.trace_parameters().to_vec()),
            Core::Ar(c) => ("ar", c.trace_parameters().to_vec()),
            Core::Daft(c) => ("daft", c.trace_parameters().to_vec()),
        }
    }

    /// Filter `len` frames; `modulation` holds the knob-domain deltas. True on a fault.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn process(
        &self,
        state: &mut ProcessorState,
        cells: &mut [[f64; 2]],
        parameters: &[ControlRamp],
        block: &mut Planar,
        len: usize,
        at: u64,
        modulation: [f64; 4],
    ) -> bool {
        match &self.core {
            Core::Cascade(c) => c.process(cells, parameters, block, len, at, modulation),
            Core::Ladder(c) => c.process(state, cells, parameters, block, len, at, modulation),
            Core::Ar(c) => c.process(state, cells, parameters, block, len, at, modulation),
            Core::Daft(c) => {
                c.process(state, parameters, block, len, at, modulation);
                false
            }
        }
    }
}
