//! DSP for the effects Kontakt stores as plain value lists
//! ([`Params::Fields`]): dynamics, delays, modulation, drive and Lo-Fi, plus
//! the rack Filter/EQ and Solid G-EQ (the group filter's sections). Kontakt's algorithms and most of
//! its knob laws are not public; each block reproduces the controls with a
//! standard topology, and the laws (confidence in `audits/EFFECTS.md`) are
//! the constants below.
//!
//! [`Drive`] is the per-sample nonlinear part, small and `Copy`, so group
//! insert racks run it per voice too. [`Block`] owns a rack slot's state;
//! [`Block::new`] allocates (delay lines), the rest never does.
//!
//! Values are kept as stored (`Fields`, layout order). Scripts set them by
//! `$ENGINE_PAR_*` normalized 0..=1; [`PARS`] maps each name to its effect,
//! field and law.

use super::{Effect, Kind, Params, params::Value};
use crate::engine::filter::RackFilter;
use std::f32::consts::{PI, TAU};

/// Values a block keeps: the longest layout (Solid G-EQ) has 12.
pub(crate) const FIELDS: usize = 12;
pub(crate) type Fields = [f32; FIELDS];

/// A stored layout's values as numbers (flags 0/1), in layout order.
pub(crate) fn fields(params: &Params) -> Option<Fields> {
    let Params::Fields(list) = params else {
        return None;
    };
    let mut out = [0.0; FIELDS];
    for (o, f) in out.iter_mut().zip(list) {
        *o = match f.value {
            Value::Number(x) => x,
            Value::Flag(b) => f32::from(u8::from(b)),
        };
    }
    Some(out)
}

/// How a script's normalized value maps onto a stored one.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Law {
    /// Stored normalized.
    Norm,
    /// Stored in units, linear from `lo` to `hi`.
    Lin(f32, f32),
    /// Stored in units, `max · x³`.
    Cube(f32),
    /// Stored in units, `lo · (hi/lo)^x`.
    Log(f32, f32),
    /// Legacy Delay's native shifted exponential, stored in milliseconds.
    DelayTime,
    /// This implementation's documented NI_SYNC_UNIT names → stored beats.
    DelayUnit,
    /// The script's raw value (`$NI_SYNC_UNIT_*`), not scaled by 1e6.
    Raw,
}

impl Law {
    fn stored(self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        match self {
            Law::Norm => x,
            Law::Lin(lo, hi) => lo + (hi - lo) * x,
            Law::Cube(max) => max * x * x * x,
            Law::Log(lo, hi) => lo * (hi / lo).powf(x),
            Law::DelayTime => (6.0272455 * x + 1.9459101).exp() - 2.0,
            Law::DelayUnit => DELAY_UNITS.get((x * 1e6).round() as usize).copied().unwrap_or(f32::NAN),
            Law::Raw => (x * 1e6).round(),
        }
    }

    fn norm(self, v: f32) -> f32 {
        let x = match self {
            Law::Norm => v,
            Law::Lin(lo, hi) => (v - lo) / (hi - lo),
            Law::Cube(max) => (v / max).max(0.0).cbrt(),
            Law::Log(lo, hi) => (v.max(lo) / lo).ln() / (hi / lo).ln(),
            Law::DelayTime => ((v.max(5.0) + 2.0).ln() - 1.9459101) * 0.16591327,
            Law::DelayUnit => if v <= 0.0 { 0.0 } else { DELAY_UNITS.iter().position(|b| (*b - v).abs() < 1e-6).map_or(f32::NAN, |n| n as f32 / 1e6) },
            Law::Raw => v.max(0.0) / 1e6,
        };
        x.clamp(0.0, 1.0)
    }
}

/// Compressor threshold, -60..=0 dB.
const THRESHOLD: Law = Law::Lin(-60.0, 0.0);
/// Compressor attack and release (ms).
const ATTACK: Law = Law::Cube(1000.0);
const RELEASE: Law = Law::Cube(5000.0);
/// Legacy Delay time: 5..=2900 ms. Saved fields already contain milliseconds;
/// only script writes/readback use this conversion.
const DELAY_TIME: Law = Law::DelayTime;
/// Delay line length (seconds), past the longest time.
const MAX_DELAY_S: f32 = 2.91;
// These IDs are our named KSP constants, not aliases of a private native enum.
const DELAY_UNITS: [f32; 16] = [-1.0, 4.0, 8.0/3.0, 2.0, 4.0/3.0, 1.0, 2.0/3.0,
    0.5, 1.0/3.0, 0.25, 1.0/6.0, 0.125, 1.0/12.0, 0.0625, 1.0/24.0, 0.015625];

/// `$ENGINE_PAR_*` effect parameters: effect, field (layout position), law.
const PARS: &[(&str, Kind, u8, Law)] = &[
    ("$ENGINE_PAR_THRESHOLD", Kind::Compressor, 1, THRESHOLD),
    ("$ENGINE_PAR_RATIO", Kind::Compressor, 2, Law::Norm),
    ("$ENGINE_PAR_COMP_ATTACK", Kind::Compressor, 3, ATTACK),
    ("$ENGINE_PAR_COMP_DECAY", Kind::Compressor, 4, RELEASE),
    // ANALOG STRINGS' script sets in-gain 500011 and release 0 on a slot
    // storing 0.00053 dB and 10 ms: (0.500011 - 0.5) · 48 = 0.00053, so
    // -24..=+24 dB (high confidence); release starts at 10 ms, its top
    // and curve are a guess.
    ("$ENGINE_PAR_LIM_IN_GAIN", Kind::Limiter, 0, Law::Lin(-24.0, 24.0)),
    ("$ENGINE_PAR_LIM_RELEASE", Kind::Limiter, 1, Law::Log(10.0, 1000.0)),
    ("$ENGINE_PAR_DL_TIME", Kind::Delay, 0, DELAY_TIME),
    ("$ENGINE_PAR_DL_DAMPING", Kind::Delay, 1, Law::Norm),
    ("$ENGINE_PAR_DL_PAN", Kind::Delay, 2, Law::Norm),
    ("$ENGINE_PAR_DL_FEEDBACK", Kind::Delay, 3, Law::Norm),
    ("$ENGINE_PAR_DL_TIME_UNIT", Kind::Delay, 4, Law::DelayUnit),
    ("$ENGINE_PAR_CH_SPEED_UNIT", Kind::Chorus, 3, Law::Raw),
    ("$ENGINE_PAR_FL_SPEED_UNIT", Kind::Flanger, 5, Law::Raw),
    ("$ENGINE_PAR_PH_SPEED_UNIT", Kind::Phaser, 4, Law::Raw),
    ("$ENGINE_PAR_CH_DEPTH", Kind::Chorus, 0, Law::Norm),
    ("$ENGINE_PAR_CH_SPEED", Kind::Chorus, 1, Law::Norm),
    ("$ENGINE_PAR_CH_PHASE", Kind::Chorus, 2, Law::Norm),
    ("$ENGINE_PAR_FL_DEPTH", Kind::Flanger, 0, Law::Norm),
    ("$ENGINE_PAR_FL_SPEED", Kind::Flanger, 1, Law::Norm),
    ("$ENGINE_PAR_FL_PHASE", Kind::Flanger, 2, Law::Norm),
    ("$ENGINE_PAR_FL_FEEDBACK", Kind::Flanger, 3, Law::Norm),
    ("$ENGINE_PAR_FL_COLOR", Kind::Flanger, 4, Law::Norm),
    ("$ENGINE_PAR_PH_DEPTH", Kind::Phaser, 0, Law::Norm),
    ("$ENGINE_PAR_PH_FEEDBACK", Kind::Phaser, 1, Law::Norm),
    ("$ENGINE_PAR_PH_SPEED", Kind::Phaser, 2, Law::Norm),
    ("$ENGINE_PAR_PH_PHASE", Kind::Phaser, 3, Law::Norm),
    ("$ENGINE_PAR_SHAPE", Kind::SurroundPanner, 0, Law::Lin(-1.0, 1.0)),
    ("$ENGINE_PAR_DRIVE", Kind::Distortion, 1, Law::Norm),
    ("$ENGINE_PAR_DAMPING", Kind::Distortion, 2, Law::Norm),
    ("$ENGINE_PAR_BITS", Kind::LoFi, 0, Law::Norm),
    ("$ENGINE_PAR_FREQUENCY", Kind::LoFi, 1, Law::Norm),
    ("$ENGINE_PAR_NOISELEVEL", Kind::LoFi, 2, Law::Norm),
    ("$ENGINE_PAR_NOISECOLOR", Kind::LoFi, 4, Law::Norm),
    ("$ENGINE_PAR_SK_TONE", Kind::Skreamer, 0, Law::Norm),
    ("$ENGINE_PAR_SK_DRIVE", Kind::Skreamer, 1, Law::Norm),
    ("$ENGINE_PAR_SK_BASS", Kind::Skreamer, 2, Law::Norm),
    ("$ENGINE_PAR_SK_BRIGHT", Kind::Skreamer, 3, Law::Norm),
    ("$ENGINE_PAR_SK_MIX", Kind::Skreamer, 4, Law::Norm),
    ("$ENGINE_PAR_TP_GAIN", Kind::TapeSaturator, 0, Law::Norm),
    ("$ENGINE_PAR_TP_WARMTH", Kind::TapeSaturator, 1, Law::Norm),
    ("$ENGINE_PAR_TP_HF_ROLLOFF", Kind::TapeSaturator, 2, Law::Norm),
    ("$ENGINE_PAR_TR_INPUT", Kind::TransientMaster, 0, Law::Norm),
    ("$ENGINE_PAR_TR_ATTACK", Kind::TransientMaster, 1, Law::Norm),
    ("$ENGINE_PAR_TR_SUSTAIN", Kind::TransientMaster, 2, Law::Norm),
    ("$ENGINE_PAR_TR_SMOOTH", Kind::TransientMaster, 3, Law::Norm),
    ("$ENGINE_PAR_SEQ_LF_GAIN", Kind::SolidGeq, 0, Law::Norm),
    ("$ENGINE_PAR_SEQ_LF_FREQ", Kind::SolidGeq, 1, Law::Norm),
    ("$ENGINE_PAR_SEQ_LF_BELL", Kind::SolidGeq, 2, Law::Norm),
    ("$ENGINE_PAR_SEQ_LMF_GAIN", Kind::SolidGeq, 3, Law::Norm),
    ("$ENGINE_PAR_SEQ_LMF_FREQ", Kind::SolidGeq, 4, Law::Norm),
    ("$ENGINE_PAR_SEQ_LMF_Q", Kind::SolidGeq, 5, Law::Norm),
    ("$ENGINE_PAR_SEQ_HMF_GAIN", Kind::SolidGeq, 6, Law::Norm),
    ("$ENGINE_PAR_SEQ_HMF_FREQ", Kind::SolidGeq, 7, Law::Norm),
    ("$ENGINE_PAR_SEQ_HMF_Q", Kind::SolidGeq, 8, Law::Norm),
    ("$ENGINE_PAR_SEQ_HF_GAIN", Kind::SolidGeq, 9, Law::Norm),
    ("$ENGINE_PAR_SEQ_HF_FREQ", Kind::SolidGeq, 10, Law::Norm),
    ("$ENGINE_PAR_SEQ_HF_BELL", Kind::SolidGeq, 11, Law::Norm),
    ("$ENGINE_PAR_SCOMP_THRESHOLD", Kind::SolidBusComp, 0, Law::Norm),
    ("$ENGINE_PAR_SCOMP_RATIO", Kind::SolidBusComp, 1, Law::Norm),
    ("$ENGINE_PAR_SCOMP_ATTACK", Kind::SolidBusComp, 2, Law::Norm),
    ("$ENGINE_PAR_SCOMP_RELEASE", Kind::SolidBusComp, 3, Law::Norm),
    ("$ENGINE_PAR_SCOMP_MAKEUP", Kind::SolidBusComp, 4, Law::Norm),
    ("$ENGINE_PAR_SCOMP_MIX", Kind::SolidBusComp, 5, Law::Norm),
    ("$ENGINE_PAR_FCOMP_INPUT", Kind::FeedbackCompressor, 0, Law::Norm),
    ("$ENGINE_PAR_FCOMP_RATIO", Kind::FeedbackCompressor, 1, Law::Norm),
    ("$ENGINE_PAR_FCOMP_ATTACK", Kind::FeedbackCompressor, 2, Law::Norm),
    ("$ENGINE_PAR_FCOMP_RELEASE", Kind::FeedbackCompressor, 3, Law::Norm),
    ("$ENGINE_PAR_FCOMP_MAKEUP", Kind::FeedbackCompressor, 4, Law::Norm),
    ("$ENGINE_PAR_FCOMP_MIX", Kind::FeedbackCompressor, 5, Law::Norm),
];

