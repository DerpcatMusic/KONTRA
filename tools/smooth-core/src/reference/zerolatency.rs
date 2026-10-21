//! Zero-latency path: the per-sample non-uniform partitioned convolver.
//!
//! In zero-latency mode (byte obj+0x237 = 1) 0x8acc30 runs two workers instead of the
//! STFT chain: 0x8b1630 (IR length 256, refreshed every 128-sample hop) and 0x8b3be0
//! (IR length 2048, refreshed every 512-sample block). Each worker streams every sample
//! of every channel through a convolver object with 0x8dec70 (call sites 0x8b3359 /
//! 0x8b34d9 and 0x8b5919 / 0x8b5a99). With identity gains every IR is a unit impulse and
//! the path is a pass-through (emulated, all four sites: `out == in` exactly).
//!
//! 0x8dec70 (`conv`, xmm1 = x) per sample:
//! - head 0x8df220: 32 direct taps at conv+0x40 into a 32-float output ring (conv+0x188,
//!   position at +0x208): `y = x*h[0] + ring[p]`, `ring[p] = 0`,
//!   `ring[(p+j)&31] += x*h[j]` for j = 1..32, `p = p+1 & 31`;
//! - every 32 samples, for each level i >= 1 (list +0xf0, FFT size N_i, half = N_i/2)
//!   with `pos & (half-1) == 0`: the last `half` inputs of the input ring (+0xc0) are
//!   zero-padded to N_i, forward FFT 0x1010, then
//!   - half < 256: multiply by partition i (list +0x108) with 0x1520, inverse 0x1280,
//!     scale `1.0/N` (0x1220);
//!   - half >= 256: frequency-domain delay line (slots +0x120, index +0x138):
//!     `slot[idx] += X*H_i` (0x15e0), inverse and scale `slot[idx]`, clear it (0x10d0,
//!     2*bins floats), then `slot[(idx+k) % L] += X*H_{i+k}` for the remaining
//!     partitions, `idx = idx+1 % L`; no later level runs;
//!   - the N_i result is added into the output ring (+0xd8) from `pos`;
//! - `y += out[pos]`, `in[pos] = x`, `out[pos] = 0`, `pos = pos+1 & mask` (+0x140, +0x148).
//!
//! Workers (0x8b1630 short, 0x8b3be0 long; run in series, short first):
//! - each channel has two convolvers, lists +0x2bd8 and +0x2bf0; both filter every input
//!   sample, and the stage output is `a*w_a + b*w_b` (0x8c48f0 -> 0x1580 `dst += a*b`
//!   into the ring at +0x2c08) with the fade tables `f32(sin²(πi/2h))` (the list that
//!   got the newest IR) and `f32(cos²(πi/2h))` (the other), i in the hop, h = hop;
//! - at the end of every hop (128 short, 1024 long) 0x8d8090 / 0x8d8b00 take the gain
//!   body's (0x8e04c0) log magnitudes per channel, design a minimum-phase IR with
//!   0x8df6b0 and load it with 0x8f0a10 into list `worker[0x2bd0]`, then flip that index;
//! - 0x8df6b0 (N = 512 short, 4096 long; `bins = N/2 + 1` log magnitudes `g`):
//!   real cepstrum `c = IFFT(g)/N` (0x1280, 0x1220), `c[1..N/2) *= 2.0`,
//!   `c[N/2+1..N) = 0`, `S = FFT(c)` (0x1010), `H = exp(g) * (cos, sin)(Im S)` (exp
//!   0x1c84a0, sincos 0x1c8560, 0x14c0), `ir = IFFT(H)/N`, `ir[0..bins) *= w` (0x1340)
//!   with the half-Hann `w` of [`ir_window`], samples past `bins` zeroed;
//!   0x8f0a10 loads `ir[0..N/2)`: head taps, then each partition's unscaled FFT of its
//!   zero-padded segment.
//!
//! Layout (constructor state, emulated 48 kHz): size 256 -> levels [64, 64, 128, 256],
//! partitions of 33/33/65/129 bins; size 2048 -> levels [64, 64, 128, 256, 512], the
//! 512 level followed by 7 uniform 257-bin partitions in a 7-slot delay line. Level 0
//! and partition 0 are never used (the head covers IR[0, 32)).

