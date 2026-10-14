//! Shaping of an impulse response by its source controls, for every format
//! (Kontakt Convolution reverse/envelope/predelay/Auto Gain; UVI SampledReverb
//! and Convolver time/damping/width/normalize).
//! The laws follow authored-impulse probes of the official processor (the
//! frozen v1 engine's measurements): Time keeps `floor(len * Time)` frames under
//! a half-cosine window, damping splits the response into a zero-phase one-pole
//! low band and its remainder and weights each by age (`0x141532fe0`), Width mixes the stereo
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
        filters: None,
        truncate_pre_delay: false,
    }
}

/// CConvolver's early/late filter (`0x1419b27a0`, coefficients `0x1419b7fb0`,
/// tick `0x1419b77f0`): a bilinear 2nd-order Butterworth low-pass (bypassed
/// above 0.45 of the rate) or high-pass (bypassed below 0.01), one causal pass.
fn butterworth(x: &mut [f32], rate: u32, hz: f32, high_pass: bool) {
    let w = hz / rate as f32;
    if (!high_pass && w > 0.45) || (high_pass && w < 0.01) || !w.is_finite() {
        return;
    }
    const SQRT2: f32 = 1.414_213_5;
    let k = (w * 3.141_592_7).tan();
    let k2 = k * k;
    let n = 1.0 / (SQRT2 * k + 1.0 + k2);
    let b = if high_pass { [n, -2.0 * n, n] } else { [k2 * n, 2.0 * k2 * n, k2 * n] };
    let a = [-(2.0 * (k2 - 1.0) * n), -((1.0 - SQRT2 * k + k2) * n)];
    let (mut x1, mut x2, mut y1, mut y2) = (0f32, 0f32, 0f32, 0f32);
    for v in x {
        let y = b[0] * *v + b[1] * x1 + b[2] * x2 + 1e-24 + a[0] * y1 + a[1] * y2;
        (x2, x1, y2, y1) = (x1, *v, y1, y);
        *v = y;
    }
}

/// The crossover sample (`0x1419b63d0`, then clamped by `0x1419b2870`), at
/// unit decimation.
fn crossover(len: usize, x: f32, rate: u32) -> usize {
    let r = rate as f32;
    ((((len as f32 * x) / r) * r + 0.5) as usize).min(len)
}

/// CConvolver's early/late composition (`0x1419b1e30`) at unit sizes: early
/// up to the crossover, late after it, a cos² crossfade 50 ms wide.
fn compose(early: &[f32], late: &[f32], x: usize, rate: u32) -> Vec<f32> {
    let len = early.len();
    let w = ((rate as f32 * 0.05 + 0.5) as i64) as f32;
    let h = w / 2.0;
    let start = (x as f32 - h + 0.5) as i64;
    let end = ((x as f32 + h + 0.5) as i64).min(len as i64).max(0) as usize;
    let first = start.max(0) as usize;
    let mut out: Vec<f32> = early[..end].iter().chain(&late[end..]).copied().collect();
    for (n, k) in (first..end).zip(first as i64 - start..) {
        let c = (k as f32 * (std::f32::consts::FRAC_PI_2 / w)).cos();
        let wgt = c * c;
        out[n] = (1.0 - wgt) * late[n] + wgt * out[n];
    }
    out
}

/// One channel through the early/late filters.
fn filter_channel(x: &[f32], rate: u32, f: &sampler_ir::ImpulseFilters) -> Vec<f32> {
    let pass = |[low, high]: [f32; 2]| {
        let mut y = x.to_vec();
        butterworth(&mut y, rate, high, false);
        butterworth(&mut y, rate, low, true);
        y
    };
    match f.crossover {
        Some(c) if f.early != f.late => compose(&pass(f.early), &pass(f.late), crossover(x.len(), c, rate), rate),
        _ => pass(f.late),
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

/// Modified Bessel function of the first kind, order 0 (power series).
fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term, q) = (1.0, 1.0, x * x / 4.0);
    for k in 1..200 {
        term *= q / f64::from(k * k);
        sum += term;
        if term < sum * 1e-17 {
            break;
        }
    }
    sum
}

