//! Kontakt 8 Replika mode-1 chain, piece 3a (`docs/re/engine/k-replika-gaps.md`, GAP-C). Not wired: the
//! mode-1 head reader is not in the Replika loop yet.
//!
//! Post filter `141aa3e00` (`0141aa0000.c` 2217-2248, CONFIRMED arithmetic): the read-side stage of a mode-1
//! head (`141ab8c60` calls it on each channel when `H+0xc0 == 1`). Input x (xmm1). The section is the
//! `140d947a0` step (`tpt_svf::Section::tick`, coefficients from the object at `+0x30..+0x38`). The envelope
//! is `ln(|x| + 1e-4) x 8.6858902` (dB), one-pole smoothed with coefficient `+0x24`, and the gain is
//! `T4(env x 9.96578407 + 1200)` (the same table as `replika::t4`). Output `+0x54 x bp x g`, with bp = the
//! section's `+0x3c`.
//!
//! Write filter `141aa3c60` (`WriteFilter`, not wired): the per-sample write path of the same head, with the
//! waveshaper `141aa31f0` / `14199c5b0` (its trig from `14199b1d0` and `14199ae00`), the MT dither `1410ec0c0`
//! (`timemod::mt::Mt`: the same MT19937 stream, checked against the object's ring) and two TPT sections. Its
//! vector test is at 1e-4, not 1e-5 (see `WRITE_TOLERANCE`).
//!
//! Piece 3a is still partial. Open (addresses in `k-replika-gaps.md`): the design chain that sets the write
//! filter's coefficients (`141abbc90`, `141ac3fd0`, `1419529e0`, `14199dbf0`, `141ac96c0`), the MT seed of the
//! lane (its init is not read), the native trig polynomial for 1e-5, the threshold branches (|v| >= 25966 in
//! `14199ae00`, not reachable with these coefficients), and everything on the kernel side.
#![allow(dead_code)] // not wired: the mode-1 reader is not in the loop yet.

use super::t4;
use crate::dsp::timemod::mt::Mt;
use crate::dsp::tpt_svf::{Section, State};

/// `1479ab570` = 20 / ln 10 (dB per neper; f32 `410af968`, `constants.tsv`).
const DB_PER_NEPER: f32 = f32::from_bits(0x410a_f968);
/// `144f385e0` = 1e-4 (f32 `38d1b717`): the floor added to |x| before the log.
const LOG_FLOOR: f32 = f32::from_bits(0x38d1_b717);
/// `144fc44fc` = 9.96578407 (f32 `411f73da`): T4 index per dB (60 steps per octave).
const INDEX_PER_DB: f32 = f32::from_bits(0x411f_73da);
/// `144fc4b50` = 1200 (f32 `44960000`): T4 index offset.
const INDEX_OFFSET: f32 = 1200.;

/// Object fields of the post filter (byte offsets from the lane object, `H+0x56c0` or `H+0x5724`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PostFilter {
    /// `+0x20`: envelope in dB.
    pub env: f32,
    /// `+0x24`: envelope smoothing coefficient.
    pub coef: f32,
    /// `+0x28`, `+0x2c`: the two integrators of the section.
    pub s0: f32,
    pub s1: f32,
    /// `+0x30`, `+0x34`, `+0x38`: section coefficients c2, c3, c4 (`140d947a0`).
    pub c2: f32,
    pub c3: f32,
    pub c4: f32,
    /// `+0x54`: output gain.
    pub gain: f32,
}

impl PostFilter {
    /// One sample. The state updates are in the order of `141aa3e00` (section, then envelope).
    pub fn tick(&mut self, x: f32) -> f32 {
        // `140d947a0` with input x: bp and lp from the state before the step, then both integrators.
        let d = x - self.s1;
        let bp = d * self.c3 + self.s0 * self.c2;
        let lp = self.c3 * self.s0 + d * self.c4 + self.s1;
        self.s0 = 2. * bp - self.s0;
        self.s1 = 2. * lp - self.s1;

        // Envelope: `(env - v) coef + v` with v = ln(|x| + floor) x 8.6858902.
        let v = (x.abs() + LOG_FLOOR).ln() * DB_PER_NEPER;
        self.env = (self.env - v) * self.coef + v;
        let g = t4(self.env * INDEX_PER_DB + INDEX_OFFSET);
        self.gain * bp * g
    }
}