use super::hop::{mul_packed, RealFft, StageId};

/// Head length and block period of 0x8dec70.
pub const HEAD: usize = 32;
/// Partition half-size from which 0x8dec70 switches to the uniform delay line.
const UNIFORM_HALF: usize = 256;

/// 0x15e0: packed complex `dst[k] += a[k] * b[k]`.
pub fn mac_packed(a: &[f32], b: &[f32], dst: &mut [f32]) {
    for ((ap, bp), dp) in a.chunks_exact(2).zip(b.chunks_exact(2)).zip(dst.chunks_exact_mut(2)) {
        let (ar, ai, br, bi) = (ap[0], ap[1], bp[0], bp[1]);
        dp[0] += ar * br - ai * bi;
        dp[1] += ar * bi + ai * br;
    }
}

/// One convolver object of 0x8b1630 / 0x8b3be0, processed by 0x8dec70.
pub struct Convolver {
    mask: usize,
    head: [f32; HEAD],
    head_ring: [f32; HEAD],
    head_pos: usize,
    input: Vec<f32>,
    out: Vec<f32>,
    pos: usize,
    /// FFT size per level (+0xf0); level 0 unused.
    levels: Vec<usize>,
    ffts: Vec<RealFft>,
    /// Packed partition spectra (+0x108): one per level, then the uniform tail.
    parts: Vec<Vec<f32>>,
    fdl: Vec<Vec<f32>>,
    fdl_idx: usize,
    time: Vec<f32>,
    spec: Vec<f32>,
}

impl Convolver {
    /// Convolver for an IR of `size` samples (a power of two >= 64), with a unit impulse.
    pub fn new(size: usize) -> Self {
        assert!(size.is_power_of_two() && size >= 2 * HEAD);
        let mut levels = vec![2 * HEAD];
        let mut covered = HEAD;
        let mut n = 2 * HEAD;
        while covered < size && n / 2 < UNIFORM_HALF {
            levels.push(n);
            covered += n / 2;
            n *= 2;
        }
        let mut parts: Vec<Vec<f32>> = levels.iter().map(|&n| vec![0.0; n + 2]).collect();
        let mut slots = 0;
        if covered < size {
            levels.push(n);
            while covered < size {
                parts.push(vec![0.0; n + 2]);
                covered += n / 2;
                slots += 1;
            }
        }
        let max_n = *levels.last().unwrap();
        let mut head = [0.0; HEAD];
        head[0] = 1.0;
        Self {
            mask: size - 1,
            head,
            head_ring: [0.0; HEAD],
            head_pos: 0,
            input: vec![0.0; size],
            out: vec![0.0; size],
            pos: 0,
            ffts: levels.iter().map(|&n| RealFft::new(n)).collect(),
            levels,
            parts,
            fdl: vec![vec![0.0; max_n + 2]; slots],
            fdl_idx: 0,
            time: vec![0.0; max_n],
            spec: vec![0.0; max_n + 2],
        }
    }

    pub fn levels(&self) -> &[usize] {
        &self.levels
    }

    pub fn fdl_slots(&self) -> usize {
        self.fdl.len()
    }

    /// Direct head taps (conv+0x40).
    pub fn head_mut(&mut self) -> &mut [f32; HEAD] {
        &mut self.head
    }

    /// Packed partition spectra (+0x108), `levels.len()` + tail entries.
    pub fn parts_mut(&mut self) -> &mut [Vec<f32>] {
        &mut self.parts
    }

    /// 0x8f0a10: load a time-domain IR of `size` samples. Only the taps and partition
    /// spectra change; rings, delay line and positions keep running.
    pub fn set_ir(&mut self, ir: &[f32]) {
        let size = self.mask + 1;
        self.head.copy_from_slice(&ir[..HEAD]);
        let mut off = HEAD;
        let mut p = 1;
        while off < size {
            let level = p.min(self.levels.len() - 1);
            let n = self.levels[level];
            let half = n / 2;
            self.time[..half].copy_from_slice(&ir[off..off + half]);
            self.time[half..n].fill(0.0);
            self.ffts[level].forward(&self.time[..n], &mut self.parts[p]);
            off += half;
            p += 1;
        }
    }

