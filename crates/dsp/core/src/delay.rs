//! Canonical RT-safe delay lines (`DelayLine` power-of-two ring and
//! `FixedRing` exact-length ring) plus Hermite (Catmull-Rom) fractional
//! interpolation shared across DSP families.

use crate::flush_f32;

/// 4-point cubic Hermite (Catmull-Rom) interpolation at `t in [0, 1]` between `y0` and `y1`.
#[inline]
#[must_use]
pub fn hermite(ym1: f32, y0: f32, y1: f32, y2: f32, t: f32) -> f32 {
    let c0 = y0;
    let c1 = 0.5 * (y1 - ym1);
    let c2 = ym1 - 2.5 * y0 + 2.0 * y1 - 0.5 * y2;
    let c3 = 0.5 * (y2 - ym1) + 1.5 * (y0 - y1);
    ((c3 * t + c2) * t + c1) * t + c0
}

/// Power-of-two mono delay line allocated once on construction, panic-free in audio paths.
///
/// Supports both self-indexed streaming (`push`, `read_int`, `read_linear`, `read_cubic`,
/// `set_len`/`read_len`/`read_late`) and caller-shared write-head indexing
/// (`write_at`, `read_at`, `read_linear_at`) for multi-line comb/Hadamard banks.
#[derive(Clone, Debug, PartialEq)]
pub struct DelayLine {
    buf: Box<[f32]>,
    mask: usize,
    write: usize,
    len: usize,
}

impl DelayLine {
    /// Allocates a power-of-two ring buffer holding at least `min_frames.max(4)` samples.
    #[must_use]
    pub fn new(min_frames: usize) -> Self {
        let max = min_frames.max(1);
        let cap = (max + 2).max(4).next_power_of_two();
        Self {
            buf: vec![0.0_f32; cap].into_boxed_slice(),
            mask: cap - 1,
            write: 0,
            len: max,
        }
    }

    /// Allocates the smallest power-of-two ring buffer of at least `min_len.max(2)` frames
    /// without extra slack (preserving exact wrap-around semantics when needed).
    #[must_use]
    pub fn exact_pow2(min_len: usize) -> Self {
        let cap = min_len.next_power_of_two().max(2);
        Self {
            buf: vec![0.0_f32; cap].into_boxed_slice(),
            mask: cap - 1,
            write: 0,
            len: min_len.min(cap - 1),
        }
    }

