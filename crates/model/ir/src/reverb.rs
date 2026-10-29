//! A reverb's controls as a script moves them. The physical value of each
//! control comes from a [`Law`] the translator supplies, so the engine only
//! ever sees physical units.

pub use kontra_law::{CombNetwork, Law, NETWORK_LINES, ReverbNetwork};

/// A reverb's structure as the translator's law gives it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Network {
    /// Prime-length lines through a feedback matrix.
    Lines(&'static ReverbNetwork),
    /// Damped combs into allpasses.
    Combs(&'static CombNetwork),
}

use crate::Reverb;

/// The controls a script can move while the reverb plays, in [`ReverbPar::ALL`] order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReverbPar {
    Seconds,
    Size,
    Damping,
    Diffusion,
    Predelay,
    HighCut,
    LowShelf,
    Stereo,
}

impl ReverbPar {
    pub const ALL: [Self; 8] = [
        Self::Seconds,
        Self::Size,
        Self::Damping,
        Self::Diffusion,
        Self::Predelay,
        Self::HighCut,
        Self::LowShelf,
        Self::Stereo,
    ];

    /// Sets this control to the physical `value`.
    pub fn set(self, r: &mut Reverb, value: f32) {
        match self {
            Self::Seconds => r.seconds = value,
            Self::Size => r.size = value,
            Self::Damping => r.damping = value,
            Self::Diffusion => r.diffusion = value,
            Self::Predelay => r.predelay_ms = value,
            Self::HighCut => r.high_cut_hz = value,
            Self::LowShelf => r.low_shelf_db = value,
            Self::Stereo => r.stereo = value,
        }
    }
}
