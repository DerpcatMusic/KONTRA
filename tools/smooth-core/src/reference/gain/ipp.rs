//! The IPP vector primitives the gain body calls, as plain loops.
//!
//! soothe3 reaches IPP through dispatch thunks: `movsxd rax, [selector]; jmp [table + rax*8]`
//! with the selector at RVA `0x21ed508`. Each thunk below was identified by running it under
//! emulation (selector 0, the SSE path) on known vectors (`research/re-tools/emulate/gain_trace.py`
//! and section 21 of the notes). Element-wise ops are exact f32 operations in the order given.
//! `dot_prod` follows IPP's SSE lane order (bit-exact, see its doc). `ln`/`exp` are not
//! bit-exact models of IPP's internals (its approximations are within about 1 ULP of the
//! correctly rounded value).
//!
//! The real plug-in may dispatch to an AVX2 variant (selector 0x14) that can fuse `add_product*`
//! into FMA; the emulator pins the SSE variant, which rounds the product and the sum separately.

/// `0x1800010d0` ippsZero_32f(dst, len).
pub fn zero(dst: &mut [f32]) {
    dst.fill(0.0);
}

/// `0x1800010a0` ippsCopy_32f(src, dst, len).
pub fn copy(src: &[f32], dst: &mut [f32]) {
    dst.copy_from_slice(&src[..dst.len()]);
}

/// `0x180001100` ippsThreshold_32f_I(srcdst, len, level, relOp) with relOp 0 (less): `x < level -> level`.
pub fn threshold_lt_i(p: &mut [f32], level: f32) {
    for x in p {
        if *x < level {
            *x = level;
        }
    }
}

/// `0x180001100` with relOp 4 (greater): `x > level -> level`.
pub fn threshold_gt_i(p: &mut [f32], level: f32) {
    for x in p {
        if *x > level {
            *x = level;
        }
    }
}

/// `0x180001250` ippsThreshold_32f(src, dst, len, level, relOp 0).
pub fn threshold_lt(src: &[f32], dst: &mut [f32], level: f32) {
    for (d, &s) in dst.iter_mut().zip(src) {
        *d = if s < level { level } else { s };
    }
}

/// `0x180001250` with relOp 4.
pub fn threshold_gt(src: &[f32], dst: &mut [f32], level: f32) {
    for (d, &s) in dst.iter_mut().zip(src) {
        *d = if s > level { level } else { s };
    }
}

/// `0x180001160` ippsSubC_32f_I(val, srcdst, len): `p -= val`.
pub fn sub_c_i(val: f32, p: &mut [f32]) {
    for x in p {
        *x -= val;
    }
}

/// `0x180001220` ippsMulC_32f_I(val, srcdst, len): `p *= val`.
pub fn mul_c_i(val: f32, p: &mut [f32]) {
    for x in p {
        *x *= val;
    }
}

/// `0x1800012e0` ippsAddC_32f_I(val, srcdst, len): `p += val`.
pub fn add_c_i(val: f32, p: &mut [f32]) {
    for x in p {
        *x += val;
    }
}

/// `0x180001190` ippsMul_32f(s1, s2, dst, len).
pub fn mul(s1: &[f32], s2: &[f32], dst: &mut [f32]) {
    for ((d, &a), &b) in dst.iter_mut().zip(s1).zip(s2) {
        *d = a * b;
    }
}

/// `0x180001340` ippsMul_32f_I(src, srcdst, len): `p *= src`.
pub fn mul_i(src: &[f32], p: &mut [f32]) {
    for (x, &s) in p.iter_mut().zip(src) {
        *x *= s;
    }
}

/// `0x1800013a0` ippsSub_32f_I(src, srcdst, len): `p -= src`.
pub fn sub_i(src: &[f32], p: &mut [f32]) {
    for (x, &s) in p.iter_mut().zip(src) {
        *x -= s;
    }
}

/// `0x180001430` ippsDiv_32f_I(src, srcdst, len): `p /= src`.
pub fn div_i(src: &[f32], p: &mut [f32]) {
    for (x, &s) in p.iter_mut().zip(src) {
        *x /= s;
    }
}

