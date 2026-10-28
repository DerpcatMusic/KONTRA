//! An integer-ratio resampler pair (Kontakt's class `0x141ee5da0`, quality
//! 2): a 16-tap Kaiser-windowed sinc (β 8, `0x141ee7340`) in `n − 1` phase
//! rows (`0x141ee6e00`), 8 low-rate frames of latency per stage. A network
//! runs between a pair above 48 kHz (wrapper `0x141aa15c7`).

/// The most host frames [`Pair::process`] takes at once.
pub(crate) const BLOCK: usize = 64;

/// The polynomial tail of the modified Bessel function I0 for `y = 3.75/x`.
fn big(y: f64) -> f64 {
    let p = (((y * 0.003_923_77 - 0.016_476_33) * y + 0.026_355_37) * y - 0.020_577_06) * y
        + 0.009_162_81;
    (((p * y - 0.001_575_65) * y + 0.002_253_19) * y + 0.013_285_92) * y + 0.398_942_28
}

/// I0(x), the polynomial approximation the native uses.
fn i0(x: f64) -> f64 {
    if x >= 3.75 {
        (x.exp() / x.sqrt()) * big(3.75 / x)
    } else {
        let y = x * x * 0.071_111_111_111_111_11;
        (((((y * 0.004_581_3 + 0.036_076_8) * y + 0.265_973_2) * y + 1.206_749_2) * y
            + 3.089_942_4)
            * y
            + 3.515_622_9)
            * y
            + 1.0
    }
}

/// The windowed sinc at `x` (`|x| < 8`), rounded to f32.
fn h(x: f32) -> f32 {
    if x == 0.0 {
        return 1.0;
    }
    let px = f64::from_bits(0x4009_21fb_5444_2d11) * f64::from(x);
    let t = f64::from((x + x) / 16.0);
    let u = (1.0 - t * t).sqrt() * 8.0;
    // I0(8) as native: f32 expf(8) / f32 sqrtf(8), times the f64 polynomial.
    let den = f64::from(8f32.exp() / 8f32.sqrt()) * big(3.75 / 8.0);
    (px.sin() / px * (i0(u) / den)) as f32
}