    /// 0x8df220: the 32-tap direct head.
    fn head(&mut self, x: f32) -> f32 {
        let p = self.head_pos;
        let old = self.head_ring[p];
        self.head_ring[p] = 0.0;
        for j in 1..HEAD {
            let r = &mut self.head_ring[(p + j) & (HEAD - 1)];
            *r = x * self.head[j] + *r;
        }
        self.head_pos = (p + 1) & (HEAD - 1);
        x * self.head[0] + old
    }

    /// 0x8dec70: one sample in, one sample out.
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.head(x);
        let size = self.mask + 1;
        if size > HEAD && self.pos & (HEAD - 1) == 0 {
            self.blocks();
        }
        let y = y + self.out[self.pos];
        self.input[self.pos] = x;
        self.out[self.pos] = 0.0;
        self.pos = (self.pos + 1) & self.mask;
        y
    }

    fn blocks(&mut self) {
        let pos = self.pos;
        for i in 1..self.levels.len() {
            let n = self.levels[i];
            let half = n / 2;
            if pos & (half - 1) != 0 {
                continue;
            }
            let start = pos.wrapping_sub(half) & self.mask;
            let first = half.min(self.input.len() - start);
            self.time[..first].copy_from_slice(&self.input[start..start + first]);
            self.time[first..half].copy_from_slice(&self.input[..half - first]);
            self.time[half..n].fill(0.0);
            let spec = &mut self.spec[..n + 2];
            self.ffts[i].forward(&self.time[..n], spec);
            let scale = 1.0 / n as f32;
            let uniform = half >= UNIFORM_HALF;
            if !uniform {
                mul_packed(&self.parts[i], spec);
                self.ffts[i].inverse(spec, &mut self.time[..n]);
            } else {
                let slots = self.fdl.len();
                let idx = self.fdl_idx;
                mac_packed(spec, &self.parts[i], &mut self.fdl[idx]);
                self.ffts[i].inverse(&self.fdl[idx], &mut self.time[..n]);
                self.fdl[idx].fill(0.0);
                for (k, part) in self.parts[i + 1..].iter().enumerate() {
                    mac_packed(spec, part, &mut self.fdl[(idx + 1 + k) % slots]);
                }
                self.fdl_idx = (idx + 1) % slots;
            }
            for (k, &v) in self.time[..n].iter().enumerate() {
                let o = &mut self.out[(pos + k) & self.mask];
                *o = v * scale + *o;
            }
            if uniform {
                break;
            }
        }
    }
}

/// Per-hop log-magnitude source of the zero-latency stages (the gain body 0x8e04c0's
/// output as 0x8d8090 / 0x8d8b00 pass it to 0x8df6b0). `log_gain` has N/2 + 1 bins;
/// all zeros is the identity.
pub trait LogGain {
    fn log_gains(&mut self, stage: StageId, channel: usize, log_gain: &mut [f32]);
}

impl<F: FnMut(StageId, usize, &mut [f32])> LogGain for F {
    fn log_gains(&mut self, stage: StageId, channel: usize, log_gain: &mut [f32]) {
        self(stage, channel, log_gain)
    }
}

/// Identity: `ln 1 = 0` in every bin.
pub struct UnitLogGain;

impl LogGain for UnitLogGain {
    fn log_gains(&mut self, _: StageId, _: usize, log_gain: &mut [f32]) {
        log_gain.fill(0.0);
    }
}

/// Fade tables of 0x8c48f0 for a hop of `h`: (in, out) = (f32(sin²(πi/2h)), f32(cos²(πi/2h))),
/// computed in f64 (bit-exact against the emulated tables for h = 128 and 1024).
pub fn fade_tables(h: usize) -> (Vec<f32>, Vec<f32>) {
    let x = |i: usize| std::f64::consts::PI * i as f64 / (2 * h) as f64;
    ((0..h).map(|i| x(i).sin().powi(2) as f32).collect(), (0..h).map(|i| x(i).cos().powi(2) as f32).collect())
}

/// 0x8a6960 with start 0.5: the falling half-Hann the designer multiplies its IR by,
/// `len` = bins = N/2 + 1. Ramp `r = f32(i * (1/len) * (1 - 0.5) + 0.5)` in f64, then
/// `f32(max((1 - cos(r * 2π)) * 0.5, 0))` in f64; bit-exact against the emulated tables.
pub fn ir_window(len: usize) -> Vec<f32> {
    let start = 0.5f32;
    let inv = 1.0 / len as f64;
    (0..len)
        .map(|i| {
            let r = ((i as f32) as f64 * inv * (1.0 - start) as f64 + start as f64) as f32;
            ((1.0 - (r as f64 * std::f64::consts::TAU).cos()) * 0.5).max(0.0) as f32
        })
        .collect()
}