/// `2^-32` (f32 `2f800000`, `14548b7c4`): scales the MT draw to [0, 1).
const UNIT_SCALE: f32 = f32::from_bits(0x2f80_0000);
/// `1/3` (f32 `3eaaaaab`, `144760108`): the cubic antiderivative of the write filter's ADAA.
const THIRD: f32 = f32::from_bits(0x3eaa_aaab);
/// `1e-4` as f64 (`1476a36d8`): the shaper switches from ADAA to its direct form below this step.
const SHAPER_EPS: f64 = 9.999_999_747_378_752e-5;

/// Write filter `141aa3c60` on one lane object (`H+0x2dc0` L, `H+0x4240` R; 0x1480 bytes). Piece 3a, not wired.
///
/// Per sample: the gain index from the previous envelope (`T4(1200 - 9.96578 env)`), the log envelope of
/// `|gain x| + 1e-4`, one MT dither draw (`1410ec0c0`, `+0xc0`), the waveshaper `141aa31f0` on the dithered
/// input, a cubic ADAA gain, and two TPT sections in series (`+0x58`, then `+0x8c`). The output is the second
/// section's `lp` (`+0xa4`, which the call returns in xmm0).
///
/// The waveshaper (native offsets): `F(t) = t^2/2 - sum w_i cos(c_i t) - k` (`14199c5b0`, linear outside
/// `|t| > 1`, `+0x38` slope offset) and its ADAA; when the step is below 1e-4 the derivative
/// `s + sum d_i sin(c_i s)` is used at `s = clamp(t, -1, 1)` (`141aa31f0`, `14199ae00`). The native trig is
/// a single-precision polynomial: it agrees with libm to 7.6e-8 on these coefficients, and that rounding is what
/// `WRITE_TOLERANCE` covers.
#[derive(Clone)]
pub struct WriteFilter {
    /// `+0x00..+0x0c`: derivative weights d_i.
    d: [f32; 4],
    /// `+0x10..+0x1c`: cosine weights w_i.
    w: [f32; 4],
    /// `+0x20..+0x2c`: harmonic frequencies c_i.
    c: [f32; 4],
    /// `+0x30`: previous shaper input t.
    prev_t: f32,
    /// `+0x34`: shaper input gain.
    in_gain: f32,
    /// `+0x38`: slope offset of the linear branches.
    lin: f32,
    /// `+0x3c`: previous antiderivative F(t).
    prev_f: f32,
    /// `+0x40`: constant offset.
    konst: f32,
    /// `+0x50`: envelope (dB). `+0x1458` holds the same value and is the one the gain index reads.
    env: f32,
    /// `+0x54`: envelope smoothing.
    env_coef: f32,
    env_idx: f32,
    /// `+0x1448`, `+0x144c`: dither range (the draw maps to [lo, hi]); `+0x1470`: dither gain.
    dither_lo: f32,
    dither_hi: f32,
    dither_gain: f32,
    /// `+0x1464`: shaper input scale.
    in_scale: f32,
    /// `+0x1468`: output gain before the ADAA.
    out_gain: f32,
    /// `+0x146c`: ADAA feedback.
    adaa_k: f32,
    /// `+0x145c`, `+0x1460`: previous z and its cubic antiderivative.
    prev_z: f32,
    prev_cubic: f32,
    /// `+0x58..+0x6c` (state, coefficients, `bp`) and `+0x8c..+0xa4`: the two sections, with their states.
    sec_a: (Section, State),
    sec_b: (Section, State),
    /// `+0xc0`: the MT19937 state of the dither (`1410ec0c0`).
    mt: Mt,
}

