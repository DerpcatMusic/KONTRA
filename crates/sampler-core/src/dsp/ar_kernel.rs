//! FilterDJ equations derived from the approved native AR trace.
//! Admission and entry points: docs/audit-2026-10-10/w15-ar-native.md.
//! This module stays disconnected until original-byte checks pass.
use std::sync::OnceLock;

const TABLE_LEN: usize = 2401;

pub(super) fn prepare() {
    table();
}

fn table() -> &'static [f32; TABLE_LEN] {
    static TABLE: OnceLock<Box<[f32; TABLE_LEN]>> = OnceLock::new();
    TABLE.get_or_init(|| {
        Box::new(std::array::from_fn(|i| {
            (i as f64 / 60. - 20.).exp2() as f32
        }))
    })
}

fn exponential(position: f32) -> f32 {
    // Native table guards use signed AX, including the second Hz-lane lookup.
    let index = position as i32 as i16;
    if index < 0 {
        return 0.;
    }
    let t = table();
    if index >= 2400 {
        return t[2400];
    }
    let i = index as usize;
    t[i] + (position - f32::from(index)) * (t[i + 1] - t[i])
}

pub(super) fn targets(cutoff: f32, resonance: f32, rate: f32) -> [f32; 3] {
    let hz = exponential((cutoff * 145.).min(140.) * 5. + 1381.8816);
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

fn saturate(x: f32) -> f32 {
    let x = x.clamp(-1000., 1000.);
    x - x.abs() * x * 0.0005
}

#[derive(Clone, Copy, Default)]
pub(super) struct ArKernel {
    pub channels: [[f32; 9]; 2],
    pub detector: f32,
    pub adaptation: f32,
    pub feedback: f32,
    pub cap: f32,
}

struct Coefficients {
    g: f32,
    product: f32,
    damping: f32,
    inverse: f32,
}

fn section(s: &mut [f32; 9], index: usize, x: f32, c: &Coefficients) -> [f32; 3] {
    let h0 = s[2 * index];
    let b0 = s[2 * index + 1];
    let l0 = s[6 + index];
    let high = (x - ((b0 * c.damping + l0) + h0 * c.product)) * c.inverse;
    let band = (h0 + high) * c.g + b0;
    let low = (band + b0) * c.g + l0;
    s[2 * index] = high;
    s[2 * index + 1] = saturate(band);
    s[6 + index] = saturate(low);
    [high, band, low]
}

fn mix([high, band, low]: [f32; 3], weights: [f32; 3]) -> f32 {
    (low * weights[0] + (band + band) * weights[1]) + high * weights[2]
}

impl ArKernel {
    /// Stereo frame; the shared detector follows the signed stereo band sum.
    pub(super) fn tick(
        &mut self,
        input: [f32; 2],
        mode: u8,
        controls: [f32; 3],
        rate_inverse: f32,
        ramping: bool,
    ) -> [f32; 2] {
        debug_assert!(mode < 9);
        let family = mode / 3;
        let weights = std::array::from_fn(|i| if i == usize::from(mode % 3) { 1. } else { 0. });
        let [g, hz_lane, resonance] = controls;
        let (floor_db, resonance_offset, resonance_scale, gain_a, gain_b) = match family {
            0 => (f32::from_bits(0x40d491d1), 0., 24., 0.9965784, 0.14948677),
            1 => (f32::from_bits(0xbf1a5e35), 6., 24., 1.195894, 0.24914463),
            _ => (-0.6, 6., 20., 1.195894, 0.29897354),
        };
        let floor = exponential(floor_db * 9.965784 + 1200.);
        let r = resonance * resonance_scale;
        let damping = 1. - self.feedback;
        let damping = (damping + damping) + g;
        let product = g * damping;
        let c = Coefficients {
            g,
            product,
            damping: g + damping,
            inverse: 1. / (product + 1.),
        };
        let gain = exponential(((gain_a - r * gain_b) * weights[1] - gain_a) * r + 1200.);
        let mut detected = 0.;
        let output = std::array::from_fn(|ch| {
            let s = &mut self.channels[ch];
            let first = section(s, 0, input[ch], &c);
            let (value, band) = match family {
                0 => (mix(first, weights), first[1]),
                1 => {
                    let second = section(s, 1, mix(first, weights), &c);
                    if mode == 4 {
                        (mix(second, weights), second[1])
                    } else {
                        let third = section(s, 2, first[1] + first[1], &c);
                        (mix([second[0], third[1], second[2]], weights), third[1])
                    }
                }
                _ => {
                    let second = section(s, 1, first[1] + first[1], &c);
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
            let frequency = exponential(hz_lane * 5. + 1381.8816);
            let a = frequency.clamp(24., 70.);
            let b = frequency.clamp(85., 130.);
            self.cap = (0.86 - (b - 130.) * 0.0031111108)
                - ((a - 70.) * -0.005982609 - (b - 130.) * (a - 70.) * -0.00002164251);
        }
        let amount = exponential((r - resonance_offset) * 9.965784 + 1200.);
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
    }
}
