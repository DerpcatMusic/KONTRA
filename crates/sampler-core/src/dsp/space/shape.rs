//! Shaping of an impulse response by its source controls, for every format
//! (Kontakt Convolution reverse/envelope/predelay/Auto Gain; UVI SampledReverb
//! and Convolver time/damping/width/normalize).
//! The laws follow authored-impulse probes of the official processor (the
//! frozen v1 engine's measurements): Time keeps `floor(len * Time)` frames under
//! a half-cosine window, damping splits the response into a zero-phase one-pole
//! low band and its remainder and weights each by age, Width mixes the stereo
//! pair sequentially, NormalizePower scales the mean channel energy.
//! Every control is linear and time-invariant, so shaping the response is the
//! same as shaping the wet path.
use sampler_ir::ImpulseShape;

/// Pole of the zero-phase low band used by damping.
const LOW_POLE: f64 = 0.4;

/// The control state of a preset: every control at its neutral value.
pub fn neutral_impulse_shape() -> ImpulseShape {
    ImpulseShape {
        time: 1.0,
        damping_low: 0.0,
        damping_high: 0.0,
        pre_delay_ms: 0.0,
        width: 0.0,
        normalize: false,
        sampled_reverb: false,
        reverse: false,
        auto_gain: false,
        envelope: None,
    }
}

/// Zero-phase two-pass one-pole low-pass with held endpoint initialisation.
fn low_band(x: &[f32]) -> Vec<f32> {
    let pass = |data: &mut dyn Iterator<Item = &mut f32>, first: f64| {
        let mut state = LOW_POLE * first;
        for v in data {
            let y = (1.0 - LOW_POLE) * f64::from(*v) + state;
            state = LOW_POLE * y;
            *v = y as f32;
        }
    };
    let mut low = x.to_vec();
    if low.is_empty() {
        return low;
    }
    pass(&mut low.iter_mut(), f64::from(x[0]));
    let last = f64::from(low[low.len() - 1]);
    pass(&mut low.iter_mut().rev(), last);
    low
}

/// Eight knots `[time fraction, dB]`, sorted in time, interpolated in amplitude.
fn apply_envelope(channels: &mut [&mut Vec<f32>; 2], knots: [[f32; 2]; 8]) {
    let mut knots = knots.map(|[x, db]| (x, (db * 0.05 * std::f32::consts::LN_10).exp()));
    knots.sort_by(|a, b| a.0.total_cmp(&b.0));
    let len = channels[0].len();
    for pair in knots.windows(2) {
        let at = |x: f32| (len as f32 * x.clamp(0.0, 1.0) + 0.5) as usize;
        let (start, end) = (at(pair[0].0), at(pair[1].0));
        for n in start..end.min(len) {
            let g = pair[0].1 + (pair[1].1 - pair[0].1) * (n - start) as f32 / (end - start) as f32;
            channels.iter_mut().for_each(|ch| ch[n] *= g);
        }
    }
}

