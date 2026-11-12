//! The waveshaper: base-rate filters (an optional one-pole low-pass and
//! [`Stage::In`] sections), then at twice the rate [`Stage::Pre`] sections,
//! a memoryless curve and [`Stage::Post`] sections, then base-rate filters
//! again (an optional one-pole low-pass, a second-order Butterworth
//! high-pass, [`Stage::Out`] sections). The curves are ported from v2
//! sampler-core dsp/color/{shaping,distortion}.rs and checked against v1's
//! measured transfer vectors (tests/drive.rs); the format translators carry
//! their provenance and knob laws.
//!
//! Twice the rate because it is audible and cheap: a full-scale 1.76 kHz
//! tone through `Tube(1.0)` at 48 kHz aliases at −30 dB under its
//! harmonics, and at −42 dB at 2x (`Soft(1.0)`: −34 and −60 dB), for one
//! halfband pair of 8 allpasses each way.
//!
//! The shaper is two parts so a voice rack can hold it: the oversampled
//! [`Core`] lives in the rack's unit enum, the base-rate [`Outer`] beside it.

use kontra_dsp_core::{Biquad, Biquad32, Halfband, Section, Sections, Stage};

/// A transfer curve and its amount.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve {
    /// −1..=1: above 0 a soft clip `2|x| − x²` blended in (fully at 0.25,
    /// with gain up to 4 beyond); below 0 a cubic expansion `(x² + q)x/(1 + q)`.
    Soft(f32),
    /// 0..=1: an asymmetric rational clip, harder on the positive half.
    Tube(f32),
    /// 0..=1: a power law on small signals (|x| < 0.25) that thins the
    /// negative half and thickens the positive one.
    Power(f32),
    /// The drive `d`: `sin(d·x)`.
    Sine(f32),
    /// The drive `d`: `asin(sin(d·x))`, a triangle fold.
    Fold(f32),
    /// The drive `d`: `tanh(d·x)`.
    Tanh(f32),
    /// The drive `d`: `atan(d·x)`.
    Atan(f32),
    /// The drive `d`: `x / √(1 + (d·x)²)`, unity gain on small signals and
    /// a ceiling of `1/d`.
    Saturate(f32),
    /// An odd curve from `points` spread evenly over 0..=1, read at `d·|x|`
    /// (held at the last point beyond 1) with linear interpolation, scaled
    /// by `1 / (d·slope at 0)` so small signals pass at unity.
    Table { drive: f32, points: &'static [f32] },
}

impl Curve {
    /// `x` through the curve (at the base rate, no filters).
    #[must_use]
    pub fn transfer(self, x: f32) -> f32 {
        Prepared::new(self).apply(x)
    }
}

/// The filters around the curve.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tone {
    /// A one-pole low-pass before the curve, pole `e^(−2π·hz/rate)`.
    pub pre_lowpass_hz: Option<f32>,
    /// A one-pole low-pass (bilinear, zero at Nyquist), its corner in Hz.
    pub lowpass_hz: Option<f32>,
    /// A second-order Butterworth high-pass, its corner in Hz.
    pub highpass_hz: Option<f32>,
    /// Linear sections at their stages; past a stage's room
    /// ([`Outer`]: 2 in, 3 out; [`Core`]: 6 oversampled) the rest are left
    /// out.
    pub sections: Sections,
}

impl Tone {
    /// No filters.
    pub const NONE: Self = Self {
        pre_lowpass_hz: None,
        lowpass_hz: None,
        highpass_hz: None,
        sections: Sections::NONE,
    };
}

/// A curve with its constants worked out once per change.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Prepared {
    Identity,
    Clip {
        amount: f32,
    },
    Expand {
        q: f32,
    },
    Tube {
        k: f32,
        negative: f32,
        positive: f32,
    },
    Power {
        drive: f32,
        power: f32,
        threshold: f32,
        scale: f32,
        inverse: f64,
    },
    Sine(f32),
    Fold(f32),
    Tanh(f32),
    Atan(f32),
    Saturate(f32),
    Table {
        /// The drive times the spans between points.
        scale: f32,
        makeup: f32,
        points: &'static [f32],
    },
}