/// Phase rows `r = 1..n`, taps `k = −8..7`: `h(phase_r + k)`; the phase
/// steps `±1/n` in f32 from 0 (up) or 1 (down).
fn rows(n: usize, down: bool) -> Box<[[f32; 16]]> {
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
pub(crate) struct Pair {
    n: usize,
    up: Box<[[f32; 16]]>,
    down: Box<[[f32; 16]]>,
    /// The last `16·n` host frames, a ring written at `hw`.
    hist: Box<[[f32; 2]]>,
    hw: usize,
    /// The last 16 low-rate frames, a ring written at `lw`.
    low_hist: [[f32; 2]; 16],
    lw: usize,
    /// Host frames since the last low-rate frame.
    phase: usize,
    /// Upsampled frames not yet played: `fill` of them from `head`.
    fifo: Box<[[f32; 2]]>,
    head: usize,
    fill: usize,
    low: Box<[[f32; 2]]>,
}

impl Pair {
    /// Not real-time: allocates.
    pub(crate) fn new(n: usize) -> Self {
        let n = n.max(2);
        Self {
            n,
            up: rows(n, false),
            down: rows(n, true),
            hist: vec![[0.0; 2]; 16 * n].into(),
            hw: 0,
            low_hist: [[0.0; 2]; 16],
            lw: 0,
            phase: 0,
            fifo: vec![[0.0; 2]; (BLOCK / n + 2) * n + n].into(),
            head: 0,
            // The low-rate frame 0 (silence) is already upsampled.
            fill: n,
            low: vec![[0.0; 2]; BLOCK / n + 2].into(),
        }
    }

    /// Runs `core` at the low rate on `io` (at most [`BLOCK`] frames) in place.
    pub(crate) fn process(&mut self, io: &mut [[f32; 2]], core: impl FnOnce(&mut [[f32; 2]])) {
        let n = self.n;
        let hl = self.hist.len();
        let mut count = 0;
        for &x in io.iter() {
            if let Some(slot) = self.hist.get_mut(self.hw) {
                *slot = x;
            }
            self.hw = if self.hw + 1 == hl { 0 } else { self.hw + 1 };
            self.phase += 1;
            if self.phase < n {
                continue;
            }
            self.phase = 0;
            // The newest input has age 0.
            let at = |a: usize| {
                self.hist
                    .get((self.hw + hl - 1 - a) % hl)
                    .copied()
                    .unwrap_or([0.0; 2])
            };
            // y[m] = (x[(m−8)n] + Σ_r Σ_k x[(m−9−k)n + r]·D_r[k]) / n.
            let mut acc = at(8 * n - 1);
            for (ri, row) in self.down.iter().enumerate() {
                for (ki, &tap) in row.iter().enumerate() {
                    let x = at(n * (ki + 1) - 2 - ri);
                    acc = [acc[0] + x[0] * tap, acc[1] + x[1] * tap];
                }
            }
            if let Some(slot) = self.low.get_mut(count) {
                *slot = acc.map(|v| v * (1.0 / n as f32));
                count += 1;
            }
        }
        let low = self.low.get_mut(..count).unwrap_or_default();
        core(low);
        // Interpolate each low-rate frame q into host frames q·n .. q·n + n − 1.
        let cap = self.fifo.len();
        for &q in low.iter() {
            if let Some(slot) = self.low_hist.get_mut(self.lw) {
                *slot = q;
            }
            self.lw = (self.lw + 1) & 15;
            let x = |a: usize| {
                self.low_hist
                    .get((self.lw + 15 - a) & 15)
                    .copied()
                    .unwrap_or([0.0; 2])
            };
            for r in 0..n {
                // y[qn + r] = Σ_k x[q−8−k]·U_r[k] (r = 0: x[q−8]).
                let y = match r.checked_sub(1).and_then(|r| self.up.get(r)) {
                    None => x(8),
                    Some(row) => row.iter().enumerate().fold([0.0; 2], |s, (k, &t)| {
                        let v = x(k);
                        [s[0] + v[0] * t, s[1] + v[1] * t]
                    }),
                };
                if let Some(slot) = self.fifo.get_mut((self.head + self.fill) % cap) {
                    *slot = y;
                }
                self.fill += 1;
            }
        }
        for frame in io.iter_mut() {
            *frame = self.fifo.get(self.head).copied().unwrap_or([0.0; 2]);
            self.head = (self.head + 1) % cap;
        }
        self.fill = self.fill.saturating_sub(io.len());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Native rows (`0x141ee6e00`): phase ½ for n = 2, phase ¼ for n = 4.
    #[test]
    fn kaiser_rows_match_native_bits() {
        let half: [u32; 8] = [
            0xb9d59b5f, 0x3b179cc5, 0xbbfab429, 0x3ca00b78, 0xbd2fc599, 0x3db3651f, 0xbe3e500d,
            0x3f209bb8,
        ];
        let row = rows(2, false)[0].map(f32::to_bits);
        assert_eq!(row[..8], half);
        assert_eq!(row[8..].iter().rev().copied().collect::<Vec<_>>(), half);
        let quarter: [u32; 16] = [
            0xb920995e, 0x3a952395, 0xbb877cd9, 0x3c362d9d, 0xbcce38b9, 0x3d54b806, 0xbddbc955,
            0x3e94ac90, 0x3f65a3eb, 0xbe2833e4, 0x3d978965, 0xbd151d8d, 0x3c8b4cfe, 0xbbe45c8a,
            0x3b15864a, 0xb9fe6b9f,
        ];
        assert_eq!(rows(4, false)[0].map(f32::to_bits), quarter);
    }

    /// An impulse at `at` through a pair in 37-frame calls: the host output,
    /// or the low-rate frames the core saw.
    fn through(n: usize, at: usize, frames: usize, low_only: bool) -> Vec<f32> {
        let mut p = Pair::new(n);
        let mut io = vec![[0f32; 2]; frames];
        io[at] = [1.0; 2];
        let mut lows = Vec::new();
        for chunk in io.chunks_mut(37) {
            p.process(chunk, |x| lows.extend(x.iter().map(|f| f[0])));
        }
        if low_only {
            lows
        } else {
            io.iter().map(|f| f[0]).collect()
        }
    }

    /// Native vectors: ÷2 of an impulse at 0 is 0.5 at low frame 8; at 1,
    /// half the phase-½ row over 1..16; ÷4 at 1: 0.22425811 at 8.
    #[test]
    fn the_decimator_matches_native_vectors() {
        let y = through(2, 0, 64, true);
        // Low frame m comes after input m·n − 1: index m − 1 here.
        assert_eq!(y[7], 0.5);
        assert!(y.iter().enumerate().all(|(i, v)| i == 7 || *v == 0.0));
        let y = through(2, 1, 64, true);
        assert!((y[7] - 0.313_688_04).abs() < 1e-7 && (y[8] - 0.313_688_04).abs() < 1e-7);
        let y = through(4, 1, 128, true);
        assert!((y[7] - 0.224_258_11).abs() < 1e-7 && (y[8] - 0.072_594_76).abs() < 1e-7);
    }

    /// A pair passes a band-limited signal with 16 low-rate frames of delay.
    #[test]
    fn a_pair_delays_by_sixteen_low_frames() {
        let y = through(2, 0, 200, false);
        let peak = (0..y.len()).max_by(|&a, &b| y[a].abs().total_cmp(&y[b].abs()));
        assert_eq!((peak, y[32]), (Some(32), 0.5));
    }
}
