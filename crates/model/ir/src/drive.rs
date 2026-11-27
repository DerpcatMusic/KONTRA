//! Nonlinear colour in physical terms: a waveshaper, a reducer and an
//! overdrive. Each runs on each voice when it sits in a group rack. [`EffectParam`] names the
//! effect controls a route can move.

use std::borrow::Cow;

use crate::{Effect, EffectKind};

/// A transfer curve and its amount (the engine's `drive::Curve`).
#[derive(Clone, Debug, PartialEq)]
pub enum DriveCurve {
    /// −1..=1: above 0 a soft clip, below 0 a cubic expansion.
    Soft(f32),
    /// 0..=1: a power law on small signals.
    Power(f32),
    /// The drive `d`: `sin(d·x)`.
    Sine(f32),
    /// The drive `d`: `asin(sin(d·x))`, a triangle fold.
    Fold(f32),
    /// The drive `d`: `tanh(d·x)`.
    Tanh(f32),
    /// The drive `d`: `atan(d·x)`.
    Atan(f32),
    /// The drive `d`: `x / √(1 + (d·x)²)` (unity on small signals, ceiling `1/d`).
    Saturate(f32),
    /// A measured odd curve: `points` spread evenly over inputs 0..=1, read
    /// at `d·|x|` (held at the last point beyond 1) with linear
    /// interpolation, scaled back by `1 / (d·slope at 0)` so small signals
    /// pass at unity.
    Table {
        drive: f32,
        points: Cow<'static, [f32]>,
    },
    /// Two measured odd curves, `rows` the lower then the upper (half its
    /// points each), each spread evenly over inputs 0..=1 (held beyond), `t`
    /// of the way from lower to upper, read at `d·|x|` and scaled back by
    /// `1/d`: a gain of `d` in and out. One slice, so a shaper stays small.
    Blend {
        drive: f32,
        rows: Cow<'static, [f32]>,
        t: f32,
    },
    /// Two asymmetric bipolar transfer curves, `rows` the lower then the upper
    /// (half its points each), each spread evenly over inputs −1..=1 (held
    /// beyond), blended linearly by `t` in 0..=1.
    AsymBlend { rows: Cow<'static, [f32]>, t: f32 },
}

impl DriveCurve {
    /// Its amount or drive.
    pub fn amount_mut(&mut self) -> &mut f32 {
        match self {
            Self::Soft(x)
            | Self::Power(x)
            | Self::Sine(x)
            | Self::Fold(x)
            | Self::Tanh(x)
            | Self::Atan(x)
            | Self::Saturate(x)
            | Self::Table { drive: x, .. }
            | Self::Blend { drive: x, .. }
            | Self::AsymBlend { t: x, .. } => x,
        }
    }
}

/// An optional one-pole low-pass, a curve, then an optional one-pole
/// low-pass and a second-order Butterworth high-pass; `sections` add linear
/// sections at the base rate around all of it and oversampled around the
/// curve.
#[derive(Clone, Debug, PartialEq)]
pub struct Shaper {
    /// The corner of a one-pole low-pass before the curve, pole
    /// `e^(−2π·hz/rate)` (no zero at Nyquist).
    pub pre_lowpass_hz: Option<f32>,
    pub curve: DriveCurve,
    pub lowpass_hz: Option<f32>,
    pub highpass_hz: Option<f32>,
    pub sections: crate::Sections,
}

/// A sample hold, a quantizer truncating toward zero, then noise; or, with
/// a [`Converter`], a modelled converter pair instead of the raw hold and
/// quantizer.
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
    /// Replaces the hold and quantizer above (which are then off).
    pub converter: Option<Converter>,
}

/// A low-quality sampler as a converter pair (the engine's
/// `drive::Converter`), run at twice the rate up to 48 kHz: a four-pole
/// low-pass, a jittered clock reading the input by cubic interpolation, a
/// companding quantizer with dither, a soft saturator, band-limited steps
/// back, a four-pole low-pass, a one-pole high-pass, then the dry mixed in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Converter {
    pub clock_hz: f32,
    /// Random clock change per captured sample, a share of the period.
    pub jitter: f32,
    /// The Butterworth low-passes before and after.
    pub pre_hz: f32,
    pub post_hz: f32,
    pub highpass_hz: f32,
    /// The quantizer step on the companded scale (2 over the levels).
    pub step: f32,
    /// The compander's `μ` (`ln(1 + μ·|x|)/ln(1 + μ)`), above 0.
    pub mu: f32,
    /// No zero level: steps sit at half steps, so silence buzzes.
    pub midrise: bool,
    /// Gaussian dither, in quantizer steps.
    pub dither: f32,
    /// Linear gains into the quantizer and out of it.
    pub drive_in: f32,
    pub drive_out: f32,
    /// The saturator `y·(1 − soft·|y|)` (`y` held to ±2), then `trim`.
    pub soft: f32,
    pub trim: f32,
    /// Wet and dry levels.
    pub wet: f32,
    pub dry: f32,
}

