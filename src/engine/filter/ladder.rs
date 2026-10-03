//! Native Ladder LP4's single-rate, steady-parameter kernel.
//! HQ resampling and Kontakt's control interpolation remain unsupported.

use std::sync::OnceLock;

static EXPONENTIAL: OnceLock<[f32; 2401]> = OnceLock::new();

/// Warm in the loader, including when a voice will later change subtype.
pub(crate) fn prepare() {
    EXPONENTIAL.get_or_init(|| {
        std::array::from_fn(|i| 2.0_f64.powf(i as f64 * (1.0 / 60.0) - 20.0) as f32)
    });
}

#[derive(Clone, Copy, Default)]
pub(super) struct Ladder {
    state: [[f32; 5]; 2],
    g: f32,
    reciprocal: f32,
    feedback: f32,
    compensation: f32,
    gain: f32,
    correction: f32,
    correction_reciprocal: f32,
    correction_previous: f32,
    correction_gain: f32,
}

/// The native exponential table is generated, rather than a captured table.
/// Adjacent entries are rounded before linear interpolation.
fn exponential(index: f32) -> f32 {
    let index = index.clamp(0.0, 2399.0);
    let integer = index as usize;
    // All prepared filter and script constructors warm this table offthread.
    // The audio path cannot initialize it, acquire a lock, or evaluate pow.
    let entries = EXPONENTIAL
        .get()
        .expect("Ladder table must be worker-prepared");
    let a = entries[integer];
    a + (entries[integer + 1] - a) * (index - integer as f32)
}

pub(crate) fn cutoff(value: f32) -> f32 {
    exponential(1481.8816 + 575.0 * value.clamp(0.0, 1.0))
}

fn pole(w: f32) -> f32 {
    let w2 = w * w;
    ((40.802_63 * w2 + 10.335_426) * w2 + std::f32::consts::PI) * w
}

fn compensation(q: f32) -> f32 {
    // Algebraic form of the three native LP4 interpolation intervals.
    // An independent float-order comparison bounds its difference to 2 ULP.
    let p = f64::from(q * q);
    if p < 0.34 {
        let u = p / 0.34;
        let t = (2.0 - u) * u;
        (1.02 + 0.98 * (0.49 * t * t + 0.51 * t)) as f32
    } else if p < 0.83 {
        2.0
    } else {
        let u = (p - 0.83) / 0.17;
        (2.0 - 0.86 * (2.0 - u) * u) as f32
    }
}

impl Ladder {
    /// Small-signal response, including the correction in the feedback path.
    /// Large signals additionally undergo the input soft clip.
    pub(super) fn magnitude(knobs: [f32; 3], hz: f32, rate: f32) -> f32 {
        let mut filter = Self::default();
        filter.tune(knobs, rate);
        let mul = |[a, b]: [f64; 2], [c, d]: [f64; 2]| [a * c - b * d, a * d + b * c];
        let div = |[a, b]: [f64; 2], [c, d]: [f64; 2]| {
            let scale = c * c + d * d;
            [(a * c + b * d) / scale, (b * c - a * d) / scale]
        };
        let w = 2.0 * std::f64::consts::PI * f64::from(hz.min(0.499 * rate) / rate);
        let (sine, cosine) = w.sin_cos();
        let transfer = |g: f32| {
            let g = f64::from(g);
            div(
                [g * (1.0 + cosine), -g * sine],
                [1.0 + g + (g - 1.0) * cosine, -(g - 1.0) * sine],
            )
        };
        let h = transfer(filter.g);
        let h2 = mul(h, h);
        let h4 = mul(h2, h2);
        let correction = transfer(filter.correction).map(|v| v * f64::from(filter.correction_gain));
        let c4 = f64::from((filter.g * filter.reciprocal).powi(4));
        let feedback = f64::from(filter.feedback);
        let d = [
            1.0 - feedback * c4 * correction[0],
            -feedback * c4 * correction[1],
        ];
        let denominator = mul(h4, [1.0 + correction[0], correction[1]]);
        let response = div(
            mul(h4, d),
            [
                d[0] + feedback * denominator[0],
                d[1] + feedback * denominator[1],
            ],
        );
        ((response[0] * response[0] + response[1] * response[1]).sqrt()
            * f64::from(filter.gain * filter.compensation)) as f32
    }

