//! The Kontakt Reverb (Galois, SerType 0x59) core, `NI::DSP::GaloisReverb`.
//!
//! Laws, from the Kontakt 8.13.1 binary:
//! - controls: setter `0x141aea370` (ten normalized controls, see
//!   [`sampler_ir::fx::space::Galois`]); Hall/Room is the dispatcher's control
//!   10 (`0x1409ab2c0`), which picks one of two cores built by the wrapper
//!   constructor `0x1407cc260` (`0x141ad9d00(core, flag)`). Control 10 at or
//!   above 0.5 runs the flag-1 core; saved type 1 is Hall (`FUN_140d019f0`)
//!   and the flag-1 lines are the longer ones, so flag 1 is called Hall.
//! - line lengths: `0x141aebb00`, `prime[(int)(slope·size·10 + 90 + base)]`
//!   (Room) or `prime[(int)((slope/32)^1.61·size·736 + 90 + base)]` (Hall),
//!   prime table `0x1479ac0c0`, base/slope tables from the constructor.
//! - feedback matrix: 31×31 orthogonal circulant of the length-31 m-sequence
//!   `c[n+5] = c[n] ^ c[n+2]` (seed 1,1,0,1,1), `a` where `c = 0`, `b` where
//!   `c = 1`, `a − b = 2/√32`, rows summing to −1. Applied here with a
//!   32-point Walsh–Hadamard transform instead of the native 32×32 product.
//! - per-line gain and damping: `0x141adc7f0` and the tail of `0x141aea370`
//!   (Jot absorption: `g^L` and a one-pole whose pole meets `(g'/g)^L` at
//!   Nyquist).
//! - processing: stereo kernels `0x141acf210` (Room, four 3-delay diffusers)
//!   and `0x141ad0590` (Hall, two), dispatched by `0x141ae2c60`; diffusers
//!   `0x141ae92b0` / `0x141ae2a80`; low shelf `0x141ac9260` / `0x1419529e0` /
//!   `0x140d947a0`; high cut and the 2 Hz delay modulation in `0x141aec0a0` /
//!   `0x141ae1d40`.
//!
//! The core's filters assume 44.1 kHz (`0x144f38d58`, `π/44100`); its line
//! lengths are samples. Above 48 kHz the native core runs decimated by
//! `trunc(rate / 44100)` behind a resampler pair whose kernel is not
//! extracted (`0x141ee5da0`); here it runs at the host rate.
//! Parameter ramps (`0x141ae5fc0`) are not reproduced: the controls are
//! constant for a unit's life.
//!
//! Verified against the native kernel executed in an emulator
//! (`tools/re/kemu.py`): see the tests.

use crate::Error;
use sampler_ir::fx::space::{Galois as Controls, Network};

const LINES: usize = 31;
/// `0x141aebb00`: the constructor's per-line base offsets into the prime table.
const BASE: [u8; LINES] = [4, 3, 5, 5, 5, 5, 2, 1, 1, 1, 4, 5, 1, 5, 1, 4, 3, 4, 2, 4, 1, 2, 2, 5, 4, 2, 3, 1, 3, 0, 3];
/// Per-line size slopes, Room and Hall cores.
const SLOPE: [[u8; LINES]; 2] = [
    [0, 1, 7, 8, 20, 21, 6, 2, 3, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 4, 5, 22, 23, 24, 25, 26, 27, 28, 29, 30],
    [0, 1, 6, 7, 4, 5, 2, 3, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30],
];
/// Input taps (line, sign) of the left and right predelayed inputs.
const IN_L: [(u8, f32); 10] = [(0, 1.), (2, 1.), (4, 1.), (6, -1.), (10, -1.), (14, 1.), (18, 1.), (22, -1.), (26, -1.), (28, -1.)];
const IN_R: [(u8, f32); 10] = [(1, 1.), (3, 1.), (5, 1.), (7, -1.), (11, -1.), (15, -1.), (19, -1.), (23, -1.), (27, 1.), (29, 1.)];
/// Output taps (line, sign) of each channel: Room, and the Hall core's
/// (`0x141ae0160`); the Hall core also drops input lines 4..=7.
const OUT_ROOM: [[(u8, f32); 8]; 2] = [
    [(0, 1.), (3, -1.), (4, 1.), (8, -1.), (12, -1.), (16, 1.), (20, -1.), (24, 1.)],
    [(1, 1.), (2, -1.), (5, 1.), (9, -1.), (13, -1.), (17, 1.), (21, -1.), (25, 1.)],
];
const OUT_HALL: [[(u8, f32); 8]; 2] = [
    [(0, 1.), (3, -1.), (6, -1.), (9, 1.), (12, -1.), (17, 1.), (20, -1.), (25, 1.)],
    [(1, 1.), (2, -1.), (7, 1.), (8, -1.), (13, 1.), (16, -1.), (21, 1.), (24, -1.)],
];
/// Hall output weights: the first pair of taps, then the rest, at full
/// diffusion (`1/√5`, `√(19/15)`); zero diffusion weighs every tap 1.
const HALL_FAR: [f32; 2] = [0.447_213_6, 1.125_462_9];
/// Diffuser delays (three per unit), Room and Hall.
const DIFFUSER: [[[usize; 3]; 4]; 2] = [
    [[31, 79, 41], [43, 73, 53], [59, 61, 67], [71, 47, 37]],
    [[73, 181, 97], [103, 179, 127], [59, 61, 67], [71, 47, 37]],
];