/// A diode ring modulator (the engine's `spatial::RingMod`): the input's
/// analytic pair times a quadrature sine, through the diode law
/// `√((k(c + x))² + 1) − √((k(c − x))² + 1)`, normalized to unit gain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RingMod {
    /// The carrier, signed: below 0 a shift moves down.
    pub hz: f32,
    /// The left carrier's lead over the right, in cycles.
    pub spread: f32,
    /// A bias added to the carrier, scaled by `1 − bias/2`: 1 is amplitude
    /// modulation, 0 ring modulation.
    pub bias: f32,
    /// The quadrature branch's level: 1 cancels one sideband (a frequency
    /// shift).
    pub quadrature: f32,
    /// The diode law's `k`; small is a clean product.
    pub drive: f32,
    /// A one-pole high-pass on the input.
    pub highpass_hz: f32,
    /// Mutes the wet path while the input is silent.
    pub gate: bool,
    /// The share of the wet output, high-passed at 20 Hz and low-passed at
    /// 15 kHz, taken back off the input (solved implicitly); 0 is none.
    pub feedback: f32,
    /// What modulates the input.
    pub source: RingSource,
    pub wet: f32,
    pub dry: f32,
}

/// A ring modulator's carrier.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RingSource {
    /// A quadrature sine at `hz`.
    Oscillator,
    /// The input's own analytic pair, its quadrature branch inverted (so a
    /// shift doubles rather than cancels), blended toward its envelope.
    Sidechain {
        /// 0 the signal itself, 1 its envelope.
        contour: f32,
        /// The envelope's attack and release in seconds.
        attack: f32,
        release: f32,
        /// A band-pass (3.5 octaves wide) on the modulator first.
        bandpass_hz: Option<f32>,
    },
    /// An AM radio: the input, scaled by `gain` and biased (`bias`), is
    /// transmitted on the sine carrier, then received through a band-pass,
    /// a rectifier (or a product with a sine at `tuned_hz`), a 50 Hz
    /// high-pass, a four-pole low-pass at half `tuned_hz` and a level
    /// control, with low-passed noise added before the band-pass.
    Radio {
        gain: f32,
        /// The receiver's band-pass centre.
        tuned_hz: f32,
        /// The band-pass damping `1/Q`.
        damping: f32,
        /// The received level's makeup.
        makeup: f32,
        /// How far demodulation is a product rather than an envelope (0..1):
        /// the received signal crossfades to the band times the tuned sine,
        /// and the bias follows the input's level that far.
        product: f32,
        /// The noise level.
        noise: f32,
    },
    /// A continuous sine-to-triangle carrier whose rate is modulated by a
    /// parabolic sine or square LFO, followed by a carrier-driven stereo FM
    /// delay line.
    Modulated {
        /// Carrier waveshape: 0 is parabolic sine, 1 is triangle.
        edge: f32,
        /// Peak stereo FM delay swing, ms.
        fm_ms: f32,
        /// LFO rate in Hz or quarter-note beats.
        lfo_rate: crate::Rate,
        /// Peak upward carrier-rate modulation multiplier above base (0 is none).
        lfo_depth: f32,
        /// `true` selects a square LFO; `false` selects a parabolic sine LFO.
        lfo_square: bool,
    },
}

/// An overdrive pedal (the engine's `drive::Overdrive`): one-pole splits
/// around a clipping curve at four times the rate, the shaped band fed back
/// into the input split and mixed with the dry one through a tone low-pass.
#[derive(Clone, Debug, PartialEq)]
pub struct Overdrive {
    /// The input split's one-pole corners in Hz: bass, then two high-passes.
    pub split_hz: [f32; 3],
    /// The output smoothers' corners in Hz: envelope, bright, tone.
    pub smooth_hz: [f32; 3],
    /// Linear gain into the curve.
    pub drive: f32,
    /// Linear gain of the dry band.
    pub mix: f32,
    /// Linear output gain.
    pub level: f32,
    /// The clipping curve at 256 points over inputs −128..=127.
    pub curve: Cow<'static, [f32]>,
}

