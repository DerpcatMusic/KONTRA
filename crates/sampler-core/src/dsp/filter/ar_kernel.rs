//! FilterDJ equations derived from the approved native AR trace.
//! Admission notes: docs/audit-2026-10-10/w15-ar-native.md. Original-instruction
//! vectors cover all nine modes; calibrated host recording acceptance is pending.
use super::linear::Zdf;
use super::shaper::{Knee, Slope};

pub(crate) fn prepare() {
    super::table::prepare();
}

use super::table::{exponential as lookup, Past};

pub(crate) fn targets(cutoff: f32, resonance: f32, rate: f32) -> [f32; 3] {
    let hz = lookup((cutoff * 145.).min(140.) * 5. + 1381.8816, Past::LastEntry);
    let inverse = 1. / rate;
    let f = hz.max(0.);
    let g = ((f * f * f * f * inverse * inverse * inverse * 40.802628
        + f * f * inverse * 10.335426)
        * inverse
        + std::f32::consts::PI)
        * f
        * inverse;
    [g, hz, resonance]
}

/// The AR state knee: clamp to +-1000, then `x - |x| x 0.0005`.
const STATE_KNEE: Knee = Knee { limit: 1000., slope: Slope::Direct(0.0005) };

#[derive(Clone, Copy, Default)]
pub(crate) struct ArKernel {
    pub channels: [[f32; 9]; 2],
    pub detector: f32,
    pub adaptation: f32,
    pub feedback: f32,
    pub cap: f32,
}

/// One zero-delay section of the kernel's `[f32; 9]` state (`h` at 2i, `b` at 2i+1, `l` at 6+i).
fn section(s: &mut [f32; 9], index: usize, x: f32, g: f32, zdf: Zdf) -> [f32; 3] {
    let mut state = [s[2 * index], s[2 * index + 1], s[6 + index]];
    let raw = zdf.tick(g, &mut state, x, STATE_KNEE);
    s[2 * index] = state[0];
    s[2 * index + 1] = state[1];
    s[6 + index] = state[2];
    raw
}

fn mix([high, band, low]: [f32; 3], weights: [f32; 3]) -> f32 {
    (low * weights[0] + (band + band) * weights[1]) + high * weights[2]
}

/// The frame terms that depend only on the mode and the resonance control. The
/// frame loop reuses them until the resonance value changes, which is the same
/// arithmetic on the same inputs, so the output bits do not change.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Resonant {
    pub(crate) resonance: f32,
    floor: f32,
    gain: f32,
    amount: f32,
    r: f32,
}

pub(crate) fn resonant(mode: u8, resonance: f32) -> Resonant {
    debug_assert!(mode < 9);
    let family = mode / 3;
    let weights: [f32; 3] = std::array::from_fn(|i| if i == usize::from(mode % 3) { 1. } else { 0. });
    let (floor_db, resonance_offset, resonance_scale, gain_a, gain_b) = match family {
        0 => (f32::from_bits(0x40d491d1), 0., 24., 0.9965784, 0.14948677),
        1 => (f32::from_bits(0xbf1a5e35), 6., 24., 1.195894, 0.24914463),
        _ => (-0.6, 6., 20., 1.195894, 0.29897354),
    };
    let floor = lookup(floor_db * 9.965784 + 1200., Past::LastEntry);
    let r = resonance * resonance_scale;
    let gain = lookup(((gain_a - r * gain_b) * weights[1] - gain_a) * r + 1200., Past::LastEntry);
    let amount = lookup((r - resonance_offset) * 9.965784 + 1200., Past::LastEntry);
    Resonant { resonance, floor, gain, amount, r }
}

impl ArKernel {
    /// Stereo frame; the shared detector follows the signed stereo band sum.
    pub(crate) fn tick(
        &mut self,
        input: [f32; 2],
        mode: u8,
        controls: [f32; 3],
        rate_inverse: f32,
        ramping: bool,
    ) -> [f32; 2] {
        self.tick_resonant(input, mode, controls, rate_inverse, ramping, resonant(mode, controls[2]))
    }

