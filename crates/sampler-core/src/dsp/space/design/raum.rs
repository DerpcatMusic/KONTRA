//! Kontakt Raum (0x65) reverb kernel `GaloisReverbExtended` (Kontakt 8.13.1
//! setter `0x14198aa20`, process `0x1419878b0`; Grounded at core `+0x1fc0`,
//! Airy at `+0x4400`):
//! - decay `T = 199.75x/(75(1-x) + 1) + 0.25` s; lengths (`0x14198ac40`, fixed
//!   samples) Grounded `prime((2 + ⌊99x⌋)·b_i + 40)`, Airy
//!   `prime((s·b_i)² div 30s + 75)`, `s = 10 + ⌊80x⌋`.
//! - gain `0x141985e30`: `√(1 - 10^(-4/min(T, 20)))·(a√size + b)`.
//! - damping `0x141985cf0`/`0x141989f50`: high-band time `Th`, `A = 10^(-3/(sr·T))`,
//!   `B = 10^(-3/(sr·Th))/A`; per line `h = max(B^L, 0.1)`, `r = 1/h²`,
//!   `c = (√(2.962758r - 0.7682742) - (r + 0.48137906))/(1 - r)`, `y = c·y + A^L(1-c)·r`.
//! - modulation: two f64 phasors at `1.5x + 1` Hz and `×1.0457125`, depth
//!   `(1 - (x-1)²)·32·(1.5x + 1)` frames, lines `4+2q`/`5+2q` `±m[q mod 4]`,
//!   28/29 `±m₀`; Airy shapes `m` by `P` and reverses phasors at random once a
//!   second.
//! - left input delayed `L₁ - L₀`, injected `L₀ - 80` behind the write with
//!   ±1 weights; matrix `s_j = Σ_k row₀[(j-k) mod 31]·v_k` (`+0.150221`,
//!   `-0.203332`; sparse: `-0.3`, `0.91·0.3^(k-1)`).
//! - out: signed taps of the damped reads (Airy blends `T3`/`T4` by
//!   diffusion) into 4 (Grounded) or 2 (Airy) coupled units, angle
//!   `±0.35x`/`±0.2625x`, cross rotation 0.01; `√(1-mix)·in + √mix·G·wet`.

use super::super::network::{Coupled, Interp, LineSpec, Matrix, Modulation, Ring, Spec, TapFrom};
use super::galois_law::{hadamard_maps_of, matrix_ab, primes};

const LINES: usize = 31;
const GROUNDED_B: [u32; LINES] = [0, 1, 10, 17, 28, 23, 6, 7, 8, 9, 2, 11, 12, 13, 14, 15, 16, 3, 18, 19, 20, 21, 22, 5, 24, 25, 26, 27, 4, 29, 30];
const AIRY_B: [u32; LINES] = [0, 1, 4, 3, 2, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30];
const ROW0: &str = "-+++-----++--+-++-++++-+-+---+-";
type W = [i8; LINES];
const G_WL: W = [1, 0, 1, 0, 1, 0, -1, 0, 0, 0, -1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, -1, 0, 0, 0, -1, 0, -1, 0, 0];
const G_WR: W = [0, 1, 0, 1, 0, 1, 0, -1, 0, 0, 0, -1, 0, 0, 0, -1, 0, 0, 0, -1, 0, 0, 0, -1, 0, 0, 0, 1, 0, 1, 0];
const A_WL: W = [1, 0, 0, 1, -1, 0, 0, 0, -1, 0, 0, 0, 0, -1, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, -1, 0, 0];
const A_WR: W = [0, 1, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, -1, 0, 0, 0, -1, 0, -1, 0, 0, -1, 0, 0, 0, 0, 0, 1, 0];
const G_TL: W = [1, 0, 0, -1, 1, 0, 0, 0, -1, 0, 0, 0, -1, 0, 0, 0, 1, 0, 0, 0, -1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];
const G_TR: W = [0, 1, -1, 0, 0, 1, 0, 0, 0, -1, 0, 0, 0, -1, 0, 0, 0, 1, 0, 0, 0, -1, 0, 0, 0, 1, 0, 0, 0, 0, 0];
const A_TL: W = [1, 0, 1, 0, 0, 0, 0, -1, 0, 0, 0, 0, 1, 0, 0, -1, 0, 0, 0, 0, 0, -1, 0, 0, 0, -1, 0, 0, 0, 0, 1];
const A_TR: W = [0, 1, 0, 1, 0, 0, -1, 0, 0, 0, 1, -1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, -1, 0, 0, 0, 0, -1, 0, 0, 0];
const UNITS: [[[usize; 3]; 4]; 2] = [[[31, 79, 41], [43, 73, 53], [59, 61, 67], [71, 47, 37]], [[73, 181, 97], [103, 179, 127], [0; 3], [0; 3]]];

