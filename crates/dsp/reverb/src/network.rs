//! A translator's reverb network ([`ReverbNetwork`]): 31 prime-length lines
//! in a circulant feedback matrix, damped on the read, tapped into two
//! allpass-unit chains, then a low shelf, a high cut and the width.

use crate::Settings;
use crate::fade::{Fade, flush};
use crate::fdn::{MAX_PREDELAY_MS, MAX_SIZE};
use kontra_law::{NETWORK_LINES as LINES, ReverbNetwork};
use std::f64::consts::PI;

/// A power-of-two ring; reads past its length wrap.
struct Ring {
    buf: Box<[f32]>,
    mask: usize,
}

impl Ring {
    /// Not real-time: allocates.
    fn new(min_len: usize) -> Self {
        let n = min_len.next_power_of_two().max(2);
        Self {
            buf: vec![0.0; n].into(),
            mask: n - 1,
        }
    }

    /// The frame written `k` frames before `w`.
    fn at(&self, w: usize, k: usize) -> f32 {
        self.buf
            .get(w.wrapping_sub(k) & self.mask)
            .copied()
            .unwrap_or(0.0)
    }

    fn put(&mut self, w: usize, v: f32) {
        if let Some(s) = self.buf.get_mut(w & self.mask) {
            *s = v;
        }
    }

    fn add(&mut self, w: usize, v: f32) {
        if let Some(s) = self.buf.get_mut(w & self.mask) {
            *s += v;
        }
    }

    /// `(1 − f)·x[w − i] + f·x[w − i − 1]`, `d = i + f`.
    fn weighted(&self, w: usize, d: f32) -> f32 {
        let i = d as usize;
        let f = d - i as f32;
        (1.0 - f) * self.at(w, i) + f * self.at(w, i + 1)
    }
}

struct Line {
    ring: Ring,
    /// Read delay before modulation, and the swing; `sin` picks the
    /// phasor's sine over its cosine.
    base: f32,
    depth: f32,
    sin: bool,
    /// Damping `v = a·read + b·v`, and `v`.
    a: f32,
    b: f32,
    v: f32,
    /// The input channel it takes, and the gain.
    right: bool,
    inject: f32,
}

/// Three delays coupled by a fixed matrix: an allpass of gain `p`.
struct Unit {
    rings: [Ring; 3],
    d: [usize; 3],
    z: [f32; 3],
    m: [f32; 9],
    b: [f32; 3],
    q: f32,
    r: f32,
}

impl Unit {
    fn tick(&mut self, w: usize, x: f32) -> f32 {
        let [z0, z1, z2] = self.z;
        let y = x * self.q + z0 * self.r;
        let [m0, m1, m2, m3, m4, m5, m6, m7, m8] = self.m;
        let [b0, b1, b2] = self.b;
        let cols = [[m0, m3, m6, b0], [m1, m4, m7, b1], [m2, m5, m8, b2]];
        let next = self.rings.iter_mut().zip(self.d).zip(cols);
        for (z, ((ring, d), [a, b, c, e])) in self.z.iter_mut().zip(next) {
            ring.put(w, flush(a * z0 + b * z1 + c * z2 + e * x));
            *z = ring.at(w.wrapping_add(1), d);
        }
        y
    }
}

pub struct Network {
    law: &'static ReverbNetwork,
    fade: Fade,
    /// Prime table up to the longest line.
    primes: Box<[u32]>,
    lines: Box<[Line]>,
    /// Feedback through a 32-point Walsh–Hadamard: `col` places line `j`,
    /// `row` reads line `i`; `u = mean·Σv + half·H(v)`.
    row: [u8; LINES],
    col: [u8; LINES],
    mean: f32,
    half: f32,
    inject_offset: usize,
    pre: [Ring; 2],
    predelay: [f32; 2],
    taps: [[(u8, f32); 8]; 2],
    units: [[Unit; 4]; 2],
    unit_count: usize,
    shelf_m: [f32; 3],
    shelf_a: [f32; 3],
    shelf: [[f32; 2]; 2],
    cut: f32,
    cuts: [f32; 2],
    width: f32,
    wet: f32,
    phasor: (f64, f64),
    rot: (f64, f64),
    w: usize,
}

