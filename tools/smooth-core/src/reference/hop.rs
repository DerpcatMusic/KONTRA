//! The normal-path STFT hop stages of the reference (zero-latency byte clear).
//!
//! 0x8a3a90 calls 0x8ad640, which runs two workers per block, in series:
//! - **short stage**: 0x8b61a0 -> hop loop 0x8c5f30 -> 0x8d9230 per hop.
//!   Frame 256, hop 128, FFT 512 (4 hops per 512-sample block).
//! - **long stage**: 0x8b7410 -> hop loop 0x8c68a0 -> 0x8db030 per hop.
//!   Frame 2048, hop 1024, FFT 4096. Its input is the short stage's output
//!   (ring fed by 0x8e0390 from the 0x8dad80 destination), so it runs every
//!   other 512-sample block.
//!
//! Each stage delays by its frame length, so the chain delays by
//! 256 + 2048 = 2304, the per-group latency `0x894d20` reports.
//!
//! One hop (`HopStage::hop`), in the reference's order, at 44.1/48 kHz:
//! 1. 0x8e67a0 -> 0x1190: `dst[i] = ring[end - frame + i] * window[i]` over
//!    `frame` samples; `dst[frame..n]` was zeroed by 0x10d0 (n = 2 * frame).
//! 2. 0x1010: unscaled real forward FFT of length n, packed
//!    `[X0.re, 0, X1.re, X1.im, ..., X_{n/2}.re, 0]` (`n + 2` floats).
//! 3. 0x1520: `X_k *= H_k` (complex, `re = hr*xr - hi*xi`, `im = hr*xi + hi*xr`).
//!    `H` is the per-bin multiplier from the gain path ([`BinGain`]); with the
//!    identity band state it is exactly `(1, 0)` in both stages.
//! 4. 0x8e6eb0 -> 0x1280: unscaled real inverse FFT, then 0x1220: `y *= 1/n`.
//! 5. 0x8e7000 -> 0x10d0: zero `frame` floats of the OLA ring at `P + n`,
//!    with `P = end - frame` the frame start.
//! 6. 0x8e7150 -> 0x11f0: `ola[P + i] += c * y[i]` over n floats, with
//!    `c = f32(hop) / f32(frame)` (0x8d9647..0x8d96a9; 0.5 in both stages).
//! 7. 0x8dad80 -> 0x10a0: the stage output for the block that starts at
//!    input time `t0` is `ola[t0 - frame ..]` (the read sits at `P - hop` in
//!    the short stage and at `P - 512` / `P` in the long stage).
//!
//! The window (0x404c1c80 short, 0x402391c0 long) is `f32(1 - cos(2*pi*i/frame))`
//! in f64: 0x8a6960 builds the periodic Hann `0.5 * (1 - cos)` and the caller
//! divides it by its mean (0x8a6b60), which gives the factor 2. Both tables
//! match this formula bit for bit (`oracle/stage_fixture_48000.json`).
//!
//! Verified against the emulated process block, every stage array of one
//! hop of each stage plus the whole chain (tests below). Not verified: a
//! non-identity `H` from the band state (the multiply kernel itself is
//! checked on random `H`), the sidechain (both rings carry the input here),
//! and the 88.2 kHz+ geometry (see `NormalPath`).

use std::sync::Arc;

use rustfft::{Fft, FftPlanner, num_complex::Complex32};

/// Which of the two stages a hop belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageId {
    /// 0x8c5f30 / 0x8d9230: frame 256, hop 128.
    Short,
    /// 0x8c68a0 / 0x8db030: frame 2048, hop 1024.
    Long,
}

/// Boundary to the per-bin multipliers of the gain path (0x8e04c0 and the
/// kernel design after it; another module translates those).
///
/// `spectrum` is this hop's packed forward spectrum (`n + 2` floats, see the
/// module docs); `multipliers` receives the packed complex `H` of the same
/// length. Closures are accepted.
pub trait BinGain {
    fn bin_gains(&mut self, stage: StageId, channel: usize, spectrum: &[f32], multipliers: &mut [f32]);
}