/// The kernel's controls (Raum core indices in brackets).
#[derive(Clone, Copy, Debug)]
pub(in super::super) struct Kernel {
    pub airy: bool,
    pub decay: f32,     // 10
    pub size: f32,      // 11
    pub damping: f32,   // 12
    pub modulation: f32, // 13
    pub diffusion: f32, // 15
    pub freeze: bool,   // 16
    pub reverb: f32,    // 17
    pub sparse: bool,   // 18
}

pub(super) fn lengths(airy: bool, size: f32) -> [usize; LINES] {
    let p = primes(3100);
    std::array::from_fn(|i| {
        let k = if airy {
            let s = 10 + (80.0 * size) as u32;
            (s * AIRY_B[i]).pow(2) / (30 * s) + 75
        } else {
            (2 + (99.0 * size) as u32) * GROUNDED_B[i] + 40
        };
        p[k as usize] as usize
    })
}

/// `T`, `G`, and per line `(b, c)` of `y = c·y + b·r`.
pub(super) fn decay(k: &Kernel, len: &[usize; LINES], rate: f64) -> (f64, f64, Vec<(f64, f64)>) {
    let x = f64::from(k.decay);
    let t = (199.75 * x / (75.0 * (1.0 - x) + 1.0) + 0.25).clamp(0.25, 200.0);
    let (a, b) = if k.airy { (0.105, 0.245) } else { (0.2275, 0.13) };
    let g = (1.0 - 10f64.powf(-4.0 / t.min(20.0))).sqrt() * (a * f64::from(k.size).sqrt() + b);
    let d = f64::from(k.damping);
    let m = (t / 2.0).min(2.0);
    let th = if d <= 0.5 { 2.0 * d * (m - t) + t } else { (2.0 * d - 1.0) * ((0.1 * t).min(0.25) - m) + m };
    // A, B and the per-line laws are f32 (state dump: A 0.99998593, B
    // 0.9999905; line 0 c 0.002454229 needs the f32 cancellation).
    let big_a = 10f64.powf(-3.0 / (rate * t)) as f32;
    let big_b = 10f64.powf(-3.0 / (rate * th)) as f32 / big_a;
    let lines = len
        .iter()
        .map(|&l| {
            let h = big_b.powf(l as f32).max(0.1);
            let r = 1.0 / (h * h);
            let c = if r == 1.0 { 0.0 } else { ((2.962_758 * r - 0.768_274_2).sqrt() - (r + 0.481_379_06)) / (1.0 - r) };
            (f64::from(big_a.powf(l as f32) * (1.0 - c)), f64::from(c))
        })
        .collect();
    (t, g, lines)
}

/// The output tap weights `(L, R)` of line `i`.
fn taps(k: &Kernel) -> [[f32; LINES]; 2] {
    if !k.airy {
        return [G_TL, G_TR].map(|t| t.map(f32::from));
    }
    let x = k.diffusion;
    let (near, far) = (0.3f32.sqrt(), (37.0f32 / 30.0).sqrt());
    [(A_TL, [0, 2]), (A_TR, [1, 3])].map(|(t3, near_lines)| {
        std::array::from_fn(|i| {
            let s = f32::from(t3[i]);
            let t4 = s * if near_lines.contains(&i) { near } else { far };
            x * t4 + (1.0 - x) * s
        })
    })
}