/// A diode clipper circuit (the engine's `drive::Clipper`), solved per
/// frame: the input soft-limited to `limit·tanh(drive·x/limit)`, a coupling
/// high-pass at `coupling_hz` into a low-pass at `tone_hz` across an
/// antiparallel diode pair conducting `saturation·(e^(slope·v) −
/// e^(−asymmetry·slope·v))` (in units of the tone resistor's current), then
/// a one-pole DC blocker at `dc_hz`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clipper {
    pub drive: f32,
    pub limit: f32,
    pub tone_hz: f32,
    pub coupling_hz: f32,
    pub saturation: f32,
    pub slope: f32,
    pub asymmetry: f32,
    pub dc_hz: f32,
}

/// Circuit voicing topology for [`AmpStack`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AmpVoicing {
    /// Clean combo with bright shelf and warm asymmetric tube breakup.
    CleanCombo,
    /// British high-gain cascaded triodes with strong upper-mid bite.
    BritishHighGain,
    /// Modern high-gain dual-channel preamp with tight low-end resonance.
    ModernHighGain,
    /// Class-A chimey combo with interactive tone stack and power-amp cut.
    ClassACombo,
    /// Lead high-gain preamp with smooth singing sustain and depth control.
    LeadHighGain,
    /// Hard-clipping op-amp distortion pedal with reverse low-pass filter.
    HardClipPedal,
    /// Tube bass amplifier with sweepable mid frequency and 9-band graphic EQ.
    BassTube,
    /// Mid-humped soft-clipping overdrive pedal with 3 clipping modes (`0..=2`).
    MidOverdrive(u8),
}

/// A multi-stage guitar/bass amplifier or distortion/overdrive pedal in
/// physical units (the engine's `drive::AmpStack`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AmpStack {
    pub voicing: AmpVoicing,
    /// Active channel preamp gain, linear (`>= 0`).
    pub preamp_gain: f32,
    /// Inactive alternate channel preamp gain stored for dual-channel amps.
    pub alt_preamp_gain: f32,
    /// Saturation/power-stage drive, `0..=1`.
    pub drive: f32,
    /// Master output level, linear (`>= 0`).
    pub master: f32,
    /// Engages high-gain / lead / overdrive voicing.
    pub high_gain: bool,
    /// Engages upper-treble bright boost.
    pub bright: bool,
    /// Engages sub-bass boost (crunch / ultra-low).
    pub low_boost: bool,
    /// Engages upper-presence boost (ultra-high).
    pub high_boost: bool,
    pub bass_db: f32,
    pub mid_db: f32,
    pub mid_hz: f32,
    pub treble_db: f32,
    pub presence_db: f32,
    pub resonance_db: f32,
    /// Post-stage tone-cut low-pass corner in Hz.
    pub tone_cut_hz: f32,
    /// Built-in tremolo LFO rate in Hz.
    pub tremolo_hz: f32,
    /// Built-in tremolo modulation depth, `0..=1`.
    pub tremolo_depth: f32,
    /// Engages the 9-band graphic equalizer.
    pub geq_enabled: bool,
    /// 9-band graphic equalizer gains in dB (`40..10k` Hz).
    pub geq_db: [f32; 9],
    /// Graphic equalizer makeup gain in dB.
    pub geq_gain_db: f32,
    /// Sums stereo input to mono before nonlinear processing.
    pub mono: bool,
}

/// A speaker cabinet and room early-reflection simulator in physical units
/// (the engine's `drive::Cabinet`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cabinet {
    /// Enclosure acoustic profile index (`0..=11`).
    pub model: u8,
    /// Enclosure size scale (`0..=1`).
    pub size: f32,
    /// Room early-reflection mix (`0..=1`).
    pub air: f32,
    /// Low-shelf tone control in dB.
    pub bass_db: f32,
    /// High-shelf tone control in dB.
    pub treble_db: f32,
}

/// A ten-character harmonic saturator with bass-preserving crossover and
/// 3-band parametric EQ in physical units (the engine's `drive::ColorSat`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorSat {
    /// Saturation character mode (`0..=9`).
    pub character: u8,
    /// Input drive (`0..=1`).
    pub drive: f32,
    /// Engages pre-drive boost.
    pub boost: bool,
    /// Pre-drive boost level in dB (`0..=24`).
    pub boost_db: f32,
    /// Clean sub-bass bypass blend (`0..=1`).
    pub bass_saver: f32,
    /// Post low-pass cutoff in Hz.
    pub lpf_hz: f32,
    /// Pre high-pass cutoff in Hz.
    pub hpf_hz: f32,
    pub bass_db: f32,
    pub mid_db: f32,
    pub mid_hz: f32,
    pub mid_q: f32,
    pub treble_db: f32,
    /// Wet/dry blend (`0..=1`).
    pub mix: f32,
}

