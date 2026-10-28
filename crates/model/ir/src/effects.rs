//! The dynamics and time effects' settings in physical units.

use crate::Rate;

/// A feed-forward compressor in physical units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Compressor {
    /// Peak level at the knee's centre: where a hard knee starts reducing;
    /// a soft knee starts `knee_db / 2` below it.
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
    pub detector: Detector,
}

/// What a compressor's level detector reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detector {
    /// The input (feed-forward).
    Input,
    /// Its own output (feedback).
    Output,
}

/// A transient shaper: a gain in dB from its attack and sustain envelopes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transient {
    /// dB per unit of the attack envelope; negative softens onsets.
    pub attack: f32,
    /// dB per unit of the sustain envelope; negative shortens tails.
    pub sustain: f32,
    pub gain_db: f32,
    /// Clip the output just inside full scale.
    pub clip: bool,
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

/// An algorithmic reverb in physical units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reverb {
    /// Time for the tail to fall 60 dB.
    pub seconds: f32,
    /// Delay-line scale, 1 a mid-size room.
    pub size: f32,
    /// 0..=1: how much faster the highs die.
    pub damping: f32,
    /// 0..=1: the tail's early density.
    pub diffusion: f32,
    pub predelay_ms: f32,
    pub high_cut_hz: f32,
    /// Gain below 200 Hz.
    pub low_shelf_db: f32,
    /// 0 mono, 1 full width.
    pub stereo: f32,
    /// How far the lines' lengths swing, and how fast.
    pub modulation_ms: f32,
    pub modulation_hz: f32,
}