pub(in super::super) fn kernel(k: &Kernel, rate: f64) -> Spec {
    let len = lengths(k.airy, k.size);
    let (_, g, damp) = decay(k, &len, rate);
    let x = f64::from(k.modulation);
    let depth = ((1.0 - (x - 1.0).powi(2)) * 32.0 * (1.5 * x + 1.0)) as f32 * if k.freeze { 0.0 } else { 1.0 };
    let (wl, wr) = if k.airy { (A_WL, A_WR) } else { (G_WL, G_WR) };
    let lines = (0..LINES)
        .map(|i| {
            let mut l = LineSpec::new(len[i] + depth as usize + 4, len[i] as f64 + 1.0);
            let (b, c) = damp[i];
            (l.a, l.b) = (b as f32, c as f32);
            let (sel, sign) = match i {
                4..=27 => (((i - 4) / 2) % 4, if i % 2 == 0 { 1.0 } else { -1.0 }),
                28 => (0, 1.0),
                29 => (0, -1.0),
                _ => (0, 0.0),
            };
            l.sel = sel as u8;
            l.depth = f64::from(sign * depth);
            if !k.freeze {
                l.inject = if wl[i] != 0 { (0, f32::from(wl[i])) } else { (1, f32::from(wr[i])) };
            }
            l
        })
        .collect();
    let (pa, pb) = matrix_ab();
    let matrix = if k.sparse {
        // `m₀ = -0.3`, `m_{31-k} = 0.91·0.3^(k-1)` (fitted to the native table).
        let row: Vec<f32> = (0..LINES).map(|j| if j == 0 { -0.3 } else { 0.91 * 0.3f32.powi(LINES as i32 - j as i32 - 1) }).collect();
        Matrix::Dense((0..LINES * LINES).map(|q| row[(q / LINES + LINES - q % LINES) % LINES]).collect())
    } else {
        // `D[i][j] = row₀[(i-j) mod 31]` is the m-sequence circulant of
        // `c[n] = row₀[-n mod 31] == '-'`.
        let c: [u8; LINES] = std::array::from_fn(|n| u8::from(ROW0.as_bytes()[(LINES - n) % LINES] == b'-'));
        let (row, col) = hadamard_maps_of(c);
        Matrix::Circulant { row, col, mean: (pa + pb) * 0.5, half: (pa - pb) * 0.5 }
    };
    let mut s = Spec::new(lines, matrix);
    s.interp = Interp::Linear;
    let f = 1.5 * x + 1.0;
    let w = std::f64::consts::TAU * f / rate;
    s.modulation = Modulation::Dual { w: [w, w * f64::from(1.045_712_5f32)], shape: k.airy, reverse: k.airy.then_some(rate as u32) };
    s.predelay = [(len[1] - len[0]) as f32, 0.0];
    s.pre_cap = len[1] - len[0] + 2;
    s.inject_offset = len[0] - 79;
    s.tap_from = TapFrom::Filtered;
    s.freeze = k.freeze;
    let t = taps(k);
    s.taps = t.map(|t| (0..LINES).filter(|&i| t[i] != 0.0).map(|i| (i as u8, t[i])).collect());
    let theta = 0.5 * k.diffusion * 0.7 * if k.airy { 0.75 } else { 1.0 };
    let units = if k.airy { 2 } else { 4 };
    let unit = |th: f32, n: [usize; 3]| {
        let (sn, c) = th.sin_cos();
        Coupled {
            a: [-sn * sn, -c * sn * sn, sn * c * c, c, -sn * sn, sn * c, 0.0, c, sn],
            b: [-sn * c, -sn * c * c, c * c * c],
            c,
            d: -sn,
            rot: (0.01f32.cos(), 0.01f32.sin()),
            n,
            u: [[0.0; 3]; 2],
            rings: [0, 1].map(|_| n.map(|n| Ring::new(n + 1))),
        }
    };
    s.chain = super::super::network::Chain::Coupled((0..units).map(|u| unit(if u % 2 == 0 { theta } else { -theta }, UNITS[usize::from(k.airy)][u])).collect());
    let mix = f64::from(k.reverb);
    s.width = Some(1.0);
    s.wet = (mix.sqrt() * g) as f32;
    s.dry = (1.0 - mix).sqrt() as f32;
    s
}