impl Prepared {
    fn new(curve: Curve) -> Self {
        match curve {
            Curve::Soft(s) if s.abs() <= 1e-4 || s.is_nan() => Self::Identity,
            Curve::Soft(s) if s < 0.0 => Self::Expand {
                q: 0.975 * (4.0 + 4.0 * s.max(-1.0)) + 0.1,
            },
            Curve::Soft(s) => Self::Clip {
                amount: 4.0 * s.min(1.0),
            },
            Curve::Tube(d) if d.is_nan() => Self::Identity,
            Curve::Tube(d) => {
                let d = d.clamp(0.0, 1.0);
                let negative = if d > 0.75 {
                    1.0
                } else {
                    (0.5625 - (0.75 - d).powi(2)) * (16.0 / 9.0)
                };
                let positive = if d < 0.25 {
                    0.0
                } else {
                    (0.5625 - (1.0 - d).powi(2)) * (16.0 / 9.0)
                };
                Self::Tube {
                    k: 1.0 + 1.5 * d,
                    negative,
                    positive,
                }
            }
            Curve::Sine(d) => Self::Sine(d),
            Curve::Fold(d) => Self::Fold(d),
            Curve::Tanh(d) => Self::Tanh(d),
            Curve::Atan(d) => Self::Atan(d),
            Curve::Saturate(d) if d.is_nan() || d <= 0.0 => Self::Identity,
            Curve::Saturate(d) => Self::Saturate(d * d),
            Curve::Table { drive, points } => {
                let spans = points.len().saturating_sub(1) as f32;
                let slope = match points {
                    [a, b, ..] => (b - a) * spans,
                    _ => 0.0,
                };
                if drive.is_nan() || drive <= 0.0 || slope.is_nan() || slope <= 0.0 {
                    return Self::Identity;
                }
                Self::Table {
                    scale: drive * spans,
                    makeup: 1.0 / (drive * slope),
                    points,
                }
            }
            Curve::Power(d) if d.is_nan() || d <= 0.0 => Self::Identity,
            Curve::Power(d) => {
                let d = d.min(1.0);
                let threshold = f64::from(8.0 * d - 12.0).exp2() as f32;
                let power = -60.0 / (48.0 * d - 60.0);
                Self::Power {
                    drive: d,
                    power,
                    threshold,
                    scale: (1.0 / 4096.0) / threshold,
                    inverse: 1.0 / f64::from(power),
                }
            }
        }
    }

    fn apply(self, x: f32) -> f32 {
        match self {
            Self::Identity => x,
            Self::Clip { amount } => {
                let m = (if amount >= 1.0 { amount * x } else { x }).abs().min(1.0);
                let bent = (2.0 * m - m * m).copysign(x);
                if amount >= 1.0 {
                    bent
                } else {
                    (1.0 - amount) * x + amount * bent
                }
            }
            Self::Expand { q } => (x * x + q) * x / (1.0 + q),
            Self::Sine(d) => (d * x).sin(),
            Self::Fold(d) => (d * x).sin().asin(),
            Self::Tanh(d) => (d * x).tanh(),
            Self::Atan(d) => (d * x).atan(),
            Self::Saturate(d2) => x / (1.0 + d2 * x * x).sqrt(),
            Self::Table {
                scale,
                makeup,
                points,
            } => {
                let at = (scale * x.abs()).min(points.len().saturating_sub(1) as f32);
                let i = at as usize;
                let a = points.get(i).copied().unwrap_or_default();
                let b = points.get(i + 1).copied().unwrap_or(a);
                ((b - a) * (at - i as f32) + a).copysign(x) * makeup
            }
            Self::Tube {
                k,
                negative,
                positive,
            } => tube(x, k, negative, positive),
            Self::Power {
                drive,
                power,
                threshold,
                scale,
                inverse,
            } => {
                if !(-0.25..0.25).contains(&x) {
                    return x;
                }
                if x < 0.0 {
                    let m = -x;
                    let shaped = if m < threshold {
                        m * scale
                    } else {
                        0.25 * (4.0 * m).powf(power)
                    };
                    (1.0 - drive) * x - drive * shaped
                } else {
                    let shaped = if x <= 1.0 / 4096.0 {
                        x / scale
                    } else {
                        0.25 * f64::from(4.0 * x).powf(inverse) as f32
                    };
                    (1.0 - drive) * x + drive * shaped
                }
            }
        }
    }
}