/// 0x8df6b0: minimum-phase IR from log magnitudes via the folded real cepstrum.
pub struct MinPhase {
    n: usize,
    fft: RealFft,
    spec: Vec<f32>,
    time: Vec<f32>,
    window: Vec<f32>,
}

impl MinPhase {
    pub fn new(n: usize) -> Self {
        Self { n, fft: RealFft::new(n), spec: vec![0.0; n + 2], time: vec![0.0; n], window: ir_window(n / 2 + 1) }
    }

    /// `log_gain`: N/2 + 1 bins. Writes `ir.len()` (at most N/2 + 1) windowed samples.
    pub fn design(&mut self, log_gain: &[f32], ir: &mut [f32]) {
        let n = self.n;
        let inv_n = 1.0 / n as f32;
        for (k, &g) in log_gain.iter().enumerate() {
            self.spec[2 * k] = g;
            self.spec[2 * k + 1] = 0.0;
        }
        self.fft.inverse(&self.spec, &mut self.time);
        for v in &mut self.time {
            *v *= inv_n;
        }
        for v in &mut self.time[1..n / 2] {
            *v *= 2.0;
        }
        self.time[n / 2 + 1..].fill(0.0);
        self.fft.forward(&self.time, &mut self.spec);
        for (k, &g) in log_gain.iter().enumerate() {
            let (s, c) = self.spec[2 * k + 1].sin_cos();
            let m = g.exp();
            self.spec[2 * k] = c * m;
            self.spec[2 * k + 1] = s * m;
        }
        self.fft.inverse(&self.spec, &mut self.time);
        for ((o, &v), &w) in ir.iter_mut().zip(&self.time).zip(&self.window) {
            *o = v * inv_n * w;
        }
    }
}

/// One zero-latency worker: two convolver lists per channel, crossfaded per hop.
pub struct ZlStage {
    id: StageId,
    hop: usize,
    /// Convolvers `[list][channel]`.
    lists: [Vec<Convolver>; 2],
    /// List the next IR goes to (`worker[0x2bd0]`).
    active: usize,
    phase: usize,
    fade_in: Vec<f32>,
    fade_out: Vec<f32>,
    designer: MinPhase,
    log_gain: Vec<f32>,
    ir: Vec<f32>,
}

impl ZlStage {
    pub fn new(id: StageId, hop: usize, fft: usize, channels: usize) -> Self {
        let size = fft / 2;
        let (fade_in, fade_out) = fade_tables(hop);
        let list = || (0..channels).map(|_| Convolver::new(size)).collect();
        Self {
            id,
            hop,
            lists: [list(), list()],
            active: 0,
            phase: 0,
            fade_in,
            fade_out,
            designer: MinPhase::new(fft),
            log_gain: vec![0.0; fft / 2 + 1],
            ir: vec![0.0; size],
        }
    }

    /// 0x8b1630: IR 256 (FFT 512), hop 128.
    pub fn short(channels: usize) -> Self {
        Self::new(StageId::Short, 128, 512, channels)
    }

    /// 0x8b3be0: IR 2048 (FFT 4096), hop 1024.
    pub fn long(channels: usize) -> Self {
        Self::new(StageId::Long, 1024, 4096, channels)
    }

