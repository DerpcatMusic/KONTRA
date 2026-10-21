//! The multi-group path: what the process block does when
//! `groups = trunc(rate / 44100)` is 2 or more (88.2 kHz and up).
//!
//! 0x897890 runs instead of handing the block straight to 0x893de0. With
//! `D = groups` and `T = 48 * D` FIR taps:
//! 1. **Decimate** (inline loop in 0x897890): one output per `D` inputs,
//!    `d[j] = sum_i x[D*j - T + 1 + i] * dec[i]`, accumulated in f32 in tap
//!    order (`acc = x * c + acc`).
//! 2. **Process** `d` with the normal-path chain at the decimated rate
//!    (0x898a10 -> 0x8acba0 -> 0x8b1120 workers; same hop stages as
//!    [`super::hop`], so its delay is 2304 decimated samples).
//! 3. **Residual** (0x8992f0 -> 0x13a0, `dst -= src`):
//!    `r[j] = p[j] - d[j - 2304]`.
//! 4. **Interpolate** (inline loop in 0x897890): each `r[j]` adds
//!    `r[j] * itp[i]` at full-rate time `D*j + i`; the ring is cleared `D`
//!    floats at a time, 0x2000 ahead of the write position.
//! 5. **Output** (0x891bca, 0x891c02 -> 0x1370): the input delayed by
//!    `2304 * D + T` plus the interpolated residual.
//!
//! So only the band the processor changes passes through the resamplers;
//! with the identity band state `r` is zero up to rounding and the output is
//! the delayed input, full band. `T = 48 * D` is the `base` term of the
//! latency (`ReferencePlan`).
//!
//! The FIR is designed once by 0x89aed0 ([`design_fir`]); 0x8965a0 stores
//! `dec = reverse(h)` and `itp = D * h` (0x1070).
//!
//! Verified against `oracle/resample_fixture_<rate>.json` (88.2, 96, 176.4,
//! 192 kHz): the designed taps, the decimated stream, the impulse response
//! of the interpolator and the whole block output (tests below). The
//! sidechain/detector channels the reference also decimates (two more rings)
//! feed the gain path only and are not modelled here.

use super::hop::{BinGain, NormalPath};

/// Decimated-rate delay of the processing chain (short 256 + long 2048).
pub const CHAIN_DELAY: usize = 2304;

/// Ring length of the reference's resampler rings (0x4000 floats).
const RING: usize = 0x4000;

/// 0x89aed0: design the `taps`-long prototype FIR `h` at `rate`.
///
/// On `M/2 + 1` bins (`M = max(32, next_pow2(taps))`), at `f_k = k * rate / M`:
/// `r = (min(f_k, fc) + eps) / fc` with `fc = 21352.236547948156` Hz,
/// `H_k = 1 - exp(c * (log2 r)^2)` with `c = -138.1551` (f32 kernels in this
/// order: 0x1250 min, 0x12e0 +eps, 0x1220 scale, 0x1c8500 ln, 0x1220 *1/ln2,
/// 0x1340 square, 0x1220 *c, 0x1c84a0 exp, 0x1220 *-1, 0x12e0 +1).
/// The zero-phase response is the real inverse FFT (0x1280) scaled by `1/M`,
/// rotated by `taps/2` and multiplied by a 4-term Blackman-Harris window over
/// `taps` points (clipped at 0): `h[i] = ir[(i + M - taps/2) % M] * w[i]`.
pub fn design_fir(rate: f32, taps: usize) -> Vec<f32> {
    let m = 32usize.max(taps.next_power_of_two());
    let bins = m / 2 + 1;
    let fc64 = 21_352.236_547_948_156_f64;
    let fc = fc64 as f32;
    let step = 1.0 / (m / 2) as f64;
    let half = f64::from(rate * 0.5);
    let mut spectrum = vec![0.0f32; m + 2];
    for k in 0..bins {
        let f = ((k as f32) as f64 * step * half) as f32;
        let mut v = f.min(fc);
        v += 1.192_092_9e-7;
        v *= (1.0 / fc64) as f32;
        v = v.ln();
        v *= 1.442_695;
        v *= v;
        v *= -138.155_1;
        v = v.exp();
        v *= -1.0;
        v += 1.0;
        spectrum[2 * k] = v;
    }
    let mut ir = vec![0.0f32; m];
    super::hop::RealFft::new(m).inverse(&spectrum, &mut ir);
    let inv_m = 1.0 / m as f32;
    for v in &mut ir {
        *v *= inv_m;
    }
    let k = std::f64::consts::PI / (taps - 1) as f64;
    (0..taps)
        .map(|i| {
            let x = i as f64 * k;
            let w = (0.35875 - 0.48829 * (x * 2.0).cos() + 0.14128 * (x * 4.0).cos()
                - 0.01168 * (x * 6.0).cos()) as f32;
            ir[(i + m - taps / 2) % m] * w.max(0.0)
        })
        .collect()
}

