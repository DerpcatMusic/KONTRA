//! Offline shaping of a convolution response before it plays: low and high
//! cuts for its early and late parts with a crossfade between them, and a
//! volume envelope. Every step is linear and time-invariant, so shaping the
//! response equals shaping the wet path. The filters are second-order
//! Butterworths run once forward; the crossfade is cos² over 50 ms.

/// Low and high cut Hz of the early and late parts, and where (a fraction
/// of the length) the late part takes over; `None` filters it all as late.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cuts {
    pub early: [f32; 2],
    pub late: [f32; 2],
    pub crossover: Option<f32>,
}

/// A low cut at or below this is open.
const OPEN_LOW_HZ: f32 = 20.0;

/// A bilinear Butterworth low- or high-pass over `x`; a low-pass above
/// 0.45 of the rate or a high-pass at or below 20 Hz (the open setting)
/// passes, as does a corner that is not a positive number.
fn butterworth(x: &mut [f32], rate: u32, hz: f32, high_pass: bool) {
    let w = hz / rate.max(1) as f32;
    if !w.is_finite() || w <= 0.0 || (!high_pass && w > 0.45) || (high_pass && hz <= OPEN_LOW_HZ) {
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

/// Where a response's early part ends, as a fraction of its length, as
/// measured natively: blocks of 2 ms and windows of 0.3 s over the energy
/// of the mono sum, and the split falls where the windows' spread (their
/// energies' second moment over the squared mean) settles below its mean
/// over the first 30 dB of decay. Not real-time: allocates.
#[must_use]
pub fn split(channels: [&[f32]; 2], rate: u32) -> f32 {
    let [l, r] = channels;
    let len = l.len().min(r.len());
    let sum: Vec<f32> = l.iter().zip(r).map(|(a, b)| a + b).collect();
    let mean = sum.iter().map(|&v| f64::from(v)).sum::<f64>() / len.max(1) as f64;
    let d: Vec<f64> = sum
        .iter()
        .map(|&v| ((f64::from(v) - mean) / 2.0).abs())
        .collect();
    // From a frame before the first sound to the last.
    let onset = d
        .iter()
        .position(|&v| v != 0.0)
        .map_or(0, |i| i.saturating_sub(1));
    let end = d
        .iter()
        .rposition(|&v| v != 0.0)
        .filter(|&i| i > 0)
        .unwrap_or(len);
    let y: Vec<f64> = d
        .get(onset..end)
        .unwrap_or_default()
        .iter()
        .map(|v| v * v)
        .collect();
    let span = y.len() as f64;
    let rate = f64::from(rate);
    let (hop, window) = (rate * 0.002, rate * 0.3);
    let (hop_n, window_n) = (hop as usize, window as usize);
    let scan = ((span - window) / hop).max(0.0) as usize;
    // Energy decay: what remains from each block on, over the whole.
    let mut decay: Vec<f64> = if y.len() < hop_n || hop_n == 0 {
        vec![y.iter().sum()]
    } else {
        let blocks = (((span - hop) / hop) as usize).max(1);
        y.chunks(hop_n)
            .take(blocks)
            .map(|b| b.iter().sum())
            .collect()
    };
    let mut rest = 0.0;
    for v in decay.iter_mut().rev() {
        rest += *v;
        *v = rest;
    }
    let whole = decay.first().copied().unwrap_or(0.0);
    let decay = |i: usize| {
        decay
            .get(i)
            .map_or(0.0, |v| if whole > 0.0 { v / whole } else { *v })
    };
    // Each window's energies' second moment over their squared mean.
    let mut sums = vec![(0.0, 0.0)];
    for v in &y {
        let (a, b) = sums.last().copied().unwrap_or_default();
        sums.push((a + v, b + v * v));
    }
    let moment = |from: usize, n: usize| {
        let at = |i: usize| sums.get(i).copied().unwrap_or_default();
        let ((a0, b0), (a1, b1)) = (at(from), at(from + n));
        let (s1, s2) = (a1 - a0, b1 - b0);
        let n = n as f64;
        if s1 > 0.0 {
            s2 / ((s1 / n).powi(2) * (n - 1.0).max(1.0))
        } else {
            0.0
        }
    };
    let stats: Vec<f64> = if y.len() < hop_n + window_n {
        vec![moment(0, y.len())]
    } else {
        let n = ((span - window) / hop - 1.0).max(0.0) as usize;
        (0..n).map(|k| moment(k * hop_n, window_n)).collect()
    };
    let stat = |i: usize| stats.get(i).copied().unwrap_or(0.0);
    let to = |floor: f64, from: usize| {
        (from..scan)
            .find(|&i| decay(i) <= floor)
            .unwrap_or(from.max(scan))
    };
    let i30 = to(1e-3, 0);
    let i60 = to(1e-6, i30);
    let mean = (0..i30).map(stat).sum::<f64>() / i30.max(1) as f64;
    let settled = (0..i60).find(|&i| stat(i) <= mean).unwrap_or(i60);
    // Back from -60 dB while the windows stay below the mean.
    let mut tail = i60;
    while tail > 0 && stat(tail) < mean {
        tail -= 1;
    }
    let block = |t: f64| (t * hop + 0.5) as usize;
    let at = if i30 < settled {
        if i30 < tail {
            (len / 2).saturating_sub(onset)
        } else {
            block(tail as f64)
        }
    } else if tail < i30 {
        block((tail + settled) as f64 / 2.0)
    } else {
        block(settled as f64)
    };
    (at + onset) as f32 / len.max(1) as f32
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

/// Taps either side of an output frame, counted at the lower rate.
const HALF: usize = 32;
/// Kernel phases per input frame; a tap reads between two of them.
const PHASES: usize = 512;
/// The passband edge as a share of the lower Nyquist.
const CUTOFF: f64 = 0.97;
/// Kaiser β: stopband about 80 dB down.
const BETA: f64 = 8.0;
/// The most phases a rational ratio may need for its exact polyphase
/// filter (44.1 to 48 kHz needs 160); past it, the tabled kernel.
const MOST_PHASES: usize = 4096;

/// `c` at rate `from` to rate `to`, at most `max` frames, through a
/// Kaiser-windowed sinc that passes up to [`CUTOFF`] of the lower Nyquist.
/// Not real-time: allocates.
#[must_use]
pub fn resample(c: &[f32], from: u32, to: u32, max: usize) -> Vec<f32> {
    if from == to || from == 0 || to == 0 {
        return c.iter().copied().take(max).collect();
    }
    // Input frames per output frame; the cutoff follows the lower rate.
    let ratio = f64::from(from) / f64::from(to);
    let scale = ratio.recip().min(1.0);
    let band = CUTOFF * scale;
    let width = HALF as f64 / scale;
    let den = crate::resample::i0(BETA);
    let kernel = |t: f64| {
        let u = t.abs() / width;
        if u >= 1.0 {
            return 0.0;
        }
        let x = std::f64::consts::PI * band * t;
        let sinc = if t == 0.0 { 1.0 } else { x.sin() / x };
        band * sinc * crate::resample::i0(BETA * (1.0 - u * u).sqrt()) / den
    };
    let frames = ((c.len() as f64 / ratio) as usize).min(max);
    let g = gcd(from, to);
    let (up, down) = ((to / g) as usize, (from / g) as usize);
    if up <= MOST_PHASES {
        // Output n sits `(n·down mod up)/up` past input `n·down div up`: one
        // exact row of taps per phase, each output a single dot.
        let reach = width.ceil() as usize;
        let len = 2 * reach + 1;
        let rows: Vec<f32> = (0..up)
            .flat_map(|p| {
                let frac = p as f64 / up as f64;
                (0..len).map(move |j| kernel(frac + reach as f64 - j as f64) as f32)
            })
            .collect();
        let mut padded = vec![0.0; c.len() + len];
        if let Some(s) = padded.get_mut(reach..reach + c.len()) {
            s.copy_from_slice(c);
        }
        return (0..frames)
            .map(|n| {
                let (i, p) = (n * down / up, n * down % up);
                let (Some(w), Some(k)) = (padded.get(i..i + len), rows.get(p * len..(p + 1) * len))
                else {
                    return 0.0;
                };
                crate::convolver::dot(w, k)
            })
            .collect();
    }
    let table: Vec<f64> = (0..=(width * PHASES as f64).ceil() as usize + 1)
        .map(|k| kernel(k as f64 / PHASES as f64))
        .collect();
    let tap = |t: f64| {
        let p = t * PHASES as f64;
        let k = p as usize;
        let f = p - k as f64;
        let a = table.get(k).copied().unwrap_or(0.0);
        let b = table.get(k + 1).copied().unwrap_or(0.0);
        a + (b - a) * f
    };
    (0..frames)
        .map(|n| {
            let x = n as f64 * ratio;
            let lo = (x - width).ceil().max(0.0) as usize;
            let hi = ((x + width).floor() as usize).min(c.len().saturating_sub(1));
            c.get(lo..=hi).map_or(0.0, |s| {
                s.iter()
                    .zip(lo..)
                    .map(|(&v, i)| f64::from(v) * tap((x - i as f64).abs()))
                    .sum::<f64>()
            }) as f32
        })
        .collect()
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}