fn tube(x: f32, k: f32, negative: f32, positive: f32) -> f32 {
    if x < 0.0 {
        if negative == 0.0 {
            return x;
        }
        let r = ((k - x) * x / (1.0 + (x - (k - 1.0)) * x)).clamp(-1.0, 1.0);
        negative * (r * r * r) + (1.0 - negative) * x
    } else {
        if positive == 0.0 {
            return x;
        }
        let r = ((k + x) * x / (1.0 + (x + k - 1.0) * x)).clamp(-1.0, 1.0);
        let c = 1.0 - r;
        positive * (1.0 - c * c * c) + (1.0 - positive) * x
    }
}

/// Base-rate sections before the core.
const IN: usize = 2;
/// Base-rate sections after the core.
const OUT: usize = 3;
/// Oversampled sections around the curve.
const INNER: usize = 6;

/// A section that passes its input.
fn identity() -> Biquad {
    Biquad::new([1.0, 0.0, 0.0, 1.0, 0.0, 0.0])
}

/// `items` into `room`'s slots in order, and how many went in; the rest are
/// left out.
fn fill<T>(room: &mut [T], items: impl Iterator<Item = T>) -> u8 {
    let mut n = 0;
    for (slot, item) in room.iter_mut().zip(items) {
        *slot = item;
        n += 1;
    }
    n
}

/// The shaper's base-rate filters, before and after its [`Core`].
#[derive(Clone, Debug, PartialEq)]
pub struct Outer {
    before: [Biquad; IN],
    after: [Biquad; OUT],
    counts: [u8; 2],
}

impl Outer {
    /// No filters.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            before: std::array::from_fn(|_| identity()),
            after: std::array::from_fn(|_| identity()),
            counts: [0; 2],
        }
    }

    fn new(tone: Tone, rate: u32) -> Self {
        let r = f64::from(rate.max(1));
        let warp = |hz: f32| (std::f64::consts::PI * f64::from(hz).clamp(1.0, 0.49 * r) / r).tan();
        let staged = |stage: Stage| {
            tone.sections
                .iter()
                .filter(move |(s, _)| *s == stage)
                .map(move |(_, s)| Biquad::analog(s, rate))
        };
        // A one-pole without a zero at Nyquist: pole `e^(−2π·hz/rate)`.
        let pre = tone.pre_lowpass_hz.filter(|hz| !hz.is_nan()).map(|hz| {
            let pole = (-std::f64::consts::TAU * f64::from(hz.max(0.0)) / r).exp();
            Biquad::new([1.0 - pole, 0.0, 0.0, 1.0, -pole, 0.0])
        });
        // One pole, bilinear (a zero at Nyquist).
        let lowpass = tone.lowpass_hz.filter(|hz| !hz.is_nan()).map(|hz| {
            let g = warp(hz);
            let pole = (1.0 - g) / (1.0 + g);
            let b = 0.5 * (1.0 - pole);
            Biquad::new([b, b, 0.0, 1.0, -pole, 0.0])
        });
        let highpass = tone.highpass_hz.filter(|hz| !hz.is_nan()).map(|hz| {
            let (k, q) = (warp(hz), std::f64::consts::SQRT_2);
            Biquad::new([
                1.0,
                -2.0,
                1.0,
                1.0 + q * k + k * k,
                2.0 * (k * k - 1.0),
                1.0 - q * k + k * k,
            ])
        });
        let mut o = Self::empty();
        o.counts = [
            fill(&mut o.before, pre.into_iter().chain(staged(Stage::In))),
            fill(
                &mut o.after,
                lowpass
                    .into_iter()
                    .chain(highpass)
                    .chain(staged(Stage::Out)),
            ),
        ];
        o
    }

    /// The filters before the core, on `io`.
    pub fn before(&mut self, io: &mut [[f32; 2]]) {
        for f in self.before.iter_mut().take(usize::from(self.counts[0])) {
            f.process(io);
        }
    }

    /// The filters after the core, on `io`.
    pub fn after(&mut self, io: &mut [[f32; 2]]) {
        for f in self.after.iter_mut().take(usize::from(self.counts[1])) {
            f.process(io);
        }
    }

    /// Takes `new`'s filters; a slot in use in both keeps its state.
    pub fn retune(&mut self, new: &Self) {
        let [b, a] = self.counts;
        for (n, (o, f)) in self.before.iter_mut().zip(&new.before).enumerate() {
            if n < usize::from(b) {
                o.retune(f)
            } else {
                *o = f.clone()
            }
        }
        for (n, (o, f)) in self.after.iter_mut().zip(&new.after).enumerate() {
            if n < usize::from(a) {
                o.retune(f)
            } else {
                *o = f.clone()
            }
        }
        self.counts = new.counts;
    }

    pub fn reset(&mut self) {
        for f in self.before.iter_mut().chain(&mut self.after) {
            f.reset();
        }
    }
}