fn primes(n: usize) -> Vec<u32> {
    let mut out = Vec::with_capacity(n);
    let mut k = 2u32;
    while out.len() < n {
        if out.iter().take_while(|&&p| p * p <= k).all(|&p| k % p != 0) {
            out.push(k);
        }
        k += 1;
    }
    out
}

/// Line lengths in samples for size control `x` (`0x141aebb00`).
fn lengths(hall: bool, x: f32) -> [u32; LINES] {
    let size = x * 3.0 + 1.0;
    let table = primes(2800);
    std::array::from_fn(|i| {
        let (slope, base) = (f32::from(SLOPE[usize::from(hall)][i]), f32::from(BASE[i]));
        let k = if hall { (slope * 0.031_25).powf(1.61) * (size * 736.0) + 90.0 } else { slope * (size * 10.0) + 90.0 };
        table[(k + base) as usize]
    })
}

/// The m-sequence (`c[n+5] = c[n] ^ c[n+2]`) of the feedback matrix.
fn sequence() -> [u8; LINES] {
    let mut c = [0u8; LINES];
    c[..5].copy_from_slice(&[1, 1, 0, 1, 1]);
    for n in 0..LINES - 5 {
        c[n + 5] = c[n] ^ c[n + 2];
    }
    c
}

/// Index maps that turn the circulant `±1` matrix `S[i][j] = (-1)^c[(j-i) mod 31]`
/// into a Walsh–Hadamard transform: `S[i][j] = (-1)^popcount(row[i] & col[j])`.
fn hadamard_maps() -> ([u8; LINES], [u8; LINES]) {
    let c = sequence();
    let col: [u8; LINES] = std::array::from_fn(|j| (0..5).map(|k| c[(j + k) % LINES] << k).sum());
    let row = std::array::from_fn(|i| {
        (1..32u8)
            .find(|&r| (0..LINES).all(|j| ((r & col[j]).count_ones() & 1) as u8 == c[(j + LINES - i) % LINES]))
            .expect("an m-sequence circulant is a permuted Hadamard matrix")
    });
    (row, col)
}

/// The matrix entries `a` (sequence 0) and `b` (sequence 1).
fn matrix_ab() -> (f32, f32) {
    let d = 2.0 / 32f64.sqrt();
    let a = (16.0 * d - 1.0) / 31.0;
    (a as f32, (a - d) as f32)
}

/// A delay ring with a power-of-two length.
struct Ring {
    buf: Vec<f32>,
    mask: usize,
}

impl Ring {
    fn new(min_len: usize) -> Self {
        let n = min_len.next_power_of_two().max(2);
        Self { buf: vec![0.0; n], mask: n - 1 }
    }
    #[inline(always)]
    fn at(&self, i: usize) -> f32 {
        self.buf[i & self.mask]
    }
    /// Linear read `d` samples behind `w` (the native `(1-f)·x[w-i] + f·x[w-i-1]`).
    #[inline(always)]
    fn frac(&self, w: usize, d: f32) -> f32 {
        let i = d as i32;
        let f = d - i as f32;
        let k = w.wrapping_sub(i as usize);
        (1.0 - f) * self.at(k) + f * self.at(k.wrapping_sub(1))
    }
}

