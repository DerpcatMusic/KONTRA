//! Offline shaping of a convolution response before it plays: low and high
//! cuts for its early and late parts with a crossfade between them, and a
//! volume envelope. Every step is linear and time-invariant, so shaping the
//! response equals shaping the wet path. The filters are second-order
//! Butterworths run once forward; the crossfade is cos² over 50 ms
//! (Kontakt's CConvolver `0x1419b27a0`, `0x1419b1e30`, v2 sampler-core
//! space/shape.rs).

/// Low and high cut Hz of the early and late parts, and where (a fraction
/// of the length) the late part takes over; `None` filters it all as late.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cuts {
    pub early: [f32; 2],
    pub late: [f32; 2],
    pub crossover: Option<f32>,
}

/// A bilinear Butterworth low- or high-pass over `x`; a low-pass above
/// 0.45 of the rate or a high-pass below 0.01 of it passes.
fn butterworth(x: &mut [f32], rate: u32, hz: f32, high_pass: bool) {
    let w = hz / rate.max(1) as f32;
    if !w.is_finite() || (!high_pass && w > 0.45) || (high_pass && w < 0.01) {
        return;
    }
    let k = (w * std::f32::consts::PI).tan();
    let k2 = k * k;
    let n = 1.0 / (std::f32::consts::SQRT_2 * k + 1.0 + k2);
    let b = if high_pass {
        [n, -2.0 * n, n]
    } else {
        [k2 * n, 2.0 * k2 * n, k2 * n]
    };
    let a = [
        -(2.0 * (k2 - 1.0) * n),
        -((1.0 - std::f32::consts::SQRT_2 * k + k2) * n),
    ];
    let (mut x1, mut x2, mut y1, mut y2) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for v in x {
        let y = b[0] * *v + b[1] * x1 + b[2] * x2 + a[0] * y1 + a[1] * y2;
        (x2, x1, y2, y1) = (x1, *v, y1, y);
        *v = if y.abs() < 1e-20 { 0.0 } else { y };
    }
}

/// `x` through one part's `[low cut, high cut]`.
fn pass(x: &[f32], rate: u32, [low, high]: [f32; 2]) -> Vec<f32> {
    let mut y = x.to_vec();
    butterworth(&mut y, rate, high, false);
    butterworth(&mut y, rate, low, true);
    y
}

/// One channel through its early and late cuts.
#[must_use]
pub fn cut(x: &[f32], rate: u32, c: &Cuts) -> Vec<f32> {
    let Some(at) = c.crossover.filter(|_| c.early != c.late) else {
        return pass(x, rate, c.late);
    };
    let (early, late) = (pass(x, rate, c.early), pass(x, rate, c.late));
    let len = x.len();
    let at = ((len as f32 * at.clamp(0.0, 1.0)) + 0.5) as usize;
    // Early before the fade, late after it, cos² across 50 ms around `at`.
    let width = (rate as f32 * 0.05 + 0.5).floor().max(1.0);
    let start = at as f32 - width / 2.0;
    early
        .iter()
        .zip(&late)
        .enumerate()
        .map(|(n, (&e, &l))| {
            let k = n as f32 - start;
            if k < 0.0 {
                e
            } else if k >= width {
                l
            } else {
                let c = (k * std::f32::consts::FRAC_PI_2 / width).cos();
                c * c * e + (1.0 - c * c) * l
            }
        })
        .collect()
}

/// Scales `channels` by eight knots `[time fraction, dB]`, interpolated in
/// amplitude between neighbours in time; frames outside the knots keep
/// their level.
pub fn envelope(channels: [&mut [f32]; 2], knots: [[f32; 2]; 8]) {
    let mut knots = knots.map(|[x, db]| (x.clamp(0.0, 1.0), 10f32.powf(db / 20.0)));
    knots.sort_by(|a, b| a.0.total_cmp(&b.0));
    for channel in channels {
        let len = channel.len();
        let at = |x: f32| ((len as f32 * x) + 0.5) as usize;
        for pair in knots.windows(2) {
            let [(x0, g0), (x1, g1)] =
                [pair.first(), pair.last()].map(|k| k.copied().unwrap_or((0.0, 1.0)));
            let (start, end) = (at(x0), at(x1));
            let span = (end.max(start + 1) - start) as f32;
            for (k, v) in channel
                .iter_mut()
                .take(end.min(len))
                .skip(start)
                .enumerate()
            {
                *v *= g0 + (g1 - g0) * k as f32 / span;
            }
        }
    }
}
