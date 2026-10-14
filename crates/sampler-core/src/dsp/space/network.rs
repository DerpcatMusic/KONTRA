//! The one algorithmic reverb engine. Every reverb (Kontakt Galois and
//! legacy Reverb, UVI SparkVerb, the generic hall/plate/comb layouts) is a
//! [`Spec`] built by [`super::design`] and run by this loop.
//!
//! Per frame, in order:
//! 1. input: channel mix `x = β·mean + (1-β)·x`, gain, one-pole low-pass
//!    `lp += k(x - lp)`, predelay (fractional, `(1-f)·a + f·b`, optional echo
//!    feedback), then a lattice allpass chain per channel
//!    (`v = x - g·z; y = z + g·v`).
//! 2. lines (up to 32): read `d` frames behind the write head (before this
//!    frame's write) with the chosen interpolation, `d` = base length plus
//!    modulation; read filter `v = a·r + b·v'`.
//! 3. feedback matrix `u = M·v`: permutation with gains (parallel combs),
//!    unnormalised Walsh–Hadamard, the 31-line m-sequence circulant, or dense.
//! 4. write: optional TDF-II biquad and DC blocker on `u`, written at the
//!    head; the injected input `c·x_ch` is added `inject_offset` frames
//!    behind it.
//! 5. output: two tap groups (plus, for a diffuser alone, the input stage) summed in order from the reads, the filtered
//!    reads or the matrix output, mixed 2×2 to L/R; a per-channel chain
//!    (Schroeder, Jezar or 3-delay allpass units), a state-variable low
//!    shelf, a one-pole high cut `lp = (lp - y)·c + y`, then width and the
//!    engine's own dry/wet.
//!
//! The interpolation, the modulation source and the write filter are const
//! parameters of the loop, chosen once when the spec is built; other stages
//! are skipped by a flag tested once per frame.

use crate::blocks::lfo::uvi_sine;

pub(super) const MAX_LINES: usize = 32;
/// Frames per modulation update of [`Modulation::Glide`].
pub(super) const GLIDE_BLOCK: usize = 64;
/// Frames per pass of the parallel-lines path.
const BLOCK: usize = 64;

/// A power-of-two delay ring. Each ring stores frame `w` at a different
/// offset (`skew`), so rings of one size written at the same `w` do not
/// all land in one cache set.
#[derive(Clone)]
pub(super) struct Ring {
    pub(super) buf: Vec<f32>,
    /// `buf.len() == 1 << bits`: slicing to `1 << bits` lets the masked
    /// index skip its bounds check.
    bits: u32,
    skew: usize,
}