/// Three delays coupled by a fixed matrix (`0x141ae92b0`, run by `0x141ae2a80`).
struct Diffuser {
    m: [f32; 9],
    b: [f32; 3],
    q: f32,
    r: f32,
    d: [usize; 3],
    z: [f32; 3],
    rings: [Ring; 3],
}

impl Diffuser {
    /// `p` is `−g` for the first and third unit and `g` for the others.
    fn new(p: f32, d: [usize; 3]) -> Self {
        const C: f32 = 0.9;
        let (cp, cp2) = (C * p, p * C * p);
        Self {
            m: [-cp2, -cp2, cp, 1.0 - cp2, -cp2, cp, 0.0, 1.0 - cp2, cp],
            b: [-p, -p, 1.0],
            q: -p,
            r: 1.0 - p * 0.9 * p,
            d,
            z: [0.0; 3],
            rings: d.map(|n| Ring::new(n + 1)),
        }
    }
    #[inline(always)]
    fn tick(&mut self, w: usize, x: f32) -> f32 {
        let (m, z) = (&self.m, self.z);
        let y = x * self.q + z[0] * self.r;
        for j in 0..3 {
            let v = m[j] * z[0] + m[3 + j] * z[1] + m[6 + j] * z[2] + self.b[j] * x;
            let ring = &mut self.rings[j];
            let mask = ring.mask;
            ring.buf[w & mask] = v;
            self.z[j] = ring.at(w.wrapping_sub(self.d[j]).wrapping_add(1));
        }
        y
    }
}

/// The 200 Hz low shelf: a trapezoidal state-variable filter (`0x140d947a0`).
struct Shelf {
    m: [f32; 3],
    a: [f32; 3],
    ic: [f32; 2],
}

impl Shelf {
    /// `FUN_141ac9260(fc 200, Q 1, dB, low shelf, 44100)`.
    fn new(db: f32) -> Self {
        let gain = 10f32.powf(db * 0.025);
        let f = (200.0f64 * std::f64::consts::PI / 44_100.0) as f32;
        let f3 = f * f * f;
        let g = (f3 * 0.333_333_34 + f + f3 * f * f * 0.133_333_34) * (1.0 / gain.sqrt());
        let a1 = 1.0 / ((g + 1.0) * g + 1.0);
        let a2 = g * a1;
        Self { m: [1.0, (gain - 1.0) * 1.0, gain * gain - 1.0], a: [a1, a2, g * a2], ic: [0.0; 2] }
    }
    #[inline(always)]
    fn tick(&mut self, x: f32) -> f32 {
        let [ic1, ic2] = self.ic;
        let v3 = x - ic2;
        let v1 = v3 * self.a[1] + ic1 * self.a[0];
        let v2 = self.a[1] * ic1 + self.a[2] * v3 + ic2;
        self.ic = [2.0 * v1 - ic1, 2.0 * v2 - ic2];
        v1 * self.m[1] + x * self.m[0] + v2 * self.m[2]
    }
}

struct Channel {
    diffusers: Vec<Diffuser>,
    shelf: Shelf,
    cut: f32,
    lp: f32,
}

pub(super) struct GaloisCore {
    lines: Vec<Ring>,
    len0: usize,
    flen: [f32; LINES],
    /// Damped line outputs of the previous sample (the matrix input).
    v: [f32; 32],
    gain: [f32; LINES],
    pole: [f32; LINES],
    row: [u8; LINES],
    col: [u8; LINES],
    /// `(a+b)/2` and `(a−b)/2`.
    mean: f32,
    half: f32,
    input: [[f32; LINES]; 2],
    output: [[f32; LINES]; 2],
    pre: [Ring; 2],
    predelay: f32,
    spread: f32,
    depth: f32,
    /// LFO phasor and its rotation (f64, as native).
    phasor: (f64, f64),
    rot: (f64, f64),
    channels: [Channel; 2],
    wet: f32,
    dry: f32,
    width: f32,
    w: usize,
}