    #[inline]
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.buf.len()
    }

    /// Underlying power-of-two buffer slice for vectorized span reads.
    #[inline]
    #[must_use]
    pub fn as_slice(&self) -> &[f32] {
        &self.buf
    }

    fn run_mut(&mut self, w: usize, len: usize) -> (&mut [f32], &mut [f32]) {
        let start = w & self.mask;
        let (wrapped, from) = self.buf.split_at_mut(start);
        let head = len.min(from.len());
        let tail = wrapped.get_mut(..len - head).unwrap_or_default();
        (from.get_mut(..head).unwrap_or_default(), tail)
    }

    /// Writes `vals` sequentially starting at external cursor `w`, wrapping at capacity.
    #[inline]
    pub fn write_slice_at(&mut self, w: usize, vals: &[f32]) {
        let (head, tail) = self.run_mut(w, vals.len());
        let (a, b) = vals.split_at(head.len());
        head.copy_from_slice(a);
        tail.copy_from_slice(b);
    }

    /// Accumulates `vals * gain` sequentially starting at external cursor `w`.
    #[inline]
    pub fn add_slice_at(&mut self, w: usize, vals: &[f32], gain: f32) {
        let (head, tail) = self.run_mut(w, vals.len());
        let (a, b) = vals.split_at(head.len());
        for (dst, src) in [(head, a), (tail, b)] {
            for (d, &s) in dst.iter_mut().zip(src) {
                *d += s * gain;
            }
        }
    }

    /// Maximum single-tap reach (`capacity - 1`).
    #[inline]
    #[must_use]
    pub fn reach(&self) -> usize {
        self.mask
    }

    /// Active read length for `read_len`/`read_late`.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Sets the active delay length (`0..=capacity - 2`). Returns `true` if it increased.
    #[inline]
    pub fn set_len(&mut self, len: usize) -> bool {
        let next = len.min(self.buf.len().saturating_sub(2));
        let grew = next > self.len;
        self.len = next;
        grew
    }

    /// Writes `sample` at the current head and advances.
    #[inline]
    pub fn push(&mut self, sample: f32) {
        if let Some(slot) = self.buf.get_mut(self.write) {
            *slot = flush_f32(sample);
        }
        self.write = (self.write + 1) & self.mask;
    }

    /// Writes `sample` at an external shared write cursor `w`.
    #[inline]
    pub fn write_at(&mut self, w: usize, sample: f32) {
        if let Some(slot) = self.buf.get_mut(w & self.mask) {
            *slot = sample;
        }
    }

    /// Accumulates `sample` into the ring slot at external write cursor `w`.
    #[inline]
    pub fn add_at(&mut self, w: usize, sample: f32) {
        if let Some(slot) = self.buf.get_mut(w & self.mask) {
            *slot += sample;
        }
    }

    /// Reads the sample `k` frames behind an external write cursor `w`.
    #[inline]
    #[must_use]
    pub fn read_offset_at(&self, w: usize, k: usize) -> f32 {
        self.buf
            .get(w.wrapping_sub(k) & self.mask)
            .copied()
            .unwrap_or(0.0)
    }

    /// Reads the sample `self.len` frames behind an external write cursor `w`.
    #[inline]
    #[must_use]
    pub fn read_at(&self, w: usize) -> f32 {
        self.read_offset_at(w, self.len)
    }

    /// Reads linearly `delay` frames behind an external write cursor `w`.
    #[inline]
    #[must_use]
    pub fn read_linear_at(&self, w: usize, delay: f32) -> f32 {
        let whole = (delay as i32).max(0);
        let t = delay - whole as f32;
        let i = whole as usize;
        let a = self.read_offset_at(w, i);
        let b = self.read_offset_at(w, i + 1);
        (1.0 - t) * a + t * b
    }

    /// Reads the sample written `self.len` frames before `self.write`.
    #[inline]
    #[must_use]
    pub fn read_len(&self) -> f32 {
        self.buf
            .get(self.write.wrapping_sub(self.len) & self.mask)
            .copied()
            .unwrap_or(0.0)
    }

    /// Reads `self.len + offset` (`offset >= 0`) frames before `self.write`, linearly interpolated.
    #[inline]
    #[must_use]
    pub fn read_late(&self, offset: f32) -> f32 {
        let max_back = self.buf.len().saturating_sub(2) as f32;
        let back = (self.len as f32 + offset).min(max_back);
        let whole = back as usize;
        let t = back - whole as f32;
        let i0 = self.write.wrapping_sub(whole) & self.mask;
        let i1 = i0.wrapping_sub(1) & self.mask;
        let a = self.buf.get(i0).copied().unwrap_or(0.0);
        let b = self.buf.get(i1).copied().unwrap_or(0.0);
        a + t * (b - a)
    }

    /// Reads an integer tap `delay` frames behind the most recently written sample.
    #[inline]
    #[must_use]
    pub fn read_int(&self, delay: usize) -> f32 {
        let d = delay.min(self.mask);
        let idx = self.write.wrapping_sub(1 + d) & self.mask;
        self.buf.get(idx).copied().unwrap_or(0.0)
    }

    /// Reads a linearly-interpolated tap `delay` frames behind the most recently written sample.
    #[inline]
    #[must_use]
    pub fn read_linear(&self, delay: f32) -> f32 {
        let max_d = (self.mask.saturating_sub(1)) as f32;
        let d = delay.clamp(0.0, max_d);
        let i = d.floor() as usize;
        let f = d - i as f32;
        let a = self.read_int(i);
        let b = self.read_int(i + 1);
        a + (b - a) * f
    }

    /// Reads a Catmull-Rom cubic-interpolated tap `delay` frames behind the write head.
    #[inline]
    #[must_use]
    pub fn read_cubic(&self, delay: f32) -> f32 {
        let max_d = (self.mask.saturating_sub(3)) as f32;
        let d = delay.clamp(1.0, max_d.max(1.0));
        let i = d.floor() as usize;
        let f = d - i as f32;
        let ym1 = self.read_int(i.saturating_sub(1));
        let y0 = self.read_int(i);
        let y1 = self.read_int(i + 1);
        let y2 = self.read_int(i + 2);
        hermite(ym1, y0, y1, y2, f)
    }

    pub fn reset(&mut self) {
        self.buf.fill(0.0);
        self.write = 0;
    }
}

/// Exact-length circular delay line (`read()` returns oldest sample,
/// `write_advance(x)` overwrites it and advances).
#[derive(Clone, Debug, PartialEq)]
pub struct FixedRing {
    buf: Box<[f32]>,
    pos: usize,
}

impl FixedRing {
    #[must_use]
    pub fn new(len: usize) -> Self {
        Self {
            buf: vec![0.0_f32; len.max(1)].into_boxed_slice(),
            pos: 0,
        }
    }

    #[inline]
    #[must_use]
    pub fn read(&self) -> f32 {
        self.buf.get(self.pos).copied().unwrap_or(0.0)
    }

    #[inline]
    pub fn write_advance(&mut self, x: f32) {
        if let Some(slot) = self.buf.get_mut(self.pos) {
            *slot = flush_f32(x);
        }
        self.pos += 1;
        if self.pos >= self.buf.len() {
            self.pos = 0;
        }
    }

    pub fn reset(&mut self) {
        self.buf.fill(0.0);
        self.pos = 0;
    }
}
