//! The dynamics and time effects' settings in physical units.

use std::borrow::Cow;

use crate::{Network, Rate, Section, Shared};

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
    /// A one-pole high-pass on what the detector reads, its corner in Hz.
    pub sidechain_hz: Option<f32>,
}

impl Compressor {
    /// The whole make-up gain in dB at the current threshold and ratio.
    pub fn makeup(&self) -> f32 {
        let reduction = -self.threshold_db * (1.0 - 1.0 / self.ratio.max(1.0));
        self.makeup_db + self.makeup_slope * reduction
    }
}

/// A compressor, then a waveshaper (a channel strip's dynamics and drive),
/// in place: the curve reads the compressed signal.
#[derive(Clone, Debug, PartialEq)]
pub struct Strip {
    pub compressor: Compressor,
    pub shaper: crate::Shaper,
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

/// A peak limiter: input gain, then a gain reduction that holds peaks to
/// the ceiling, optionally smoothed and looking ahead.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limiter {
    /// Gain before limiting.
    pub input_db: f32,
    /// The highest peak it lets out.
    pub ceiling_db: f32,
    /// Time for the held peak to halve (6 dB of recovery); 0 = no limit.
    pub release_ms: f32,
    /// Width of the soft knee around the ceiling, dB.
    pub knee_db: f32,
    /// Time a rising reduction takes to cover two thirds of the step.
    pub attack_ms: f32,
    /// Time a falling reduction takes to fall to a third; 0 = no smoothing.
    pub smooth_ms: f32,
    /// Share, 0..=1, of a fall 100× slower than `smooth_ms`.
    pub slow: f32,
    /// How far ahead the gain looks; the audio is delayed by this much.
    pub lookahead_ms: f32,
    /// Clip the output at the ceiling.
    pub clip: bool,
}

impl Limiter {
    /// A peak follower and a clip at full scale: no knee, smoothing or
    /// lookahead.
    pub const PEAK: Self = Self {
        input_db: 0.0,
        ceiling_db: 0.0,
        release_ms: 0.0,
        knee_db: 0.0,
        attack_ms: 0.0,
        smooth_ms: 0.0,
        slow: 0.0,
        lookahead_ms: 0.0,
        clip: true,
    };
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

/// A scanner vibrato in physical units: the band below `split_hz` runs down
/// a lumped LC delay line (`sections` series inductors, shunt capacitors to
/// ground, `load_ohm` at its end); a pickup turning `hz` times a second
/// sweeps out across `taps` and back, crossfading neighbours. The band above
/// passes around the line at `high`; `blend` mixes the line's input into
/// the pickup's signal (a chorus, half each at 1), `mix` all of it against
/// the input. The line takes both channels' sum; each side has its own
/// pickup, the right `lead` turns ahead.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scanner {
    pub sections: u8,
    pub henry: f32,
    /// Each section's capacitance, F; the last section's is `last_farad`.
    pub farad: f32,
    pub last_farad: f32,
    pub load_ohm: f32,
    /// Resistances to ground at the first six nodes after the input, Ω.
    pub shunt_ohm: [f32; 6],
    /// Nodes the pickup crosses, out from the first (0: the line's input).
    pub taps: [u8; 9],
    pub hz: f32,
    pub lead: f32,
    pub split_hz: f32,
    pub high: f32,
    pub blend: f32,
    pub mix: f32,
}

