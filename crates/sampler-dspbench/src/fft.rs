//! Radix-2 FFT and windowed power spectrum, enough for the bench's aliasing and THD metrics.

use std::f64::consts::TAU;

#[derive(Clone, Copy, Default)]
pub struct C {
    pub re: f64,
    pub im: f64,
}

/// In-place iterative radix-2 FFT. `x.len()` must be a power of two.
pub fn fft(x: &mut [C]) {
    let n = x.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            x.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -TAU / len as f64;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (s, c) = (ang * k as f64).sin_cos();
                let (a, b) = (x[start + k], x[start + k + len / 2]);
                let t = C { re: b.re * c - b.im * s, im: b.re * s + b.im * c };
                x[start + k] = C { re: a.re + t.re, im: a.im + t.im };
                x[start + k + len / 2] = C { re: a.re - t.re, im: a.im - t.im };
            }
        }
        len <<= 1;
    }
}

/// Power per bin (bins 0..=n/2) of the last `n` samples, Blackman-Harris windowed and normalised so
/// a full-scale sine's peak bin is about 0 dB relative to the window's coherent gain.
pub fn power_spectrum(x: &[f32], n: usize) -> Vec<f64> {
    let start = x.len().saturating_sub(n);
    let mut buf: Vec<C> = (0..n)
        .map(|i| {
            let w = 0.35875 - 0.48829 * (TAU * i as f64 / n as f64).cos() + 0.14128 * (2.0 * TAU * i as f64 / n as f64).cos()
                - 0.01168 * (3.0 * TAU * i as f64 / n as f64).cos();
            let v = f64::from(*x.get(start + i).unwrap_or(&0.0));
            C { re: v * w, im: 0.0 }
        })
        .collect();
    fft(&mut buf);
    let scale = 2.0 / n as f64;
    buf[..=n / 2].iter().map(|c| (c.re * c.re + c.im * c.im) * scale * scale).collect()
}

pub fn bin_hz(rate: f64, n: usize, bin: usize) -> f64 {
    bin as f64 * rate / n as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sine on an exact bin lands in that bin.
    #[test]
    fn sine_peaks_in_its_bin() {
        let n = 4096;
        let x: Vec<f32> = (0..n).map(|i| (TAU * 64.0 * i as f64 / n as f64).sin() as f32).collect();
        let p = power_spectrum(&x, n);
        let peak = (0..p.len()).max_by(|&a, &b| p[a].total_cmp(&p[b])).unwrap();
        assert_eq!(peak, 64);
    }
}
