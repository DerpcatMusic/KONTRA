//! The dynamics and time effects' settings in physical units.

use crate::{Network, Rate};

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
    /// The input past the threshold triggers a hold that the gain follows
    /// to a fixed depth, `threshold·(1 − 1/ratio)` dB, by the attack; the
    /// hold falls by the release while nothing retriggers it. Linked, the
    /// channels' mean triggers both. No knee.
    Trigger,
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

/// A 2×2 gain matrix, row `j` the gains of channels 0 and 1 into channel `j`.
pub type Matrix = [[f32; 2]; 2];

/// Each channel to itself.
pub const IDENTITY: Matrix = [[1.0, 0.0], [0.0, 1.0]];

/// A modulated stereo delay in physical units: two lines, one or two heads
/// an LFO sweeps on each, the heads filtered, then mixed back into the lines
/// and out by matrices.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Delay {
    /// The head's delay at the LFO's centre.
    pub time: DelayTime,
    /// The LFO's swing either side of `time`.
    pub depth_ms: f32,
    /// A second head at half the delay, its LFO slightly ahead.
    pub second_head: bool,
    /// The input channels into the lines.
    pub input: Matrix,
    /// The heads back into the lines.
    pub feedback: Matrix,
    /// The heads to the output.
    pub output: Matrix,
    /// The heads' one-pole low-pass corner; `f32::INFINITY` for none.
    pub damping_hz: f32,
    /// The heads' one-pole high-pass corner; 0 for none.
    pub low_cut_hz: f32,
    pub rate: Rate,
    /// Cycles the right LFO runs ahead of the left.
    pub phase: f32,
}

/// A phaser in physical units: `stages` first-order allpasses whose corner
/// the LFO sweeps, `hz + spread_hz·(e^(exponent·lfo) − 1)`, around a
/// feedback loop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Phaser {
    /// 1..=24.
    pub stages: u8,
    /// A triangle LFO instead of a sine (both fall first).
    pub triangle: bool,
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
    /// Delay-line scale, 1 a mid-size room; a network's own size control.
    pub size: f32,
    /// The translator's network; `None` runs the generic eight-line one.
    pub network: Option<Network>,
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

/// Low and high cut Hz of a response's early and late parts, and where (a
/// fraction of its length) the late part takes over; `None`: all late.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImpulseCuts {
    pub early: [f32; 2],
    pub late: [f32; 2],
    pub crossover: Option<f32>,
}

/// A filter whose 0..=1 cutoff and resonance knobs follow a control clock:
/// every 32 frames the cutoff steps at most `1/(31·resonance + 4)` (within
/// 1/35..1/4) toward its knob and the resonance 0.015 of the way.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clocked {
    pub shape: ClockedShape,
    pub cutoff: f32,
    pub resonance: f32,
}

/// What a [`Clocked`] filter runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockedShape {
    /// Four first-order allpasses around up to 0.85 of feedback, mixed half
    /// and half with the input.
    Allpass,
    /// Two formant sections in parallel, the low one at unit gain at DC.
    VowelLow,
    /// As [`Self::VowelLow`], mirrored: unit gain at Nyquist.
    VowelHigh,
}

/// Three resonant band-passes in parallel placed by a vowel table: `talk`
/// sweeps the vowels, `size` scales the centres, `sharp` narrows and lifts
/// the bands (each 0..=1); `set` picks the vowels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Formant {
    pub talk: f32,
    pub sharp: f32,
    pub size: f32,
    pub set: VowelSet,
}

/// Which five vowels of the table `talk` sweeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VowelSet {
    First,
    Second,
}

/// Three explicit state-variable stages in series, each morphing from low
/// through band to high pass and blended into the signal by its wet amount.
/// The output is normalised by the stages' resonant peaks, then scaled by
/// `gain`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cascade {
    /// Stage 0's corner; stage n + 1 sits `shifts[n]` octaves above stage n.
    pub hz: f32,
    pub shifts: [f32; 2],
    pub stages: [CascadeStage; 3],
    /// Linear, after the normalisation.
    pub gain: f32,
}

/// One stage of a [`Cascade`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CascadeStage {
    /// The resonance: the stage damps by `2 − 2q`.
    pub q: f32,
    /// The highest integrator gain (`2π·hz/rate`) the stage runs at.
    pub limit: f32,
    /// The stage's resonant peak at full wet, which the normaliser divides out.
    pub peak: f32,
    /// 0 low pass, 0.5 band pass, 1 high pass; between them a crossfade.
    pub morph: f32,
    /// How much of the stage's output replaces its input.
    pub wet: f32,
}
