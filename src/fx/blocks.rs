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
            Law::Raw => (x * 1e6).round(),
        }
    }

    fn norm(self, v: f32) -> f32 {
        let x = match self {
            Law::Norm => v,
            Law::Lin(lo, hi) => (v - lo) / (hi - lo),
            Law::Cube(max) => (v / max).max(0.0).cbrt(),
            Law::Log(lo, hi) => (v.max(lo) / lo).ln() / (hi / lo).ln(),
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
/// Delay time (ms).
const DELAY_TIME: Law = Law::Cube(2000.0);
/// Delay line length (seconds), past the longest time.
const MAX_DELAY_S: f32 = 2.1;

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
    ("$ENGINE_PAR_DL_TIME_UNIT", Kind::Delay, 4, Law::Raw),
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

/// Keeps decaying feedback out of subnormal floats.
const ANTI_DENORMAL: f32 = 1e-20;

/// A per-sample stereo stage: Saturation (stored as `0x1d`, Kontakt's
/// "Surround Panner" slot class but `$ENGINE_PAR_SHAPE`'s effect),
/// Distortion, Lo-Fi, Skreamer, Tape Saturator.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Drive {
    kind: DriveKind,
    c: [f32; 8],
    /// Filter and sample-hold states, left then right.
    s: [f32; 8],
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
        self.kind = k;
        let x = |i: usize| f[i].clamp(0.0, 1.0);
        self.c = match k {
            // Shape -1..=1: positive bends toward tanh, negative expands.
            DriveKind::Saturator => [f[0].clamp(-1.0, 1.0), 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            // Gain 0..=48 dB into tanh (transistor: hard clip), half of it
            // made up after; damping is a low-pass 20 kHz..=200 Hz.
            DriveKind::Distortion => [
                db(48.0 * x(1)),
                db(-24.0 * x(1)),
                one_pole(20_000.0 * 0.01f32.powf(x(2)), rate),
                f32::from(f[0] >= 0.5),
                0.0,
                0.0,
                0.0,
                0.0,
            ],
            // Bits 1..=32; the sample rate held down by 1..=64 times.
            DriveKind::LoFi => {
                let bits = 1.0 + 31.0 * x(0);
                let step = if bits >= 24.0 { 0.0 } else { (1.0 - bits).exp2() };
                let hold = 1.0 + 63.0 * x(1).powi(3);
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
        };
        true
    }

    pub(crate) fn clear(&mut self) {
        self.s = [0.0; 8];
    }

    pub(crate) fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        match self.kind {
            DriveKind::Saturator => {
                let s = self.c[0];
                if s == 0.0 {
                    return;
                }
                let e = s.abs();
                let curve = |x: f32| {
                    let bent = if s > 0.0 { 0.5 * soft(2.0 * x) } else { x * x.abs().min(1.0) };
                    x + e * (bent - x)
                };
                left.iter_mut().chain(right.iter_mut()).for_each(|x| *x = curve(*x));
            }
            DriveKind::Distortion => {
                let [g, out, a, hard, ..] = self.c;
                for (ch, buf) in [left, right].into_iter().enumerate() {
                    let mut lp = self.s[ch];
                    for x in buf.iter_mut() {
                        let y = if hard > 0.0 { (g * *x).clamp(-1.0, 1.0) } else { soft(g * *x) };
                        lp += a * (y * out - lp) + ANTI_DENORMAL;
                        *x = lp;
                    }
                    self.s[ch] = lp;
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
struct Comp {
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
struct Transient {
    input: f32,
    attack: f32,
    sustain: f32,
    k: [f32; 4],
    env: [f32; 2],
}

impl Transient {
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
        self.pos = (self.pos + 1) & self.mask;
        self.buf[0][self.pos] = l;
        self.buf[1][self.pos] = r;
    }

    fn clear(&mut self) {
        self.buf.iter_mut().for_each(|b| b.fill(0.0));
    }
}

/// Legacy Delay: time (ms while the unit is -1, as stored, or
/// `$NI_SYNC_UNIT_ABS`; other units read as sixteenths at 120 BPM), damping (a low-pass in the loop, 20 kHz..=1 kHz), pan (how much
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
    fn tune(&mut self, f: &Fields, rate: f32) {
        // ponytail: no host tempo here; synced delays assume 120 BPM.
        let ms = if f[4] <= 0.0 { f[0] } else { f[0] * 125.0 };
        self.frames = (ms.clamp(1.0, MAX_DELAY_S * 1000.0 - 10.0) * 0.001 * rate).max(1.0);
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
    dsp: Dsp,
}

impl Block {
    /// `None` for effects without DSP here. Allocates.
    pub(crate) fn new(fx: &Effect, rate: f32) -> Option<Box<Self>> {
        let kind = fx.kind;
        // Filters, EQs and the Solid G-EQ: the group filter's sections.
        if matches!(fx.params, Params::Filter(_) | Params::Eq(_)) || kind == Kind::SolidGeq {
            let dsp = Dsp::Filter(RackFilter::new(fx, rate)?);
            return Some(Box::new(Self { kind, fields: [0.0; FIELDS], rate, dsp }));
        }
        let fields = fields(&fx.params)?;
        let dsp = match kind {
            k if Drive::supports(k) => Dsp::Drive(Drive::default()),
            Kind::Compressor | Kind::Limiter | Kind::SolidBusComp | Kind::FeedbackCompressor => Dsp::Comp(Comp::new()),
            Kind::TransientMaster => {
                Dsp::Transient(Transient { input: 1.0, attack: 0.0, sustain: 0.0, k: [1.0; 4], env: [FLOOR_DB; 2] })
            }
            Kind::Delay => Dsp::Delay(Delay {
                lines: Lines::new((MAX_DELAY_S * rate) as usize),
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
        let mut out = Box::new(Self { kind, fields, rate, dsp });
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
            Dsp::Delay(d) => d.tune(f, rate),
            Dsp::Sweep(s) => s.tune(kind, f, rate),
            Dsp::Phaser(p) => p.tune(f, rate),
            Dsp::Filter(_) => {}
        }
    }

    /// Set stored `field` of a `kind` effect from a script's normalized
    /// value; false when this is another effect.
    pub(crate) fn set(&mut self, kind: Kind, field: u8, x: f32) -> bool {
        if let Dsp::Filter(f) = &mut self.dsp {
            return f.set(kind, field, stored(kind, field, x));
        }
        let Some(v) = (kind == self.kind).then_some(()).and(self.fields.get_mut(field as usize)) else {
            return false;
        };
        *v = stored(kind, field, x);
        self.tune();
        true
    }

    /// A stored field as the script reads it.
    pub(crate) fn get(&self, kind: Kind, field: u8) -> Option<f32> {
        if let Dsp::Filter(f) = &self.dsp {
            return f.get(kind, field);
        }
        (kind == self.kind).then_some(())?;
        Some(normalized(kind, field, *self.fields.get(field as usize)?))
    }

    pub(crate) fn clear(&mut self) {
        match &mut self.dsp {
            Dsp::Drive(d) => d.clear(),
            Dsp::Comp(c) => c.clear(),
            Dsp::Transient(t) => t.env = [FLOOR_DB; 2],
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
    fn drives_keep_quiet_signals_near_unity_and_bend_loud_ones() {
        // Saturation: unity small-signal gain, compressed peaks.
        let mut d = Drive::default();
        assert!(d.tune(Kind::SurroundPanner, &[0.5; FIELDS], RATE));
        let (mut l, mut r) = sine(200.0, 0.01, 4800);
        d.process(&mut l, &mut r);
        assert!((rms(&l) / (0.01 / 2f32.sqrt()) - 1.0).abs() < 0.01);
        let (mut l, mut r) = sine(200.0, 1.0, 4800);
        d.process(&mut l, &mut r);
        assert!(l.iter().fold(0f32, |m, x| m.max(x.abs())) < 0.85);
        // Lo-Fi at the stored defaults is all but transparent.
        let mut f = [0.0; FIELDS];
        f[..5].copy_from_slice(&[0.5, 0.1, 0.0, 0.0, 0.2]);
        assert!(d.tune(Kind::LoFi, &f, RATE));
        let (mut l, mut r) = sine(200.0, 0.5, 4800);
        let dry = l.clone();
        d.process(&mut l, &mut r);
        let err: Vec<f32> = l.iter().zip(&dry).map(|(a, b)| a - b).collect();
        assert!(rms(&err) < 0.02, "{}", rms(&err));
        // Crushed: 2 bits, held 64 times.
        f[..2].copy_from_slice(&[0.03, 1.0]);
        d.tune(Kind::LoFi, &f, RATE);
        let (mut l, mut r) = sine(200.0, 0.5, 4800);
        d.process(&mut l, &mut r);
        let levels: std::collections::BTreeSet<i32> = l.iter().map(|x| (x * 1000.0) as i32).collect();
        assert!(levels.len() <= 5, "{levels:?}");
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