/// UVI SampledReverb damping envelope over `len` frames (`0x141534d80`), or
/// `None` at 0. Positive `d` decays: the second half of a `2·len` Kaiser
/// window of `β = 6d`, raised to `1 + 90d²`; negative `d` lifts the tail:
/// `1 / (w·e^(-10i/len) + 1 - w)`, `w = (-d)^0.2`. Values below 1e-10 are 0.
fn damping_envelope(d: f64, len: usize) -> Option<Vec<f64>> {
    let d = d.clamp(-1.0, 1.0);
    if d == 0.0 || len == 0 {
        return None;
    }
    let env = (0..len).map(|i| {
        let g = if d > 0.0 {
            let beta = 6.0 * d;
            let x = (2 * i + 1) as f64 / (2 * len - 1).max(1) as f64;
            (bessel_i0(beta * (1.0 - x * x).max(0.0).sqrt()) / bessel_i0(beta)).powf(1.0 + 90.0 * d * d)
        } else {
            let w = (-d).powf(0.2);
            1.0 / (w * (-10.0 * i as f64 / len as f64).exp() + 1.0 - w)
        };
        if g < 1e-10 { 0.0 } else { g }
    });
    Some(env.collect())
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
    if let Some(f) = &s.filters {
        (l, r) = (filter_channel(&l, rate, f), filter_channel(&r, rate, f));
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
    let lo = damping_envelope(s.damping_low, len);
    let hi = damping_envelope(s.damping_high, len);
    if lo.is_some() || hi.is_some() {
        for channel in [&mut l, &mut r] {
            let low = low_band(channel);
            for (i, (x, low)) in channel.iter_mut().zip(low).enumerate() {
                let high = f64::from(*x) - f64::from(low);
                let (gl, gh) = (lo.as_ref().map_or(1.0, |e| e[i]), hi.as_ref().map_or(1.0, |e| e[i]));
                *x = (f64::from(low) * gl + high * gh) as f32;
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
        // Kontakt's convolution wrapper, 0x140ab6740..0x140ab67eb: f32 sum of
        // squares per channel, the largest, sqrt(0.5 / energy) capped at 2,
        // unity below 0.001.
        let energy = [&l, &r].iter().map(|ch| ch.iter().fold(0f32, |s, x| s + x * x)).fold(0f32, f32::max);
        if energy >= 0.001 {
            f64::from((0.5 / energy).sqrt().min(2.0))
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
    let delay = if s.truncate_pre_delay {
        ((rate as f32 * s.pre_delay_ms.max(0.0) as f32) * 0.001) as usize
    } else {
        (s.pre_delay_ms.max(0.0) * f64::from(rate) / 1000.0).round() as usize
    };
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

    /// UVI `0x141534d80` vectors, four frames.
    #[test]
    fn damping_envelopes_match_native_vectors() {
        let close = |d: f64, want: [f64; 4]| {
            let e = damping_envelope(d, 4).unwrap();
            assert!(e.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-5), "{d}: {e:?}");
        };
        close(0.5, [0.557_179, 0.004_375, 0.0, 0.0]);
        close(0.1, [0.996_663, 0.970_26, 0.919_018, 0.845_946]);
        close(-0.5, [1.0, 4.977_388, 7.390_155, 7.696_397]);
        assert_eq!(low_band(&[1.0, 0.0, 0.0, 0.0]), [0.715_456, 0.288_64, 0.121_6, 0.064]);
    }

    #[test]
    fn negative_damping_cuts_late_energy() {
        let (l, r) = decaying(2000);
        let (a, _) = shape_impulse(48000, &l, &r, &ImpulseShape { damping_high: -1.0, ..neutral_impulse_shape() });
        let sum = |v: &[f32]| v.iter().map(|x| x * x).sum::<f32>();
        assert!(sum(&a) < sum(&l) || a != l);
        assert!(a.iter().all(|v| v.is_finite()));
    }

    /// `0x1419b7fb0` / `0x1419b77f0` impulse vectors at 48 kHz, 4.8 kHz.
    #[test]
    fn early_late_filters_match_native_vectors() {
        let run = |hp| {
            let mut x = [1.0, 0.0, 0.0, 0.0];
            butterworth(&mut x, 48_000, 4_800.0, hp);
            x
        };
        let close = |a: [f32; 4], b: [f32; 4]| a.iter().zip(&b).all(|(a, b)| (a - b).abs() < 1e-6);
        assert!(close(run(false), [0.067_455_28, 0.212_010_62, 0.281_933_64, 0.234_726_31]), "{:?}", run(false));
        assert!(close(run(true), [0.638_945_5, -0.547_588_8, -0.250_695_56, -0.060_494_572]), "{:?}", run(true));
        let mut x = [1.0, 0.5];
        butterworth(&mut x, 44_100, 20_000.0, false);
        butterworth(&mut x, 44_100, 20.0, true);
        assert_eq!(x, [1.0, 0.5], "both bypass at their defaults");
    }

    /// `0x1419b1e30` vector: rate 40, ten samples, crossover 5.
    #[test]
    fn composition_crossfades_early_into_late() {
        let out = compose(&[1.0; 10], &[0.0; 10], crossover(10, 0.5, 40), 40);
        assert_eq!(out, [1.0, 1.0, 1.0, 1.0, 1.0, 0.499_999_97, 0.0, 0.0, 0.0, 0.0]);
    }
}