/// An effect control a route or a script moves, in the effect's own units:
/// a [`crate::Formant`] knob, a curve's amount, a [`Crush`] rate or bits, a
/// dynamics or modulation effect's physical setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EffectParam {
    FormantTalk,
    FormantSharp,
    FormantSize,
    /// A [`Shaper`]'s curve amount and low-pass corner.
    ShaperAmount,
    ShaperLowpassHz,
    CrushRate,
    CrushBits,
    /// Octaves on an [`EffectKind::Eq`] band's centre, kept within 20 Hz to
    /// 20 kHz.
    BellOctaves(u8),
    CompressorThreshold,
    CompressorRatio,
    CompressorAttack,
    CompressorRelease,
    /// A [`crate::Compressor`]'s make-up dB and wet share.
    CompressorMakeup,
    CompressorMix,
    /// A [`crate::Compressor`]'s soft-knee width in dB.
    CompressorKnee,
    /// A free-running [`crate::Delay`] or [`crate::Phaser`] LFO's Hz.
    ModRate,
    /// Cycles a [`crate::Delay`]'s or [`crate::Phaser`]'s right LFO runs ahead.
    ModPhase,
    PhaserExponent,
    PhaserFeedback,
    TransientAttack,
    TransientSustain,
    TransientGain,
    /// An [`EffectKind::Eq`] band's centre Hz, Q and gain, by band.
    BellHz(u8),
    BellQ(u8),
    BellGain(u8),
    /// A [`crate::Reverb`]'s tail, damping, stereo, high cut and predelay.
    ReverbSeconds,
    ReverbDamping,
    ReverbStereo,
    ReverbHighCut,
    ReverbPredelay,
    /// A [`crate::Limiter`]'s input gain dB and release ms.
    LimiterInput,
    LimiterRelease,
    /// A [`crate::Filter`]'s cutoff Hz.
    FilterHz,
    /// One of a [`crate::Filter`]'s stage dampings, by stage.
    FilterDamping(u8),
    /// A [`crate::Filter`]'s output gain, linear.
    FilterGain,
    /// A [`crate::Filter`]'s last stage's damping, whatever its stages.
    FilterLastDamping,
    /// A [`crate::Cascade`]'s stage 0 Hz, its shifts in octaves, and by
    /// stage its q, limit, peak, morph and wet; its output gain.
    CascadeHz,
    CascadeShift(u8),
    CascadeQ(u8),
    CascadeLimit(u8),
    CascadePeak(u8),
    CascadeMorph(u8),
    CascadeWet(u8),
    CascadeGain,
    /// A [`crate::Clocked`] filter's cutoff and resonance knobs.
    ClockedCutoff,
    ClockedResonance,
    /// A [`crate::Delay`]'s time in ms (not a synced one), its input and
    /// feedback matrix cells (row-major, 0..4) and its damping corner.
    DelayMs,
    DelayInput(u8),
    DelayLoop(u8),
    DelayDampingHz,
    /// A [`crate::Delay`]'s high-pass corner, and its high shelf's corner
    /// and gain.
    DelayLowCutHz,
    DelayShelfHz,
    DelayShelfDb,
    /// A tempo-synced [`crate::Delay`] or [`crate::Phaser`] LFO's beats a
    /// cycle (not numbered for scripts: [`Self::code`] is `None`).
    ModBeats,
    /// The slot's own [`Effect::output`] and [`Effect::dry`] levels, linear
    /// (moved on a bus's rack: [`Self::add_to`]).
    Output,
    Dry,
}