impl Ring {
    pub(super) fn new(min_len: usize) -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = min_len.next_power_of_two().max(2);
        // 17 cache lines (16 floats each) apart.
        let skew = NEXT.fetch_add(17 * 16, std::sync::atomic::Ordering::Relaxed);
        Self { buf: vec![0.0; n], bits: n.trailing_zeros(), skew }
    }
    #[inline(always)]
    pub(super) fn at(&self, w: usize, k: usize) -> f32 {
        self.view_ref()[w.wrapping_add(self.skew).wrapping_sub(k) & ((1 << self.bits) - 1)]
    }
    #[inline(always)]
    pub(super) fn put(&mut self, w: usize, v: f32) {
        let n = 1 << self.bits;
        self.buf[..n][w.wrapping_add(self.skew) & (n - 1)] = v;
    }
    #[inline(always)]
    fn add(&mut self, w: usize, v: f32) {
        let n = 1 << self.bits;
        self.buf[..n][w.wrapping_add(self.skew) & (n - 1)] += v;
    }
    /// `(1-f)·x[w-i] + f·x[w-i-1]`, `d = i + f`.
    #[inline(always)]
    pub(super) fn weighted(&self, w: usize, d: f32) -> f32 {
        let i = d as usize;
        let f = d - i as f32;
        (1.0 - f) * self.at(w, i) + f * self.at(w, i + 1)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Interp {
    /// `x[w - ⌊d⌋]`.
    Floor = 0,
    /// `a + (b - a)·f`.
    Linear = 1,
    /// `(1-f)·a + f·b`.
    Weighted = 2,
    /// Four-point third-order Lagrange.
    Lagrange = 3,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Modulation {
    /// Every line reads its base length.
    None,
    /// A quadrature f64 phasor rotated by `w` radians per frame; line `i`
    /// adds `depth_i · (cos, sin)[sel_i]` (f32) to its base.
    Quad { w: f64 },
    /// Per line, the UVI sine table at a u32 phase stepping `inc` per frame,
    /// sampled every [`GLIDE_BLOCK`] frames and glided across the block (the
    /// step clamped to the block length).
    Glide,
    /// Two quadrature f64 phasors (`w` radians per frame each); line `i`
    /// adds `depth_i · m[sel_i]`, `m = [sin₁, cos₁, sin₂, cos₂]` (f32), shaped
    /// by the odd polynomial `P` when `shape` (Kontakt Raum Airy). With
    /// `reverse`, every `period` frames an LCG (`x·0x19fb4787 + 0x16d1b86b`,
    /// seed 0) reverses each phasor with probability 1/4.
    Dual { w: [f64; 2], shape: bool, reverse: Option<u32> },
}

#[derive(Clone, Debug)]
pub(super) enum Matrix {
    /// `u_i = g_i · v_{p_i}`.
    Permutation { p: [u8; MAX_LINES], g: [f32; MAX_LINES] },
    /// Unnormalised Walsh–Hadamard over a power-of-two line count.
    Hadamard,
    /// The 31×31 circulant of an m-sequence: `mean·Σv + half·H(v)` through
    /// index maps into a 32-point transform.
    Circulant { row: [u8; 31], col: [u8; 31], mean: f32, half: f32 },
    /// Row-major `n×n`, for a measured matrix with no structure.
    #[allow(dead_code)]
    Dense(Vec<f32>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TapFrom {
    Read,
    Filtered,
    Mixed,
}

/// An allpass of one of the chain forms.
#[derive(Clone)]
pub(super) struct Allpass {
    pub(super) ring: Ring,
    pub(super) len: usize,
    pub(super) g: f32,
}

impl Allpass {
    pub(super) fn new(len: usize, g: f32) -> Self {
        Self { ring: Ring::new(len + 1), len, g }
    }
    /// Lattice: `v = x - g·z; y = z + g·v`.
    #[inline(always)]
    fn lattice(&mut self, w: usize, x: f32) -> f32 {
        let z = self.ring.at(w, self.len);
        let v = x - self.g * z;
        self.ring.put(w, v);
        z + self.g * v
    }
    /// Schroeder: `y = z - g·x`, write `x + g·y`.
    #[inline(always)]
    fn schroeder(&mut self, w: usize, x: f32) -> f32 {
        let y = x * -self.g + self.ring.at(w, self.len);
        self.ring.put(w, y * self.g + x);
        y
    }
    /// Jezar's Freeverb: `y = z - x`, write `x + g·z`.
    #[inline(always)]
    fn jezar(&mut self, w: usize, x: f32) -> f32 {
        let z = self.ring.at(w, self.len);
        self.ring.put(w, x + z * self.g);
        -x + z
    }
}

/// Three delays coupled by a fixed matrix (Kontakt Galois `0x141ae92b0`).
#[derive(Clone)]
pub(super) struct Unit {
    pub(super) m: [f32; 9],
    pub(super) b: [f32; 3],
    pub(super) q: f32,
    pub(super) r: f32,
    pub(super) d: [usize; 3],
    pub(super) z: [f32; 3],
    pub(super) rings: [Ring; 3],
}

impl Unit {
    #[inline(always)]
    fn tick(&mut self, w: usize, x: f32) -> f32 {
        let (m, z) = (&self.m, self.z);
        let y = x * self.q + z[0] * self.r;
        for j in 0..3 {
            let v = m[j] * z[0] + m[3 + j] * z[1] + m[6 + j] * z[2] + self.b[j] * x;
            let ring = &mut self.rings[j];
            ring.put(w, v);
            self.z[j] = ring.at(w.wrapping_add(1), self.d[j]);
        }
        y
    }
}

/// A stereo pair of three-line orthogonal units cross-rotated after each
/// state update (Kontakt Raum `GaloisReverbExtended`): per channel
/// `v = Aᵀu + b·x`, `y = c·u₀ + d·x`; then `w_L = cos φ·v_L + sin φ·v_R`,
/// `w_R = -sin φ·v_L + cos φ·v_R` written to the lines, and `u_j` read
/// `n_j - 2` frames behind the write.
#[derive(Clone)]
pub(super) struct Coupled {
    pub(super) a: [f32; 9],
    pub(super) b: [f32; 3],
    pub(super) c: f32,
    pub(super) d: f32,
    pub(super) rot: (f32, f32),
    pub(super) n: [usize; 3],
    pub(super) u: [[f32; 3]; 2],
    pub(super) rings: [[Ring; 3]; 2],
}

impl Coupled {
    #[inline(always)]
    fn tick(&mut self, w: usize, x: [f32; 2]) -> [f32; 2] {
        let a = &self.a;
        let mut v = [[0f32; 3]; 2];
        let mut y = [0f32; 2];
        for ch in 0..2 {
            let u = self.u[ch];
            for j in 0..3 {
                v[ch][j] = a[j] * u[0] + a[3 + j] * u[1] + a[6 + j] * u[2] + self.b[j] * x[ch];
            }
            y[ch] = self.c * u[0] + self.d * x[ch];
        }
        let (c, s) = self.rot;
        for j in 0..3 {
            let (l, r) = (v[0][j], v[1][j]);
            self.rings[0][j].put(w, c * l + s * r);
            self.rings[1][j].put(w, -s * l + c * r);
            for ch in 0..2 {
                self.u[ch][j] = self.rings[ch][j].at(w, self.n[j] - 2);
            }
        }
        y
    }
}

/// The per-channel output chain.
#[derive(Clone)]
pub(super) enum Chain {
    None,
    Schroeder([Vec<Allpass>; 2]),
    Jezar([Vec<Allpass>; 2]),
    Units([Vec<Unit>; 2]),
    Coupled(Vec<Coupled>),
}

/// A trapezoidal state-variable low shelf (Kontakt `0x140d947a0`).
#[derive(Clone, Copy)]
pub(super) struct Shelf {
    pub(super) m: [f32; 3],
    pub(super) a: [f32; 3],
}

/// One delay line's constants.
#[derive(Clone, Copy, Debug)]
pub(super) struct LineSpec {
    /// Ring length at least.
    pub(super) cap: usize,
    /// Base delay in frames (read `d` behind the write head before writing).
    pub(super) base: f64,
    /// Quad: peak excursion (frames) and the phasor component (0 cos, 1 sin).
    /// Glide: peak excursion, starting phase and phase step per frame.
    pub(super) depth: f64,
    pub(super) sel: u8,
    pub(super) phase: u32,
    pub(super) inc: u32,
    /// Read filter `v = a·r + b·v'`.
    pub(super) a: f32,
    pub(super) b: f32,
    /// Input injection: channel and gain.
    pub(super) inject: (u8, f32),
    /// Write biquad `b0 b1 b2 a1 a2` (when the spec enables it).
    pub(super) bq: [f32; 5],
}

impl LineSpec {
    pub(super) fn new(cap: usize, base: f64) -> Self {
        Self { cap, base, depth: 0.0, sel: 0, phase: 0, inc: 0, a: 1.0, b: 0.0, inject: (0, 0.0), bq: [1.0, 0.0, 0.0, 0.0, 0.0] }
    }
}

pub(super) const MAX_INPUT_BQ: usize = 4;

/// A tapped ring of the input stage's left output (Kontakt Plate's
/// injection and early-reflection taps): each frame writes the input, then
/// `inject` adds `c·x[w - d]` to line `i`'s matrix input and `early` adds
/// `[cL, cR]·x[w - d]` to the output groups.
#[derive(Clone)]
pub(super) struct Feed {
    pub(super) ring: Ring,
    pub(super) inject: Vec<(usize, u8, f32)>,
    pub(super) early: Vec<(usize, [f32; 2])>,
}

/// Everything the engine runs, at one sample rate.
#[derive(Clone)]
pub(super) struct Spec {
    pub(super) mix: f32,
    pub(super) gain: f32,
    pub(super) input_lp: Option<f32>,
    /// Direct-form-I input biquads `b0 b1 b2 a1 a2`, in series (at most
    /// [`MAX_INPUT_BQ`]).
    pub(super) input_bq: Vec<[f32; 5]>,
    /// Predelay per channel in frames (fractional) and the ring length.
    pub(super) predelay: [f32; 2],
    pub(super) pre_cap: usize,
    pub(super) echo: f32,
    pub(super) diffuser: [Vec<Allpass>; 2],
    pub(super) lines: Vec<LineSpec>,
    pub(super) interp: Interp,
    pub(super) modulation: Modulation,
    pub(super) matrix: Matrix,
    /// The write biquad and the DC blocker pole (`None`: neither).
    pub(super) write_filter: Option<f32>,
    pub(super) inject_offset: usize,
    /// The lines' biquads filter the reads (TDF-II) instead of the writes;
    /// the read filter `a`/`b` is then unused.
    pub(super) read_biquad: bool,
    /// A tapped input ring feeding the matrix input and the output.
    pub(super) feed: Option<Feed>,
    /// Freeze: the matrix takes the raw reads (taps still see the read
    /// filter).
    pub(super) freeze: bool,
    pub(super) tap_from: TapFrom,
    /// Two groups of `(line, coefficient)`, summed in order.
    pub(super) taps: [Vec<(u8, f32)>; 2],
    /// Output L/R from the two groups.
    pub(super) out: [[f32; 2]; 2],
    /// The input stage's output added to each channel (a diffuser alone).
    pub(super) direct: f32,
    pub(super) chain: Chain,
    pub(super) shelf: Option<Shelf>,
    pub(super) high_cut: Option<f32>,
    /// Mid/side width, then `wet·y + dry·input`.
    pub(super) width: Option<f32>,
    pub(super) wet: f32,
    pub(super) dry: f32,
}

impl Spec {
    /// Empty network with every optional stage off.
    pub(super) fn new(lines: Vec<LineSpec>, matrix: Matrix) -> Self {
        Self {
            mix: 0.0,
            gain: 1.0,
            input_lp: None,
            input_bq: Vec::new(),
            read_biquad: false,
            feed: None,
            freeze: false,
            predelay: [0.0; 2],
            pre_cap: 1,
            echo: 0.0,
            diffuser: [Vec::new(), Vec::new()],
            lines,
            interp: Interp::Floor,
            modulation: Modulation::None,
            matrix,
            write_filter: None,
            inject_offset: 0,
            tap_from: TapFrom::Read,
            taps: [Vec::new(), Vec::new()],
            out: [[1.0, 0.0], [0.0, 1.0]],
            direct: 0.0,
            chain: Chain::None,
            shelf: None,
            high_cut: None,
            width: None,
            wet: 1.0,
            dry: 0.0,
        }
    }
}

/// The running network.
/// Per-line constants as arrays padded to [`MAX_LINES`] (unused lines are
/// zero), so the arithmetic passes run as vector code.
#[derive(Clone, Copy)]
struct Lanes {
    /// Unmodulated reads: whole and fractional delay.
    j: [usize; MAX_LINES],
    f: [f32; MAX_LINES],
    /// Modulated reads: `Σ lfo_k·dm_k + base` in f32 (one `dm_k` is nonzero).
    base: [f32; MAX_LINES],
    dm: [[f32; MAX_LINES]; 4],
    /// Read filter, diagonal gain, and the injection: one input channel
    /// (`ch`) times `inj` (no `0·x` term for the other channel, as natively).
    a: [f32; MAX_LINES],
    b: [f32; MAX_LINES],
    g: [f32; MAX_LINES],
    inj: [f32; MAX_LINES],
    ch: [u8; MAX_LINES],
    /// Write biquad.
    bq: [[f32; MAX_LINES]; 5],
}

/// Per-line state.
#[derive(Clone, Copy)]
struct State {
    s0: [f32; MAX_LINES],
    s1: [f32; MAX_LINES],
    dc: [f32; MAX_LINES],
}

const ZERO: [f32; MAX_LINES] = [0.0; MAX_LINES];

pub(super) struct Network {
    spec: Spec,
    k: Box<Lanes>,
    st: Box<State>,
    v: [f32; MAX_LINES],
    n: usize,
    /// Whether the predelay stage runs (a predelay or an echo).
    pre_on: bool,
    /// A permutation matrix that is the identity: parallel lines.
    diagonal: bool,
    /// Parallel lines: lines run a block at a time.
    pub(super) parallel: bool,
    rings: Vec<Ring>,
    pre: [Ring; 2],
    lp: [f32; 2],
    df1: [[[f32; 4]; 2]; MAX_INPUT_BQ],
    phasor: (f64, f64),
    rot: (f64, f64),
    /// [`Modulation::Dual`]'s second phasor, its countdown and LCG.
    phasor2: (f64, f64),
    rot2: (f64, f64),
    count: u32,
    lcg: u32,
    /// Glide: current delay per line, and this block's start and step.
    cur: [f64; MAX_LINES],
    glide: [(f64, f64); MAX_LINES],
    phase: [u32; MAX_LINES],
    /// Frames left in the current glide block.
    left: usize,
    shelf: [[f32; 2]; 2],
    cut: [f32; 2],
    w: usize,
}

impl Network {
    pub(super) fn new(spec: Spec) -> Self {
        let n = spec.lines.len();
        assert!(n <= MAX_LINES);
        let (rot, rot2) = match spec.modulation {
            Modulation::Quad { w } => ((w.cos(), w.sin()), (1.0, 0.0)),
            Modulation::Dual { w, .. } => ((w[0].cos(), w[0].sin()), (w[1].cos(), w[1].sin())),
            _ => ((1.0, 0.0), (1.0, 0.0)),
        };
        let mut k = Lanes { j: [0; MAX_LINES], f: ZERO, base: ZERO, dm: [ZERO; 4], a: ZERO, b: ZERO, g: ZERO, inj: ZERO, ch: [0; MAX_LINES], bq: [ZERO; 5] };
        let gains = match &spec.matrix {
            Matrix::Permutation { g, .. } => *g,
            _ => [1.0; MAX_LINES],
        };
        for (i, l) in spec.lines.iter().enumerate() {
            k.j[i] = l.base as usize;
            k.f[i] = (l.base - k.j[i] as f64) as f32;
            k.base[i] = l.base as f32;
            k.dm[usize::from(l.sel.min(3))][i] = l.depth as f32;
            (k.a[i], k.b[i], k.g[i]) = (l.a, l.b, gains[i]);
            (k.ch[i], k.inj[i]) = (l.inject.0.min(1), l.inject.1);
            for c in 0..5 {
                k.bq[c][i] = l.bq[c];
            }
        }
        let diagonal = matches!(&spec.matrix, Matrix::Permutation { p, .. } if p[..n].iter().enumerate().all(|(i, &k)| usize::from(k) == i));
        let mut s = Self {
            k: Box::new(k),
            st: Box::new(State { s0: ZERO, s1: ZERO, dc: ZERO }),
            v: ZERO,
            pre_on: spec.predelay != [0.0; 2] || spec.echo != 0.0,
            diagonal,
            parallel: diagonal && !spec.read_biquad && spec.feed.is_none() && !spec.freeze,
            n,
            rings: spec.lines.iter().map(|l| Ring::new(l.cap)).collect(),
            pre: [Ring::new(spec.pre_cap), Ring::new(spec.pre_cap)],
            lp: [0.0; 2],
            df1: [[[0.0; 4]; 2]; MAX_INPUT_BQ],
            phasor: (1.0, 0.0),
            rot,
            phasor2: (1.0, 0.0),
            rot2,
            count: 0,
            lcg: 0,
            cur: [0.0; MAX_LINES],
            glide: [(0.0, 0.0); MAX_LINES],
            phase: [0; MAX_LINES],
            left: 0,
            shelf: [[0.0; 2]; 2],
            cut: [0.0; 2],
            w: 0,
            spec,
        };
        s.clear();
        s
    }

    pub(super) fn clear(&mut self) {
        for r in self.rings.iter_mut().chain(&mut self.pre) {
            r.buf.fill(0.0);
        }
        let s = &mut self.spec;
        for a in s.diffuser.iter_mut().flatten() {
            a.ring.buf.fill(0.0);
        }
        match &mut s.chain {
            Chain::None => {}
            Chain::Schroeder(c) | Chain::Jezar(c) => c.iter_mut().flatten().for_each(|a| a.ring.buf.fill(0.0)),
            Chain::Units(c) => {
                for u in c.iter_mut().flatten() {
                    u.z = [0.0; 3];
                    u.rings.iter_mut().for_each(|r| r.buf.fill(0.0));
                }
            }
            Chain::Coupled(c) => {
                for u in c {
                    u.u = [[0.0; 3]; 2];
                    u.rings.iter_mut().flatten().for_each(|r| r.buf.fill(0.0));
                }
            }
        }
        for (i, l) in s.lines.iter().enumerate() {
            self.cur[i] = l.base;
            self.phase[i] = l.phase;
        }
        (self.lp, self.v, self.df1) = ([0.0; 2], ZERO, [[[0.0; 4]; 2]; MAX_INPUT_BQ]);
        if let Some(f) = &mut s.feed {
            f.ring.buf.fill(0.0);
        }
        *self.st = State { s0: ZERO, s1: ZERO, dc: ZERO };
        (self.phasor, self.left, self.shelf, self.cut) = ((1.0, 0.0), 0, [[0.0; 2]; 2], [0.0; 2]);
        (self.phasor2, self.lcg) = ((1.0, 0.0), 0);
        if let Modulation::Dual { w, reverse, .. } = s.modulation {
            (self.rot, self.rot2) = ((w[0].cos(), w[0].sin()), (w[1].cos(), w[1].sin()));
            self.count = reverse.unwrap_or(0);
        }
    }

    /// Replace `l`/`r` with the network's output.
    pub(super) fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len().min(r.len());
        let (l, r) = (&mut l[..n], &mut r[..n]);
        macro_rules! run {
            ($($i:literal $m:literal $f:literal),*) => {
                match (self.spec.interp as u8, self.mod_kind(), self.spec.write_filter.is_some()) {
                    $(($i, $m, $f) => sampler_simd::dispatch(#[inline(always)] || self.run::<$i, $m, $f>(l, r)),)*
                    _ => unreachable!(),
                }
            };
        }
        run!(0 0 false, 0 0 true, 0 1 false, 0 1 true, 0 2 false, 0 2 true,
             1 0 false, 1 0 true, 1 1 false, 1 1 true, 1 2 false, 1 2 true,
             2 0 false, 2 0 true, 2 1 false, 2 1 true, 2 2 false, 2 2 true,
             3 0 false, 3 0 true, 3 1 false, 3 1 true, 3 2 false, 3 2 true,
             0 3 false, 1 3 false, 2 3 false, 3 3 false);
    }

    fn mod_kind(&self) -> u8 {
        match self.spec.modulation {
            Modulation::None => 0,
            Modulation::Quad { .. } => 1,
            Modulation::Glide => 2,
            Modulation::Dual { .. } => 3,
        }
    }

    /// Start a glide block of `len` frames: advance each line's phase, take
    /// the table's target and glide towards it.
    fn glide_block(&mut self, len: usize) {
        let span = len as f64 - 1e-10;
        for (i, line) in self.spec.lines.iter().enumerate() {
            self.phase[i] = self.phase[i].wrapping_add((len as u32).wrapping_mul(line.inc));
            let target = line.base + line.depth * f64::from(uvi_sine(self.phase[i]));
            let next = self.cur[i] + (target - self.cur[i]).clamp(-span, span);
            self.glide[i] = (self.cur[i], (next - self.cur[i]) / len as f64);
            self.cur[i] = next;
        }
    }

    #[inline(always)]
    fn run<const I: u8, const M: u8, const F: bool>(&mut self, l: &mut [f32], r: &mut [f32]) {
        let mut k = 0;
        while k < l.len() {
            let len = if M == 2 {
                if self.left == 0 {
                    // A glide block never straddles a call: the native engine
                    // processes at most 64 frames per call.
                    let len = (l.len() - k).min(GLIDE_BLOCK);
                    self.glide_block(len);
                    self.left = len;
                }
                self.left
            } else {
                l.len() - k
            };
            if self.parallel {
                for c in (k..k + len).step_by(BLOCK) {
                    let e = (c + BLOCK).min(k + len);
                    self.block::<I, M, F>(&mut l[c..e], &mut r[c..e], c - k);
                }
            } else {
                for c in (k..k + len).step_by(BLOCK) {
                    let e = (c + BLOCK).min(k + len);
                    let mut xs = [[0f32; 2]; BLOCK];
                    let w0 = self.w;
                    self.input_block(&l[c..e], &r[c..e], w0, &mut xs);
                    let mut o = [[0f32; BLOCK]; 2];
                    for j in c..e {
                        let step = if M == 2 { (len - self.left + j - k) as f64 } else { 0.0 };
                        [o[0][j - c], o[1][j - c]] = self.frame::<I, M, F>(xs[j - c], step);
                    }
                    self.finish(&mut o, &mut l[c..e], &mut r[c..e], w0);
                }
            }
            if M == 2 {
                self.left -= len;
            }
            k += len;
        }
    }

    /// Advance the quadrature phasor: this frame's `[cos, sin]`.
    #[inline(always)]
    fn lfo(&mut self) -> [f32; 4] {
        let (c, si) = self.phasor;
        let (cw, sw) = self.rot;
        let (nc, ns) = (c * cw - sw * si, c * sw + si * cw);
        self.phasor = (nc, ns);
        [nc as f32, ns as f32, 0.0, 0.0]
    }

    /// [`Modulation::Dual`]: the random reversals, then both phasors.
    #[inline(always)]
    fn lfo2(&mut self) -> [f32; 4] {
        let Modulation::Dual { shape, reverse, .. } = self.spec.modulation else { return [0.0; 4] };
        if let Some(period) = reverse {
            self.count -= 1;
            if self.count == 0 {
                self.count = period;
                for rot in [&mut self.rot, &mut self.rot2] {
                    let x1 = self.lcg.wrapping_mul(0x19fb_4787).wrapping_add(0x16d1_b86b);
                    let x2 = x1.wrapping_mul(0x19fb_4787).wrapping_add(0x16d1_b86b);
                    self.lcg = x2;
                    if (f64::from(x2) * 4_294_967_296.0 + f64::from(x1)) * 5.421_010_862_427_522e-20 < 0.25 {
                        rot.1 = -rot.1;
                    }
                }
            }
        }
        let step = |(a, b): (f64, f64), (c, s): (f64, f64)| (a * c - s * b, a * s + b * c);
        self.phasor = step(self.phasor, self.rot);
        self.phasor2 = step(self.phasor2, self.rot2);
        let m = [self.phasor.1 as f32, self.phasor.0 as f32, self.phasor2.1 as f32, self.phasor2.0 as f32];
        if shape {
            // Odd asin-like series with P(1) = 1.
            m.map(|v| {
                let v2 = v * v;
                v * (0.759_479_3 + v2 * (0.126_579_9 + v2 * (0.056_960_95 + v2 * (0.033_905_33 + v2 * 0.023_074_46))))
            })
        } else {
            m
        }
    }

    /// The input stage for frames `w0..` (at most a block). Every stage is a
    /// per-channel chain with private state, so it runs a stage at a time.
    #[inline(always)]
    fn input_block(&mut self, l: &[f32], r: &[f32], w0: usize, xs: &mut [[f32; 2]; BLOCK]) {
        let len = l.len();
        let s = &mut self.spec;
        let mut ch = [[0f32; BLOCK]; 2];
        for j in 0..len {
            let mut x = [l[j], r[j]];
            if s.mix != 0.0 {
                let mean = 0.5 * x[1] + 0.5 * x[0];
                let keep = 1.0 - s.mix;
                x = x.map(|v| s.mix * mean + keep * v);
            }
            [ch[0][j], ch[1][j]] = x;
        }
        // A fully mono input runs the stage once.
        let chans = if s.mix == 1.0 { 1 } else { 2 };
        for (c, v) in ch.iter_mut().enumerate().take(chans) {
            let v = &mut v[..len];
            v.iter_mut().for_each(|v| *v *= s.gain);
            for (q, &[b0, b1, b2, a1, a2]) in s.input_bq.iter().enumerate() {
                let mut d = self.df1[q][c];
                for v in v.iter_mut() {
                    let [x1, x2, y1, y2] = d;
                    let y = b0 * *v + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
                    d = [*v, x1, y, y1];
                    *v = y;
                }
                self.df1[q][c] = d;
            }
            if let Some(k) = s.input_lp {
                let mut lp = self.lp[c];
                for v in v.iter_mut() {
                    lp += k * (*v - lp);
                    *v = lp;
                }
                self.lp[c] = lp;
            }
            if self.pre_on {
                let ring = &mut self.pre[c];
                let d = s.predelay[c];
                for (j, v) in v.iter_mut().enumerate() {
                    let w = w0.wrapping_add(j);
                    let echo = if s.echo != 0.0 { s.echo * ring.weighted(w, d) } else { 0.0 };
                    ring.put(w, *v + echo);
                    *v = ring.weighted(w, d);
                }
            }
            for a in &mut s.diffuser[c] {
                for (j, v) in v.iter_mut().enumerate() {
                    *v = a.lattice(w0.wrapping_add(j), *v);
                }
            }
        }
        for j in 0..len {
            xs[j] = [ch[0][j], ch[usize::from(chans == 2)][j]];
        }
    }

    #[inline(always)]
    /// One frame of the lines, matrix and taps: the mixed output before the
    /// chain.
    fn frame<const I: u8, const M: u8, const F: bool>(&mut self, x: [f32; 2], step: f64) -> [f32; 2] {
        let n = self.n;
        let w = self.w;
        self.w = w.wrapping_add(1);
        let lfo = match M {
            1 => self.lfo(),
            3 => self.lfo2(),
            _ => [0.0; 4],
        };
        let s = &mut self.spec;
        // Lines.
        let (k, st) = (&*self.k, &mut *self.st);
        let mut rd = [0f32; MAX_LINES];
        for (i, ring) in self.rings.iter().enumerate() {
            let (j, f) = position::<M>(k, i, lfo, self.glide[i], step);
            rd[i] = read::<I, _>(ring, w, j, f);
        }
        // Scalar per line: mixing vector and scalar passes over the same
        // arrays stalls on store forwarding.
        let mut u = [0f32; MAX_LINES];
        let one = s.write_filter.unwrap_or(0.0);
        let at = w.wrapping_sub(s.inject_offset);
        let v = &mut self.v;
        if self.diagonal {
            for (i, ring) in self.rings.iter_mut().enumerate() {
                v[i] = rd[i] * k.a[i] + k.b[i] * v[i];
                u[i] = v[i] * k.g[i];
                write::<F>(k, st, one, i, ring, w, at, u[i], x);
            }
        } else {
            if s.read_biquad {
                let [b0, b1, b2, a1, a2] = &k.bq;
                for i in 0..n {
                    let z = b0[i] * rd[i] + st.s0[i];
                    st.s0[i] = b1[i] * rd[i] - a1[i] * z + st.s1[i];
                    st.s1[i] = b2[i] * rd[i] - a2[i] * z;
                    v[i] = z;
                }
            } else {
                for i in 0..n {
                    v[i] = rd[i] * k.a[i] + k.b[i] * v[i];
                }
            }
            if let Some(f) = &mut s.feed {
                f.ring.put(w, x[0]);
                let mut m = *v;
                for &(d, i, c) in &f.inject {
                    m[usize::from(i)] += c * f.ring.at(w, d);
                }
                apply(&s.matrix, &m, n, &mut u);
            } else {
                apply(&s.matrix, if s.freeze { &rd } else { v }, n, &mut u);
            }
            for (i, ring) in self.rings.iter_mut().enumerate() {
                write::<F>(k, st, one, i, ring, w, at, u[i], x);
            }
        }
        let v = &self.v;
        let src: &[f32; MAX_LINES] = match s.tap_from {
            TapFrom::Read => &rd,
            TapFrom::Filtered => v,
            TapFrom::Mixed => &u,
        };
        let mut g = s.taps.each_ref().map(|t| t.iter().fold(0f32, |acc, &(i, c)| acc + c * src[usize::from(i)]));
        if let Some(f) = &s.feed {
            for &(d, [cl, cr]) in &f.early {
                let e = f.ring.at(w, d);
                g = [g[0] + cl * e, g[1] + cr * e];
            }
        }
        self.mix_out(g, x)
    }

    /// Parallel lines, `l.len() ≤ BLOCK` frames, `step0` frames into the
    /// glide block: the input stage per frame, then each line across the
    /// block, then the output stage per frame. Same arithmetic, same order
    /// as [`Self::frame`].
    #[inline(always)]
    fn block<const I: u8, const M: u8, const F: bool>(&mut self, l: &mut [f32], r: &mut [f32], step0: usize) {
        let len = l.len();
        let w0 = self.w;
        let mut xs = [[0f32; 2]; BLOCK];
        let mut lfo = [[0f32; 4]; BLOCK];
        for j in 0..len {
            lfo[j] = match M {
                1 => self.lfo(),
                3 => self.lfo2(),
                _ => [0.0; 4],
            };
        }
        self.input_block(l, r, w0, &mut xs);
        let mut src = [[0f32; BLOCK]; MAX_LINES];
        let (k, st) = (&*self.k, &mut *self.st);
        let s = &self.spec;
        let one = s.write_filter.unwrap_or(0.0);
        let line = Line { k, one, off: s.inject_offset, tap: s.tap_from, len, w0, step0, lfo: &lfo, xs: &xs };
        // Four lines at a time: each line's filter is a serial recurrence,
        // so four interleaved chains keep the core busy.
        let mut i = 0;
        for rings in self.rings.chunks_mut(4) {
            let n = rings.len();
            let (v, glide, src) = (&mut self.v[i..i + n], &self.glide[i..i + n], &mut src[i..i + n]);
            match rings {
                [a, b, c, d] => line.run::<I, M, F, 4>(i, [a, b, c, d], st, v, glide, src),
                [a, b, c] => line.run::<I, M, F, 3>(i, [a, b, c], st, v, glide, src),
                [a, b] => line.run::<I, M, F, 2>(i, [a, b], st, v, glide, src),
                [a] => line.run::<I, M, F, 1>(i, [a], st, v, glide, src),
                _ => unreachable!(),
            }
            i += n;
        }
        self.w = w0.wrapping_add(len);
        // Taps across the block (each frame still sums in tap order), then
        // the output stage; allpass chains run a stage at a time.
        let mut g = [[0f32; BLOCK]; 2];
        for (g, t) in g.iter_mut().zip(&self.spec.taps) {
            for &(i, c) in t {
                let s = &src[usize::from(i)];
                for j in 0..len {
                    g[j] += c * s[j];
                }
            }
        }
        let mut o = [[0f32; BLOCK]; 2];
        for j in 0..len {
            [o[0][j], o[1][j]] = self.mix_out([g[0][j], g[1][j]], xs[j]);
        }
        // Allpass chains inline on the local array (measurably fewer
        // instructions than through `finish`).
        let jezar = matches!(self.spec.chain, Chain::Jezar(_));
        if let Chain::Schroeder(c) | Chain::Jezar(c) = &mut self.spec.chain {
            for (o, c) in o.iter_mut().zip(c.iter_mut()) {
                for a in c {
                    for j in 0..len {
                        let w = w0.wrapping_add(j);
                        o[j] = if jezar { a.jezar(w, o[j]) } else { a.schroeder(w, o[j]) };
                    }
                }
            }
            for j in 0..len {
                [l[j], r[j]] = self.post([o[0][j], o[1][j]], l[j], r[j]);
            }
            return;
        }
        self.finish(&mut o, l, r, w0);
    }

    /// The output chain and post stage over a block of mixed outputs `o`,
    /// writing `l`/`r`. Every chain stage has private state, so the chain
    /// runs a stage at a time.
    #[inline(always)]
    fn finish(&mut self, o: &mut [[f32; BLOCK]; 2], l: &mut [f32], r: &mut [f32], w0: usize) {
        let len = l.len();
        match &mut self.spec.chain {
            Chain::None => {}
            Chain::Schroeder(c) => {
                for (o, c) in o.iter_mut().zip(c.iter_mut()) {
                    for a in c {
                        for j in 0..len {
                            o[j] = a.schroeder(w0.wrapping_add(j), o[j]);
                        }
                    }
                }
            }
            Chain::Jezar(c) => {
                for (o, c) in o.iter_mut().zip(c.iter_mut()) {
                    for a in c {
                        for j in 0..len {
                            o[j] = a.jezar(w0.wrapping_add(j), o[j]);
                        }
                    }
                }
            }
            Chain::Units(c) => {
                for (o, c) in o.iter_mut().zip(c.iter_mut()) {
                    for u in c {
                        for j in 0..len {
                            o[j] = u.tick(w0.wrapping_add(j), o[j]);
                        }
                    }
                }
            }
            Chain::Coupled(c) => {
                for u in c {
                    for j in 0..len {
                        [o[0][j], o[1][j]] = u.tick(w0.wrapping_add(j), [o[0][j], o[1][j]]);
                    }
                }
            }
        }
        for j in 0..len {
            [l[j], r[j]] = self.post([o[0][j], o[1][j]], l[j], r[j]);
        }
    }

    /// The output groups' L/R mix and the direct input.
    #[inline(always)]
    fn mix_out(&self, g: [f32; 2], x: [f32; 2]) -> [f32; 2] {
        let s = &self.spec;
        let o = [s.out[0][0] * g[0] + s.out[0][1] * g[1], s.out[1][0] * g[0] + s.out[1][1] * g[1]];
        if s.direct != 0.0 { [o[0] + s.direct * x[0], o[1] + s.direct * x[1]] } else { o }
    }

    /// Shelf, high cut, width and the dry/wet mix for one frame.
    #[inline(always)]
    fn post(&mut self, mut o: [f32; 2], inl: f32, inr: f32) -> [f32; 2] {
        let s = &self.spec;
        if let Some(sh) = s.shelf {
            for ch in 0..2 {
                let [ic1, ic2] = self.shelf[ch];
                let v3 = o[ch] - ic2;
                let v1 = v3 * sh.a[1] + ic1 * sh.a[0];
                let v2 = sh.a[1] * ic1 + sh.a[2] * v3 + ic2;
                self.shelf[ch] = [2.0 * v1 - ic1, 2.0 * v2 - ic2];
                o[ch] = v1 * sh.m[1] + o[ch] * sh.m[0] + v2 * sh.m[2];
            }
        }
        if let Some(c) = s.high_cut {
            for ch in 0..2 {
                self.cut[ch] = (self.cut[ch] - o[ch]) * c + o[ch];
                o[ch] = self.cut[ch];
            }
        }
        if let Some(width) = s.width {
            let mid = (o[0] + o[1]) * 0.5;
            o = [((o[0] - mid) * width + mid) * s.wet + s.dry * inl, ((o[1] - mid) * width + mid) * s.wet + s.dry * inr];
        }
        o
    }
}

/// One block of one parallel line. A function of its own so the ring, the
/// state and the lanes arrive as non-aliasing references: the ring's fields
/// then stay in registers across the buffer writes.
struct Line<'a> {
    k: &'a Lanes,
    one: f32,
    off: usize,
    tap: TapFrom,
    len: usize,
    w0: usize,
    step0: usize,
    lfo: &'a [[f32; 4]; BLOCK],
    xs: &'a [[f32; 2]; BLOCK],
}

impl Line<'_> {
    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    fn run<const I: u8, const M: u8, const F: bool, const N: usize>(&self, i0: usize, rings: [&mut Ring; N], st: &mut State, v: &mut [f32], glide: &[(f64, f64)], src: &mut [[f32; BLOCK]]) {
        let k = self.k;
        let mut y: [f32; N] = std::array::from_fn(|c| v[c]);
        let mut rings = rings.map(Ring::view);
        let len = self.len;
        // Fixed delays of at least a block (+2 for the widest read) never
        // read what this block writes: read the block's span out, run the
        // filters on arrays, write the span back. Same arithmetic per frame.
        if M == 0 && self.off == 0 && (0..N).all(|c| k.j[i0 + c] >= len + 2 && k.j[i0 + c] + 2 < rings[c].buf.len()) {
            let mut rd = [[0f32; BLOCK]; N];
            for c in 0..N {
                rings[c].read_block::<I>(self.w0, k.j[i0 + c], k.f[i0 + c], &mut rd[c][..len]);
            }
            let (a, b, g) = (std::array::from_fn::<f32, N, _>(|c| k.a[i0 + c]), std::array::from_fn::<f32, N, _>(|c| k.b[i0 + c]), std::array::from_fn::<f32, N, _>(|c| k.g[i0 + c]));
            let (ch, inj): ([usize; N], [f32; N]) = (std::array::from_fn(|c| usize::from(k.ch[i0 + c] & 1)), std::array::from_fn(|c| k.inj[i0 + c]));
            let mut out = [[0f32; BLOCK]; N];
            for j in 0..len {
                let x = self.xs[j];
                for c in 0..N {
                    let i = i0 + c;
                    let r = rd[c][j];
                    y[c] = r * a[c] + b[c] * y[c];
                    let u = y[c] * g[c];
                    let mut z = u;
                    if F {
                        let [b0, b1, b2, a1, a2] = &k.bq;
                        let q = b0[i] * z + st.s0[i];
                        st.s0[i] = b1[i] * z - a1[i] * q + st.s1[i];
                        st.s1[i] = b2[i] * z - a2[i] * q;
                        st.dc[i] += self.one * (q - st.dc[i]);
                        z = q - st.dc[i];
                    }
                    out[c][j] = z + x[ch[c]] * inj[c];
                    src[c][j] = match self.tap {
                        TapFrom::Read => r,
                        TapFrom::Filtered => y[c],
                        TapFrom::Mixed => u,
                    };
                }
            }
            for c in 0..N {
                rings[c].write_block(self.w0, &out[c][..len]);
            }
            v[..N].copy_from_slice(&y);
            return;
        }
        for j in 0..self.len {
            let w = self.w0.wrapping_add(j);
            for c in 0..N {
                let i = i0 + c;
                let (jj, f) = position::<M>(k, i, self.lfo[j], glide[c], (self.step0 + j) as f64);
                let rd = read::<I, _>(&rings[c], w, jj, f);
                y[c] = rd * k.a[i] + k.b[i] * y[c];
                let u = y[c] * k.g[i];
                write::<F>(k, st, self.one, i, &mut rings[c], w, w.wrapping_sub(self.off), u, self.xs[j]);
                src[c][j] = match self.tap {
                    TapFrom::Read => rd,
                    TapFrom::Filtered => y[c],
                    TapFrom::Mixed => u,
                };
            }
        }
        v[..N].copy_from_slice(&y);
    }
}

/// A ring's buffer, mask and skew as plain locals: a store through `buf`
/// cannot alias them, so the hot loop keeps them in registers.
pub(super) struct View<'a> {
    buf: &'a mut [f32],
    bits: u32,
    skew: usize,
}

impl Ring {
    #[inline(always)]
    fn view(&mut self) -> View<'_> {
        View { bits: self.bits, skew: self.skew, buf: &mut self.buf[..1 << self.bits] }
    }
    #[inline(always)]
    fn view_ref(&self) -> &[f32] {
        &self.buf[..1 << self.bits]
    }
}

/// Ring access shared by [`Ring`] and [`View`].
pub(super) trait Tape {
    fn at(&self, w: usize, k: usize) -> f32;
    fn put(&mut self, w: usize, v: f32);
    fn add(&mut self, w: usize, v: f32);
}

impl Tape for Ring {
    #[inline(always)]
    fn at(&self, w: usize, k: usize) -> f32 {
        Ring::at(self, w, k)
    }
    #[inline(always)]
    fn put(&mut self, w: usize, v: f32) {
        Ring::put(self, w, v);
    }
    #[inline(always)]
    fn add(&mut self, w: usize, v: f32) {
        Ring::add(self, w, v);
    }
}

impl View<'_> {
    /// `out[t] = x[w0 + t - k]` (contiguous, wrapping once at most).
    #[inline(always)]
    fn span(&self, w0: usize, k: usize, out: &mut [f32]) {
        let n = self.buf.len();
        let at = w0.wrapping_add(self.skew).wrapping_sub(k) & (n - 1);
        let first = out.len().min(n - at);
        out[..first].copy_from_slice(&self.buf[at..at + first]);
        let rest = out.len() - first;
        out[first..].copy_from_slice(&self.buf[..rest]);
    }
    /// [`read`] for frames `w0..w0 + out.len()` at a fixed `j + f`.
    #[inline(always)]
    fn read_block<const I: u8>(&self, w0: usize, j: usize, f: f32, out: &mut [f32]) {
        let len = out.len();
        self.span(w0, j, out);
        if I == 0 {
            return;
        }
        let mut b = [0f32; BLOCK];
        self.span(w0, j + 1, &mut b[..len]);
        match I {
            1 => out.iter_mut().zip(&b).for_each(|(a, b)| *a = *a + (*b - *a) * f),
            2 => out.iter_mut().zip(&b).for_each(|(a, b)| *a = (1.0 - f) * *a + f * *b),
            _ => {
                let (mut m, mut c) = ([0f32; BLOCK], [0f32; BLOCK]);
                self.span(w0, j - 1, &mut m[..len]);
                self.span(w0, j + 2, &mut c[..len]);
                let (fm, fp, f2) = (f - 1.0, f + 1.0, f - 2.0);
                for t in 0..len {
                    out[t] = -f * fm * f2 / 6.0 * m[t] + fp * fm * f2 / 2.0 * out[t] - fp * f * f2 / 2.0 * b[t] + fp * f * fm / 6.0 * c[t];
                }
            }
        }
    }
    /// `x[w0 + t] = v[t]`.
    #[inline(always)]
    fn write_block(&mut self, w0: usize, v: &[f32]) {
        let n = self.buf.len();
        let at = w0.wrapping_add(self.skew) & (n - 1);
        let first = v.len().min(n - at);
        self.buf[at..at + first].copy_from_slice(&v[..first]);
        let rest = v.len() - first;
        self.buf[..rest].copy_from_slice(&v[first..]);
    }
}