/// The groups >= 2 path around the normal chain.
pub struct GroupPath {
    d: usize,
    taps: usize,
    dec: Vec<f32>,
    itp: Vec<f32>,
    dry_delay: usize,
    chain: NormalPath,
    /// Per channel: full-rate input ring, decimated input ring, interpolation ring.
    input: Vec<Vec<f32>>,
    decimated: Vec<Vec<f32>>,
    up: Vec<Vec<f32>>,
    /// Input samples pushed so far.
    t: usize,
    // Block scratch at the decimated rate: chain input and output.
    dblock: Vec<Vec<f32>>,
    pblock: Vec<Vec<f32>>,
}

impl GroupPath {
    /// `groups` is `trunc(rate / 44100)` (at least 2); `max_block` must be a
    /// multiple of `128 * groups`.
    pub fn new(rate: f32, groups: usize, channels: usize, max_block: usize) -> Self {
        assert!(groups >= 2, "the resampler only runs at two or more groups");
        let taps = 48 * groups;
        let h = design_fir(rate, taps);
        let dry_delay = CHAIN_DELAY * groups + taps;
        let in_len = (dry_delay + taps + max_block).next_power_of_two().max(RING);
        let dmax = max_block / groups;
        Self {
            d: groups,
            taps,
            dec: h.iter().rev().copied().collect(),
            itp: h.iter().map(|&v| v * groups as f32).collect(),
            dry_delay,
            chain: NormalPath::new(channels, dmax),
            input: vec![vec![0.0; in_len]; channels],
            decimated: vec![vec![0.0; RING]; channels],
            up: vec![vec![0.0; RING]; channels],
            t: 0,
            dblock: vec![vec![0.0; dmax]; channels],
            pblock: vec![vec![0.0; dmax]; channels],
        }
    }

    /// `2304 * groups + 48 * groups`, the reported latency of the normal path.
    pub fn latency(&self) -> usize {
        self.dry_delay
    }