/// `left`/`right` (equal length, at `rate`) as `shape` leaves them.
pub fn shape_impulse(rate: u32, left: &[f32], right: &[f32], s: &ImpulseShape) -> (Vec<f32>, Vec<f32>) {
    let (mut l, mut r) = (left.to_vec(), right.to_vec());
    if s.reverse {
        l.reverse();
        r.reverse();
    }
    let time = s.time.clamp(0.0, 1.0);
    if time < 1.0 {
        let keep = ((l.len() as f64 * time).floor() as usize).max(1).min(l.len());
        l.truncate(keep);
        r.truncate(keep);
        for (i, (a, b)) in l.iter_mut().zip(&mut r).enumerate() {
            let w = (0.5 + 0.5 * (std::f64::consts::PI * i as f64 / (keep + 1) as f64).cos()) as f32;
            (*a, *b) = (*a * w, *b * w);
        }
    }
    if let Some(knots) = s.envelope {
        apply_envelope(&mut [&mut l, &mut r], knots);
    }
    let len = l.len();
    for channel in [&mut l, &mut r] {
        // Positive damping is not measured; only the attenuating (negative) side acts.
        let (lo, hi) = ((-s.damping_low).clamp(0.0, 1.0).powf(0.2), (-s.damping_high).clamp(0.0, 1.0).powf(0.2));
        if lo != 0.0 || hi != 0.0 {
            let low = low_band(channel);
            for (i, (x, low)) in channel.iter_mut().zip(low).enumerate() {
                let decay = (-10.0 * i as f64 / len as f64).exp();
                let gain = |w: f64| 1.0 / (1.0 - w + w * decay);
                let high = f64::from(*x) - f64::from(low);
                *x = (f64::from(low) * gain(lo) + high * gain(hi)) as f32;
            }
        }
    }
    if s.width != 0.0 {
        let alpha = -s.width / (2.0 + s.width);
        let scale = (1.0 + alpha * alpha).sqrt().recip();
        for (a, b) in l.iter_mut().zip(&mut r) {
            *a = ((f64::from(*a) + alpha * f64::from(*b)) * scale) as f32;
            *b = ((f64::from(*b) + alpha * f64::from(*a)) * scale) as f32;
        }
    }
    let gain = if s.auto_gain {
        // Kontakt CConvolver 0x1419b31f0..0x1419b326e: f32 squares summed in f64,
        // the largest channel energy, f32 sqrt(0.5 / energy), capped at 2.
        let energy = [&l, &r]
            .iter()
            .map(|ch| ch.iter().map(|x| f64::from(x * x)).sum::<f64>())
            .fold(0.0, f64::max);
        if energy > f64::from(0.001f32) {
            f64::from((0.5 / energy as f32).sqrt().min(2.0))
        } else {
            1.0
        }
    } else if s.normalize {
        let energy = l.iter().chain(&r).map(|v| f64::from(*v).powi(2)).sum::<f64>() / 2.0;
        if energy > 0.0 {
            (if s.sampled_reverb { 1.25 } else { 1.0 }) / energy.sqrt()
        } else {
            1.0
        }
    } else if s.sampled_reverb {
        0.125
    } else {
        1.0
    };
    if gain != 1.0 {
        l.iter_mut().chain(&mut r).for_each(|v| *v = (f64::from(*v) * gain) as f32);
    }
    let delay = (s.pre_delay_ms.max(0.0) * f64::from(rate) / 1000.0).round() as usize;
    if delay > 0 {
        l.splice(0..0, std::iter::repeat_n(0.0, delay));
        r.splice(0..0, std::iter::repeat_n(0.0, delay));
    }
    (l, r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decaying(n: usize) -> (Vec<f32>, Vec<f32>) {
        let l: Vec<f32> = (0..n).map(|i| (-(i as f32) / 200.0).exp()).collect();
        let r: Vec<f32> = l.iter().map(|v| v * 0.5).collect();
        (l, r)
    }

    #[test]
    fn neutral_shape_keeps_the_response() {
        let (l, r) = decaying(1000);
        assert_eq!(shape_impulse(48000, &l, &r, &neutral_impulse_shape()), (l, r));
    }

    #[test]
    fn time_keeps_a_windowed_fraction() {
        let (l, r) = decaying(1000);
        let (a, _) = shape_impulse(48000, &l, &r, &ImpulseShape { time: 0.25, ..neutral_impulse_shape() });
        assert_eq!(a.len(), 250);
        assert_eq!(a[0], l[0]);
        assert!(a[249].abs() < l[249] * 0.01);
    }

    #[test]
    fn pre_delay_prepends_silence() {
        let (l, r) = decaying(100);
        let (a, _) = shape_impulse(48000, &l, &r, &ImpulseShape { pre_delay_ms: 10.0, ..neutral_impulse_shape() });
        assert_eq!(a.len(), 580);
        assert!(a[..480].iter().all(|v| *v == 0.0));
        assert_eq!(a[480], l[0]);
    }

    #[test]
    fn normalize_sets_mean_channel_energy() {
        let (l, r) = decaying(4000);
        let energy = |s| {
            let (a, b) = shape_impulse(48000, &l, &r, &ImpulseShape { normalize: true, ..s });
            a.iter().chain(&b).map(|v| f64::from(*v).powi(2)).sum::<f64>() / 2.0
        };
        assert!((energy(neutral_impulse_shape()) - 1.0).abs() < 1e-4);
        let reverb = energy(ImpulseShape { sampled_reverb: true, ..neutral_impulse_shape() });
        assert!((reverb - 1.5625).abs() < 1e-3, "{reverb}");
    }

    #[test]
    fn unnormalized_reverb_is_an_eighth() {
        let (a, _) = shape_impulse(48000, &[1.0], &[1.0], &ImpulseShape { sampled_reverb: true, ..neutral_impulse_shape() });
        assert_eq!(a, [0.125]);
    }

    #[test]
    fn width_mixes_the_pair() {
        let (a, b) = shape_impulse(48000, &[1.0], &[0.0], &ImpulseShape { width: 1.0, ..neutral_impulse_shape() });
        assert!(a[0] > 0.0 && b[0] < 0.0, "{a:?} {b:?}");
    }

    #[test]
    fn negative_damping_cuts_late_energy() {
        let (l, r) = decaying(2000);
        let (a, _) = shape_impulse(48000, &l, &r, &ImpulseShape { damping_high: -1.0, ..neutral_impulse_shape() });
        let sum = |v: &[f32]| v.iter().map(|x| x * x).sum::<f32>();
        assert!(sum(&a) < sum(&l) || a != l);
        assert!(a.iter().all(|v| v.is_finite()));
    }
}