impl<F> BinGain for F
where
    F: FnMut(StageId, usize, &[f32], &mut [f32]),
{
    fn bin_gains(&mut self, stage: StageId, channel: usize, spectrum: &[f32], multipliers: &mut [f32]) {
        self(stage, channel, spectrum, multipliers)
    }
}

/// The identity band state: `H_k = (1, 0)` at every bin, as the emulated
/// constructor state gives in both stages.
#[derive(Clone, Copy, Debug, Default)]
pub struct IdentityGain;

impl BinGain for IdentityGain {
    fn bin_gains(&mut self, _: StageId, _: usize, _: &[f32], multipliers: &mut [f32]) {
        for pair in multipliers.chunks_exact_mut(2) {
            pair[0] = 1.0;
            pair[1] = 0.0;
        }
    }
}

/// The analysis window of a stage: `f32(1 - cos(2*pi*i/len))`, computed in
/// f64 (0x8a6960 Hann, normalised by its mean in 0x8a6b60).
pub fn reference_window(len: usize) -> Vec<f32> {
    (0..len)
        .map(|i| (1.0 - (std::f64::consts::TAU * i as f64 / len as f64).cos()) as f32)
        .collect()
}

/// 0x1190: `dst[i] = a[i] * b[i]`.
pub fn mul(a: &[f32], b: &[f32], dst: &mut [f32]) {
    for ((d, &x), &y) in dst.iter_mut().zip(a).zip(b) {
        *d = x * y;
    }
}

/// 0x1520: packed complex `x[k] *= h[k]`, in the kernel's f32 order.
pub fn mul_packed(h: &[f32], x: &mut [f32]) {
    for (hp, xp) in h.chunks_exact(2).zip(x.chunks_exact_mut(2)) {
        let (hr, hi, xr, xi) = (hp[0], hp[1], xp[0], xp[1]);
        xp[0] = hr * xr - hi * xi;
        xp[1] = hr * xi + hi * xr;
    }
}

/// 0x11f0: `dst[i] += c * src[i]`.
pub fn add_product(src: &[f32], c: f32, dst: &mut [f32]) {
    for (d, &s) in dst.iter_mut().zip(src) {
        *d += c * s;
    }
}

/// Unscaled real FFT pair with the reference's packed layout (0x1010 forward,
/// 0x1280 inverse). Built on a complex FFT of the same length; buffers are
/// allocated once.
pub struct RealFft {
    n: usize,
    fwd: Arc<dyn Fft<f32>>,
    inv: Arc<dyn Fft<f32>>,
    buf: Vec<Complex32>,
    scratch: Vec<Complex32>,
}

impl RealFft {
    pub fn new(n: usize) -> Self {
        let mut planner = FftPlanner::new();
        let fwd = planner.plan_fft_forward(n);
        let inv = planner.plan_fft_inverse(n);
        let scratch_len = fwd.get_inplace_scratch_len().max(inv.get_inplace_scratch_len());
        Self {
            n,
            fwd,
            inv,
            buf: vec![Complex32::default(); n],
            scratch: vec![Complex32::default(); scratch_len],
        }
    }

    /// 0x1010: `n` reals in, `n + 2` packed floats out, no scaling.
    pub fn forward(&mut self, x: &[f32], packed: &mut [f32]) {
        let n = self.n;
        for (c, &v) in self.buf.iter_mut().zip(&x[..n]) {
            *c = Complex32::new(v, 0.0);
        }
        self.fwd.process_with_scratch(&mut self.buf, &mut self.scratch);
        for (k, c) in self.buf[..=n / 2].iter().enumerate() {
            packed[2 * k] = c.re;
            packed[2 * k + 1] = c.im;
        }
        packed[1] = 0.0;
        packed[n + 1] = 0.0;
    }

