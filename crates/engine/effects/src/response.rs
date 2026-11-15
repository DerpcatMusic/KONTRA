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
    // Where the early part ends, as a fraction, once stretched.
    let mut boundary = None;
    if impulse.size != [1.0, 1.0] {
        let at = impulse
            .cuts
            .and_then(|c| c.crossover)
            .unwrap_or_else(|| kontra_dsp::reverb::shape::split([&left, &right], from));
        let at = ((left.len() as f32 * at.clamp(0.0, 1.0)) as usize).min(left.len());
        let [early, late] = impulse.size;
        for c in [&mut left, &mut right] {
            let (a, b) = c.split_at(at.min(c.len()));
            let mut out = kontra_dsp::reverb::shape::stretch(a, early);
            out.extend(kontra_dsp::reverb::shape::stretch(b, late));
            *c = out;
        }
        boundary = Some(stretched_at(at, left.len(), early));
    }
    if let Some(c) = impulse.cuts {
        let c = kontra_dsp::reverb::shape::Cuts {
            early: c.early,
            late: c.late,
            // None: where the response's decay turns diffuse.
            crossover: boundary.or(c.crossover).or_else(|| {
                (c.early != c.late).then(|| kontra_dsp::reverb::shape::split([&left, &right], from))
            }),
        };
        left = kontra_dsp::reverb::shape::cut(&left, from, &c);
        right = kontra_dsp::reverb::shape::cut(&right, from, &c);
    }
    if impulse.length < 1.0 || impulse.window_whole {
        window([&mut left, &mut right], impulse.length);
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

/// The first `max(2, length·frames)` frames of each channel, faded to
/// silence by `(1 + cos(π·i/(n + 1)))/2`; gains under 1e-10 are silence.
fn window(channels: [&mut Vec<f32>; 2], length: f32) {
    for c in channels {
        let n = ((c.len() as f32 * length.max(0.0)).round() as usize).max(2).min(c.len());
        c.truncate(n);
        let step = std::f32::consts::PI / (n + 1) as f32;
        for (i, x) in c.iter_mut().enumerate() {
            let w = ((i as f32 * step).cos() + 1.0) * 0.5;
            *x *= if w >= 1e-10 { w } else { 0.0 };
        }
    }
}

/// The early part's share of the stretched response: `at` frames of the
/// original, stretched by `early`, over the new length `len`.
fn stretched_at(at: usize, len: usize, early: f32) -> f32 {
    (at as f32 * early / len.max(1) as f32).clamp(0.0, 1.0)
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
    fn a_shorter_length_keeps_its_share_faded_by_a_raised_cosine() {
        let (mut l, mut r) = (vec![1.0f32; 1000], vec![1.0f32; 1000]);
        window([&mut l, &mut r], 0.25);
        assert_eq!(l.len(), 250);
        // (1 + cos(π·i/251))/2: whole at the start, half at the middle, near silence at the end.
        assert_eq!(l[0], 1.0);
        assert!((l[125] - 0.5).abs() < 0.01, "{}", l[125]);
        assert!(l[249] < 1e-3 && l[249] > 0.0);
        assert_eq!(l, r);
    }

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
