//! A convolution response decoded and shaped off the audio thread.

use kontra_ir as ir;

/// Longest response: 20 s at 192 kHz.
const MAX_RESPONSE: u64 = 20 * 192_000;

/// `impulse`'s sample decoded, prepared and at `rate`.
/// `sample` prepared as `impulse` says, at `rate`. Not real-time.
pub fn response(sample: &ir::Sample, impulse: &ir::Impulse, rate: u32) -> Result<Response, String> {
    let (from, frames) =
        kontra_stream::decode(sample, MAX_RESPONSE).map_err(|e| format!("{}: {e}", sample.name))?;
    let (mut left, mut right): (Vec<f32>, Vec<f32>) = frames.iter().map(|f| (f[0], f[1])).unzip();
    if impulse.reverse {
        left.reverse();
        right.reverse();
    }
    if let Some(c) = impulse.cuts {
        let c = kontra_dsp::reverb::shape::Cuts {
            early: c.early,
            late: c.late,
            // None: where the response's decay turns diffuse.
            crossover: c.crossover.or_else(|| {
                (c.early != c.late).then(|| kontra_dsp::reverb::shape::split([&left, &right], from))
            }),
        };
        left = kontra_dsp::reverb::shape::cut(&left, from, &c);
        right = kontra_dsp::reverb::shape::cut(&right, from, &c);
    }
    if let Some(knots) = impulse.envelope {
        kontra_dsp::reverb::shape::envelope([&mut left, &mut right], knots);
    }
    if let Some([l, r]) = impulse.mix {
        for (a, b) in left.iter_mut().zip(&mut right) {
            (*a, *b) = (l[0] * *a + l[1] * *b, r[0] * *a + r[1] * *b);
        }
    }
    // At most a second of predelay, and never more than a capped response holds.
    let most = (MAX_RESPONSE * u64::from(from) / u64::from(rate.max(1))) as usize;
    let delay = ((impulse.predelay_ms.clamp(0.0, 1000.0) * from as f32 * 0.001) as usize).min(most);
    for c in [&mut left, &mut right] {
        c.splice(0..0, std::iter::repeat_n(0.0, delay));
    }
    Ok(resample(left, right, from, rate, impulse.normalize))
}

/// A prepared impulse response.
#[derive(Clone, Debug, PartialEq)]
pub struct Response {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
}

fn resample(
    mut left: Vec<f32>,
    mut right: Vec<f32>,
    from: u32,
    to: u32,
    normal: Option<ir::Normalize>,
) -> Response {
    if let Some(n) = normal.filter(|n| !n.after_resample) {
        normalize(&mut left, &mut right, n);
    }
    let most = MAX_RESPONSE as usize;
    let mut left = kontra_dsp::reverb::shape::resample(&left, from, to, most);
    let mut right = kontra_dsp::reverb::shape::resample(&right, from, to, most);
    if let Some(n) = normal.filter(|n| n.after_resample) {
        normalize(&mut left, &mut right, n);
    }
    Response { left, right }
}

fn normalize(left: &mut [f32], right: &mut [f32], n: ir::Normalize) {
    let energy = |c: &[f32]| c.iter().map(|&v| f64::from(v * v)).sum::<f64>();
    let (l, r) = (energy(left), energy(right));
    let e = match n.channels {
        ir::EnergyChannels::Loudest => l.max(r),
        ir::EnergyChannels::Mean => (l + r) * 0.5,
    };
    if e > f64::from(n.floor) {
        let gain = (n.energy / e as f32).sqrt().min(n.max_gain);
        left.iter_mut()
            .chain(right.iter_mut())
            .for_each(|v| *v *= gain);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_can_follow_rate_conversion() {
        let wave: Vec<f32> = (0..8192)
            .map(|i| (std::f32::consts::TAU * 1000.0 * i as f32 / 96_000.0).sin())
            .collect();
        let n = ir::Normalize {
            energy: 1.0,
            max_gain: 1e5,
            floor: 0.0,
            channels: ir::EnergyChannels::Mean,
            after_resample: true,
        };
        let after = resample(wave.clone(), wave.clone(), 96_000, 48_000, Some(n));
        let before = resample(
            wave.clone(),
            wave,
            96_000,
            48_000,
            Some(ir::Normalize {
                after_resample: false,
                ..n
            }),
        );
        let energy = |x: &[f32]| x.iter().map(|v| v * v).sum::<f32>();
        assert!((energy(&after.left) - 1.0).abs() < 1e-5);
        assert!((energy(&before.left) - 0.5).abs() < 0.005);
    }

    #[test]
    fn stereo_energy_can_use_the_mean_or_loudest_channel() {
        let n = ir::Normalize {
            energy: 1.0,
            max_gain: 1e5,
            floor: 0.0,
            channels: ir::EnergyChannels::Mean,
            after_resample: true,
        };
        let (mut left, mut right) = ([1.0, 0.0], [0.0, 0.0]);
        normalize(&mut left, &mut right, n);
        assert!((left[0] - std::f32::consts::SQRT_2).abs() < 1e-6);
        assert_eq!(right, [0.0, 0.0]);
        let (mut left, mut right) = ([1.0, 0.0], [0.0, 0.0]);
        normalize(
            &mut left,
            &mut right,
            ir::Normalize {
                channels: ir::EnergyChannels::Loudest,
                ..n
            },
        );
        assert_eq!(left, [1.0, 0.0]);
    }
}
