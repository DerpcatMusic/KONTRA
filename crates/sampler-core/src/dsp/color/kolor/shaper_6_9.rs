//! Kontakt Kolor shaper cases 6-9 (`cfb0`), recovered from Kontakt 8 and checked against
//! emulator vectors (`tools/re/fixtures/kolor_shaper_6_9_*.json`).
//!
//! Per sample, with `x` the input and `s` the 8-float state (`shaper[0..8]`):
//!
//! ```text
//! y  = cc70(P = shaper+0x30, V = [x, s])     returned; reads the state before this sample's update
//! s' = c830(P2 = shaper+0x2c0, x, s)         state update, written back to shaper[0..8]
//! ```
//!
//! `cc70` (see [`Cc70`]) is the readout. `c830` is a GRU cell (input size 1, hidden size 8), using
//! Eigen's `ptanh_float` for the tanh and the sigmoid built from it (`141943870`):
//!
//! ```text
//! A = M s + b                  M 24x8 column-major at P2[0x30..0xf0], b = P2[0xf0..0x108]
//! r = sigmoid(x p[i]   + p[0x18+i] + A[i])        i = 0..8
//! z = sigmoid(x p[8+i] + p[0x20+i] + A[8+i])      i = 0..8
//! n = tanh(r * A[16+i] + x p[0x10+i] + p[0x28+i])
//! s'[i] = (1 - z) n + z s[i]
//! ```
//!
//! Verified against native `c830` (`kolor_shaper_6_9_c830.json`, 1e-5) and against warmed
//! multi-block `cfb0` runs with steady and moving parameters (`kolor_shaper_6_9_sequence.json`).
//! Still open: case 9 (`1419748b0`), the inlined twin of `cfb0` that writes state back.

/// Eigen `ptanh_float`, as in `141943490` / `141943550` (clamp, rational approximation).
fn ptanh(x: f32) -> f32 {
    const A: [f32; 7] = [
        -2.7607683663038313e-16,
        2.0001879384549948e-13,
        -8.604671836165423e-11,
        5.122297253024044e-08,
        1.4857223504805006e-05,
        6.372619536705315e-04,
        4.893524572253227e-03,
    ];
    const B: [f32; 4] = [
        1.1982583600911312e-06,
        1.1853470641653985e-04,
        2.2684347350150347e-03,
        4.8935250379145145e-03,
    ];
    let c = x.clamp(-7.905311107635498, 7.905311107635498);
    if x.abs() < 0.0004 {
        return c;
    }
    let x2 = c * c;
    let p = c * ((((((A[0] * x2 + A[1]) * x2 + A[2]) * x2 + A[3]) * x2 + A[4]) * x2 + A[5]) * x2 + A[6]);
    let q = ((B[0] * x2 + B[1]) * x2 + B[2]) * x2 + B[3];
    p / q
}

/// `141943870`: `0.5 * tanh(0.5 v) + 0.5`.
fn sigmoid(v: f32) -> f32 {
    0.5 * ptanh(0.5 * v) + 0.5
}

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

/// The `c830` parameter block (`shaper+0x2c0` in the binary), 0x108 floats.
pub struct Gru {
    pub p: [f32; 0x108],
}

impl Gru {
    pub fn from_floats(p: &[f32]) -> Self {
        let mut s = Self { p: [0.; 0x108] };
        s.p.copy_from_slice(&p[..0x108]);
        s
    }

    /// One state update: `s` is replaced by the next state for input `x`.
    pub fn step(&self, x: f32, s: &mut [f32; 8]) {
        let p = &self.p;
        let mut a = [0.; 24];
        for (r, ar) in a.iter_mut().enumerate() {
            let mut acc = 0.;
            for (c, &sc) in s.iter().enumerate() {
                acc += p[0x30 + c * 24 + r] * sc;
            }
            *ar = acc + p[0xf0 + r];
        }
        for i in 0..8 {
            let r = sigmoid(x * p[i] + p[0x18 + i] + a[i]);
            let z = sigmoid(x * p[8 + i] + p[0x20 + i] + a[8 + i]);
            let n = ptanh(r * a[16 + i] + (x * p[0x10 + i] + p[0x28 + i]));
            s[i] = (1. - z) * n + z * s[i];
        }
    }
}