/// The Raum `Spec` for core controls `p` (`Topology::Raum`); Cosmic (type 2)
/// is not built.
pub(in super::super) fn spec(p: &[f32; 20], rate: u32) -> Result<Spec, crate::Error> {
    let kind = ((3.0 * p[9]) as u32).min(2);
    if kind == 2 {
        return Err(crate::Error::InvalidInput);
    }
    let k = Kernel { airy: kind == 1, decay: p[10], size: p[11], damping: p[12], modulation: p[13], diffusion: p[15], freeze: p[16] >= 0.5, reverb: p[17], sparse: p[18] >= 0.5 };
    Ok(kernel(&k, f64::from(rate)))
}

/// `fexp2` of the Raum low shelf (`0x141982300`): nearest-even split,
/// `1 + 0.70588f + 0.23529f²`.
fn fexp2(y: f32) -> f32 {
    let i = y.round_ties_even() as i32;
    let f = y - i as f32;
    ((f32::from_bits(0x3e70_f0f1) * f + f32::from_bits(0x3f34_b4b5)) * f + 1.0) * f32::from_bits((i.wrapping_add(127) as u32) << 23)
}

/// `t + t³/3 + 2t⁵/15` (`0x3eaaaaab`, `0x3e088889`), no wrap above Nyquist.
fn tanpoly(t: f32) -> f32 {
    let t2 = t * t;
    t + t * t2 * (f32::from_bits(0x3eaa_aaab) + t2 * f32::from_bits(0x3e08_8889))
}

/// The outer SVF: `bp = (g(x - s2) + s1)·d`, `s1 ← 2bp - s1`,
/// `s2' = s2 + 2g·bp`, `lp = (s2 + s2')/2`; out `x + m_lp·lp + m_bp·bp`.
#[derive(Clone, Copy)]
struct Svf {
    g: f32,
    d: f32,
    m: [f32; 2],
    lp_only: bool,
}

impl Svf {
    /// High cut, core 7 (`0x141982520`).
    fn high_cut(x: f32, rate: f32) -> Self {
        let fc = if x <= f32::from_bits(0x3f73_3333) { f32::from_bits(0x46b6_3834) * x * x + 1000.0 } else { (x - f32::from_bits(0x3f73_3333)) * f32::from_bits(0x49d7_5500) + f32::from_bits(0x46ac_4400) };
        let g = tanpoly(std::f32::consts::PI * fc / rate);
        Self { g, d: 1.0 / ((g + std::f32::consts::SQRT_2) * g + 1.0), m: [0.0; 2], lp_only: true }
    }
    /// Low shelf, core 8 (`0x141982300`).
    fn low_shelf(x: f32, rate: f32) -> Self {
        let fc = 200.0 + 200.0 * (x - 0.5).max(0.0);
        let a2 = fexp2(-24.0 * x * 0.166_096_4);
        let a = a2.sqrt();
        let k = 0.70711f32;
        let g = tanpoly(fc / a.sqrt() * std::f32::consts::PI / rate);
        Self { g, d: 1.0 / (2.0 * g * k + 1.0 + g * g), m: [a2 - 1.0, (a - 1.0) * 2.0 * k], lp_only: false }
    }
    #[inline(always)]
    fn tick(&self, s: &mut [f32; 2], x: f32) -> f32 {
        let bp = (self.g * (x - s[1]) + s[0]) * self.d;
        s[0] = 2.0 * bp - s[0];
        let s2 = s[1] + 2.0 * self.g * bp;
        let lp = 0.5 * (s[1] + s2);
        s[1] = s2;
        if self.lp_only { lp } else { x + self.m[0] * lp + self.m[1] * bp }
    }
}

