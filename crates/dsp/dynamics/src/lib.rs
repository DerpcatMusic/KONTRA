//! One compressor, one peak limiter and one transient shaper; design in
//! docs/architecture-v3/03-DSP.md. Formats load their laws into these
//! classes.

mod compressor;
mod limiter;
mod transient;

pub use compressor::{Compressor, Detector, Settings as CompressorSettings};
pub use limiter::{Limiter, Settings as LimiterSettings};
pub use transient::{Settings as TransientSettings, Transient};

/// `db` as a linear gain.
fn gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
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
