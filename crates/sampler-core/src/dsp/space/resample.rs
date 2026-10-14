//! Kontakt's integer-ratio resampler pair (class `0x141ee5da0`, quality 2):
//! a 16-tap Kaiser-windowed sinc (β 8, `0x141ee7340`) in `N - 1` phase rows
//! (`0x141ee6e00`), 8 low-rate frames of latency per stage. The Galois
//! Reverb runs its core between a pair above 48 kHz (wrapper `0x141aa15c7`,
//! processing `0x141aa9480`).

/// Bessel `I0` (the Numerical Recipes polynomials, f64).
fn i0(u: f64) -> f64 {
    if u < 3.75 {
        let t = u * u * (1.0 / 14.0625);
        1.0 + t * (3.515_622_9 + t * (3.089_942_4 + t * (1.206_749_2 + t * (0.265_973_2 + t * (0.036_076_8 + t * 0.004_581_3)))))
    } else {
        u.exp() / u.sqrt() * poly(3.75 / u)
    }
}

fn poly(y: f64) -> f64 {
    0.398_942_28
        + y * (0.013_285_92 + y * (0.002_253_19 + y * (-0.001_575_65 + y * (0.009_162_81 + y * (-0.020_577_06 + y * (0.026_355_37 + y * (-0.016_476_33 + y * 0.003_923_77)))))))
}

/// The windowed sinc at `x` (`|x| < 8`), rounded to f32.
pub(super) fn h(x: f32) -> f32 {
    if x == 0.0 {
        return 1.0;
    }
    let px = f64::from_bits(0x4009_21fb_5444_2d11) * f64::from(x);
    let t = f64::from((x + x) / 16.0);
    let u = (1.0 - t * t).sqrt() * 8.0;
    // I0(8) as native: f32 expf(8) / f32 sqrtf(8), times the f64 polynomial.
    let den = f64::from(8f32.exp() / 8f32.sqrt()) * poly(3.75 / 8.0);
    (px.sin() / px * (i0(u) / den)) as f32
}

/// Phase rows `r = 1..N`, taps `k = -8..7`: `h(phase_r + k)`; the phase
/// accumulates `±1/N` in f32 from 0 (up) or 1 (down).
fn rows(n: usize, down: bool) -> Vec<[f32; 16]> {
    let step = 1.0 / n as f32;
    let mut phase = if down { 1.0f32 } else { 0.0 };
    (1..n)
        .map(|_| {
            phase = if down { phase - step } else { phase + step };
            std::array::from_fn(|k| h(phase + (k as f32 - 8.0)))
        })
        .collect()
}

/// A stereo decimate-by-`n`, process, interpolate-by-`n` pair.
pub(super) struct Pair {
    n: usize,
    up: Vec<[f32; 16]>,
    down: Vec<[f32; 16]>,
    /// The last `16·n` host inputs and the last 16 low-rate outputs, per
    /// channel, as rings.
    hist: [Vec<f32>; 2],
    low_hist: [[f32; 16]; 2],
    hw: usize,
    lw: usize,
    /// Host inputs since the last low-rate sample.
    phase: usize,
    /// Upsampled frames not yet played.
    fifo: [Vec<f32>; 2],
    head: usize,
    fill: usize,
    low: [Vec<f32>; 2],
}

impl Pair {
    /// For host blocks of at most `block` frames.
    pub(super) fn new(n: usize, block: usize) -> Self {
        let cap = (block / n + 2) * n + n;
        let mut p = Self {
            n,
            up: rows(n, false),
            down: rows(n, true),
            hist: [vec![0.0; 16 * n], vec![0.0; 16 * n]],
            low_hist: [[0.0; 16]; 2],
            hw: 0,
            lw: 0,
            phase: 0,
            fifo: [vec![0.0; cap], vec![0.0; cap]],
            head: 0,
            fill: 0,
            low: [vec![0.0; block / n + 2], vec![0.0; block / n + 2]],
        };
        p.clear();
        p
    }

    pub(super) fn clear(&mut self) {
        self.hist.iter_mut().for_each(|h| h.fill(0.0));
        self.fifo.iter_mut().for_each(|h| h.fill(0.0));
        (self.low_hist, self.hw, self.lw, self.phase, self.head) = ([[0.0; 16]; 2], 0, 0, 0, 0);
        // The low-rate sample 0 (silence) is already upsampled.
        self.fill = self.n;
    }

