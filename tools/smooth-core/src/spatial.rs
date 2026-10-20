//! Clean mathematical profiles and the admitted mode-4 block spatial operator.
//! Per-channel E, intermediate bank curves and the mode-1 consumer remain separate inputs.
use crate::dsp::shelf_tilt32;
use crate::numerics::{div32, exp32, fractional_domain, ln32, pow32, recip32};

const EPSILON: f32 = 1.1920928955078125e-7;

pub fn common_timing_profile(frequency: f32, hard: bool) -> Result<f32, &'static str> {
    if !frequency.is_finite() || frequency < 0.0 {
        return Err("common timing profile requires a finite nonnegative frequency");
    }
    let center: f32 = if hard { 250.0 } else { 1000.0 };
    let scale = 1.0_f32 / center.max(EPSILON);
    let shifted = frequency + EPSILON;
    let coordinate = (shifted * scale).min(2.0 * center); // Exponent-one cap is identity.
    let denominator = coordinate + 1.0;
    let inverse = recip32(denominator).max(0.0);
    if !inverse.is_normal() || inverse <= 0.0 || inverse > 1.0 {
        return Err("unsupported common timing fractional input");
    }
    let exponent = if hard { 1.0 } else { 2.0 } * f32::from_bits(0x3e2aaaab);
    let profile = pow32(inverse, exponent).min(1.0);
    Ok(if hard { profile * 2.0 } else { profile })
}

pub fn first_weight(frequency: f32, adaptive: bool) -> f32 {
    let (center, amount): (f32, f32) = if adaptive {
        (2000.0, 3.0)
    } else {
        (3000.0, 2.0)
    };
    let width = 2.5_f64 * 0.601591280067049;
    let sigma = width * 0.7071067811865475;
    let kappa = (-0.5_f64 / (sigma * sigma)) as f32;
    let inverse = (1.0_f64 / (center as f64).max(EPSILON as f64)) as f32;
    let power = 10.0_f32.powf(amount * 0.05000000074505806_f32);
    let gain = power.ln();
    let coordinate = (frequency + EPSILON) * inverse;
    let log2 = ln32(coordinate) * 1.4426950216293335_f32;
    let square = log2 * log2;
    let exponent = square * kappa;
    let nested = exp32(exponent);
    exp32(nested * gain)
}

pub fn final_weight(frequency: f32, shape: f32, hard: bool, adaptive: bool) -> f32 {
    let square = shape.powf(2.0).max(0.0).min(1.0);
    let high: f32 = if hard { 16000.0 } else { 6000.0 };
    let low_log = 1000.0_f32.ln();
    let span = high.ln() - low_log;
    let scaled = square * span;
    let center = (scaled + low_log).exp();
    let inverse = (1.0_f64 / (center as f64).max(EPSILON as f64)) as f32;
    let coordinate = (frequency + EPSILON) * inverse;
    let cap = (2.0_f64.powf(0.3333333333333333) * center as f64) as f32;
    let coordinate = coordinate.min(cap);
    let squared = coordinate * coordinate;
    let cube = squared * coordinate;
    let profile = recip32(cube + 1.0).max(0.0).min(1.0);
    if adaptive {
        profile
    } else {
        -1.0 * profile + 1.0
    }
}

pub fn bank_terminal_clamp(weight: f32) -> f32 {
    let upper = 10.0_f32.powf(2.4000000953674316_f32);
    let lower = 10.0_f32.powf(-4.800000190734863_f32);
    weight.min(upper).max(lower)
}