/// The shaper's oversampled part: sections, the curve, sections.
///
/// ponytail: the sections run one after another, a frame at a time (a
/// Tape Saturator's eleven cost 35 µs per 512-frame block at 48 kHz over
/// the bare shaper's 10); pairing sections across SIMD lanes is the upgrade
/// if per-voice shapers show in a profile.
#[derive(Clone, Debug, PartialEq)]
pub struct Core {
    curve: Prepared,
    halfband: Halfband,
    inner: [Biquad32; INNER],
    /// Sections before the curve, then after it.
    counts: [u8; 2],
}

impl Core {
    fn new(curve: Curve, tone: Tone, rate: u32) -> Self {
        let twice = rate.max(1).saturating_mul(2);
        let staged = |stage: Stage| {
            tone.sections
                .iter()
                .filter(move |(s, _)| *s == stage)
                .map(move |(_, s): (Stage, Section)| Biquad32::analog(s, twice))
        };
        let mut inner = [Biquad32::default(); INNER];
        let pre = fill(&mut inner, staged(Stage::Pre));
        let (_, rest) = inner.split_at_mut(usize::from(pre));
        let post = fill(rest, staged(Stage::Post));
        Self {
            curve: Prepared::new(curve),
            halfband: Halfband::new(rate),
            inner,
            counts: [pre, post],
        }
    }

    /// Shapes `io` in place.
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        let curve = self.curve;
        let [pre, post] = self.counts.map(usize::from);
        let (before, rest) = self.inner.split_at_mut(pre.min(INNER));
        let after = rest.get_mut(..post).unwrap_or_default();
        self.halfband.process(io, |mut f| {
            for s in before.iter_mut() {
                f = s.tick(f);
            }
            f = f.map(|x| curve.apply(x));
            for s in after.iter_mut() {
                f = s.tick(f);
            }
            f
        });
        for s in &mut self.inner {
            s.settle();
        }
    }

    /// Takes `new`'s curve and sections; a slot in use in both keeps its
    /// state, and the halfband carries on.
    pub fn retune(&mut self, new: &Self) {
        let used = usize::from(self.counts[0] + self.counts[1]);
        for (n, (o, f)) in self.inner.iter_mut().zip(&new.inner).enumerate() {
            if n < used { o.retune(f) } else { *o = *f }
        }
        self.counts = new.counts;
        self.curve = new.curve;
    }

    /// Moves the curve's amount; filter states carry on.
    pub fn set_curve(&mut self, curve: Curve) {
        self.curve = Prepared::new(curve);
    }

    pub fn reset(&mut self) {
        self.halfband.reset();
        for s in &mut self.inner {
            s.reset();
        }
    }
}

/// A stereo waveshaper with its oversampling and filter state.
#[derive(Clone, Debug, PartialEq)]
pub struct Shaper {
    pub outer: Outer,
    pub core: Core,
}

impl Shaper {
    /// A shaper at `rate`; each corner is held within 1 Hz..=0.49 of the
    /// rate it runs at, and a NaN corner leaves its filter out.
    #[must_use]
    pub fn new(curve: Curve, tone: Tone, rate: u32) -> Self {
        Self {
            outer: Outer::new(tone, rate),
            core: Core::new(curve, tone, rate),
        }
    }

    /// Moves the curve and every filter to `curve` and `tone`; the halfband
    /// and filter states carry on (a filter that appears starts from rest).
    /// Never allocates.
    pub fn retune(&mut self, curve: Curve, tone: Tone, rate: u32) {
        let new = Self::new(curve, tone, rate);
        self.outer.retune(&new.outer);
        self.core.retune(&new.core);
    }

    /// Moves the curve's amount; filter states carry on.
    pub fn set_curve(&mut self, curve: Curve) {
        self.core.set_curve(curve);
    }

    /// Shapes `io` (frames of `[left, right]`) in place.
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        self.outer.before(io);
        self.core.process(io);
        self.outer.after(io);
    }

    /// Clears the halfband and filter state; the curve and tone stay.
    pub fn reset(&mut self) {
        self.outer.reset();
        self.core.reset();
    }
}
