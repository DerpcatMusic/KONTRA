//! Zero-latency non-uniformly partitioned FFT convolution.
//!
//! The head covers the first `TAIL_FACTOR * head_block` samples of the IR with
//! small partitions and recomputes the current partial block on every call, so
//! output has no added latency for any host block size. Tail stages follow,
//! each with partitions [`STAGE_GROWTH`] times longer than the last (up to
//! [`MAX_STAGE_BLOCK`]), computed once per stage block; a stage's natural
//! one-block latency lines up exactly with its IR offset, which is never
//! shorter than its block. Long partitions cut the spectral multiply-adds
//! per frame from IR length / head block to a few dozen.

use realfft::{ComplexToReal, RealFftPlanner, RealToComplex, num_complex::Complex32};
use std::sync::Arc;

/// The first tail stage's partitions are this many head blocks long.
const TAIL_FACTOR: usize = 16;
/// Each further tail stage has partitions this many times longer…
const STAGE_GROWTH: usize = 4;
/// …up to this many frames: longer blocks would put ever larger FFTs into
/// the one callback that completes them.
const MAX_STAGE_BLOCK: usize = 16_384;

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
    /// Which `segments` are all zeros (silent input): their products are
    /// skipped, which leaves every sum as it was.
    silent: Vec<bool>,
    current: usize,
    /// Sum of all but the newest partition, fixed for the current block.
    history: Vec<Complex32>,
    /// Only silent segments went into `history`: it is all zeros.
    quiet_history: bool,
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
            silent: vec![true; count],
            current: 0,
            history: vec![Complex32::default(); bins],
            quiet_history: true,
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
        self.silent.fill(true);
        self.history.fill(Complex32::default());
        self.quiet_history = true;
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
            let fresh = &input[done..done + n];
            self.input[start..start + n].copy_from_slice(fresh);

            let current = self.current * bins;
            let segment = &mut self.segments[current..current + bins];
            // The spectrum of a silent block is all zeros.
            if self.input[..start + n].iter().any(|&x| x != 0.0) {
                self.time[..self.block].copy_from_slice(&self.input);
                self.time[self.block..].fill(0.0);
                let _ = self.fft.process_with_scratch(&mut self.time, segment, &mut self.scratch);
                self.silent[self.current] = false;
            } else if !self.silent[self.current] {
                segment.fill(Complex32::default());
                self.silent[self.current] = true;
            }

            if start == 0 {
                self.accumulate(count);
            }
            if self.quiet_history && self.silent[self.current] {
                // Nothing but zeros to transform back.
                self.time.fill(0.0);
            } else {
                self.spectrum.copy_from_slice(&self.history);
                if !self.silent[self.current] {
                    mac(&mut self.spectrum, &self.ir[..bins], &self.segments[current..current + bins]);
                }
                self.spectrum[0].im = 0.0;
                self.spectrum[bins - 1].im = 0.0;
                let _ = self.ifft.process_with_scratch(
                    &mut self.spectrum,
                    &mut self.time,
                    &mut self.scratch,
                );
            }

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
                if !self.quiet_history {
                    self.history.fill(Complex32::default());
                    self.quiet_history = true;
                }
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
            let seg = (self.current + i) % count;
            if !self.silent[seg] {
                let seg = seg * bins;
                mac(
                    &mut self.history,
                    &self.ir[i * bins..(i + 1) * bins],
                    &self.segments[seg..seg + bins],
                );
                self.quiet_history = false;
            }
            self.pending += 1;
        }
    }
}

/// `acc += a * b` over spectra. Uses an AVX2 build when the CPU has it:
/// the same operations in the same order (no FMA), so the same result.
#[inline]
fn mac(acc: &mut [Complex32], a: &[Complex32], b: &[Complex32]) {
    #[cfg(target_arch = "x86_64")]
    if std::arch::is_x86_feature_detected!("avx2") {
        // SAFETY: the running CPU supports AVX2.
        return unsafe { mac_avx2(acc, a, b) };
    }
    mac_body(acc, a, b);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
fn mac_avx2(acc: &mut [Complex32], a: &[Complex32], b: &[Complex32]) {
    mac_body(acc, a, b);
}

/// A plain zip loop so LLVM vectorizes it.
#[inline(always)]
fn mac_body(acc: &mut [Complex32], a: &[Complex32], b: &[Complex32]) {
    for ((acc, a), b) in acc.iter_mut().zip(a).zip(b) {
        acc.re += a.re * b.re - a.im * b.im;
        acc.im += a.re * b.im + a.im * b.re;
    }
}

/// One channel: head (zero latency) plus tail stages.
pub struct Convolver {
    head: Partitioned,
    tails: Box<[Tail]>,
    scratch: Vec<f32>,
    len: usize,
    /// Sum of the IR's magnitudes from every [`QUIET_STEP`]th frame on:
    /// output that many frames after the input stops is at most the input
    /// peak times it.
    remaining: Box<[f32]>,
}

/// Frames between [`Convolver::remaining`] entries.
const QUIET_STEP: usize = 256;

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
        let mut tails = Vec::new();
        let (mut start, mut size) = (split, block * TAIL_FACTOR);
        while start < ir.len() {
            let next = (size * STAGE_GROWTH).min(MAX_STAGE_BLOCK);
            // A longer stage pays for its larger FFTs only over a few
            // partitions; short of that, this stage takes the rest.
            let end = if next > size && ir.len() >= 3 * next { next } else { ir.len() };
            tails.push(Tail {
                conv: Partitioned::new(&mut planner, &ir[start..end], size),
                input: vec![0.0; size],
                output: vec![0.0; size],
                pos: 0,
            });
            (start, size) = (end, next);
        }
        let mut sum = 0.0;
        let mut remaining: Vec<f32> = (ir.chunks(QUIET_STEP).rev())
            .map(|c| {
                sum += c.iter().map(|x| x.abs()).sum::<f32>();
                sum
            })
            .collect();
        remaining.reverse();
        Self {
            head,
            tails: tails.into(),
            scratch: vec![0.0; max_block],
            len: ir.len(),
            remaining: remaining.into(),
        }
    }

    /// Frames of output after input at most `peak` falls silent, until it
    /// is below −120 dBFS: where the rest of the IR can no longer add up to it.
    pub fn tail(&self, peak: f32) -> usize {
        let quiet = self.remaining.partition_point(|&r| r * peak >= super::processor::SILENCE);
        (quiet * QUIET_STEP).min(self.len)
    }

    /// Silences the convolver's history.
    pub fn clear(&mut self) {
        self.head.clear();
        for tail in &mut self.tails {
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
        for tail in &mut self.tails {
            tail.process(input, io);
        }
    }
}

