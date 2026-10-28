//! A translator's reverb network ([`ReverbNetwork`]): 31 prime-length lines
//! in a circulant feedback matrix, damped on the read, tapped into two
//! allpass-unit chains, then a low shelf, a high cut and the width.

use crate::Settings;
use crate::fade::flush;
use crate::fdn::{MAX_PREDELAY_MS, MAX_SIZE};
use crate::resample::{BLOCK, Pair};
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

/// The controls that glide, by index: wet level, decay seconds, damping,
/// modulation depth (frames), unit gain, predelay (frames), the left
/// predelay's line offset (frames), high cut (Hz), low shelf (dB), width.
const WET: usize = 0;
const SECONDS: usize = 1;
const DAMPING: usize = 2;
const DEPTH: usize = 3;
const GAIN: usize = 4;
const PREDELAY: usize = 5;
const OFFSET: usize = 6;
const CUT: usize = 7;
const SHELF: usize = 8;
const WIDTH: usize = 9;
const CONTROLS: usize = 10;

/// Values gliding linearly to a target over a fixed number of frames; a new
/// target starts again from where they are.
struct Glide<const N: usize> {
    now: [f32; N],
    start: [f32; N],
    target: [f32; N],
    phase: f32,
    step: f32,
}

impl<const N: usize> Glide<N> {
    fn new(v: [f32; N], frames: f32) -> Self {
        Self {
            now: v,
            start: v,
            target: v,
            phase: 1.0,
            step: 1.0 / frames.max(1.0),
        }
    }

    fn to(&mut self, target: [f32; N]) {
        (self.start, self.target) = (self.now, target);
        self.phase = if target == self.now { 1.0 } else { 0.0 };
    }

    fn moving(&self) -> bool {
        self.phase < 1.0
    }

    /// Whether value `i` is on its way.
    fn moves(&self, i: usize) -> bool {
        self.moving() && self.start.get(i) != self.target.get(i)
    }

    /// One frame on; `true` on the frame it arrives.
    fn tick(&mut self) -> bool {
        if !self.moving() {
            return false;
        }
        self.phase += self.step;
        if self.phase >= 1.0 {
            self.now = self.target;
            return true;
        }
        for ((v, a), b) in self.now.iter_mut().zip(self.start).zip(self.target) {
            *v = a + (b - a) * self.phase;
        }
        false
    }
}

pub struct Network {
    law: &'static ReverbNetwork,
    requested: Settings,
    controls: Glide<CONTROLS>,
    lengths: Glide<LINES>,
    /// Frames since the network started, for the coarser recomputes.
    ticks: u32,
    /// Above the law's highest rate, the network runs between this pair.
    pair: Option<Pair>,
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
        let rate = rate.max(1);
        let n = (rate as f32 * (1.0 / law.rate)) as u32;
        let pair = (rate as f32 > law.max_rate && n > 1 && rate.is_multiple_of(n))
            .then(|| Pair::new(n as usize));
        // The rate the lines run at.
        let host = if pair.is_some() { rate / n } else { rate } as f32;
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
        let [glide, glide_lines] = law.glide_s.map(|t| t * host);
        let mut n = Self {
            law,
            requested: s,
            controls: Glide::new([0.0; CONTROLS], glide),
            lengths: Glide::new([0.0; LINES], glide_lines),
            ticks: 0,
            pair,
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
        for (i, line) in n.lines.iter_mut().enumerate() {
            line.sin = i % 2 == 0;
        }
        for (ch, taps) in law.inputs.iter().enumerate() {
            for &(i, sign) in taps.iter().filter(|t| t.1 != 0.0) {
                if let Some(line) = n.lines.get_mut(usize::from(i)) {
                    (line.right, line.inject) = (ch == 1, sign);
                }
            }
        }
        let (controls, lengths) = n.targets(s);
        n.controls.now = controls;
        n.lengths.now = lengths;
        n.controls.to(controls);
        n.lengths.to(lengths);
        n.arrive();
        n
    }

    /// Changes every setting while it plays: the input point and the output
    /// taps at once, the rest by the law's glides. Never allocates.
    pub fn set(&mut self, s: Settings) {
        if s == self.requested {
            return;
        }
        self.requested = s;
        let (controls, lengths) = self.targets(s);
        self.controls.to(controls);
        self.lengths.to(lengths);
    }

