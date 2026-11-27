//! One compressor, one peak limiter and one transient shaper; design in
//! docs/architecture-v3/03-DSP.md. Formats load their laws into these
//! classes.

mod compressor;
mod follower;
mod limiter;
mod transient;

pub use compressor::{Compressor, Detector, Settings as CompressorSettings, bus_coeff};
pub use follower::{EnvelopeFollower, Settings as EnvelopeFollowerSettings};
pub use limiter::{Limiter, Settings as LimiterSettings};
pub use transient::{Settings as TransientSettings, Transient};

/// Unified static-dispatch dynamics processor covering compressors (all 5
/// detector/ballistic laws), peak/lookahead limiters, and transient shapers.
pub enum Dynamics {
    Compressor(Box<Compressor>, u32),
    Limiter(Box<Limiter>, u32),
    Transient(Box<Transient>),
}

impl Dynamics {
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        match self {
            Self::Compressor(c, _) => c.process(io),
            Self::Limiter(l, _) => l.process(io),
            Self::Transient(t) => t.process(io),
        }
    }

    pub fn process_sidechain(&mut self, io: &mut [[f32; 2]], key: &[[f32; 2]]) {
        match self {
            Self::Compressor(c, _) => c.process_sidechain(io, key),
            Self::Limiter(l, _) => l.process(io),
            Self::Transient(t) => t.process(io),
        }
    }
}

/// `db` as a linear gain.
#[inline]
fn gain(db: f32) -> f32 {
    kontra_dsp_core::math::db_to_gain_f32(db)
}

/// One-pole coefficient that covers `1 - 1/e` of a step in `ms`.
fn pole(ms: f32, rate: f32) -> f32 {
    let frames = ms.max(0.0) * 0.001 * rate;
    if frames < 1.0 {
        1.0
    } else {
        1.0 - (-1.0 / frames).exp()
    }
}