impl WriteFilter {
    /// One sample, the order of `141aa3c60`.
    pub fn tick(&mut self, x: f32) -> f32 {
        let g = t4(INDEX_OFFSET - self.env_idx * INDEX_PER_DB);
        let v = ((g * x).abs() + LOG_FLOOR).ln() * DB_PER_NEPER;
        self.env = (self.env - v) * self.env_coef + v;
        self.env_idx = self.env;

        let u = self.mt.next() as f32;
        let dither = (self.dither_hi - self.dither_lo) * u * UNIT_SCALE + self.dither_lo;
        let y = self.shaper((dither * self.dither_gain + g * x) * self.in_scale);

        let z = y * self.out_gain;
        let dz = z - self.prev_z;
        let cubic = z * z * z * THIRD;
        let mut adaa = z * z;
        if LOG_FLOOR < dz {
            adaa = (cubic - self.prev_cubic) / dz;
        }
        self.prev_z = z;
        self.prev_cubic = cubic;

        let (_, lp_a) = self.sec_a.0.tick(&mut self.sec_a.1, z - adaa * self.adaa_k);
        let (_, lp_b) = self.sec_b.0.tick(&mut self.sec_b.1, lp_a);
        lp_b
    }

    /// `141aa31f0`: the waveshaper's ADAA over the step from the previous input, or its direct form.
    fn shaper(&mut self, arg: f32) -> f32 {
        let t = arg * self.in_gain;
        let d = t - self.prev_t;
        let f = self.antiderivative(t);
        let y = if SHAPER_EPS <= f64::from(d).abs() {
            ((f - self.prev_f) as f64 / f64::from(d)) as f32
        } else {
            let s = t.clamp(-1., 1.);
            let sin = |i: usize| (s * self.c[i]).sin() * self.d[i];
            (sin(2) + sin(0)) + (sin(3) + sin(1)) + s
        };
        self.prev_t = t;
        self.prev_f = f;
        y
    }