    pub fn process(&mut self, input: &[&[f32]], output: &mut [&mut [f32]], gain: &mut impl BinGain) {
        let n = input.first().map_or(0, |s| s.len());
        let (d, taps) = (self.d, self.taps);
        assert_eq!(n % d, 0, "block {n} is not a multiple of {d}");
        let t0 = self.t;
        let in_mask = self.input[0].len() - 1;
        let nd = n / d;
        let j0 = t0 / d;
        for b in self.dblock.iter_mut() {
            b.resize(nd, 0.0); // within capacity: no allocation
        }
        for (c, src) in input.iter().enumerate() {
            let ring = &mut self.input[c];
            for (i, &s) in src.iter().enumerate() {
                ring[(t0 + i) & in_mask] = s;
            }
            // 1. decimate: newest tap at x[D*j]
            for (k, out) in self.dblock[c].iter_mut().enumerate() {
                let newest = (j0 + k) * d;
                let first = newest.wrapping_sub(taps - 1);
                let mut acc = 0.0f32;
                for (i, &cf) in self.dec.iter().enumerate() {
                    acc = ring[first.wrapping_add(i) & in_mask] * cf + acc;
                }
                *out = acc;
            }
        }
        self.t += n;
        // 2. the chain at the decimated rate
        for b in self.pblock.iter_mut() {
            b.resize(nd, 0.0); // within capacity: no allocation
        }
        self.chain.process(&self.dblock, &mut self.pblock, gain);
        for c in 0..input.len() {
            let (dring, up) = (&mut self.decimated[c], &mut self.up[c]);
            for k in 0..nd {
                let j = j0 + k;
                dring[j & (RING - 1)] = self.dblock[c][k];
                // 3. residual against the dry decimated signal 2304 back
                let r = self.pblock[c][k] - dring[j.wrapping_sub(CHAIN_DELAY) & (RING - 1)];
                // 4. interpolate: clear D floats 0x2000 ahead, then accumulate
                let w = j * d;
                for z in 0..d {
                    up[(w + 0x2000 + z) & (RING - 1)] = 0.0;
                }
                for (i, &cf) in self.itp.iter().enumerate() {
                    let slot = &mut up[(w + i) & (RING - 1)];
                    *slot = r * cf + *slot;
                }
            }
            // 5. dry input delayed by 2304*D + T, plus the interpolated residual
            let ring = &self.input[c];
            for (i, o) in output[c][..n].iter_mut().enumerate() {
                let t = t0 + i;
                *o = ring[t.wrapping_sub(self.dry_delay) & in_mask] + up[t & (RING - 1)];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reference::hop::IdentityGain;
    use crate::reference_plan::ReferencePlan;
    use serde_json::Value;

    fn fixture(rate: u32) -> Value {
        let path = format!(
            "{}/research/oracle/resample_fixture_{rate}.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
    }

    fn floats(v: &Value) -> Vec<f32> {
        v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap() as f32).collect()
    }

    fn max_err(got: &[f32], want: &[f32]) -> (f32, f32) {
        let scale = want.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let err = got.iter().zip(want).fold(0.0f32, |m, (g, w)| m.max((g - w).abs()));
        (err, scale)
    }

    const RATES: [u32; 4] = [88_200, 96_000, 176_400, 192_000];

    #[test]
    fn designed_fir_matches_the_constructor_tables() {
        for rate in RATES {
            let doc = fixture(rate);
            let groups = doc["groups"].as_u64().unwrap() as usize;
            let taps = doc["taps"].as_u64().unwrap() as usize;
            assert_eq!(taps, 48 * groups);
            let want = floats(&doc["h"]);
            let (err, scale) = max_err(&design_fir(rate as f32, taps), &want);
            assert!(err <= 1e-5 * scale, "{rate}: {err} of {scale}");
            // 0x8965a0: dec = reverse(h), itp = D * h, bit for bit
            let dec: Vec<f32> = want.iter().rev().copied().collect();
            assert_eq!(dec, floats(&doc["dec"]), "{rate} dec");
            let itp: Vec<f32> = want.iter().map(|v| v * groups as f32).collect();
            assert_eq!(itp, floats(&doc["itp"]), "{rate} itp");
        }
    }

    fn run(rate: u32, input: &[Vec<f32>], block: usize) -> (GroupPath, Vec<Vec<f32>>) {
        let plan = ReferencePlan::new(f64::from(rate), false);
        let mut path = GroupPath::new(rate as f32, plan.groups, input.len(), block);
        assert_eq!(path.latency(), plan.latency());
        let mut out = vec![vec![0.0f32; input[0].len()]; input.len()];
        for start in (0..input[0].len()).step_by(block) {
            let ins: Vec<&[f32]> = input.iter().map(|c| &c[start..start + block]).collect();
            let mut outs: Vec<&mut [f32]> = out.iter_mut().map(|c| &mut c[start..start + block]).collect();
            path.process(&ins, &mut outs, &mut IdentityGain);
        }
        (path, out)
    }

    /// Whole block output and the decimated stream against the emulator.
    #[test]
    fn group_path_matches_the_emulated_block() {
        for rate in RATES {
            let doc = fixture(rate);
            let input: Vec<Vec<f32>> = doc["input"].as_array().unwrap().iter().map(floats).collect();
            let want: Vec<Vec<f32>> = doc["output"].as_array().unwrap().iter().map(floats).collect();
            let (_, got) = run(rate, &input, 512);
            for c in 0..2 {
                let (err, scale) = max_err(&got[c], &want[c]);
                assert!(err <= 1e-5 * scale, "{rate} ch{c}: {err} of {scale}");
            }
            // decimator alone: the 0x8a3886 copies of ch0
            let groups = doc["groups"].as_u64().unwrap() as usize;
            let mut path = GroupPath::new(rate as f32, groups, 1, 512);
            let dwant = floats(&doc["decimated_ch0"]);
            let mut dgot = Vec::new();
            let mut sink = vec![0.0f32; 512];
            for block in input[0].chunks(512) {
                path.process(&[block], &mut [&mut sink], &mut IdentityGain);
                dgot.extend_from_slice(&path.dblock[0][..512 / groups]);
            }
            let (err, scale) = max_err(&dgot, &dwant);
            assert!(err <= 1e-5 * scale, "{rate} decimated: {err} of {scale}");
        }
    }

    /// r[j] = 1 at one decimated sample (set after 0x13a0 in the emulator)
    /// comes out as `itp` starting at full-rate time `D * j`.
    #[test]
    fn interpolator_matches_the_emulated_impulse() {
        for rate in RATES {
            let doc = fixture(rate);
            let groups = doc["groups"].as_u64().unwrap() as usize;
            let itp = floats(&doc["itp"]);
            let want = floats(&doc["imp_output_ch0"]);
            let j = doc["imp_block"].as_u64().unwrap() as usize * (512 / groups)
                + doc["imp_j"].as_u64().unwrap() as usize;
            let mut got = vec![0.0f32; want.len()];
            got[groups * j..groups * j + itp.len()].copy_from_slice(&itp);
            let (err, scale) = max_err(&got, &want);
            assert!(err <= 1e-6 * scale, "{rate}: {err} of {scale}");
        }
    }
}
