//! Clean working-spectrum fragments; global branch/transform aliases require the bridge.
use crate::numerics::scalar_power32;
use crate::reduction::dot32;
use crate::reduction::subtract32;
use crate::spatial::SpatialMode4;

/// The scale pass precedes this per-channel stage; the Whard pass follows all such stages.
pub fn hard_spatial_residual(
    working: &mut [f32],
    scratch: &mut [f32],
    first: &SpatialMode4,
    second: &SpatialMode4,
) {
    scratch.copy_from_slice(working);
    first.apply(scratch);
    subtract32(working, scratch);
    second.apply(working);
    for value in working {
        *value = (*value).max(0.0);
    }
}

#[repr(align(16))]
pub struct ThresholdWeights(pub [f32; 32]);
pub static THRESHOLD_WEIGHTS: ThresholdWeights = ThresholdWeights([
    0.04792085289955139,
    0.047863129526376724,
    0.047690100967884064,
    0.04740218073129654,
    0.04700006917119026,
    0.046484723687171936,
    0.045857395976781845,
    0.045119594782590866,
    0.044273097068071365,
    0.0433199368417263,
    0.04226242005825043,
    0.04110308736562729,
    0.039844732731580734,
    0.03849038854241371,
    0.03704332187771797,
    0.03550700843334198,
    0.033885158598423004,
    0.03218167647719383,
    0.03040066733956337,
    0.028546418994665146,
    0.026623399928212166,
    0.024636242538690567,
    0.022589733824133873,
    0.020488804206252098,
    0.018338516354560852,
    0.016144048422574997,
    0.013910689391195774,
    0.011643817648291588,
    0.009348894469439983,
    0.00703144958242774,
    0.004697064869105816,
    0.0023513648193329573,
]);

#[derive(Clone, Copy, Debug)]
pub enum ConditioningBranch {
    GlobalMaximumFloor,
    WeightedOrderedSubtraction,
}

pub fn roughness_scale(magnitude: &mut [f32]) -> Result<(), &'static str> {
    let count = magnitude.len() / 4;
    if count == 0 {
        return Err("roughness requires K>=1");
    }
    let mut previous = magnitude[0];
    let mut accumulator = 0.0_f32;
    for &value in &magnitude[1..count] {
        let difference = (value - previous).abs();
        let denominator = previous as f64 + 1.0e-7_f64;
        let ratio = difference as f64 / denominator;
        accumulator = (accumulator as f64 + ratio) as f32;
        previous = value;
    }
    let ratio = accumulator / count as f32;
    if (ratio as f64) < 0.001 {
        for value in magnitude {
            *value *= ratio;
        }
    }
    Ok(())
}