    /// The glide targets for `s`; sets the input point and the output taps.
    fn targets(&mut self, s: Settings) -> ([f32; CONTROLS], [f32; LINES]) {
        let law = self.law;
        let rate = law.rate;
        let len = lengths(law, &self.primes, s.size.clamp(0.0, MAX_SIZE));
        let [len0, len1] = [len[0], len[1]];
        // ponytail: the input point moves at once, as the native's integer
        // lengths do (+0x6a90); the lines glide behind it.
        self.inject_offset = len0.saturating_sub(law.inject_lead) as usize;
        let t = s.seconds.max(0.01);
        let g = s.diffusion.clamp(0.0, 1.0) * 0.5;
        let w = g + g;
        // ponytail: the taps take a new diffusion at once; the native blends
        // them only in its full recompute (0x141aec0a0), not while it ramps.
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
        let [l0, l1] = law.level;
        let mut c = [0.0; CONTROLS];
        c[WET] = (1.0 - 10f32.powf(-l0 / t)).sqrt() * l1;
        c[SECONDS] = t;
        c[DAMPING] = s.damping.clamp(0.0, 1.0);
        c[DEPTH] = s.modulation_ms.max(0.0) * 0.001 * rate;
        c[GAIN] = g;
        c[PREDELAY] = s.predelay_ms.clamp(0.0, MAX_PREDELAY_MS) * 0.001 * rate;
        c[OFFSET] = (i64::from(len1) - i64::from(len0)) as f32;
        c[CUT] = s.high_cut_hz;
        c[SHELF] = s.low_shelf_db;
        c[WIDTH] = s.stereo.clamp(0.0, 1.0);
        (c, len.map(|l| l as f32))
    }

    /// The glides one frame on, and what follows them, as the native
    /// ramping kernel (0x141ad4f20) does: the filters every frame, the units
    /// every 8 and the line decay every 32 frames while theirs move.
    fn glide(&mut self) {
        let k = self.ticks;
        self.ticks = k.wrapping_add(1);
        let arrived = self.controls.tick() | self.lengths.tick();
        if arrived {
            return self.arrive();
        }
        let c = &self.controls;
        let filters = c.moves(CUT) || c.moves(SHELF);
        let units = k.is_multiple_of(8) && c.moves(GAIN);
        let decay =
            k.is_multiple_of(32) && (c.moves(SECONDS) || c.moves(DAMPING) || self.lengths.moving());
        if filters {
            self.filters();
        }
        if units {
            self.units();
        }
        if decay {
            self.decay();
        }
        self.follow();
    }

    /// Everything at the glides' current values.
    fn arrive(&mut self) {
        self.filters();
        self.units();
        self.decay();
        self.follow();
    }

    /// Each line's damping one-pole and loop gain: 60 dB in the decay
    /// seconds (frames at the law's rate), the highs in `t_hf`.
    fn decay(&mut self) {
        let law = self.law;
        let c = self.controls.now;
        let t = c[SECONDS];
        let [h0, h1, h2] = law.hf;
        let damp = c[DAMPING];
        let l = (t + t).ln();
        let t_hf = ((1.0 - damp * h0) * (1.0 - l * h2) + (1.0 - damp * h1) * l * h2) * t;
        let k = -3.0 / law.rate;
        let base = 10f32.powf(k / t);
        let ratio = 10f32.powf(k / t_hf) / base;
        for (line, f) in self.lines.iter_mut().zip(self.lengths.now) {
            let p = ratio.powf(f);
            let r = 1.0 / (p * p);
            // The one-pole whose highs fall `r` times the lows per pass.
            let a = if r == 1.0 {
                0.0
            } else {
                ((r * 2.962_758 - 0.768_274_2).sqrt() - (r + 0.481_379_06)) / (1.0 - r)
            };
            (line.a, line.b) = ((1.0 - a) * base.powf(f), a);
        }
    }

    /// The allpass units at the current gain.
    fn units(&mut self) {
        let g = self.controls.now[GAIN];
        let c = self.law.unit_feedback;
        for chain in &mut self.units {
            for (k, u) in chain.iter_mut().enumerate() {
                let p = if k % 2 == 0 { -g } else { g };
                let (cp, cp2) = (c * p, p * c * p);
                u.m = [-cp2, -cp2, cp, 1.0 - cp2, -cp2, cp, 0.0, 1.0 - cp2, cp];
                (u.b, u.q, u.r) = ([-p, -p, 1.0], -p, 1.0 - p * c * p);
            }
        }
    }

    /// The low shelf and the high cut at their current settings.
    fn filters(&mut self) {
        let c = self.controls.now;
        let rate = f64::from(self.law.rate);
        let f = (f64::from(self.law.shelf_hz) * PI / rate) as f32;
        (self.shelf_m, self.shelf_a) = shelf(c[SHELF], f);
        self.cut = (f64::from(c[CUT] * -2.0) * (PI / rate)).exp() as f32;
    }

    /// The per-frame values: line lengths, modulation depth, predelay,
    /// width and level.
    fn follow(&mut self) {
        let c = self.controls.now;
        for (i, (line, f)) in self.lines.iter_mut().zip(self.lengths.now).enumerate() {
            line.base = f;
            line.depth = if i == 0 { 0.0 } else { c[DEPTH] };
        }
        self.predelay = [c[PREDELAY] + c[OFFSET], c[PREDELAY]];
        self.width = c[WIDTH];
        self.wet = c[WET];
    }

    /// Replaces `io` with the reverb (the wet path only).
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        match self.pair.take() {
            Some(mut pair) => {
                for chunk in io.chunks_mut(BLOCK) {
                    pair.process(chunk, |low| self.run(low));
                }
                self.pair = Some(pair);
            }
            None => self.run(io),
        }
    }

    fn run(&mut self, io: &mut [[f32; 2]]) {
        for frame in io {
            if self.controls.moving() || self.lengths.moving() {
                self.glide();
            }
            *frame = self.frame(*frame);
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