impl Network {
    /// Not real-time: allocates the lines for the largest size and predelay.
    #[must_use]
    pub fn new(law: &'static ReverbNetwork, s: Settings, rate: u32) -> Self {
        let host = rate.max(1) as f32;
        let primes = primes(law, MAX_SIZE);
        let longest = lengths(law, &primes, MAX_SIZE);
        let lines = longest
            .iter()
            .map(|&len| Line {
                ring: Ring::new(len as usize),
                base: 0.0,
                depth: 0.0,
                sin: false,
                a: 0.0,
                b: 0.0,
                v: 0.0,
                right: false,
                inject: 0.0,
            })
            .collect();
        let spread = longest.iter().max().copied().unwrap_or(0) as f32;
        let pre = MAX_PREDELAY_MS * 0.001 * law.rate + spread + 2.0;
        let unit = |d: [u16; 3]| Unit {
            rings: d.map(|n| Ring::new(usize::from(n) + 1)),
            d: d.map(usize::from),
            z: [0.0; 3],
            m: [0.0; 9],
            b: [0.0; 3],
            q: 0.0,
            r: 0.0,
        };
        let (row, col) = hadamard_maps(law.sequence);
        // ±1 entries scaled to an orthogonal matrix: `a` where the sequence
        // is 0, `b` where it is 1.
        let d = 2.0 / 32f64.sqrt();
        let a = (16.0 * d - 1.0) / 31.0;
        let (a, b) = (a as f32, (a - d) as f32);
        let w = std::f64::consts::TAU * f64::from(s.modulation_hz.max(0.0)) / f64::from(host);
        let mut n = Self {
            law,
            fade: Fade::new(s, host),
            primes,
            lines,
            row,
            col,
            mean: (a + b) * 0.5,
            half: (a - b) * 0.5,
            inject_offset: 0,
            pre: [Ring::new(pre as usize), Ring::new(pre as usize)],
            predelay: [0.0; 2],
            taps: [[(0, 0.0); 8]; 2],
            units: [law.units.map(unit), law.units.map(unit)],
            unit_count: usize::from(law.unit_count).min(4),
            shelf_m: [1.0, 0.0, 0.0],
            shelf_a: [0.0; 3],
            shelf: [[0.0; 2]; 2],
            cut: 0.0,
            cuts: [0.0; 2],
            width: 1.0,
            wet: 0.0,
            phasor: (1.0, 0.0),
            rot: (w.cos(), w.sin()),
            w: 0,
        };
        n.apply(s);
        n
    }

    /// Changes every setting while it plays, by the [`Fade`] rule. Never allocates.
    pub fn set(&mut self, s: Settings) {
        if let Some(s) = self.fade.set(s) {
            self.apply(s);
        }
    }

