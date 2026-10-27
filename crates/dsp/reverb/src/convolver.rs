//! Zero-latency convolution: a direct FIR head plus a uniform partitioned FFT tail.

use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};

/// Head length and FFT block size, in frames.
const BLOCK: usize = 64;
/// Inputs below this magnitude count as silence.
const SILENT: f32 = 1e-10;

type Spectrum = Vec<Complex<f32>>;

/// One channel: a direct FIR over the first `BLOCK` taps, and an FFT tail for the rest.
struct Mono {
    head: Vec<f32>,
    /// Last `BLOCK` inputs, stored twice so the window `pos..pos + BLOCK` is contiguous.
    hist: Vec<f32>,
    pos: usize,
    /// `[previous block, current block]`; the current block fills from `BLOCK`.
    buf: Vec<f32>,
    fill: usize,
    /// Tail output for the current block, computed when the previous block completed.
    tail: Vec<f32>,
    /// Non-zero tail partitions as `(age in blocks, spectrum)`.
    parts: Vec<(usize, Spectrum)>,
    /// Spectra of the last `slots.len()` input blocks, newest at `slot`.
    slots: Vec<Spectrum>,
    slot: usize,
    acc: Spectrum,
    fft_in: Vec<f32>,
    fwd: Arc<dyn RealToComplex<f32>>,
    inv: Arc<dyn ComplexToReal<f32>>,
    fwd_scratch: Spectrum,
    inv_scratch: Spectrum,
}

impl Mono {
    fn new(
        ir: &[f32],
        fwd: &Arc<dyn RealToComplex<f32>>,
        inv: &Arc<dyn ComplexToReal<f32>>,
    ) -> Self {
        let mut head = vec![0.0; BLOCK];
        for (h, t) in head.iter_mut().zip(ir) {
            *h = *t;
        }

        let tail_ir = ir.get(BLOCK..).unwrap_or(&[]);
        let count = tail_ir.len().div_ceil(BLOCK);
        let mut fft_in = fwd.make_input_vec();
        let mut scratch = fwd.make_scratch_vec();
        let mut parts = Vec::new();
        for (p, chunk) in tail_ir.chunks(BLOCK).enumerate() {
            fft_in.fill(0.0);
            for (d, s) in fft_in.iter_mut().zip(chunk) {
                *d = *s;
            }
            let mut spec = fwd.make_output_vec();
            // Lengths are fixed by the plan, so this cannot fail.
            let _ = fwd.process_with_scratch(&mut fft_in, &mut spec, &mut scratch);
            if spec.iter().any(|c| c.norm_sqr() > 0.0) {
                parts.push((p, spec));
            }
        }

        Self {
            head,
            hist: vec![0.0; 2 * BLOCK],
            pos: 0,
            buf: vec![0.0; 2 * BLOCK],
            fill: 0,
            tail: vec![0.0; BLOCK],
            parts,
            slots: (0..count).map(|_| fwd.make_output_vec()).collect(),
            slot: 0,
            acc: fwd.make_output_vec(),
            fft_in,
            fwd: Arc::clone(fwd),
            inv: Arc::clone(inv),
            fwd_scratch: scratch,
            inv_scratch: inv.make_scratch_vec(),
        }
    }

    fn push(&mut self, x: f32) -> f32 {
        self.pos = (self.pos + BLOCK - 1) % BLOCK;
        if let Some(v) = self.hist.get_mut(self.pos) {
            *v = x;
        }
        if let Some(v) = self.hist.get_mut(self.pos + BLOCK) {
            *v = x;
        }
        let head = self
            .hist
            .get(self.pos..self.pos + BLOCK)
            .map_or(0.0, |w| dot(w, &self.head));
        let y = head + self.tail.get(self.fill).copied().unwrap_or(0.0);

        if let Some(v) = self.buf.get_mut(BLOCK + self.fill) {
            *v = x;
        }
        self.fill += 1;
        if self.fill == BLOCK {
            self.fill = 0;
            self.block_done();
        }
        y
    }

    /// Turns the completed block into the tail for the next block.
    fn block_done(&mut self) {
        if self.parts.is_empty() {
            return;
        }
        for (d, s) in self.fft_in.iter_mut().zip(&self.buf) {
            *d = *s;
        }
        let n = self.slots.len();
        self.slot = (self.slot + 1) % n;
        if let Some(spec) = self.slots.get_mut(self.slot) {
            let _ = self
                .fwd
                .process_with_scratch(&mut self.fft_in, spec, &mut self.fwd_scratch);
        }

        self.acc.fill(Complex::default());
        for (p, h) in &self.parts {
            let Some(x) = self.slots.get((self.slot + n - p) % n) else {
                continue;
            };
            for (a, (hv, xv)) in self.acc.iter_mut().zip(h.iter().zip(x)) {
                *a += hv * xv;
            }
        }
        // DC and Nyquist are real for a real signal; drop rounding residue so the inverse is clean.
        if let Some(c) = self.acc.first_mut() {
            c.im = 0.0;
        }
        if let Some(c) = self.acc.last_mut() {
            c.im = 0.0;
        }

        let _ =
            self.inv
                .process_with_scratch(&mut self.acc, &mut self.fft_in, &mut self.inv_scratch);
        let scale = 1.0 / (2 * BLOCK) as f32;
        for (t, v) in self.tail.iter_mut().zip(self.fft_in.iter().skip(BLOCK)) {
            *t = v * scale;
        }

        self.buf.rotate_left(BLOCK);
    }

    fn reset(&mut self) {
        self.pos = 0;
        self.fill = 0;
        self.slot = 0;
        self.hist.fill(0.0);
        self.buf.fill(0.0);
        self.tail.fill(0.0);
        for s in &mut self.slots {
            s.fill(Complex::default());
        }
    }
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Stereo convolver: input L is convolved with `left`, input R with `right`.
pub struct Convolver {
    left: Mono,
    right: Mono,
    taps: usize,
    /// Consecutive frames whose input was silent.
    quiet: usize,
}

impl Convolver {
    /// Builds the convolver from one impulse response per channel. Allocates; not real-time.
    pub fn new(left: &[f32], right: &[f32]) -> Self {
        let mut planner = RealFftPlanner::<f32>::new();
        let fwd = planner.plan_fft_forward(2 * BLOCK);
        let inv = planner.plan_fft_inverse(2 * BLOCK);
        Self {
            left: Mono::new(left, &fwd, &inv),
            right: Mono::new(right, &fwd, &inv),
            taps: left.len().max(right.len()),
            quiet: 0,
        }
    }

    /// Convolves `io` in place with zero latency. Real-time safe.
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        for frame in io.iter_mut() {
            let [l, r] = frame;
            let silent = l.abs() < SILENT && r.abs() < SILENT;
            self.quiet = if silent {
                self.quiet.saturating_add(1)
            } else {
                0
            };
            *l = self.left.push(*l);
            *r = self.right.push(*r);
        }
    }

    /// True once the input has been silent for at least the IR length, so no tail is left.
    pub fn is_silent(&self) -> bool {
        self.quiet >= self.taps
    }

    /// Clears all history and tails.
    pub fn reset(&mut self) {
        self.left.reset();
        self.right.reset();
        self.quiet = 0;
    }
}