/// A modulated stereo delay in physical units: two lines, one or two heads
/// an LFO sweeps on each, the heads filtered, then mixed back into the lines
/// and out by matrices.
#[derive(Clone, Debug, PartialEq)]
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
    /// Sections on the heads' sum, inside the loop after the low cut and
    /// the shelf (a mode's own body: a tape's or a bucket brigade's tone);
    /// the first four run.
    pub body: Cow<'static, [crate::Section]>,
    /// A triangle LFO averaged over this many seconds (0: sharp corners)
    /// instead of the sine; it falls first as the sine does.
    pub triangle_s: Option<f32>,
    /// A curve on what is written into the lines (the input and the
    /// feedback): points evenly over −1..=1, read with linear interpolation
    /// and held beyond; empty for none (a mode's tape or bucket brigade
    /// saturation).
    pub shape: Cow<'static, [f32]>,
    /// Noise written into the lines after the curve: white, its rms
    /// `level·rms^power` of the line's input over 10 ms; level 0 for none.
    pub noise: [f32; 2],
    /// A high shelf on the heads, inside the loop (`q` its slope).
    pub high_shelf: Option<crate::Bell>,
    /// How much shorter the left line runs and longer the right, ms.
    pub spread_ms: f32,
    pub rate: Rate,
    /// Cycles the right LFO runs ahead of the left.
    pub phase: f32,
    /// The input straight to the output, with the heads (0 for none): a
    /// chorus that mixes its own dry signal.
    pub direct: f32,
}

/// A pitch-shifting echo in physical units: two lines, on each two heads
/// gliding at `speed` against the write head, crossfaded so each grain is
/// centred on the delay; their sum back into the lines by `feedback`,
/// `cross` of it from the other side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shifter {
    /// The left side's delay.
    pub time: DelayTime,
    /// The right side's delay over the left's.
    pub right: f32,
    /// Each side's heads' speed over the write head's: 1 an echo, 2 an
    /// octave up, negative backwards.
    pub speed: [f32; 2],
    pub feedback: f32,
    /// 0..=1: the other side's share of each line's feedback.
    pub cross: f32,
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
#[derive(Clone, Debug, PartialEq)]
pub struct Ensemble {
    pub voices: Shared<EnsembleVoices>,
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
    /// Feed-forward: the input delayed, not the output (no ringing).
    pub forward: bool,
    /// Octaves a [`crate::ParamAddr::FilterCutoff`] depth of 1 moves `hz` (the range
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
#[derive(Clone, Debug, PartialEq)]
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
    /// The share of the response kept ([`crate::Impulse::length`]).
    Length,
    /// Reverses the response when on ([`crate::Impulse::reverse`]).
    Reverse,
    /// Normalizes the response when on ([`crate::Impulse::normalize`]).
    AutoGain,
}

impl ImpulseKnob {
    /// Sets this knob of `law` to `y` (a switch is on above 0.5; Auto Gain
    /// then normalizes as `fresh` does); false when `law` already had it.
    pub fn apply(self, law: &mut crate::Impulse, y: f32, fresh: Option<crate::Normalize>) -> bool {
        let on = y > 0.5;
        match self {
            Self::Reverse => std::mem::replace(&mut law.reverse, on) != on,
            Self::AutoGain => {
                let want = fresh.filter(|_| on);
                std::mem::replace(&mut law.normalize, want) != want
            }
            _ => self
                .field_mut(law)
                .is_some_and(|at| std::mem::replace(at, y) != y),
        }
    }

    /// This knob's setting in `law` (a switch reads 0 or 1).
    #[must_use]
    pub fn read(self, law: &crate::Impulse) -> f32 {
        match self {
            Self::Reverse => f32::from(u8::from(law.reverse)),
            Self::AutoGain => f32::from(u8::from(law.normalize.is_some())),
            Self::PredelayMs => law.predelay_ms,
            Self::Length => law.length,
            Self::Size { late } => {
                let [early, late_part] = law.size;
                if late { late_part } else { early }
            }
            Self::Cut { late, high } => {
                let [low, hi] = law
                    .cuts
                    .map_or([20.0, 20_000.0], |c| if late { c.late } else { c.early });
                if high { hi } else { low }
            }
        }
    }

