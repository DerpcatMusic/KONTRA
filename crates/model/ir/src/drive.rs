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

/// An effect control a route moves, in the effect's own units: a
/// [`crate::Formant`] knob, a curve's amount, a [`Crush`] rate or bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectParam {
    FormantTalk,
    FormantSharp,
    FormantSize,
    /// A [`Shaper`]'s curve amount.
    ShaperAmount,
    CrushRate,
    CrushBits,
    /// Octaves on an [`EffectKind::Eq`] band's centre, kept within 20 Hz to
    /// 20 kHz.
    BellOctaves(u8),
}

impl EffectParam {
    /// Adds `delta` to this control of `kind`; false when `kind` has none.
    pub fn add(self, kind: &mut EffectKind, delta: f32) -> bool {
        let at = match (self, kind) {
            (Self::FormantTalk, EffectKind::Formant(f)) => &mut f.talk,
            (Self::FormantSharp, EffectKind::Formant(f)) => &mut f.sharp,
            (Self::FormantSize, EffectKind::Formant(f)) => &mut f.size,
            (Self::ShaperAmount, EffectKind::Shaper(s)) => match &mut s.curve {
                Curve::Soft(x) | Curve::Tube(x) | Curve::Power(x) => x,
            },
            (Self::CrushRate, EffectKind::Crush(c)) => &mut c.rate,
            (Self::CrushBits, EffectKind::Crush(c)) => &mut c.bits,
            (Self::BellOctaves(band), EffectKind::Eq(bells)) => {
                let Some(b) = bells.get_mut(usize::from(band)) else {
                    return false;
                };
                b.hz = (b.hz * delta.exp2()).clamp(20.0, 20_000.0);
                return true;
            }
            _ => return false,
        };
        *at += delta;
        true
    }
}
