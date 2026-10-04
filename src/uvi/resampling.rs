//! Offline IR reconstruction measured against official Workstation 4.0.9 with
//! original PCM16 impulse, DC, sine and endpoint fixtures. No native code was
//! copied. The rational FIR has 1600 taps: sinc centre 799, Kaiser beta 12,
//! window centre/radius 799.5, and DC gain equal to the interpolation factor.
//! 44.1/88.2 -> 48 kHz complete curves agree within 2.5e-8 output amplitude.
//! Other integer ratios establish the kernel, but not every output-end rounding.

use super::sample::Sample;
use anyhow::{Context, Result, ensure};

const TAPS: usize = 1600;
const MAX_FRAMES: usize = (128 << 20) / size_of::<f32>();

/// Complete output curves and endpoint lengths have a native oracle for these
/// rates. Other integer ratios share the FIR but still need endpoint probes.
pub(super) fn verified_rate(source: u32, target: f64) -> bool {
    f64::from(source) == target || (target == 48000. && matches!(source, 44100 | 88200))
}

fn target_rate(rate: f64) -> Result<u32> {
    ensure!(
        rate.is_finite() && (8000. ..=192000.).contains(&rate) && rate.fract() == 0.,
        "UVI impulse reconstruction requires an integer sample rate in 8..192 kHz"
    );
    Ok(rate as u32)
}

/// Allocation size before reading PCM. Native equal-rate preparation converts
/// duration to float32 seconds; this can drop one final source frame.
pub(super) fn reconstructed_frames(sample: &Sample, rate: f64) -> Result<usize> {
    let rate = target_rate(rate)?;
    ensure!(
        sample.rate > 0 && sample.frames > 0,
        "Invalid UVI impulse timing"
    );
    let frames = if sample.rate == rate {
        f64::from((sample.frames as f64 / f64::from(sample.rate)) as f32) * f64::from(rate)
    } else {
        let frames = sample.frames as f64 * f64::from(rate) / f64::from(sample.rate);
        if verified_rate(sample.rate, f64::from(rate)) {
            let (_, down) = factors(sample.rate, rate);
            frames + 800. / down as f64 - (800. / down as f64).round()
        } else {
            frames
        }
    };
    ensure!(
        frames >= 1. && frames < (MAX_FRAMES + 1) as f64,
        "UVI reconstructed impulse exceeds memory bound"
    );
    Ok(frames as usize)
}

/// Reconstruct one ordered source channel without copying packed source PCM.
/// The caller also bounds the total allocation across all prepared channels.
pub(super) fn reconstruct_channel(sample: &Sample, channel: usize, rate: f64) -> Result<Vec<f32>> {
    let frames = reconstructed_frames(sample, rate)?;
    let rate = target_rate(rate)?;
    ensure!(
        channel < sample.channels
            && sample.frames.checked_mul(sample.channels) == Some(sample.interleaved.len()),
        "Invalid UVI impulse channel dimensions"
    );
    let source_frames = if sample.rate != rate && verified_rate(sample.rate, f64::from(rate)) {
        sample.frames - 1
    } else {
        (f64::from((sample.frames as f64 / f64::from(sample.rate)) as f32) * f64::from(sample.rate))
            as usize
    };
    let source_frames = source_frames.min(sample.frames);
    let at = |frame: usize| -> Result<f64> {
        let value = sample
            .interleaved
            .value(frame * sample.channels + channel)
            .context("Invalid UVI impulse PCM bounds")?;
        ensure!(value.is_finite(), "Nonfinite UVI impulse PCM");
        Ok(f64::from(value))
    };
    let mut output = Vec::new();
    output.try_reserve_exact(frames)?;
    if sample.rate == rate {
        for frame in 0..frames {
            output.push(at(frame)? as f32);
        }
        return Ok(output);
    }
    let (up, down) = factors(sample.rate, rate);
    let cutoff = up.max(down) as f64;
    let mut coefficients = [0.; TAPS];
    let denominator = bessel_i0(12.);
    for (index, coefficient) in coefficients.iter_mut().enumerate() {
        let x = (index as f64 - 799.) / cutoff;
        let sinc = if x == 0. {
            1.
        } else {
            (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x)
        };
        let window = (index as f64 - 799.5) / 799.5;
        *coefficient = sinc * bessel_i0(12. * (1. - window * window).sqrt()) / denominator;
    }
    let normalization = up as f64 / coefficients.iter().sum::<f64>();
    for coefficient in &mut coefficients {
        *coefficient *= normalization;
    }
    let delay = (800. / down as f64).round() as u64;
    // Adjacent FIR windows reread immutable source PCM. Cache only decoded
    // values within this call; every first read retains its original checks.
    let mut cached_sources = [usize::MAX; 2048];
    let mut cached_values = [0.; 2048];
    let mut cached_at = |source: usize| -> Result<f64> {
        let slot = source & 2047;
        if cached_sources[slot] != source {
            cached_values[slot] = at(source)?;
            cached_sources[slot] = source;
        }
        Ok(cached_values[slot])
    };
    for frame in 0..frames {
        let position = (frame as u64 + delay)
            .checked_mul(down)
            .context("UVI impulse reconstruction position overflow")?;
        let first = position.saturating_sub((TAPS - 1) as u64).div_ceil(up);
        let end = (position / up).min(source_frames.saturating_sub(1) as u64);
        let mut value = 0.;
        if source_frames > 0 && first <= end {
            for source in first..=end {
                let index = (position - source * up) as usize;
                value += cached_at(source as usize)? * coefficients[index];
            }
        }
        let value = value as f32;
        ensure!(value.is_finite(), "Nonfinite reconstructed UVI impulse");
        output.push(value);
    }
    Ok(output)
}