    /// Any block length; IRs are redesigned at every hop boundary.
    pub fn process<I: AsRef<[f32]>, O: AsMut<[f32]>>(&mut self, input: &[I], output: &mut [O], gain: &mut impl LogGain) {
        let len = input.first().map_or(0, |s| s.as_ref().len());
        let mut i = 0;
        while i < len {
            let m = (self.hop - self.phase).min(len - i);
            // the list loaded last fades in; the other fades out
            let newest = self.active ^ 1;
            for (c, (src, dst)) in input.iter().zip(output.iter_mut()).enumerate() {
                let (src, dst) = (&src.as_ref()[i..i + m], &mut dst.as_mut()[i..i + m]);
                for (j, (&x, y)) in src.iter().zip(dst).enumerate() {
                    let a = self.lists[0][c].process(x);
                    let b = self.lists[1][c].process(x);
                    let (fi, fo) = (self.fade_in[self.phase + j], self.fade_out[self.phase + j]);
                    let (wa, wb) = if newest == 0 { (fi, fo) } else { (fo, fi) };
                    *y = a * wa + b * wb;
                }
            }
            self.phase += m;
            i += m;
            if self.phase == self.hop {
                self.phase = 0;
                for c in 0..self.lists[0].len() {
                    gain.log_gains(self.id, c, &mut self.log_gain);
                    self.designer.design(&self.log_gain, &mut self.ir);
                    self.lists[self.active][c].set_ir(&self.ir);
                }
                self.active ^= 1;
            }
        }
    }
}

/// The zero-latency path of 0x8acc30: short worker then long worker, latency 0.
pub struct ZeroLatencyPath {
    short: ZlStage,
    long: ZlStage,
    mid: Vec<Vec<f32>>,
}

impl ZeroLatencyPath {
    pub fn new(channels: usize, max_block: usize) -> Self {
        Self { short: ZlStage::short(channels), long: ZlStage::long(channels), mid: vec![vec![0.0; max_block]; channels] }
    }

    pub fn latency(&self) -> usize {
        0
    }

    pub fn process<I: AsRef<[f32]>, O: AsMut<[f32]>>(&mut self, input: &[I], output: &mut [O], gain: &mut impl LogGain) {
        let m = input.first().map_or(0, |s| s.as_ref().len());
        for v in &mut self.mid {
            v.resize(m, 0.0); // within the max_block capacity: no allocation
        }
        self.short.process(input, &mut self.mid, gain);
        self.long.process(&self.mid, output, gain);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn floats(v: &Value) -> Vec<f32> {
        v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap() as f32).collect()
    }