    pub(super) fn tune(&mut self, [frequency, resonance, gain]: [f32; 3], rate: f32) {
        self.g = pole(cutoff(frequency) / rate).min(8.0);
        self.reciprocal = 1.0 / (1.0 + self.g);
        let q = 1.0 - (1.0 - resonance).powi(2);
        self.feedback = 4.16 * q;
        self.compensation = compensation(q);
        self.gain = exponential(1200.0 + 119.589_41 * gain);
        // R is a dimensionless fixed feedback gain; 80/R is in Hz.
        let r = exponential(1196.0 + 0.681_396_5);
        let w = (80.0 / r) / rate;
        let w2 = w * w;
        self.correction = std::f32::consts::PI * w * (1.0 + w2 * (3.289_868_4 + 12.987_881 * w2));
        self.correction_reciprocal = 1.0 / (1.0 + self.correction);
        self.correction_previous = self.correction - 1.0;
        self.correction_gain = r * r - 1.0;
    }

    pub(super) fn clear(&mut self) {
        self.state = [[0.0; 5]; 2];
    }

    pub(super) fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let c = self.g * self.reciprocal;
        let c4 = ((c * c) * c) * c;
        for (channel, samples) in [left, right].into_iter().enumerate() {
            let state = &mut self.state[channel];
            for sample in samples {
                let input = self.gain * *sample;
                let mut histories = 2.0 * state[0] * c + 2.0 * state[1];
                histories = histories * c + 2.0 * state[2];
                histories = histories * c + 2.0 * state[3];
                let predicted =
                    (c4 * input + histories * self.reciprocal) / (1.0 + self.feedback * c4);
                let feedback = self.feedback * predicted;
                let correction =
                    (feedback - state[4] * self.correction_previous) * self.correction_reciprocal;
                let feedback =
                    feedback + (state[4] + correction) * self.correction * self.correction_gain;
                state[4] = correction;
                let mut value = (input - feedback).clamp(-24.0, 24.0);
                value *= 1.0 - value.abs() / 48.0;
                for history in &mut state[..4] {
                    let previous = *history;
                    let predicted = (2.0 * previous + self.g * value) * self.reciprocal;
                    *history = previous + self.g * (value - predicted);
                    value = previous + *history;
                }
                *sample = value * self.compensation;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_ladder_lp4_scalar_laws_partition_reset_and_resonance() {
        prepare();
        assert_eq!(std::mem::size_of::<Ladder>(), 76);
        assert!((cutoff(0.0) - 25.956726).abs() < 0.0001);
        assert!((cutoff(1.0) - 19912.266).abs() < 0.01);
        assert_eq!(exponential(1200.0), 1.0, "native Gain0 is unity");
        assert!((exponential(1200.0 + 119.58941) - 10_f32.powf(12.0 / 20.0)).abs() < 0.0001);
        for rate in [32_000.0, 44_100.0, 48_000.0, 96_000.0] {
            for frequency in [0.0, 0.5, 1.0] {
                let mut filter = Ladder::default();
                filter.tune([frequency, 0.0, 0.0], rate);
                // Independent double direct-form one-poles. They have the
                // same transfer function as the half-integrator state, but
                // neither its histories nor its update recurrence.
                let w = f64::from(cutoff(frequency)) / f64::from(rate);
                let g = (std::f64::consts::PI
                    + w * w * (10.335426330566406 + 40.80263137817383 * w * w))
                    * w;
                let g = g.min(8.0);
                let (b, a) = (g / (1.0 + g), (1.0 - g) / (1.0 + g));
                let mut direct = [[0_f64; 2]; 4];
                let mut reference = [0.0; 512];
                let source: [f32; 512] = std::array::from_fn(|i| match i {
                    0 => 0.5,
                    53 => -0.75,
                    129 => 24.0,
                    257 => -48.0,
                    _ => 0.0,
                });
                for (sample, expected) in source.iter().zip(&mut reference) {
                    let input = f64::from(*sample).clamp(-24.0, 24.0);
                    let mut x = input * (1.0 - input.abs() / 48.0);
                    for history in &mut direct {
                        let y = b * (x + history[0]) + a * history[1];
                        *history = [x, y];
                        x = y;
                    }
                    *expected = x * 1.02;
                }
                let (mut left, mut right) = (source, source.map(|v| -v));
                let mut partitioned = filter;
                let (mut split_l, mut split_r) = (left, right);
                filter.process(&mut left, &mut right);
                for range in [0..1, 1..37, 37..128, 128..129, 129..512] {
                    partitioned.process(&mut split_l[range.clone()], &mut split_r[range]);
                }
                assert_eq!(left, split_l);
                assert_eq!(right, split_r);
                for ((&l, &r), expected) in left.iter().zip(&right).zip(&reference) {
                    assert_eq!(l, -r);
                    assert!(
                        (f64::from(l) - expected).abs() < 0.00002,
                        "independent pole/clip rate={rate} cutoff={frequency}"
                    );
                }
                filter.clear();
                let (mut silent_l, mut silent_r) = ([0.0; 128], [0.0; 128]);
                filter.process(&mut silent_l, &mut silent_r);
                assert_eq!(silent_l, [0.0; 128]);
                assert_eq!(silent_r, [0.0; 128]);

                // Constant input checks the physical DC response separately
                // from the signed impulse and direct-form transient above.
                let mut last = 0.0;
                for _ in 0..(rate as usize).div_ceil(128) {
                    let (mut l, mut r) = ([0.0001; 128], [-0.0001; 128]);
                    filter.process(&mut l, &mut r);
                    last = l[127];
                }
                assert!((last - 1.02 * 0.0001 * (1.0 - 0.0001 / 48.0)).abs() < 0.00000002);
                for resonance in [0.0, 0.25, 0.6, 0.7] {
                    let knobs = [frequency, resonance, 0.0];
                    filter.clear();
                    filter.tune(knobs, rate);
                    let hz = cutoff(frequency).min(rate * 0.4);
                    let (mut ip, mut op) = (0_f64, 0_f64);
                    for block in 0..2048 {
                        let mut l = std::array::from_fn::<_, 128, _>(|i| {
                            (((block * 128 + i) as f64 * std::f64::consts::TAU * f64::from(hz)
                                / f64::from(rate))
                            .sin()
                                * 1e-5) as f32
                        });
                        let mut r = l.map(|x| -x);
                        if block >= 1024 {
                            ip += l.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>();
                        }
                        filter.process(&mut l, &mut r);
                        if block >= 1024 {
                            op += l.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>();
                        }
                    }
                    let measured = (op / ip).sqrt();
                    let expected = f64::from(Ladder::magnitude(knobs, hz, rate));
                    assert!(
                        (measured / expected - 1.0).abs() < 0.002,
                        "small-signal rate={rate} cutoff={frequency} res={resonance}"
                    );
                }
                // Full resonance can self-oscillate. Its nonlinear bounded
                // output is checked instead of a stable linear sine gain.
                filter.clear();
                filter.tune([frequency, 1.0, 1.0], rate);
                for block in 0..512 {
                    let mut l = [0.0; 128];
                    if block == 0 {
                        l[0] = 0.1;
                    }
                    let mut r = l.map(|x| -x);
                    filter.process(&mut l, &mut r);
                    assert!(l.iter().chain(&r).all(|x| x.is_finite() && x.abs() < 48.0));
                }
            }
        }
    }
}
