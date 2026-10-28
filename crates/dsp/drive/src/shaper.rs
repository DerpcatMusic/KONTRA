//! The waveshaper: a memoryless curve run at twice the rate, then an
//! optional one-pole low-pass and a second-order Butterworth high-pass
//! (its DC blocker). The curves are v2's, decompiled from Kontakt's
//! Saturation (Classic) and Distortion (Tube, Transistor) and checked
//! against v1's measured transfer vectors (tests/shaper.rs).
//!
//! Twice the rate because it is audible and cheap: a full-scale 1.76 kHz
//! tone through `Tube(1.0)` at 48 kHz aliases at −30 dB under its
//! harmonics, and at −42 dB at 2x (`Soft(1.0)`: −34 and −60 dB), for one
//! halfband pair of 8 allpasses each way.

use kontra_dsp_core::{Biquad, flush};

use crate::halfband::Halfband;

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
}

impl Curve {
    /// `x` through the curve (at the base rate, no filters).
    #[must_use]
    pub fn transfer(self, x: f32) -> f32 {
        Prepared::new(self).apply(x)
    }
}

/// The filters after the curve.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tone {
    /// A one-pole low-pass (bilinear, zero at Nyquist).
    pub lowpass_hz: Option<f32>,
    /// A second-order Butterworth high-pass.
    pub highpass_hz: Option<f32>,
}

impl Tone {
    pub const NONE: Self = Self {
        lowpass_hz: None,
        highpass_hz: None,
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

#[derive(Clone, Debug, PartialEq)]
pub struct Shaper {
    curve: Prepared,
    halfband: Halfband,
    /// The low-pass pole and per channel `[x₁, y₁]`.
    lowpass: Option<(f64, [[f64; 2]; 2])>,
    highpass: Option<Biquad>,
}

impl Shaper {
    #[must_use]
    pub fn new(curve: Curve, tone: Tone, rate: u32) -> Self {
        let r = f64::from(rate.max(1));
        let warp = |hz: f32| (std::f64::consts::PI * f64::from(hz).clamp(1.0, 0.49 * r) / r).tan();
        let lowpass = tone.lowpass_hz.map(|hz| {
            let g = warp(hz);
            ((1.0 - g) / (1.0 + g), [[0.0; 2]; 2])
        });
        let highpass = tone.highpass_hz.map(|hz| {
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
        Self {
            curve: Prepared::new(curve),
            halfband: Halfband::new(rate),
            lowpass,
            highpass,
        }
    }

    /// Moves the curve's amount; filter states carry on.
    pub fn set_curve(&mut self, curve: Curve) {
        self.curve = Prepared::new(curve);
    }

    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        let curve = self.curve;
        for frame in io.iter_mut() {
            self.halfband.run(frame, |x| curve.apply(x));
        }
        self.halfband.flush();
        if let Some((pole, state)) = &mut self.lowpass {
            let b = 0.5 * (1.0 - *pole);
            for frame in io.iter_mut() {
                for (x, [x1, y1]) in frame.iter_mut().zip(state.iter_mut()) {
                    let v = f64::from(*x);
                    *y1 = b * (v + *x1) + *pole * *y1;
                    *x1 = v;
                    *x = *y1 as f32;
                }
            }
            flush(state.as_flattened_mut());
        }
        if let Some(h) = &mut self.highpass {
            h.process(io);
        }
    }

    /// Whether its filters have rung out.
    #[must_use]
    pub fn is_silent(&self) -> bool {
        self.highpass.as_ref().is_none_or(Biquad::is_silent)
            && self
                .lowpass
                .is_none_or(|(_, s)| s.iter().flatten().all(|v| v.abs() < 1e-10))
    }

    pub fn reset(&mut self) {
        self.halfband.reset();
        if let Some((_, s)) = &mut self.lowpass {
            *s = [[0.0; 2]; 2];
        }
        if let Some(h) = &mut self.highpass {
            h.reset();
        }
    }
}
