//! Sidechain path of soothe3's process block (`0x890a00`), translated from the emulated binary.
//!
//! What is here, and where it comes from (evidence: `research/re-tools/emulate/sidechain_fixture.py`,
//! `fixtures/sidechain.json`; notes in `research/stereo-ms-sidechain-output.md`):
//!
//! - [`detector_source`]: the detector input mux at `0x890bee`. The block stages the analysis frame
//!   from the side table (r9) when the side pointer is non-null, else from the main table (r8). The
//!   sidechain flag (param 14, `+0xa5`, setter case `0x0e`) is not read by the block: emulated outputs
//!   are bit-identical with the flag at 0 and at 1 (checked on the output and the meter).
//! - [`sidechain_meter`]: host id 13 (`sidechain_input_level`), published at `0x893928` once per
//!   512-sample sub-block. The value is the maximum absolute sample of the side frame over the
//!   sub-block and the channels (`0x1800013d0`, which is `ippsMaxAbs_32f`; the emulated values are
//!   `|x|`, so the notes' "signed max" is wrong). With no side pointer, or a silent side, the value
//!   is 0. Unsmoothed, linear amplitude.
//! - [`listen_block`]: sidechain listen (param 15, `+0xa7`, read at `0x890a7d` into the block's
//!   stack and tested at `0x892e43`). With listen on, the output is the side signal delayed by the
//!   block latency and multiplied by the out-trim ramp, stepped on the absolute sample index. Dry and
//!   wet gains and the mix do not apply. With no side pointer the output is zero.
//! - [`focus_gains`] and [`apply_focus`]: the stereo focus law `gL = clamp(2(1-f))`,
//!   `gR = clamp(2f)` (snippet `0x892b34..0x892bf7`, verified bit-exact on the emulated snippet).
//!   `f` is the band's balance (`+0x120` of the band record; setter id `0x13 + 9*band + 6`). Gains
//!   are applied in place to the group's channel pair (IPP `ippsMulC_32f_I`, `0x180001220`).
//! - [`solo_band_index`]: setter case `0x5b` (param 91, band solo, `+0x100`). The value is clamped
//!   to `[0, 8]`, rounded half away from zero (`libm` at `0x1817bb92a`, then `cvttss2si`), and mapped
//!   to `min(r - 1, 7)` with a signed compare. Value 0 gives `-1` (no solo).
//! - [`LAYOUT_CHANNELS`] and [`layout_channels`]: `0x56cad0`, the layout index to channel count
//!   table (23 entries, `0..=22`). Read from the emulated jump table.
//!
//! Not here: the bypass-with-listen interaction (emulated output is the main input, see
//! `listen_block`), the group list and the per-group analysis loop (the emulator's group list is
//! empty, so the focus gains are only checked on the isolated snippet), the link and M/S order, and
//! layouts with more than two channels (the group loop and `0x88acb0`'s group tables are not decoded).

use super::output::{GainPlan, GainRecord, SUB_BLOCK};

/// The meter is published once per sub-block of this many samples (ceil(N / 512) values per block).
pub const METER_BLOCK: usize = SUB_BLOCK;

/// Which signal the analysis reads (`0x890bee`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetectorSource {
    /// The main input (the side pointer is null).
    Main,
    /// The side pair (the side pointer is non-null).
    Side,
}

/// The detector mux: the side pointer alone decides, not the sidechain flag.
pub fn detector_source(side_present: bool) -> DetectorSource {
    if side_present {
        DetectorSource::Side
    } else {
        DetectorSource::Main
    }
}

/// Maximum absolute sample over `channels[..][start..start + len]`, across channels (the meter's
/// `ippsMaxAbs_32f` per channel, then the running maximum). Returns 0 for an empty range.
pub fn peak_abs(channels: &[&[f32]], start: usize, len: usize) -> f32 {
    let mut peak = 0.0f32;
    for ch in channels {
        for &x in &ch[start..start + len] {
            peak = peak.max(x.abs());
        }
    }
    peak
}

