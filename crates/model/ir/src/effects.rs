//! The dynamics and time effects' settings in physical units.

use crate::{Network, Rate, Section};

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
    /// Gain after compression, before [`Self::makeup_slope`]'s share.
    pub makeup_db: f32,
    /// Make-up that follows the settings: this many dB per dB of the
    /// threshold's full-scale reduction `−threshold·(1 − 1/ratio)`; 0 is none.
    pub makeup_slope: f32,
    /// Both channels follow the louder one.
    pub link: bool,
    /// 0..=1: the compressed share of the output.
    pub mix: f32,
    pub detector: Detector,
}

impl Compressor {
    /// The whole make-up gain in dB at the current threshold and ratio.
    pub fn makeup(&self) -> f32 {
        let reduction = -self.threshold_db * (1.0 - 1.0 / self.ratio.max(1.0));
        self.makeup_db + self.makeup_slope * reduction
    }
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
    /// The low cut is a second-order high-pass, Q ½, not the one-pole.
    pub steep_low_cut: bool,
    /// A high shelf on the heads, inside the loop (`q` its slope).
    pub high_shelf: Option<crate::Bell>,
    /// How much shorter the left line runs and longer the right, ms.
    pub spread_ms: f32,
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

/// A notch phaser: `stages` second-order allpasses at one corner the LFO
/// sweeps, inside a loop through its own band-pass, averaged with their
/// input, mixed with the dry signal at a held level, then high-passed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Notches {
    /// 1..=12.
    pub stages: u8,
    /// The corner with the LFO at 0, and the LFO's swing of it, octaves.
    pub hz: f32,
    pub octaves: f32,
    /// The stages' damping is `2·2^r` (their Q `1/(2·2^r)`),
    /// `r = spread + spread_swing·lfo` held within `±spread_limit`.
    pub spread: f32,
    pub spread_swing: f32,
    pub spread_limit: f32,
    /// The loop gain; its band-pass spans the corner `÷a` to `·a`,
    /// `a = r + √(1 + r²)`, `r = loop_width·2^r`, the top edge at most
    /// `loop_top` as `tan(π·f/rate)` at the running rate.
    pub feedback: f32,
    pub loop_width: f32,
    pub loop_top: f32,
    /// The LFO's rate, rounding (0: a triangle), depth and warp, and the
    /// channels' phases either side of it, cycles.
    pub rate: Rate,
    pub rounding: f32,
    pub depth: f32,
    pub warp: f32,
    pub phase: f32,
    pub polarity: Polarity,
    pub mix: f32,
    /// The output's high-pass, and the rate it runs nearest to by octaves.
    pub low_cut_hz: f32,
    pub internal_hz: f32,
}

/// A chorus ensemble: [`ENSEMBLE_VOICES`] swept delay lines whose reads
/// feed back into every line through a circulant `scatter`; each voice's
/// tap (its read plus its input) turns in the stereo field to a wet bus
/// with a low shelf on it, plus `direct` of the input. The voices' fixed
/// shape is shared data; the knobs set the rest.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ensemble {
    pub voices: &'static EnsembleVoices,
    /// The rest delays' scale: voice `v` rests at `delay[v]·span` s.
    pub span: f32,
    /// The LFOs' scale: voice `v` runs at `rate[v]·hz`.
    pub hz: f32,
    /// A voice's swing times its LFO rate, s·Hz: every voice sweeps to the
    /// same pitch whatever its rate.
    pub sweep: f32,
    /// Each voice's level on the wet bus (0: heard only through `scatter`)
    /// and its turn in the stereo field (radians, a rotation).
    pub levels: [f32; ENSEMBLE_VOICES],
    pub angles: [f32; ENSEMBLE_VOICES],
    /// The loop's circulant mix, its first row: line `i` takes
    /// `Σₖ scatter[(k − i) mod 7]` of read `k` (`[1, 0, …]` each its own).
    pub scatter: [f32; ENSEMBLE_VOICES],
    /// The first voice's loop gain; a voice `k` times as long takes it to
    /// the power `k`, so all decay alike per second.
    pub feedback: f32,
    /// The reads' polarity, in the loop and on the bus.
    pub polarity: Polarity,
    /// The input into the lines, the wet bus's gain out, and the input
    /// straight out.
    pub input: f32,
    pub wet: f32,
    pub direct: f32,
    /// The filter on the wet bus.
    pub shelf: Section,
}

pub const ENSEMBLE_VOICES: usize = 7;

