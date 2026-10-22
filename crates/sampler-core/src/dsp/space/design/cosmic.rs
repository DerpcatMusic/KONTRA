//! Kontakt Raum Cosmic (type 2) on the chain bank (flag 0): eight serial
//! two-line stages whose delay positions two complex oscillators steer. It
//! runs as [`Chain::Cosmic`](super::super::network::Chain) in the kit; the
//! Raum outer ([`super::raum`]) is unchanged. Spec:
//! `~/.cache/k-reverbs-oracle/findings/cosmic.md`.
//!
//! Per stage and frame, with `y` the two lines' reads and `x` the stage
//! input: `A = M y + Y x` is written to the lines, the output is `Z y + g0 x`
//! (CONFIRMED, findings §6–7). With `Z = s I`, `g0 = c`, `M = -c R`, `Y = s R`
//! this is `(c + z⁻ᴺ)/(1 + c z⁻ᴺ)` per channel, rotated across the pair.
//! Labels in the comments: CONFIRMED as the findings state them; INFERRED
//! where they are silent (chosen like the kit's Airy LFO).

use super::super::network::{Ring, lcg_uniform};
use std::f64::consts::TAU;

/// Lanes: `Re·A` and `Im·A` of oscillator 1 and 2 (findings §4).
const U1: usize = 0;
const V1: usize = 1;
const U2: usize = 2;
const V2: usize = 3;

/// The odd polynomial `P(x) = C1 x + C2 x³ + C3 x⁵ + C4 x⁷ + C5 x⁹` (§9, bits).
const POLY: [f32; 5] = [f32::from_bits(0x3f42_6d3d), f32::from_bits(0x3e01_9e29), f32::from_bits(0x3d69_4fe3), f32::from_bits(0x3d0a_e050), f32::from_bits(0x3cbd_06a6)];
/// The second oscillator's frequency ratio, `1.42857003` (§4, §9).
const RATIO2: f32 = f32::from_bits(0x3fb6_db62);
/// Chain ring lengths `[L, R]` in samples (§8, fixed, not rate-dependent).
const LENGTHS: [[usize; 2]; 8] = [[577, 587], [653, 643], [811, 821], [1103, 1087], [1823, 1861], [2297, 2293], [2791, 2797], [3739, 3733]];
/// Per stage and channel, the lane and sign of the delay ratio `B ± P(lane)`
/// (§4, CONFIRMED).
const RATIOS: [[(usize, f32); 2]; 8] = [
    [(V1, 1.0), (U1, 1.0)],
    [(V1, -1.0), (U1, -1.0)],
    [(U1, 1.0), (V1, 1.0)],
    [(U1, -1.0), (V1, -1.0)],
    [(V2, 1.0), (U2, 1.0)],
    [(V2, -1.0), (U2, -1.0)],
    [(U2, 1.0), (V2, 1.0)],
    [(U2, -1.0), (V2, -1.0)],
];

/// The stage rotation `th_k` (§7): +0.05 for even k up to 6, -0.05 for odd k up
/// to 5, -0.02 for 7.
fn theta(k: usize) -> f64 {
    match k {
        7 => -0.02,
        k if k % 2 == 0 => 0.05,
        _ => -0.05,
    }
}

/// The lane shaping `P` (§4).
fn poly(x: f32) -> f32 {
    let x2 = x * x;
    x * (POLY[0] + x2 * (POLY[1] + x2 * (POLY[2] + x2 * (POLY[3] + x2 * POLY[4]))))
}

/// A rotating unit phasor. The sine is flippable (§2, §5).
#[derive(Clone, Copy)]
struct Osc {
    re: f64,
    im: f64,
    c: f64,
    s: f64,
    theta: f64,
}

impl Osc {
    /// Start at `(1, 0)` (INFERRED, as the kit's Airy phasor).
    fn new(theta: f64) -> Self {
        Self { re: 1.0, im: 0.0, c: theta.cos(), s: theta.sin(), theta }
    }
}

