#![allow(dead_code)]
// The UVI SparkVerb engine (Workstation 4.0.9, init `0x1416799a0`, process
// `0x141679c90`), from the binary:
// - lines: `2^Quality` (`0x14167b8c0`); lengths `0x14167f190`: with
//   `a = (N/2)^Shape`, `r = (2a)^(-1/(N-1))`,
//   `x_i = r^i·N(1-r)·RoomSize·rate/340 / (1 - r^(N+1))`, truncated, raised
//   to the next odd prime (table `0x14202c3e0`, to 191999), sorted.
// - decay (`0x14167b8c0`): `x_i = -600·L_i/(rate·Decay·RoomSize)` dB
//   (frozen: `-0.006·L_i/rate`), band gain `10^(x_i/(20·D_band))`; the line
//   gain is the mid gain over `√N`, the low/high gains are two first-order
//   shelves (`0x141649120`) or a low cut (`0x141649410`) / high cut
//   (`0x141649220`) in one TDF-II biquad (`0x1416450c0`), then a 1 Hz DC
//   blocker (`0x141641be0`).
// - input gain `√((1-ḡ)/(1+ḡ))`, `ḡ` the mean squared mid gain.
// - modulation (`0x14167bd80`): per line, a 256-entry sine table read at
//   `f_i = ModRate·rate/(16·L_i)`, depth `cents·ln2·rate/(1200·2π·f_i)`
//   samples, sampled once per ≤64-frame block and glided across it (step
//   clamped to the block length).
// - Hadamard feedback (`0x1416864d0`), output taps after it
//   (`0x14167c0e0`): `√(2/N)·(cos θ·Σeven + sin θ·Σodd)` and mirrored,
//   `θ = (1-Width)·π/4`; line `i` takes input channel `i mod 2`.
// - input diffuser (`0x14167e0f0`/`0x14167e420`): four serial allpasses per
//   channel, gain Diffusion, lengths `⌊(π/2)^k·rate·DiffusionStart/1000 + ½⌋`
//   plus the stereo offsets (`0x14202c2d0`).
// - Rolloff: one-pole low-pass `exp(-2π·f/rate)` on each output, off at or
//   above 19999 Hz.
//
// Parameter ramps (the 10 ms gain ramps, the delay glide after a size
// change) are not reproduced: controls are constant for a unit's life.

use crate::Error;
use sampler_ir::fx::space::{Network, SparkVerb};
use std::f64::consts::PI;

const BLOCK: usize = 64;
const MAX_PRIME: u32 = 191_999;

fn is_prime(n: u32) -> bool {
    n >= 2 && (2..).take_while(|d| d * d <= n).all(|d| n % d != 0)
}

/// Smallest odd prime at or above `n` (3 at least, at most 191999).
fn prime_at_least(n: u32) -> u32 {
    let mut k = n.max(3) | 1;
    while k < MAX_PRIME && !is_prime(k) {
        k += 2;
    }
    k.min(MAX_PRIME)
}

/// Sorted line lengths in samples (`0x14167f190`).
fn lengths(n: usize, room_m: f64, shape: f64, rate: f64) -> Vec<u32> {
    let a = (n as f64 * 0.5).powf(shape);
    let r = (2.0 * a).powf(-1.0 / (n as f64 - 1.0));
    let scale = n as f64 * (1.0 - r) * room_m * rate * 0.002_941_176_470_588_235_3 / (1.0 - r.powi(n as i32 + 1));
    let mut out: Vec<u32> = (0..n).map(|i| prime_at_least((r.powi(i as i32) * scale) as u32)).collect();
    out.sort_unstable();
    out
}

/// First-order section `(b0, b1, a1)` with DC gain `g0` and Nyquist gain
/// `gpi` (`0x141649120`).
fn shelf(fc: f64, g0: f64, gpi: f64) -> [f64; 3] {
    let r = g0 / gpi;
    let t = (PI * fc).tan();
    let (lo, hi) = (t / r.sqrt(), t * r.sqrt());
    [gpi * (hi + 1.0) / (lo + 1.0), gpi * (hi - 1.0) / (lo + 1.0), (lo - 1.0) / (lo + 1.0)]
}

/// Low cut (`0x141649410`) and high cut (`0x141649220`) sections.
fn cut(fc: f64, high: bool) -> [f64; 3] {
    let t = (PI * fc).tan();
    let s = 0.5 / fc;
    let d = 1.0 / (t + 1.0);
    if high {
        let k = 1.0 / (1.0 + s * s).sqrt();
        [(t + k) * d, (t - k) * d, (t - 1.0) * d]
    } else {
        let b0 = d * s / (1.0 + s * s).sqrt();
        [b0, -b0, (t - 1.0) * d]
    }
}

struct Ring {
    buf: Vec<f32>,
    mask: usize,
}