impl Tape for View<'_> {
    #[inline(always)]
    fn at(&self, w: usize, k: usize) -> f32 {
        let n = 1 << self.bits;
        self.buf[..n][w.wrapping_add(self.skew).wrapping_sub(k) & (n - 1)]
    }
    #[inline(always)]
    fn put(&mut self, w: usize, v: f32) {
        let n = 1 << self.bits;
        self.buf[..n][w.wrapping_add(self.skew) & (n - 1)] = v;
    }
    #[inline(always)]
    fn add(&mut self, w: usize, v: f32) {
        let n = 1 << self.bits;
        self.buf[..n][w.wrapping_add(self.skew) & (n - 1)] += v;
    }
}

/// Line `i`'s whole and fractional delay this frame.
#[inline(always)]
fn position<const M: u8>(k: &Lanes, i: usize, lfo: [f32; 4], glide: (f64, f64), step: f64) -> (usize, f32) {
    match M {
        0 => (k.j[i], k.f[i]),
        1 => {
            let d = lfo[0] * k.dm[0][i] + lfo[1] * k.dm[1][i] + k.base[i];
            let whole = d as i32;
            (whole as usize, d - whole as f32)
        }
        3 => {
            let d = lfo[0] * k.dm[0][i] + lfo[1] * k.dm[1][i] + lfo[2] * k.dm[2][i] + lfo[3] * k.dm[3][i] + k.base[i];
            let whole = d as i32;
            (whole as usize, d - whole as f32)
        }
        _ => {
            let d = glide.0 + step * glide.1;
            let j = d as usize;
            (j, (d - j as f64) as f32)
        }
    }
}