/// The effect and field a `$ENGINE_PAR_*` name sets.
pub fn engine_par(name: &str) -> Option<(Kind, u8)> {
    PARS.iter().find(|p| p.0 == name).map(|p| (p.1, p.2))
}

fn law(kind: Kind, field: u8) -> Law {
    PARS.iter().find(|p| (p.1, p.2) == (kind, field)).map_or(Law::Norm, |p| p.3)
}

/// A script's normalized value as `kind`'s stored `field`.
pub(crate) fn stored(kind: Kind, field: u8, x: f32) -> f32 {
    law(kind, field).stored(x)
}

/// A stored value as the script reads it, 0..=1.
pub(crate) fn normalized(kind: Kind, field: u8, v: f32) -> f32 {
    law(kind, field).norm(v)
}

/// Saved Delay Time can be milliseconds, or a count of the saved beat unit.
/// Legacy records retain an absolute-control law until the unit is changed.
pub(crate) fn normalized_field(kind: Kind, field: u8, f: &Fields, tempo: f32) -> Option<f32> {
    let v = *f.get(field as usize)?;
    let x = if kind == Kind::Delay && field == 0 && f[4] > 0.0 {
        if f[7] == 0.0 { ((v - 1.0) / 11.0).clamp(0.0, 1.0) }
        else { DELAY_TIME.norm(delay_ms(f, tempo)) }
    } else { normalized(kind, field, v) };
    x.is_finite().then_some(x)
}

/// Update the existing native caches before changing Time mode; also used by
/// queued getters and off-thread init so they read the same state as the DSP.
pub(crate) fn set_delay_field(f: &mut Fields, field: u8, x: f32, tempo: f32) -> bool {
    if field > 4 || !x.is_finite() { return false }
    if field == 4 {
        if x < 0.0 { return false }
        let unit = stored(Kind::Delay, field, x);
        if !unit.is_finite() { return false }
        if unit != f[4] {
            let old_sync = f[4] > 0.0;
            if f[7] != 0.0 {
                if old_sync { f[0] = f[0].clamp(1.0, 12.0); }
                f[7] = 0.0;
            }
            f[5 + usize::from(old_sync)] = f[0];
            f[4] = unit;
            f[0] = f[5 + usize::from(unit > 0.0)];
        }
    } else {
        f[field as usize] = if field == 0 && f[4] > 0.0 {
            if f[7] == 0.0 { (1.0 + 11.0 * x.clamp(0.0, 1.0)).round() }
            else { (DELAY_TIME.stored(x) * tempo / (60_000.0 * f[4])).round().max(1.0) }
        } else { stored(Kind::Delay, field, x) };
    }
    true
}

pub(crate) fn delay_display(f: &Fields, value: i32) -> Option<crate::engine::Disp> {
    (f[4] > 0.0 && f[7] == 0.0).then(||
        crate::engine::Disp::Num((1.0 + 11.0 * (value as f32 / 1e6).clamp(0.0, 1.0)).round(), 0))
}

fn delay_ms(f: &Fields, tempo: f32) -> f32 {
    if f[4] <= 0.0 { f[0] }
    else { (f[0].max(1.0) * f[4] * (60_000.0 / tempo)).clamp(0.0, 5800.0) }
}


/// Values a freshly loaded effect starts with: the ones local presets keep
/// most (Kontakt's own defaults are not stored anywhere).
pub(crate) fn defaults(kind: Kind) -> Option<&'static [f32]> {
    Some(match kind {
        Kind::Compressor => &[0.0, -14.0, 0.5, 26.0, 200.0, 1.0],
        Kind::Limiter => &[0.0, 10.0],
        Kind::Delay => &[250.0, 0.3, 1.0, 0.2, -1.0, 0.0, 1.0, 1.0],
        Kind::Chorus => &[0.5, 0.77, 0.44, -1.0, 0.0, 1.0, 1.0],
        Kind::Flanger => &[0.5, 0.5, 0.5, 0.25, 1.5, -1.0, 0.0, 1.0, 1.0],
        Kind::Phaser => &[0.5, 0.4, 0.5, 0.5, -1.0, 0.0, 1.0, 1.0],
        Kind::SurroundPanner => &[0.0, 0.0],
        Kind::Distortion => &[0.0, 0.5, 0.0],
        Kind::LoFi => &[0.5, 0.1, 0.0, 0.0, 0.2],
        Kind::Skreamer => &[0.7, 0.5, 0.2, 0.9, 0.0],
        Kind::TapeSaturator => &[0.33, 0.5, 0.3, 1.0],
        Kind::TransientMaster => &[0.5, 0.5, 0.5, 0.0],
        Kind::SolidGeq => &[0.5, 0.5, 0.0, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.0],
        Kind::SolidBusComp => &[0.3, 0.3, 0.167, 0.167, 0.167, 1.0, 1.0, 0.0, 0.63],
        Kind::FeedbackCompressor => &[0.3, 0.8, 0.45, 0.2, 0.2, 1.0, 0.0, 0.0, 1.0, 0.0],
        _ => return None,
    })
}

fn db(x: f32) -> f32 {
    10f32.powf(x / 20.0)
}

/// One-pole low-pass coefficient for `hz`.
fn one_pole(hz: f32, rate: f32) -> f32 {
    1.0 - (-TAU * hz.min(0.45 * rate) / rate).exp()
}

/// `tanh` to within 2.5% (Padé), exact ±1 past ±3: cheap enough per voice.
#[inline(always)]
fn soft(x: f32) -> f32 {
    let x = x.clamp(-3.0, 3.0);
    x * (27.0 + x * x) / (27.0 + 9.0 * x * x)
}

/// Native Tube Distortion scalar core; filters and Output follow it.
#[inline(always)]
fn tube_curve(x: f32, k: f32, negative: f32, positive: f32) -> f32 {
    if x < 0.0 {
        if negative == 0.0 { return x; }
        let r = ((k - x) * x / (1.0 + (x - (k - 1.0)) * x)).clamp(-1.0, 1.0);
        negative * (r * r * r) + (1.0 - negative) * x
    } else {
        if positive == 0.0 { return x; }
        let r = ((k + x) * x / (1.0 + (x + k - 1.0) * x)).clamp(-1.0, 1.0);
        let complement = 1.0 - r;
        positive * (1.0 - complement * complement * complement) + (1.0 - positive) * x
    }
}

/// Native Transistor core. The low-level joins avoid powers, and the
/// negative/positive power branches retain native float/double precision.
#[inline(always)]
fn transistor_curve(x: f32, drive: f32, power: f32, threshold: f32, scale: f32, inverse_power: f64) -> f32 {
    if drive == 0.0 || x >= 0.25 || x < -0.25 { return x; }
    if x < 0.0 {
        let magnitude = -x;
        let shaped = if magnitude < threshold { magnitude * scale } else { 0.25 * (4.0 * magnitude).powf(power) };
        (1.0 - drive) * x - drive * shaped
    } else {
        let shaped = if x <= 1.0 / 4096.0 { x / scale } else { 0.25 * f64::from(4.0 * x).powf(inverse_power) as f32 };
        (1.0 - drive) * x + drive * shaped
    }
}

/// Keeps decaying feedback out of subnormal floats.
const ANTI_DENORMAL: f32 = 1e-20;

/// Native steady-state Distortion damping pole. The normalized cutoff uses
/// an exponential polynomial followed by a sine-ratio bilinear transform.
/// Native Damping parameter smoothing remains unmodelled.
fn distortion_damping(damping: f32, rate: f32) -> f32 {
    let p = (1.0 - 0.4 * damping.clamp(0.0, 1.0)).clamp(0.0, 1.0);
    let x = (f64::from(p) * 6.15) as f32;
    let exp = 1.0 + x * (1.0 + x * (0.5 + x * (1.0 / 6.0 + x * (1.0 / 24.0
        + x * (1.0 / 120.0 + x * (1.0 / 720.0 + x * (1.0 / 5040.0 + x / 40320.0)))))));
    let normalized = ((exp - 1.0) * 0.0025728317).clamp(0.0, 1.0) + 0.002;
    let normalized = (normalized * (44011.97604790419 / f64::from(rate)) as f32).clamp(0.0, 1.0);
    let angle = (f64::from(normalized) * std::f64::consts::FRAC_PI_2) as f32;
    let angle = (f64::from(angle) * 0.99) as f32;
    let angle = (f64::from(angle) * 0.9987809049669) as f32;
    let sin = |z: f32| {
        let z2 = z * z;
        z * (1.0 + z2 * (-1.0 / 6.0 + z2 * (1.0 / 120.0 - z2 / 5040.0)))
    };
    let k = f64::from(sin(0.5 - angle) / sin(0.5 + angle));
    let prototype = f64::from(0.29340800642967224f32);
    ((prototype + k) / (1.0 + prototype * k)) as f32
}

