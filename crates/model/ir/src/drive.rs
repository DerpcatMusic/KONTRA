//! Nonlinear colour in physical terms: a waveshaper, a reducer and an
//! overdrive. Each runs on each voice when it sits in a group rack. [`EffectParam`] names the
//! effect controls a route can move.

use crate::EffectKind;

/// A transfer curve and its amount (the engine's `drive::Curve`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve {
    /// −1..=1: above 0 a soft clip, below 0 a cubic expansion.
    Soft(f32),
    /// 0..=1: an asymmetric rational clip.
    Tube(f32),
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
}

impl Curve {
    /// Its amount or drive.
    pub fn amount_mut(&mut self) -> &mut f32 {
        match self {
            Self::Soft(x)
            | Self::Tube(x)
            | Self::Power(x)
            | Self::Sine(x)
            | Self::Fold(x)
            | Self::Tanh(x)
            | Self::Atan(x) => x,
        }
    }
}

/// An optional one-pole low-pass, a curve, then an optional one-pole
/// low-pass and a second-order Butterworth high-pass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shaper {
    /// The corner of a one-pole low-pass before the curve, pole
    /// `e^(−2π·hz/rate)` (no zero at Nyquist).
    pub pre_lowpass_hz: Option<f32>,
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

/// An overdrive pedal (the engine's `drive::Overdrive`): one-pole splits
/// around a clipping curve at four times the rate, the shaped band fed back
/// into the input split and mixed with the dry one through a tone low-pass.
#[derive(Clone, Copy, Debug, PartialEq)]
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
    pub curve: &'static [f32; 256],
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

/// An effect control a route or a script moves, in the effect's own units:
/// a [`crate::Formant`] knob, a curve's amount, a [`Crush`] rate or bits, a
/// dynamics or modulation effect's physical setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    /// A [`crate::Delay`]'s time in ms (not a synced one), its input and
    /// feedback matrix cells (row-major, 0..4) and its damping corner.
    DelayMs,
    DelayInput(u8),
    DelayLoop(u8),
    DelayDampingHz,
}

impl EffectParam {
    /// This control of `kind`; `None` when `kind` has none.
    pub fn field_mut(self, kind: &mut EffectKind) -> Option<&mut f32> {
        Some(match (self, kind) {
            (Self::FormantTalk, EffectKind::Formant(f)) => &mut f.talk,
            (Self::FormantSharp, EffectKind::Formant(f)) => &mut f.sharp,
            (Self::FormantSize, EffectKind::Formant(f)) => &mut f.size,
            (Self::ShaperAmount, EffectKind::Shaper(s)) => s.curve.amount_mut(),
            (Self::ShaperLowpassHz, EffectKind::Shaper(s)) => s.lowpass_hz.as_mut()?,
            (Self::CrushRate, EffectKind::Crush(c)) => &mut c.rate,
            (Self::CrushBits, EffectKind::Crush(c)) => &mut c.bits,
            (Self::CompressorThreshold, EffectKind::Compressor(c)) => &mut c.threshold_db,
            (Self::CompressorRatio, EffectKind::Compressor(c)) => &mut c.ratio,
            (Self::CompressorAttack, EffectKind::Compressor(c)) => &mut c.attack_ms,
            (Self::CompressorRelease, EffectKind::Compressor(c)) => &mut c.release_ms,
            (Self::ModRate, EffectKind::Delay(crate::Delay { rate, .. }))
            | (Self::ModRate, EffectKind::Phaser(crate::Phaser { rate, .. })) => match rate {
                crate::Rate::Hz(hz) => hz,
                crate::Rate::Beats(_) | crate::Rate::Steps { .. } => return None,
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
            _ => return None,
        })
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
    /// One of a [`crate::Wave::Multi`]'s weights, by its index there.
    LfoWeight(u8),
    /// A control whose law depends on the slot's effect: the slot's own
    /// [`SlotLaw`]s map the knob onto its fields.
    Slot(SlotKnob),
}

/// A knob whose law the effect in the slot sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotKnob {
    Cutoff,
    Resonance,
    Attack,
    Release,
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
    /// A delay's time, feedback, pan and damping knobs.
    DelayTime,
    DelayFeedback,
    DelayPan,
    DelayDamping,
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

/// Shape `id` a script may switch the filter in slot `slot` of `at` to:
/// its pass, stages, ladder taps and ranges come from `filter`. The
/// resonance moves to the new last stage, the others stay critically damped.
#[derive(Clone, Debug, PartialEq)]
pub struct SlotShape {
    pub at: RackAt,
    pub slot: u8,
    pub id: i32,
    pub filter: crate::Filter,
    /// The shape's own laws for the slot's knobs; empty when the slot's hold.
    pub laws: std::sync::Arc<[(SlotKnob, EffectParam, crate::Law)]>,
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
