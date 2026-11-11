//! A reverb's controls as a script moves them. The physical value of each
//! control comes from a [`Law`] the translator supplies, so the engine only
//! ever sees physical units.

pub use kontra_law::{CombNetwork, HadamardNetwork, Law, NETWORK_LINES, ReverbNetwork};

/// A reverb's structure as the translator's law gives it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Network {
    /// Prime-length lines through a feedback matrix.
    Lines(&'static ReverbNetwork),
    /// Damped combs into allpasses.
    Combs(&'static CombNetwork),
    /// Geometric lines through a Hadamard transform.
    Hadamard(HadamardNetwork),
}

pub use kontra_params::{ReverbControls, ReverbPar};

impl crate::Reverb {
    /// Reads the physical value of a control.
    pub const fn control(&self, par: ReverbPar) -> f32 {
        match par {
            ReverbPar::Seconds => self.seconds,
            ReverbPar::Size => self.size,
            ReverbPar::Damping => self.damping,
            ReverbPar::Diffusion => self.diffusion,
            ReverbPar::Predelay => self.predelay_ms,
            ReverbPar::HighCut => self.high_cut_hz,
            ReverbPar::LowShelf => self.low_shelf_db,
            ReverbPar::Stereo => self.stereo,
        }
    }

    /// Sets a control to its physical value.
    pub fn set_control(&mut self, par: ReverbPar, value: f32) {
        match par {
            ReverbPar::Seconds => self.seconds = value,
            ReverbPar::Size => self.size = value,
            ReverbPar::Damping => self.damping = value,
            ReverbPar::Diffusion => self.diffusion = value,
            ReverbPar::Predelay => self.predelay_ms = value,
            ReverbPar::HighCut => self.high_cut_hz = value,
            ReverbPar::LowShelf => self.low_shelf_db = value,
            ReverbPar::Stereo => self.stereo = value,
        }
    }
}
