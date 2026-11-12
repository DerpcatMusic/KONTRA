//! The network a span of frames at a time: each line reads, damps and
//! writes its run of frames in turn, and the feedback transform runs across
//! the frames. Every value is computed by the same operations, in the same
//! order, as frame by frame; only the order of independent work changes.

use super::{Line, Network, Ring, Unit};
use crate::fade::flush;

/// The longest span; its scratch lives on the stack.
pub(super) const MAX: usize = 64;

impl Line {
    /// The longest span this line can run with no read seeing a value put
    /// in the same span: every read reaches past the span's start, and none
    /// past the ring's end, where it wraps onto frames just written (the
    /// native kernel's rings are as short, so it hears them).
    fn span(&self) -> usize {
        // The phasor's swing stays within ±1 (a rotation), with margin; a
        // read touches the frame `d` back and the one before it.
        let swing = self.depth.abs() * 1.01 + 1.0;
        let nearest = (self.base - swing).floor();
        let farthest = (self.base + swing).ceil() + 1.0;
        if nearest < 1.0 || farthest >= self.ring.buf.len() as f32 {
            1
        } else {
            nearest as usize
        }
    }
}

impl Network {
    /// How many frames may run as one span now: within every line's nearest
    /// read, and within the input's offset so no later put lands on an
    /// earlier frame's input.
    pub(super) fn span(&self) -> usize {
        (self.lines.iter())
            .map(Line::span)
            .fold(MAX.min(self.inject_offset.max(1)), usize::min)
    }

    /// `io` (at most [`MAX`] frames, within [`Self::span`]) as
    /// [`Self::frame`] would leave it, frame after frame.
    pub(super) fn run_span(&mut self, io: &mut [[f32; 2]]) {
        let n = io.len().min(MAX);
        let w0 = self.w;
        self.w = w0.wrapping_add(n);
        let mut lfo = [[0f32; 2]; MAX];
        let mut x = [[0f32; 2]; MAX];
        for (f, ((lfo, x), input)) in lfo.iter_mut().zip(&mut x).zip(io.iter()).enumerate() {
            let (c, s) = self.phasor;
            let (cw, sw) = self.rot;
            self.phasor = (c * cw - sw * s, c * sw + s * cw);
            *lfo = [self.phasor.0 as f32, self.phasor.1 as f32];
            let w = w0.wrapping_add(f);
            for ((x, ring), (&v, &d)) in
                (x.iter_mut().zip(&mut self.pre)).zip(input.iter().zip(&self.predelay))
            {
                ring.put(w, v);
                *x = ring.weighted(w, d);
            }
        }
        // Each line's reads, the input added behind them as a frame adds
        // it after its reads; then the damping, four lines side by side.
        let swing = [lfo.map(|l| l[0]), lfo.map(|l| l[1])];
        let input = [x.map(|x| x[0]), x.map(|x| x[1])];
        let at = w0.wrapping_sub(self.inject_offset);
        let mut v = [[0f32; MAX]; 31];
        for (line, read) in self.lines.iter_mut().zip(&mut v) {
            let swing = swing.get(usize::from(line.sin)).unwrap_or(&[0.0; MAX]);
            let x = input.get(usize::from(line.right)).unwrap_or(&[0.0; MAX]);
            line.read(w0, n, swing, read);
            if line.inject != 0.0 {
                if line.reads_own_input(self.inject_offset, n) {
                    line.read_with_input(w0, n, swing, (at, x), read);
                } else {
                    line.ring
                        .add_run(at, x.get(..n).unwrap_or_default(), line.inject);
                }
            }
        }
        for (lines, reads) in self.lines.chunks_mut(4).zip(v.chunks_mut(4)) {
            damp(lines, reads, n);
        }
        let mut t = [[0f32; MAX]; 32];
        let mut sum = [0f32; MAX];
        for (out, &col) in v.iter().zip(&self.col) {
            if let Some(slot) = t.get_mut(usize::from(col) & 31) {
                *slot = *out;
            }
            for (s, v) in sum.iter_mut().zip(out) {
                *s += v;
            }
        }
        hadamard(&mut t, n);
        // Feed the transform back.
        let mut peak = [self.written; 8];
        let mut u = [0f32; MAX];
        for (line, &row) in self.lines.iter_mut().zip(&self.row) {
            let t = t.get(usize::from(row) & 31).unwrap_or(&[0.0; MAX]);
            for (u, (&s, &t)) in u.iter_mut().zip(sum.iter().zip(t)) {
                *u = flush(self.mean * s + self.half * t);
            }
            for chunk in u.get(..n).unwrap_or_default().chunks(8) {
                for (p, &u) in peak.iter_mut().zip(chunk) {
                    if u.abs() > *p {
                        *p = u.abs();
                    }
                }
            }
            line.ring.put_run(w0, u.get(..n).unwrap_or_default());
        }
        self.written = peak.into_iter().fold(0.0, |m, p| if p > m { p } else { m });
        // The taps, then each allpass unit over the span in turn (a unit's
        // frames stay in order; units meet only at the same frame), then
        // the output filters frame by frame.
        let mut o = self.taps.map(|taps| {
            let mut acc = [0f32; MAX];
            for (i, c) in taps {
                let line = v.get(usize::from(i)).unwrap_or(&[0.0; MAX]);
                for (acc, &v) in acc.iter_mut().zip(line) {
                    *acc += c * v;
                }
            }
            acc
        });
        let mut written = self.written;
        for (o, chain) in o.iter_mut().zip(&mut self.units) {
            for u in chain.iter_mut().take(self.unit_count) {
                u.run(w0, n, o, &mut written);
            }
        }
        self.written = written;
        for (f, frame) in io.iter_mut().take(n).enumerate() {
            let at = |o: &[f32; MAX]| o.get(f).copied().unwrap_or(0.0);
            *frame = self.post([at(&o[0]), at(&o[1])]);
        }
    }
}

