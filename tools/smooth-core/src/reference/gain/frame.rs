//! Frame builders of the gain body `0x8e04c0`: the ring cursor, the ring split and the
//! windowed frame (soothe3 `0x894a70`, `0x89ba70`, `0x8e67a0`).
//!
//! Data model (decoded from the disassembly and checked in emulation, `gain_fx_frame.py`):
//!
//! - A ring is one buffer per channel, `cap` samples long, `cap` a power of two. Its write
//!   position `pos` is a sample index in `0..cap`.
//! - A ring view (the 0x98-byte struct the body passes as `arg5`/`arg6`) names the ring and a
//!   position. The position moves backwards to take a window of samples: `0x894a70` copies the
//!   view and sets `pos' = (pos + delta) & (cap - 1)` (`delta` is the negated length, a signed
//!   32-bit value). The copy shares the channel buffers, so the window is `pos'..pos' + n`
//!   modulo `cap`.
//! - Chronological order: sample `i` of the window is `ring[(pos' + i) % cap]`.
//!   `0x89ba70` splits the window into two segments: `[pos', cap)` and then `[0, rest)`. Only
//!   the second segment exists when `pos' + n > cap`.
//! - Frames are `&mut [f32]` per channel. Object views and stack views keep channel pointers
//!   and a length; here that is the slice length. The re-length `0x894210` only copies the
//!   pointers and sets `n`, so it has no separate function here.
//!
//! The copy helpers of the binary (`0x894a70`, `0x894210`) and the split (`0x89ba70`) only
//! move pointers and integers, so they are exact. `0x8e67a0` multiplies, through the IPP
//! thunks `0x180001190` (`ippsMul_32f`, exact f32 product), so the frame is bit-exact.
//!
//! `0x8e6da0` (called only when the delta flag, setter id 16 / `cfg+0x52`, is set) runs the
//! per-record kernel `0x8dfb90` on the windowed frame. See [`pair_records`].
//!
//! Evidence: `fixtures/gain_frame.json` (synthetic rings, plus in-situ calls captured from the
//! gain body in the base and the sidechain configurations).

use std::ops::Range;

use super::ipp;

/// `0x894a70`: the window position after moving back by `delta` samples (`delta` is the
/// signed 32-bit value of `r8`, normally `-len`). Returns `(pos + delta) & (cap - 1)` with
/// 32-bit wrapping arithmetic, as `add edx, r8d; and edx, ecx` does.
///
/// `cap` must be a power of two (the ring length).
pub fn cursor_back(pos: u32, delta: i32, cap: u32) -> u32 {
    (pos as i32).wrapping_add(delta) as u32 & cap.wrapping_sub(1)
}

/// The two segments of a window, as sample ranges within one channel buffer.
///
/// `first` is always present (possibly empty). `second` is empty unless the window wraps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segments {
    pub first: Range<usize>,
    pub second: Range<usize>,
}

/// `0x89ba70`: split `n` samples starting at `pos` in a ring of `cap` samples.
///
/// No wrap (`pos + n <= cap`): `first = pos..pos + n`, `second` empty. Wrap: `first =
/// pos..cap` and `second = 0..pos + n - cap`. The binary's `iVar4 = pos - cap + n` test
/// (`test edi, edi; jle`) is the same condition.
pub fn ring_segments(cap: usize, pos: usize, n: usize) -> Segments {
    assert!(pos < cap || (pos == 0 && cap == 0), "ring position outside the ring");
    assert!(n <= cap, "window longer than the ring");
    if pos + n <= cap {
        Segments { first: pos..pos + n, second: 0..0 }
    } else {
        let first_len = cap - pos;
        Segments { first: pos..cap, second: 0..n - first_len }
    }
}