/// Read `j + f` frames behind `w`.
#[inline(always)]
fn read<const I: u8, R: Tape>(ring: &R, w: usize, j: usize, f: f32) -> f32 {
    match I {
        0 => ring.at(w, j),
        1 => {
            let (a, b) = (ring.at(w, j), ring.at(w, j + 1));
            a + (b - a) * f
        }
        2 => (1.0 - f) * ring.at(w, j) + f * ring.at(w, j + 1),
        _ => {
            let (m, a, b, c) = (ring.at(w, j.wrapping_sub(1)), ring.at(w, j), ring.at(w, j + 1), ring.at(w, j + 2));
            let (fm, fp, f2) = (f - 1.0, f + 1.0, f - 2.0);
            -f * fm * f2 / 6.0 * m + fp * fm * f2 / 2.0 * a - fp * f * f2 / 2.0 * b + fp * f * fm / 6.0 * c
        }
    }
}

/// Line `i`'s write: the optional biquad and DC blocker, then the input
/// injection at the head or at `at`.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn write<const F: bool>(k: &Lanes, st: &mut State, one: f32, i: usize, ring: &mut impl Tape, w: usize, at: usize, mut y: f32, x: [f32; 2]) {
    if F {
        let [b0, b1, b2, a1, a2] = &k.bq;
        let z = b0[i] * y + st.s0[i];
        st.s0[i] = b1[i] * y - a1[i] * z + st.s1[i];
        st.s1[i] = b2[i] * y - a2[i] * z;
        st.dc[i] += one * (z - st.dc[i]);
        y = z - st.dc[i];
    }
    let inj = x[usize::from(k.ch[i] & 1)] * k.inj[i];
    if at == w {
        ring.put(w, y + inj);
    } else {
        ring.put(w, y);
        ring.add(at, inj);
    }
}