impl Ring {
    fn new(len: usize) -> Self {
        let n = len.next_power_of_two().max(2);
        Self { buf: vec![0.0; n], mask: n - 1 }
    }
    #[inline(always)]
    fn at(&self, w: usize, k: usize) -> f32 {
        self.buf[w.wrapping_sub(k) & self.mask]
    }
    #[inline(always)]
    fn put(&mut self, w: usize, v: f32) {
        let m = self.mask;
        self.buf[w & m] = v;
    }
    /// Read `d` frames behind `w` with interpolation `mode`.
    #[inline(always)]
    fn read(&self, w: usize, d: f64, mode: u8) -> f32 {
        let i = d as usize;
        let x = (d - i as f64) as f32;
        match mode {
            0 => self.at(w, i),
            1 => {
                let (a, b) = (self.at(w, i), self.at(w, i + 1));
                a + (b - a) * x
            }
            _ => {
                let (m, a, b, n) = (self.at(w, i - 1), self.at(w, i), self.at(w, i + 1), self.at(w, i + 2));
                let (xm, xp, x2) = (x - 1.0, x + 1.0, x - 2.0);
                -x * xm * x2 / 6.0 * m + xp * xm * x2 / 2.0 * a - xp * x * x2 / 2.0 * b + xp * x * xm / 6.0 * n
            }
        }
    }
}

struct Line {
    ring: Ring,
    base: f64,
    cur: f64,
    amp: f64,
    phase: u32,
    inc: u32,
    gain: f32,
    /// b0, b1, b2, a1, a2 and the two TDF-II states.
    bq: [f32; 5],
    s: [f32; 2],
    dc: f32,
}

struct Allpass {
    ring: Ring,
    len: usize,
}

pub(crate) struct Spark {
    lines: Vec<Line>,
    mode: u8,
    input_gain: f32,
    pre: [Ring; 2],
    pre_len: usize,
    diffuser: Option<(f32, [[Allpass; 4]; 2])>,
    out: [f32; 2],
    dc_pole: f32,
    rolloff: Option<f32>,
    lp: [f32; 2],
    w: usize,
    scratch: Vec<f32>,
}

impl Spark {
    pub(crate) fn new(n: &Network, rate: u32) -> Result<Self, Error> {
        let s = n.spark.ok_or(Error::InvalidInput)?;
        Ok(Self::with(&s, f64::from(rate)))
    }

    pub(crate) fn with(s: &SparkVerb, rate: f64) -> Self {
        let n = 1usize << s.quality;
        let lens = lengths(n, f64::from(s.room_m), f64::from(s.shape), rate);
        let (fl, fh) = ((f64::from(s.freq_low) / rate).min(0.499), (f64::from(s.freq_high) / rate).min(0.499));
        let mut mean = 0.0;
        let lines: Vec<Line> = lens
            .iter()
            .enumerate()
            .map(|(i, &len)| {
                let l = f64::from(len);
                let x = if s.freeze { -0.006 * l / rate } else { -600.0 * l / (rate * f64::from(s.decay) * f64::from(s.room_m)) };
                let band = |d: f32| 10f64.powf(0.05 * x / f64::from(d));
                let (gm, gl, gh) = (band(1.0), band(s.decay_low), band(s.decay_high));
                mean += gm * gm;
                let lo = if s.low_cut { cut(fl, false) } else { shelf(fl, gl / gm, 1.0) };
                let hi = if s.high_cut { cut(fh, true) } else { shelf(fh, 1.0, gh / gm) };
                let bq = [lo[0] * hi[0], lo[0] * hi[1] + lo[1] * hi[0], lo[1] * hi[1], lo[2] + hi[2], lo[2] * hi[2]];
                let f = f64::from(s.mod_hz) * rate / (16.0 * l);
                let amp = f64::from(s.mod_cents) * std::f64::consts::LN_2 * rate / (7_539.822_368_615_503 * f);
                Line {
                    ring: Ring::new(len as usize + amp.ceil() as usize + BLOCK + 4),
                    base: l,
                    cur: l,
                    amp,
                    phase: if i % 2 == 1 { 1 << 31 } else { 0 },
                    inc: (4_294_967_296.0 * f / rate) as i64 as u32,
                    gain: (gm / (n as f64).sqrt()) as f32,
                    bq: bq.map(|v| v as f32),
                    s: [0.0; 2],
                    dc: 0.0,
                }
            })
            .collect();
        let mean = mean / n as f64;
        let theta = f64::from(1.0 - s.width) * PI / 4.0;
        let sc = (2.0 / n as f64).sqrt();
        let pre_len = (f64::from(s.predelay_seconds) * rate) as usize;
        let diffuser = s.diffuser.map(|(g, ms)| {
            const OFFSET: [[usize; 4]; 2] = [[0, 23, 0, 23], [23, 0, 23, 0]];
            let base = |k: i32| ((PI / 2.0).powi(k) * rate * 0.001 * f64::from(ms) + 0.5).floor() as usize;
            (
                g,
                std::array::from_fn(|ch| {
                    std::array::from_fn(|k| {
                        let len = (base(k as i32) + OFFSET[ch][k]).max(1);
                        Allpass { ring: Ring::new(len + 1), len }
                    })
                }),
            )
        });
        Self {
            lines,
            mode: s.mode.min(2),
            input_gain: ((1.0 - mean) / (1.0 + mean)).max(0.0).sqrt() as f32,
            pre: [Ring::new(pre_len + 1), Ring::new(pre_len + 1)],
            pre_len,
            diffuser,
            out: [(sc * theta.cos()) as f32, (sc * theta.sin()) as f32],
            dc_pole: (-2.0 * PI / rate).exp() as f32,
            rolloff: (s.rolloff_hz < 19_999.0).then(|| (-2.0 * PI * f64::from(s.rolloff_hz) / rate).exp() as f32),
            lp: [0.0; 2],
            w: 0,
            scratch: vec![0.0; n],
        }
    }

