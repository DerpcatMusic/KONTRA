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
            crossover: c.crossover,
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
    if let Some(n) = impulse.normalize {
        let energy = |c: &[f32]| c.iter().map(|&v| f64::from(v * v)).sum::<f64>();
        let e = energy(&left).max(energy(&right));
        if e > f64::from(n.floor) {
            let gain = (n.energy / e as f32).sqrt().min(n.max_gain);
            left.iter_mut()
                .chain(right.iter_mut())
                .for_each(|v| *v *= gain);
        }
    }
    // At most a second of predelay, and never more than a capped response holds.
    let most = (MAX_RESPONSE * u64::from(from) / u64::from(rate.max(1))) as usize;
    let delay = ((impulse.predelay_ms.clamp(0.0, 1000.0) * from as f32 * 0.001) as usize).min(most);
    for c in [&mut left, &mut right] {
        c.splice(0..0, std::iter::repeat_n(0.0, delay));
    }
    let most = MAX_RESPONSE as usize;
    Ok(Response {
        left: kontra_dsp::reverb::shape::resample(&left, from, rate, most),
        right: kontra_dsp::reverb::shape::resample(&right, from, rate, most),
    })
}

/// A prepared impulse response.
#[derive(Clone, Debug, PartialEq)]
pub struct Response {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
}