impl GaloisCore {
    pub(super) fn new(n: &Network, rate: u32) -> Result<Self, Error> {
        let g = n.galois.ok_or(Error::InvalidInput)?;
        if rate == 0 || g.controls.iter().any(|v| !(0.0..=1.0).contains(v)) {
            return Err(Error::InvalidInput);
        }
        Ok(Self::with(&g, rate as f64))
    }

    fn with(g: &Controls, rate: f64) -> Self {
        let x = g.controls;
        let hall = g.hall;
        let len = lengths(hall, x[Controls::SIZE]);
        let flen = len.map(|l| l as f32);
        // Decay (setter case 0, 0x141adc7f0).
        let t = (x[Controls::TIME] * 3.7).exp() * 0.5;
        let norm = (1.0 - 10f32.powf(-4.0 / t)).sqrt() * 0.5;
        let damp = x[Controls::DAMPING].powf(0.25);
        let l = (t + t).ln();
        let t_hf = ((1.0 - damp * 0.5) * (1.0 - l * 0.270_270_26) + (1.0 - damp * 0.95) * l * 0.270_270_26) * t;
        const K: f32 = -6.802_720_6e-5;
        let base = 10f32.powf(K / t);
        let ratio = 10f32.powf(K / t_hf) / base;
        let mut gain = [0.0; LINES];
        let mut pole = [0.0; LINES];
        for i in 0..LINES {
            let p = ratio.powf(flen[i]);
            let r = 1.0 / (p * p);
            let a = if r == 1.0 { 0.0 } else { ((r * 2.962_758 - 0.768_274_2).sqrt() - (r + 0.481_379_06)) / (1.0 - r) };
            pole[i] = a;
            gain[i] = (1.0 - a) * base.powf(flen[i]);
        }
        let (row, col) = hadamard_maps();
        let (a, b) = matrix_ab();
        let mut input = [[0.0; LINES]; 2];
        for (ch, taps) in [IN_L, IN_R].iter().enumerate() {
            for &(line, sign) in taps {
                if !(hall && (4..8).contains(&line)) {
                    input[ch][usize::from(line)] = sign;
                }
            }
        }
        let diffusion = x[Controls::DIFFUSION] * 0.5;
        let mut output = [[0.0; LINES]; 2];
        for ch in 0..2 {
            let taps = if hall { OUT_HALL[ch] } else { OUT_ROOM[ch] };
            for (k, &(line, sign)) in taps.iter().enumerate() {
                output[ch][usize::from(line)] = if hall {
                    // 0x141aea370: C = 2g·far + (1 − 2g)·near.
                    let w = diffusion + diffusion;
                    w * (sign * HALL_FAR[usize::from(k >= 2)]) + (1.0 - w) * sign
                } else {
                    sign
                };
            }
        }
        let cut = ((f64::from(((1.0 - x[Controls::HIGH_CUT]) * 19_000.0 + 2000.0) * -2.0)) * (std::f64::consts::PI / 44_100.0)).exp() as f32;
        let units = if hall { 2 } else { 4 };
        let channel = || Channel {
            diffusers: (0..units)
                .map(|k| Diffuser::new(if k % 2 == 0 { -diffusion } else { diffusion }, DIFFUSER[usize::from(hall)][k]))
                .collect(),
            shelf: Shelf::new(x[Controls::LOW_SHELF] * -12.0),
            cut,
            lp: 0.0,
        };
        let (s, c) = (x[Controls::MIX] * std::f32::consts::FRAC_PI_2).sin_cos();
        let w = 12.566_370_614_359_172 / rate;
        Self {
            // The constructor sizes each ring to the power of two above the
            // line's largest (size 1) length; a modulated read past it wraps,
            // as native (Room line 7 is 1021 of 1024 at full size).
            lines: lengths(hall, 1.0).iter().map(|&l| Ring::new(l as usize)).collect(),
            len0: len[0] as usize,
            flen,
            v: [0.0; 32],
            gain,
            pole,
            row,
            col,
            mean: (a + b) * 0.5,
            half: (a - b) * 0.5,
            input,
            output,
            pre: [Ring::new(16_384), Ring::new(16_384)],
            predelay: x[Controls::PREDELAY] * 11_025.0,
            spread: (len[1] - len[0]) as f32,
            depth: x[Controls::MODULATION] * 10.0,
            phasor: (1.0, 0.0),
            rot: (w.cos(), w.sin()),
            channels: [channel(), channel()],
            wet: s * norm,
            dry: c,
            width: x[Controls::STEREO],
            w: 0,
        }
    }