/// Raum's outer core (`0x141981200`) around the kernel: per channel
/// `x·p19² + fb` into the predelay, read `D - 1` behind; `fb = √p6·gate·0.707
/// ·clamp(1.4144272·LP(shelf(pd)), ±2.5)` for the next frame; kernel; then
/// `dry·x + wet·LP(shelf(v))`. The type gain is taken settled at 1.
pub(in super::super) struct Outer {
    ring: [Vec<f32>; 2],
    delay: usize,
    pos: usize,
    input: f32,
    feedback: f32,
    fb: [f32; 2],
    shelf: Svf,
    lp: Svf,
    /// `[ch][feedback shelf, feedback LP, post shelf, post LP]`.
    st: [[[f32; 2]; 4]; 2],
    mix: [f32; 2],
}

impl Outer {
    pub(in super::super) fn new(p: &[f32; 20], rate: u32) -> Self {
        let sr = rate as f32;
        let x = f64::from(p[1]);
        let t = if x <= 0.5 { 0.08 * (2.0 * x).powi(2) } else { 1.92 * (2.0 * x - 1.0).powi(2) + 0.08 };
        let delay = (t * f64::from(rate)).max(2.0).round() as usize;
        let gate = if t < f64::from(f32::from_bits(0x38d1_b717)) { 0.0 } else { 1.0 };
        let m = p[0];
        Self {
            ring: [vec![0.0; delay], vec![0.0; delay]],
            delay,
            pos: 0,
            input: p[19] * p[19],
            feedback: p[6].sqrt() * gate * f32::from_bits(0x3f34_fdf4),
            fb: [0.0; 2],
            shelf: Svf::low_shelf(p[8], sr),
            lp: Svf::high_cut(p[7], sr),
            st: [[[0.0; 2]; 4]; 2],
            mix: if m < 0.5 { [1.0, (2.0 * m).powi(3).sqrt()] } else { [2.0 - 2.0 * m, 1.0] },
        }
    }

    pub(in super::super) fn clear(&mut self) {
        self.ring.iter_mut().for_each(|r| r.fill(0.0));
        (self.fb, self.st) = ([0.0; 2], [[[0.0; 2]; 4]; 2]);
    }