#[derive(Clone)]
struct Stage {
    rings: [Ring; 2],
    len: [usize; 2],
    tap: [(usize, f32); 2],
    /// `M` and `Y`, row-major; `Z = z·I`; `g0`.
    m: [f32; 4],
    y: [f32; 4],
    z: f32,
    g0: f32,
}

impl Stage {
    /// Stage `k` at `c = cos(1.17 v)` and `s = sin(1.17 v)` (§3, CONFIRMED).
    fn new(k: usize, c: f32, s: f32) -> Self {
        let th = theta(k);
        let (cc, ss) = (th.cos() as f32, th.sin() as f32);
        // R(th) = [[C, -S], [S, C]]; M = -c R; Y = s R; Z = s I; g0 = c (§7).
        let r = [cc, -ss, ss, cc];
        Self {
            rings: LENGTHS[k].map(Ring::new),
            len: LENGTHS[k],
            tap: RATIOS[k],
            m: r.map(|v| -c * v),
            y: r.map(|v| s * v),
            z: s,
            g0: c,
        }
    }

    #[inline(always)]
    fn tick(&mut self, w: usize, x: [f32; 2], p: &[f32; 4], base: f32) -> [f32; 2] {
        let mut d = [0f32; 2];
        for (ch, (lane, sign)) in self.tap.into_iter().enumerate() {
            // Positions `N·r` read at `N·r - 1` frames back (the interpolation
            // point of the taps `w+2-u .. w-u-1` of §6); the newest written
            // sample is `N·r - 2` behind.
            let ratio = base + sign * p[lane];
            d[ch] = self.rings[ch].catmull_rom(w, self.len[ch] as f32 * ratio - 1.0);
        }
        let [m, y] = [self.m, self.y];
        let a = [m[0] * d[0] + m[1] * d[1] + y[0] * x[0] + y[1] * x[1], m[2] * d[0] + m[3] * d[1] + y[2] * x[0] + y[3] * x[1]];
        self.rings[0].put(w, a[0]);
        self.rings[1].put(w, a[1]);
        [self.z * d[0] + self.g0 * x[0], self.z * d[1] + self.g0 * x[1]]
    }
}

/// Cosmic's chain bank: the LFO, the sign flips and eight stages.
#[derive(Clone)]
pub(in super::super) struct Cosmic {
    osc: [Osc; 2],
    /// `B = S - A` (the base ratio) and `A` (the depth) (§4).
    base: f32,
    depth: f64,
    /// Flip period `int(sr)` and its down-counter (§2, §5).
    period: u32,
    count: u32,
    lcg: u32,
    stages: [Stage; 8],
}

impl Cosmic {
    /// From the Raum core controls: `decay` (core 10, `v`), `size` (core 11),
    /// `modulation` (core 13) and `rate` (core 14, 0..1), at `sr` frames a
    /// second. Slot-to-name mapping is INFERRED from the core order.
    pub(in super::super) fn new(decay: f32, size: f32, modulation: f32, rate: f32, sr: u32) -> Self {
        // Decay (§3): g = π/2 - 1.17 v, so c = sin g = cos(1.17 v), s = cos g = sin(1.17 v).
        let g = 1.17 * f64::from(decay);
        let (c, s) = (g.cos() as f32, g.sin() as f32);
        // Size S = 0.05 + 0.95 v² (§3); rate R = 0.01 + 34.99 v / (1 + 23 (1 - v)) Hz (§3).
        let size = 0.05 + 0.95 * f64::from(size) * f64::from(size);
        let rate = f64::from(rate);
        let hz = 0.01 + 34.99 * rate / (1.0 + 23.0 * (1.0 - rate));
        // Depth A = min(0.47 S, 0.02) m / √max(R, 1); base B = S - A (§4).
        let depth = (0.47 * size).min(0.02) * f64::from(modulation) / hz.max(1.0).sqrt();
        let sr_f = f64::from(sr.max(1));
        let thetas = [TAU * hz / sr_f, TAU * f64::from(RATIO2) * hz / sr_f];
        let period = sr.max(1);
        Self {
            osc: thetas.map(Osc::new),
            base: (size - depth) as f32,
            depth,
            period,
            count: period,
            lcg: 0,
            stages: std::array::from_fn(|k| Stage::new(k, c, s)),
        }
    }

