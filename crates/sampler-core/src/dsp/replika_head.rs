//! Kontakt 8 Replika delay-head addressing and the output term (`docs/re/engine/k-replika-gaps.md`,
//! "Heads and feedback pass"). Generic: no product names.
//!
//! CONFIRMED (decompile arithmetic): the mode-0 ring write slot (`141acd110`); the mode-0 tap position
//! (`141ab7e00`: add the write index, wrap once each way, clamp to `[0, len - 1]`); the mode-0 offset
//! (`141ab82e0`: `min(0, s k T + b) + base`); the output term `OUT = F1 Ctl(0x60) + F2` (`141aa9480`
//! step 13) with `Ctl(0x60)` = 1.0 from the slot-1 init (`0141a90000.c:9833`).
//! INFERRED: the sign of the bias `b` (`-trunc(a k + 2)`; the text shows a uint cast and a sign-bit XOR).
//! NOT PORTED: the sample blend of the tap read (the return value of `141ab7e00` is lost in the text);
//! the playing reader (mode 1, `141ab8c60`) whose kernels `141ab8a00` and `141ab7fe0` return nothing in
//! the text and `141ab87d0` needs the unread `14199dcc0`; F1 and F2 placement, which needs heads 2 and 3.
#![allow(dead_code)] // not wired: heads 2 and 3 (mode 1 reader) are not in the Replika loop yet.

/// Ctl(0x60), F1's output gain. Slot-1 init writes 1.0 (`0141a90000.c:9833`). The bypass fade
/// (`141acc600`, `0141ac0000.c:8022`) switches it between 0.0 and 1.0 with the sub-effect enables; those
/// enables are not bound to saved fields, so the port keeps the slot-1 default.
pub const OUT_GAIN_DEFAULT: f32 = 1.0;

/// `OUT = F1 Ctl(0x60) + F2` for one channel (`141aa9480` step 13).
pub fn output_term(f1: f32, f2: f32, ctl60: f32) -> f32 {
    f1 * ctl60 + f2
}

/// Ring slot for write index `a` in a ring of `len` frames (`141acd110`, mode 0): `a` for
/// `0 <= a < len`, `a - len` for `len <= a`, `a + len` for `a < 0`.
pub fn write_slot(a: i32, len: i32) -> i32 {
    let i = if a < len { a } else { a - len };
    if i >= 0 { i } else { i + len }
}

/// Mode-0 tap position (`141ab7e00`): `p` plus the write index `a`, wrapped once each way, then
/// clamped to `[0, len - 1]`. The sample blend at this position is not ported.
pub fn tap_position(a: i32, len: i32, p: f64) -> f64 {
    let len = f64::from(len);
    let mut x = f64::from(a) + p;
    if x >= len {
        x -= len;
    }
    if x < 0. {
        x += len;
    }
    x.min(len - 1.).max(0.)
}

/// Bias `b` of the mode-0 offset (`H+0xb4`). INFERRED: `-trunc(a k + 2)`, with `a` = `H+0xa0`,
/// `k` = `H+0xb0`, and 2.0 = f32 `144714994`.
pub fn head_bias(a: f32, k: f32) -> f32 {
    -(a * k + 2.).trunc()
}

/// Mode-0 left-channel read offset (`141ab82e0`): `min(0, s k T + b) + base`, with `s` = `H+0x98`,
/// `k` = `H+0xb0`, `T` = the rotator output (`C+0x968`), `b` = `H+0xb4`, `base` = `H+0xd8`. The right
/// channel uses `g U + (1 - g) T` in place of `T`.
pub fn mode0_offset(s: f64, k: f32, t: f32, b: f32, base: f64) -> f64 {
    let v = s as f32 * k * t + b;
    f64::from(v.min(0.)) + base
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Head read offset law from the decoded constants: bias `-trunc(a k + 2)`, then `min(0, ...)`.
    #[test]
    fn mode0_head_offset_is_the_clipped_bias_law() {
        let b = head_bias(10., 1.5);
        assert_eq!(b, -17.);
        assert_eq!(mode0_offset(1., 1.5, 0., b, 0.), -17.);
        assert_eq!(mode0_offset(1., 1.5, 1., b, 0.), -15.5);
        assert_eq!(
            mode0_offset(1., 1.5, 20., b, 0.),
            0.,
            "positive excursion is clipped at zero"
        );
    }

    /// At the slot-1 default (1.0) the term is F1 + F2 bit for bit; at 0.0 it is F2 alone.
    #[test]
    fn output_term_at_default_is_f1_plus_f2() {
        let (f1, f2) = (0.25_f32, -0.5_f32);
        assert_eq!(
            output_term(f1, f2, OUT_GAIN_DEFAULT).to_bits(),
            (f1 + f2).to_bits()
        );
        assert_eq!(output_term(f1, f2, 0.).to_bits(), f2.to_bits());
    }

    #[test]
    fn write_slot_and_tap_position_wrap_the_ring() {
        let len = 8;
        assert_eq!(write_slot(3, len), 3);
        assert_eq!(write_slot(9, len), 1);
        assert_eq!(write_slot(-2, len), 6);
        assert_eq!(tap_position(6, len, 3.5), 1.5);
        assert_eq!(tap_position(0, len, -2.), 6.);
        assert_eq!(tap_position(0, len, 100.), 7.);
    }
}