    /// Replace `l`/`r` (at most one block) with Raum's output; `kernel`
    /// runs the network in place.
    pub(in super::super) fn process(&mut self, l: &mut [f32], r: &mut [f32], kernel: impl FnOnce(&mut [f32], &mut [f32])) {
        let n = l.len().min(crate::dsp::BLOCK);
        let mut dry = [[0f32; crate::dsp::BLOCK]; 2];
        dry[0][..n].copy_from_slice(&l[..n]);
        dry[1][..n].copy_from_slice(&r[..n]);
        let len = self.delay;
        for i in 0..n {
            let read = (self.pos + 1) % len;
            for (ch, io) in [&mut l[i], &mut r[i]].into_iter().enumerate() {
                self.ring[ch][self.pos] = *io * self.input + self.fb[ch];
                let pd = self.ring[ch][read];
                let st = &mut self.st[ch];
                let sh = self.shelf.tick(&mut st[0], pd);
                let f = self.lp.tick(&mut st[1], sh);
                // ponytail: the NI limiter is reduced to its ±2.5 clamp (its gain table is not extracted).
                self.fb[ch] = self.feedback * (f32::from_bits(0x3fb5_0bf3) * f).clamp(-2.5, 2.5);
                *io = pd;
            }
            self.pos = read;
        }
        kernel(&mut l[..n], &mut r[..n]);
        for (ch, io) in [&mut l[..n], &mut r[..n]].into_iter().enumerate() {
            let st = &mut self.st[ch];
            for (i, v) in io.iter_mut().enumerate() {
                let sh = self.shelf.tick(&mut st[2], *v);
                let w = self.lp.tick(&mut st[3], sh);
                *v = self.mix[0] * dry[ch][i] + self.mix[1] * w;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AIRY: Kernel = Kernel { airy: true, decay: 0.8, size: 0.5, damping: 0.25, modulation: 0.0, diffusion: 0.25, freeze: false, reverb: 1.0, sparse: false };

    /// Setter `0x14198aa20` at Airy defaults, 48 kHz.
    #[test]
    fn airy_laws_match_native() {
        let len = lengths(true, 0.5);
        assert_eq!((len[0], len[30]), (383, 13259));
        let (t, g, d) = decay(&AIRY, &len, 48_000.0);
        assert!((t - 10.2375).abs() < 1e-6 && (g - 0.2459014).abs() < 1e-7, "{t} {g}");
        assert!((d[0].1 - 0.002454229).abs() < 1e-8 && (d[0].0 - 0.9921858).abs() < 1e-7, "{:?}", d[0]);
    }

    /// Kernel `0x1419878b0` alone, Airy defaults with mod off: L = 1 at 0,
    /// R = 0.5 at 3.
    #[test]
    fn airy_impulse_matches_native() {
        let mut net = super::super::super::network::Network::new(kernel(&AIRY, 48_000.0));
        let (mut l, mut r) = (vec![0f32; 100], vec![0f32; 100]);
        l[0] = 1.0;
        r[3] = 0.5;
        net.process(&mut l, &mut r);
        assert!(l[..86].iter().all(|v| *v == 0.0), "{:?}", &l[..90]);
        let want = [-9.30591e-4, -2.28388e-6, -5.60517e-9];
        // Within the native reconstruction's own error (about 4e-9).
        assert!(l[86..89].iter().zip(want).all(|(g, w)| (g - w).abs() < 1e-8), "{:?}", &l[86..89]);
    }
    /// Whole core `0x141981200` at p0 0.3, p1 0.1, p7 0.6, p8 0.7, Airy
    /// defaults, mod off; same input as above.
    #[test]
    fn outer_core_matches_native() {
        let sh = Svf::low_shelf(0.7, 48_000.0);
        assert!((sh.g - 0.025_470_27).abs() < 2e-8 && (sh.d - 0.964_627_9).abs() < 1e-6 && (sh.m[0] + 1.0 - 0.144_784_3).abs() < 1e-7, "{} {} {}", sh.g, sh.d, sh.m[0]);
        let lp = Svf::high_cut(0.6, 48_000.0);
        assert!((lp.g - 0.704_282_9).abs() < 1e-6 && (lp.d - 0.401_280_8).abs() < 1e-6, "{} {}", lp.g, lp.d);
        let mut p = [0.5, 0.1, 0.0, 0.25, 0.0625, 0.0, 0.0, 0.6, 0.7, 0.5, 0.8, 0.5, 0.25, 0.0, 0.25, 0.25, 0.0, 1.0, 0.0, 1.0];
        p[0] = 0.3;
        let mut o = Outer::new(&p, 48_000);
        assert_eq!(o.delay, 154);
        assert!((o.mix[1] - 0.464_758).abs() < 1e-6);
        let mut net = super::super::super::network::Network::new(spec(&p, 48_000).unwrap());
        let (mut l, mut r) = (vec![0f32; 256], vec![0f32; 256]);
        l[0] = 1.0;
        r[3] = 0.5;
        for (l, r) in l.chunks_mut(64).zip(r.chunks_mut(64)) {
            o.process(l, r, |a, b| net.process(a, b));
        }
        assert_eq!(l[0], 1.0);
        let want = [-8.41861e-5, -1.988_768e-4, -1.369_708e-4];
        assert!(l[239..242].iter().zip(want).all(|(g, w)| (g - w).abs() < 2e-9), "{:?}", &l[236..242]);
    }
}