/// `0x8e67a0`: `dst[k] = ring_segment[k] * window[k]`, the ring read in chronological order.
///
/// - `dst`: one slice per output channel, all `n` long (the stack view's length).
/// - `ring`: one full channel buffer per ring channel (`cap` samples each).
/// - `pos`: the window start, as `cursor_back` gives it.
/// - `window`: one slice per window channel, at least `n` long.
///
/// Channel rule, as the binary's guards (`cmp eax, edi`, `cmp edi, 1`): when the window has
/// `dst.len()` channels, channel `k` multiplies by `window[k]`; when the window has one
/// channel, every channel uses `window[0]`; otherwise nothing is written. Channels beyond the
/// ring's channel count are left alone. The second segment uses the window samples after the
/// first segment (`window` pointers advance by `first_len * 4`, `0x8e6a10`..`0x8e6a30`).
pub fn window_frame(dst: &mut [&mut [f32]], ring: &[&[f32]], pos: usize, window: &[&[f32]]) {
    if window.len() != dst.len() && window.len() != 1 {
        return;
    }
    let n = dst.first().map_or(0, |c| c.len());
    for (k, out) in dst.iter_mut().enumerate() {
        let Some(buf) = ring.get(k) else { break };
        let w = if window.len() == 1 { window[0] } else { window[k] };
        assert!(w.len() >= n, "window shorter than the frame");
        let seg = ring_segments(buf.len(), pos, n);
        let first = seg.first;
        let second = seg.second;
        let l1 = first.len();
        ipp::mul(&buf[first], &w[..l1], &mut out[..l1]);
        let l2 = second.len();
        ipp::mul(&buf[second], &w[l1..l1 + l2], &mut out[l1..l1 + l2]);
    }
}

/// One record of the vector at `analyzer + 0x10` (0x18 bytes). `0x8dfb90` reads the channel
/// index at `+8` and the flag at `+0x10`; `+0` is not read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PairRecord {
    pub ch: usize,
    pub flag: u64,
}

/// `0x8e6da0` on the windowed frame, with `0x8dfb90` as the per-record kernel.
///
/// The binary passes two copies of the frame view (`rsp+0x30` and `rsp+0x440`, both with the
/// same channel pointers) as the input and the output of `0x8dfb90`, with flag 1 (the scale
/// flag at `[rsp+0x20]`). For each record, in order:
///
/// - `flag == 1`: the output channel `ch` is copied from the input channel `ch`. Both are the
///   same buffer, so nothing changes (the binary's `ippsCopy` is a self-copy).
/// - any other flag: with `a = frame[ch]`, `b = frame[ch + 1]`, the sum `a + b` and the
///   difference `a - b` are computed into scratch first (`0x15b0` ippsAdd, `0x11c0`
///   ippsSub with `dst = s2 - s1`), then `frame[ch] = 0.5 * sum` and
///   `frame[ch + 1] = 0.5 * diff` (`0x180001070` ippsMulC with 0.5 from `0x18209d1c0`).
///
/// `sum` and `diff` are scratch of at least `frame[0].len()` samples (the binary's arena
/// slots). The per-record order matters: a later record sees the earlier writes.
///
/// Evidence: `fixtures/gain_frame.json` (`xform`). The in-situ records vector is empty in the
/// harness (see the notes in `gain_fx_frame.py`), so the pair path is checked on synthetic
/// records only.
pub fn pair_records(
    frame: &mut [&mut [f32]],
    records: &[PairRecord],
    sum: &mut [f32],
    diff: &mut [f32],
) {
    for rec in records {
        if rec.flag == 1 {
            continue;
        }
        let (lo, hi) = frame.split_at_mut(rec.ch + 1);
        let a: &mut [f32] = &mut *lo[rec.ch];
        let b: &mut [f32] = &mut *hi[0];
        let n = a.len();
        assert!(b.len() == n, "pair channels differ in length");
        let sum = &mut sum[..n];
        let diff = &mut diff[..n];
        ipp::add(a, b, sum);
        ipp::sub(b, a, diff);
        scale_half(sum, a);
        scale_half(diff, b);
    }
}

