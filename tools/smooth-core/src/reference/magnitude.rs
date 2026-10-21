//! Complex magnitude kernel, translated from soothe3 `0x6181b0`
//! (VA `0x1806181b0`; decompile `re-corpus/corpus/fn/006181b0.c`).
//!
//! The kernel takes `dst`, `src` and a bin count `n`. `src` holds `n` interleaved
//! complex bins `(re, im)`. Stage one writes the power `re*re + im*im` per bin into
//! `dst` (x64 SSE, one product per term, no FMA). Stage two is an indirect tail call
//! through the runtime dispatch table (selector `0x21ed508`, slot 0 -> `0x1c8740` ->
//! pointer `0x21ece78` -> IPP sqrt at `0x233f40`). That is IPP code, outside the
//! plugin scope, so it is modelled here as the correctly rounded `f32::sqrt`.
//!
//! Evidence: `fixtures/magnitude.json`, emulated with the selector pinned to 0.
//! The power stage matches the emulator bit-for-bit on every bin. The full magnitude
//! is within 1 ULP of `sqrt(power)`; the 1 ULP is IPP's approximation, not translated.

/// Stage one of `0x6181b0`: `dst[k] = re_k * re_k + im_k * im_k`.
///
/// `dst.len()` is the bin count `n`. `interleaved` must hold at least `2 * n` floats.
pub fn power_bins(dst: &mut [f32], interleaved: &[f32]) {
    assert!(
        interleaved.len() >= 2 * dst.len(),
        "interleaved input shorter than 2 * bins"
    );
    for (out, bin) in dst.iter_mut().zip(interleaved.chunks_exact(2)) {
        *out = bin[0] * bin[0] + bin[1] * bin[1];
    }
}

/// `0x6181b0` in full: stage one, then the sqrt tail (modelled, see module docs).
///
/// `dst` is written in place with `|Y_k|`. Allocates nothing.
pub fn magnitude_bins(dst: &mut [f32], interleaved: &[f32]) {
    power_bins(dst, interleaved);
    for out in dst.iter_mut() {
        *out = out.sqrt();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("fixtures/magnitude.json");

    fn bits(v: &serde_json::Value) -> Vec<f32> {
        v.as_array()
            .expect("array of f32 bit patterns")
            .iter()
            .map(|b| f32::from_bits(b.as_u64().expect("u32") as u32))
            .collect()
    }

    /// ULP distance between two finite f32 of the same sign class.
    fn ulp_distance(a: f32, b: f32) -> u32 {
        a.to_bits().abs_diff(b.to_bits())
    }

    #[test]
    fn power_stage_is_bit_exact_against_emulated_kernel() {
        let fixture: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert!(cases.len() >= 40, "fixture should cover every loop shape");
        for case in cases {
            let bins = case["bins"].as_u64().unwrap() as usize;
            let input = bits(&case["in"]);
            let expected = bits(&case["power"]);
            let mut got = vec![0.0f32; bins];
            power_bins(&mut got, &input);
            for (k, (g, e)) in got.iter().zip(&expected).enumerate() {
                assert_eq!(g.to_bits(), e.to_bits(), "bins={bins} k={k}");
            }
        }
    }

    #[test]
    fn magnitude_matches_emulated_kernel_within_one_ulp() {
        let fixture: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let bins = case["bins"].as_u64().unwrap() as usize;
            let input = bits(&case["in"]);
            let expected = bits(&case["out"]);
            let mut got = vec![0.0f32; bins];
            magnitude_bins(&mut got, &input);
            for (k, (g, e)) in got.iter().zip(&expected).enumerate() {
                assert!(
                    ulp_distance(*g, *e) <= 1,
                    "bins={bins} k={k} got={g} want={e}"
                );
            }
        }
    }

    #[test]
    fn empty_block_is_a_no_op() {
        let mut dst: [f32; 0] = [];
        magnitude_bins(&mut dst, &[]);
    }
}