    /// [`Self::tick`] with the resonance terms precomputed by [`resonant`] for
    /// `controls[2]`.
    pub(crate) fn tick_resonant(
        &mut self,
        input: [f32; 2],
        mode: u8,
        controls: [f32; 3],
        rate_inverse: f32,
        ramping: bool,
        terms: Resonant,
    ) -> [f32; 2] {
        debug_assert!(mode < 9 && terms.resonance.to_bits() == controls[2].to_bits());
        let family = mode / 3;
        let weights = std::array::from_fn(|i| if i == usize::from(mode % 3) { 1. } else { 0. });
        let [g, hz_lane, resonance] = controls;
        let Resonant { floor, gain, amount, .. } = terms;
        let zdf = Zdf::new(g, (1. - self.feedback) + (1. - self.feedback));
        let mut detected = 0.;
        let output = std::array::from_fn(|ch| {
            let s = &mut self.channels[ch];
            let first = section(s, 0, input[ch], g, zdf);
            let (value, band) = match family {
                0 => (mix(first, weights), first[1]),
                1 => {
                    let second = section(s, 1, mix(first, weights), g, zdf);
                    if mode == 4 {
                        (mix(second, weights), second[1])
                    } else {
                        let third = section(s, 2, first[1] + first[1], g, zdf);
                        (mix([second[0], third[1], second[2]], weights), third[1])
                    }
                }
                _ => {
                    let second = section(s, 1, first[1] + first[1], g, zdf);
                    let band = second[1] + second[1];
                    let cross = (resonance * 0.5) * band;
                    let value = ((first[2] + cross) * weights[0] + band * weights[1])
                        + (cross + first[0]) * weights[2];
                    (value, second[1])
                }
            };
            detected = if family == 0 || family == 2 {
                detected + (band + band)
            } else {
                (detected + band) + band
            };
            value * gain
        });
        let mut level = (detected * 0.5).abs();
        if level < self.detector {
            level = (level - self.detector) * rate_inverse * 21.903446 + self.detector;
        }
        self.detector = level;
        if ramping {
            let frequency = lookup(hz_lane * 5. + 1381.8816, Past::LastEntry);
            let a = frequency.clamp(24., 70.);
            let b = frequency.clamp(85., 130.);
            self.cap = (0.86 - (b - 130.) * 0.0031111108)
                - ((a - 70.) * -0.005982609 - (b - 130.) * (a - 70.) * -0.00002164251);
        }
        let target = (1. - 1. / ((amount - 1.) * (floor / level.max(floor).min(1e8)) + 1.)).max(0.);
        // The native ramp kernel rounds this coefficient before multiplying the delta.
        let adaptation_rate = rate_inverse * 2092.3;
        self.adaptation = (target - self.adaptation) * adaptation_rate + self.adaptation;
        self.feedback = self.adaptation * self.cap;
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_subtypes_match_original_instructions_at_native_checkpoints() {
        let native: serde_json::Value =
            serde_json::from_str(include_str!("ar_native_vectors.json")).unwrap();
        assert_eq!(
            native["binary_sha256"].as_str().unwrap(),
            "0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8"
        );
        let scenarios = native["scenarios"].as_array().unwrap();
        assert_eq!(scenarios.len(), 81);
        let (mut error, mut signal) = (0.0f64, 0.0f64);
        for case in scenarios {
            let mode = case["mode"].as_u64().unwrap() as u8;
            let rate = case["rate"].as_f64().unwrap() as f32;
            let amplitude = case["amplitude"].as_f64().unwrap();
            let mut now = targets(
                case["cutoff"].as_f64().unwrap() as f32,
                case["resonance"].as_f64().unwrap() as f32,
                rate,
            );
            let mut kernel = ArKernel::default();
            for phase in 0..3 {
                let frames = if phase == 2 { 32 } else { 64 };
                let points: &[usize] = if phase == 2 {
                    &[0, 1, 2, 3, 7, 15, 31]
                } else {
                    &[0, 1, 2, 3, 7, 15, 31, 63]
                };
                let target = targets(0.35, 0.3, rate);
                let quanta = ((rate * 0.001 / 32. + 0.5) as u32).max(1);
                let delta: [f32; 3] = std::array::from_fn(|i| {
                    let delta = (target[i] - now[i]) * (1. / (quanta * 32) as f32);
                    if delta * delta < 1e-15 { 0. } else { delta }
                });
                if phase == 2 {
                    for i in 0..3 {
                        if delta[i] == 0. {
                            now[i] = target[i];
                        }
                    }
                }
                for i in 0..frames {
                    let input = std::array::from_fn(|ch| {
                        let angle = if phase == 2 {
                            (i + 1) as f64 * 0.13 + ch as f64 * 0.7
                        } else {
                            (i + 1) as f64 * 0.17 + ch as f64 * 0.4
                        };
                        (amplitude * angle.sin()) as f32
                    });
                    if phase == 2 {
                        for lane in 0..3 {
                            now[lane] += delta[lane];
                        }
                    }
                    let output = kernel.tick(input, mode, now, 1. / rate, phase != 1);
                    if let Some(point) = points.iter().position(|n| *n == i) {
                        for ch in 0..2 {
                            let expected =
                                case["checkpoints"][phase][point][ch].as_f64().unwrap() as f32;
                            error += (f64::from(output[ch]) - f64::from(expected)).powi(2);
                            signal += f64::from(expected).powi(2);
                            assert!(
                                (output[ch] - expected).abs() <= 2e-6,
                                "mode={mode} rate={rate} phase={phase} frame={i} ch={ch} actual={} native={expected}",
                                output[ch]
                            );
                        }
                    }
                    assert!(output.iter().all(|v| v.is_finite()));
                }
            }
        }
        println!("FilterDJ original-instruction checkpoint null: {:.2} dB", 10. * (error / signal).log10());
    }

    /// FNV-1a over the output bits of one mode. Recorded before the AR section moved onto
    /// `Zdf` and the state knee moved onto `shaper::Knee`; both were bit-identical to the old
    /// arithmetic over 2e6 knee and 2e5 section probes.
    #[test]
    fn output_is_bit_stable() {
        let mut kernel = ArKernel::default();
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for n in 0..4096usize {
            let t = n as f32 / 48_000.;
            let x = 0.3 * (std::f32::consts::TAU * 220. * t).sin();
            let out = kernel.tick([x, x * 0.5], 1, [0.4, 0.6, 0.7], 1. / 48_000., n == 0);
            for v in out {
                hash = (hash ^ u64::from(v.to_bits())).wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        assert_eq!(hash, 0x9167_fd7b_74bd_03ae);
    }
}