/// Native fixed DC filter preparation: order-two, zero-resonance prototype,
/// all-pass frequency transform, HP sign change, then unity Nyquist gain.
fn distortion_dc(rate: f32) -> [f32; 5] {
    let tangent = 0.5f64.tan();
    let pole = -std::f64::consts::FRAC_1_SQRT_2 * 8.0 * tangent;
    let square = 4.0 * tangent * tangent;
    let norm = 1.0 / (4.0 - pole + square);
    let b = (square * norm) as f32;
    let (b0, b1, b2) = (f64::from(b), f64::from(2.0 * b), f64::from(b));
    let a1 = f64::from(((8.0 - 2.0 * square) * norm) as f32);
    let a2 = f64::from(((-4.0 - pole - square) * norm) as f32);
    let angle = (23.561944901923447 / f64::from(rate * 0.5)) as f32;
    let angle = (f64::from(angle) * 0.99713317384107) as f32;
    let cos = |z: f32| {
        let z2 = z * z;
        let z4 = z2 * z2;
        let z6 = z4 * z2;
        (z4 * (1.0 / 24.0) - z6 * (1.0 / 720.0)) + (1.0 - 0.5 * z2)
    };
    let k = f64::from(-cos(0.5 + angle) / cos(0.5 - angle));
    let k2 = k * k;
    let norm = 1.0 / (1.0 + a1 * k - a2 * k2);
    // The native transform rounds before and after denominator scaling.
    let scaled = |x: f64| (f64::from(x as f32) * norm) as f32;
    let transformed_b0 = scaled(b0 - b1 * k + b2 * k2);
    let transformed_b1 = -scaled(b1 - 2.0 * b0 * k + b1 * k2 - 2.0 * b2 * k);
    let transformed_b2 = scaled(b0 * k2 - b1 * k + b2);
    let transformed_a1 = -scaled(a1 * (k2 + 1.0) + 2.0 * k - 2.0 * a2 * k);
    let transformed_a2 = scaled(a2 - a1 * k - k2);
    let (mut b0, mut b1, mut b2, a1, a2) = (transformed_b0, transformed_b1, transformed_b2, transformed_a1, transformed_a2);
    let gain = (1.0 - f64::from(a2 - a1)) / f64::from((b0 - b1) + b2);
    b0 = (f64::from(b0) * gain) as f32;
    b1 = (f64::from(b1) * gain) as f32;
    b2 = (f64::from(b2) * gain) as f32;
    [b0, b1, b2, a1, a2]
}

/// A per-sample stereo stage: Saturation (stored as `0x1d`, Kontakt's
/// "Surround Panner" slot class but `$ENGINE_PAR_SHAPE`'s effect),
/// Distortion, Lo-Fi, Skreamer, Tape Saturator.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Drive {
    kind: DriveKind,
    c: [f32; 12],
    /// Distortion shares LP output history with HP input history (five/channel).
    /// Other drive types use only their existing leading state slots.
    s: [f32; 10],
    noise: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum DriveKind {
    #[default]
    Saturator,
    Distortion,
    LoFi,
    Skreamer,
    Tape,
}

impl Drive {
    pub(crate) fn supports(kind: Kind) -> bool {
        Self::kind_of(kind).is_some()
    }

    fn kind_of(kind: Kind) -> Option<DriveKind> {
        Some(match kind {
            Kind::SurroundPanner => DriveKind::Saturator,
            Kind::Distortion => DriveKind::Distortion,
            Kind::LoFi => DriveKind::LoFi,
            Kind::Skreamer => DriveKind::Skreamer,
            Kind::TapeSaturator => DriveKind::Tape,
            _ => return None,
        })
    }

    /// Coefficients for `kind` at `f`; the state carries on. False when
    /// `kind` is not a drive.
    pub(crate) fn tune(&mut self, kind: Kind, f: &Fields, rate: f32) -> bool {
        let Some(k) = Self::kind_of(kind) else {
            return false;
        };
        if self.noise == 0 {
            self.noise = 0x9E37_79B9;
        }
        let dc = if k == DriveKind::Distortion {
            if self.kind == k && self.c[11] == rate {
                self.c[6..11].try_into().unwrap()
            } else { distortion_dc(rate) }
        } else { [0.0; 5] };
        self.kind = k;
        let x = |i: usize| f[i].clamp(0.0, 1.0);
        self.c[..8].copy_from_slice(&match k {
            // Classic: native Shape/2 feeds the piecewise quadratic/cubic
            // kernel. Enhanced and Drums retain the existing proxy below.
            DriveKind::Saturator => {
                let s = f[0].clamp(-1.0, 1.0);
                let a = 4.0 * s;
                let q = 0.975 * (4.0 + a) + 0.1;
                [s, f[1], a, q, 1.0 / (1.0 + q), 0.0, 0.0, 0.0]
            }
            // Neither native scalar core has drive makeup. Damping uses its
            // native steady-state pole and DC filter; smoothing remains unsupported.
            DriveKind::Distortion => {
                let d = x(1);
                let a = distortion_damping(x(2), rate);
                if f[0] >= 0.5 {
                    let threshold = f64::from(8.0 * d - 12.0).exp2() as f32;
                    [d, -60.0 / (48.0 * d - 60.0), a, 1.0,
                        threshold, (1.0 / 4096.0) / threshold, 0.0, 0.0]
                } else {
                    let negative = if d > 0.75 { 1.0 } else { (0.5625 - (0.75 - d).powi(2)) * (16.0 / 9.0) };
                    let positive = if d < 0.25 { 0.0 } else { (0.5625 - (1.0 - d).powi(2)) * (16.0 / 9.0) };
                    [1.0 + 1.5 * d, 1.0, a, 0.0, negative, positive, 0.0, 0.0]
                }
            }
            // Bits 1..=32; higher native FREQUENCY means less reduction.
            // ANALOG STRINGS' authored SRate control defaults to 1M and says
            // higher values are pristine; its normalized field is not inverted.
            // ponytail: retain the 1..=64 hold proxy; exact Hz calibration,
            // Kontakt's 50 Hz floor and proprietary interpolation remain unresolved.
            DriveKind::LoFi => {
                let bits = 1.0 + 31.0 * x(0);
                let step = if bits >= 24.0 { 0.0 } else { (1.0 - bits).exp2() };
                let hold = 1.0 + 63.0 * (1.0 - x(1)).powi(3);
                let noise = if x(2) > 0.0 { db(-96.0 + 90.0 * x(2)) } else { 0.0 };
                let color = one_pole(20_000.0 * 0.02f32.powf(x(4)), rate);
                [step, 1.0 / hold, noise, color, 0.0, 0.0, 0.0, 0.0]
            }
            // A Tube Screamer: the clipped high-passed signal over the clean
            // one, a tone low-pass, `mix` read as the clean share.
            DriveKind::Skreamer => [
                one_pole(720.0 - 560.0 * x(2), rate),
                1.0 + 117.0 * x(1) * x(1),
                one_pole(600.0 * (4.5 * x(0)).exp2(), rate),
                0.5 * x(3),
                1.0 - x(4),
                x(4),
                0.0,
                0.0,
            ],
            // Gain ±12 dB into an offset tanh at unity small-signal gain,
            // then the HF roll-off, 20 kHz..=1.6 kHz.
            DriveKind::Tape => {
                let d = db(24.0 * x(0) - 12.0);
                let bias = 0.3 * x(1);
                let t = bias.tanh();
                [d, bias, t, 1.0 / (d * (1.0 - t * t)), one_pole(20_000.0 * 0.08f32.powf(x(2)), rate), 0.0, 0.0, 0.0]
            }
        });
        if k == DriveKind::Distortion {
            self.c[6..11].copy_from_slice(&dc);
            self.c[11] = rate;
        }
        true
    }

    pub(crate) fn clear(&mut self) {
        self.s = [0.0; 10];
    }

    pub(crate) fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        match self.kind {
            DriveKind::Saturator => {
                let [s, mode, a, q, inverse, ..] = self.c;
                if s == 0.0 || (mode == 0.0 && s.abs() <= 0.0001) {
                    return;
                }
                let e = s.abs();
                let curve = |x: f32| {
                    if mode == 0.0 {
                        return if s < 0.0 {
                            (x * x + q) * x * inverse
                        } else {
                            let u = (if a >= 1.0 { a * x } else { x }).abs().min(1.0);
                            let bent = (2.0 * u - u * u).copysign(x);
                            if a >= 1.0 { bent } else { (1.0 - a) * x + a * bent }
                        };
                    }
                    let bent = if s > 0.0 { 0.5 * soft(2.0 * x) } else { x * x.abs().min(1.0) };
                    x + e * (bent - x)
                };
                left.iter_mut().chain(right.iter_mut()).for_each(|x| *x = curve(*x));
            }
            DriveKind::Distortion => {
                let [drive, power, a, transistor, c4, c5, b0, b1, b2, a1, a2, ..] = self.c;
                let inverse_power = 1.0 / f64::from(power);
                for (ch, buf) in [left, right].into_iter().enumerate() {
                    let base = 5 * ch;
                    let state: [f32; 5] = self.s[base..base + 5].try_into().unwrap();
                    let [mut previous, mut lp, mut lp2, mut hp, mut hp2] = state;
                    let b = 0.5 * (1.0 - a);
                    for x in buf.iter_mut() {
                        let y = if transistor > 0.0 {
                            transistor_curve(*x, drive, power, c4, c5, inverse_power)
                        } else { tube_curve(*x, drive, c4, c5) };
                        let damped = b * y + b * previous + a * lp + ANTI_DENORMAL;
                        // Native SIMD feedforward-first order avoids the biased
                        // steady DC tail of its scalar feedback-first sum.
                        let output = (((damped * b0 + lp * b1) + lp2 * b2) + hp * a1) + hp2 * a2;
                        (previous, lp2, lp, hp2, hp) = (y, lp, damped, hp, output);
                        *x = output;
                    }
                    self.s[base..base + 5].copy_from_slice(&[previous, lp, lp2, hp, hp2]);
                }
            }
            DriveKind::LoFi => {
                let [step, rate, noise, color, ..] = self.c;
                let mut phase = self.s[4];
                let mut rng = self.noise;
                for (l, r) in left.iter_mut().zip(right.iter_mut()) {
                    phase += rate;
                    if phase >= 1.0 {
                        phase -= 1.0;
                        self.s[0] = *l;
                        self.s[1] = *r;
                    }
                    for (ch, x) in [l, r].into_iter().enumerate() {
                        let mut y = self.s[ch];
                        if step > 0.0 {
                            y = (y / step).round() * step;
                        }
                        if noise > 0.0 {
                            rng ^= rng << 13;
                            rng ^= rng >> 17;
                            rng ^= rng << 5;
                            let white = rng as i32 as f32 * (1.0 / 2_147_483_648.0);
                            self.s[2 + ch] += color * (white - self.s[2 + ch]) + ANTI_DENORMAL;
                            y += noise * self.s[2 + ch];
                        }
                        *x = y;
                    }
                }
                (self.s[4], self.noise) = (phase, rng);
            }
            DriveKind::Skreamer => {
                let [hp, gain, tone, bright, wet, clean, ..] = self.c;
                for (ch, buf) in [left, right].into_iter().enumerate() {
                    let (mut low, mut lp) = (self.s[ch], self.s[2 + ch]);
                    for x in buf.iter_mut() {
                        low += hp * (*x - low) + ANTI_DENORMAL;
                        let d = gain * (*x - low);
                        let sum = *x + 0.5 * d / (1.0 + d.abs());
                        lp += tone * (sum - lp) + ANTI_DENORMAL;
                        *x = wet * (lp + bright * (sum - lp)) + clean * *x;
                    }
                    (self.s[ch], self.s[2 + ch]) = (low, lp);
                }
            }
            DriveKind::Tape => {
                let [d, bias, t, norm, a, ..] = self.c;
                for (ch, buf) in [left, right].into_iter().enumerate() {
                    let mut lp = self.s[ch];
                    for x in buf.iter_mut() {
                        let y = (soft(d * *x + bias) - t) * norm;
                        lp += a * (y - lp) + ANTI_DENORMAL;
                        *x = lp;
                    }
                    self.s[ch] = lp;
                }
            }
        }
    }
}

