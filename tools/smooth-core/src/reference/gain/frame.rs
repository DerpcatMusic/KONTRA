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
}
