//! Kontakt Kolor shaper stage `cc70` (cases 6-9), recovered from Kontakt 8 and checked against
//! emulator vectors (`tools/re/fixtures/kolor_shaper_6_9_cc70.json`, `..._cfb0.json`).
//!
//! With `V = [x, s0..s7]` (x the input, s the 8-float shaper state):
//!
//! ```text
//! A = M1 V + b1          (8 lanes)
//! Q = M2 A + off         (8 lanes)
//! X = Q / sqrt(Q^2 + 1) + A
//! y = w . X + c          (the sample)
//! ```
//!
//! `cfb0` returns `y`. Its state update (c830, shaper[0..8]) is not modelled yet, so this stage is
//! exact for the first sample of a fresh state only. See docs/re/engine/kolor-shaper-cases.md.

/// The `cc70` parameter block (`shaper+0x30` in the binary). Matrices are column-major.
pub struct Cc70 {
    /// 8x9, `m1[c * 8 + r]`, applied to `V`.
    pub m1: [f32; 72],
    pub b1: [f32; 8],
    /// 8x8, `m2[c * 8 + r]`, applied to `A`.
    pub m2: [f32; 64],
    pub off: [f32; 8],
    pub w: [f32; 8],
    pub c: f32,
}

impl Cc70 {
    /// Reads the block from the binary's f32 layout (`p[0..161]`, offsets 0x000..0x280).
    pub fn from_floats(p: &[f32]) -> Self {
        let mut s = Self {
            m1: [0.; 72],
            b1: [0.; 8],
            m2: [0.; 64],
            off: [0.; 8],
            w: [0.; 8],
            c: p[160],
        };
        s.m1.copy_from_slice(&p[0..72]);
        s.b1.copy_from_slice(&p[72..80]);
        s.m2.copy_from_slice(&p[80..144]);
        s.off.copy_from_slice(&p[144..152]);
        s.w.copy_from_slice(&p[152..160]);
        s
    }

    /// The sample for input `x` and state `s`.
    pub fn sample(&self, x: f32, s: &[f32; 8]) -> f32 {
        let mut v = [0.; 9];
        v[0] = x;
        v[1..].copy_from_slice(s);
        let mut a = self.b1;
        for (c, &vc) in v.iter().enumerate() {
            for r in 0..8 {
                a[r] += self.m1[c * 8 + r] * vc;
            }
        }
        let mut y = self.c;
        for r in 0..8 {
            let mut q = self.off[r];
            for c in 0..8 {
                q += self.m2[c * 8 + r] * a[c];
            }
            let shaped = q / (q * q + 1.).sqrt();
            y += self.w[r] * (shaped + a[r]);
        }
        y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> serde_json::Value {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/re/fixtures/");
        let text = std::fs::read_to_string(format!("{path}{name}")).expect("fixture");
        serde_json::from_str(&text).expect("json")
    }

    fn floats(v: &serde_json::Value) -> Vec<f32> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_f64().unwrap() as f32)
            .collect()
    }

    #[test]
    fn cc70_matches_native() {
        let doc = fixture("kolor_shaper_6_9_cc70.json");
        for case in doc["cases"].as_array().unwrap() {
            let cc = Cc70::from_floats(&floats(&case["p"]));
            let v = floats(&case["v"]);
            let s: [f32; 8] = v[1..].try_into().unwrap();
            let y = cc.sample(v[0], &s);
            let want = case["y"].as_f64().unwrap() as f32;
            assert!((y - want).abs() < 1e-5, "y {y} want {want}");
        }
    }

    #[test]
    fn cfb0_sample_matches_native() {
        // cfb0 reads P = shaper+0x30 and S = shaper[0..8]; its returned sample is cc70's y.
        let doc = fixture("kolor_shaper_6_9_cfb0.json");
        for case in doc["cases"].as_array().unwrap() {
            let shaper = floats(&case["shaper"]);
            let cc = Cc70::from_floats(&shaper[12..12 + 161]);
            let s: [f32; 8] = shaper[0..8].try_into().unwrap();
            let y = cc.sample(case["x"].as_f64().unwrap() as f32, &s);
            let want = case["y"].as_f64().unwrap() as f32;
            assert!((y - want).abs() < 1e-5, "y {y} want {want}");
        }
    }
}