    /// Writes `s` into the lines and coefficients. Never allocates.
    fn apply(&mut self, s: Settings) {
        let law = self.law;
        let rate = law.rate;
        let len = lengths(law, &self.primes, s.size.clamp(0.0, MAX_SIZE));
        let flen = len.map(|l| l as f32);
        // Decay: 60 dB in `t` (frames at `rate`), the highs in `t_hf`.
        let t = s.seconds.max(0.01);
        let [h0, h1, h2] = law.hf;
        let damp = s.damping.clamp(0.0, 1.0);
        let l = (t + t).ln();
        let t_hf = ((1.0 - damp * h0) * (1.0 - l * h2) + (1.0 - damp * h1) * l * h2) * t;
        let k = -3.0 / rate;
        let base = 10f32.powf(k / t);
        let ratio = 10f32.powf(k / t_hf) / base;
        let depth = s.modulation_ms.max(0.0) * 0.001 * rate;
        let [len0, len1] = [len[0], len[1]];
        for (i, (line, f)) in self.lines.iter_mut().zip(flen).enumerate() {
            let p = ratio.powf(f);
            let r = 1.0 / (p * p);
            // The one-pole whose highs fall `r` times the lows per pass.
            let a = if r == 1.0 {
                0.0
            } else {
                ((r * 2.962_758 - 0.768_274_2).sqrt() - (r + 0.481_379_06)) / (1.0 - r)
            };
            line.base = f;
            line.depth = if i == 0 { 0.0 } else { depth };
            line.sin = i % 2 == 0;
            (line.a, line.b) = ((1.0 - a) * base.powf(f), a);
            (line.right, line.inject) = (false, 0.0);
        }
        for (ch, taps) in law.inputs.iter().enumerate() {
            for &(i, sign) in taps.iter().filter(|t| t.1 != 0.0) {
                if let Some(line) = self.lines.get_mut(usize::from(i)) {
                    (line.right, line.inject) = (ch == 1, sign);
                }
            }
        }
        self.inject_offset = len0.saturating_sub(law.inject_lead) as usize;
        let g = s.diffusion.clamp(0.0, 1.0) * 0.5;
        let w = g + g;
        for (out, taps) in self.taps.iter_mut().zip(law.taps) {
            *out = taps.map(|(i, near, far)| {
                (
                    i,
                    if near == far {
                        near
                    } else {
                        w * far + (1.0 - w) * near
                    },
                )
            });
        }
        let c = law.unit_feedback;
        for chain in &mut self.units {
            for (k, u) in chain.iter_mut().enumerate() {
                let p = if k % 2 == 0 { -g } else { g };
                let (cp, cp2) = (c * p, p * c * p);
                u.m = [-cp2, -cp2, cp, 1.0 - cp2, -cp2, cp, 0.0, 1.0 - cp2, cp];
                (u.b, u.q, u.r) = ([-p, -p, 1.0], -p, 1.0 - p * c * p);
            }
        }
        let pd = s.predelay_ms.clamp(0.0, MAX_PREDELAY_MS) * 0.001 * rate;
        self.predelay = [pd + (i64::from(len1) - i64::from(len0)) as f32, pd];
        (self.shelf_m, self.shelf_a) = shelf(
            s.low_shelf_db,
            (f64::from(law.shelf_hz) * PI / f64::from(rate)) as f32,
        );
        self.cut = (f64::from(s.high_cut_hz * -2.0) * (PI / f64::from(rate))).exp() as f32;
        self.width = s.stereo.clamp(0.0, 1.0);
        let [l0, l1] = law.level;
        self.wet = (1.0 - 10f32.powf(-l0 / t)).sqrt() * l1;
    }

    /// Replaces `io` with the reverb (the wet path only).
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        for frame in io {
            let (gain, pending) = self.fade.tick();
            if let Some(s) = pending {
                self.apply(s);
            }
            *frame = self.frame(*frame).map(|v| v * gain);
        }
    }

    fn frame(&mut self, input: [f32; 2]) -> [f32; 2] {
        let w = self.w;
        self.w = w.wrapping_add(1);
        let (c, s) = self.phasor;
        let (cw, sw) = self.rot;
        self.phasor = (c * cw - sw * s, c * sw + s * cw);
        let lfo = [self.phasor.0 as f32, self.phasor.1 as f32];
        let mut x = [0.0f32; 2];
        for ((x, ring), (v, d)) in x
            .iter_mut()
            .zip(&mut self.pre)
            .zip(input.into_iter().zip(self.predelay))
        {
            ring.put(w, v);
            *x = ring.weighted(w, d);
        }
        // Read and damp every line, then feed the matrix back.
        let mut t = [0f32; 32];
        let mut sum = 0.0;
        for (line, &col) in self.lines.iter_mut().zip(&self.col) {
            let swing = if line.sin { lfo[1] } else { lfo[0] };
            let d = swing * line.depth + line.base;
            let read = line.ring.weighted(w, d);
            line.v = read * line.a + line.b * line.v;
            if let Some(slot) = t.get_mut(usize::from(col) & 31) {
                *slot = line.v;
            }
            sum += line.v;
        }
        hadamard(&mut t);
        let m = self.mean * sum;
        let at = w.wrapping_sub(self.inject_offset);
        for (line, &row) in self.lines.iter_mut().zip(&self.row) {
            let u = m + self.half * t.get(usize::from(row) & 31).copied().unwrap_or(0.0);
            line.ring.put(w, flush(u));
            line.ring
                .add(at, if line.right { x[1] } else { x[0] } * line.inject);
        }
        let mut o = self.taps.map(|taps| {
            taps.iter().fold(0f32, |acc, &(i, c)| {
                acc + c * self.lines.get(usize::from(i)).map_or(0.0, |l| l.v)
            })
        });
        for (o, chain) in o.iter_mut().zip(&mut self.units) {
            for u in chain.iter_mut().take(self.unit_count) {
                *o = u.tick(w, *o);
            }
        }
        self.post(o)
    }

    /// Low shelf, high cut, width and level.
    fn post(&mut self, mut o: [f32; 2]) -> [f32; 2] {
        let (m, a) = (self.shelf_m, self.shelf_a);
        for ((o, st), cut) in o.iter_mut().zip(&mut self.shelf).zip(&mut self.cuts) {
            let [ic1, ic2] = *st;
            let v3 = *o - ic2;
            let v1 = v3 * a[1] + ic1 * a[0];
            let v2 = a[1] * ic1 + a[2] * v3 + ic2;
            *st = [flush(2.0 * v1 - ic1), flush(2.0 * v2 - ic2)];
            *o = v1 * m[1] + *o * m[0] + v2 * m[2];
            *cut = flush((*cut - *o) * self.cut + *o);
            *o = *cut;
        }
        let mid = (o[0] + o[1]) * 0.5;
        o.map(|v| ((v - mid) * self.width + mid) * self.wet)
    }
}