    /// `14199c5b0`: the antiderivative of the waveshaper; linear outside |t| > 1.
    fn antiderivative(&self, t: f32) -> f32 {
        if 1. < t {
            return t + self.lin;
        }
        if t < -1. {
            return self.lin - t;
        }
        let cos = |i: usize| (t * self.c[i]).cos() * self.w[i];
        let sum = ((cos(2) + cos(0)) + cos(3)) + cos(1);
        t * t * 0.5 - sum - self.konst
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const TOLERANCE: f32 = 1e-5;

    fn vector(name: &str) -> Value {
        let path = format!(
            "{}/../../docs/re/engine/vectors/replika-native-mode1-postfilter-{name}.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(path).expect("vector file"))
            .expect("vector json")
    }

    fn floats(v: &Value) -> Vec<f32> {
        v.as_array()
            .expect("array")
            .iter()
            .map(|x| x.as_f64().expect("number") as f32)
            .collect()
    }

    /// The native post filter (kemu, `141aa3e00` called on the head-0 state of the mode-1 run) matches the
    /// Rust law within 1e-5 at every sample, for the left and right objects.
    #[test]
    fn postfilter_matches_native_vectors() {
        for name in ["l", "r"] {
            let doc = vector(name);
            let init = &doc["init"];
            let get = |k: &str| init[k].as_f64().expect("field") as f32;
            let mut pf = PostFilter {
                env: get("env"),
                coef: get("coef"),
                s0: get("s0"),
                s1: get("s1"),
                c2: get("c2"),
                c3: get("c3"),
                c4: get("c4"),
                gain: get("gain"),
            };
            let (xs, ys) = (floats(&doc["x"]), floats(&doc["y"]));
            let max = xs
                .iter()
                .zip(&ys)
                .map(|(&x, &y)| (pf.tick(x) - y).abs())
                .fold(0f32, f32::max);
            eprintln!("replika mode1 postfilter {name}: max abs error {max:.3e}");
            assert!(max < TOLERANCE, "postfilter {name}: max abs error {max:e}");
        }
    }

    fn write_vector(name: &str) -> Value {
        let path = format!(
            "{}/../../docs/re/engine/vectors/replika-native-mode1-write-{name}.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(path).expect("vector file"))
            .expect("vector json")
    }

    /// The lane object as the native has it: `words` is the 0x1480-byte object (1312 u32). Offsets are the
    /// native ones (`/4` for words). The MT ring is `+0xc4..+0x1440` (1248 entries, word `48 + k` for k in
    /// 1..=1248) with the index at `+0xc0`: index p <= 624 is the block at ring[1..=624] at position p, index
    /// p > 624 is the block at ring[625..=1248] at position p - 624 (`1410ec0c0` twists into the other half).
    fn load(words: &[u32]) -> WriteFilter {
        let f = |off: usize| f32::from_bits(words[off / 4]);
        let sec = |o: usize| {
            let state = State {
                s0: f(o),
                s1: f(o + 4),
            };
            (
                Section::from_coefficients(f(o + 8), f(o + 12), f(o + 16)),
                state,
            )
        };
        let p = words[48] as usize;
        let (block, i) = if p <= 624 {
            (1..=624, p)
        } else {
            (625..=1248, p - 624)
        };
        let mut s = [0u32; 624];
        for (j, k) in block.enumerate() {
            s[j] = words[48 + k];
        }
        WriteFilter {
            d: [f(0x00), f(0x04), f(0x08), f(0x0c)],
            w: [f(0x10), f(0x14), f(0x18), f(0x1c)],
            c: [f(0x20), f(0x24), f(0x28), f(0x2c)],
            prev_t: f(0x30),
            in_gain: f(0x34),
            lin: f(0x38),
            prev_f: f(0x3c),
            konst: f(0x40),
            env: f(0x50),
            env_coef: f(0x54),
            env_idx: f(0x1458),
            dither_lo: f(0x1448),
            dither_hi: f(0x144c),
            dither_gain: f(0x1470),
            in_scale: f(0x1464),
            out_gain: f(0x1468),
            adaa_k: f(0x146c),
            prev_z: f(0x145c),
            prev_cubic: f(0x1460),
            sec_a: sec(0x58),
            sec_b: sec(0x8c),
            mt: Mt { s, i },
        }
    }

    /// Looser than `TOLERANCE`. The waveshaper's ADAA divides the antiderivative step by the input step, and
    /// some steps are small (4.4e-4 at sample 83 of the left vector, above the 1e-4 switch). The native's
    /// antiderivative differs from libm by its own f32 rounding (7.6e-8 max on these coefficients, measured
    /// against `14199c5b0`), so that division turns it into about 6e-5 at the output. Closing to 1e-5 needs
    /// the native trig polynomial (`14199b1d0`, `14199ae00`), not ported.
    const WRITE_TOLERANCE: f32 = 1e-4;

    /// The native write filter (kemu, `141aa3c60` on the head-0 lane objects after a mode-1 run, noise in)
    /// matches the Rust filter sample by sample, left and right. The dither is the native MT stream from the
    /// object's own ring, so the draws are compared too.
    #[test]
    fn write_filter_matches_native_vectors() {
        for name in ["l", "r"] {
            let doc = write_vector(name);
            let words: Vec<u32> = doc["words"]
                .as_array()
                .expect("words")
                .iter()
                .map(|v| v.as_u64().expect("u32") as u32)
                .collect();
            let mut wf = load(&words);
            let (xs, ys) = (floats(&doc["x"]), floats(&doc["y"]));
            let max = xs
                .iter()
                .zip(&ys)
                .map(|(&x, &y)| (wf.tick(x) - y).abs())
                .fold(0f32, f32::max);
            eprintln!("replika mode1 write filter {name}: max abs error {max:.3e}");
            assert!(
                max < WRITE_TOLERANCE,
                "write filter {name}: max abs error {max:e}"
            );
        }
    }
}
