//! Native Ladder LP4's single-rate kernel and ordinary 32/4-frame control clock.
//! HQ, modulation source sampling, and nonzero secondary cutoff clocks remain gaps.

use std::sync::OnceLock;

static EXPONENTIAL: OnceLock<[f32; 2401]> = OnceLock::new();

/// Warm in the loader, including when a voice will later change subtype.
pub(crate) fn prepare() {
    EXPONENTIAL.get_or_init(|| {
        std::array::from_fn(|i| 2.0_f64.powf(i as f64 * (1.0 / 60.0) - 20.0) as f32)
    });
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Ladder {
    state: [[f32; 5]; 2],
    controls: Controls,
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

/// Ordinary native control interpolation, at 32 host / four internal frames.
/// Gain is linear here; cutoff is normalized and resonance is already shaped.
#[derive(Clone, Copy, Debug, Default)]
struct Controls {
    current: [f32; 3],
    target: [f32; 3],
    increment: [f32; 3],
    rate: f32,
    remaining: u32,
    phase: u8,
    pending: bool,
    started: bool,
    modes: u8, // bit0: version-derived instant cutoff; bit1: enabled target route
}

impl Controls {
    fn clock(&mut self) {
        if self.pending {
            self.pending = false;
            let ticks = (self.rate * 0.002 / 32.0 + 0.5).floor().max(1.0) as u32;
            let inverse = 1.0 / (ticks as f32 * 32.0);
            for lane in 0..3 {
                let increment = (self.target[lane] - self.current[lane]) * inverse;
                if (lane == 1 && self.modes & 1 != 0) || f64::from(increment * increment) < 1e-15 {
                    self.current[lane] = self.target[lane];
                    self.increment[lane] = 0.0;
                } else {
                    self.increment[lane] = increment;
                }
            }
            self.remaining = if self.increment.iter().any(|&v| v != 0.0) {
                ticks
            } else {
                0
            };
        } else if self.remaining != 0 {
            self.remaining -= 1;
            if self.remaining == 0 {
                self.current = self.target;
                self.increment = [0.0; 3];
            }
        }
    }

    fn advance(&mut self) {
        for lane in 0..3 {
            self.current[lane] += 4.0 * self.increment[lane];
        }
    }
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

/// These readers enable the separate cutoff path whose native constructor
/// and rate preparation leave its clock inverse zero. Other versions remain
/// unknown; their ordinary cutoff clock is an explicitly diagnosed fallback.
pub(super) fn instant_cutoff(version: u16) -> bool {
    matches!(version, 0x90..=0x92)
}

impl Ladder {
    pub(super) fn record_version(&mut self, version: u16) {
        self.controls.modes = (self.controls.modes & !1) | u8::from(instant_cutoff(version));
    }

    /// Enabled native target routes write at every control tick, including
    /// unchanged or zero-depth targets. Route switches preserve audio history
    /// and phase; their next write occurs on the existing 32-frame clock.
    pub(super) fn enabled_modulation(&mut self, enabled: bool) {
        self.controls.modes = (self.controls.modes & !2) | (u8::from(enabled) << 1);
    }

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
        let target = [
            exponential(1200.0 + 119.589_41 * gain),
            frequency,
            1.0 - (1.0 - resonance).powi(2),
        ];
        if !self.controls.started {
            // Native activation snapshots targets before the first audio frame.
            self.controls = Controls {
                current: target,
                target,
                rate,
                modes: self.controls.modes,
                ..Controls::default()
            };
            self.coefficients(target, rate);
        } else {
            self.controls.target = target;
            self.controls.rate = rate;
            self.controls.pending = true;
        }
        // R is a dimensionless fixed feedback gain; 80/R is in Hz.
        let r = exponential(1196.0 + 0.681_396_5);
        let w = (80.0 / r) / rate;
        let w2 = w * w;
        self.correction = std::f32::consts::PI * w * (1.0 + w2 * (3.289_868_4 + 12.987_881 * w2));
        self.correction_reciprocal = 1.0 / (1.0 + self.correction);
        self.correction_previous = self.correction - 1.0;
        self.correction_gain = r * r - 1.0;
    }

    fn coefficients(&mut self, [gain, frequency, q]: [f32; 3], rate: f32) {
        self.g = pole(cutoff(frequency) / rate).min(8.0);
        self.reciprocal = 1.0 / (1.0 + self.g);
        self.feedback = 4.16 * q;
        self.compensation = compensation(q);
        self.gain = gain;
    }

    pub(super) fn clear(&mut self) {
        self.state = [[0.0; 5]; 2];
        self.controls.started = false;
    }

    pub(super) fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len();
        if n == 0 {
            return;
        }
        if !self.controls.started {
            // Local reset policy snapshots even when a cached unchanged key
            // suppresses tune(). Pending clocks cannot survive that reset.
            let target = self.controls.target;
            let rate = self.controls.rate;
            self.controls = Controls {
                current: target,
                target,
                rate,
                started: true,
                modes: self.controls.modes,
                ..Controls::default()
            };
            self.coefficients(target, rate);
        }
        if !self.controls.pending && self.controls.remaining == 0 && self.controls.modes & 2 == 0 {
            self.process_held(left, right);
            self.controls.phase = ((usize::from(self.controls.phase) + n) % 32) as u8;
            return;
        }
        let mut start = 0;
        while start < n {
            if self.controls.phase == 0 {
                self.controls.pending |= self.controls.modes & 2 != 0;
                self.controls.clock();
                if self.controls.remaining == 0 {
                    self.coefficients(self.controls.current, self.controls.rate);
                }
            }
            let end = if self.controls.remaining == 0 && !self.controls.pending {
                (start + 32 - usize::from(self.controls.phase)).min(n)
            } else {
                if self.controls.phase % 4 == 0 {
                    self.controls.advance();
                    self.coefficients(self.controls.current, self.controls.rate);
                }
                (start + 4 - usize::from(self.controls.phase % 4)).min(n)
            };
            self.process_held(&mut left[start..end], &mut right[start..end]);
            self.controls.phase = ((usize::from(self.controls.phase) + end - start) % 32) as u8;
            start = end;
        }
    }

    fn process_held(&mut self, left: &mut [f32], right: &mut [f32]) {
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
    fn native_ladder_enabled_routes_rearm_on_persistent_control_ticks() {
        prepare();
        for rate in [32_000.0_f32, 44_100.0, 48_000.0, 96_000.0] {
            let ticks = (rate * 0.002 / 32.0 + 0.5).floor().max(1.0) as usize;
            let target = 10_f64.powf(6.0 / 20.0);
            for disable in [false, true] {
                let mut filter = Ladder::default();
                filter.record_version(0x92);
                filter.tune([1.0, 0.0, 0.0], rate);
                filter.process(&mut [0.0; 17], &mut [0.0; 17]);
                filter.tune([1.0, 0.0, 0.5], rate);
                filter.enabled_modulation(true);
                let mut split = filter;
                let (mut l, mut r) = ([0.0001; 512], [-0.0001; 512]);
                let (mut sl, mut sr) = (l, r);
                // Route enable/disable is mid-tick; neither switch changes phase.
                let cut = if disable { 128 } else { 512 }; // global frame145
                filter.process(&mut l[..cut], &mut r[..cut]);
                if disable {
                    filter.enabled_modulation(false);
                }
                filter.process(&mut l[cut..], &mut r[cut..]);
                for range in [
                    0..1,
                    1..15,
                    15..47,
                    47..83,
                    83..128,
                    128..129,
                    129..303,
                    303..512,
                ] {
                    if disable && range.start == 128 {
                        split.enabled_modulation(false);
                    }
                    split.process(&mut sl[range.clone()], &mut sr[range]);
                }
                assert_eq!(l, sl);
                assert_eq!(r, sr);
                // Independent geometric approach for repeated setter writes,
                // followed by a finite linear remainder when the route disconnects.
                // This differs from a single finite 2ms ramp even at zero depth.
                let hz = 2_f64.powf((1481.881591796875 + 575.0) / 60.0 - 20.0);
                let w = hz / f64::from(rate);
                let g = (w
                    * (std::f64::consts::PI
                        + w * w * (10.335426330566406 + 40.80263137817383 * w * w)))
                    .min(8.0);
                let mut voltage = [0_f64; 4];
                for (i, &actual) in l.iter().enumerate() {
                    let frame = i + 17;
                    let gain = if frame < 32 {
                        1.0
                    } else {
                        let block = ((frame - 32) / 32).min(if disable { 3 } else { usize::MAX });
                        let begin =
                            target - (target - 1.0) * (1.0 - 1.0 / ticks as f64).powi(block as i32);
                        let elapsed = ((frame - (32 + block * 32)) / 4 + 1) * 4;
                        begin + (target - begin) * (elapsed as f64 / (32 * ticks) as f64).min(1.0)
                    };
                    let input = 0.0001 * gain;
                    let mut value = input * (1.0 - input / 48.0);
                    for memory in &mut voltage {
                        let out = (*memory + g * value) / (1.0 + g);
                        *memory = 2.0 * out - *memory;
                        value = out;
                    }
                    assert!(
                        (f64::from(actual) - 1.02 * value).abs() < 2e-8,
                        "repeated physical reference rate={rate} frame={frame} disable={disable}"
                    );
                    assert_eq!(actual, -r[i]);
                }
                if disable {
                    assert_eq!(filter.controls.current, filter.controls.target);
                }
                filter.clear();
                filter.process(&mut [0.0; 1], &mut [0.0; 1]);
                assert_eq!(filter.controls.current, filter.controls.target);
                assert_eq!(filter.controls.modes & 2 != 0, !disable);
            }
        }
    }

    #[test]
    fn native_ladder_legacy_cutoff_snap_retains_gain_resonance_clock() {
        prepare();
        for version in [0x90, 0x91, 0x92, 0, 0x95] {
            for rate in [32_000.0_f32, 44_100.0, 48_000.0, 96_000.0] {
                let duration = ((rate * 0.002 / 32.0 + 0.5).floor() as usize).max(1) * 32;
                let mut filter = Ladder::default();
                filter.record_version(version);
                filter.tune([0.25, 0.0, 0.0], rate);
                let (mut l, mut r) = ([0.0; 9], [0.0; 9]);
                filter.process(&mut l, &mut r);
                filter.tune([0.75, 0.5, 0.5], rate);
                let mut split = filter;
                let (mut l, mut r) = ([0.00001; 512], [-0.00001; 512]);
                let (mut sl, mut sr) = (l, r);
                filter.process(&mut l, &mut r);
                for range in [0..1, 1..23, 23..71, 71..117, 117..512] {
                    split.process(&mut sl[range.clone()], &mut sr[range]);
                }
                assert_eq!(l, sl);
                assert_eq!(r, sr);
                let mut clock = Ladder::default();
                clock.record_version(version);
                clock.tune([0.25, 0.0, 0.0], rate);
                clock.process(&mut [0.0; 9], &mut [0.0; 9]);
                clock.tune([0.75, 0.5, 0.5], rate);
                for frame in 9..32 + duration {
                    clock.process(&mut [0.0; 1], &mut [0.0; 1]);
                    let elapsed = if frame < 32 {
                        0
                    } else {
                        (((frame - 32) / 4 + 1) * 4).min(duration)
                    };
                    let fraction = elapsed as f64 / duration as f64;
                    let cutoff_fraction = if matches!(version, 0x90..=0x92) && frame >= 32 {
                        1.0
                    } else {
                        fraction
                    };
                    assert!(
                        (f64::from(clock.controls.current[1]) - (0.25 + 0.5 * cutoff_fraction))
                            .abs()
                            < 0.000002
                    );
                    assert!(
                        (f64::from(clock.gain)
                            - (1.0 + (10_f64.powf(6.0 / 20.0) - 1.0) * fraction))
                            .abs()
                            < 0.00003
                    );
                    assert!((f64::from(clock.feedback) - 4.16 * 0.75 * fraction).abs() < 0.000004);
                }
                // Retarget downward: the zero-inverse secondary clock snaps;
                // no positive-increment cap artifact is introduced or claimed.
                clock.tune([0.1, 0.5, 0.5], rate);
                clock.process(&mut [0.0; 1], &mut [0.0; 1]);
                if matches!(version, 0x90..=0x92) {
                    assert_eq!(clock.controls.current[1], 0.1);
                }
                clock.clear();
                clock.process(&mut [0.0; 1], &mut [0.0; 1]);
                assert_eq!(clock.controls.current, clock.controls.target);
                assert_eq!(clock.controls.remaining, 0);
                assert_eq!(
                    clock.controls.modes & 1 != 0,
                    matches!(version, 0x90..=0x92)
                );
            }
        }
    }

    #[test]
    fn native_ladder_lp4_control_steps_follow_physical_clock_and_partition() {
        prepare();
        for rate in [32_000.0_f32, 44_100.0, 48_000.0, 96_000.0] {
            let duration = ((rate * 0.002 / 32.0 + 0.5).floor() as usize).max(1) * 32;
            let mut filter = Ladder::default();
            filter.tune([0.2, 0.0, -0.25], rate);
            // All initial writes are snapshotted before activation, without a ramp.
            filter.tune([0.3, 0.0, 0.0], rate);
            let (mut warm_l, mut warm_r) = ([0.0; 9], [0.0; 9]);
            filter.process(&mut warm_l, &mut warm_r);
            assert_eq!(filter.controls.current, filter.controls.target);
            filter.tune([0.75, 0.0, 0.5], rate);
            let mut split = filter;
            let source: [f32; 512] = std::array::from_fn(|i| {
                if i == 91 {
                    -0.012
                } else {
                    (i as f32 * 0.17).sin() * 0.007
                }
            });
            let (mut left, mut right) = (source, source.map(|v| -v));
            filter.process(&mut left, &mut right);
            let (mut split_l, mut split_r) = (source, source.map(|v| -v));
            for range in [0..1, 1..22, 22..23, 23..28, 28..68, 68..511, 511..512] {
                split.process(&mut split_l[range.clone()], &mut split_r[range]);
            }
            assert_eq!(left, split_l);
            assert_eq!(right, split_r);
            assert_eq!(filter.controls.current, filter.controls.target);
            assert_eq!(filter.controls.remaining, 0);

            // Independent trapezoidal RC voltages, in double precision. The
            // step starts at host frame32, and the first quartet uses 4/duration.
            // Gain interpolates in amplitude; cutoff interpolates before Hz mapping.
            let gain_end = 10_f64.powf(6.0 / 20.0);
            let mut voltage = [0.0_f64; 4];
            for (i, (&input, &actual)) in source.iter().zip(&left).enumerate() {
                let frame = i + 9;
                let elapsed = if frame < 32 {
                    0
                } else {
                    (((frame - 32) / 4 + 1) * 4).min(duration)
                };
                let fraction = elapsed as f64 / duration as f64;
                let gain = 1.0 + (gain_end - 1.0) * fraction;
                let raw_cutoff = 0.3 + (0.75 - 0.3) * fraction;
                let hz = 2_f64.powf((1481.881591796875 + 575.0 * raw_cutoff) / 60.0 - 20.0);
                let w = hz / f64::from(rate);
                let g = (w
                    * (std::f64::consts::PI
                        + w * w * (10.335426330566406 + 40.80263137817383 * w * w)))
                    .min(8.0);
                let input = f64::from(input) * gain;
                let mut value = input * (1.0 - input.abs() / 48.0);
                for memory in &mut voltage {
                    let output = (*memory + g * value) / (1.0 + g);
                    *memory = 2.0 * output - *memory;
                    value = output;
                }
                assert!(
                    (f64::from(actual) - value * 1.02).abs() < 0.000001,
                    "physical moving-pole reference: rate={rate} frame={frame}"
                );
                assert_eq!(actual, -right[i]);
            }

            // Resonance is smoothed after its nonlinear shaping, rather than
            // treating its normalized control or resulting Hz as a linear ramp.
            let mut resonance = Ladder::default();
            resonance.tune([0.5, 0.0, 0.0], rate);
            let (mut l, mut r) = ([0.0; 32], [0.0; 32]);
            resonance.process(&mut l, &mut r);
            resonance.tune([0.5, 0.5, 0.0], rate);
            for frame in 0..duration {
                let (mut l, mut r) = ([0.0; 1], [0.0; 1]);
                resonance.process(&mut l, &mut r);
                let fraction = ((frame / 4 + 1) * 4) as f32 / duration as f32;
                assert!((resonance.feedback - 4.16 * 0.75 * fraction).abs() < 0.000003);
            }
            // A target change partway through a ramp waits for the next
            // 32-frame boundary and starts from the value reached there.
            let mut retarget = Ladder::default();
            retarget.tune([0.5, 0.0, 0.0], rate);
            retarget.process(&mut l, &mut r);
            retarget.tune([0.5, 0.5, 0.0], rate);
            let (mut l17, mut r17) = ([0.0; 17], [0.0; 17]);
            retarget.process(&mut l17, &mut r17);
            retarget.tune([0.5, 1.0, 0.0], rate);
            let (mut l15, mut r15) = ([0.0; 15], [0.0; 15]);
            retarget.process(&mut l15, &mut r15);
            let reached = 0.75 * 32.0 / duration as f32;
            assert!((retarget.controls.current[2] - reached).abs() < 0.000001);
            let (mut one_l, mut one_r) = ([0.0; 1], [0.0; 1]);
            retarget.process(&mut one_l, &mut one_r);
            let expected = reached + (1.0 - reached) * 4.0 / duration as f32;
            assert!((retarget.controls.current[2] - expected).abs() < 0.000001);

            let mut tiny = Ladder::default();
            tiny.tune([0.5, 0.0, 0.0], rate);
            tiny.process(&mut l, &mut r);
            tiny.tune([0.500001, 0.0000001, 0.000001], rate);
            tiny.process(&mut one_l, &mut one_r);
            assert_eq!(tiny.controls.current, tiny.controls.target);
            assert_eq!(tiny.controls.remaining, 0, "sub-threshold deltas snap");

            for retuned in [false, true] {
                let mut reset = Ladder::default();
                reset.tune([0.2, 0.0, 0.0], rate);
                reset.process(&mut l, &mut r);
                reset.tune([0.75, 0.5, 0.5], rate);
                reset.process(&mut l17, &mut r17);
                assert_ne!(reset.controls.remaining, 0);
                reset.clear();
                let target = if retuned {
                    [0.3, 0.25, -0.5]
                } else {
                    [0.75, 0.5, 0.5]
                };
                if retuned {
                    reset.tune(target, rate);
                }
                let mut fresh = Ladder::default();
                fresh.tune(target, rate);
                let mut reset_l = [0.0001; 71];
                let mut reset_r = [-0.0001; 71];
                let (mut fresh_l, mut fresh_r) = (reset_l, reset_r);
                reset.process(&mut reset_l, &mut reset_r);
                fresh.process(&mut fresh_l, &mut fresh_r);
                assert_eq!(reset_l, fresh_l, "reset snapshots with retune={retuned}");
                assert_eq!(reset_r, fresh_r);
                assert_eq!(reset.controls.current, reset.controls.target);
                assert_eq!(reset.controls.remaining, 0);
                assert!(!reset.controls.pending);
            }

            resonance.process(&mut l, &mut r);
            assert_eq!(resonance.controls.current, resonance.controls.target);
            assert_eq!(resonance.controls.remaining, 0);
        }
    }

    #[test]
    fn native_ladder_lp4_scalar_laws_partition_reset_and_resonance() {
        prepare();
        assert_eq!(std::mem::size_of::<Ladder>(), 124);
        assert!((cutoff(0.0) - 25.956726).abs() < 0.0001);
        assert!((cutoff(1.0) - 19912.266).abs() < 0.01);
        assert_eq!(exponential(1200.0), 1.0, "native Gain0 is unity");
        assert!((exponential(1200.0 + 119.58941) - 10_f32.powf(12.0 / 20.0)).abs() < 0.0001);
        // Independent dB gain and DC/soft-clip law, including signed direct Gain.
        for rate in [32_000.0, 44_100.0, 48_000.0, 96_000.0] {
            for raw in [-1.0_f32, -0.25, 0.0, 0.5, 1.0] {
                let mut filter = Ladder::default();
                filter.tune([0.5, 0.0, raw], rate);
                let amplitude = 0.0001_f64 * 10_f64.powf(12.0 * f64::from(raw) / 20.0);
                let expected = 1.02 * amplitude * (1.0 - amplitude / 48.0);
                let mut last = 0.0;
                for _ in 0..128 {
                    let (mut l, mut r) = ([0.0001; 128], [-0.0001; 128]);
                    filter.process(&mut l, &mut r);
                    last = l[127];
                    assert!((l[127] + r[127]).abs() < 1e-8);
                }
                assert!(
                    (f64::from(last) - expected).abs() < 2e-8,
                    "signed Gain{raw} rate{rate}"
                );
            }
        }
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