/// Feed-forward (or feedback) compressor with a level detector in dB.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Comp {
    threshold: f32,
    /// `1 - 1/ratio`.
    slope: f32,
    attack: f32,
    release: f32,
    input: f32,
    makeup: f32,
    mix: f32,
    link: bool,
    feedback: bool,
    /// Detector level (dB) per channel, and the last gains (feedback).
    env: [f32; 2],
    last: [f32; 2],
}

/// Group insert DSP with fixed per-voice state; no delay lines or heap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum VoiceEffect {
    Drive(Drive),
    Comp(Comp),
    Transient(Transient),
}

impl Default for VoiceEffect {
    fn default() -> Self { Self::Drive(Drive::default()) }
}

impl VoiceEffect {
    fn dynamics(kind: Kind) -> bool {
        matches!(kind, Kind::Compressor | Kind::FeedbackCompressor | Kind::Limiter | Kind::SolidBusComp)
    }

    pub(crate) fn supports_at(kind: Kind, amp_split: Option<u8>) -> bool {
        Drive::supports(kind) || (amp_split.is_some() && (Self::dynamics(kind) || kind == Kind::TransientMaster))
    }

    pub(crate) fn tune(&mut self, kind: Kind, fields: &Fields, rate: f32) {
        if kind == Kind::TransientMaster {
            if !matches!(self, Self::Transient(_)) { *self = Self::Transient(Transient::new()); }
            if let Self::Transient(transient) = self { transient.tune(fields, rate); }
        } else if Self::dynamics(kind) {
            if !matches!(self, Self::Comp(_)) { *self = Self::Comp(Comp::new()); }
            if let Self::Comp(comp) = self { comp.tune(kind, fields, rate); }
        } else {
            if !matches!(self, Self::Drive(_)) { *self = Self::Drive(Drive::default()); }
            if let Self::Drive(drive) = self { drive.tune(kind, fields, rate); }
        }
    }

    pub(crate) fn clear(&mut self) {
        match self { Self::Drive(d) => d.clear(), Self::Comp(c) => c.clear(), Self::Transient(t) => t.clear() }
    }

    pub(crate) fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        match self { Self::Drive(d) => d.process(left, right), Self::Comp(c) => c.process(left, right), Self::Transient(t) => t.process(left, right) }
    }
}

/// Detector floor (dB).
const FLOOR_DB: f32 = -120.0;

impl Comp {
    fn new() -> Self {
        Self {
            threshold: 0.0,
            slope: 0.0,
            attack: 1.0,
            release: 1.0,
            input: 1.0,
            makeup: 1.0,
            mix: 1.0,
            link: true,
            feedback: false,
            env: [FLOOR_DB; 2],
            last: [1.0; 2],
        }
    }

    fn time(ms: f32, rate: f32) -> f32 {
        1.0 - (-1000.0 / (ms.max(0.01) * rate)).exp()
    }

    fn tune(&mut self, kind: Kind, f: &Fields, rate: f32) {
        let x = |i: usize| f[i].clamp(0.0, 1.0);
        let (threshold, ratio, attack, release) = match kind {
            // Stored in dB and ms; ratio normalized, 1..=50 (log).
            Kind::Compressor => {
                self.link = f[5] >= 0.5;
                (f[1], 50f32.powf(x(2)), f[3], f[4])
            }
            // Input gain (dB) into a 0 dBFS ceiling; release (ms). ANALOG
            // STRINGS stores 0.0005 and 10.
            Kind::Limiter => {
                self.input = db(f[0].clamp(-24.0, 24.0));
                (-0.1, 1000.0, 0.01, f[1].max(0.1))
            }
            // SSL bus compressor: threshold ±15 dB, stepped ratio, attack
            // and release; makeup 0..=+20 dB, mix.
            Kind::SolidBusComp => {
                let ratio = [2.0, 4.0, 10.0][((x(1) * 3.0) as usize).min(2)];
                let attack = [0.1, 0.3, 1.0, 3.0, 10.0, 30.0][(x(2) * 5.0).round() as usize];
                let release = [100.0, 300.0, 600.0, 1200.0, 400.0][(x(3) * 4.0).round() as usize];
                self.makeup = db(20.0 * x(4));
                self.mix = x(5);
                self.link = f[6] >= 0.5;
                (30.0 * x(0) - 15.0, ratio, attack, release)
            }
            // Feedback design: input ±20 dB into a -20 dB threshold; ratio
            // 1..=20; attack 0.1..=100 ms, release 20..=2000 ms; makeup
            // 0..=+30 dB, mix.
            Kind::FeedbackCompressor => {
                self.feedback = true;
                self.input = db(40.0 * x(0) - 20.0);
                self.makeup = db(30.0 * x(4));
                self.mix = x(5);
                self.link = f[8] >= 0.5;
                (-20.0, 1.0 + 19.0 * x(1), 0.1 * 1000f32.powf(x(2)), 20.0 * 100f32.powf(x(3)))
            }
            _ => return,
        };
        self.threshold = threshold;
        self.slope = 1.0 - 1.0 / ratio.max(1.0);
        self.attack = Self::time(attack, rate);
        self.release = Self::time(release, rate);
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let (dl, dr) = (*l * self.input, *r * self.input);
            let level = |x: f32| if x.abs() > 1e-6 { 20.0 * x.abs().log10() } else { FLOOR_DB };
            let (sl, sr) = if self.feedback { (dl * self.last[0], dr * self.last[1]) } else { (dl, dr) };
            let peaks = if self.link {
                let p = level(sl.abs().max(sr.abs()));
                [p, p]
            } else {
                [level(sl), level(sr)]
            };
            let mut gains = [1.0; 2];
            for ch in 0..2 {
                let e = &mut self.env[ch];
                let k = if peaks[ch] > *e { self.attack } else { self.release };
                *e += k * (peaks[ch] - *e);
                let over = *e - self.threshold;
                gains[ch] = if over > 0.0 { db(-over * self.slope) } else { 1.0 };
            }
            self.last = gains;
            let (wl, wr) = (dl * gains[0] * self.makeup, dr * gains[1] * self.makeup);
            *l = self.mix * wl + (1.0 - self.mix) * *l;
            *r = self.mix * wr + (1.0 - self.mix) * *r;
        }
    }

    fn clear(&mut self) {
        (self.env, self.last) = ([FLOOR_DB; 2], [1.0; 2]);
    }
}

/// Transient Master: a fast and a slow envelope; their difference (dB)
/// scaled by attack (-1..=1) boosts or cuts onsets, by sustain the rest.
/// Input ±12 dB about 0.5; smooth lengthens the slow release.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Transient {
    input: f32,
    attack: f32,
    sustain: f32,
    k: [f32; 4],
    env: [f32; 2],
}

impl Transient {
    fn new() -> Self {
        Self { input: 1.0, attack: 0.0, sustain: 0.0, k: [1.0; 4], env: [FLOOR_DB; 2] }
    }

    fn clear(&mut self) { self.env = [FLOOR_DB; 2]; }

    fn tune(&mut self, f: &Fields, rate: f32) {
        let x = |i: usize| f[i].clamp(0.0, 1.0);
        self.input = db(24.0 * x(0) - 12.0);
        (self.attack, self.sustain) = (2.0 * x(1) - 1.0, 2.0 * x(2) - 1.0);
        self.k = [
            Comp::time(0.5, rate),
            Comp::time(30.0, rate),
            Comp::time(20.0, rate),
            Comp::time(300.0 * (1.0 + 3.0 * x(3)), rate),
        ];
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let p = (l.abs().max(r.abs()) * self.input).max(1e-6);
            let p = 20.0 * p.log10();
            for (e, (att, rel)) in self.env.iter_mut().zip([(self.k[0], self.k[1]), (self.k[2], self.k[3])]) {
                *e += if p > *e { att } else { rel } * (p - *e);
            }
            let diff = self.env[0] - self.env[1];
            let g = if diff > 0.0 { self.attack * diff } else { -self.sustain * diff };
            let g = db(g.clamp(-24.0, 24.0)) * self.input;
            (*l, *r) = (*l * g, *r * g);
        }
    }
}

/// Power-of-two stereo ring buffer.
struct Lines {
    buf: [Vec<f32>; 2],
    mask: usize,
    pos: usize,
}

impl Lines {
    fn new(frames: usize) -> Self {
        let len = (frames + 4).next_power_of_two();
        Self { buf: [vec![0.0; len], vec![0.0; len]], mask: len - 1, pos: 0 }
    }

    /// `delay` frames back (fractional, linear), at least one.
    #[inline]
    fn read(&self, ch: usize, delay: f32) -> f32 {
        let d = delay.clamp(1.0, (self.mask - 2) as f32);
        let (i, t) = (d as usize, d.fract());
        let b = &self.buf[ch];
        let a = b[self.pos.wrapping_sub(i) & self.mask];
        let c = b[self.pos.wrapping_sub(i + 1) & self.mask];
        a + (c - a) * t
    }

    #[inline]
    fn write(&mut self, l: f32, r: f32) {
        self.buf[0][self.pos] = l;
        self.buf[1][self.pos] = r;
        self.pos = (self.pos + 1) & self.mask;
    }

    fn clear(&mut self) {
        self.buf.iter_mut().for_each(|b| b.fill(0.0));
    }
}

/// Legacy Delay: Time is milliseconds while unsynced, otherwise a count of
/// the saved quarter-beat multiplier. Damping (a low-pass in the loop, 20 kHz..=1 kHz), pan (how much
/// of the feedback crosses channels: ping-pong at 1) and feedback 0..=1.
/// Wet only; the slot mixes the dry signal.
struct Delay {
    lines: Lines,
    frames: f32,
    feedback: f32,
    cross: f32,
    damp: f32,
    lp: [f32; 2],
}

impl Delay {
    fn tune(&mut self, f: &Fields, rate: f32, tempo: f32) {
        let max = if f[4] > 0.0 { 5800.0 } else { MAX_DELAY_S * 1000.0 - 10.0 };
        self.frames = (delay_ms(f, tempo).clamp(1.0, max) * 0.001 * rate).max(1.0);
        self.damp = one_pole(20_000.0 * 0.05f32.powf(f[1].clamp(0.0, 1.0)), rate);
        self.cross = 0.5 * f[2].clamp(0.0, 1.0);
        self.feedback = f[3].clamp(0.0, 0.99);
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let (dl, dr) = (self.lines.read(0, self.frames), self.lines.read(1, self.frames));
            self.lp[0] += self.damp * (dl - self.lp[0]) + ANTI_DENORMAL;
            self.lp[1] += self.damp * (dr - self.lp[1]) + ANTI_DENORMAL;
            let [fl, fr] = self.lp;
            let c = self.cross;
            let back_l = (1.0 - c) * fl + c * fr;
            let back_r = (1.0 - c) * fr + c * fl;
            self.lines.write(*l + self.feedback * back_l, *r + self.feedback * back_r);
            (*l, *r) = (dl, dr);
        }
    }

    fn tail(&self, peak: f32) -> usize {
        let repeats = if self.feedback > 1e-3 && peak > 1e-6 {
            ((1e-6 / peak).ln() / self.feedback.ln()).ceil().max(1.0)
        } else {
            1.0
        };
        ((repeats + 1.0) * self.frames) as usize
    }
}

