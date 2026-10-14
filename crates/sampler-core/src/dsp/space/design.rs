//! Presets: every reverb as a [`Spec`] of the one engine in
//! [`super::network`], at a sample rate.
//!
//! | reverb | lines, matrix | read / filters | input | output |
//! |---|---|---|---|---|
//! | Kontakt Galois (0x59) | 31 prime lengths, m-sequence circulant | weighted linear, quadrature 2 Hz modulation, one-pole Jot damping on the read | per-channel fractional predelay, ±1 injection 49 frames into the line | ±1 / Hall-weighted taps of the damped reads, 3-delay allpass units, 200 Hz SVF shelf, high cut, width, own mix |
//! | Kontakt legacy Reverb (0x15) | 2×8 Freeverb combs, diagonal | integer, comb damping on the read | channel mix, gain, one-pole, predelay | comb reads summed in native order, 4 Schroeder allpasses |
//! | UVI SparkVerb | `2^q` prime lengths, Hadamard | floor / linear / Lagrange, sine-table glide, shelving biquad + DC blocker on the write | gain, predelay, 4 lattice allpasses | even/odd sums rotated by width, high cut |
//! | UVI Diffusion | none | — | 4 lattice allpasses per channel | the diffused input |
//! | generic hall (`Fdn`) | 8 lines, Hadamard | linear, quadrature modulation, damping / band shelves on the write | mono, predelay with echo, one-pole, 2 lattice allpasses | alternating-sign taps, width, low shelf |
//! | generic comb/allpass | 2×8 classic combs, diagonal | integer, comb damping | predelay with echo, one-pole | 4 Schroeder allpasses, width, low shelf |
//! | generic plate | 4-line figure-eight | linear, quadrature modulation, damping on two reads | mono, predelay, 4 lattice allpasses | signed taps, width, low shelf |

use super::network::{Allpass, Chain, Interp, LineSpec, MAX_LINES, Matrix, Modulation, Shelf, Spec, TapFrom, Unit};
use crate::Error;
use sampler_ir::fx::space::{Freeverb, Galois, Network, SparkVerb, Topology};
use std::f64::consts::PI;

/// The spec of `n` at `rate`.
pub(super) fn spec(n: &Network, rate: u32) -> Result<Spec, Error> {
    if rate == 0 {
        return Err(Error::InvalidInput);
    }
    let r = f64::from(rate);
    Ok(match n.topology {
        Topology::Galois => galois(&n.galois.ok_or(Error::InvalidInput)?, r),
        Topology::Freeverb => freeverb(n, &n.freeverb.ok_or(Error::InvalidInput)?, rate as f32),
        Topology::SparkVerb => spark(&n.spark.ok_or(Error::InvalidInput)?, r),
        Topology::Diffuser => diffusion(n.diffusion as f32, n.diffusion_start_ms as f32, r),
        Topology::Fdn => generic_fdn(n, r),
        Topology::CombAllpass => generic_comb(n, r),
        Topology::Plate => generic_plate(n, r),
    })
}

fn identity() -> Matrix {
    Matrix::Permutation { p: std::array::from_fn(|i| i as u8), g: [1.0; MAX_LINES] }
}

/// Coefficient of a one-pole low-pass `y += k(x - y)` at `hz`.
pub(super) fn one_pole(hz: f64, rate: f64) -> f32 {
    (1.0 - (-2.0 * PI * hz.min(rate * 0.49) / rate).exp()) as f32
}

/// The trapezoidal SVF low shelf (Kontakt `FUN_141ac9260`, Q 1) at `db`,
/// with `f = π·fc/rate` as the native passes it.
fn shelf(db: f32, f: f32) -> Shelf {
    let gain = 10f32.powf(db * 0.025);
    let f3 = f * f * f;
    let g = (f3 * 0.333_333_34 + f + f3 * f * f * 0.133_333_34) * (1.0 / gain.sqrt());
    let a1 = 1.0 / ((g + 1.0) * g + 1.0);
    let a2 = g * a1;
    Shelf { m: [1.0, (gain - 1.0) * 1.0, gain * gain - 1.0], a: [a1, a2, g * a2] }
}

/// The generic presets' wet width and low shelf.
fn tone(s: &mut Spec, n: &Network, rate: f64) {
    if n.width != 1.0 {
        s.width = Some(n.width as f32);
    }
    if n.low_shelf_db != 0.0 {
        s.shelf = Some(shelf(n.low_shelf_db as f32, (250.0 * PI / rate) as f32));
    }
}

// ---------------------------------------------------------------- Galois

