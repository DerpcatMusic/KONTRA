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
//! Piece 3a is only this law. Open (addresses in `k-replika-gaps.md`): the write filter `141aa3c60` (needs the
//! `14199ae00` return, which the decompile text lost), its design `141abbc90` / `141ac3fd0`, the MT dither
//! `1410ec0c0`, and everything on the kernel side.
#![allow(dead_code)] // not wired: the mode-1 reader is not in the loop yet.

use super::t4;

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
        serde_json::from_str(&std::fs::read_to_string(path).expect("vector file")).expect("vector json")
    }

    fn floats(v: &Value) -> Vec<f32> {
        v.as_array().expect("array").iter().map(|x| x.as_f64().expect("number") as f32).collect()
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
}