pub struct MPreparation {
    weight: Vec<f32>,
    hard_weight: Vec<f32>,
}
impl MPreparation {
    pub fn new(owner_rate: f32, length: usize) -> Result<Self, &'static str> {
        if !owner_rate.is_finite() || owner_rate < 44100.0 || length < 32 {
            return Err("unsupported M weighting geometry");
        }
        let inverse = 1.0_f64 / length as f64;
        let half_rate = owner_rate * 0.5;
        let hard_weight: Vec<f32> = (0..length)
            .map(|k| {
                let unit = (k as f32) as f64 * inverse;
                let frequency = (unit * half_rate as f64) as f32;
                frequency * 0.0010000000474974513_f32
            })
            .collect();
        let weight = hard_weight
            .iter()
            .map(|x| scalar_power32(*x, 0.75))
            .collect();
        Ok(Self {
            weight,
            hard_weight,
        })
    }
    pub fn weights(&self) -> &[f32] {
        &self.weight
    }
    pub fn hard_weights(&self) -> &[f32] {
        &self.hard_weight
    }
    pub fn condition(
        &self,
        rows: &mut [&mut [f32]],
        branch: ConditioningBranch,
        detail: f32,
    ) -> Result<(), &'static str> {
        if rows.is_empty()
            || !detail.is_finite()
            || rows.iter().any(|r| {
                r.len() != self.weight.len() || r.iter().any(|x| !x.is_finite() || *x < 0.0)
            })
        {
            return Err("unsupported working spectrum");
        }
        for row in rows.iter_mut() {
            roughness_scale(row)?;
        }
        match branch {
            ConditioningBranch::GlobalMaximumFloor => {
                let peak = rows
                    .iter()
                    .flat_map(|r| r.iter())
                    .copied()
                    .fold(0.0_f32, f32::max);
                let scaled = 20.0 * detail.clamp(0.0, 1.0);
                let shifted = scaled - 60.0;
                let gain = 10.0_f32.powf(shifted * 0.05000000074505806_f32);
                let floor = peak * gain;
                for row in rows.iter_mut() {
                    for value in row.iter_mut() {
                        *value = (*value).max(floor);
                    }
                }
            }
            ConditioningBranch::WeightedOrderedSubtraction => {
                for row in rows.iter_mut() {
                    for (value, &weight) in row.iter_mut().zip(&self.weight) {
                        *value *= weight;
                    }
                }
                let scale = 10.0_f32.powf(-0.45000001788139343_f32);
                for channel in 0..rows.len() {
                    let threshold = dot32(&rows[channel][..32], &THRESHOLD_WEIGHTS.0, 0)? * scale;
                    // Every later channel dot sees all earlier global subtractions.
                    for row in rows.iter_mut() {
                        for value in row.iter_mut() {
                            *value -= threshold;
                        }
                    }
                }
                for row in rows.iter_mut() {
                    for value in row.iter_mut() {
                        *value = (*value).max(0.0);
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reduction::phase32;
    #[test]
    fn hard_residual_uses_separate_bandwidths_and_preserves_precursor_history() {
        use crate::precursor::{MPrecursor, SourceMode};
        use crate::spatial::common_bandwidth;
        let n = 257;
        let frequencies: Vec<f32> = (0..n).map(|k| k as f32 * (44100.0 / 512.0)).collect();
        let b = common_bandwidth(0.5);
        let first_input = vec![b * 0.1599999964237213_f32 + 30.0; n];
        let second_input = vec![b; n];
        let mut first = SpatialMode4::new(n).unwrap();
        let mut second = SpatialMode4::new(n).unwrap();
        first
            .update_with_bandwidth(44100.0, &frequencies, 0.5, &first_input)
            .unwrap();
        second
            .update_with_bandwidth(44100.0, &frequencies, 0.5, &second_input)
            .unwrap();
        assert_ne!(first.alpha()[20], second.alpha()[20]);
        let mut precursor = MPrecursor::new(n, 1, 44100.0, 1024, SourceMode::Hard, 0.5).unwrap();
        let mut values: Vec<f32> = (0..n).map(|k| 1.0 + (k * 17 % 13) as f32 * 0.1).collect();
        precursor.update_channel(0, &mut values);
        let history = precursor.state(0).to_vec();
        let mut scratch = vec![0.0; n];
        hard_spatial_residual(&mut values, &mut scratch, &first, &second);
        assert!(values.iter().all(|v| v.is_finite() && *v >= 0.0));
        assert!(values.iter().any(|v| *v > 0.0));
        assert_eq!(precursor.state(0), history.as_slice());
        let prep = MPreparation::new(44100.0, n).unwrap();
        for (value, weight) in values.iter_mut().zip(prep.hard_weights()) {
            *value *= weight;
        }
        assert_eq!(values[0].to_bits(), 0); // Whard's exponent1 identity preserves DC zero.
    }
    #[test]
    fn fixed_weight_zero_roughness_and_ordered_threshold_contracts() {
        assert_eq!(phase32(&THRESHOLD_WEIGHTS.0), 0);
        let prep = MPreparation::new(44100.0, 32).unwrap();
        assert_eq!(prep.weights()[0].to_bits(), 0);
        let mut constant = [2.0_f32; 32];
        roughness_scale(&mut constant).unwrap();
        assert!(constant.iter().all(|v| v.to_bits() == 0));
        let a: Vec<f32> = (0..32)
            .map(|k| if k % 2 == 0 { 1.0 } else { 2.0 })
            .collect();
        let b: Vec<f32> = (0..32)
            .map(|k| if k % 2 == 0 { 1.0 } else { 3.0 })
            .collect();
        let mut left = a.clone();
        let mut right = b.clone();
        prep.condition(
            &mut [&mut left, &mut right],
            ConditioningBranch::WeightedOrderedSubtraction,
            0.5,
        )
        .unwrap();
        let mut reversed_left = a;
        let mut reversed_right = b;
        prep.condition(
            &mut [&mut reversed_right, &mut reversed_left],
            ConditioningBranch::WeightedOrderedSubtraction,
            0.5,
        )
        .unwrap();
        assert!(left.iter().all(|v| v.is_finite() && *v >= 0.0));
        assert_ne!(left[31].to_bits(), reversed_left[31].to_bits());
        let mut global: Vec<f32> = (0..32)
            .map(|k| if k % 2 == 0 { 1.0 } else { 3.0 })
            .collect();
        global[20] = 0.0;
        prep.condition(
            &mut [&mut global],
            ConditioningBranch::GlobalMaximumFloor,
            0.0,
        )
        .unwrap();
        assert_eq!(
            global[20].to_bits(),
            (3.0_f32 * 10.0_f32.powf(-3.0)).to_bits()
        );
    }
}
