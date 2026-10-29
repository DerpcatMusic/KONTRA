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
const STAGES: [(usize, usize); 4] = [(64, 512), (256, 2048), (1024, 8192), (8192, usize::MAX)];
/// Inputs below this magnitude count as silence.
const SILENT: f32 = 1e-10;

type Spectrum = Vec<Complex<f32>>;

/// A spectrum as its real and imaginary parts apart, so a product of two
/// runs on whole vector registers without shuffling the parts.
#[derive(Clone)]
struct Split {
    re: Vec<f32>,
    im: Vec<f32>,
}

impl Split {
    fn of(spec: &[Complex<f32>]) -> Self {
        Self {
            re: spec.iter().map(|c| c.re).collect(),
            im: spec.iter().map(|c| c.im).collect(),
        }
    }

    fn set(&mut self, spec: &[Complex<f32>]) {
        for ((re, im), c) in self.re.iter_mut().zip(&mut self.im).zip(spec) {
            (*re, *im) = (c.re, c.im);
        }
    }

    fn clear(&mut self) {
        self.re.fill(0.0);
        self.im.fill(0.0);
    }

    /// Adds `h · x`.
    fn mac(&mut self, h: &Self, x: &Self) {
        let acc = self.re.iter_mut().zip(&mut self.im);
        let hx = h.re.iter().zip(&h.im).zip(x.re.iter().zip(&x.im));
        for ((ar, ai), ((hr, hi), (xr, xi))) in acc.zip(hx) {
            *ar += hr * xr - hi * xi;
            *ai += hr * xi + hi * xr;
        }
    }
}

/// One uniformly partitioned stretch of the response, block `block`.
struct Stage {
    block: usize,
    /// `[previous block, current block]`; the current block fills from `block`.
    buf: Vec<f32>,
    fill: usize,
    /// Output for the current block, computed when the previous block completed.
    tail: Vec<f32>,
    /// Non-zero partitions as `(age in blocks, spectrum)`.
    parts: Vec<(usize, Split)>,
    /// Spectra of the last `slots.len()` input blocks, newest at `slot`.
    slots: Vec<Split>,
    slot: usize,
    acc: Split,
    /// The transforms' side of `acc` and the slots.
    spec: Spectrum,
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
                parts.push((p, Split::of(&spec)));
            }
        }
        let count = parts.last()?.0 + 1;
        Some(Self {
            block,
            buf: vec![0.0; 2 * block],
            fill: 0,
            tail: vec![0.0; block],
            parts,
            slots: vec![Split::of(&fwd.make_output_vec()); count],
            slot: 0,
            acc: Split::of(&fwd.make_output_vec()),
            spec: fwd.make_output_vec(),
            fft_in,
            fwd_scratch: scratch,
            inv_scratch: inv.make_scratch_vec(),
            fwd,
            inv,
        })
    }

    /// Adds this stage's output for the frames `x` to `y`.
    fn process(&mut self, x: &[f32], y: &mut [f32]) {
        let mut done = 0;
        while done < x.len() {
            let take = (self.block - self.fill).min(x.len() - done);
            let (from, to) = (self.fill, self.fill + take);
            let (Some(input), Some(out)) = (x.get(done..done + take), y.get_mut(done..done + take))
            else {
                return;
            };
            if let Some(b) = self.buf.get_mut(self.block + from..self.block + to) {
                b.copy_from_slice(input);
            }
            for (o, t) in out
                .iter_mut()
                .zip(self.tail.get(from..to).unwrap_or_default())
            {
                *o += t;
            }
            (self.fill, done) = (to, done + take);
            if self.fill == self.block {
                self.fill = 0;
                self.block_done();
            }
        }
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
        let _ =
            self.fwd
                .process_with_scratch(&mut self.fft_in, &mut self.spec, &mut self.fwd_scratch);
        if let Some(x) = self.slots.get_mut(self.slot) {
            x.set(&self.spec);
        }

        self.acc.clear();
        for (p, h) in &self.parts {
            if let Some(x) = self.slots.get((self.slot + n - p) % n) {
                self.acc.mac(h, x);
            }
        }
        for (c, (re, im)) in self
            .spec
            .iter_mut()
            .zip(self.acc.re.iter().zip(&self.acc.im))
        {
            *c = Complex::new(*re, *im);
        }
        // DC and Nyquist are real for a real signal; drop rounding residue so the inverse is clean.
        if let Some(c) = self.spec.first_mut() {
            c.im = 0.0;
        }
        if let Some(c) = self.spec.last_mut() {
            c.im = 0.0;
        }

        let _ =
            self.inv
                .process_with_scratch(&mut self.spec, &mut self.fft_in, &mut self.inv_scratch);
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
            s.clear();
        }
    }
}