/// `u = M·v` over the first `n` lines.
#[inline(always)]
pub(super) fn apply(matrix: &Matrix, v: &[f32; MAX_LINES], n: usize, u: &mut [f32; MAX_LINES]) {
    match matrix {
        Matrix::Permutation { p, g } => {
            for i in 0..n {
                u[i] = v[usize::from(p[i])] * g[i];
            }
        }
        Matrix::Hadamard => {
            // Whole-array copy: a fixed size inlines (a `[..n]` copy calls memcpy).
            *u = *v;
            hadamard(&mut u[..n]);
        }
        Matrix::Circulant { row, col, mean, half } => {
            let mut t = [0f32; 32];
            let mut sum = 0.0;
            for j in 0..31 {
                t[usize::from(col[j]) & 31] = v[j];
                sum += v[j];
            }
            hadamard(&mut t);
            let m = mean * sum;
            for i in 0..31 {
                u[i] = m + half * t[usize::from(row[i]) & 31];
            }
        }
        Matrix::Dense(m) => {
            for i in 0..n {
                u[i] = (0..n).map(|j| m[i * n + j] * v[j]).sum();
            }
        }
    }
}

#[inline(always)]
fn hadamard_n<const N: usize>(x: &mut [f32; N]) {
    let mut h = 1;
    while h < N {
        for i in (0..N).step_by(2 * h) {
            for j in i..i + h {
                let (a, b) = (x[j], x[j + h]);
                (x[j], x[j + h]) = (a + b, a - b);
            }
        }
        h *= 2;
    }
}

