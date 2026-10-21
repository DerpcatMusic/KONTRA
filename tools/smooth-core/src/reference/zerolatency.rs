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
//! Layout (constructor state, emulated 48 kHz): size 256 -> levels [64, 64, 128, 256],
//! partitions of 33/33/65/129 bins; size 2048 -> levels [64, 64, 128, 256, 512], the
//! 512 level followed by 7 uniform 257-bin partitions in a 7-slot delay line. Level 0
//! and partition 0 are never used (the head covers IR[0, 32)).

use super::hop::{mul_packed, RealFft};

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
}