    pub(crate) fn clear(&mut self) {
        for line in &mut self.lines {
            line.ring.buf.fill(0.0);
            line.cur = line.base;
            (line.s, line.dc) = ([0.0; 2], 0.0);
        }
        for (i, line) in self.lines.iter_mut().enumerate() {
            line.phase = if i % 2 == 1 { 1 << 31 } else { 0 };
        }
        for r in &mut self.pre {
            r.buf.fill(0.0);
        }
        if let Some((_, d)) = &mut self.diffuser {
            d.iter_mut().flatten().for_each(|a| a.ring.buf.fill(0.0));
        }
        self.lp = [0.0; 2];
    }

    /// Replace `l`/`r` with the wet signal.
    pub(crate) fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len().min(r.len());
        let mut start = 0;
        while start < n {
            let len = (n - start).min(BLOCK);
            self.block(&mut l[start..start + len], &mut r[start..start + len]);
            start += len;
        }
    }

    fn block(&mut self, l: &mut [f32], r: &mut [f32]) {
        let len = l.len();
        let span = len as f64 - 1e-10;
        let mut glide = [(0f64, 0f64); 32];
        for (line, g) in self.lines.iter_mut().zip(&mut glide) {
            line.phase = line.phase.wrapping_add((len as u32).wrapping_mul(line.inc));
            let target = line.base + line.amp * f64::from(crate::blocks::lfo::uvi_sine(line.phase));
            let next = line.cur + (target - line.cur).clamp(-span, span);
            *g = (line.cur, (next - line.cur) / len as f64);
            line.cur = next;
        }
        let one = 1.0 - self.dc_pole;
        for k in 0..len {
            let w = self.w;
            self.w = w.wrapping_add(1);
            let mut x = [l[k] * self.input_gain, r[k] * self.input_gain];
            for (c, ring) in self.pre.iter_mut().enumerate() {
                ring.put(w, x[c]);
                x[c] = ring.at(w, self.pre_len);
            }
            if let Some((g, units)) = &mut self.diffuser {
                for (c, chain) in units.iter_mut().enumerate() {
                    for a in chain.iter_mut() {
                        let out = a.ring.at(w, a.len);
                        let v = x[c] - *g * out;
                        a.ring.put(w, v);
                        x[c] = out + *g * v;
                    }
                }
            }
            let s = &mut self.scratch;
            for (i, line) in self.lines.iter().enumerate() {
                let (d0, step) = glide[i];
                s[i] = line.ring.read(w, d0 + k as f64 * step, self.mode) * line.gain;
            }
            let mut h = 1;
            while h < s.len() {
                for i in (0..s.len()).step_by(2 * h) {
                    for j in i..i + h {
                        let (a, b) = (s[j], s[j + h]);
                        (s[j], s[j + h]) = (a + b, a - b);
                    }
                }
                h *= 2;
            }
            let (mut even, mut odd) = (0f32, 0f32);
            for (i, v) in s.iter().enumerate() {
                if i % 2 == 0 { even += v } else { odd += v }
            }
            let mut out = [self.out[0] * even + self.out[1] * odd, self.out[1] * even + self.out[0] * odd];
            for (i, line) in self.lines.iter_mut().enumerate() {
                let v = s[i];
                let [b0, b1, b2, a1, a2] = line.bq;
                let y = b0 * v + line.s[0];
                line.s[0] = b1 * v - a1 * y + line.s[1];
                line.s[1] = b2 * v - a2 * y;
                line.dc += one * (y - line.dc);
                line.ring.put(w, y - line.dc + x[i % 2]);
            }
            if let Some(a) = self.rolloff {
                for c in 0..2 {
                    self.lp[c] += (1.0 - a) * (out[c] - self.lp[c]);
                    out[c] = self.lp[c];
                }
            }
            l[k] = out[0];
            r[k] = out[1];
        }
    }
}