/// Frames a channel takes at once.
const CHUNK: usize = 64;

/// One channel: a direct FIR over the first `HEAD` taps, and the stages for the rest.
struct Mono {
    /// The head taps, last first, so a window of `line` dots with them.
    head: [f32; HEAD],
    /// The last `HEAD − 1` inputs, then the chunk being convolved.
    line: [f32; HEAD - 1 + CHUNK],
    stages: Vec<Stage>,
}

impl Mono {
    fn new(ir: &[f32], planner: &mut RealFftPlanner<f32>) -> Self {
        let mut head = [0.0; HEAD];
        for (h, t) in head.iter_mut().rev().zip(ir) {
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
            line: [0.0; HEAD - 1 + CHUNK],
            stages,
        }
    }

    /// Convolves `io`, at most [`CHUNK`] frames, in place.
    fn process(&mut self, io: &mut [f32]) {
        let n = io.len().min(CHUNK);
        let Some(fresh) = self.line.get_mut(HEAD - 1..HEAD - 1 + n) else {
            return;
        };
        fresh.copy_from_slice(io.get(..n).unwrap_or_default());
        let mut out = [0.0; CHUNK];
        for (o, window) in out.iter_mut().zip(self.line.windows(HEAD)).take(n) {
            *o = dot(window, &self.head);
        }
        let out = out.get_mut(..n).unwrap_or_default();
        for s in &mut self.stages {
            s.process(io, out);
        }
        io.get_mut(..n).unwrap_or_default().copy_from_slice(out);
        self.line.copy_within(n..n + HEAD - 1, 0);
    }

    fn reset(&mut self) {
        self.line.fill(0.0);
        for s in &mut self.stages {
            s.reset();
        }
    }
}

/// Sixteen running sums: four vector registers of independent chains, so the
/// adds overlap instead of waiting on each other (float addition order keeps
/// the compiler from splitting one sum itself).
fn dot(a: &[f32], b: &[f32]) -> f32 {
    let (a16, a_rest) = a.as_chunks::<16>();
    let (b16, b_rest) = b.as_chunks::<16>();
    let mut lanes = [0.0f32; 16];
    for (x, y) in a16.iter().zip(b16) {
        for ((l, x), y) in lanes.iter_mut().zip(x).zip(y) {
            *l += x * y;
        }
    }
    let rest: f32 = a_rest.iter().zip(b_rest).map(|(x, y)| x * y).sum();
    // Halves added as vectors: a short tree, not sixteen scalar adds in a row.
    let (lo, hi) = lanes.split_at(8);
    let mut s8 = [0.0f32; 8];
    for ((s, x), y) in s8.iter_mut().zip(lo).zip(hi) {
        *s = x + y;
    }
    let (lo, hi) = s8.split_at(4);
    let mut s4 = [0.0f32; 4];
    for ((s, x), y) in s4.iter_mut().zip(lo).zip(hi) {
        *s = x + y;
    }
    let [a, b, c, d] = s4;
    (a + c) + (b + d) + rest
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
        for chunk in io.chunks_mut(CHUNK) {
            let (mut l, mut r) = ([0.0; CHUNK], [0.0; CHUNK]);
            for ((frame, l), r) in chunk.iter().zip(&mut l).zip(&mut r) {
                [*l, *r] = *frame;
                let silent = l.abs() < SILENT && r.abs() < SILENT;
                self.quiet = if silent {
                    self.quiet.saturating_add(1)
                } else {
                    0
                };
            }
            let n = chunk.len();
            self.left.process(l.get_mut(..n).unwrap_or_default());
            self.right.process(r.get_mut(..n).unwrap_or_default());
            for ((frame, l), r) in chunk.iter_mut().zip(l).zip(r) {
                *frame = [l, r];
            }
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
        for n in [0, 63, 64, 511, 512, 2_047, 2_048, 8_191, 8_192, 25_000, 29_999] {
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