    /// 0x1280: `n + 2` packed floats in, `n` reals out, equal to `n * x` (no 1/n).
    pub fn inverse(&mut self, packed: &[f32], y: &mut [f32]) {
        let n = self.n;
        for k in 0..=n / 2 {
            let c = Complex32::new(packed[2 * k], packed[2 * k + 1]);
            self.buf[k] = c;
            if k != 0 && k != n / 2 {
                self.buf[n - k] = c.conj();
            }
        }
        self.inv.process_with_scratch(&mut self.buf, &mut self.scratch);
        for (o, c) in y[..n].iter_mut().zip(&self.buf) {
            *o = c.re;
        }
    }
}

/// One STFT stage (short or long), all channels.
pub struct HopStage {
    id: StageId,
    frame: usize,
    hop: usize,
    n: usize,
    window: Vec<f32>,
    /// `c` of the OLA add, `f32(hop) / f32(frame)`.
    ola_gain: f32,
    /// The 0x1220 factor, `1 / n` in f32.
    inv_n: f32,
    fft: RealFft,
    /// Per channel: input history ring and OLA ring, both `mask + 1` long.
    input: Vec<Vec<f32>>,
    ola: Vec<Vec<f32>>,
    mask: usize,
    /// Input samples pushed so far (ring positions are this, masked).
    t: usize,
    /// Pushed samples not yet consumed by a hop.
    pending: usize,
    // Hop scratch: padded frame / time output (n), spectrum and multipliers (n + 2).
    frame_buf: Vec<f32>,
    spectrum: Vec<f32>,
    multipliers: Vec<f32>,
}

impl HopStage {
    /// `max_block` is the largest block passed to [`HopStage::process`].
    pub fn new(id: StageId, frame: usize, hop: usize, channels: usize, max_block: usize) -> Self {
        let n = 2 * frame;
        let len = (2 * n + 2 * frame + max_block).next_power_of_two();
        Self {
            id,
            frame,
            hop,
            n,
            window: reference_window(frame),
            ola_gain: hop as f32 / frame as f32,
            inv_n: 1.0 / n as f32,
            fft: RealFft::new(n),
            input: vec![vec![0.0; len]; channels],
            ola: vec![vec![0.0; len]; channels],
            mask: len - 1,
            t: 0,
            pending: 0,
            frame_buf: vec![0.0; n],
            spectrum: vec![0.0; n + 2],
            multipliers: vec![0.0; n + 2],
        }
    }

    /// The short stage at 44.1/48 kHz (0x8c5f30: hop 0x80, frame 0x100).
    pub fn short(channels: usize) -> Self {
        Self::new(StageId::Short, 256, 128, channels, 128)
    }

    /// The long stage at 44.1/48 kHz (0x8c68a0: hop 0x400, frame 0x800).
    pub fn long(channels: usize, max_block: usize) -> Self {
        Self::new(StageId::Long, 2048, 1024, channels, max_block)
    }

    /// Delay of this stage in samples: its frame length.
    pub fn latency(&self) -> usize {
        self.frame
    }

    /// Push one block, run every hop it completes, and write the block's
    /// output (the OLA ring `frame` samples behind the block start).
    pub fn process(&mut self, input: &[&[f32]], output: &mut [&mut [f32]], gain: &mut impl BinGain) {
        let m = input.first().map_or(0, |s| s.len());
        let t0 = self.t;
        for (c, src) in input.iter().enumerate() {
            self.push(c, src);
        }
        self.advance(m, gain);
        for (c, out) in output.iter_mut().enumerate() {
            self.read(c, t0, out);
        }
    }

    /// Input time of the next sample to push.
    pub fn time(&self) -> usize {
        self.t
    }

    /// Write one channel's block into its input ring at the current time.
    pub fn push(&mut self, c: usize, src: &[f32]) {
        let ring = &mut self.input[c];
        for (i, &s) in src.iter().enumerate() {
            ring[(self.t + i) & self.mask] = s;
        }
    }