    /// One frame through the eight stages in series.
    #[inline(always)]
    pub(in super::super) fn tick(&mut self, w: usize, x: [f32; 2]) -> [f32; 2] {
        self.count -= 1;
        if self.count == 0 {
            self.count = self.period;
            // Every `int(sr)` frames each oscillator's sine is negated with
            // probability 0.25 (§5, CONFIRMED). INFERRED: oscillator 1 draws
            // first, and the counter starts full.
            for o in &mut self.osc {
                if lcg_uniform(&mut self.lcg) < 0.25 {
                    o.s = -o.s;
                }
            }
        }
        // Rotation `(Re C - Im S, Re S + Im C)` before the read (§4, CONFIRMED;
        // the read-after-step order is INFERRED).
        for o in &mut self.osc {
            (o.re, o.im) = (o.re * o.c - o.im * o.s, o.re * o.s + o.im * o.c);
        }
        let [a, b] = self.osc;
        let depth = self.depth;
        let p = [a.re, a.im, b.re, b.im].map(|v| poly((v * depth) as f32));
        let base = self.base;
        self.stages.iter_mut().fold(x, |x, st| st.tick(w, x, &p, base))
    }

    /// The initial state: phasors at `(1, 0)`, empty lines, LCG seed 0 (INFERRED).
    pub(in super::super) fn clear(&mut self) {
        for o in &mut self.osc {
            *o = Osc::new(o.theta);
        }
        (self.count, self.lcg) = (self.period, 0);
        for st in &mut self.stages {
            st.rings.iter_mut().for_each(|r| r.buf.fill(0.0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §8: stage lengths and ring sizes (next power of two).
    #[test]
    fn chain_lengths_and_ring_sizes() {
        let c = Cosmic::new(0.8, 0.5, 0.5, 0.25, 48_000);
        let got: Vec<([usize; 2], [usize; 2])> = c.stages.iter().map(|s| (s.len, s.rings.each_ref().map(|r| r.buf.len()))).collect();
        let want = [([577, 587], [1024, 1024]), ([653, 643], [1024; 2]), ([811, 821], [1024; 2]), ([1103, 1087], [2048; 2]), ([1823, 1861], [2048; 2]), ([2297, 2293], [4096; 2]), ([2791, 2797], [4096; 2]), ([3739, 3733], [4096; 2])];
        assert_eq!(got, want);
    }

    /// Stage 0 (th = +0.05) at `1.17 v = π/4`, so `c = s = √½`: the §7 formulas
    /// `M = -c R`, `Y = s R`, `Z = s I`, `g0 = c` with `R = [[C, -S], [S, C]]`.
    #[test]
    fn stage_zero_matrices_match_the_formulas() {
        let v = std::f32::consts::FRAC_PI_4 / 1.17;
        let c = Cosmic::new(v, 0.5, 0.0, 0.25, 48_000);
        let st = &c.stages[0];
        let (cc, ss) = (0.05f64.cos(), 0.05f64.sin());
        let k = std::f64::consts::FRAC_1_SQRT_2;
        let want_m = [-k * cc, k * ss, -k * ss, -k * cc];
        let want_y = [k * cc, -k * ss, k * ss, k * cc];
        for (got, want) in st.m.iter().zip(want_m).chain(st.y.iter().zip(want_y)) {
            assert!((f64::from(*got) - want).abs() < 1e-6, "{got} {want}");
        }
        assert!((f64::from(st.z) - k).abs() < 1e-6 && (f64::from(st.g0) - k).abs() < 1e-6, "{} {}", st.z, st.g0);
    }
}