/// Kontakt Reverb (Galois, SerType 0x59), `NI::DSP::GaloisReverb`, from the
/// Kontakt 8.13.1 binary:
/// - controls: setter `0x141aea370` (ten normalized controls); Hall/Room is
///   the dispatcher's control 10 (`0x1409ab2c0`), which picks one of the two
///   cores the wrapper constructor `0x1407cc260` builds
///   (`0x141ad9d00(core, flag)`); flag 1 (control 10 ≥ 0.5, saved type 1)
///   has the longer lines and is Hall.
/// - line lengths `0x141aebb00`: `prime[(int)(slope·size·10 + 90 + base)]`
///   (Room) or `prime[(int)((slope/32)^1.61·size·736 + 90 + base)]` (Hall),
///   prime table `0x1479ac0c0`.
/// - feedback matrix: the 31×31 orthogonal circulant of the m-sequence
///   `c[n+5] = c[n] ^ c[n+2]` (seed 1,1,0,1,1), `a` where `c = 0`, `b`
///   where `c = 1`, `a − b = 2/√32`, rows summing to −1; run as a 32-point
///   Walsh–Hadamard transform through index maps.
/// - gain and damping `0x141adc7f0` and the tail of `0x141aea370`: Jot
///   absorption, `g^L` and a one-pole whose pole meets `(g'/g)^L` at Nyquist.
/// - processing `0x141acf210` (Room, four 3-delay units) / `0x141ad0590`
///   (Hall, two), dispatched by `0x141ae2c60`; units `0x141ae92b0` /
///   `0x141ae2a80`; low shelf `0x141ac9260` / `0x1419529e0` / `0x140d947a0`;
///   high cut and the 2 Hz modulation in `0x141aec0a0` / `0x141ae1d40`.
/// - the native reads a line after writing it, `L − 1` behind, and injects
///   the input 50 frames ahead of line 0's read; this engine reads before
///   writing, so the same signal is `L` behind and the injection
///   `L_0 − 49` behind the head.
///
/// The core's filters assume 44.1 kHz (`π/44100`); line lengths are
/// samples. Above 48 kHz the native core runs decimated by
/// `trunc(rate / 44100)` behind a resampler pair (`0x141ee5da0`, not
/// extracted); here it runs at the host rate. Parameter ramps
/// (`0x141ae5fc0`) are not reproduced.
pub(super) mod galois_law {
    pub(crate) const LINES: usize = 31;
    /// `0x141aebb00`: per-line base offsets into the prime table.
    pub(crate) const BASE: [u8; LINES] = [4, 3, 5, 5, 5, 5, 2, 1, 1, 1, 4, 5, 1, 5, 1, 4, 3, 4, 2, 4, 1, 2, 2, 5, 4, 2, 3, 1, 3, 0, 3];
    /// Per-line size slopes, Room and Hall.
    pub(crate) const SLOPE: [[u8; LINES]; 2] = [
        [0, 1, 7, 8, 20, 21, 6, 2, 3, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 4, 5, 22, 23, 24, 25, 26, 27, 28, 29, 30],
        [0, 1, 6, 7, 4, 5, 2, 3, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30],
    ];
    /// Input taps (line, sign) of the left and right predelayed inputs.
    pub(crate) const IN_L: [(u8, f32); 10] = [(0, 1.), (2, 1.), (4, 1.), (6, -1.), (10, -1.), (14, 1.), (18, 1.), (22, -1.), (26, -1.), (28, -1.)];
    pub(crate) const IN_R: [(u8, f32); 10] = [(1, 1.), (3, 1.), (5, 1.), (7, -1.), (11, -1.), (15, -1.), (19, -1.), (23, -1.), (27, 1.), (29, 1.)];
    /// Output taps (line, sign): Room, and Hall (`0x141ae0160`); the Hall core
    /// also drops input lines 4..=7.
    pub(crate) const OUT_ROOM: [[(u8, f32); 8]; 2] = [
        [(0, 1.), (3, -1.), (4, 1.), (8, -1.), (12, -1.), (16, 1.), (20, -1.), (24, 1.)],
        [(1, 1.), (2, -1.), (5, 1.), (9, -1.), (13, -1.), (17, 1.), (21, -1.), (25, 1.)],
    ];
    pub(crate) const OUT_HALL: [[(u8, f32); 8]; 2] = [
        [(0, 1.), (3, -1.), (6, -1.), (9, 1.), (12, -1.), (17, 1.), (20, -1.), (25, 1.)],
        [(1, 1.), (2, -1.), (7, 1.), (8, -1.), (13, 1.), (16, -1.), (21, 1.), (24, -1.)],
    ];
    /// Hall output weights at full diffusion: the first tap pair, then the
    /// rest (`1/√5`, `√(19/15)`); zero diffusion weighs every tap 1.
    pub(crate) const HALL_FAR: [f32; 2] = [0.447_213_6, 1.125_462_9];
    /// Unit delays (three per unit), Room and Hall.
    pub(crate) const UNITS: [[[usize; 3]; 4]; 2] = [
        [[31, 79, 41], [43, 73, 53], [59, 61, 67], [71, 47, 37]],
        [[73, 181, 97], [103, 179, 127], [59, 61, 67], [71, 47, 37]],
    ];