/// A sine LFO's rate (Hz) from a normalized speed: 0.01..=10 Hz (log).
fn lfo_hz(x: f32) -> f32 {
    0.01 * 1000f32.powf(x.clamp(0.0, 1.0))
}

/// Legacy Chorus and Flanger: a delay swept by a sine LFO, the right
/// channel's LFO `phase · 180°` ahead. Chorus: 7 ms + depth · 8 ms.
/// Flanger: color · 1 ms (0.1..=10) + depth · 5 ms, with feedback.
struct Sweep {
    lines: Lines,
    base: f32,
    depth: f32,
    feedback: f32,
    inc: f32,
    offset: f32,
    phase: f32,
    last: [f32; 2],
}

impl Sweep {
    fn tune(&mut self, kind: Kind, f: &Fields, rate: f32) {
        let x = |i: usize| f[i].clamp(0.0, 1.0);
        let ms = rate * 0.001;
        (self.base, self.depth, self.feedback) = if kind == Kind::Flanger {
            (f[4].clamp(0.1, 10.0) * ms, 5.0 * x(0) * ms, 0.9 * (2.0 * x(3) - 1.0))
        } else {
            (7.0 * ms, 8.0 * x(0) * ms, 0.0)
        };
        self.inc = lfo_hz(x(1)) / rate;
        self.offset = 0.5 * x(2);
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            self.phase = (self.phase + self.inc).fract();
            let lfo = |p: f32| 0.5 + 0.5 * (TAU * p).sin();
            let dl = self.base + self.depth * lfo(self.phase);
            let dr = self.base + self.depth * lfo(self.phase + self.offset);
            let (wl, wr) = (self.lines.read(0, dl), self.lines.read(1, dr));
            let fb = |x: f32| self.feedback * x + ANTI_DENORMAL;
            self.lines.write(*l + fb(self.last[0]), *r + fb(self.last[1]));
            self.last = [wl, wr];
            (*l, *r) = (wl, wr);
        }
    }
}

/// Legacy Phaser: six first-order all-passes per channel swept from
/// 200 Hz up `depth · 5` octaves by a sine LFO, with feedback. Wet only:
/// the notches come from the slot's dry mix.
struct Phaser {
    inc: f32,
    phase: f32,
    offset: f32,
    octaves: f32,
    feedback: f32,
    rate: f32,
    s: [[f32; 6]; 2],
    last: [f32; 2],
}

/// Frames between all-pass coefficient updates.
const PHASER_STEP: usize = 16;

impl Phaser {
    fn tune(&mut self, f: &Fields, rate: f32) {
        let x = |i: usize| f[i].clamp(0.0, 1.0);
        self.octaves = 5.0 * x(0);
        self.feedback = 0.9 * x(1);
        self.inc = lfo_hz(x(2)) / rate * PHASER_STEP as f32;
        self.offset = 0.5 * x(3);
        self.rate = rate;
    }

    fn coefficient(&self, p: f32) -> f32 {
        let hz = 200.0 * (self.octaves * (0.5 + 0.5 * (TAU * p).sin())).exp2();
        let t = (PI * hz.min(0.45 * self.rate) / self.rate).tan();
        (t - 1.0) / (t + 1.0)
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len();
        for start in (0..n).step_by(PHASER_STEP) {
            let end = (start + PHASER_STEP).min(n);
            self.phase = (self.phase + self.inc).fract();
            let coeffs = [self.coefficient(self.phase), self.coefficient(self.phase + self.offset)];
            for (ch, buf) in [&mut left[start..end], &mut right[start..end]].into_iter().enumerate() {
                let a = coeffs[ch];
                for x in buf.iter_mut() {
                    let mut y = *x + self.feedback * self.last[ch];
                    for s in &mut self.s[ch] {
                        let out = a * y + *s;
                        *s = y - a * out + ANTI_DENORMAL;
                        y = out;
                    }
                    self.last[ch] = y;
                    *x = y;
                }
            }
        }
    }
}

enum Dsp {
    Drive(Drive),
    Comp(Comp),
    Transient(Transient),
    Delay(Delay),
    Sweep(Sweep),
    Phaser(Phaser),
    Filter(RackFilter),
}

/// A rack slot's effect: its stored values and DSP state.
pub(crate) struct Block {
    kind: Kind,
    fields: Fields,
    rate: f32,
    tempo: f32,
    dsp: Dsp,
}

impl Block {
    pub(crate) fn reduction(&self, channel: usize) -> Option<f32> {
        match &self.dsp {
            Dsp::Comp(c) => c.last.get(channel).copied(),
            _ => None,
        }
    }
    /// `None` for effects without DSP here. Allocates.
    pub(crate) fn new(fx: &Effect, rate: f32) -> Option<Box<Self>> {
        let kind = fx.kind;
        // Filters, EQs and the Solid G-EQ: the group filter's sections.
        if matches!(fx.params, Params::Filter(_) | Params::Eq(_)) || kind == Kind::SolidGeq {
            let dsp = Dsp::Filter(RackFilter::new(fx, rate)?);
            return Some(Box::new(Self { kind, fields: [0.0; FIELDS], rate, tempo: 120.0, dsp }));
        }
        let fields = fields(&fx.params)?;
        let dsp = match kind {
            k if Drive::supports(k) => Dsp::Drive(Drive::default()),
            Kind::Compressor | Kind::Limiter | Kind::SolidBusComp | Kind::FeedbackCompressor => Dsp::Comp(Comp::new()),
            Kind::TransientMaster => Dsp::Transient(Transient::new()),
            Kind::Delay => Dsp::Delay(Delay {
                // At least the native 262144-frame line, including slow synced clocks.
                lines: Lines::new(((MAX_DELAY_S * rate) as usize).max(262_140)),
                frames: 1.0,
                feedback: 0.0,
                cross: 0.0,
                damp: 1.0,
                lp: [0.0; 2],
            }),
            Kind::Chorus | Kind::Flanger => Dsp::Sweep(Sweep {
                lines: Lines::new((0.03 * rate) as usize),
                base: 1.0,
                depth: 0.0,
                feedback: 0.0,
                inc: 0.0,
                offset: 0.0,
                phase: 0.0,
                last: [0.0; 2],
            }),
            Kind::Phaser => Dsp::Phaser(Phaser {
                inc: 0.0,
                phase: 0.0,
                offset: 0.0,
                octaves: 0.0,
                feedback: 0.0,
                rate,
                s: [[0.0; 6]; 2],
                last: [0.0; 2],
            }),
            _ => return None,
        };
        let mut out = Box::new(Self { kind, fields, rate, tempo: 120.0, dsp });
        out.tune();
        Some(out)
    }

    fn tune(&mut self) {
        let (f, rate, kind) = (&self.fields, self.rate, self.kind);
        match &mut self.dsp {
            Dsp::Drive(d) => {
                d.tune(kind, f, rate);
            }
            Dsp::Comp(c) => c.tune(kind, f, rate),
            Dsp::Transient(t) => t.tune(f, rate),
            Dsp::Delay(d) => d.tune(f, rate, self.tempo),
            Dsp::Sweep(s) => s.tune(kind, f, rate),
            Dsp::Phaser(p) => p.tune(f, rate),
            Dsp::Filter(_) => {}
        }
    }

    pub(crate) fn set_tempo(&mut self, tempo: f32) {
        if tempo.is_finite() && tempo > 0.0 && tempo != self.tempo {
            self.tempo = tempo;
            if let Dsp::Delay(d) = &mut self.dsp { d.tune(&self.fields, self.rate, tempo); }
        }
    }

    /// Set stored `field` of a `kind` effect from a script's normalized
    /// value; false when this is another effect.
    pub(crate) fn set(&mut self, kind: Kind, field: u8, x: f32) -> bool {
        if let Dsp::Filter(f) = &mut self.dsp {
            return f.set(kind, field, stored(kind, field, x));
        }
        if kind != self.kind || field as usize >= FIELDS || !x.is_finite() { return false }
        if kind == Kind::Delay {
            if !set_delay_field(&mut self.fields, field, x, self.tempo) { return false }
        } else { self.fields[field as usize] = stored(kind, field, x); }
        self.tune();
        true
    }

    pub(crate) fn restore_delay(&mut self, saved: &super::DelayState) -> bool {
        if self.kind != Kind::Delay || !saved.apply(&mut self.fields) { return false }
        self.tune();
        true
    }

    pub(crate) fn delay_fields(&self) -> Option<Fields> {
        (self.kind == Kind::Delay).then_some(self.fields)
    }

    pub(crate) fn set_filter(&mut self, knob: crate::engine::filter::Knob, value: f32) -> bool {
        match &mut self.dsp {
            Dsp::Filter(f) => f.set_knob(knob, value),
            _ => false,
        }
    }

    pub(crate) fn filter_param(&self, knob: crate::engine::filter::Knob) -> Option<f32> {
        match &self.dsp {
            Dsp::Filter(f) => f.knob(knob),
            _ => None,
        }
    }

    /// A stored field as the script reads it.
    pub(crate) fn get(&self, kind: Kind, field: u8) -> Option<f32> {
        if let Dsp::Filter(f) = &self.dsp {
            return f.get(kind, field);
        }
        (kind == self.kind).then_some(())?;
        normalized_field(kind, field, &self.fields, self.tempo)
    }

    pub(crate) fn clear(&mut self) {
        match &mut self.dsp {
            Dsp::Drive(d) => d.clear(),
            Dsp::Comp(c) => c.clear(),
            Dsp::Transient(t) => t.clear(),
            Dsp::Delay(d) => {
                d.lines.clear();
                d.lp = [0.0; 2];
            }
            Dsp::Sweep(s) => {
                s.lines.clear();
                s.last = [0.0; 2];
            }
            Dsp::Phaser(p) => (p.s, p.last) = ([[0.0; 6]; 2], [0.0; 2]),
            Dsp::Filter(f) => f.clear(),
        }
    }

    /// Frames of output after input at most `peak` falls silent.
    pub(crate) fn tail(&self, peak: f32) -> usize {
        let ms = |ms: f32| (ms * 0.001 * self.rate) as usize;
        match &self.dsp {
            Dsp::Delay(d) => d.tail(peak),
            Dsp::Sweep(s) => {
                let rings = if s.feedback.abs() > 1e-3 { 200.0 } else { 1.0 };
                (rings * (s.base + s.depth)) as usize
            }
            Dsp::Comp(_) | Dsp::Transient(_) => ms(50.0),
            Dsp::Phaser(_) | Dsp::Filter(_) => ms(100.0),
            Dsp::Drive(d) if d.kind == DriveKind::Distortion => {
                let (a1, a2) = (d.c[9], d.c[10]);
                let discriminant = a1 * a1 + 4.0 * a2;
                let radius = if discriminant < 0.0 { (-a2).sqrt() } else {
                    let root = discriminant.sqrt();
                    (0.5 * (a1 + root)).abs().max((0.5 * (a1 - root)).abs())
                }.max(d.c[2].abs());
                if peak <= super::processor::SILENCE { 0 } else {
                    ((super::processor::SILENCE / peak).ln() / radius.ln()).ceil().max(0.0) as usize
                }
            }
            Dsp::Drive(_) => ms(10.0),
        }
    }