/// The first primes, enough for every line at `size`. Not real-time.
fn primes(law: &ReverbNetwork, size: f32) -> Box<[u32]> {
    let n = law
        .growth
        .iter()
        .zip(law.offset)
        .map(|(g, o)| (g * size + o).max(0.0) as usize + 1)
        .max()
        .unwrap_or(1);
    let mut out = Vec::with_capacity(n);
    let mut k = 2u32;
    while out.len() < n {
        if out
            .iter()
            .take_while(|&&p| p * p <= k)
            .all(|&p| !k.is_multiple_of(p))
        {
            out.push(k);
        }
        k += 1;
    }
    out.into()
}

/// Line lengths in frames at `size`.
fn lengths(law: &ReverbNetwork, primes: &[u32], size: f32) -> [u32; LINES] {
    std::array::from_fn(|i| {
        let (g, o) = (
            law.growth.get(i).copied().unwrap_or(0.0),
            law.offset.get(i).copied().unwrap_or(0.0),
        );
        let at = (g * size + o).max(0.0) as usize;
        primes.get(at).or(primes.last()).copied().unwrap_or(2)
    })
}

/// Index maps turning `S[i][j] = (−1)^c[(j−i) mod 31]` into a 32-point
/// Walsh–Hadamard transform: `S[i][j] = (−1)^popcount(row[i] & col[j])`.
/// A sequence that is not an m-sequence finds no row and reads slot 0.
fn hadamard_maps(c: [u8; LINES]) -> ([u8; LINES], [u8; LINES]) {
    let at = |k: usize| c.get(k % LINES).copied().unwrap_or(0);
    let col: [u8; LINES] = std::array::from_fn(|j| (0..5).map(|k| at(j + k) << k).sum());
    let row = std::array::from_fn(|i| {
        (1..32u8)
            .find(|&r| {
                col.iter()
                    .enumerate()
                    .all(|(j, &cj)| ((r & cj).count_ones() & 1) as u8 == at(j + LINES - i))
            })
            .unwrap_or(0)
    });
    (row, col)
}

/// In-place unnormalised 32-point Walsh–Hadamard transform.
fn hadamard(x: &mut [f32; 32]) {
    for h in [1, 2, 4, 8, 16] {
        for pair in x.chunks_exact_mut(2 * h) {
            let (lo, hi) = pair.split_at_mut(h);
            for (a, b) in lo.iter_mut().zip(hi) {
                (*a, *b) = (*a + *b, *a - *b);
            }
        }
    }
}

/// The trapezoidal state-variable low shelf at `db`, Q 1, `f = π·fc/rate`:
/// output mix `m` and coefficients `a`.
fn shelf(db: f32, f: f32) -> ([f32; 3], [f32; 3]) {
    let gain = 10f32.powf(db * 0.025);
    let f3 = f * f * f;
    let g = (f3 * 0.333_333_34 + f + f3 * f * f * 0.133_333_34) * (1.0 / gain.sqrt());
    let a1 = 1.0 / ((g + 1.0) * g + 1.0);
    let a2 = g * a1;
    ([1.0, gain - 1.0, gain * gain - 1.0], [a1, a2, g * a2])
}
