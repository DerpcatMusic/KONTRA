//! Zero-latency two-stage partitioned FFT convolution.
//!
//! The head covers the first `TAIL_FACTOR * head_block` samples of the IR with
//! small partitions and recomputes the current partial block on every call, so
//! output has no added latency for any host block size. The tail uses large
//! partitions computed once per tail block; its natural one-block latency lines
//! up exactly with its IR offset.

use realfft::{ComplexToReal, RealFftPlanner, RealToComplex, num_complex::Complex32};
use std::sync::Arc;

/// Tail partitions are this many head blocks long.
const TAIL_FACTOR: usize = 16;

/// Uniformly partitioned overlap-add convolver (`FFTConvolver` scheme).
struct Partitioned {
    block: usize,
    bins: usize,
    fft: Arc<dyn RealToComplex<f32>>,
    ifft: Arc<dyn ComplexToReal<f32>>,
    /// IR partition spectra, `bins` each.
    ir: Vec<Complex32>,
    /// Ring of input block spectra, `bins` each; `current` is being filled.
    segments: Vec<Complex32>,
    current: usize,
    /// Sum of all but the newest partition, fixed for the current block.
    history: Vec<Complex32>,
    /// Next partition to add to `history`; lets callers spread the work.
    pending: usize,
    spectrum: Vec<Complex32>,
    time: Vec<f32>,
    input: Vec<f32>,
    fill: usize,
    overlap: Vec<f32>,
    scratch: Vec<Complex32>,
    scale: f32,
}

impl Partitioned {
    fn new(planner: &mut RealFftPlanner<f32>, ir: &[f32], block: usize) -> Self {
        let size = 2 * block;
        let bins = block + 1;
        let fft = planner.plan_fft_forward(size);
        let ifft = planner.plan_fft_inverse(size);
        let count = ir.len().div_ceil(block).max(1);
        let mut scratch =
            vec![Complex32::default(); fft.get_scratch_len().max(ifft.get_scratch_len())];
        let mut time = vec![0.0; size];
        let mut spectra = vec![Complex32::default(); count * bins];
        for (part, spectrum) in ir.chunks(block).zip(spectra.chunks_exact_mut(bins)) {
            time.fill(0.0);
            time[..part.len()].copy_from_slice(part);
            // Lengths come from this plan, so the transform cannot fail.
            let _ = fft.process_with_scratch(&mut time, spectrum, &mut scratch);
        }
        Self {
            block,
            bins,
            fft,
            ifft,
            ir: spectra,
            segments: vec![Complex32::default(); count * bins],
            current: 0,
            history: vec![Complex32::default(); bins],
            pending: 1,
            spectrum: vec![Complex32::default(); bins],
            time,
            input: vec![0.0; block],
            fill: 0,
            overlap: vec![0.0; block],
            scratch,
            scale: 1.0 / size as f32,
        }
    }

    fn clear(&mut self) {
        self.segments.fill(Complex32::default());
        self.history.fill(Complex32::default());
        self.input.fill(0.0);
        self.overlap.fill(0.0);
        (self.current, self.pending, self.fill) = (0, 1, 0);
    }

    fn count(&self) -> usize {
        self.segments.len() / self.bins
    }

    fn process(&mut self, input: &[f32], output: &mut [f32]) {
        let (bins, count) = (self.bins, self.count());
        let mut done = 0;
        while done < input.len() {
            let start = self.fill;
            let n = (input.len() - done).min(self.block - start);
            self.input[start..start + n].copy_from_slice(&input[done..done + n]);

            self.time[..self.block].copy_from_slice(&self.input);
            self.time[self.block..].fill(0.0);
            let current = self.current * bins;
            let _ = self.fft.process_with_scratch(
                &mut self.time,
                &mut self.segments[current..current + bins],
                &mut self.scratch,
            );

            if start == 0 {
                self.accumulate(count);
            }
            self.spectrum.copy_from_slice(&self.history);
            mac(
                &mut self.spectrum,
                &self.ir[..bins],
                &self.segments[current..current + bins],
            );
            self.spectrum[0].im = 0.0;
            self.spectrum[bins - 1].im = 0.0;
            let _ = self.ifft.process_with_scratch(
                &mut self.spectrum,
                &mut self.time,
                &mut self.scratch,
            );

            let out = &mut output[done..done + n];
            let fresh = &self.time[start..start + n];
            let tail = &self.overlap[start..start + n];
            for ((y, a), b) in out.iter_mut().zip(fresh).zip(tail) {
                *y = (a + b) * self.scale;
            }

            self.fill += n;
            done += n;
            if self.fill == self.block {
                self.fill = 0;
                self.input.fill(0.0);
                self.overlap.copy_from_slice(&self.time[self.block..]);
                self.current = (self.current + count - 1) % count;
                self.history.fill(Complex32::default());
                self.pending = 1;
            }
        }
    }

