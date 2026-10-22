//! Kolor shaper case 4 of the dispatch `1419749f0`: wrapper `14197a150`, decoded from `Kontakt8.exe`.
//!
//! The wrapper keeps a 7-float recurrent state at shaper+0..0x1c and writes one output float
//! through its scratch pointer. The dispatch epilogue reads that float as the case output.
//! The output is `w . v + b`, where `v` is the 8-lane state of the feedback network and `w` (8 floats
//! at shaper+0x260) and `b` (shaper+0x280) are zero-filled statics (`DAT_14a64ba20`/`ba30`/`ba40`).
//! Nothing in the decompile writes them, and knob changes and blocks never touch them, so the
//! output is `0.0` for every input.
//!
//! The recurrent state is therefore unobservable. It is not ported. If those statics are ever
//! populated natively, this stops matching and the native fixtures below will fail.
//!
//! Vectors: `tools/re/fixtures/kolor_shaper_4_block.json` (512 wrapper calls from a real mode-4 block)
//! and `tools/re/fixtures/kolor_shaper_4_sweep.json` (extreme and ramp inputs), made by
//! `tools/re/kolor_shaper4_kemu.py`.

use super::KolorShaper;

/// Case 4: the output stage is zero, so the output is zero for every input.
pub struct Silent;

impl KolorShaper for Silent {
    fn process(&mut self, _x: f32) -> f32 {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const BLOCK: &str =
        include_str!("../../../../../../tools/re/fixtures/kolor_shaper_4_block.json");
    const SWEEP: &str =
        include_str!("../../../../../../tools/re/fixtures/kolor_shaper_4_sweep.json");

    fn vectors(json: &str, key: &str) -> Vec<(f32, f32)> {
        let v: Value = serde_json::from_str(json).unwrap();
        v[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                (
                    r["x"].as_f64().unwrap() as f32,
                    r["out0"].as_f64().unwrap() as f32,
                )
            })
            .collect()
    }

    #[test]
    fn matches_native_block_calls() {
        let rows = vectors(BLOCK, "block");
        assert_eq!(rows.len(), 512);
        let mut shaper = Silent;
        for (x, native) in rows {
            assert!(
                (shaper.process(x) - native).abs() < 1e-5,
                "x={x} native={native}"
            );
        }
    }

    #[test]
    fn matches_native_sweep_including_extremes() {
        let rows = vectors(SWEEP, "sweep");
        let mut shaper = Silent;
        for (x, native) in rows {
            assert!(
                (shaper.process(x) - native).abs() < 1e-5,
                "x={x} native={native}"
            );
        }
    }
}