    #[test]
    fn convolver_matches_the_emulated_0x8dec70() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/research/oracle/zl_fixture_48000.json");
        let doc: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        for name in ["short", "long"] {
            let c = &doc[name];
            let size = c["size"].as_u64().unwrap() as usize;
            let mut conv = Convolver::new(size);
            let levels: Vec<usize> = c["levels"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as usize).collect();
            assert_eq!(conv.levels(), &levels[..], "{name}: levels");
            assert_eq!(conv.fdl_slots(), c["fdl_slots"].as_u64().unwrap() as usize, "{name}: slots");
            conv.head_mut().copy_from_slice(&floats(&c["head"]));
            let parts = c["parts"].as_array().unwrap();
            assert_eq!(conv.parts_mut().len(), parts.len(), "{name}: partitions");
            for (dst, src) in conv.parts_mut().iter_mut().zip(parts) {
                let src = floats(src);
                assert_eq!(dst.len(), src.len(), "{name}: partition bins");
                dst.copy_from_slice(&src);
            }
            let want = floats(&c["output"]);
            let got: Vec<f32> = floats(&c["input"]).iter().map(|&x| conv.process(x)).collect();
            let scale = want.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            let (i, err) = got.iter().zip(&want).map(|(g, w)| (g - w).abs()).enumerate().fold((0, 0.0f32), |a, (i, e)| if e > a.1 { (i, e) } else { a });
            assert!(err <= 1e-5 * scale, "{name}: max error {err} at {i} (scale {scale})");
            // the head alone (first 32 outputs) is bit-exact
            assert_eq!(got[..HEAD], want[..HEAD], "{name}: head");
        }
    }

    fn fixture() -> Value {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/research/oracle/zl_path_fixture_48000.json");
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn assert_close(what: &str, got: &[f32], want: &[f32], rel: f32) {
        assert_eq!(got.len(), want.len(), "{what}: length");
        let scale = want.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let (i, err) = got.iter().zip(want).map(|(g, w)| (g - w).abs()).enumerate().fold((0, 0.0f32), |a, (i, e)| if e > a.1 { (i, e) } else { a });
        assert!(err <= rel * scale, "{what}: max error {err} at {i} (scale {scale})");
    }

    #[test]
    fn min_phase_design_matches_the_emulated_0x8df6b0() {
        let doc = fixture();
        for name in ["short", "long"] {
            let d = &doc["direct"][name];
            let n = d["fft"].as_u64().unwrap() as usize;
            let len = d["ir_len"].as_u64().unwrap() as usize;
            let want = floats(&d["ir"]);
            assert!(want[len..].iter().all(|&v| v == 0.0), "{name}: zero tail");
            assert_eq!(ir_window(len), floats(&d["window"]), "{name}: window");
            let mut ir = vec![0.0; len];
            MinPhase::new(n).design(&floats(&d["log_gain"]), &mut ir);
            assert_close(name, &ir, &want[..len], 1e-5);
        }
    }

    #[test]
    fn fade_tables_are_complementary_squares() {
        let (fi, fo) = fade_tables(128);
        assert_eq!((fi[0], fo[0]), (0.0, 1.0));
        assert!((fi[64] - 0.5).abs() < 1e-7 && (fo[64] - 0.5).abs() < 1e-7);
        assert!(fi.iter().zip(&fo).all(|(a, b)| (a + b - 1.0).abs() < 1e-6));
    }

    #[test]
    fn zero_latency_path_matches_the_emulated_workers() {
        let doc = fixture();
        let input: Vec<Vec<f32>> = doc["input"].as_array().unwrap().iter().map(floats).collect();
        let curves = |name: &str| -> Vec<Vec<f32>> { doc["designs"][name].as_array().unwrap().iter().map(floats).collect() };
        let (short, long) = (curves("short"), curves("long"));
        let (mut si, mut li) = (0, 0);
        let mut gain = |stage: StageId, _: usize, g: &mut [f32]| {
            let src = match stage {
                StageId::Short => (&short[si], si += 1).0,
                StageId::Long => (&long[li], li += 1).0,
            };
            g.copy_from_slice(src);
        };
        let channels = input.len();
        let total = input[0].len();
        let mut path = ZeroLatencyPath::new(channels, 512);
        let mut out = vec![vec![0.0f32; total]; channels];
        let mut mid = vec![vec![0.0f32; total]; channels];
        for start in (0..total).step_by(512) {
            let blk: Vec<&[f32]> = input.iter().map(|v| &v[start..start + 512]).collect();
            let mut o: Vec<&mut [f32]> = out.iter_mut().map(|v| &mut v[start..start + 512]).collect();
            path.process(&blk, &mut o, &mut gain);
            for (c, m) in mid.iter_mut().enumerate() {
                m[start..start + 512].copy_from_slice(&path.mid[c]);
            }
        }
        assert_eq!((si, li), (short.len(), long.len()), "designs consumed");
        let want_mid: Vec<Vec<f32>> = doc["short_out"].as_array().unwrap().iter().map(floats).collect();
        let want: Vec<Vec<f32>> = doc["output"].as_array().unwrap().iter().map(floats).collect();
        for c in 0..channels {
            assert_close(&format!("short out ch{c}"), &mid[c], &want_mid[c], 1e-5);
            assert_close(&format!("output ch{c}"), &out[c], &want[c], 1e-5);
        }
    }

    #[test]
    fn identity_matches_the_emulated_zero_latency_block() {
        for rate in [44_100, 48_000] {
            let path = format!("{}/research/oracle/process_fixture_{rate}_m1.json", env!("CARGO_MANIFEST_DIR"));
            let doc: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            assert_eq!(doc["latency_getter"].as_u64(), Some(0));
            let input: Vec<Vec<f32>> = doc["input"].as_array().unwrap().iter().map(floats).collect();
            let want: Vec<Vec<f32>> = doc["output"].as_array().unwrap().iter().map(floats).collect();
            let block = doc["block_size"].as_u64().unwrap() as usize;
            let mut zl = ZeroLatencyPath::new(input.len(), block);
            let mut got = vec![vec![0.0f32; input[0].len()]; input.len()];
            for start in (0..input[0].len()).step_by(block) {
                let blk: Vec<&[f32]> = input.iter().map(|v| &v[start..start + block]).collect();
                let mut o: Vec<&mut [f32]> = got.iter_mut().map(|v| &mut v[start..start + block]).collect();
                zl.process(&blk, &mut o, &mut UnitLogGain);
            }
            for c in 0..input.len() {
                assert_close(&format!("{rate} ch{c}"), &got[c], &want[c], 1e-6);
            }
        }
    }
}