/// The meter values for one block of `n` samples (`0x893928`, host id 13). Writes one value per
/// [`METER_BLOCK`] sub-block into `out` and returns how many were written. `side` is the side pair,
/// or `None` for a null side pointer (every value is 0). `out` must hold `n.div_ceil(METER_BLOCK)`.
pub fn sidechain_meter(side: Option<&[&[f32]]>, n: usize, out: &mut [f32]) -> usize {
    let blocks = n.div_ceil(METER_BLOCK);
    assert!(
        out.len() >= blocks,
        "meter output needs one slot per sub-block"
    );
    for (b, slot) in out[..blocks].iter_mut().enumerate() {
        let start = b * METER_BLOCK;
        let len = (n - start).min(METER_BLOCK);
        *slot = match side {
            Some(s) => peak_abs(s, start, len),
            None => 0.0,
        };
    }
    blocks
}

/// Sidechain listen output (`0x890a7d` / `0x892e43`) with a side pair, not bypassed.
///
/// `out[c][k] = side[c][k - delay] * g[k]` for `k >= delay`, zero before that. `g` is the out-trim
/// record ([`GainRecord`]) run over all `n` samples in sub-blocks of [`SUB_BLOCK`], so the ramp is
/// indexed by the absolute sample (the output starts `delay` samples in, after the ramp has already
/// moved). `out_rec.target` must be the block's out target (`10^(dB/20)`, or 1 when bypassed).
/// With `side == None` the output is zero.
///
/// Known gap: with bypass on, the emulated output is the delayed main input, not the detector.
pub fn listen_block(
    side: Option<&[&[f32]]>,
    delay: usize,
    out_rec: &mut GainRecord,
    out: &mut [&mut [f32]],
) {
    let n = out[0].len();
    for (c, ch) in out.iter_mut().enumerate() {
        match side {
            Some(s) => {
                ch[..delay.min(n)].fill(0.0);
                if delay < n {
                    ch[delay..n].copy_from_slice(&s[c][..n - delay]);
                }
            }
            None => ch.fill(0.0),
        }
    }
    let mut gains = [0.0f32; SUB_BLOCK];
    let mut start = 0;
    while start < n {
        let len = (n - start).min(SUB_BLOCK);
        let plan = out_rec.plan(len, &mut gains);
        for ch in out.iter_mut() {
            let x = &mut ch[start..start + len];
            match plan {
                GainPlan::Skip => {}
                GainPlan::Zero => x.fill(0.0),
                GainPlan::Ramp => {
                    for (v, g) in x.iter_mut().zip(&gains[..len]) {
                        *v *= g;
                    }
                }
            }
        }
        start += len;
    }
}

/// Clamp to `[0, 1]` as the binary does: NaN and negatives give 0 (`comiss`/`cmovae` pairs), values
/// above 1 give 1.
fn clamp_unit(x: f32) -> f32 {
    if x >= 0.0 {
        if x <= 1.0 {
            x
        } else {
            1.0
        }
    } else {
        0.0
    }
}

/// Stereo focus gains for one band balance `f` (`0x892b34..0x892bb7`): `[clamp(2(1-f)), clamp(2f)]`.
/// The binary computes `(1 - f) * 2` and `f * 2` in f32, then clamps each to `[0, 1]`.
pub fn focus_gains(balance: f32) -> [f32; 2] {
    let left = (1.0 - balance) * 2.0;
    let right = balance * 2.0;
    [clamp_unit(left), clamp_unit(right)]
}

/// Applies the focus gains in place to the group's channel pair (`0x180001220`, `ippsMulC_32f_I`:
/// `dst[i] *= gain`). `gains[0]` goes to the first channel of the pair, `gains[1]` to the second.
/// A zero gain writes `+0.0` (the emulated helper zero-fills; a plain multiply would leave `-0.0`
/// for negative samples).
pub fn apply_focus(gains: [f32; 2], left: &mut [f32], right: &mut [f32]) {
    scale_in_place(gains[0], left);
    scale_in_place(gains[1], right);
}