impl Tail {
    /// Adds the stage's output for `input` to `io`.
    fn process(&mut self, input: &[f32], io: &mut [f32]) {
        let mut done = 0;
        while done < input.len() {
            let len = (input.len() - done).min(self.input.len() - self.pos);
            let range = self.pos..self.pos + len;
            self.input[range.clone()].copy_from_slice(&input[done..done + len]);
            for (y, t) in io[done..done + len].iter_mut().zip(&self.output[range]) {
                *y += t;
            }
            self.pos += len;
            done += len;
            // Spread the stage's history sum evenly over its block.
            let count = self.conv.count();
            self.conv
                .accumulate(1 + (count - 1) * self.pos / self.input.len());
            if self.pos == self.input.len() {
                self.pos = 0;
                self.conv.process(&self.input, &mut self.output);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn direct(x: &[f32], h: &[f32]) -> Vec<f32> {
        (0..x.len())
            .map(|n| (0..=n.min(h.len() - 1)).map(|k| f64::from(h[k]) * f64::from(x[n - k])).sum::<f64>() as f32)
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
        let mut x = noise(9000, 3);
        // A silent stretch: its blocks' spectra are skipped, then input resumes.
        x[2000..6500].fill(0.0);
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
    fn long_ir_matches_direct_convolution_through_every_stage() {
        // Head 32, stages 512, 2048, 8192 and the capped 16384 with three
        // partitions (block 1); head 128, stages 2048, 8192, 16384 (block 100).
        let h: Vec<f32> = noise(60_000, 5)
            .iter()
            .enumerate()
            .map(|(i, v)| v * (-(i as f32) / 20_000.0).exp())
            .collect();
        let x = noise(70_000, 9);
        let expected = direct(&x, &h);
        let peak = expected.iter().fold(0f32, |m, v| m.max(v.abs()));
        for block in [1, 100] {
            let mut conv = Convolver::new(&h, block);
            assert_eq!(conv.tails.len(), if block == 1 { 4 } else { 3 });
            let mut y = x.clone();
            for chunk in y.chunks_mut(block) {
                conv.process(chunk);
            }
            let err = y.iter().zip(&expected).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
            assert!(err < 1e-5 * peak, "block {block}: max error {err} of peak {peak}");
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

    /// Cost of a 3 s IR in 128-frame blocks, on 4 s of noise and on 1 s of
    /// noise then 3 s of silence (a ringing tail), and a hash of the output
    /// bits to compare builds:
    /// `cargo test --release --lib convolution_speed -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn convolution_speed() {
        let h: Vec<f32> = noise(144_000, 7)
            .iter()
            .enumerate()
            .map(|(i, v)| v * (-(i as f32) / 30_000.0).exp())
            .collect();
        let busy = noise(48_000 * 4, 3);
        let mut tail = busy.clone();
        tail[48_000..].fill(0.0);
        for (name, x) in [("noise", busy), ("tail", tail)] {
            let (mut best, mut hash) = (f64::MAX, 0u64);
            for _ in 0..5 {
                let mut conv = Convolver::new(&h, 128);
                let mut y = x.clone();
                let t = std::time::Instant::now();
                for chunk in y.chunks_mut(128) {
                    conv.process(chunk);
                }
                best = best.min(t.elapsed().as_secs_f64());
                hash = y.iter().fold(0, |h, v| (h ^ u64::from(v.to_bits())).wrapping_mul(0x100_0000_01b3));
            }
            println!("{name}: {:.1} µs per 128-frame block · output hash {hash:016x}", best / (x.len() / 128) as f64 * 1e6);
        }
    }
}