    /// Commit `m` pushed samples per channel and run the hops they complete.
    pub fn advance(&mut self, m: usize, gain: &mut impl BinGain) {
        self.t += m;
        self.pending += m;
        while self.pending >= self.hop {
            self.pending -= self.hop;
            let end = self.t - self.pending;
            for c in 0..self.input.len() {
                self.hop_channel(c, end, gain);
            }
        }
    }

    /// Output of channel `c` for the block that started at input time `t0`:
    /// the OLA ring from `t0 - frame` (0x8dad80).
    pub fn read(&self, c: usize, t0: usize, out: &mut [f32]) {
        let start = t0.wrapping_sub(self.frame);
        let ring = &self.ola[c];
        for (i, o) in out.iter_mut().enumerate() {
            *o = ring[start.wrapping_add(i) & self.mask];
        }
    }

    /// One hop of one channel, for the frame that ends at input time `end`.
    fn hop_channel(&mut self, c: usize, end: usize, gain: &mut impl BinGain) {
        let (frame, n, mask) = (self.frame, self.n, self.mask);
        let p = end.wrapping_sub(frame);
        // 0x8e67a0 -> 0x1190, the rest zeroed (0x10d0).
        let ring = &self.input[c];
        for (i, (d, &w)) in self.frame_buf[..frame].iter_mut().zip(&self.window).enumerate() {
            *d = ring[p.wrapping_add(i) & mask] * w;
        }
        self.frame_buf[frame..].fill(0.0);
        // 0x1010
        self.fft.forward(&self.frame_buf, &mut self.spectrum);
        // gain path, then 0x1520
        gain.bin_gains(self.id, c, &self.spectrum, &mut self.multipliers);
        mul_packed(&self.multipliers, &mut self.spectrum);
        // 0x8e6eb0: 0x1280 then 0x1220
        self.fft.inverse(&self.spectrum, &mut self.frame_buf);
        for v in &mut self.frame_buf {
            *v *= self.inv_n;
        }
        // 0x8e7000: zero `frame` floats at P + n, then 0x8e7150 -> 0x11f0
        let ola = &mut self.ola[c];
        for i in 0..frame {
            ola[p.wrapping_add(n + i) & mask] = 0.0;
        }
        for (i, &y) in self.frame_buf.iter().enumerate() {
            ola[p.wrapping_add(i) & mask] += self.ola_gain * y;
        }
    }
}

/// The normal path at one group (44.1 / 48 kHz): short stage per 128-sample
/// hop, then the long stage per block. Output is the input delayed by 2304
/// with the identity band state.
pub struct NormalPath {
    short: HopStage,
    long: HopStage,
    mid: Vec<Vec<f32>>,
}

impl NormalPath {
    pub fn new(channels: usize, max_block: usize) -> Self {
        Self {
            short: HopStage::short(channels),
            long: HopStage::long(channels, max_block),
            mid: vec![vec![0.0; max_block]; channels],
        }
    }

    pub fn latency(&self) -> usize {
        self.short.latency() + self.long.latency()
    }