impl EffectParam {
    /// This control of `kind`; `None` when `kind` has none.
    pub fn field_mut(self, kind: &mut EffectKind) -> Option<&mut f32> {
        Some(match (self, kind) {
            (Self::FormantTalk, EffectKind::Formant(f)) => &mut f.talk,
            (Self::FormantSharp, EffectKind::Formant(f)) => &mut f.sharp,
            (Self::FormantSize, EffectKind::Formant(f)) => &mut f.size,
            (
                Self::ShaperAmount,
                EffectKind::Shaper(s) | EffectKind::Strip(crate::Strip { shaper: s, .. }),
            ) => s.curve.amount_mut(),
            (
                Self::ShaperLowpassHz,
                EffectKind::Shaper(s) | EffectKind::Strip(crate::Strip { shaper: s, .. }),
            ) => s.lowpass_hz.as_mut()?,
            (Self::CrushRate, EffectKind::Crush(c)) => &mut c.rate,
            (Self::CrushBits, EffectKind::Crush(c)) => &mut c.bits,
            (
                Self::CompressorThreshold,
                EffectKind::Compressor(c) | EffectKind::Strip(crate::Strip { compressor: c, .. }),
            ) => &mut c.threshold_db,
            (
                Self::CompressorRatio,
                EffectKind::Compressor(c) | EffectKind::Strip(crate::Strip { compressor: c, .. }),
            ) => &mut c.ratio,
            (
                Self::CompressorAttack,
                EffectKind::Compressor(c) | EffectKind::Strip(crate::Strip { compressor: c, .. }),
            ) => &mut c.attack_ms,
            (
                Self::CompressorRelease,
                EffectKind::Compressor(c) | EffectKind::Strip(crate::Strip { compressor: c, .. }),
            ) => &mut c.release_ms,
            (
                Self::CompressorMakeup,
                EffectKind::Compressor(c) | EffectKind::Strip(crate::Strip { compressor: c, .. }),
            ) => &mut c.makeup_db,
            (
                Self::CompressorMix,
                EffectKind::Compressor(c) | EffectKind::Strip(crate::Strip { compressor: c, .. }),
            ) => &mut c.mix,
            (
                Self::CompressorKnee,
                EffectKind::Compressor(c) | EffectKind::Strip(crate::Strip { compressor: c, .. }),
            ) => &mut c.knee_db,
            (Self::ModRate, EffectKind::Delay(crate::Delay { rate, .. }))
            | (Self::ModRate, EffectKind::Phaser(crate::Phaser { rate, .. })) => match rate {
                crate::Rate::Hz(hz) => hz,
                crate::Rate::Beats(_) | crate::Rate::Steps { .. } => return None,
            },
            (Self::ModBeats, EffectKind::Delay(crate::Delay { rate, .. }))
            | (Self::ModBeats, EffectKind::Phaser(crate::Phaser { rate, .. })) => match rate {
                crate::Rate::Beats(b) => b,
                crate::Rate::Hz(_) | crate::Rate::Steps { .. } => return None,
            },
            (Self::ModPhase, EffectKind::Delay(d)) => &mut d.phase,
            (Self::ModPhase, EffectKind::Phaser(p)) => &mut p.phase,
            (Self::PhaserExponent, EffectKind::Phaser(p)) => &mut p.exponent,
            (Self::PhaserFeedback, EffectKind::Phaser(p)) => &mut p.feedback,
            (Self::TransientAttack, EffectKind::Transient(t)) => &mut t.attack,
            (Self::TransientSustain, EffectKind::Transient(t)) => &mut t.sustain,
            (Self::TransientGain, EffectKind::Transient(t)) => &mut t.gain_db,
            (Self::BellHz(n), EffectKind::Eq(b)) => &mut b.get_mut(usize::from(n))?.hz,
            (Self::BellQ(n), EffectKind::Eq(b)) => &mut b.get_mut(usize::from(n))?.q,
            (Self::BellGain(n), EffectKind::Eq(b)) => &mut b.get_mut(usize::from(n))?.gain_db,
            (Self::ReverbSeconds, EffectKind::Reverb(r)) => &mut r.seconds,
            (Self::ReverbDamping, EffectKind::Reverb(r)) => &mut r.damping,
            (Self::ReverbStereo, EffectKind::Reverb(r)) => &mut r.stereo,
            (Self::ReverbHighCut, EffectKind::Reverb(r)) => &mut r.high_cut_hz,
            (Self::ReverbPredelay, EffectKind::Reverb(r)) => &mut r.predelay_ms,
            (Self::LimiterInput, EffectKind::Limiter(l)) => &mut l.input_db,
            (Self::LimiterRelease, EffectKind::Limiter(l)) => &mut l.release_ms,
            (Self::FilterHz, EffectKind::Filter(f)) => &mut f.hz,
            (Self::FilterDamping(n), EffectKind::Filter(f)) => f.damping.get_mut(usize::from(n))?,
            (Self::FilterGain, EffectKind::Filter(f)) => &mut f.gain,
            (Self::FilterLastDamping, EffectKind::Filter(f)) => {
                f.damping.get_mut(usize::from(f.stages.checked_sub(1)?))?
            }
            (Self::DelayMs, EffectKind::Delay(d)) => match &mut d.time {
                crate::DelayTime::Ms(ms) => ms,
                crate::DelayTime::Beats(_) => return None,
            },
            (Self::DelayInput(n), EffectKind::Delay(d)) => {
                d.input.as_flattened_mut().get_mut(usize::from(n))?
            }
            (Self::DelayLoop(n), EffectKind::Delay(d)) => {
                d.feedback.as_flattened_mut().get_mut(usize::from(n))?
            }
            (Self::DelayDampingHz, EffectKind::Delay(d)) => &mut d.damping_hz,
            (Self::DelayLowCutHz, EffectKind::Delay(d)) => &mut d.low_cut_hz,
            (Self::DelayShelfHz, EffectKind::Delay(d)) => &mut d.high_shelf.as_mut()?.hz,
            (Self::DelayShelfDb, EffectKind::Delay(d)) => &mut d.high_shelf.as_mut()?.gain_db,
            (Self::CascadeHz, EffectKind::Cascade(c)) => &mut c.hz,
            (Self::CascadeShift(n), EffectKind::Cascade(c)) => c.shifts.get_mut(usize::from(n))?,
            (Self::CascadeGain, EffectKind::Cascade(c)) => &mut c.gain,
            (Self::ClockedCutoff, EffectKind::Clocked(c)) => &mut c.cutoff,
            (Self::ClockedResonance, EffectKind::Clocked(c)) => &mut c.resonance,
            (
                Self::CascadeQ(n)
                | Self::CascadeLimit(n)
                | Self::CascadePeak(n)
                | Self::CascadeMorph(n)
                | Self::CascadeWet(n),
                EffectKind::Cascade(c),
            ) => {
                let s = c.stages.get_mut(usize::from(n))?;
                match self {
                    Self::CascadeQ(_) => &mut s.q,
                    Self::CascadeLimit(_) => &mut s.limit,
                    Self::CascadePeak(_) => &mut s.peak,
                    Self::CascadeMorph(_) => &mut s.morph,
                    _ => &mut s.wet,
                }
            }
            _ => return None,
        })
    }