    pub(crate) fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        match &mut self.dsp {
            Dsp::Drive(d) => d.process(left, right),
            Dsp::Comp(c) => c.process(left, right),
            Dsp::Transient(t) => t.process(left, right),
            Dsp::Delay(d) => d.process(left, right),
            Dsp::Sweep(s) => s.process(left, right),
            Dsp::Phaser(p) => p.process(left, right),
            Dsp::Filter(f) => f.process(left, right),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fx::params::Field;

    const RATE: f32 = 48_000.0;

    fn effect(kind: Kind, values: &[f32]) -> Effect {
        let layout = crate::fx::params::layout_names(kind).expect("layout");
        let fields = layout
            .iter()
            .zip(values)
            .map(|(&name, &v)| Field { name, value: Value::Number(v) })
            .collect();
        Effect {
            slot: 0,
            kind,
            version: 0,
            bypass: false,
            output_gain: 1.0,
            dry_level: 0.0,
            params: Params::Fields(fields),
        }
    }

    /// Stereo sine at `hz` for `n` frames, amplitude `a`.
    fn sine(hz: f32, a: f32, n: usize) -> (Vec<f32>, Vec<f32>) {
        let l: Vec<f32> = (0..n).map(|i| a * (TAU * hz * i as f32 / RATE).sin()).collect();
        (l.clone(), l)
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }

    /// Runs `block` over `l`/`r` in 128-frame blocks; checks finite and
    /// not subnormal.
    fn run(block: &mut Block, l: &mut [f32], r: &mut [f32]) {
        for (cl, cr) in l.chunks_mut(128).zip(r.chunks_mut(128)) {
            block.process(cl, cr);
        }
        let bad = l.iter().chain(r.iter()).find(|x| !x.is_finite() || (**x != 0.0 && x.abs() < 1e-30));
        assert!(bad.is_none(), "{:?}: {bad:?}", block.kind);
    }

    #[test]
    fn every_kind_with_defaults_plays_finite() {
        for (_, kind, _) in crate::fx::kind::TABLE {
            let Some(values) = defaults(*kind) else { continue };
            let mut b = Block::new(&effect(*kind, values), RATE).expect("block");
            let (mut l, mut r) = sine(220.0, 0.5, 9600);
            run(&mut b, &mut l, &mut r);
            // Silence afterwards decays to (near) silence within the tail.
            let tail = b.tail(0.5).min(10 * RATE as usize);
            let (mut l, mut r) = (vec![0.0; tail + 4800], vec![0.0; tail + 4800]);
            run(&mut b, &mut l, &mut r);
            let end = &l[l.len() - 480..];
            assert!(rms(end) < 1e-3, "{kind:?} rings: {}", rms(end));
        }
    }

    #[test]
    fn compressor_reduces_loud_input_by_its_ratio() {
        // -6 dBFS sine, threshold -20 dB, ratio 50^0.5 ≈ 7: about 12 dB down.
        let mut b = Block::new(&effect(Kind::Compressor, &[0.0, -20.0, 0.5, 1.0, 100.0, 1.0]), RATE).unwrap();
        let (mut l, mut r) = sine(1000.0, 0.5, 48_000);
        run(&mut b, &mut l, &mut r);
        let out = 20.0 * (rms(&l[24_000..]) / (0.5 / 2f32.sqrt())).log10();
        assert!((-14.0..-9.0).contains(&out), "{out}");
        // Below the threshold: untouched.
        let mut b = Block::new(&effect(Kind::Compressor, &[0.0, -20.0, 0.5, 1.0, 100.0, 1.0]), RATE).unwrap();
        let (mut l, mut r) = sine(1000.0, 0.01, 48_000);
        run(&mut b, &mut l, &mut r);
        assert!((rms(&l[24_000..]) / (0.01 / 2f32.sqrt()) - 1.0).abs() < 0.02);
    }

    #[test]
    fn delay_repeats_at_its_time_with_feedback() {
        let mut b = Block::new(&effect(Kind::Delay, &[100.0, 0.0, 0.0, 0.5, -1.0, 0.0, 1.0, 1.0]), RATE).unwrap();
        let mut l = vec![0.0; 24_000];
        l[0] = 1.0;
        let mut r = l.clone();
        run(&mut b, &mut l, &mut r);
        let peak = |at: usize| l[at - 10..at + 10].iter().fold(0f32, |m, x| m.max(x.abs()));
        assert!(peak(4800) > 0.9 && peak(9600) > 0.4 && peak(9600) < 0.6, "{} {}", peak(4800), peak(9600));
        assert!(l[100..4700].iter().all(|x| x.abs() < 1e-3));
    }

    #[test]
    fn legacy_delay_time_law_preserves_saved_units_and_authored_echo_clocks_without_heap() {
        // Independent authored quarter-note writes at 60/120/240 BPM. These
        // are absolute Time writes: no sync-unit conversion is involved.
        for (raw, ms) in [(823_567, 1000.0), (708_896, 500.0), (594_553, 250.0), (1_000_000, 2900.0)] {
            let x = raw as f32 / 1e6;
            assert!((stored(Kind::Delay, 0, x) - ms).abs() < 0.01);
            assert!((normalized(Kind::Delay, 0, ms) * 1e6 - raw as f32).abs() <= 1.0);
            let at = (ms * RATE * 0.001).round() as usize;
            let initial = effect(Kind::Delay, &[ms, 0.0, 0.0, 0.5, -1.0, 0.0, 1.0, 1.0]);
            let (mut a, mut b) = (Block::new(&initial, RATE).unwrap(), Block::new(&initial, RATE).unwrap());
            assert_eq!(a.fields[0], ms, "imported physical time is not normalized again");
            assert!((a.get(Kind::Delay, 0).unwrap() * 1e6 - raw as f32).abs() <= 1.0);
            let mut al = vec![0.0; 2 * at + 32];
            al[0] = 1.0;
            let (mut ar, mut bl, mut br) = (al.clone(), al.clone(), al.clone());
            let mut exercise = || {
                assert!(a.set(Kind::Delay, 0, x));
                assert!(b.set(Kind::Delay, 0, x));
                assert!((a.get(Kind::Delay, 0).unwrap() - x).abs() < 1e-6);
                for (l, r) in al.chunks_mut(128).zip(ar.chunks_mut(128)) { a.process(l, r); }
                for (l, r) in bl.chunks_mut(31).zip(br.chunks_mut(31)) { b.process(l, r); }
            };
            #[cfg(feature = "plugin")]
            assert_eq!(crate::plugin::tests::allocations(|| exercise()), 0);
            #[cfg(not(feature = "plugin"))]
            exercise();
            assert_eq!(al, bl, "host block partition preserves delay clock");
            assert_eq!(ar, br);
            assert!(al.iter().chain(&ar).all(|v| v.is_finite()));
            assert!(al[..at - 2].iter().all(|v| v.abs() < 1e-6), "no premature echo for {ms} ms");
            // Fractional reads split an impulse between adjacent frames;
            // feedback interpolates the first echo again. Their area and
            // centroid preserve gain and time independently of tap peaks.
            for (center, area, tolerance) in [(at, 1.0, 0.25), (2 * at, 0.5, 0.5)] {
                // Include the short 20kHz loop-filter tail in the feedback area.
                let taps = &al[center - 2..center + 24];
                let sum: f32 = taps.iter().sum();
                assert!((sum - area).abs() < 1e-6, "{ms} ms: echo area {sum}, expected {area}");
                let offset = taps.iter().enumerate().map(|(i, v)| (i as f32 - 2.0) * v).sum::<f32>() / sum;
                assert!(offset.abs() < tolerance, "{ms} ms: echo centroid offset {offset}");
            }
        }
        assert!((stored(Kind::Delay, 0, 0.0) - 5.0).abs() < 1e-5);
        assert_eq!(normalized(Kind::Delay, 0, 5.0), 0.0);
        assert!((normalized(Kind::Delay, 0, 2900.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn classic_saturation_matches_native_piecewise_transfer_and_output_without_heap() {
        // Independent scalar values at branch boundaries, with signals above
        // unity as well as quiet inputs. This is the Classic mode, not tanh.
        let input = [-4.0, -1.0, -0.5, -0.25, -0.01, 0.0, 0.01, 0.25, 0.5, 1.0, 4.0];
        let reference = |shape: f64, x: f64| {
            if shape.abs() <= 0.0001 { return x; }
            if shape < 0.0 {
                let q = 4.0 + 3.9 * shape;
                return x * (x * x + q) / (1.0 + q);
            }
            let amount = 4.0 * shape;
            let magnitude = (x * if amount >= 1.0 { amount } else { 1.0 }).abs().min(1.0);
            let quadratic = (2.0 * magnitude - magnitude * magnitude).copysign(x);
            if amount >= 1.0 { quadratic } else { (1.0 - amount) * x + amount * quadratic }
        };
        let mut values = [0.0; FIELDS];
        let mut d = Drive::default();
        for shape in [-1.0, -0.5, -0.0001, 0.0, 0.0001, 0.125, 0.2499, 0.25, 0.2501, 0.5, 1.0] {
            values[0] = shape;
            let (mut l, mut r) = (input, input.map(|x| -x));
            assert_eq!(crate::plugin::tests::allocations(|| {
                assert!(d.tune(Kind::SurroundPanner, &values, RATE));
                d.process(&mut l, &mut r);
            }), 0);
            for (x, (l, r)) in input.iter().zip(l.iter().zip(&r)) {
                let expected = reference(f64::from(shape), f64::from(*x));
                assert!((f64::from(*l) - expected).abs() < 2e-5, "shape{shape}, input{x}");
                assert!((f64::from(*r) + expected).abs() < 2e-5);
            }
        }
        // The common effect wrapper applies linear Output after shaping.
        let effect = crate::fx::Effect {
            slot: 0, kind: Kind::SurroundPanner, version: 0, bypass: false,
            output_gain: 0.79292566, dry_level: 0.0,
            params: Params::Fields(vec![
                crate::fx::params::Field { name: "param_0", value: Value::Number(1.0) },
                crate::fx::params::Field { name: "param_1", value: Value::Number(0.0) },
            ]),
        };
        let mut program = crate::fx::ProgramFx { insert: crate::fx::Chain { slots: vec![effect] }, ..Default::default() };
        assert!(program.warnings().is_empty());
        assert!(crate::engine::filter::unsupported_at(&program.insert, Some(8)).is_empty());
        let mut processor = program.processor(RATE, input.len());
        let (mut l, mut r) = (input, input);
        assert_eq!(crate::plugin::tests::allocations(|| processor.process(&mut l, &mut r)), 0);
        for (x, y) in input.iter().zip(l) {
            assert!((f64::from(y) - reference(1.0, f64::from(*x)) * 0.79292566).abs() < 2e-6);
        }
        assert!((l[6] - 0.06216537).abs() < 1e-7, "quiet signals receive the native saturation gain");
        let Params::Fields(fields) = &mut program.insert.slots[0].params else { unreachable!() };
        fields[1].value = Value::Number(1.0);
        assert!(program.warnings().iter().any(|w| w.contains("Enhanced/Drums modes use an unverified")));
        assert!(crate::engine::filter::unsupported_at(&program.insert, Some(8)).iter()
            .any(|w| w.contains("Enhanced/Drums modes use an unverified")));
    }

    #[test]
    fn tube_distortion_matches_independent_numeric_boundaries_without_heap() {
        // Decimal scalar reference values, independently evaluated at the
        // native Drive blend boundaries. These are before filtering/Output.
        let input = [-4.0, -1.0, -0.25, -0.01, 0.0, 0.01, 0.25, 1.0, 4.0];
        let cases: [(f32, [f64; 9]); 6] = [
            (0.0, [-4.0, -1.0, -0.25, -0.01, 0.0, 0.01, 0.25, 1.0, 4.0]),
            (0.2499, [-2.33386672, -1.0, -0.13523993351503, -0.0044476805163085, 0.0, 0.01, 0.25, 1.0, 4.0]),
            (0.25, [-2.3333333333333, -1.0, -0.13520752308188, -0.0044459034950162, 0.0, 0.01, 0.25, 1.0, 4.0]),
            (0.75, [-1.0, -1.0, -0.086269133535412, -9.407824380947e-6, 0.0, 0.056225468642652, 0.76211423732082, 1.0, 1.3333333333333]),
            (0.7501, [-1.0, -1.0, -0.08627825680086, -9.409765565272e-6, 0.0, 0.056233800503188, 0.76217837834686, 1.0, 1.33306672]),
            (1.0, [-1.0, -1.0, -0.10939426317087, -1.511801183946e-5, 0.0, 0.072360783382485, 0.85797649379469, 1.0, 1.0]),
        ];
        let mut values = [0.0; FIELDS];
        let mut d = Drive::default();
        for (drive, expected) in cases {
            values[1] = drive;
            assert_eq!(crate::plugin::tests::allocations(|| {
                assert!(d.tune(Kind::Distortion, &values, RATE));
                let [k, out, _, _, negative, positive, ..] = d.c;
                assert_eq!(out, 1.0, "Tube has no drive makeup");
                for (x, expected) in input.into_iter().zip(expected) {
                    let actual = tube_curve(x, k, negative, positive);
                    assert!((f64::from(actual) - expected).abs() < 2e-6, "drive{drive}, input{x}: {actual} != {expected}");
                    if drive == 0.0 { assert_eq!(actual, x); }
                }
            }), 0);
        }
        let mut fx = crate::fx::ProgramFx {
            insert: crate::fx::Chain { slots: vec![effect(Kind::Distortion, &[0.0, 0.0, 0.0])] },
            ..Default::default()
        };
        assert!(fx.warnings().iter().any(|w| w.contains("Damping parameter smoothing is not applied")));
        assert!(fx.warnings().iter().all(|w| !w.contains("Transistor")));
        fx.insert.slots[0] = effect(Kind::Distortion, &[1.0, 0.0, 0.0]);
        assert!(crate::engine::filter::unsupported_at(&fx.insert, Some(8)).iter()
            .any(|w| w.contains("Damping parameter smoothing is not applied")));
    }

    // Independently evaluated native float preparation, not effect-owned data.
    fn dc_reference(rate: f32) -> [f32; 5] {
        match rate as u32 {
            44100 => [0.9984942674636841, -1.9969885349273682, 0.9984942674636841, 1.9969862699508667, -0.9969909191131592],
            48000 => [0.9986164569854736, -1.9972329139709473, 0.9986164569854736, 1.9972310066223145, -0.9972348213195801],
            96000 => [0.9993079900741577, -1.9986159801483154, 0.9993079900741577, 1.9986155033111572, -0.9986165165901184],
            _ => unreachable!(),
        }
    }

    fn dc_reference_step(x: f32, history: &mut [f32; 4], c: [f32; 5]) -> f32 {
        let [b0, b1, b2, a1, a2] = c;
        let [x1, x2, y1, y2] = *history;
        // Native SIMD sum order, independent prepared coefficients above.
        let y = (((x * b0 + x1 * b1) + x2 * b2) + y1 * a1) + y2 * a2;
        *history = [x, x1, y, y1];
        y
    }

    #[test]
    fn transistor_distortion_matches_independent_boundaries_and_rates_without_heap() {
        // Independently evaluated decimal reference, including both threshold
        // joins, quarter-amplitude exits, quiet signals and above-unit input.
        let input = [-4.0, -0.25000006, -0.25, -0.24999997, -0.06250001, -0.0625,
            -0.06249999, -0.00390625, -0.000244140625, 0.0, 0.00024412, 0.000244140625,
            0.00024416, 0.00390625, 0.01, 0.24999997, 0.25, 4.0];
        let cases: [(f32, [f64; 18]); 3] = [
            (0.0, [-4.0, -0.25000006, -0.25, -0.24999997, -0.06250001, -0.0625,
                -0.06249999, -0.00390625, -0.000244140625, 0.0, 0.00024412, 0.000244140625,
                0.00024416, 0.00390625, 0.01, 0.24999997, 0.25, 4.0]),
            (0.5, [-4.0, -0.25000006, -0.25, -0.24999996, -0.043651579025587,
                -0.043651570718502, -0.043651562411416, -0.0020751953125, -0.00012969970703125,
                0.0, 0.00207502, 0.0020751953125, 0.002075297998524, 0.012261780552913,
                0.023119491591942, 0.249999976, 0.25, 4.0]),
            (1.0, [-4.0, -0.25000006, -0.25, -0.24999985000004, -0.00024414082031256,
                -0.000244140625, -0.0002441405859375, -1.52587890625e-5, -9.5367431640625e-7,
                0.0, 0.06249472, 0.0625, 0.062500991968511, 0.10881882041202,
                0.13132639022019, 0.249999994, 0.25, 4.0]),
        ];
        let mut values = [0.0; FIELDS];
        values[0] = 1.0;
        let mut d = Drive::default();
        for rate in [44_100.0f32, 96_000.0] {
            for (drive, expected) in cases {
                values[1] = drive;
                let (mut l, mut r) = (input, input);
                r.reverse();
                assert_eq!(crate::plugin::tests::allocations(|| {
                    d.clear();
                    assert!(d.tune(Kind::Distortion, &values, rate));
                    let [drive, power, _, _, threshold, scale, ..] = d.c;
                    let inverse_power = 1.0 / f64::from(power);
                    for (x, expected) in input.into_iter().zip(expected) {
                        let actual = transistor_curve(x, drive, power, threshold, scale, inverse_power);
                        assert!((f64::from(actual) - expected).abs() < 2e-7, "drive{drive}, input{x}: {actual} != {expected}");
                        if drive == 0.0 { assert_eq!(actual, x); }
                        if x >= 0.25 || x < -0.25 { assert_eq!(actual, x); }
                    }
                    d.process(&mut l, &mut r);
                }), 0);
                // Independent Decimal damping coefficients at native Damping 0.
                let a = if rate == 44_100.0 { -0.968830620627632 } else { 0.072020615289497 };
                assert!((f64::from(d.c[2]) - a).abs() < 3e-6);
                let pole = d.c[2]; // Verified above, preserve the native float clock.
                let b = 0.5 * (1.0 - pole);
                for (buffer, reversed) in [(&l, false), (&r, true)] {
                    let (mut previous, mut state) = (0.0f32, 0.0f32);
                    let mut hp = [0.0; 4];
                    for (i, actual) in buffer.iter().enumerate() {
                        let source = expected[if reversed { expected.len() - 1 - i } else { i }] as f32;
                        state = (b * source + b * previous) + pole * state + 1e-20;
                        previous = source;
                        let reference = dc_reference_step(state, &mut hp, dc_reference(rate));
                        assert!((*actual - reference).abs() < 2e-6);
                    }
                }
            }
        }
    }

    #[test]
    fn distortion_damping_rates_impulse_partition_reset_without_heap() {
        // Independent Decimal evaluation of the exponential and sine Taylor
        // polynomials, followed by the bilinear pole and unity-DC normalization.
        // Rows are native Damping 0, 0.5, 1; columns are 44.1, 48, 96 kHz.
        let poles = [
            [-0.968830620627632, -0.748475234218230, 0.072020615289497],
            [0.281417361896059, 0.326787237731620, 0.613962547016527],
            [0.727181173700699, 0.746823690647603, 0.865513282079401],
        ];
        for mode in [0.0, 1.0] {
            for (rate_index, rate) in [44_100.0, 48_000.0, 96_000.0].into_iter().enumerate() {
                for (damping_index, damping) in [0.0, 0.5, 1.0].into_iter().enumerate() {
                    let mut fields = [0.0; FIELDS];
                    fields[0] = mode; // Drive 0 isolates the damping filter.
                    fields[2] = damping;
                    let mut full = Drive::default();
                    let (mut l, mut r) = ([0.0; 256], [0.0; 256]);
                    l[0] = 1.0;
                    r[7] = -0.25;
                    let (mut split_l, mut split_r) = (l, r);
                    assert_eq!(crate::plugin::tests::allocations(|| {
                        assert!(full.tune(Kind::Distortion, &fields, rate));
                        let a = poles[damping_index][rate_index];
                        assert!((f64::from(full.c[2]) - a).abs() < 3e-6);
                        assert!(full.c[2].abs() < 1.0);
                        let mut partitioned = full;
                        full.process(&mut l, &mut r);
                        for (start, end) in [(0, 13), (13, 45), (45, 48), (48, 256)] {
                            partitioned.process(&mut split_l[start..end], &mut split_r[start..end]);
                        }
                        assert_eq!(l, split_l);
                        assert_eq!(r, split_r);
                        assert_eq!(full.s, partitioned.s);
                        // Coefficients are checked against the independent
                        // Decimal law above; the state clock uses native f32.
                        let pole = full.c[2];
                        let b = 0.5 * (1.0 - pole);
                        for (buffer, delay, amplitude) in [(&l, 0, 1.0), (&r, 7, -0.25)] {
                            let (mut previous, mut state) = (0.0f32, 0.0f32);
                            let mut hp = [0.0; 4];
                            for (i, actual) in buffer.iter().enumerate() {
                                let x = if i == delay { amplitude } else { 0.0 };
                                state = (b * x + b * previous) + pole * state + 1e-20;
                                previous = x;
                                let reference = dc_reference_step(state, &mut hp, dc_reference(rate));
                                assert!((*actual - reference).abs() < 4e-6,
                                    "mode{mode} rate{rate} damping{damping} frame{i}");
                            }
                        }
                        // A live retune keeps history; clear removes it even
                        // after a different-rate preparation and warm processing.
                        let state = full.s;
                        assert!(full.tune(Kind::Distortion, &fields, rate * 2.0));
                        assert_eq!(full.s, state);
                        full.clear();
                        assert_eq!(full.s, [0.0; 10]);
                        assert!(full.tune(Kind::Distortion, &fields, rate));
                        let (mut reset_l, mut reset_r) = ([0.0; 256], [0.0; 256]);
                        reset_l[0] = 1.0;
                        reset_r[7] = -0.25;
                        full.process(&mut reset_l, &mut reset_r);
                        assert_eq!(l, reset_l);
                        assert_eq!(r, reset_r);
                    }), 0);
                }
            }
        }
    }

    #[test]
    fn distortion_dc_native_coefficients_constant_frequency_and_state_without_heap() {
        eprintln!("Distortion DC state: Drive={} VoiceEffect={} VoiceFilter={} bytes",
            std::mem::size_of::<Drive>(), std::mem::size_of::<VoiceEffect>(),
            std::mem::size_of::<crate::engine::filter::VoiceFilter>());
        assert_eq!(std::mem::size_of::<Drive>(), 96);
        for rate in [44_100.0, 48_000.0, 96_000.0] {
            let mut fields = [0.0; FIELDS];
            fields[2] = 1.0;
            let mut drive = Drive::default();
            let mut dc_residual = [0.0; 2];
            assert_eq!(crate::plugin::tests::allocations(|| {
                drive.tune(Kind::Distortion, &fields, rate);
                assert_eq!(&drive.c[6..11], &dc_reference(rate));
                let coefficients = drive.c;
                for _ in 0..16 { drive.tune(Kind::Distortion, &fields, rate); }
                assert_eq!(drive.c, coefficients, "same-rate preparation stays cached");
                // Feed a stereo DC step for one second. Neither normalized
                // Drive0 core changes the input; the final DC stage rejects it.
                for _ in 0..(rate as usize / 64) {
                    drive.process(&mut [1.0; 64], &mut [-0.25; 64]);
                }
                let (mut l, mut r) = ([1.0; 64], [-0.25; 64]);
                drive.process(&mut l, &mut r);
                dc_residual = [l.iter().fold(0.0f32, |peak, x| peak.max(x.abs())),
                    r.iter().fold(0.0f32, |peak, x| peak.max(x.abs()))];
                let c = dc_reference(rate).map(f64::from);
                assert_eq!(c[0] + c[1] + c[2], 0.0, "zero at DC");
                assert!(((c[0] - c[1] + c[2]) / (1.0 + c[3] - c[4]) - 1.0).abs() < 1e-7,
                    "native Nyquist normalization");
                // Compare steady sine amplitude with the independently
                // prepared LP+HP transfer functions, including the DC corner.
                for hz in [5.0, 15.0, 100.0] {
                    drive.clear();
                    let w = std::f64::consts::TAU * hz / f64::from(rate);
                    let (mut sum_sin, mut sum_cos, mut measured) = (0.0, 0.0, 0usize);
                    let frames = 2 * rate as usize;
                    for start in (0..frames).step_by(64) {
                        let length = (frames - start).min(64);
                        let mut l = [0.0; 64];
                        let mut r = [0.0; 64];
                        for (i, x) in l[..length].iter_mut().enumerate() { *x = (w * (start + i) as f64).sin() as f32; }
                        drive.process(&mut l[..length], &mut r[..length]);
                        if start >= rate as usize {
                            for (i, x) in l[..length].iter().enumerate() {
                                let phase = w * (start + i) as f64;
                                sum_sin += f64::from(*x) * phase.sin();
                                sum_cos += f64::from(*x) * phase.cos();
                                measured += 1;
                            }
                        }
                    }
                    let hp_num = (c[0] + c[1] * w.cos() + c[2] * (2.0 * w).cos()).powi(2)
                        + (c[1] * w.sin() + c[2] * (2.0 * w).sin()).powi(2);
                    let hp_den = (1.0 - c[3] * w.cos() - c[4] * (2.0 * w).cos()).powi(2)
                        + (c[3] * w.sin() + c[4] * (2.0 * w).sin()).powi(2);
                    let a = match rate as u32 { 44100 => 0.727181173700699, 48000 => 0.746823690647603, _ => 0.865513282079401 };
                    let b = 0.5 * (1.0 - a);
                    let lp_gain = ((b * (1.0 + w.cos())).powi(2) + (b * w.sin()).powi(2))
                        / ((1.0 - a * w.cos()).powi(2) + (a * w.sin()).powi(2));
                    let expected = (hp_num / hp_den * lp_gain).sqrt();
                    let actual = 2.0 * sum_sin.hypot(sum_cos) / measured as f64;
                    assert!((actual - expected).abs() < 0.002, "rate{rate} hz{hz}: {actual} != {expected}");
                }
                drive.clear();
                assert_eq!(drive.s, [0.0; 10]);
            }), 0);
            eprintln!("Distortion DC residual rate{rate}: left={} right={}", dc_residual[0], dc_residual[1]);
            assert!(dc_residual.into_iter().all(|x| x < 1e-4));
            let block = Block::new(&effect(Kind::Distortion, &[0.0, 0.0, 1.0]), rate).unwrap();
            assert_eq!(crate::plugin::tests::allocations(|| {
                assert_eq!(block.tail(0.0), 0);
                let radius = (-dc_reference(rate)[4]).sqrt();
                let expected = ((super::super::processor::SILENCE / 1.0).ln() / radius.ln()).ceil() as usize;
                assert_eq!(block.tail(1.0), expected);
                assert!(block.tail(1.0) > (0.1 * rate) as usize);
            }), 0);
        }
    }

    #[test]
    fn lofi_reduction_preserves_pristine_and_crushed_endpoints() {
        let mut d = Drive::default();
        // Lo-Fi at maximum sample rate is all but transparent.
        let mut f = [0.0; FIELDS];
        f[..5].copy_from_slice(&[0.5, 1.0, 0.0, 0.0, 0.2]);
        assert!(d.tune(Kind::LoFi, &f, RATE));
        let (mut l, mut r) = sine(200.0, 0.5, 4800);
        let dry = l.clone();
        d.process(&mut l, &mut r);
        let err: Vec<f32> = l.iter().zip(&dry).map(|(a, b)| a - b).collect();
        assert!(rms(&err) < 0.02, "{}", rms(&err));
        // Crushed: 2 bits, held 64 times.
        f[..2].copy_from_slice(&[0.03, 0.0]);
        d.tune(Kind::LoFi, &f, RATE);
        let (mut l, mut r) = sine(200.0, 0.5, 4800);
        d.process(&mut l, &mut r);
        let levels: std::collections::BTreeSet<i32> = l.iter().map(|x| (x * 1000.0) as i32).collect();
        assert!(levels.len() <= 5, "{levels:?}");
    }

    #[test]
    fn lofi_frequency_direction_endpoints_and_retuning_preserve_state_without_heap() {
        let check = || {
            assert_eq!(engine_par("$ENGINE_PAR_FREQUENCY"), Some((Kind::LoFi, 1)));
            for rate in [44_100., 48_000., 96_000.] {
                let mut f = [0.0; FIELDS];
                f[0] = 1.0; // Full bit depth, no noise: isolate sample-rate reduction.
                let mut d = Drive::default();
                assert!(d.tune(Kind::LoFi, &f, rate));
                let dry = std::array::from_fn::<_, 65, _>(|i| i as f32 * 0.001);
                let (mut l, mut r) = (dry, dry);
                d.process(&mut l, &mut r);
                assert!(l[..63].iter().all(|v| *v == 0.0));
                assert_eq!((l[63], l[64]), (dry[63], dry[63]));
                let (state, noise) = (d.s, d.noise);
                let mut previous = d.c[1];
                for frequency in [0.25, 0.5, 0.97368, 1.0] {
                    f[1] = frequency;
                    assert!(d.tune(Kind::LoFi, &f, rate));
                    assert!(d.c[1] > previous, "higher frequency samples more often");
                    previous = d.c[1];
                    assert_eq!((d.s, d.noise), (state, noise), "retuning retains held samples and clock phase");
                }
                let (mut l, mut r) = (dry, dry);
                d.process(&mut l, &mut r);
                assert_eq!((l, r), (dry, dry), "the pristine endpoint samples every frame");
            }
        };
        #[cfg(feature = "plugin")]
        assert_eq!(crate::plugin::tests::allocations(check), 0);
        #[cfg(not(feature = "plugin"))]
        check();
    }

    #[test]
    fn geq_boosts_its_band() {
        // HMF +15 dB at 2 kHz-ish (0.5 of 600..7000 Hz = 2049 Hz).
        let mut f = [0.5; FIELDS];
        f[2] = 0.0;
        f[11] = 0.0;
        f[6] = 1.0;
        let mut e = effect(Kind::SolidGeq, &f);
        e.slot = 1;
        let mut b = Block::new(&e, RATE).unwrap();
        let (mut l, mut r) = sine(2049.0, 0.1, 9600);
        run(&mut b, &mut l, &mut r);
        let gain = 20.0 * (rms(&l[4800..]) / (0.1 / 2f32.sqrt())).log10();
        assert!((gain - 15.0).abs() < 1.0, "{gain}");
        let (mut l, mut r) = sine(100.0, 0.1, 9600);
        run(&mut b, &mut l, &mut r);
        assert!(20.0 * (rms(&l[4800..]) / (0.1 / 2f32.sqrt())).log10() < 1.0);
    }

    #[test]
    fn phaser_and_chorus_move() {
        for kind in [Kind::Phaser, Kind::Chorus, Kind::Flanger] {
            let mut b = Block::new(&effect(kind, defaults(kind).unwrap()), RATE).unwrap();
            let (mut l, mut r) = sine(1000.0, 0.5, 48_000);
            run(&mut b, &mut l, &mut r);
            // The wet signal's short-term level or phase varies: compare
            // two windows a quarter LFO cycle apart against the input.
            let (dry, _) = sine(1000.0, 0.5, 48_000);
            let diff = |at: usize| rms(&l[at..at + 480].iter().zip(&dry[at..at + 480]).map(|(a, b)| a - b).collect::<Vec<_>>());
            assert!((diff(10_000) - diff(30_000)).abs() > 1e-3 || diff(10_000) > 1e-2, "{kind:?}");
        }
    }

    #[test]
    fn scripts_set_fields_through_their_laws() {
        assert_eq!(engine_par("$ENGINE_PAR_THRESHOLD"), Some((Kind::Compressor, 1)));
        let mut b = Block::new(&effect(Kind::Compressor, defaults(Kind::Compressor).unwrap()), RATE).unwrap();
        assert!(b.set(Kind::Compressor, 1, 0.5));
        assert_eq!(b.fields[1], -30.0);
        assert_eq!(b.get(Kind::Compressor, 1), Some(0.5));
        assert!(!b.set(Kind::Delay, 0, 0.5));
        assert!((stored(Kind::Delay, 0, normalized(Kind::Delay, 0, 500.0)) - 500.0).abs() < 0.01);
        // What ANALOG STRINGS' script sets and its preset stores.
        assert!((stored(Kind::Limiter, 0, 0.500011) - 0.00053).abs() < 1e-4);
        assert_eq!(stored(Kind::Limiter, 1, 0.0), 10.0);
        assert!((normalized(Kind::Limiter, 1, 100.0) - 0.5).abs() < 1e-5);
    }
}