impl Unit {
    /// [`Self::tick`] on `n` frames from `w0`, `x` in and out. While every
    /// delay outlasts the span, each frame's state is read before the span
    /// and the frames run side by side.
    fn run(&mut self, w0: usize, n: usize, x: &mut [f32; MAX], written: &mut f32) {
        if self.d.iter().any(|&d| d < n) {
            for (f, x) in x.iter_mut().take(n).enumerate() {
                *x = self.tick(w0.wrapping_add(f), *x, written);
            }
            return;
        }
        let mut z = [[0f32; MAX]; 3];
        for ((z, ring), (&d, &z0)) in z
            .iter_mut()
            .zip(&self.rings)
            .zip(self.d.iter().zip(&self.z))
        {
            for (f, z) in z.iter_mut().enumerate().take(n) {
                *z = if f == 0 {
                    z0
                } else {
                    ring.at(w0.wrapping_add(f), d)
                };
            }
        }
        let [m0, m1, m2, m3, m4, m5, m6, m7, m8] = self.m;
        let [b0, b1, b2] = self.b;
        let cols = [[m0, m3, m6, b0], [m1, m4, m7, b1], [m2, m5, m8, b2]];
        let [z0, z1, z2] = &z;
        let mut peak = [*written; 8];
        let mut v = [0f32; MAX];
        for (ring, [a, b, c, e]) in self.rings.iter_mut().zip(cols) {
            for (v, (((&z0, &z1), &z2), &x)) in
                v.iter_mut().zip(z0.iter().zip(z1).zip(z2).zip(x.iter()))
            {
                *v = flush(a * z0 + b * z1 + c * z2 + e * x);
            }
            let v = v.get(..n).unwrap_or_default();
            for chunk in v.chunks(8) {
                for (p, &v) in peak.iter_mut().zip(chunk) {
                    *p = p.max(v.abs());
                }
            }
            ring.put_run(w0, v);
        }
        *written = peak.into_iter().fold(*written, f32::max);
        let (q, r) = (self.q, self.r);
        for (x, &z0) in x.iter_mut().zip(z0) {
            *x = *x * q + z0 * r;
        }
        let end = w0.wrapping_add(n);
        for ((z, ring), &d) in self.z.iter_mut().zip(&self.rings).zip(&self.d) {
            *z = ring.at(end, d);
        }
    }
}

impl Ring {
    /// The ring's stretch from `w` for `len` frames (`len` within the
    /// ring), as the run up to its end and the rest from its start.
    fn run_mut(&mut self, w: usize, len: usize) -> (&mut [f32], &mut [f32]) {
        let start = w & self.mask;
        let (wrapped, from) = self.buf.split_at_mut(start);
        let head = len.min(from.len());
        let tail = wrapped.get_mut(..len - head).unwrap_or_default();
        (from.get_mut(..head).unwrap_or_default(), tail)
    }