fn scale_in_place(gain: f32, x: &mut [f32]) {
    if gain == 0.0 {
        x.fill(0.0);
    } else {
        for v in x.iter_mut() {
            *v *= gain;
        }
    }
}

/// Band solo index for the setter value (`0x894545`, case `0x5b`). Returns `-1` for "no solo" (value
/// 0 after rounding), otherwise `0..=7`. The setter clamps to `[0, 8]` first.
pub fn solo_band_index(value: f32) -> i32 {
    let rounded = value.clamp(0.0, 8.0).round() as i32;
    (rounded - 1).min(7)
}

/// Channel count per input layout index, from `0x56cad0` (emulated jump table, indices `0..=22`).
pub const LAYOUT_CHANNELS: [usize; 23] = [
    0, 1, 2, 3, 4, 4, 5, 6, 6, 7, 7, 7, 8, 8, 8, 10, 9, 10, 12, 14, 16, 14, 16,
];

/// Channel count of a layout index, `None` outside `0..=22`.
pub fn layout_channels(index: usize) -> Option<usize> {
    LAYOUT_CHANNELS.get(index).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("fixtures/sidechain.json");

    fn doc() -> serde_json::Value {
        serde_json::from_str(FIXTURE).expect("sidechain fixture parses")
    }

    fn f32s(v: &serde_json::Value) -> Vec<f32> {
        v.as_array()
            .expect("array of f32 bit patterns")
            .iter()
            .map(|b| f32::from_bits(b.as_u64().expect("u32") as u32))
            .collect()
    }

    fn channels(v: &serde_json::Value) -> Vec<Vec<f32>> {
        v.as_array().unwrap().iter().map(f32s).collect()
    }

    #[test]
    fn detector_mux_follows_the_side_pointer_only() {
        assert_eq!(detector_source(false), DetectorSource::Main);
        assert_eq!(detector_source(true), DetectorSource::Side);
    }

    #[test]
    fn meter_matches_the_emulated_block_bit_for_bit() {
        let d = doc();
        let cases = d["meter"].as_array().unwrap();
        assert!(
            cases.len() >= 10,
            "fixture covers sizes, null and silent side"
        );
        for (idx, case) in cases.iter().enumerate() {
            let n = case["n"].as_u64().unwrap() as usize;
            let want = f32s(&case["meter"]);
            let side = (!case["side"].is_null()).then(|| channels(&case["side"]));
            let refs: Option<Vec<&[f32]>> = side
                .as_ref()
                .map(|s| s.iter().map(|c| c.as_slice()).collect());
            let mut got = vec![0.0f32; n.div_ceil(METER_BLOCK)];
            let written = sidechain_meter(refs.as_deref(), n, &mut got);
            assert_eq!(written, want.len(), "case {idx}: meter count");
            for (b, (g, w)) in got.iter().zip(&want).enumerate() {
                assert_eq!(g.to_bits(), w.to_bits(), "case {idx} block {b}");
            }
            assert_eq!(
                case["side_present"].as_bool().unwrap(),
                side.is_some(),
                "case {idx} side flag"
            );
        }
    }

    #[test]
    fn listen_matches_the_emulated_block_to_1e6() {
        let d = doc();
        let delay = d["delay"].as_u64().unwrap() as usize;
        for (idx, case) in d["listen"].as_array().unwrap().iter().enumerate() {
            let side = channels(&case["side"]);
            let refs: Vec<&[f32]> = side.iter().map(|c| c.as_slice()).collect();
            let rec_in = f32s(&case["out_rec_in"]);
            let rec_out = f32s(&case["out_rec_out"]);
            // Target is the block's out target (set by the setter); current and coeff are the constructor's.
            let mut rec = GainRecord {
                target: rec_out[0],
                current: rec_in[1],
                coeff: rec_in[2],
            };
            let n = side[0].len();
            let mut bufs: Vec<Vec<f32>> = vec![vec![0.0; n]; 2];
            let mut outs: Vec<&mut [f32]> = bufs.iter_mut().map(|b| b.as_mut_slice()).collect();
            listen_block(Some(refs.as_slice()), delay, &mut rec, &mut outs);
            let want = channels(&case["out"]);
            for c in 0..2 {
                let peak = want[c].iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-6);
                for (i, (g, w)) in bufs[c].iter().zip(&want[c]).enumerate() {
                    let err = (g - w).abs() / peak;
                    assert!(
                        err <= 1e-6,
                        "case {idx} ch {c} sample {i}: got {g}, emulated {w}"
                    );
                }
            }
        }
    }

    #[test]
    fn listen_without_side_outputs_zero() {
        let d = doc();
        let case = &d["listen_no_side"];
        let n = case["n"].as_u64().unwrap() as usize;
        let mut rec = GainRecord {
            target: 1.0,
            current: 1.0,
            coeff: 0.1,
        };
        let mut bufs: Vec<Vec<f32>> = vec![vec![0.5; n]; 2];
        let mut outs: Vec<&mut [f32]> = bufs.iter_mut().map(|b| b.as_mut_slice()).collect();
        listen_block(None, 2304, &mut rec, &mut outs);
        assert!(bufs.iter().all(|b| b.iter().all(|&v| v == 0.0)));
        let want = channels(&case["out"]);
        assert!(
            want.iter().all(|c| c.iter().all(|&v| v == 0.0)),
            "emulated listen without side is zero"
        );
    }

    #[test]
    fn focus_matches_the_emulated_snippet_bit_for_bit() {
        let d = doc();
        let cases = d["focus"].as_array().unwrap();
        assert!(cases.len() >= 17);
        for (idx, case) in cases.iter().enumerate() {
            let f = f32::from_bits(case["balance_stored"].as_u64().unwrap() as u32);
            let mut left = f32s(&case["left"]);
            let mut right = f32s(&case["right"]);
            apply_focus(focus_gains(f), &mut left, &mut right);
            for (i, (g, w)) in left.iter().zip(f32s(&case["left_out"])).enumerate() {
                assert_eq!(g.to_bits(), w.to_bits(), "case {idx} left {i}");
            }
            for (i, (g, w)) in right.iter().zip(f32s(&case["right_out"])).enumerate() {
                assert_eq!(g.to_bits(), w.to_bits(), "case {idx} right {i}");
            }
        }
    }

    #[test]
    fn focus_law_endpoints_and_clamps() {
        assert_eq!(focus_gains(0.0), [1.0, 0.0]);
        assert_eq!(focus_gains(0.5), [1.0, 1.0]);
        assert_eq!(focus_gains(1.0), [0.0, 1.0]);
        assert_eq!(focus_gains(0.75), [0.5, 1.0]);
        assert_eq!(
            focus_gains(f32::NAN),
            [0.0, 0.0],
            "NaN clamps to 0 as the comiss chain does"
        );
    }

    #[test]
    fn solo_index_matches_the_emulated_setter() {
        let d = doc();
        for (idx, case) in d["solo"].as_array().unwrap().iter().enumerate() {
            let v = f32::from_bits(case["value"].as_u64().unwrap() as u32);
            let want = case["index"].as_i64().unwrap() as i32;
            assert_eq!(solo_band_index(v), want, "case {idx} value {v}");
        }
        assert_eq!(solo_band_index(0.0), -1, "0 is no solo");
        assert_eq!(
            solo_band_index(0.5),
            0,
            "0.5 rounds away from zero to band 1"
        );
        assert_eq!(solo_band_index(9.0), 7, "clamped to band 8, index 7");
    }

    #[test]
    fn layout_table_matches_the_emulated_jump_table() {
        let d = doc();
        let rows = d["layout"].as_array().unwrap();
        assert_eq!(rows.len(), LAYOUT_CHANNELS.len());
        for row in rows {
            let i = row["index"].as_u64().unwrap() as usize;
            assert_eq!(
                layout_channels(i),
                Some(row["channels"].as_u64().unwrap() as usize),
                "layout {i}"
            );
        }
        assert_eq!(layout_channels(2), Some(2), "stereo");
        assert_eq!(layout_channels(23), None);
    }
}
