//! The dynamics and time effects' settings in physical units.

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