    /// `block` must be a multiple of 128 (the short hop) and at most `max_block`.
    /// The short stage runs once per 128 samples (0x8c5f30's loop), then the
    /// long stage takes the whole block (0x8c68a0).
    pub fn process(&mut self, input: &[&[f32]], output: &mut [&mut [f32]], gain: &mut impl BinGain) {
        let m = input.first().map_or(0, |s| s.len());
        let hop = self.short.hop;
        assert_eq!(m % hop, 0, "block {m} is not a multiple of the short hop");
        for start in (0..m).step_by(hop) {
            let t0 = self.short.time();
            for (c, src) in input.iter().enumerate() {
                self.short.push(c, &src[start..start + hop]);
            }
            self.short.advance(hop, gain);
            for (c, mid) in self.mid.iter_mut().enumerate() {
                self.short.read(c, t0, &mut mid[start..start + hop]);
            }
        }
        let t0 = self.long.time();
        for (c, mid) in self.mid.iter().enumerate() {
            self.long.push(c, &mid[..m]);
        }
        self.long.advance(m, gain);
        for (c, out) in output.iter_mut().enumerate() {
            self.long.read(c, t0, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn fixture(name: &str) -> Value {
        let path = format!("{}/research/oracle/{name}", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
        serde_json::from_str(&text).unwrap()
    }

    fn floats(v: &Value) -> Vec<f32> {
        v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap() as f32).collect()
    }

    fn channels(v: &Value) -> Vec<Vec<f32>> {
        v.as_array().unwrap().iter().map(floats).collect()
    }

    /// `max |got - want| <= rel * max |want|` (the brief's 1e-5 relative, per array).
    fn assert_close(what: &str, got: &[f32], want: &[f32], rel: f32) {
        assert_eq!(got.len(), want.len(), "{what}: length");
        let scale = want.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(f32::MIN_POSITIVE);
        let (i, err) = got
            .iter()
            .zip(want)
            .map(|(g, w)| (g - w).abs())
            .enumerate()
            .fold((0, 0.0f32), |a, (i, e)| if e > a.1 { (i, e) } else { a });
        assert!(err <= rel * scale, "{what}: max error {err} at {i} (scale {scale})");
    }

    #[test]
    fn windows_match_the_emulated_tables_bit_for_bit() {
        let doc = fixture("stage_fixture_48000.json");
        for stage in ["short", "long"] {
            let s = &doc[stage];
            let frame = s["frame"].as_u64().unwrap() as usize;
            assert_eq!(reference_window(frame), floats(&s["window"]), "{stage} window");
        }
    }

    #[test]
    fn kernels_match_the_emulated_kernels_bit_for_bit() {
        let k = &fixture("stage_fixture_48000.json")["kernels"];
        let m = &k["mul_0x1520"];
        let mut x = floats(&m["x"]);
        mul_packed(&floats(&m["h"]), &mut x);
        assert_eq!(x, floats(&m["y"]), "0x1520");
        let o = &k["ola_0x11f0"];
        let mut dst = floats(&o["dst"]);
        add_product(&floats(&o["src"]), o["c"].as_f64().unwrap() as f32, &mut dst);
        assert_eq!(dst, floats(&o["out"]), "0x11f0");
    }

    /// Every array of one emulated hop, short and long stage, channel 0.
    #[test]
    fn hop_stages_match_the_emulated_hop() {
        let doc = fixture("stage_fixture_48000.json");
        for (stage, hop, out_offset) in [("short", 128.0f32, -128i64), ("long", 1024.0, -512)] {
            let s = &doc[stage];
            let frame = s["frame"].as_u64().unwrap() as usize;
            let n = s["fft_len"].as_u64().unwrap() as usize;
            assert_eq!(n, 2 * frame);
            // 1. window multiply (0x1190) and zero padding
            let mut windowed = vec![0.0f32; n];
            mul(&floats(&s["ring_frame"]), &reference_window(frame), &mut windowed[..frame]);
            assert_eq!(windowed, floats(&s["windowed"]), "{stage} windowed");
            // 2. forward FFT (0x1010), unscaled, packed
            let mut fft = RealFft::new(n);
            let mut spectrum = vec![0.0f32; n + 2];
            fft.forward(&windowed, &mut spectrum);
            assert_close(&format!("{stage} spectrum"), &spectrum, &floats(&s["spectrum"]), 1e-5);
            // 3. multiply by H (0x1520); the identity state gives H = (1, 0)
            let h = floats(&s["multipliers"]);
            let mut ident = vec![0.0f32; n + 2];
            IdentityGain.bin_gains(StageId::Short, 0, &spectrum, &mut ident);
            assert_eq!(h, ident, "{stage} multipliers");
            let mut product = floats(&s["spectrum"]);
            mul_packed(&h, &mut product);
            assert_eq!(product, floats(&s["product"]), "{stage} product");
            // 4. inverse FFT (0x1280), unscaled, then 1/n (0x1220)
            let mut y = vec![0.0f32; n];
            fft.inverse(&product, &mut y);
            assert_close(&format!("{stage} inverse"), &y, &floats(&s["inverse"]), 1e-5);
            let scale = s["scale"].as_f64().unwrap() as f32;
            assert_eq!(scale, 1.0 / n as f32, "{stage} scale");
            let mut scaled = floats(&s["inverse"]);
            scaled.iter_mut().for_each(|v| *v *= scale);
            assert_eq!(scaled, floats(&s["scaled"]), "{stage} scaled");
            // 5-6. zero ahead at P + n, OLA dst += (hop / frame) * y (0x11f0)
            assert_eq!(s["zero_offset"].as_u64().unwrap() as usize, n, "{stage} zero offset");
            assert_eq!(s["zero_len"].as_u64().unwrap() as usize, frame, "{stage} zero length");
            let c = s["ola_gain"].as_f64().unwrap() as f32;
            assert_eq!(c, hop / frame as f32, "{stage} OLA gain");
            let mut ola = floats(&s["ola_before"]);
            add_product(&scaled, c, &mut ola);
            assert_eq!(ola, floats(&s["ola_after"]), "{stage} OLA");
            // 7. output read position relative to P
            assert_eq!(s["out_offset"].as_i64().unwrap(), out_offset, "{stage} read offset");
        }
    }

    /// The short stage alone, 128-sample hops, against the emulated hop outputs.
    #[test]
    fn short_stage_matches_the_emulated_sequence() {
        let doc = fixture("stage_fixture_48000.json");
        let input = channels(&doc["input"]);
        let want = channels(&doc["short_out"]);
        let mut stage = HopStage::short(2);
        let mut got = vec![vec![0.0f32; input[0].len()]; 2];
        for start in (0..input[0].len()).step_by(128) {
            let ins: Vec<&[f32]> = input.iter().map(|c| &c[start..start + 128]).collect();
            let mut outs: Vec<&mut [f32]> = got.iter_mut().map(|c| &mut c[start..start + 128]).collect();
            stage.process(&ins, &mut outs, &mut IdentityGain);
        }
        for c in 0..2 {
            assert_close(&format!("short ch{c}"), &got[c], &want[c], 1e-5);
        }
    }

    fn run_chain(input: &[Vec<f32>], block: usize) -> Vec<Vec<f32>> {
        let mut path = NormalPath::new(input.len(), block);
        assert_eq!(path.latency(), 2304);
        let mut got = vec![vec![0.0f32; input[0].len()]; input.len()];
        for start in (0..input[0].len()).step_by(block) {
            let ins: Vec<&[f32]> = input.iter().map(|c| &c[start..start + block]).collect();
            let mut outs: Vec<&mut [f32]> = got.iter_mut().map(|c| &mut c[start..start + block]).collect();
            path.process(&ins, &mut outs, &mut IdentityGain);
        }
        got
    }

    /// Both stages in series against the emulated process block output.
    #[test]
    fn normal_path_matches_the_emulated_block() {
        let doc = fixture("stage_fixture_48000.json");
        let got = run_chain(&channels(&doc["input"]), 512);
        let want = channels(&doc["output"]);
        for c in 0..2 {
            assert_close(&format!("chain ch{c}"), &got[c], &want[c], 1e-5);
        }
        for rate in [44_100, 48_000] {
            let doc = fixture(&format!("process_fixture_{rate}_m0.json"));
            let got = run_chain(&channels(&doc["input"]), 512);
            let want = channels(&doc["output"]);
            for c in 0..2 {
                assert_close(&format!("{rate} ch{c}"), &got[c], &want[c], 1e-5);
            }
        }
    }
}