    /// As [`Self::add`], on the slot's levels too.
    pub fn add_to(self, effect: &mut Effect, delta: f32) -> bool {
        match self {
            Self::Output => effect.output += delta,
            Self::Dry => effect.dry += delta,
            _ => return self.add(&mut effect.kind, delta),
        }
        true
    }

    /// Adds `delta` to this control of `kind`; false when `kind` has none.
    pub fn add(self, kind: &mut EffectKind, delta: f32) -> bool {
        if let (Self::BellOctaves(band), EffectKind::Eq(bells)) = (self, &mut *kind) {
            let Some(b) = bells.get_mut(usize::from(band)) else {
                return false;
            };
            b.hz = (b.hz * delta.exp2()).clamp(20.0, 20_000.0);
            return true;
        }
        let Some(at) = self.field_mut(kind) else {
            return false;
        };
        *at += delta;
        true
    }
}

/// Numbers every [`EffectParam`] for a script host: the plain controls in
/// order, then each indexed one by its index below [`INDEXED`].
macro_rules! param_codes {
    (plain: $($p:ident),* ; indexed: $($i:ident),*) => {
        impl EffectParam {
            /// This control's number; `None` past [`INDEXED`].
            #[must_use]
            pub fn code(self) -> Option<u16> {
                let plain = [$(Self::$p),*];
                if let Some(n) = plain.iter().position(|&p| p == self) {
                    return u16::try_from(n).ok();
                }
                let mut base = u16::try_from(plain.len()).ok()?;
                $(
                    if let Self::$i(k) = self {
                        return (k < INDEXED).then(|| base + u16::from(k));
                    }
                    base += u16::from(INDEXED);
                )*
                let _ = base;
                None
            }

            /// The control numbered `code` ([`Self::code`]).
            #[must_use]
            pub fn from_code(code: u16) -> Option<Self> {
                let plain = [$(Self::$p),*];
                if let Some(&p) = plain.get(usize::from(code)) {
                    return Some(p);
                }
                let mut c = code.checked_sub(u16::try_from(plain.len()).ok()?)?;
                $(
                    if c < u16::from(INDEXED) {
                        return u8::try_from(c).ok().map(Self::$i);
                    }
                    c -= u16::from(INDEXED);
                )*
                let _ = c;
                None
            }
        }
    };
}

/// Indexed controls (bands, stages, matrix cells) numbered per index.
pub const INDEXED: u8 = 8;

param_codes!(
    plain: FormantTalk, FormantSharp, FormantSize, ShaperAmount, ShaperLowpassHz, CrushRate,
    CrushBits, CompressorThreshold, CompressorRatio, CompressorAttack, CompressorRelease,
    CompressorMakeup, CompressorMix, CompressorKnee, ModRate, ModPhase, PhaserExponent,
    PhaserFeedback, TransientAttack, TransientSustain, TransientGain, ReverbSeconds,
    ReverbDamping, ReverbStereo, ReverbHighCut, ReverbPredelay, LimiterInput, LimiterRelease,
    FilterHz, FilterGain, FilterLastDamping, CascadeHz, CascadeGain, ClockedCutoff,
    ClockedResonance, DelayMs, DelayDampingHz, DelayLowCutHz, DelayShelfHz, DelayShelfDb,
    Output, Dry;
    indexed: BellOctaves, BellHz, BellQ, BellGain, FilterDamping, CascadeShift, CascadeQ,
    CascadeLimit, CascadePeak, CascadeMorph, CascadeWet, DelayInput, DelayLoop
);