    /// Where a continuous setting lives in `law` (`None` for a switch). A
    /// cut on a response with none first gets open cuts (20 Hz and 20 kHz)
    /// and no crossover.
    fn field_mut(self, law: &mut crate::Impulse) -> Option<&mut f32> {
        Some(match self {
            Self::PredelayMs => &mut law.predelay_ms,
            Self::Length => &mut law.length,
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
            Self::Reverse | Self::AutoGain => return None,
        })
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

/// A two-rotor rotary speaker in physical units: a crossover splits the signal
/// into a high-frequency horn and a low-frequency drum, each spinning between
/// slow and fast speeds with independent acceleration inertia, driving stereo
/// Doppler delay modulation and directional amplitude modulation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rotary {
    /// Target speed state: `true` for fast, `false` for slow.
    pub fast: bool,
    /// Slow and fast rotation rates for the horn (Hz).
    pub horn_slow_hz: f32,
    pub horn_fast_hz: f32,
    /// Slow and fast rotation rates for the drum (Hz).
    pub drum_slow_hz: f32,
    pub drum_fast_hz: f32,
    /// Horn acceleration responsiveness, `0..=1` (higher accelerates faster).
    pub accel_hi: f32,
    /// Drum acceleration responsiveness, `0..=1` (higher accelerates faster).
    pub accel_lo: f32,
    /// Crossover frequency between drum and horn (Hz).
    pub crossover_hz: f32,
    /// Drum-to-horn balance, `0..=1` (`0` drum only, `0.5` equal, `1` horn only).
    pub balance: f32,
    /// Microphone distance / stereo spread, `0..=1` (`0` close/wide, `1` far/blended).
    pub distance: f32,
    /// Wet share of the output, `0..=1`.
    pub mix: f32,
}

/// A rhythmic step/pulse amplitude gater in physical units: gates the stereo
/// signal (and an optional internal white-noise generator) at a free or
/// tempo-synced step rate with smoothed edge shaping and optional stutter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gater {
    /// Step clock rate (free Hz or tempo-synced beats).
    pub rate: Rate,
    /// Gate duty cycle and edge smoothness, `0..=1`.
    pub shape: f32,
    /// Internal white-noise level injected into the gated path, `0..=1`.
    pub noise: f32,
    /// Wet share of the output, `0..=1`.
    pub mix: f32,
    /// Doubles the rhythmic gate subdivision for stutter accents.
    pub stutter: bool,
    /// Mutes the audio input into the gate so only gated noise passes.
    pub mute_input: bool,
    /// Engages rhythmic gating (when `false`, passes un-gated).
    pub active: bool,
}

/// A tempo-synced real-time loop/stutter masher in physical units: captures a
/// beat-locked bar into a circular buffer and loops a rotated, optionally
/// reversed and gated slice of `length_beats`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BeatMasher {
    /// Looped slice length in quarter-note beats.
    pub length_beats: f32,
    /// Start offset rotation across the captured bar, `0..=1`.
    pub rotate: f32,
    /// Bipolar slice gate shaping, `-1..=1` (`0` full open).
    pub gate: f32,
    /// Wet share of the output, `0..=1`.
    pub mix: f32,
    /// Plays the captured slice backwards.
    pub reverse: bool,
    /// Wraps slice reads inside the active bar boundary.
    pub wrap: bool,
    /// Engages capture lock and rhythmic looping.
    pub mash: bool,
}

/// A tempo-synced beat capture and slice-reordering engine in physical units:
/// divides a 1-bar or 2-bar capture into rhythmic slices and replays them
/// according to `pattern`, `style`, micro-repeat `buzz`, and `gate`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BeatSlicer {
    /// Capture length in 4/4 bars (`1` or `2`).
    pub bars: u8,
    /// Slice permutation pattern index (`0..=4`).
    pub pattern: u8,
    /// Playback and envelope style index (`0..=4`).
    pub style: u8,
    /// Sub-slice buzz / micro-repeat intensity, `0..=1`.
    pub buzz: f32,
    /// Shortens slice envelopes for percussive gating.
    pub gate: bool,
    /// Engages capture and slice reordering.
    pub slice: bool,
    /// Wet share of the output, `0..=1`.
    pub mix: f32,
}