    pub(crate) fn primes(n: usize) -> Vec<u32> {
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

    /// Line lengths in samples for size control `x`.
    pub(crate) fn lengths(hall: bool, x: f32) -> [u32; LINES] {
        let size = x * 3.0 + 1.0;
        let table = primes(2800);
        std::array::from_fn(|i| {
            let (slope, base) = (f32::from(SLOPE[usize::from(hall)][i]), f32::from(BASE[i]));
            let k = if hall { (slope * 0.031_25).powf(1.61) * (size * 736.0) + 90.0 } else { slope * (size * 10.0) + 90.0 };
            table[(k + base) as usize]
        })
    }

    /// The m-sequence of the feedback matrix.
    pub(crate) fn sequence() -> [u8; LINES] {
        let mut c = [0u8; LINES];
        c[..5].copy_from_slice(&[1, 1, 0, 1, 1]);
        for n in 0..LINES - 5 {
            c[n + 5] = c[n] ^ c[n + 2];
        }
        c
    }

    /// Index maps turning `S[i][j] = (-1)^c[(j-i) mod 31]` into a
    /// Walsh–Hadamard transform: `S[i][j] = (-1)^popcount(row[i] & col[j])`.
    pub(crate) fn hadamard_maps() -> ([u8; LINES], [u8; LINES]) {
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
    pub(crate) fn matrix_ab() -> (f32, f32) {
        let d = 2.0 / 32f64.sqrt();
        let a = (16.0 * d - 1.0) / 31.0;
        (a as f32, (a - d) as f32)
    }
}

pub(super) fn galois(g: &Galois, rate: f64) -> Spec {
    use galois_law::*;
    let x = g.controls;
    let hall = g.hall;
    let len = lengths(hall, x[Galois::SIZE]);
    let cap = lengths(hall, 1.0);
    let flen = len.map(|l| l as f32);
    // Decay (setter case 0, 0x141adc7f0).
    let t = (x[Galois::TIME] * 3.7).exp() * 0.5;
    let norm = (1.0 - 10f32.powf(-4.0 / t)).sqrt() * 0.5;
    let damp = x[Galois::DAMPING].powf(0.25);
    let l = (t + t).ln();
    let t_hf = ((1.0 - damp * 0.5) * (1.0 - l * 0.270_270_26) + (1.0 - damp * 0.95) * l * 0.270_270_26) * t;
    const K: f32 = -6.802_720_6e-5;
    let base = 10f32.powf(K / t);
    let ratio = 10f32.powf(K / t_hf) / base;
    let depth = x[Galois::MODULATION] * 10.0;
    let mut lines: Vec<LineSpec> = (0..LINES)
        .map(|i| {
            let p = ratio.powf(flen[i]);
            let r = 1.0 / (p * p);
            let a = if r == 1.0 { 0.0 } else { ((r * 2.962_758 - 0.768_274_2).sqrt() - (r + 0.481_379_06)) / (1.0 - r) };
            // The constructor sizes each ring to the power of two above the
            // line's size-1 length; a modulated read past it wraps, as native.
            let mut line = LineSpec::new(cap[i] as usize, f64::from(flen[i]));
            line.depth = if i == 0 { 0.0 } else { f64::from(depth) };
            line.sel = ((i + 1) % 2) as u8;
            (line.a, line.b) = ((1.0 - a) * base.powf(flen[i]), a);
            line
        })
        .collect();
    for (ch, taps) in [IN_L, IN_R].iter().enumerate() {
        for &(line, sign) in taps {
            if !(hall && (4..8).contains(&line)) {
                lines[usize::from(line)].inject = (ch as u8, sign);
            }
        }
    }
    let (row, col) = hadamard_maps();
    let (a, b) = matrix_ab();
    let mut s = Spec::new(lines, Matrix::Circulant { row, col, mean: (a + b) * 0.5, half: (a - b) * 0.5 });
    let diffusion = x[Galois::DIFFUSION] * 0.5;
    for ch in 0..2 {
        let taps = if hall { OUT_HALL[ch] } else { OUT_ROOM[ch] };
        let mut t: Vec<(u8, f32)> = taps
            .iter()
            .enumerate()
            .map(|(k, &(line, sign))| {
                // 0x141aea370: C = 2g·far + (1 − 2g)·near.
                let w = diffusion + diffusion;
                (line, if hall { w * (sign * HALL_FAR[usize::from(k >= 2)]) + (1.0 - w) * sign } else { sign })
            })
            .collect();
        t.sort_by_key(|&(line, _)| line);
        s.taps[ch] = t;
    }
    let unit = |p: f32, d: [usize; 3]| {
        const C: f32 = 0.9;
        let (cp, cp2) = (C * p, p * C * p);
        Unit {
            m: [-cp2, -cp2, cp, 1.0 - cp2, -cp2, cp, 0.0, 1.0 - cp2, cp],
            b: [-p, -p, 1.0],
            q: -p,
            r: 1.0 - p * 0.9 * p,
            d,
            z: [0.0; 3],
            rings: d.map(|n| super::network::Ring::new(n + 1)),
        }
    };
    let units = if hall { 2 } else { 4 };
    let chain = || (0..units).map(|k| unit(if k % 2 == 0 { -diffusion } else { diffusion }, UNITS[usize::from(hall)][k])).collect();
    let (sn, cs) = (x[Galois::MIX] * std::f32::consts::FRAC_PI_2).sin_cos();
    let predelay = x[Galois::PREDELAY] * 11_025.0;
    s.predelay = [predelay + (len[1] - len[0]) as f32, predelay];
    s.pre_cap = 16_384;
    s.interp = Interp::Weighted;
    s.modulation = Modulation::Quad { w: 12.566_370_614_359_172 / rate };
    s.inject_offset = len[0] as usize - 49;
    s.tap_from = TapFrom::Filtered;
    s.chain = Chain::Units([chain(), chain()]);
    s.shelf = Some(shelf(x[Galois::LOW_SHELF] * -12.0, (200.0f64 * PI / 44_100.0) as f32));
    s.high_cut = Some(((f64::from(((1.0 - x[Galois::HIGH_CUT]) * 19_000.0 + 2000.0) * -2.0)) * (PI / 44_100.0)).exp() as f32);
    s.width = Some(x[Galois::STEREO]);
    (s.wet, s.dry) = (sn * norm, cs);
    s
}

// --------------------------------------------------------------- Freeverb

/// Kontakt legacy Reverb (0x15), exact to `FUN_140aa3d10` (process),
/// `FUN_14094c760` (delay lengths) and the case-0x3e setter in
/// `FUN_1409619b0`: per channel an input mix, gain, low-pass and predelay,
/// eight damped combs (`store = d·out + (1-d)·store`, write
/// `fb·store + in`) summed in the native order into four Schroeder
/// allpasses. Lengths are `int((offset_ms + tuning_ms)·rate·0.001)`, the
/// classic Freeverb tuning in milliseconds, the right channel 23/44.1 ms
/// longer.
///
/// The input low-pass is the one stage not proved: Kontakt runs a shared
/// biquad-prototype filter object (vtable `0x144e95870`); a one-pole at
/// `input_cutoff_hz` stands in.
pub(super) mod freeverb_law {
    pub(crate) const COMB_MS: [f64; 8] = [
        25.306_122_448_979_593,
        26.938_775_510_204_08,
        28.956_916_099_773_24,
        30.748_299_319_727_92,
        32.244_897_959_183_675,
        33.809_523_809_523_81,
        35.306_122_448_979_59,
        36.666_666_666_666_664,
    ];
    pub(crate) const ALLPASS_MS: [f64; 4] = [12.607_709_750_566_894, 10.0, 7.732_426_303_854_876, 5.102_040_816_326_531];
    /// The right channel's offset (23 samples at 44.1 kHz).
    pub(crate) const SPREAD_MS: f64 = 0.521_541_950_113_378_7;
    /// Predelay ring; a longer predelay is dropped.
    pub(crate) const PRE: usize = 0x2000;
    /// The native comb sum order.
    pub(crate) const SUM: [u8; 8] = [3, 2, 1, 0, 7, 6, 5, 4];

    /// `int((offset + tuning) * rate * 0.001)`.
    pub(crate) fn length(offset: f32, tuning: f64, rate: f32) -> usize {
        (((f64::from(offset) + tuning) * (f64::from(rate) * 0.001)) as i32).max(1) as usize
    }
}

fn freeverb(n: &Network, f: &Freeverb, rate: f32) -> Spec {
    use freeverb_law::*;
    let offsets = [0.0f32, SPREAD_MS as f32];
    let damp = f.damp as f32;
    let lines = (0..16)
        .map(|i| {
            let (c, j) = (i / 8, i % 8);
            let len = length(offsets[c], COMB_MS[j], rate);
            let mut line = LineSpec::new(len + 1, len as f64);
            (line.a, line.b) = (damp, 1.0 - damp);
            line.inject = (c as u8, 1.0);
            line
        })
        .collect();
    let mut s = Spec::new(lines, Matrix::Permutation { p: std::array::from_fn(|i| i as u8), g: [f.feedback as f32; MAX_LINES] });
    let beta = (2.0 * (1.0 - f.width)) as f32;
    s.mix = if beta > 1e-12 { beta } else { 0.0 };
    s.gain = f.input_gain as f32;
    s.input_lp = (n.input_cutoff_hz < 0.49 * f64::from(rate)).then(|| one_pole(n.input_cutoff_hz, f64::from(rate)));
    // iVar12 = (int)(ms * rate * 0.001f).
    let pre = ((n.predelay_seconds * 1000.0) as f32 * rate * 0.001) as usize;
    s.predelay = [if pre >= PRE { 0.0 } else { pre as f32 }; 2];
    s.pre_cap = PRE;
    s.taps = [0u8, 8].map(|o| SUM.iter().map(|&k| (k + o, 1.0)).collect());
    let g = f.allpass as f32;
    s.chain = Chain::Schroeder([0, 1].map(|c| ALLPASS_MS.iter().map(|&ms| Allpass::new(length(offsets[c], ms, rate), g)).collect()));
    s
}

// --------------------------------------------------------------- SparkVerb

/// UVI SparkVerb (Workstation 4.0.9, init `0x1416799a0`, process
/// `0x141679c90`):
/// - lines: `2^Quality` (`0x14167b8c0`); lengths `0x14167f190`: with
///   `a = (N/2)^Shape`, `r = (2a)^(-1/(N-1))`,
///   `x_i = r^i·N(1-r)·RoomSize·rate/340 / (1 - r^(N+1))`, truncated, raised
///   to the next odd prime (table `0x14202c3e0`, to 191999), sorted.
/// - decay (`0x14167b8c0`): `x_i = -600·L_i/(rate·Decay·RoomSize)` dB
///   (frozen: `-0.006·L_i/rate`), band gain `10^(x_i/(20·D_band))`; the line
///   gain is the mid gain over `√N`, the low/high gains two first-order
///   shelves (`0x141649120`) or a low cut (`0x141649410`) / high cut
///   (`0x141649220`) in one TDF-II biquad (`0x1416450c0`), then a 1 Hz DC
///   blocker (`0x141641be0`).
/// - input gain `√((1-ḡ)/(1+ḡ))`, `ḡ` the mean squared mid gain.
/// - modulation (`0x14167bd80`): per line, the 256-entry sine table read at
///   `f_i = ModRate·rate/(16·L_i)`, depth `cents·ln2·rate/(1200·2π·f_i)`
///   samples, sampled once per ≤64-frame block and glided across it.
/// - Hadamard feedback (`0x1416864d0`), output taps after it (`0x14167c0e0`):
///   `√(2/N)·(cos θ·Σeven + sin θ·Σodd)` and mirrored, `θ = (1-Width)·π/4`;
///   line `i` takes input channel `i mod 2`.
/// - input diffuser (`0x14167e0f0`/`0x14167e420`): four lattice allpasses
///   per channel, gain Diffusion, lengths `⌊(π/2)^k·rate·DiffusionStart/1000 + ½⌋`
///   plus the stereo offsets (`0x14202c2d0`).
/// - Rolloff: one-pole low-pass `exp(-2π·f/rate)` on each output, off at or
///   above 19999 Hz.
///
/// The 10 ms gain ramps and the delay glide after a size change are not
/// reproduced: controls are constant for a unit's life.
pub(super) mod spark_law {
    use std::f64::consts::PI;

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
    pub(crate) fn lengths(n: usize, room_m: f64, shape: f64, rate: f64) -> Vec<u32> {
        let a = (n as f64 * 0.5).powf(shape);
        let r = (2.0 * a).powf(-1.0 / (n as f64 - 1.0));
        let scale = n as f64 * (1.0 - r) * room_m * rate * 0.002_941_176_470_588_235_3 / (1.0 - r.powi(n as i32 + 1));
        let mut out: Vec<u32> = (0..n).map(|i| prime_at_least((r.powi(i as i32) * scale) as u32)).collect();
        out.sort_unstable();
        out
    }

    /// First-order section `(b0, b1, a1)` with DC gain `g0` and Nyquist gain
    /// `gpi` (`0x141649120`).
    pub(crate) fn shelf(fc: f64, g0: f64, gpi: f64) -> [f64; 3] {
        let r = g0 / gpi;
        let t = (PI * fc).tan();
        let (lo, hi) = (t / r.sqrt(), t * r.sqrt());
        [gpi * (hi + 1.0) / (lo + 1.0), gpi * (hi - 1.0) / (lo + 1.0), (lo - 1.0) / (lo + 1.0)]
    }

    /// Low cut (`0x141649410`) and high cut (`0x141649220`) sections.
    pub(crate) fn cut(fc: f64, high: bool) -> [f64; 3] {
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

    /// Two first-order sections as one biquad `b0 b1 b2 a1 a2`.
    pub(crate) fn cascade(lo: [f64; 3], hi: [f64; 3]) -> [f64; 5] {
        [lo[0] * hi[0], lo[0] * hi[1] + lo[1] * hi[0], lo[1] * hi[1], lo[2] + hi[2], lo[2] * hi[2]]
    }
}

/// UVI's four-allpass input diffuser (the kernel `0x14167e640`, lattice
/// form, gain `g`): lengths `⌊base^k·rate·0.001·ms + ½⌋` plus the stereo
/// offsets (0, 23, 0, 23) / (23, 0, 23, 0).
fn uvi_diffuser(base: f64, g: f32, ms: f32, rate: f64) -> [Vec<Allpass>; 2] {
    const OFFSET: [[usize; 4]; 2] = [[0, 23, 0, 23], [23, 0, 23, 0]];
    let len = |k: i32| (base.powi(k) * rate * 0.001 * f64::from(ms) + 0.5).floor() as usize;
    [0, 1].map(|ch| (0..4).map(|k| Allpass::new((len(k as i32) + OFFSET[ch][k]).max(1), g)).collect())
}

fn spark(s: &SparkVerb, rate: f64) -> Spec {
    use spark_law::*;
    let n = 1usize << s.quality;
    let lens = lengths(n, f64::from(s.room_m), f64::from(s.shape), rate);
    let (fl, fh) = ((f64::from(s.freq_low) / rate).min(0.499), (f64::from(s.freq_high) / rate).min(0.499));
    let mut mean = 0.0;
    let lines: Vec<LineSpec> = lens
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
            let f = f64::from(s.mod_hz) * rate / (16.0 * l);
            let amp = f64::from(s.mod_cents) * std::f64::consts::LN_2 * rate / (7_539.822_368_615_503 * f);
            let mut line = LineSpec::new(len as usize + amp.ceil() as usize + super::network::GLIDE_BLOCK + 4, l);
            line.depth = amp;
            line.phase = if i % 2 == 1 { 1 << 31 } else { 0 };
            line.inc = (4_294_967_296.0 * f / rate) as i64 as u32;
            (line.a, line.b) = ((gm / (n as f64).sqrt()) as f32, 0.0);
            line.inject = ((i % 2) as u8, 1.0);
            line.bq = cascade(lo, hi).map(|v| v as f32);
            line
        })
        .collect();
    let mean = mean / n as f64;
    let theta = f64::from(1.0 - s.width) * PI / 4.0;
    let sc = (2.0 / n as f64).sqrt();
    let mut spec = Spec::new(lines, Matrix::Hadamard);
    spec.gain = ((1.0 - mean) / (1.0 + mean)).max(0.0).sqrt() as f32;
    let pre = (f64::from(s.predelay_seconds) * rate) as usize;
    spec.predelay = [pre as f32; 2];
    spec.pre_cap = pre + 1;
    if let Some((g, ms)) = s.diffuser {
        spec.diffuser = uvi_diffuser(PI / 2.0, g, ms, rate);
    }
    spec.interp = match s.mode.min(2) {
        0 => Interp::Floor,
        1 => Interp::Linear,
        _ => Interp::Lagrange,
    };
    spec.modulation = Modulation::Glide;
    spec.write_filter = Some(1.0 - (-2.0 * PI / rate).exp() as f32);
    spec.tap_from = TapFrom::Mixed;
    spec.taps = [0, 1].map(|p| (0..n).filter(|i| i % 2 == p).map(|i| (i as u8, 1.0)).collect());
    let out = [(sc * theta.cos()) as f32, (sc * theta.sin()) as f32];
    spec.out = [[out[0], out[1]], [out[1], out[0]]];
    spec.high_cut = (s.rolloff_hz < 19_999.0).then(|| (-2.0 * PI * f64::from(s.rolloff_hz) / rate).exp() as f32);
    spec
}

// --------------------------------------------------------------- Diffusion

/// UVI Diffusion (constructor `0x14130fc60`): the SparkVerb diffuser kernel
/// alone, four lattice allpasses per channel of gain `g`, lengths
/// `⌊π^k·rate·0.001·ms + ½⌋` (`0x141310da0`, π at `0x141d3d908`) plus the
/// stereo offsets (`0x141e81350` / `0x141e81390`).
fn diffusion(g: f32, ms: f32, rate: f64) -> Spec {
    let mut s = Spec::new(Vec::new(), identity());
    s.diffuser = uvi_diffuser(PI, g, ms, rate);
    s.direct = 1.0;
    s
}

// ------------------------------------------------------------ generic ones

/// Generic hall: eight Hadamard-coupled modulated lines (base lengths spread
/// over an octave, Jot gains for the decay), damping or two band shelves on
/// the write, a mono input through predelay, one-pole and two allpasses.
fn generic_fdn(n: &Network, rate: f64) -> Spec {
    const BASE_MS: [f64; 8] = [31.7, 37.1, 41.3, 43.9, 53.3, 59.9, 67.1, 73.7];
    const ALLPASS_MS: [f64; 2] = [4.77, 3.59];
    let depth = n.modulation_seconds * rate * 0.5;
    let rt60 = n.decay_seconds;
    let loss = |d: f64, rt60: f64| 10f64.powf(-3.0 * d / (rt60 * rate));
    let lines = (0..8)
        .map(|i| {
            let d = BASE_MS[i] * 0.001 * rate * n.size;
            let mut line = LineSpec::new((d + 2.0 * depth) as usize + 4, d + depth);
            line.depth = if i % 4 < 2 { depth } else { -depth };
            line.sel = (i % 2) as u8;
            line.inject = ((i % 2) as u8, 1.0);
            let g = loss(d, rt60);
            match n.bands {
                Some(b) => {
                    let (lo, hi) = ((b.low_hz / rate).min(0.499), (b.high_hz / rate).min(0.499));
                    let lo = spark_law::shelf(lo, loss(d, rt60 * b.low) / g, 1.0);
                    let hi = spark_law::shelf(hi, 1.0, loss(d, rt60 * b.high) / g);
                    let bq = spark_law::cascade(lo, hi);
                    line.a = (g / 8f64.sqrt()) as f32;
                    line.bq = bq.map(|v| v as f32);
                }
                None => {
                    // Damping as a one-pole on the read.
                    let k = f64::from(one_pole(n.damping_hz, rate));
                    (line.a, line.b) = ((k * g / 8f64.sqrt()) as f32, (1.0 - k) as f32);
                }
            }
            line
        })
        .collect();
    let mut s = Spec::new(lines, Matrix::Hadamard);
    s.mix = 1.0;
    s.input_lp = Some(one_pole(n.input_cutoff_hz, rate));
    let pre = (n.predelay_seconds * rate) as usize;
    s.predelay = [pre as f32; 2];
    s.pre_cap = pre + 2;
    s.echo = if pre > 0 { n.echo_feedback as f32 } else { 0.0 };
    let g = -(n.diffusion as f32);
    s.diffuser = [0, 1].map(|_| ALLPASS_MS.iter().map(|ms| Allpass::new(((ms * 0.001 * rate * n.size) as usize).max(1), g)).collect());
    s.interp = Interp::Linear;
    s.modulation = Modulation::Quad { w: 2.0 * PI * n.modulation_hz / rate };
    s.write_filter = n.bands.map(|_| 0.0);
    s.taps = [[(0, 0.5), (2, -0.5), (4, 0.5), (6, -0.5)].to_vec(), [(1, 0.5), (3, -0.5), (5, 0.5), (7, -0.5)].to_vec()];
    tone(&mut s, n, rate);
    s
}

/// Generic comb/allpass: per channel eight combs at the classic Schroeder /
/// Moorer tuning scaled by size (right 23 samples longer), Jot feedback,
/// one damping coefficient, into four Schroeder allpasses.
fn generic_comb(n: &Network, rate: f64) -> Spec {
    const COMB: [f64; 8] = [1116., 1188., 1277., 1356., 1422., 1491., 1557., 1617.];
    const ALLPASS: [f64; 4] = [556., 441., 341., 225.];
    let scale = rate / 44_100.0 * n.size;
    let len = |v: f64, c: usize| (((v + 23.0 * c as f64) * scale) as usize).max(2);
    let k = one_pole(n.damping_hz, rate);
    let mut g = [0f32; MAX_LINES];
    let lines = (0..16)
        .map(|i| {
            let (c, j) = (i / 8, i % 8);
            let l = len(COMB[j], c);
            g[i] = 10f64.powf(-3.0 * l as f64 / (n.decay_seconds * rate)) as f32;
            let mut line = LineSpec::new(l + 1, l as f64);
            (line.a, line.b) = (k, 1.0 - k);
            line.inject = (c as u8, 1.0);
            line
        })
        .collect();
    let mut s = Spec::new(lines, Matrix::Permutation { p: std::array::from_fn(|i| i as u8), g });
    s.gain = 0.125;
    s.input_lp = Some(one_pole(n.input_cutoff_hz, rate));
    let pre = (n.predelay_seconds * rate) as usize;
    s.predelay = [pre as f32; 2];
    s.pre_cap = pre + 2;
    s.echo = if pre > 0 { n.echo_feedback as f32 } else { 0.0 };
    s.taps = [0u8, 8].map(|o| (0..8).map(|k| (k + o, 1.0)).collect());
    let gd = n.diffusion as f32;
    s.chain = Chain::Schroeder([0, 1].map(|c| ALLPASS.iter().map(|&v| Allpass::new(len(v, c), gd)).collect()));
    tone(&mut s, n, rate);
    s
}

/// Generic plate: Dattorro's figure-eight tank (JAES 1997) reduced to four
/// lines — each branch a modulated delay with damping and decay, then a
/// second delay feeding the other branch — behind a mono input diffuser of
/// four allpasses; lengths are the paper's at 29.761 kHz scaled by rate and
/// size. Kontakt's Plate Reverb (0x57) topology is not recovered.
fn generic_plate(n: &Network, rate: f64) -> Spec {
    const PAPER_RATE: f64 = 29_761.0;
    const INPUT_AP: [f64; 4] = [142., 107., 379., 277.];
    /// Branch A delays 1 and 2, branch B delays 1 and 2.
    const LINES: [f64; 4] = [4453. + 672., 3720. + 1800., 4217. + 908., 3163. + 2656.];
    let scale = rate / PAPER_RATE * n.size;
    let depth = n.modulation_seconds * rate * 0.5;
    let loop_len: f64 = LINES.iter().sum::<f64>() * scale;
    // The loop passes both branches once; each branch applies `decay` once.
    let decay = 10f64.powf(-3.0 * loop_len * 0.5 / (n.decay_seconds * rate)) as f32;
    let k = one_pole(n.damping_hz, rate);
    let lines = LINES
        .iter()
        .enumerate()
        .map(|(i, &l)| {
            let d = (l * scale).max(2.0);
            let mut line = LineSpec::new((d + 2.0 * depth) as usize + 4, d + depth);
            if i % 2 == 0 {
                line.depth = depth;
                line.sel = (i / 2) as u8;
                (line.a, line.b) = (k, 1.0 - k);
                line.inject = ((i / 2) as u8, 1.0);
            }
            line
        })
        .collect();
    // A1 → A2 → B1 → B2 → A1, the decay after each damped read.
    let mut p = [0u8; MAX_LINES];
    p[..4].copy_from_slice(&[3, 0, 1, 2]);
    let mut g = [1f32; MAX_LINES];
    g[1] = decay;
    g[3] = decay;
    let mut s = Spec::new(lines, Matrix::Permutation { p, g });
    s.mix = 1.0;
    s.input_lp = Some(one_pole(n.input_cutoff_hz, rate));
    let pre = (n.predelay_seconds * rate) as usize;
    s.predelay = [pre as f32; 2];
    s.pre_cap = pre + 2;
    s.echo = if pre > 0 { n.echo_feedback as f32 } else { 0.0 };
    let gd = n.diffusion as f32;
    let ap = |ch: usize| {
        INPUT_AP
            .iter()
            .enumerate()
            .map(|(i, &l)| Allpass::new(((l * scale) as usize + 3 * ch).max(1), if i < 2 { gd } else { gd * 0.8333 }))
            .collect()
    };
    s.diffuser = [ap(0), ap(1)];
    s.interp = Interp::Linear;
    s.modulation = Modulation::Quad { w: 2.0 * PI * n.modulation_hz / rate };
    s.taps = [vec![(0, 0.6), (1, -0.6), (2, 0.3)], vec![(2, 0.6), (3, -0.6), (0, 0.3)]];
    tone(&mut s, n, rate);
    s
}

#[cfg(test)]
mod tests;
