//! Port from v1 0cb7a8a0:src/fx/blocks.rs, native Distortion core and filters.
use super::{Planar, ProcessorState};

/// Native Tube Distortion scalar core; filters and Output follow it.
#[inline(always)]
fn tube_curve(x: f32, k: f32, negative: f32, positive: f32) -> f32 {
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
        let complement = 1.0 - r;
        positive * (1.0 - complement * complement * complement) + (1.0 - positive) * x
    }
}

/// Native Transistor core. The low-level joins avoid powers, and the
/// negative/positive power branches retain native float/double precision.
#[inline(always)]
fn transistor_curve(
    x: f32,
    drive: f32,
    power: f32,
    threshold: f32,
    scale: f32,
    inverse_power: f64,
) -> f32 {
    if drive == 0.0 || x >= 0.25 || x < -0.25 {
        return x;
    }
    if x < 0.0 {
        let magnitude = -x;
        let shaped = if magnitude < threshold {
            magnitude * scale
        } else {
            0.25 * (4.0 * magnitude).powf(power)
        };
        (1.0 - drive) * x - drive * shaped
    } else {
        let shaped = if x <= 1.0 / 4096.0 {
            x / scale
        } else {
            0.25 * f64::from(4.0 * x).powf(inverse_power) as f32
        };
        (1.0 - drive) * x + drive * shaped
    }
}

/// Keeps decaying feedback out of subnormal floats.
const ANTI_DENORMAL: f32 = 1e-20;

/// Native steady-state Distortion damping pole. The normalized cutoff uses
/// an exponential polynomial followed by a sine-ratio bilinear transform.
/// Script updates use the template-copy law; internal modulation is separate.
fn distortion_damping(damping: f32, rate: f32) -> f32 {
    let p = (1.0 - 0.4 * damping.clamp(0.0, 1.0)).clamp(0.0, 1.0);
    let x = (f64::from(p) * 6.15) as f32;
    let exp = 1.0
        + x * (1.0
            + x * (0.5
                + x * (1.0 / 6.0
                    + x * (1.0 / 24.0
                        + x * (1.0 / 120.0
                            + x * (1.0 / 720.0 + x * (1.0 / 5040.0 + x / 40320.0)))))));
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
    let (mut b0, mut b1, mut b2, a1, a2) = (
        transformed_b0,
        transformed_b1,
        transformed_b2,
        transformed_a1,
        transformed_a2,
    );
    let gain = (1.0 - f64::from(a2 - a1)) / f64::from((b0 - b1) + b2);
    b0 = (f64::from(b0) * gain) as f32;
    b1 = (f64::from(b1) * gain) as f32;
    b2 = (f64::from(b2) * gain) as f32;
    [b0, b1, b2, a1, a2]
}

pub(crate) struct Distortion {
    curve: [f32; 6],
    dc: [f32; 5],
}

impl Distortion {
    pub(super) fn compile(transistor: bool, drive: f32, damping: f32, rate: u32) -> Self {
        Self {
            curve: Self::curve(transistor, drive, damping, rate),
            dc: distortion_dc(rate as f32),
        }
    }

    fn curve(transistor: bool, drive: f32, damping: f32, rate: u32) -> [f32; 6] {
        let pole = distortion_damping(damping, rate as f32);
        if transistor {
            let threshold = f64::from(8. * drive - 12.).exp2() as f32;
            [
                drive,
                -60. / (48. * drive - 60.),
                pole,
                1.,
                threshold,
                (1. / 4096.) / threshold,
            ]
        } else {
            let negative = if drive > 0.75 {
                1.
            } else {
                (0.5625 - (0.75 - drive).powi(2)) * (16. / 9.)
            };
            let positive = if drive < 0.25 {
                0.
            } else {
                (0.5625 - (1. - drive).powi(2)) * (16. / 9.)
            };
            [1. + 1.5 * drive, 1., pole, 0., negative, positive]
        }
    }

    pub(super) fn process(&self, state: &mut ProcessorState, block: &mut Planar, len: usize) {
        let [drive, power, pole, transistor, c4, c5] = self.curve;
        let [b0, b1, b2, a1, a2] = self.dc;
        let inverse_power = 1. / f64::from(power);
        let b = 0.5 * (1. - pole);
        for (channel, buffer) in block.iter_mut().enumerate() {
            let base = 5 * channel;
            let [mut previous, mut lp, mut lp2, mut hp, mut hp2] =
                std::array::from_fn(|i| state.aux[base + i] as f32);
            for x in &mut buffer[..len] {
                let y = if transistor > 0. {
                    transistor_curve(*x as f32, drive, power, c4, c5, inverse_power)
                } else {
                    tube_curve(*x as f32, drive, c4, c5)
                };
                let damped = b * y + b * previous + pole * lp + ANTI_DENORMAL;
                let output = (((damped * b0 + lp * b1) + lp2 * b2) + hp * a1) + hp2 * a2;
                (previous, lp2, lp, hp2, hp) = (y, lp, damped, hp, output);
                *x = f64::from(output);
            }
            for (out, value) in state.aux[base..base + 5]
                .iter_mut()
                .zip([previous, lp, lp2, hp, hp2])
            {
                *out = f64::from(value);
            }
        }
    }