/// A dual-window raised-cosine reverse granular engine in physical units:
/// reads overlapping Hann-windowed grains from a live capture buffer with
/// independent traversal speed, grain duration, pitch ratio, and direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReverseGrain {
    /// Capture traversal speed, `0..=1`.
    pub speed: f32,
    /// Grain window duration, ms.
    pub grain_ms: f32,
    /// Grain playback pitch ratio (`1.0` unity, `2.0` octave up).
    pub pitch_ratio: f32,
    /// Wet share of the output, `0..=1`.
    pub mix: f32,
    /// Runs forward capture traversal.
    pub forward: bool,
    /// Flips individual grain playback direction.
    pub invert_grains: bool,
    /// Engages reverse granular capture playback.
    pub reverse: bool,
}

/// A real-time WSOLA / overlap-add pitch-shift and time-stretch processor in
/// physical units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransposeStretch {
    /// Pitch transposition in semitones (`-60..=12`, `0` unity).
    pub semitones: f32,
    /// Time-stretch slowdown amount when stretching, `0..=1`.
    pub amount: f32,
    /// Grain window duration, ms.
    pub grain_ms: f32,
    /// Wet share of the output, `0..=1`.
    pub mix: f32,
    /// Engages beat-buffer time-stretch capture.
    pub stretch: bool,
    /// Selects granular freeze texture vs continuous WSOLA overlap-add.
    pub grain_mode: bool,
    /// Capture length in 4/4 bars (`1` or `2`).
    pub bars: u8,
}

/// A dual independent stereo delay in physical units: per-channel pre-delay
/// lines feeding independent main delay lines with self-feedback,
/// cross-channel feedback, independent wet output levels, and stereo width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DualDelay {
    pub predelay_l: DelayTime,
    pub predelay_r: DelayTime,
    pub time_l: DelayTime,
    pub time_r: DelayTime,
    pub feedback_l: f32,
    pub feedback_r: f32,
    pub cross_feedback: f32,
    pub level_l: f32,
    pub level_r: f32,
    /// Stereo image: 0 swaps channels, 0.5 sums to mono, 1 is normal stereo.
    pub width: f32,
}

/// A stereo pitch-detuning widener in physical units: a crossover splits the
/// low band (passed unchanged) from the high band, which is pitch-shifted up
/// and down across two windowed stereo channels, delayed, spread, and blended
/// with the dry input.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StereoDetune {
    /// Crossover frequency between dry low band and detuned high band, Hz.
    pub split_hz: f32,
    /// Pitch ratio (`>= 1.0`): one side shifts by `1/ratio`, the other by `ratio`.
    pub ratio: f32,
    /// Stereo cross-spread, 0..=1.
    pub spread: f32,
    /// Wet/dry blend, 0..=1.
    pub mix: f32,
}

/// A four-stage resonant wah filter in physical units: a swept band-pass peak,
/// a low-mid contour stage, a mid vocal stage, and a high presence stage, with
/// optional mono summing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wah {
    pub peak_hz: f32,
    pub peak_q: f32,
    pub peak_gain: f32,
    pub low_hz: f32,
    pub low_gain: f32,
    pub mid_hz: f32,
    pub mid_gain: f32,
    pub high_hz: f32,
    pub high_gain: f32,
    pub mono: bool,
}

/// A dynamic four-band spectral/formant morphing filter in physical units
/// (the engine's `filter::SpectralMorph`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpectralMorph {
    /// Position across the active bank's spectral profiles (`0..=1`).
    pub morph: f32,
    /// Wet spectral morphing depth (`0..=1`).
    pub amount: f32,
    /// Acoustic profile bank index (`0..=3`).
    pub bank: u8,
    /// Enables stereo formant tracking offset.
    pub tracking: bool,
}