    /// `vals` put at `w`, `w + 1`, …, as [`Ring::put`] one by one.
    fn put_run(&mut self, w: usize, vals: &[f32]) {
        let (head, tail) = self.run_mut(w, vals.len());
        let (a, b) = vals.split_at(head.len());
        for (s, v) in [(head, a), (tail, b)] {
            for (s, &v) in s.iter_mut().zip(v) {
                *s = v;
            }
        }
    }

    /// `vals · gain` added at `w`, `w + 1`, …, as [`Ring::add`] one by one.
    fn add_run(&mut self, w: usize, vals: &[f32], gain: f32) {
        let (head, tail) = self.run_mut(w, vals.len());
        let (a, b) = vals.split_at(head.len());
        for (s, v) in [(head, a), (tail, b)] {
            for (s, &v) in s.iter_mut().zip(v) {
                *s += v * gain;
            }
        }
    }
}

impl Line {
    /// The undamped reads of `n` frames from `w0`, as [`Ring::weighted`]
    /// at `swing·depth + base`.
    fn read(&self, w0: usize, n: usize, swing: &[f32; MAX], out: &mut [f32; MAX]) {
        let (depth, base, mask) = (self.depth, self.base, self.ring.mask);
        let d = swing.map(|s| s * depth + base);
        let i = d.map(|d| (d as i32).max(0));
        for (f, (out, (&d, &i))) in out.iter_mut().zip(d.iter().zip(&i)).take(n).enumerate() {
            let frac = d - i as f32;
            let at = w0.wrapping_add(f).wrapping_sub(i as usize);
            let x0 = self.ring.buf.get(at & mask).copied().unwrap_or(0.0);
            let x1 = self
                .ring
                .buf
                .get(at.wrapping_sub(1) & mask)
                .copied()
                .unwrap_or(0.0);
            *out = (1.0 - frac) * x0 + frac * x1;
        }
    }

    /// Whether a read in a span of `n` frames may reach an input added
    /// earlier in the same span, `offset` frames behind its frame.
    fn reads_own_input(&self, offset: usize, n: usize) -> bool {
        let swing = self.depth.abs() * 1.01 + 1.0;
        let farthest = (self.base + swing).ceil() + 1.0;
        let nearest = (self.base - swing).floor();
        farthest > offset as f32 && nearest < (offset + n) as f32
    }

    /// [`Self::read`] with the input `x` added at `at + f` after frame
    /// `f`'s read, as frame by frame.
    fn read_with_input(
        &mut self,
        w0: usize,
        n: usize,
        swing: &[f32; MAX],
        (at, x): (usize, &[f32; MAX]),
        out: &mut [f32; MAX],
    ) {
        let frames = out.iter_mut().zip(swing.iter().zip(x)).take(n).enumerate();
        for (f, (out, (&swing, &x))) in frames {
            *out = self
                .ring
                .weighted(w0.wrapping_add(f), swing * self.depth + self.base);
            self.ring.add(at.wrapping_add(f), x * self.inject);
        }
    }
}

/// The damping of up to four lines' reads in place, the lines' recursions
/// side by side.
fn damp(lines: &mut [Line], reads: &mut [[f32; MAX]], n: usize) {
    let mut held = [0f32; 4];
    let mut a = [0f32; 4];
    let mut b = [0f32; 4];
    for (((h, a), b), line) in held.iter_mut().zip(&mut a).zip(&mut b).zip(lines.iter()) {
        (*h, *a, *b) = (line.v, line.a, line.b);
    }
    for f in 0..n {
        for (((h, a), b), read) in held.iter_mut().zip(a).zip(b).zip(reads.iter_mut()) {
            if let Some(r) = read.get_mut(f) {
                *h = *r * a + b * *h;
                *r = *h;
            }
        }
    }
    for (line, h) in lines.iter_mut().zip(held) {
        line.v = h;
    }
}

/// [`super::hadamard`] on each of the first `n` frames at once.
fn hadamard(x: &mut [[f32; MAX]; 32], n: usize) {
    for h in [1, 2, 4, 8, 16] {
        for pair in x.chunks_exact_mut(2 * h) {
            let (lo, hi) = pair.split_at_mut(h);
            for (a, b) in lo.iter_mut().zip(hi) {
                for (a, b) in a.iter_mut().zip(b.iter_mut()).take(n) {
                    (*a, *b) = (*a + *b, *a - *b);
                }
            }
        }
    }
}