    pub(crate) fn damping_pole(&self) -> f64 {
        f64::from(self.curve[2])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distortion_retains_v1_native_transfer_and_filter_vectors() {
        let (mut error, mut signal) = (0., 0.);
        let inputs = [-0.25, -0.01, 0., 0.01, 0.25];
        let tube = [
            -0.10939426317087,
            -1.511801183946e-5,
            0.,
            0.072360783382485,
            0.85797649379469,
        ];
        let transistor = [-0.25, -0.0000390625, 0., 0.13132639022019, 0.25];
        for (x, expected) in inputs.into_iter().zip(tube) {
            let actual = f64::from(tube_curve(x, 2.5, 1., 1.));
            error += (actual - expected).powi(2);
            signal += expected * expected;
            assert!((actual - expected).abs() < 2e-7);
        }
        for (x, expected) in inputs.into_iter().zip(transistor) {
            let actual = f64::from(transistor_curve(x, 1., 5., 0.0625, 1. / 256., 0.2));
            error += (actual - expected).powi(2);
            signal += expected * expected;
            assert!((actual - expected).abs() < 2e-7);
        }
        let null_db = 10. * (error / signal).log10();
        assert!(null_db < -130.);
        println!("Distortion decimal-transfer-vector null: {null_db:.2} dB");
        for (damping, expected) in [
            (0., -0.748475234218230),
            (0.5, 0.326787237731620),
            (1., 0.746823690647603),
        ] {
            assert!((f64::from(distortion_damping(damping, 48000.)) - expected).abs() < 3e-6);
        }
        let dc = [
            0.9986164569854736,
            -1.9972329139709473,
            0.9986164569854736,
            1.9972310066223145,
            -0.9972348213195801,
        ];
        assert_eq!(distortion_dc(48000.), dc);
        let kernel = Distortion::compile(false, 0.523648, 0., 48000);
        let mut whole = [[0.; super::super::BLOCK]; 2];
        for (i, x) in whole[0][..32].iter_mut().enumerate() {
            *x = ((i as f64 * 0.7).sin()) * 0.1;
        }
        whole[1] = whole[0];
        let input = whole;
        let mut full_state = ProcessorState::default();
        kernel.process(&mut full_state, &mut whole, 32);
        let mut split_state = ProcessorState::default();
        let mut output = Vec::new();
        let mut at = 0;
        for count in [1, 7, 24] {
            let mut block = [[0.; super::super::BLOCK]; 2];
            for c in 0..2 {
                block[c][..count].copy_from_slice(&input[c][at..at + count]);
            }
            kernel.process(&mut split_state, &mut block, count);
            output.extend_from_slice(&block[0][..count]);
            at += count;
        }
        assert_eq!(output, whole[0][..32]);
        assert_eq!(split_state.aux, full_state.aux);
    }
}

/// NI Distortion: template copy 0x1408373c0 and embedded LP copy 0x140837e40
/// replace Drive/current Damping directly when no internal modulator is attached.
/// shortcut: internal modulation has a separate 32-frame clock; wire it when admitted.
pub(crate) struct LiveDistortion {
    pub(crate) drive: super::control::PreparedParameter,
    pub(crate) damping: super::control::PreparedParameter,
    transistor: bool,
    rate: u32,
    dc: [f32; 5],
}
impl LiveDistortion {
    pub(super) fn compile(
        transistor: bool,
        drive: super::control::PreparedParameter,
        damping: super::control::PreparedParameter,
        rate: u32,
    ) -> Self {
        Self {
            transistor,
            drive,
            damping,
            rate,
            dc: distortion_dc(rate as f32),
        }
    }
    pub(super) fn process(
        &self,
        state: &mut ProcessorState,
        block: &mut Planar,
        len: usize,
        parameters: &[super::ControlRamp],
        at: u64,
    ) {
        let drive = self.drive.value(parameters, at, None) as f32;
        let damping = self.damping.value(parameters, at, None) as f32;
        // These event-rate lanes are constant within a render fragment. Retain
        // coefficients and audio histories independently across parameter edits.
        if state.z[1][0] == 0. || state.z[0] != [f64::from(drive), f64::from(damping)] {
            let curve = Distortion::curve(self.transistor, drive, damping, self.rate);
            for (out, value) in state.aux[10..16].iter_mut().zip(curve) {
                *out = f64::from(value);
            }
            state.z[0] = [f64::from(drive), f64::from(damping)];
            state.z[1][0] = 1.;
        }
        Distortion {
            curve: std::array::from_fn(|i| state.aux[10 + i] as f32),
            dc: self.dc,
        }
        .process(state, block, len);
    }
}