/// One `cfb0` lane: the readout `cc70` and the GRU state update `c830`, state in `shaper[0..8]`.
pub struct Shaper6_9 {
    cc: Cc70,
    gru: Gru,
    state: [f32; 8],
}

impl Shaper6_9 {
    /// `shaper` is the 0x6e0-byte shaper block (`shaper[0..8]` = state, P at 12, P2 at 0xb0).
    pub fn new(shaper: &[f32]) -> Self {
        let mut s = Self {
            cc: Cc70::from_floats(&shaper[12..12 + 161]),
            gru: Gru::from_floats(&shaper[0xb0..0xb0 + 0x108]),
            state: [0.; 8],
        };
        s.state.copy_from_slice(&shaper[0..8]);
        s
    }

    /// Re-reads the parameter blocks after the control lane has written them. The state is kept.
    pub fn load_params(&mut self, shaper: &[f32]) {
        self.cc = Cc70::from_floats(&shaper[12..12 + 161]);
        self.gru = Gru::from_floats(&shaper[0xb0..0xb0 + 0x108]);
    }

    pub fn state(&self) -> &[f32; 8] {
        &self.state
    }

    /// Returns `cc70(x, s)` for the current state, then advances the state with `x`.
    pub fn sample(&mut self, x: f32) -> f32 {
        let y = self.cc.sample(x, &self.state);
        self.gru.step(x, &mut self.state);
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
    fn ptanh_and_sigmoid_match_native() {
        let doc = fixture("kolor_shaper_6_9_tanh.json");
        for case in doc["cases"].as_array().unwrap() {
            let x = case["x"].as_f64().unwrap() as f32;
            let (want_t, want_s) = (case["tanh"].as_f64().unwrap() as f32, case["sigmoid"].as_f64().unwrap() as f32);
            assert!((ptanh(x) - want_t).abs() < 1e-6, "ptanh({x}) {} want {want_t}", ptanh(x));
            assert!((sigmoid(x) - want_s).abs() < 1e-6, "sigmoid({x}) {} want {want_s}", sigmoid(x));
        }
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

    #[test]
    fn c830_step_matches_native() {
        let doc = fixture("kolor_shaper_6_9_c830.json");
        for case in doc["cases"].as_array().unwrap() {
            let gru = Gru::from_floats(&floats(&case["p2"]));
            let x = case["x"].as_f64().unwrap() as f32;
            let mut s: [f32; 8] = floats(&case["s"]).try_into().unwrap();
            gru.step(x, &mut s);
            for (got, want) in s.iter().zip(floats(&case["s_next"])) {
                assert!((got - want).abs() < 1e-5, "state {got} want {want}");
            }
        }
    }

    #[test]
    fn shaper_sequence_matches_native() {
        // Warmed multi-block runs of cfb0 (state carried across samples; some blocks rewrite P/P2).
        let doc = fixture("kolor_shaper_6_9_sequence.json");
        for case in doc["cases"].as_array().unwrap() {
            let mut buf = floats(&case["init"]);
            let mut sh = Shaper6_9::new(&buf);
            for (k, step) in case["steps"].as_array().unwrap().iter().enumerate() {
                let writes = step["writes"].as_array().unwrap();
                for w in writes {
                    let at = w["at"].as_u64().unwrap() as usize;
                    let vals = floats(&w["vals"]);
                    buf[at..at + vals.len()].copy_from_slice(&vals);
                }
                if !writes.is_empty() {
                    sh.load_params(&buf);
                }
                let y = sh.sample(step["x"].as_f64().unwrap() as f32);
                let want = step["y"].as_f64().unwrap() as f32;
                assert!((y - want).abs() < 1e-5, "case step {k}: y {y} want {want}");
                for (got, want) in sh.state().iter().zip(floats(&step["state"])) {
                    assert!((got - want).abs() < 1e-5, "case step {k}: state {got} want {want}");
                }
            }
        }
    }
}