fn factors(source: u32, target: u32) -> (u64, u64) {
    let (mut divisor, mut remainder) = (source, target);
    while remainder != 0 {
        (divisor, remainder) = (remainder, divisor % remainder);
    }
    (u64::from(target / divisor), u64::from(source / divisor))
}

// The convergent I0 power series at beta <= 12 needs no numerical dependency.
fn bessel_i0(x: f64) -> f64 {
    let x = x * x * 0.25;
    let (mut term, mut sum) = (1., 1.);
    for order in 1..=64 {
        term *= x / f64::from(order * order);
        sum += term;
        if term <= sum * f64::EPSILON {
            break;
        }
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uvi::storage::Storage;

    fn sample(rate: u32, channels: usize, frames: usize) -> Sample {
        let mut values = vec![0.; frames * channels];
        for channel in 0..channels {
            values[10000 * channels + channel] = 0.5 / (channel + 1) as f32;
        }
        Sample {
            rate,
            channels,
            frames,
            interleaved: Storage::from_f32(values).unwrap(),
            loops: Vec::new(),
            unity_note: None,
            wavetable_cycle_frames: None,
            wavetable_image: false,
            riff_metadata: Vec::new(),
        }
    }

    #[test]
    fn source_read_cache_wrap_keeps_shifted_impulse_amplitude() {
        let mut source = sample(96000, 1, 16001);
        let mut values = vec![0.; source.frames];
        // These positions share a cache slot but carry different values.
        values[1024] = 0.5;
        values[3072] = -0.25;
        source.interleaved = Storage::from_f32(values).unwrap();
        let output = reconstruct_channel(&source, 0, 48000.).unwrap();
        let mut observed = 0;
        for frame in 112..913 {
            if output[frame] != 0. {
                observed += 1;
                assert_eq!(output[frame + 1024].to_bits(), (-0.5 * output[frame]).to_bits());
            }
        }
        assert!(observed > 700);
    }
    #[test]
    fn authored_native_ir_reconstruction() {
        // Native mono capture divided by the independently measured input .125.
        // These numeric observations derive from authored mathematical impulses.
        let cases: &[(u32, usize, &[f32])] = &[
            (
                44100,
                10880,
                &[
                    0.0000906926,
                    -0.0018095697,
                    0.012005527,
                    -0.04588835,
                    0.14766788,
                    0.46578097,
                    -0.03717895,
                    0.0028628618,
                    0.0009358048,
                    -0.00025702536,
                    0.0000077641025,
                ],
            ),
            (
                88200,
                5438,
                &[
                    0.0000859268,
                    -0.0012557873,
                    0.0075857746,
                    -0.030032594,
                    0.123317644,
                    0.20369616,
                    -0.04013239,
                    0.0106721185,
                    -0.0020080102,
                    0.00018096791,
                    -0.0000014780404,
                ],
            ),
            (
                32000,
                14993,
                &[
                    0.031755455,
                    -0.037546024,
                    0.,
                    0.05904124,
                    -0.08267702,
                    0.,
                    0.41348922,
                    0.41349667,
                    0.,
                    -0.08268444,
                    0.059048668,
                ],
            ),
            (
                96000,
                4995,
                &[
                    0.017669588,
                    -0.022724982,
                    0.031822413,
                    -0.053045932,
                    0.15915209,
                    0.15915495,
                    -0.05304879,
                    0.031825274,
                    -0.022727843,
                    0.017672446,
                    -0.014454336,
                ],
            ),
        ];
        for &(rate, start, expected) in cases {
            let source = sample(rate, 4, 24000);
            let output = reconstruct_channel(&source, 0, 48000.).unwrap();
            assert_eq!(output.len(), 24000 * 48000 / rate as usize);
            for (index, &want) in expected.iter().enumerate() {
                assert!(
                    (output[start + index] - want).abs() < 1e-7,
                    "rate {rate}, tap {}: {} != {want}",
                    start + index,
                    output[start + index]
                );
            }
            let other = reconstruct_channel(&source, 3, 48000.).unwrap();
            assert!(
                output
                    .iter()
                    .zip(other)
                    .all(|(&x, y)| (x * 0.25 - y).abs() < 1e-8)
            );
        }
        // Complete DC endpoints establish the final omitted input sample and
        // the retained phase-dependent length, including an extra odd 88.2k tap.
        let endpoints: &[(u32, usize, usize, &[f32])] = &[
            (
                44100,
                4111,
                4475,
                &[
                    0.12532352,
                    0.12305126,
                    0.13136768,
                    0.108476266,
                    -0.006018651,
                ],
            ),
            (
                88200,
                4106,
                2235,
                &[0.12496122, 0.12543696, 0.122768946, 0.13305987, 0.08523843],
            ),
            (
                44100,
                4095,
                4457,
                &[0.12497352, 0.12548973, 0.12171686, 0.13875471, 0.05500547],
            ),
            (
                44100,
                4097,
                4459,
                &[0.12497036, 0.12552054, 0.12179501, 0.13702261, 0.079328224],
            ),
            (
                88200,
                4095,
                2229,
                &[0.12496278, 0.12542287, 0.12283536, 0.1328195, 0.08690753],
            ),
            (
                88200,
                4097,
                2230,
                &[0.12497298, 0.12532595, 0.12330798, 0.1310958, 0.0972936],
            ),
            (
                44100,
                24000,
                26122,
                &[0.124978006, 0.12542996, 0.1223878, 0.13418272, 0.09600051],
            ),
            (
                88200,
                24000,
                13061,
                &[0.1250008, 0.12494765, 0.12551993, 0.12245529, 0.13420023],
            ),
        ];
        for &(rate, frames, length, expected) in endpoints {
            let mut source = sample(rate, 1, 24000);
            source.frames = frames;
            source.interleaved = Storage::from_f32(vec![0.125; frames]).unwrap();
            let output = reconstruct_channel(&source, 0, 48000.).unwrap();
            assert_eq!(output.len(), length);
            assert!(
                output[length - expected.len()..]
                    .iter()
                    .zip(expected)
                    .all(|(&got, &want)| (got - want).abs() < 3e-7),
                "DC endpoint {rate}/{frames}"
            );
        }
        let mut source = sample(48000, 1, 24000);
        assert!(reconstruct_channel(&source, 1, 48000.).is_err());
        assert!(reconstruct_channel(&source, 0, 48000.5).is_err());
        assert!(reconstruct_channel(&source, 0, f64::NAN).is_err());
        source.frames = usize::MAX;
        assert!(reconstructed_frames(&source, 48000.).is_err());
        source.frames = 4096;
        assert_eq!(reconstructed_frames(&source, 48000.).unwrap(), 4095);
        assert_eq!(reconstructed_frames(&source, 44100.).unwrap(), 3763);
    }
}