/// `0x180001550` ippsDiv_32f(s1, s2, dst, len): `dst = s2 / s1`.
pub fn div(s1: &[f32], s2: &[f32], dst: &mut [f32]) {
    for ((d, &a), &b) in dst.iter_mut().zip(s1).zip(s2) {
        *d = b / a;
    }
}

/// `0x1800011c0` ippsSub_32f(s1, s2, dst, len): `dst = s2 - s1`.
pub fn sub(s1: &[f32], s2: &[f32], dst: &mut [f32]) {
    for ((d, &a), &b) in dst.iter_mut().zip(s1).zip(s2) {
        *d = b - a;
    }
}

/// `0x1800011f0` ippsAddProductC_32f(src, val, srcdst, len): `p += src * val`.
pub fn add_product_c(src: &[f32], val: f32, p: &mut [f32]) {
    for (x, &s) in p.iter_mut().zip(src) {
        *x += s * val;
    }
}

/// `0x180001580` ippsAddProduct_32f(s1, s2, srcdst, len): `p += s1 * s2`.
pub fn add_product(s1: &[f32], s2: &[f32], p: &mut [f32]) {
    for ((x, &a), &b) in p.iter_mut().zip(s1).zip(s2) {
        *x += a * b;
    }
}

/// `0x180001400` ippsDotProd_32f(s1, s2, len, &dp), SSE variant `0x1d6a510`, aligned path.
///
/// Up to 15 elements: products of pairs go to two f32 lanes (an odd tail joins lane 1), result
/// `lane1 + lane0`. From 16: four `mulps`/`addps` accumulators over blocks of 16 (lane `i % 16`),
/// a pair/single tail into lanes 0/1, then `(a0 + a1) + (a2 + a3)` lane-wise, then
/// `(v1 + v3) + (v0 + v2)`. Bit-exact against the emulated thunk on 400 random vectors
/// (16-byte aligned operands, as the body's tables and arena buffers are). Misaligned operands
/// take a scalar prologue in IPP and are not modelled.
pub fn dot_prod(s1: &[f32], s2: &[f32]) -> f32 {
    let n = s1.len().min(s2.len());
    if n <= 15 {
        let (mut l0, mut l1) = (0.0f32, 0.0f32);
        let mut i = 0;
        while i + 2 <= n {
            l0 += s1[i] * s2[i];
            l1 += s1[i + 1] * s2[i + 1];
            i += 2;
        }
        if i < n {
            l1 += s1[i] * s2[i];
        }
        return l1 + l0;
    }
    let mut acc = [0.0f32; 16];
    let mut i = 0;
    while i + 16 <= n {
        for (j, a) in acc.iter_mut().enumerate() {
            *a += s1[i + j] * s2[i + j];
        }
        i += 16;
    }
    while i + 2 <= n {
        acc[0] += s1[i] * s2[i];
        acc[1] += s1[i + 1] * s2[i + 1];
        i += 2;
    }
    if i < n {
        acc[0] += s1[i] * s2[i];
    }
    let v: [f32; 4] = std::array::from_fn(|j| (acc[j] + acc[4 + j]) + (acc[8 + j] + acc[12 + j]));
    (v[1] + v[3]) + (v[0] + v[2])
}

/// `0x1801c8500` ippsLn_32f(src, dst, len).
pub fn ln(src: &[f32], dst: &mut [f32]) {
    for (d, &s) in dst.iter_mut().zip(src) {
        *d = s.ln();
    }
}

/// `0x1801c84a0` ippsExp_32f(src, dst, len).
pub fn exp(src: &[f32], dst: &mut [f32]) {
    for (d, &s) in dst.iter_mut().zip(src) {
        *d = s.exp();
    }
}

/// In-place forms of [`ln`] and [`exp`] (the body calls both with `src == dst`).
pub fn ln_i(p: &mut [f32]) {
    for x in p {
        *x = x.ln();
    }
}

pub fn exp_i(p: &mut [f32]) {
    for x in p {
        *x = x.exp();
    }
}
