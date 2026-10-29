//! Zero-latency convolution: a direct FIR head, then non-uniformly
//! partitioned FFT stages, each block size covering a later stretch of the
//! response (Gardner 1995). A stage of block `B` delays by `B`, so it may
//! start no earlier than `B` into the response.

use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};

/// Head length, in frames.
const HEAD: usize = 64;
/// Each stage's block size and where its stretch of the response ends;
/// it starts where the one before ends. The last runs to the end.
const STAGES: [(usize, usize); 3] = [(64, 1024), (512, 8192), (4096, usize::MAX)];
/// Inputs below this magnitude count as silence.
const SILENT: f32 = 1e-10;

type Spectrum = Vec<Complex<f32>>;

/// One uniformly partitioned stretch of the response, block `block`.
struct Stage {
    block: usize,
    /// `[previous block, current block]`; the current block fills from `block`.
    buf: Vec<f32>,
    fill: usize,
    /// Output for the current block, computed when the previous block completed.
    tail: Vec<f32>,
    /// Non-zero partitions as `(age in blocks, spectrum)`.
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

impl Stage {
    /// The stage for `ir[from..to]`, `from` a multiple of `block` and at least `block`.
    fn new(
        ir: &[f32],
        block: usize,
        from: usize,
        to: usize,
        planner: &mut RealFftPlanner<f32>,
    ) -> Option<Self> {
        let fwd = planner.plan_fft_forward(2 * block);
        let inv = planner.plan_fft_inverse(2 * block);
        // Partition p holds ir[block + p·block ..]; only those inside from..to.
        let tail = ir.get(block..to.min(ir.len()))?;
        let mut fft_in = fwd.make_input_vec();
        let mut scratch = fwd.make_scratch_vec();
        let mut parts = Vec::new();
        for (p, chunk) in tail.chunks(block).enumerate() {
            if block + p * block < from {
                continue;
            }
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
        let count = parts.last()?.0 + 1;
        Some(Self {
            block,
            buf: vec![0.0; 2 * block],
            fill: 0,
            tail: vec![0.0; block],
            parts,
            slots: (0..count).map(|_| fwd.make_output_vec()).collect(),
            slot: 0,
            acc: fwd.make_output_vec(),
            fft_in,
            fwd_scratch: scratch,
            inv_scratch: inv.make_scratch_vec(),
            fwd,
            inv,
        })
    }

    fn push(&mut self, x: f32) -> f32 {
        let y = self.tail.get(self.fill).copied().unwrap_or(0.0);
        if let Some(v) = self.buf.get_mut(self.block + self.fill) {
            *v = x;
        }
        self.fill += 1;
        if self.fill == self.block {
            self.fill = 0;
            self.block_done();
        }
        y
    }

    /// Turns the completed block into the output for the next block.
    // ponytail: a long stage does all its work on the frame its block
    // completes; spread it over the block if that spike ever misses a deadline.
    fn block_done(&mut self) {
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
        let scale = 1.0 / (2 * self.block) as f32;
        for (t, v) in self
            .tail
            .iter_mut()
            .zip(self.fft_in.iter().skip(self.block))
        {
            *t = v * scale;
        }

        self.buf.rotate_left(self.block);
    }

    fn reset(&mut self) {
        self.fill = 0;
        self.slot = 0;
        self.buf.fill(0.0);
        self.tail.fill(0.0);
        for s in &mut self.slots {
            s.fill(Complex::default());
        }
    }
}

/// One channel: a direct FIR over the first `HEAD` taps, and the stages for the rest.
struct Mono {
    head: Vec<f32>,
    /// Last `HEAD` inputs, stored twice so the window `pos..pos + HEAD` is contiguous.
    hist: Vec<f32>,
    pos: usize,
    stages: Vec<Stage>,
}

impl Mono {
    fn new(ir: &[f32], planner: &mut RealFftPlanner<f32>) -> Self {
        let mut head = vec![0.0; HEAD];
        for (h, t) in head.iter_mut().zip(ir) {
            *h = *t;
        }
        let mut from = HEAD;
        let mut stages = Vec::new();
        for (block, to) in STAGES {
            stages.extend(Stage::new(ir, block, from, to, planner));
            from = to;
        }
        Self {
            head,
            hist: vec![0.0; 2 * HEAD],
            pos: 0,
            stages,
        }
    }

    fn push(&mut self, x: f32) -> f32 {
        self.pos = (self.pos + HEAD - 1) % HEAD;
        if let Some(v) = self.hist.get_mut(self.pos) {
            *v = x;
        }
        if let Some(v) = self.hist.get_mut(self.pos + HEAD) {
            *v = x;
        }
        let head = self
            .hist
            .get(self.pos..self.pos + HEAD)
            .map_or(0.0, |w| dot(w, &self.head));
        head + self.stages.iter_mut().map(|s| s.push(x)).sum::<f32>()
    }

    fn reset(&mut self) {
        self.pos = 0;
        self.hist.fill(0.0);
        for s in &mut self.stages {
            s.reset();
        }
    }
}

/// Eight running sums, so the compiler keeps them in one vector register; a
/// single serial sum can't vectorize (float addition order).
fn dot(a: &[f32], b: &[f32]) -> f32 {
    let (a8, a_rest) = a.as_chunks::<8>();
    let (b8, b_rest) = b.as_chunks::<8>();
    let mut lanes = [0.0f32; 8];
    for (x, y) in a8.iter().zip(b8) {
        for ((l, x), y) in lanes.iter_mut().zip(x).zip(y) {
            *l += x * y;
        }
    }
    let rest: f32 = a_rest.iter().zip(b_rest).map(|(x, y)| x * y).sum();
    lanes.iter().sum::<f32>() + rest
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
        Self {
            left: Mono::new(left, &mut planner),
            right: Mono::new(right, &mut planner),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stage_matches_direct_convolution_at_unity() {
        // A response reaching the last stage, and a noise input.
        let mut seed = 1u32;
        let mut noise = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1 << 24) as f32 - 0.5
        };
        let ir: Vec<f32> = (0..20_000).map(|_| noise() * 0.01).collect();
        let input: Vec<f32> = (0..30_000).map(|_| noise()).collect();
        let mut c = Convolver::new(&ir, &ir);
        let mut io: Vec<[f32; 2]> = input.iter().map(|&x| [x, x]).collect();
        for block in io.chunks_mut(256) {
            c.process(block);
        }
        for n in [0, 63, 64, 1_000, 1_024, 8_191, 8_192, 25_000, 29_999] {
            let direct: f32 = (0..=n.min(ir.len() - 1))
                .map(|k| ir[k] * input[n - k])
                .sum();
            assert!(
                (io[n][0] - direct).abs() < 1e-4,
                "{n}: {} vs {direct}",
                io[n][0]
            );
        }
    }
}