    pub(super) fn clear(&mut self) {
        for r in self.lines.iter_mut().chain(&mut self.pre) {
            r.buf.fill(0.0);
        }
        self.v = [0.0; 32];
        self.phasor = (1.0, 0.0);
        for ch in &mut self.channels {
            for d in &mut ch.diffusers {
                d.z = [0.0; 3];
                for r in &mut d.rings {
                    r.buf.fill(0.0);
                }
            }
            ch.shelf.ic = [0.0; 2];
            ch.lp = 0.0;
        }
    }

    /// `M·v`: the mean term plus the Walsh–Hadamard form of the sign matrix.
    #[inline(always)]
    fn mix(&self) -> [f32; LINES] {
        let mut t = [0f32; 32];
        let mut sum = 0.0;
        for j in 0..LINES {
            t[usize::from(self.col[j])] = self.v[j];
            sum += self.v[j];
        }
        let mut h = 1;
        while h < 32 {
            for i in (0..32).step_by(2 * h) {
                for k in i..i + h {
                    let (x, y) = (t[k], t[k + h]);
                    t[k] = x + y;
                    t[k + h] = x - y;
                }
            }
            h *= 2;
        }
        let m = self.mean * sum;
        std::array::from_fn(|i| m + self.half * t[usize::from(self.row[i])])
    }

    /// Replace `l`/`r` with the core's output (its own dry/wet mix included).
    pub(super) fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        for (xl, xr) in l.iter_mut().zip(r.iter_mut()) {
            let w = self.w;
            self.w = w.wrapping_add(1);
            let (c, s) = self.phasor;
            let (cw, sw) = self.rot;
            let ns = c * sw + s * cw;
            let nc = c * cw - sw * s;
            self.phasor = (nc, ns);
            let (inl, inr) = (*xl, *xr);
            let mut x = [0f32; 2];
            for (k, (v, d)) in [(inl, self.predelay + self.spread), (inr, self.predelay)].into_iter().enumerate() {
                let ring = &mut self.pre[k];
                let mask = ring.mask;
                ring.buf[w & mask] = v;
                x[k] = ring.frac(w, d);
            }
            let y = self.mix();
            let lfo = [nc as f32, ns as f32];
            for i in 0..LINES {
                let inj = x[0] * self.input[0][i] + x[1] * self.input[1][i];
                let pos = if i == 0 { self.flen[0] } else { lfo[(i + 1) % 2] * self.depth + self.flen[i] };
                let ring = &mut self.lines[i];
                let mask = ring.mask;
                ring.buf[w & mask] = y[i];
                let j = w.wrapping_sub(self.len0).wrapping_add(50) & mask;
                ring.buf[j] += inj;
                let out = ring.frac(w, pos - 1.0);
                self.v[i] = out * self.gain[i] + self.pole[i] * self.v[i];
            }
            let mut o = [0f32; 2];
            for (ch, taps) in self.output.iter().enumerate() {
                o[ch] = taps.iter().zip(&self.v).map(|(c, v)| c * v).sum();
            }
            for (ch, chan) in self.channels.iter_mut().enumerate() {
                let mut v = o[ch];
                for d in &mut chan.diffusers {
                    v = d.tick(w, v);
                }
                let v = chan.shelf.tick(v);
                chan.lp = (chan.lp - v) * chan.cut + v;
                o[ch] = chan.lp;
            }
            let mid = (o[0] + o[1]) * 0.5;
            *xl = ((o[0] - mid) * self.width + mid) * self.wet + self.dry * inl;
            *xr = ((o[1] - mid) * self.width + mid) * self.wet + self.dry * inr;
        }
    }
}

#[cfg(test)]
mod tests;