/// In-place unnormalised Walsh–Hadamard transform (power-of-two length).
#[inline(always)]
pub(super) fn hadamard(x: &mut [f32]) {
    // Fixed sizes unroll into vector code.
    match x.len() {
        8 => return hadamard_n::<8>(x.try_into().expect("len 8")),
        16 => return hadamard_n::<16>(x.try_into().expect("len 16")),
        32 => return hadamard_n::<32>(x.try_into().expect("len 32")),
        _ => {}
    }
    let mut h = 1;
    while h < x.len() {
        for i in (0..x.len()).step_by(2 * h) {
            for j in i..i + h {
                let (a, b) = (x[j], x[j + h]);
                (x[j], x[j + h]) = (a + b, a - b);
            }
        }
        h *= 2;
    }
}

#[cfg(test)]
mod tests {
    /// The parallel-lines path is bit-identical to the per-frame path.
    #[test]
    fn parallel_lines_match_the_frame_path() {
        let mut seed = 1u32;
        let mut noise = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1u32 << 23) as f32 - 1.0
        };
        let input: Vec<[f32; 2]> = (0..3000).map(|_| [noise(), noise()]).collect();
        let mut checked = 0;
        for (name, net) in super::super::tests::presets() {
            let spec = super::super::design::spec(&net, 48_000).unwrap();
            let (mut a, mut b) = (super::Network::new(spec.clone()), super::Network::new(spec));
            if !a.parallel {
                continue;
            }
            b.parallel = false;
            for chunk in input.chunks(37) {
                let (mut l, mut r): (Vec<f32>, Vec<f32>) = chunk.iter().map(|f| (f[0], f[1])).unzip();
                let (mut l2, mut r2) = (l.clone(), r.clone());
                a.process(&mut l, &mut r);
                b.process(&mut l2, &mut r2);
                let bits = |v: &[f32]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
                assert_eq!((bits(&l), bits(&r)), (bits(&l2), bits(&r2)), "{name}");
            }
            checked += 1;
        }
        assert!(checked >= 3, "{checked}");
    }
}