/// `0x180001070` ippsMulC_32f with the constant 0.5 (`0x18209d1c0`): `dst = src * 0.5`.
fn scale_half(src: &[f32], dst: &mut [f32]) {
    for (d, &s) in dst.iter_mut().zip(src) {
        *d = s * 0.5;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../fixtures/gain_frame.json");

    fn floats(v: &serde_json::Value) -> Vec<f32> {
        v.as_array()
            .expect("array of f32 bit patterns")
            .iter()
            .map(|b| f32::from_bits(b.as_u64().expect("u32") as u32))
            .collect()
    }

    fn rows(v: &serde_json::Value) -> Vec<Vec<f32>> {
        v.as_array().expect("channel rows").iter().map(floats).collect()
    }

    fn fixture() -> serde_json::Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    #[test]
    fn cursor_matches_emulated_view_copy() {
        let f = fixture();
        let cases = f["cases"]["cursor"].as_array().unwrap();
        assert!(cases.len() >= 100, "fixture should cover the cursor wrap");
        for c in cases {
            let cap = c["cap"].as_u64().unwrap() as u32;
            let pos = c["pos"].as_u64().unwrap() as u32;
            let delta = c["delta"].as_i64().unwrap() as i32;
            let want = c["pos_out"].as_u64().unwrap() as u32;
            assert_eq!(cursor_back(pos, delta, cap), want, "cap {cap} pos {pos} delta {delta}");
        }
    }

    #[test]
    fn ring_split_matches_emulated_segments() {
        let f = fixture();
        let cases = f["cases"]["segments"].as_array().unwrap();
        assert!(cases.iter().any(|c| c["len2"].as_u64().unwrap() > 0), "fixture covers a wrap");
        for c in cases {
            let cap = c["cap"].as_u64().unwrap() as usize;
            let pos = c["pos"].as_u64().unwrap() as usize;
            let n = c["n"].as_u64().unwrap() as usize;
            let seg = ring_segments(cap, pos, n);
            let len1 = c["len1"].as_u64().unwrap() as usize;
            let len2 = c["len2"].as_u64().unwrap() as usize;
            assert_eq!(seg.first.len(), len1, "cap {cap} pos {pos} n {n}");
            assert_eq!(seg.second.len(), len2, "cap {cap} pos {pos} n {n}");
            assert_eq!(seg.first.start, c["start1"].as_u64().unwrap() as usize);
            if len2 > 0 {
                assert_eq!(seg.second.start, c["start2"].as_u64().unwrap() as usize);
            }
        }
    }

    #[test]
    fn window_frame_is_bit_exact_against_emulated_callee() {
        let f = fixture();
        let cases = f["cases"]["window"].as_array().unwrap();
        assert!(cases.len() >= 50, "fixture should cover the window cases");
        let mut wraps = 0;
        let mut insitu = 0;
        for c in cases {
            let pos = c["pos"].as_u64().unwrap() as usize;
            let ring = rows(&c["ring"]);
            let window = rows(&c["window"]);
            let mut dst = rows(&c["dst_in"]);
            let want = rows(&c["dst_out"]);
            let n = c["n"].as_u64().unwrap() as usize;
            if pos + n > ring.first().map_or(0, Vec::len) {
                wraps += 1;
            }
            if c["src"] != "synthetic" {
                insitu += 1;
            }
            let ring_refs: Vec<&[f32]> = ring.iter().map(Vec::as_slice).collect();
            let win_refs: Vec<&[f32]> = window.iter().map(Vec::as_slice).collect();
            let mut dst_refs: Vec<&mut [f32]> = dst.iter_mut().map(Vec::as_mut_slice).collect();
            window_frame(&mut dst_refs, &ring_refs, pos, &win_refs);
            for (k, (g, e)) in dst.iter().zip(&want).enumerate() {
                assert_eq!(g.len(), e.len());
                for (i, (a, b)) in g.iter().zip(e).enumerate() {
                    assert_eq!(a.to_bits(), b.to_bits(), "{} ch {k} sample {i}", c["src"]);
                }
            }
        }
        assert!(wraps >= 20, "fixture should include wrapped windows ({wraps})");
        assert!(insitu >= 8, "fixture should include in-situ calls ({insitu})");
    }

    #[test]
    fn pair_records_is_bit_exact_against_emulated_transform() {
        let f = fixture();
        let cases = f["cases"]["xform"].as_array().unwrap();
        assert!(cases.len() >= 40, "fixture should cover the transform");
        let mut pair_path = 0;
        for c in cases {
            let records: Vec<PairRecord> = c["records"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| PairRecord {
                    ch: r[0].as_u64().unwrap() as usize,
                    flag: r[1].as_u64().unwrap(),
                })
                .collect();
            pair_path += records.iter().filter(|r| r.flag != 1).count();
            let mut frame = rows(&c["frame_in"]);
            let want = rows(&c["frame_out"]);
            let n = frame.first().map_or(0, Vec::len);
            let mut sum = vec![0.0f32; n];
            let mut diff = vec![0.0f32; n];
            let mut refs: Vec<&mut [f32]> = frame.iter_mut().map(Vec::as_mut_slice).collect();
            pair_records(&mut refs, &records, &mut sum, &mut diff);
            for (k, (g, e)) in frame.iter().zip(&want).enumerate() {
                for (i, (a, b)) in g.iter().zip(e).enumerate() {
                    assert_eq!(a.to_bits(), b.to_bits(), "{} ch {k} sample {i}", c["src"]);
                }
            }
        }
        assert!(pair_path >= 20, "fixture should exercise the pair path ({pair_path})");
    }
}
