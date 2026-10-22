//! Kontakt Kolor shaper cases 0, 1 and 3 (dispatch `1419749f0`, selector byte at shaper+0x6f0),
//! checked against warmed native runs of the dispatch (`tools/re/fixtures/kolor_shaper_{0,1_3}_sequence.json`).
//!
//! Each case is the cfb0 sequence, the same as cases 6-8 (`shaper_6_9`):
//!
//! ```text
//! y  = cc70(P = shaper+0x30, V = [x, s])     returned in xmm0; reads the state before this sample's update
//! s' = c830(P2 = shaper+0x2c0, x, s)         state update, written back to shaper[0..8]
//! ```
//!
//! Case 0 builds `V = [x, s0..s7]` on the stack. Cases 1 and 3 (identical code) stage the same nine floats
//! through the 1-element wrap `1419426e0` and the 8-float ring push `141974860`, which writes `s` at float 1..8
//! of the buffer after `x`. The buffer is then passed to `cc70` as `V`. Both give `V = [x, s]`, so the
//! numbers match case 0 and [`Shaper6_9`] exactly.

pub use super::shaper_6_9::Shaper6_9;

/// Case 0 (`141974a2e`): cc70 then c830 on the 9-float `[x, s]`. Same as [`Shaper6_9`].
pub type Shaper0 = Shaper6_9;

/// Cases 1 and 3: the ring-staged `[x, s]`, then cc70 and c830. Same as [`Shaper6_9`].
pub type Shaper1_3 = Shaper6_9;

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

    /// Replays a warmed dispatch run: the block is rewritten at the recorded steps (P2 / P), the state is
    /// carried across samples, and each sample's `y` and post-call state must match native.
    fn replay(doc: &serde_json::Value, label: &str, check_sel: bool) {
        for case in doc["cases"].as_array().unwrap() {
            if check_sel {
                let sel = case["sel"].as_i64().unwrap();
                assert!(sel == 1 || sel == 3, "{label}: unexpected selector {sel}");
            }
            let mut buf = floats(&case["init"]);
            let mut sh = Shaper0::new(&buf);
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
                assert!(
                    (y - want).abs() < 1e-5,
                    "{label} step {k}: y {y} want {want}"
                );
                for (got, want) in sh.state().iter().zip(floats(&step["state"])) {
                    assert!(
                        (got - want).abs() < 1e-5,
                        "{label} step {k}: state {got} want {want}"
                    );
                }
            }
        }
    }

    #[test]
    fn case0_sequence_matches_native() {
        replay(&fixture("kolor_shaper_0_sequence.json"), "case 0", false);
    }

    #[test]
    fn case1_3_sequence_matches_native() {
        replay(&fixture("kolor_shaper_1_3_sequence.json"), "case 1/3", true);
    }
}
