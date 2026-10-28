//! Nonlinear colour in physical terms: a waveshaper and a reducer. Both run
//! on each voice when they sit in a group rack.

/// A transfer curve and its amount (the engine's `drive::Curve`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve {
    /// −1..=1: above 0 a soft clip, below 0 a cubic expansion.
    Soft(f32),
    /// 0..=1: an asymmetric rational clip.
    Tube(f32),
    /// 0..=1: a power law on small signals.
    Power(f32),
}

/// A curve, then an optional one-pole low-pass and a second-order
/// Butterworth high-pass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shaper {
    pub curve: Curve,
    pub lowpass_hz: Option<f32>,
    pub highpass_hz: Option<f32>,
}

/// A sample hold, a quantizer truncating toward zero, then noise.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crush {
    /// The hold: after each captured frame it holds `1/rate` more (at most
    /// a fiftieth of a second); 1 or more holds nothing. Routes move it
    /// linearly.
    pub rate: f32,
    /// `2^bits` steps per unit, truncating toward zero; 16 or more is off.
    pub bits: f32,
    /// Noise level, linear.
    pub noise: f32,
    /// The noise's one-pole smoothing per frame, 0..=1 (1 white).
    pub noise_tone: f32,
}