pub fn common_bandwidth(shape: f32) -> f32 {
    let normalized = shape.clamp(0.0, 1.0);
    let scaled = normalized * 1.0986123085021973_f32;
    let shifted = scaled - 0.6931471824645996_f32;
    let position = (shifted.exp() - 0.5).clamp(0.0, 1.0);
    let span = 300.0_f32.ln() - 2.079441547393799_f32;
    let scaled = position * span;
    (scaled + 2.079441547393799_f32).exp()
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SpatialBlock {
    previous: [f32; 4],
    input: [[f32; 4]; 4],
}
impl SpatialBlock {
    pub fn from_alpha(alpha: [f64; 4]) -> Self {
        let mut block = Self::default();
        for lane in 0..4 {
            let mut product = alpha[0];
            for value in &alpha[1..=lane] {
                product *= value;
            }
            block.previous[lane] = product as f32;
            for input in 0..=lane {
                let complement = 1.0_f64 - alpha[input];
                let mut product = 1.0_f64;
                for value in &alpha[input + 1..=lane] {
                    product *= value;
                }
                block.input[input][lane] = (product * complement) as f32;
            }
        }
        block
    }
    pub fn evaluate(&self, previous: f32, input: [f32; 4]) -> [f32; 4] {
        std::array::from_fn(|lane| {
            let p0 = previous * self.previous[lane];
            let p1 = input[0] * self.input[0][lane];
            let p2 = input[1] * self.input[1][lane];
            let p3 = input[2] * self.input[2][lane];
            let p4 = input[3] * self.input[3][lane];
            let first = p1 + p0;
            let left = first + p2;
            let right = p4 + p3;
            left + right
        })
    }
}

pub struct SpatialMode4 {
    alpha: Vec<f64>,
    forward: Vec<SpatialBlock>,
    reverse: Vec<SpatialBlock>,
    final_scalar: [f32; 2],
}
impl SpatialMode4 {
    /// All allocation belongs to plan preparation. `update` reuses this storage.
    pub fn new(bins: usize) -> Result<Self, &'static str> {
        if bins < 5 || (bins - 1) % 4 != 0 {
            return Err("mode-4 spatial operator requires N=4K+1 with K positive");
        }
        let blocks = (bins - 1) / 4;
        Ok(Self {
            alpha: vec![0.0; bins],
            forward: vec![SpatialBlock::default(); blocks],
            reverse: vec![SpatialBlock::default(); blocks],
            final_scalar: [0.0; 2],
        })
    }
    pub fn alpha(&self) -> &[f64] {
        &self.alpha
    }

    pub fn update(
        &mut self,
        owner_rate: f32,
        frequencies: &[f32],
        shape: f32,
        tilt_low: f32,
        tilt_high: f32,
        channel_curve: &[f32],
    ) -> Result<(), &'static str> {
        if channel_curve.len() != frequencies.len()
            || !tilt_low.is_finite()
            || tilt_low <= 0.0
            || !tilt_high.is_finite()
            || tilt_high <= 0.0
            || channel_curve.iter().any(|v| !v.is_finite() || *v < 0.0)
        {
            return Err("unsupported spatial bandwidth inputs");
        }
        let bandwidth = common_bandwidth(shape);
        self.update_program(owner_rate, frequencies, shape, |k, frequency| {
            let tilted = bandwidth * shelf_tilt32(frequency, tilt_low, tilt_high);
            tilted * channel_curve[k]
        })
    }

    /// Direct V input for distinct source first/second tables; no division to simulate E.
    pub fn update_with_bandwidth(
        &mut self,
        owner_rate: f32,
        frequencies: &[f32],
        shape: f32,
        bandwidth_input: &[f32],
    ) -> Result<(), &'static str> {
        if bandwidth_input.len() != frequencies.len()
            || bandwidth_input.iter().any(|v| !v.is_finite() || *v < 0.0)
        {
            return Err("unsupported direct spatial bandwidth input");
        }
        self.update_program(owner_rate, frequencies, shape, |k, _| bandwidth_input[k])
    }

    fn update_program(
        &mut self,
        owner_rate: f32,
        frequencies: &[f32],
        shape: f32,
        bandwidth: impl Fn(usize, f32) -> f32,
    ) -> Result<(), &'static str> {
        let bins = self.alpha.len();
        if frequencies.len() != bins
            || !owner_rate.is_finite()
            || owner_rate <= 0.0
            || !shape.is_finite()
            || !(0.0..=1.0).contains(&shape)
        {
            return Err("unsupported spatial parameter geometry");
        }
        let inverse_bins = 1.0_f64 / bins as f64;
        let half_rate = owner_rate as f64 * 0.5;
        let scale = 1000.0_f64 / half_rate;
        let divisor = (scale * bins as f64) as f32;
        let delta_rate = 48000.0_f32 / owner_rate;
        let delta = delta_rate * bins as u64 as f32;
        if !divisor.is_normal() || divisor <= 0.0 || !delta.is_normal() || delta <= 0.0 {
            return Err("unsupported spatial divisor or delta");
        }
        let inverse_delta = (1.0_f64 / delta as f64) as f32;
        let above = (shape - 0.5).max(0.0);
        let position = (above * 2.0).max(0.0).min(1.0);
        let exponent = 0.6000000238418579_f32 - position * 0.5;
        for k in 0..bins {
            let frequency = frequencies[k];
            if !frequency.is_finite() || frequency < 0.0 {
                return Err("unsupported spatial frequency or bandwidth curve");
            }
            let unit = (k as f32) as f64 * inverse_bins;
            let coordinate = (unit * (bins as f32) as f64) as f32;
            let coordinate = if k == 0 { 1.0 } else { coordinate };
            let normalized = div32(coordinate, divisor);
            let low = normalized.min(1.0);
            let high = normalized.max(1.0);
            let input = low * high;
            if !fractional_domain(input, exponent) {
                return Err("unsupported spatial fractional input");
            }
            let shaped = pow32(input, exponent);
            let channel = bandwidth(k, frequency);
            let product = shaped * channel;
            let bounded = product.abs() + 600.0;
            let saturation = recip32(bounded) * 600.0;
            let saturated = product * saturation;
            let normalized = saturated * inverse_delta;
            let alpha = exp32(normalized * -6.2831854820251465_f32);
            if !alpha.is_finite() || alpha < 0.0 {
                return Err("nonfinite spatial coefficient");
            }
            let alpha = if k == 0 { 0.0 } else { alpha };
            self.alpha[k] = (alpha as f64).clamp(EPSILON as f64, (1.0_f32 - EPSILON) as f64);
        }
        for block in 0..self.forward.len() {
            let start = block * 4;
            self.forward[block] =
                SpatialBlock::from_alpha(std::array::from_fn(|i| self.alpha[start + i]));
            let start = bins - 2 - block * 4;
            self.reverse[block] =
                SpatialBlock::from_alpha(std::array::from_fn(|i| self.alpha[start - i]));
        }
        let final_alpha = self.alpha[bins - 1];
        self.final_scalar = [final_alpha as f32, (1.0_f64 - final_alpha) as f32];
        Ok(())
    }

    /// Exactly the block grouping and addition tree from the admitted mode-4 contract.
    pub fn apply(&self, values: &mut [f32]) {
        assert_eq!(values.len(), self.alpha.len());
        let mut previous = values[0];
        for (index, block) in self.forward.iter().enumerate() {
            let start = index * 4;
            let input = std::array::from_fn(|i| values[start + i]);
            let output = block.evaluate(previous, input);
            values[start..start + 4].copy_from_slice(&output);
            previous = output[3];
        }
        let last = values.len() - 1;
        let own = values[last] * self.final_scalar[1];
        let carried = previous * self.final_scalar[0];
        values[last] = own + carried;
        previous = values[last];
        for (index, block) in self.reverse.iter().enumerate() {
            let start = last - 1 - index * 4;
            let input = std::array::from_fn(|i| values[start - i]);
            let output = block.evaluate(previous, input);
            for i in 0..4 {
                values[start - i] = output[i];
            }
            previous = output[3];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode4_two_pass_block_recurrence_bits() {
        // Nine abstract response values, independently evaluated from the clean recurrence.
        // No FFT, streaming engine, audio samples or vendor runtime are involved.
        let alpha = [
            0x34000000, 0x3f19999a, 0x3f555555, 0x3dcccccd, 0x3f7ffffe, 0x3f000001, 0x3eaaaaab,
            0x3f7ffffc, 0x3f400001,
        ]
        .map(|bits| f32::from_bits(bits) as f64);
        let mut operator = SpatialMode4::new(9).unwrap();
        operator.alpha.copy_from_slice(&alpha);
        for block in 0..2 {
            operator.forward[block] =
                SpatialBlock::from_alpha(std::array::from_fn(|i| alpha[4 * block + i]));
            operator.reverse[block] =
                SpatialBlock::from_alpha(std::array::from_fn(|i| alpha[7 - 4 * block - i]));
        }
        operator.final_scalar = [alpha[8] as f32, (1.0 - alpha[8]) as f32];
        let mut values = [-0.012345, -3.7, -0.1, -1.2, -0.9, -5.3, -0.6, -1.7, -0.2];
        operator.apply(&mut values);
        assert_eq!(
            values.map(f32::to_bits),
            [
                0xbc4a435d, 0xbfb07e75, 0xbfa73b32, 0xbfa884fc, 0xc014296a, 0xc014296a, 0xbfb049a2,
                0xbf94e3d1, 0xbf94e3d0
            ]
        );
    }

    #[test]
    fn supplied_profiles_and_supported_coefficient_geometry() {
        assert!(SpatialMode4::new(8).is_err());
        assert!(common_timing_profile(-1.0, false).is_err());
        assert!((common_timing_profile(0.0, true).unwrap() - 2.0).abs() < 3.0e-7);
        assert!((common_timing_profile(0.0, false).unwrap() - 1.0).abs() < 3.0e-7);
        for hard in [false, true] {
            let low = common_timing_profile(100.0, hard).unwrap();
            let high = common_timing_profile(20000.0, hard).unwrap();
            assert!(low > high && high > 0.0);
        }
        for adaptive in [false, true] {
            let center = if adaptive { 2000.0 } else { 3000.0 };
            assert!(first_weight(center, adaptive) > first_weight(20.0, adaptive));
        }
        for hard in [false, true] {
            let low = final_weight(20.0, 0.5, hard, true);
            let high = final_weight(20000.0, 0.5, hard, true);
            assert!(low > high && (0.0..=1.0).contains(&high));
            assert!((high + final_weight(20000.0, 0.5, hard, false) - 1.0).abs() < 1.0e-7);
        }
        assert!((common_bandwidth(0.0) - 8.0).abs() < 2.0e-6);
        assert!((common_bandwidth(1.0) - 300.0).abs() < 0.0001);
        let bins = 257;
        let frequencies: Vec<_> = (0..bins).map(|k| k as f32 * (44100.0 / 512.0)).collect();
        let curve = vec![1.0; bins];
        let mut operator = SpatialMode4::new(bins).unwrap();
        for shape in [0.0, 0.6, 1.0] {
            // Includes the exact-half spatial branch.
            operator
                .update(44100.0, &frequencies, shape, 1.0, 1.0, &curve)
                .unwrap();
            assert_eq!(operator.alpha()[0], EPSILON as f64);
            assert!(operator.alpha().iter().all(|a| a.is_finite()
                && *a >= EPSILON as f64
                && *a <= (1.0_f32 - EPSILON) as f64));
        }
    }
}