    /// Run `core` at the low rate on `l`/`r` in place.
    pub(super) fn process(&mut self, l: &mut [f32], r: &mut [f32], core: impl FnOnce(&mut [f32], &mut [f32])) {
        let (n, len) = (self.n, l.len());
        let hl = 16 * n;
        // Decimate: a low-rate sample after every n inputs.
        let mut count = 0;
        for i in 0..len {
            for (c, x) in [l[i], r[i]].into_iter().enumerate() {
                self.hist[c][self.hw] = x;
            }
            self.hw = if self.hw + 1 == hl { 0 } else { self.hw + 1 };
            self.phase += 1;
            if self.phase == n {
                self.phase = 0;
                for c in 0..2 {
                    // Newest input is m·n - 1; age a = m·n - 1 - index.
                    let at = |a: usize| self.hist[c][(self.hw + hl - 1 - a) % hl];
                    // y[m] = (x[(m-8)n] + Σ_r Σ_k x[(m-9-k)n + r]·D_r[k]) / n.
                    let mut acc = at(8 * n - 1);
                    for (ri, row) in self.down.iter().enumerate() {
                        let r = ri + 1;
                        for (ki, tap) in row.iter().enumerate() {
                            // k = ki - 8: index (m-9-k)n + r, age n·(k+9) - 1 - r.
                            acc += at(n * (ki + 1) - 1 - r) * tap;
                        }
                    }
                    self.low[c][count] = acc * (1.0 / n as f32);
                }
                count += 1;
            }
        }
        let [lo, ro] = &mut self.low;
        core(&mut lo[..count], &mut ro[..count]);
        // Interpolate each low-rate sample q into frames q·n .. q·n + n - 1.
        let cap = self.fifo[0].len();
        for q in 0..count {
            for c in 0..2 {
                self.low_hist[c][self.lw] = self.low[c][q];
            }
            self.lw = (self.lw + 1) & 15;
            for r in 0..n {
                let at = (self.head + self.fill) % cap;
                for c in 0..2 {
                    let x = |a: usize| self.low_hist[c][(self.lw + 15 - a) & 15];
                    // y[qn + r] = Σ_k x[q-8-k]·U_r[k] (r = 0: x[q-8]).
                    self.fifo[c][at] = if r == 0 { x(8) } else { self.up[r - 1].iter().enumerate().map(|(ki, t)| x(ki) * t).fold(0.0, |s, v| s + v) };
                }
                self.fill += 1;
            }
        }
        for i in 0..len {
            l[i] = self.fifo[0][self.head];
            r[i] = self.fifo[1][self.head];
            self.head = (self.head + 1) % cap;
        }
        self.fill -= len;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Native rows (`0x141ee6e00`): phase ½ for N = 2, phase ¼ for N = 4.
    #[test]
    fn kaiser_rows_match_native_bits() {
        let half: [u32; 8] = [0xb9d59b5f, 0x3b179cc5, 0xbbfab429, 0x3ca00b78, 0xbd2fc599, 0x3db3651f, 0xbe3e500d, 0x3f209bb8];
        let row = rows(2, false)[0].map(f32::to_bits);
        assert_eq!(row[..8], half);
        assert_eq!(row[8..].iter().rev().copied().collect::<Vec<_>>(), half);
        let quarter: [u32; 16] = [
            0xb920995e, 0x3a952395, 0xbb877cd9, 0x3c362d9d, 0xbcce38b9, 0x3d54b806, 0xbddbc955, 0x3e94ac90, 0x3f65a3eb, 0xbe2833e4, 0x3d978965, 0xbd151d8d, 0x3c8b4cfe, 0xbbe45c8a,
            0x3b15864a, 0xb9fe6b9f,
        ];
        assert_eq!(rows(4, false)[0].map(f32::to_bits), quarter);
    }

    fn through(n: usize, at: usize, frames: usize, low_only: bool) -> Vec<f32> {
        let mut p = Pair::new(n, 64);
        let mut l = vec![0f32; frames];
        l[at] = 1.0;
        let mut r = l.clone();
        let mut lows = Vec::new();
        for (a, b) in l.chunks_mut(37).zip(r.chunks_mut(37)) {
            p.process(a, b, |x, _| lows.extend_from_slice(x));
        }
        if low_only { lows } else { l }
    }

    /// Native vectors: ÷2 of an impulse at 0 is 0.5 at low sample 8; at 1,
    /// half the phase-½ row over 1..16; ÷4 at 1: 0.22425811 at 8.
    #[test]
    fn decimator_matches_native_vectors() {
        let y = through(2, 0, 64, true);
        // Low sample m is emitted after input m·n - 1: index m - 1 here.
        assert_eq!(y[7], 0.5);
        assert!(y.iter().enumerate().all(|(i, v)| i == 7 || *v == 0.0));
        let y = through(2, 1, 64, true);
        assert!((y[7] - 0.313_688_04).abs() < 1e-7 && (y[8] - 0.313_688_04).abs() < 1e-7, "{:?}", &y[..17]);
        let y = through(4, 1, 128, true);
        assert!((y[7] - 0.224_258_11).abs() < 1e-7 && (y[8] - 0.072_594_76).abs() < 1e-7, "{:?}", &y[..17]);
    }

    /// A pair passes a band-limited signal with 16 low-rate frames of delay.
    #[test]
    fn pair_delays_by_sixteen_low_frames() {
        let y = through(2, 0, 200, false);
        let peak = y.iter().enumerate().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).unwrap().0;
        assert_eq!((peak, y[32]), (32, 0.5), "{:?}", &y[25..45]);
    }
}