impl EffectParam {
    /// This control's value on `effect`, the slot's levels included.
    pub fn value(self, effect: &mut Effect) -> Option<f32> {
        match self {
            Self::Output => Some(effect.output),
            Self::Dry => Some(effect.dry),
            _ => self.field_mut(&mut effect.kind).map(|v| *v),
        }
    }

    /// Sets this control of `effect` to `value`; false when it has none.
    pub fn set(self, effect: &mut Effect, value: f32) -> bool {
        let at = match self {
            Self::Output => &mut effect.output,
            Self::Dry => &mut effect.dry,
            _ => match self.field_mut(&mut effect.kind) {
                Some(at) => at,
                None => return false,
            },
        };
        *at = value;
        true
    }
}

/// What a script's 0..=1 knob sets, through its format's law.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KnobTarget {
    /// A control of the effect in the addressed slot.
    Effect(EffectParam),
    /// A free-running LFO's Hz. The row after it, when it is
    /// [`Self::LfoSteps`], sets a stepped LFO instead.
    LfoRate,
    /// A [`crate::Rate::Steps`] LFO's count.
    LfoSteps,
    /// A pulse or Multi LFO's pulse width.
    LfoWidth,
    /// An LFO's start phase, in cycles.
    LfoPhase,
    /// One of a [`crate::Wave::Multi`]'s weights, by its index there.
    LfoWeight(u8),
    /// A control whose law depends on the slot's effect: the slot's own
    /// [`SlotLaw`]s map the knob onto its fields.
    Slot(SlotKnob),
    /// How the addressed convolution's response is prepared.
    Impulse(crate::ImpulseKnob),
    /// The addressed group's [`crate::Group::speed`].
    Speed,
}

/// A knob whose law the effect in the slot sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotKnob {
    Cutoff,
    Resonance,
    Attack,
    Release,
    /// A compressor's threshold, ratio, input drive and make-up knobs.
    Threshold,
    Ratio,
    Input,
    Makeup,
    /// The wet share of an effect whose knobs a [`KnobModel`] reads.
    Mix,
    /// A reverb's knobs.
    ReverbPredelay,
    ReverbSize,
    ReverbColour,
    ReverbStereo,
    ReverbDamping,
    /// An EQ band's frequency, bandwidth and gain knobs, by band.
    BandHz(u8),
    BandWidth(u8),
    BandGain(u8),
    /// An EQ band's switch between its shelf and a bell, by band.
    BandBell(u8),
    /// A delay's time, feedback, pan, damping, low-cut and high-cut knobs.
    DelayTime,
    DelayFeedback,
    DelayPan,
    DelayDamping,
    DelayLowCut,
    DelayHighCut,
    /// A [`crate::Cascade`]'s knobs: by stage its shift (stages 1 and 2),
    /// resonance (stages 1 and 2; stage 0 is [`Self::Resonance`]), morph and
    /// wet; its output gain.
    StageShift(u8),
    StageResonance(u8),
    StageMorph(u8),
    StageWet(u8),
    OutputGain,
    /// A [`KnobModel`]'s knob with no name shared across effects, by its
    /// place in that model.
    Knob(u8),
}

/// The rack a [`SlotLaw`]'s slot is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RackAt {
    /// A group's inserts, by group index.
    Group(u32),
    /// The instrument's inserts.
    Inserts,
    /// The instrument's send effects, by send slot.
    Sends,
    /// The instrument's main rack.
    Main,
    /// An instrument bus's inserts, by the format's bus number.
    Bus(u8),
}

/// Knob `knob` on slot `slot` of `at` sets the effect's `field` to `law(x)`;
/// a knob may move several fields.
#[derive(Clone, Debug, PartialEq)]
pub struct SlotLaw {
    pub at: RackAt,
    pub slot: u8,
    pub knob: SlotKnob,
    pub field: EffectParam,
    pub law: crate::Law,
}

/// A slot whose knobs interact: the effect is rebuilt from all of them
/// whenever a script moves one, so the result does not depend on the order
/// they were set in, and each reads back exactly as set.
#[derive(Clone, Debug, PartialEq)]
pub struct KnobModel {
    /// Identifier of the registered [`crate::EffectModel`] that evaluates
    /// [`Self::saved`] and live knob positions.
    pub id: crate::ModelId,
    /// The knobs, in the order the model reads their positions.
    pub knobs: Box<[SlotKnob]>,
    /// The saved positions, then any fixed settings the model reads after them.
    pub saved: Box<[f32]>,
}