/// The voices' fixed shape, a field per row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnsembleVoices {
    /// The rest delay per unit span.
    pub delay: [f32; ENSEMBLE_VOICES],
    /// The LFO's rate per unit hz, and its start (cycles). The LFO is a
    /// triangle from the rest delay up to the swing and back.
    pub rate: [f32; ENSEMBLE_VOICES],
    pub start: [f32; ENSEMBLE_VOICES],
    /// The triangle's bend: `u` becomes `(1 − b)·u + b·u²`.
    pub bend: [f32; ENSEMBLE_VOICES],
    /// A second LFO, a cosine starting at its low point: its rate over the
    /// voice's, and its depth in swings (0 for none). It moves the delay by
    /// `depth·(1 + ½·cos)`.
    pub wobble_rate: [f32; ENSEMBLE_VOICES],
    pub wobble: [f32; ENSEMBLE_VOICES],
    /// The one-pole that rounds the sweep, Hz (infinite for none).
    pub smooth_hz: [f32; ENSEMBLE_VOICES],
    /// The filter on each line's input.
    pub filter: [Section; ENSEMBLE_VOICES],
}

/// Whether a signal passes as it is or turned over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Polarity {
    Normal,
    Inverted,
}

/// Four stereo combs on one input, each a line around a soft-clipped,
/// rotated feedback loop with a smoother in it, read by a swept head, its
/// mirror and a still head; a triangle LFO sweeps the heads or scans a
/// window across the voices. Its input mixed in (slot `dry` 0). What the
/// voices share is carried once; per voice only the interval and detune.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CombBank {
    /// The first voice's pitch before its interval and detune.
    pub root_hz: f32,
    /// Each voice's interval over the root (it also sets the voice's loop
    /// gain, `feedback^(1/ratio)`) and its detune factor.
    pub ratio: [f32; 4],
    pub detune: [f32; 4],
    /// The lowest voice pitch any setting reaches: the lines' length.
    pub lowest_hz: f32,
    /// The root's loop gain a pass.
    pub feedback: f32,
    /// 0..=3: the voices after the first, fading in one by one; where the
    /// scan rests.
    pub voices: f32,
    /// 0..=¼: the loop smoother's outer tap, ¼ a zero at Nyquist.
    pub damping: f32,
    /// 0..=1: the heads' swing, a share of their rest delay.
    pub depth: f32,
    pub rate: Rate,
    /// How much further back the still head reads, a share of the rest.
    pub offset: f32,
    /// The swept head's sign, out and round the loop.
    pub polarity: Polarity,
    pub mode: CombMode,
    /// The share of its shape the LFO moves the scan window.
    pub scan_depth: f32,
    pub scan_shape: ScanShape,
    /// 0..=1: the heads turned apart (and the mirror head in).
    pub width: f32,
    /// The wet share; the input makes up the rest.
    pub mix: f32,
}

/// A feedback comb on one voice: the input plus its own output, delayed one
/// period of `hz` and scaled by `feedback` (0..1). Runs per voice, so a route
/// can move `hz` for each note.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Comb {
    pub hz: f32,
    pub feedback: f32,
    /// Octaves a [`crate::ModKind::Cutoff`] depth of 1 moves `hz` (the range
    /// of the format's frequency control).
    pub octaves: f32,
}

/// The lowest `hz` a [`Comb`] reaches: its line's length.
pub const COMB_LOWEST_HZ: f32 = 20.0;

/// What a [`CombBank`]'s voices play.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CombMode {
    /// The swept heads.
    Standard,
    /// The swept heads around the still one.
    ThruZero,
    /// Still heads, a window scanning the voices.
    Scan,
}

/// What moves a [`CombBank`]'s scan window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanShape {
    /// From the first voice to the last and back.
    Triangle,
    /// Once round all the voices, rising.
    SawUp,
    /// Once round, falling.
    SawDown,
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
/// fraction of its length) the late part takes over; `None`: where the
/// response's decay turns diffuse, found from the response itself.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImpulseCuts {
    pub early: [f32; 2],
    pub late: [f32; 2],
    pub crossover: Option<f32>,
}

/// A setting of how a convolution slot's response is prepared, which a
/// script's knob moves ([`crate::KnobTarget::Impulse`]): the response is
/// prepared again with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImpulseKnob {
    PredelayMs,
    /// The early or late part's low (`high` false) or high cut, in Hz.
    Cut {
        late: bool,
        high: bool,
    },
    /// The early or late part's length ratio ([`crate::Impulse::size`]).
    Size {
        late: bool,
    },
}

impl ImpulseKnob {
    /// Where the setting lives in `law`. A cut on a response with none
    /// first gets open cuts (20 Hz and 20 kHz) and no crossover.
    pub fn field_mut(self, law: &mut crate::Impulse) -> &mut f32 {
        match self {
            Self::PredelayMs => &mut law.predelay_ms,
            Self::Size { late } => {
                let [early, late_part] = &mut law.size;
                if late { late_part } else { early }
            }
            Self::Cut { late, high } => {
                let cuts = law.cuts.get_or_insert(ImpulseCuts {
                    early: [20.0, 20_000.0],
                    late: [20.0, 20_000.0],
                    crossover: None,
                });
                let part = if late {
                    &mut cuts.late
                } else {
                    &mut cuts.early
                };
                let [low, hi] = part;
                if high { hi } else { low }
            }
        }
    }
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