    /// Adds partitions `pending..upto` to the history of the block being filled.
    /// Their input blocks are already complete, so this can run ahead of time.
    fn accumulate(&mut self, upto: usize) {
        let (bins, count) = (self.bins, self.count());
        while self.pending < upto.min(count) {
            let i = self.pending;
            let seg = (self.current + i) % count * bins;
            mac(
                &mut self.history,
                &self.ir[i * bins..(i + 1) * bins],
                &self.segments[seg..seg + bins],
            );
            self.pending += 1;
        }
    }
}

/// `acc += a * b` over spectra; a plain zip loop so LLVM vectorizes it.
#[inline]
fn mac(acc: &mut [Complex32], a: &[Complex32], b: &[Complex32]) {
    for ((acc, a), b) in acc.iter_mut().zip(a).zip(b) {
        acc.re += a.re * b.re - a.im * b.im;
        acc.im += a.re * b.im + a.im * b.re;
    }
}

/// One channel: head (zero latency) plus optional tail.
pub struct Convolver {
    head: Partitioned,
    tail: Option<Tail>,
    scratch: Vec<f32>,
    len: usize,
}

struct Tail {
    conv: Partitioned,
    input: Vec<f32>,
    output: Vec<f32>,
    pos: usize,
}

impl Convolver {
    pub fn new(ir: &[f32], max_block: usize) -> Self {
        let block = max_block.next_power_of_two().clamp(32, 512);
        let split = (block * TAIL_FACTOR).min(ir.len());
        let mut planner = RealFftPlanner::new();
        let head = Partitioned::new(&mut planner, &ir[..split], block);
        let tail = (split < ir.len()).then(|| {
            let tail_block = block * TAIL_FACTOR;
            Tail {
                conv: Partitioned::new(&mut planner, &ir[split..], tail_block),
                input: vec![0.0; tail_block],
                output: vec![0.0; tail_block],
                pos: 0,
            }
        });
        Self {
            head,
            tail,
            scratch: vec![0.0; max_block],
            len: ir.len(),
        }
    }

    /// Impulse response length in frames.
    pub fn ir_len(&self) -> usize {
        self.len
    }

    /// Silences the convolver's history.
    pub fn clear(&mut self) {
        self.head.clear();
        if let Some(tail) = &mut self.tail {
            tail.conv.clear();
            tail.input.fill(0.0);
            tail.output.fill(0.0);
            tail.pos = 0;
        }
    }

    /// Convolves `io` in place; `io.len()` must not exceed `max_block`.
    pub fn process(&mut self, io: &mut [f32]) {
        let n = io.len().min(self.scratch.len());
        let (io, input) = (&mut io[..n], &mut self.scratch[..n]);
        input.copy_from_slice(io);
        self.head.process(input, io);
        let Some(tail) = &mut self.tail else { return };
        let mut done = 0;
        while done < n {
            let len = (n - done).min(tail.input.len() - tail.pos);
            let range = tail.pos..tail.pos + len;
            tail.input[range.clone()].copy_from_slice(&input[done..done + len]);
            for (y, t) in io[done..done + len].iter_mut().zip(&tail.output[range]) {
                *y += t;
            }
            tail.pos += len;
            done += len;
            // Spread the tail's history sum evenly over the head blocks.
            let count = tail.conv.count();
            tail.conv
                .accumulate(1 + (count - 1) * tail.pos / tail.input.len());
            if tail.pos == tail.input.len() {
                tail.pos = 0;
                tail.conv.process(&tail.input, &mut tail.output);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn direct(x: &[f32], h: &[f32]) -> Vec<f32> {
        (0..x.len())
            .map(|n| (0..=n.min(h.len() - 1)).map(|k| h[k] * x[n - k]).sum())
            .collect()
    }

    fn noise(len: usize, mut seed: u32) -> Vec<f32> {
        (0..len)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 8) as f32 / (1 << 24) as f32 - 0.5
            })
            .collect()
    }

    #[test]
    fn matches_direct_convolution_across_odd_block_sizes() {
        // Long enough to exercise both head and tail stages.
        let h: Vec<f32> = noise(3000, 7)
            .iter()
            .enumerate()
            .map(|(i, v)| v * (-(i as f32) / 800.0).exp())
            .collect();
        let x = noise(9000, 3);
        let expected = direct(&x, &h);
        for block in [1, 37, 64, 128, 300] {
            let mut conv = Convolver::new(&h, block);
            let mut y = x.clone();
            for chunk in y.chunks_mut(block) {
                conv.process(chunk);
            }
            let err = y
                .iter()
                .zip(&expected)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0, f32::max);
            assert!(err < 1e-4, "block {block}: max error {err}");
        }
    }

    #[test]
    fn impulse_reproduces_ir_with_zero_latency() {
        let h = [0.5, -0.25, 0.125];
        let mut conv = Convolver::new(&h, 64);
        let mut io = [0.0; 64];
        io[0] = 1.0;
        conv.process(&mut io);
        for (i, v) in io.iter().enumerate() {
            let want = h.get(i).copied().unwrap_or(0.0);
            assert!((v - want).abs() < 1e-6, "{i}: {v}");
        }
    }
}