impl KnobModel {
    /// The most positions a model holds: its live positions fit a fixed
    /// array, so moving a knob allocates nothing.
    pub const MAX: usize = 24;

    /// Evaluates the registered model's [`crate::EffectModel::build`] on `params`.
    #[inline]
    #[must_use]
    pub fn build(&self, params: &[f32]) -> Option<crate::EffectKind> {
        crate::eval_model(self.id, params)
    }

    /// Evaluates the registered model's [`crate::EffectModel::build_effect`] on `params`.
    #[inline]
    #[must_use]
    pub fn build_effect(&self, params: &[f32], current: &Effect) -> Option<Effect> {
        crate::eval_model_effect(self.id, params, current)
    }
}

/// Shape `id` a script may switch a filter slot
/// ([`SlotShapes::slots`]) to: its pass, stages, ladder taps and
/// ranges come from `filter`. The resonance moves to the new last stage, the
/// others stay critically damped.
#[derive(Clone, Debug, PartialEq)]
pub struct SlotShape {
    pub id: i32,
    pub filter: crate::Filter,
    /// The shape's own laws for the slot's knobs; empty when the slot's hold.
    pub laws: std::sync::Arc<[(SlotKnob, EffectParam, crate::Law)]>,
}

/// The filter shapes a script may switch slots to, the slots that switch,
/// and the effects a script may load into any slot.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SlotShapes {
    pub shapes: Vec<SlotShape>,
    /// Filter slots that may switch to any of [`Self::shapes`].
    pub slots: Vec<(RackAt, u8)>,
    pub types: Vec<SlotType>,
    /// Each filter slot's saved subtype, as a script reads it back.
    pub subtypes: Vec<(RackAt, u8, i32)>,
    /// Each saved slot's effect type, as a script reads it back.
    pub saved_types: Vec<(RackAt, u8, i32)>,
    /// The saved values of opaque slot controls not backed by a processor
    /// ([`crate::EnginePar::Opaque`]): slot, [`crate::held_key`], raw value.
    pub held: Vec<(RackAt, u8, u32, i32)>,
    /// The held controls a script-loaded type starts with: effect type id
    /// ([`SlotType::id`]), [`crate::held_key`], raw value.
    pub fresh: Vec<(i32, u32, i32)>,
    /// How a script-loaded convolution normalizes the first response it
    /// loads (its fresh Auto Gain); `None` plays it as recorded.
    pub fresh_normalize: Option<crate::Normalize>,
}

impl SlotShapes {
    pub const NONE: Self = Self {
        shapes: Vec::new(),
        slots: Vec::new(),
        types: Vec::new(),
        subtypes: Vec::new(),
        saved_types: Vec::new(),
        held: Vec::new(),
        fresh: Vec::new(),
        fresh_normalize: None,
    };

    /// How a script-made convolution ([`crate::Impulse::NONE`]) prepares the
    /// first response it loads.
    #[must_use]
    pub fn fresh_impulse(&self) -> crate::Impulse {
        crate::Impulse {
            normalize: self.fresh_normalize,
            ..crate::Impulse::PLAIN
        }
    }
}

/// An effect a script loads into a slot by the format's type `id` (or a
/// filter `subtype` of another kind): a new unit's settings, nothing for an
/// empty slot, and the laws its knobs then move it by.
#[derive(Clone, Debug, PartialEq)]
pub struct SlotType {
    pub id: i32,
    pub subtype: bool,
    /// The subtype a script reads back from the loaded slot.
    pub reads: i32,
    /// The effect type a script reads back from it: `id`, or for a
    /// subtype the type of effect it is a subtype of.
    pub kind: i32,
    pub effect: Option<crate::Effect>,
    pub laws: std::sync::Arc<[(SlotKnob, EffectParam, crate::Law)]>,
    /// How its knobs rebuild it together, when they interact.
    pub model: Option<KnobModel>,
}

impl SlotShape {
    /// Switches `filter` to this shape.
    pub fn apply(&self, filter: &mut crate::Filter) {
        let last = |f: &crate::Filter| usize::from(f.stages).checked_sub(1);
        let resonance = last(filter).and_then(|n| filter.damping.get(n).copied());
        let to = &self.filter;
        (filter.pass, filter.stages, filter.ladder) = (to.pass, to.stages, to.ladder);
        (filter.octaves, filter.q_octaves) = (to.octaves, to.q_octaves);
        filter.damping = [2.0; 3];
        if let Some((n, k)) = last(filter).zip(resonance)
            && let Some(d) = filter.damping.get_mut(n)
        {
            *d = k;
        }
    }
}
