//! The dynamics and time effects' settings in physical units.

use crate::Rate;

/// A feed-forward compressor in physical units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Compressor {
    /// Peak level where gain reduction starts.
    pub threshold_db: f32,
    /// Input dB over the threshold per output dB; infinite limits.
    pub ratio: f32,
    /// Soft-knee width around the threshold; 0 is hard.
    pub knee_db: f32,
    /// Time constants of the gain reduction rising and falling.
    pub attack_ms: f32,
    pub release_ms: f32,
    /// Gain after compression.
    pub makeup_db: f32,
    /// Both channels follow the louder one.
    pub link: bool,
    /// 0..=1: the compressed share of the output.
    pub mix: f32,
}

/// A peak limiter: input gain, then nothing past the ceiling.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limiter {
    /// Gain before limiting.
    pub input_db: f32,
    /// The highest peak it lets out.
    pub ceiling_db: f32,
    /// Time for the held peak to halve.
    pub release_ms: f32,
}

/// A delay length: fixed, or in quarter-note beats at the host's tempo.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DelayTime {
    Ms(f32),
    Beats(f32),
}

/// A modulated delay line in physical units: one or two heads an LFO
/// sweeps, a low-pass on each, feedback into the same and the other channel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Delay {
    /// The head's delay at the LFO's centre.
    pub time: DelayTime,
    /// The LFO's swing either side of `time`.
    pub depth_ms: f32,
    /// A second head at half the delay, its LFO slightly ahead.
    pub second_head: bool,
    /// Gain of each input channel into its line.
    pub input: [f32; 2],
    pub feedback: f32,
    pub cross: f32,
    /// The heads' low-pass corner; `f32::INFINITY` for none.
    pub damping_hz: f32,
    pub rate: Rate,
    /// Cycles the right LFO runs ahead of the left.
    pub phase: f32,
}

/// A phaser in physical units: four allpasses whose corner the LFO sweeps,
/// `hz + spread_hz·(e^(exponent·lfo) − 1)`, around a feedback loop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Phaser {
    pub hz: f32,
    pub spread_hz: f32,
    pub exponent: f32,
    pub feedback: f32,
    pub rate: Rate,
    /// Cycles the right LFO runs ahead of the left.
    pub phase: f32,
}
