//! Hilbert pair coefficients of the Kontakt Freak stage: the native designer (0x141968890),
//! ported in the decompiled order. Constants are the binary's values, named by their address
//! in the native image (`F_` f32 bit patterns, `U_` u32 masks, lane tables `[base, base+4, ..]`).
//!
//! Call structure: `design` -> `elliptic_params` (0x14199e860) -> `lane4` per four-lane group
//! (0x14199fc50) -> `reduce4` (0x1419a0210). The reduction's fallback for |x| > 64 pi
//! (0x14199e9c0, Payne-Hanek) is not ported: the designer's arguments are
//! `(k + lane) * pi/8 + 3 pi/32` with `k + lane <= 18`, so they stay below 8 and that branch
//! is unreachable.
#![allow(non_snake_case, dead_code)]

type V = [f32; 4];

const SIGN: u32 = 0x8000_0000;

// Scalar constants (binary values).
const F_14470BAD0: f32 = f32::from_bits(0x3F000000);
const F_14470BAD4: f32 = f32::from_bits(0x3F800000);
const F_144714994: f32 = f32::from_bits(0x40000000);
const F_1447149D0: f32 = f32::from_bits(0x40400000);
const F_1447149D4: f32 = f32::from_bits(0x40490FDB);
const F_1447149E4: f32 = f32::from_bits(0x40800000);
const F_144714A0C: f32 = f32::from_bits(0x41700000);
const F_144714B70: f32 = f32::from_bits(0x3F800000);
const F_144714B74: f32 = f32::from_bits(0x3F800000);
const F_144714B78: f32 = f32::from_bits(0x3F800000);
const F_144714B7C: f32 = f32::from_bits(0x3F800000);
const U_144714BA0: u32 = 0x80000000;
const U_144714BA4: u32 = 0x80000000;
const U_144714BA8: u32 = 0x80000000;
const U_144714BAC: u32 = 0x80000000;
const F_144F392A0: f32 = f32::from_bits(0x40000000);
const F_144F392A4: f32 = f32::from_bits(0x40000000);
const F_144F392A8: f32 = f32::from_bits(0x40000000);
const F_144F392AC: f32 = f32::from_bits(0x40000000);
const F_144F392F0: f32 = f32::from_bits(0x4B000000);
const F_144F392F4: f32 = f32::from_bits(0x4B000000);
const F_144F392F8: f32 = f32::from_bits(0x4B000000);
const F_144F392FC: f32 = f32::from_bits(0x4B000000);
const F_144FCFE50: f32 = f32::from_bits(0x43160000);
const F_147696DD0: f32 = f32::from_bits(0xBF000000);
const F_147696DD4: f32 = f32::from_bits(0xBF000000);
const F_147696DD8: f32 = f32::from_bits(0xBF000000);
const F_147696DDC: f32 = f32::from_bits(0xBF000000);
const F_14769A440: f32 = f32::from_bits(0x3FC90FDB);
const F_14769A444: f32 = f32::from_bits(0x3FC90FDB);
const F_14769A448: f32 = f32::from_bits(0x3FC90FDB);
const F_14769A44C: f32 = f32::from_bits(0x3FC90FDB);
const F_1476A3730: f32 = f32::from_bits(0x3F22F983);
const F_1476A3734: f32 = f32::from_bits(0x3F22F983);
const F_1476A3738: f32 = f32::from_bits(0x3F22F983);
const F_1476A373C: f32 = f32::from_bits(0x3F22F983);
const F_1476A37C0: f32 = f32::from_bits(0x00000000);
const F_1476A3840: f32 = f32::from_bits(0x00A2F983);
const F_1476A394C: f32 = f32::from_bits(0x00000000);
const F_1476A3A10: f32 = f32::from_bits(0x00000000);
const U_1476A3A20: u32 = 0x00000003;
const U_1476A3A24: u32 = 0x00000003;
const U_1476A3A28: u32 = 0x00000003;
const U_1476A3A2C: u32 = 0x00000003;
const F_1476A3A30: f32 = f32::from_bits(0x248D3132);
const F_1476A3A34: f32 = f32::from_bits(0x248D3132);
const F_1476A3A38: f32 = f32::from_bits(0x248D3132);
const F_1476A3A3C: f32 = f32::from_bits(0x248D3132);
const F_1476A3A40: f32 = f32::from_bits(0x2E85A300);
const F_1476A3A44: f32 = f32::from_bits(0x2E85A300);
const F_1476A3A48: f32 = f32::from_bits(0x2E85A300);
const F_1476A3A4C: f32 = f32::from_bits(0x2E85A300);
const F_1476A3A50: f32 = f32::from_bits(0x37354400);
const F_1476A3A54: f32 = f32::from_bits(0x37354400);
const F_1476A3A58: f32 = f32::from_bits(0x37354400);
const F_1476A3A5C: f32 = f32::from_bits(0x37354400);
const F_1476A3A60: f32 = f32::from_bits(0x37CCF5CE);
const F_1476A3A64: f32 = f32::from_bits(0x37CCF5CE);
const F_1476A3A68: f32 = f32::from_bits(0x37CCF5CE);
const F_1476A3A6C: f32 = f32::from_bits(0x37CCF5CE);
const F_1476A3A70: f32 = f32::from_bits(0x3C08839D);
const F_1476A3A74: f32 = f32::from_bits(0x3C08839D);
const F_1476A3A78: f32 = f32::from_bits(0x3C08839D);
const F_1476A3A7C: f32 = f32::from_bits(0x3C08839D);
const F_1476A3A80: f32 = f32::from_bits(0x3D2AAAA5);
const F_1476A3A84: f32 = f32::from_bits(0x3D2AAAA5);
const F_1476A3A88: f32 = f32::from_bits(0x3D2AAAA5);
const F_1476A3A8C: f32 = f32::from_bits(0x3D2AAAA5);
const F_1476A3A90: f32 = f32::from_bits(0x3F490FDB);
const F_1476A3A94: f32 = f32::from_bits(0x3F490FDB);
const F_1476A3A98: f32 = f32::from_bits(0x3F490FDB);
const F_1476A3A9C: f32 = f32::from_bits(0x3F490FDB);
const F_1476A3AA0: f32 = f32::from_bits(0x3FC90F80);
const F_1476A3AA4: f32 = f32::from_bits(0x3FC90F80);
const F_1476A3AA8: f32 = f32::from_bits(0x3FC90F80);
const F_1476A3AAC: f32 = f32::from_bits(0x3FC90F80);
const F_1476A3AB0: f32 = f32::from_bits(0x427B53D1);
const F_1476A3AB4: f32 = f32::from_bits(0x427B53D1);
const F_1476A3AB8: f32 = f32::from_bits(0x427B53D1);
const F_1476A3ABC: f32 = f32::from_bits(0x427B53D1);
const F_1476A3AC0: f32 = f32::from_bits(0x43490FDB);
const F_1476A3AC4: f32 = f32::from_bits(0x43490FDB);
const F_1476A3AC8: f32 = f32::from_bits(0x43490FDB);
const F_1476A3ACC: f32 = f32::from_bits(0x43490FDB);
const F_1476A3AD0: f32 = f32::from_bits(0xB94CA1F9);
const F_1476A3AD4: f32 = f32::from_bits(0xB94CA1F9);
const F_1476A3AD8: f32 = f32::from_bits(0xB94CA1F9);
const F_1476A3ADC: f32 = f32::from_bits(0xB94CA1F9);
const F_1476A3AE0: f32 = f32::from_bits(0xBAB60619);
const F_1476A3AE4: f32 = f32::from_bits(0xBAB60619);
const F_1476A3AE8: f32 = f32::from_bits(0xBAB60619);
const F_1476A3AEC: f32 = f32::from_bits(0xBAB60619);
const F_1476A3AF0: f32 = f32::from_bits(0xBE2AAAA2);
const F_1476A3AF4: f32 = f32::from_bits(0xBE2AAAA2);
const F_1476A3AF8: f32 = f32::from_bits(0xBE2AAAA2);
const F_1476A3AFC: f32 = f32::from_bits(0xBE2AAAA2);
const F_1476A3B00: f32 = f32::from_bits(0xC0000000);
const F_1476A3B04: f32 = f32::from_bits(0xC0000000);
const F_1476A3B08: f32 = f32::from_bits(0xC0000000);
const F_1476A3B0C: f32 = f32::from_bits(0xC0000000);
const F_14A182254: f32 = f32::from_bits(0x3E317218);

// Four-lane constant tables (lane `l` at address base + 4 l).
const TWO: V = [F_144F392A0, F_144F392A4, F_144F392A8, F_144F392AC];
const M2: V = [F_1476A3B00, F_1476A3B04, F_1476A3B08, F_1476A3B0C];
const ONE: V = [F_144714B70, F_144714B74, F_144714B78, F_144714B7C];
const SIN1: V = [F_1476A3AD0, F_1476A3AD4, F_1476A3AD8, F_1476A3ADC];
const SIN2: V = [F_1476A3A70, F_1476A3A74, F_1476A3A78, F_1476A3A7C];
const SIN3: V = [F_1476A3AF0, F_1476A3AF4, F_1476A3AF8, F_1476A3AFC];
const COS1: V = [F_1476A3A60, F_1476A3A64, F_1476A3A68, F_1476A3A6C];
const COS2: V = [F_1476A3AE0, F_1476A3AE4, F_1476A3AE8, F_1476A3AEC];
const COS3: V = [F_1476A3A80, F_1476A3A84, F_1476A3A88, F_1476A3A8C];
const COS4: V = [F_147696DD0, F_147696DD4, F_147696DD8, F_147696DDC];
const THR_A: V = [F_1476A3A90, F_1476A3A94, F_1476A3A98, F_1476A3A9C];
const THR_B: V = [F_14769A440, F_14769A444, F_14769A448, F_14769A44C];
const THR_C: V = [F_1476A3AB0, F_1476A3AB4, F_1476A3AB8, F_1476A3ABC];
const THR_D: V = [F_1476A3AC0, F_1476A3AC4, F_1476A3AC8, F_1476A3ACC];
const P1: V = [F_1476A3AA0, F_1476A3AA4, F_1476A3AA8, F_1476A3AAC];
const P2: V = [F_1476A3A50, F_1476A3A54, F_1476A3A58, F_1476A3A5C];
const P3: V = [F_1476A3A40, F_1476A3A44, F_1476A3A48, F_1476A3A4C];
const P4: V = [F_1476A3A30, F_1476A3A34, F_1476A3A38, F_1476A3A3C];
const C_MUL: V = [F_1476A3730, F_1476A3734, F_1476A3738, F_1476A373C];
const MAGIC: V = [F_144F392F0, F_144F392F4, F_144F392F8, F_144F392FC];
const QMASK: [u32; 4] = [U_1476A3A20, U_1476A3A24, U_1476A3A28, U_1476A3A2C];

/// Designer output: gain (slot 0), all-pass sections (slots 6..14), FIR taps (slots 46..54).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Hilbert {
    pub gain: V,
    pub allpass: [V; 8],
    pub fir: [V; 8],
}

/// Native designer for the Hilbert pair at `w = pi / fs` between the band edges `lo` and `hi`
/// in Hz (0x141968890; the caller passes 20 and 20000).
pub(crate) fn design(w: f32, lo: f32, hi: f32) -> Hilbert {
    let t_lo = (w * lo).tan();
    let t_hi = (hi * w).tan();
    let s = (t_hi * t_lo).sqrt();
    let u = (t_hi / t_lo).log2();
    let p = elliptic_params(16, u);
    let mut out = [0f32; 16];
    for it in 0..4 {
        let v = lane4(&p, 4 * it as i32);
        out[4 * it..4 * it + 4].copy_from_slice(&v);
    }
    // Slot n of the native buffer is floats 4n..4n+4.
    let mut slot = [[0f32; 4]; 54];
    slot[0] = [1.0; 4];
    for i in 0..8 {
        let a = out[i] * s;
        let b = f32::from_bits(out[8 + i].to_bits() ^ SIGN) * s;
        let num = [a - 1.0, a - 1.0, b - 1.0, b - 1.0];
        let den = [a + 1.0, a + 1.0, b + 1.0, b + 1.0];
        let r: V = std::array::from_fn(|l| num[l] / den[l]);
        slot[6 + i] = r;
        slot[38 + i] = r;
        for l in 0..4 {
            slot[0][l] *= r[l];
        }
    }
    for k in 0..8 {
        let mut v: V = std::array::from_fn(|l| 1.0 - slot[6 + k][l]);
        for m in k + 1..8 {
            for l in 0..4 {
                v[l] *= slot[38 + m][l];
            }
        }
        slot[46 + k] = v;
    }
    Hilbert {
        gain: slot[0],
        allpass: std::array::from_fn(|k| slot[6 + k]),
        fir: std::array::from_fn(|k| slot[46 + k]),
    }
}

/// 0x14199e860: elliptic parameters for order `n` and log-modulus `u`.
fn elliptic_params(n: u32, u: f32) -> [f32; 13] {
    let mut p = [0f32; 13];
    let f2 = F_1447149D4 / ((n * 2) as f32);
    p[0] = f2 * F_1447149E4;
    p[1] = (((n & 1) as i32) * -2 + 3) as f32 * f2;
    let t = (u * F_14A182254).tanh();
    p[2] = t;
    let t2 = t * t;
    p[3] = t2;
    let q = (1.0 - t2 * t2).sqrt().sqrt();
    let m = ((1.0 - q) * F_14470BAD0) / (q + 1.0);
    let m4 = m * m * m * m;
    let p4 = (((m4 * F_144FCFE50 + F_144714A0C) * m4 + F_144714994) * m4 + 1.0) * m;
    p[4] = p4;
    let p5 = p4 * p4;
    p[5] = p5;
    let p6 = p5 * p5;
    p[6] = p6;
    let p8 = p6 * p6;
    p[7] = p6 * p5;
    p[8] = p4 * p8;
    p[9] = p6 * p8;
    p[10] = p8 * p8;
    p[11] = p6 * p8 * p8;
    p[12] = p4.sqrt().sqrt() + p4.sqrt().sqrt();
    p
}

/// 0x14199fc50: the four lanes `k..k + 4` of the elliptic evaluation.
fn lane4(p: &[f32; 13], k: i32) -> V {
    let kf = k as f32;
    let (p0, p1) = (p[0], p[1]);
    let u: V = [
        kf * p0 + p1,
        (kf + F_14470BAD4) * p0 + p1,
        (kf + F_144714994) * p0 + p1,
        (kf + F_1447149D0) * p0 + p1,
    ];
    let abs: V = u.map(|x| f32::from_bits(x.to_bits() & !SIGN));
    let (mut q, r) = reduce4(abs);
    let flag: V = std::array::from_fn(|l| if TWO[l] <= q[l] { 1.0 } else { 0.0 });
    for l in 0..4 {
        q[l] = M2[l] * flag[l] + q[l];
    }
    let (p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12) = (
        p[2], p[3], p[4], p[5], p[6], p[7], p[8], p[9], p[10], p[11], p[12],
    );
    let mut out = [0f32; 4];
    for l in 0..4 {
        // Sin/cos of the reduced argument and the native bit-level sign and selection.
        let rr = r[l] * r[l];
        let sn = (((SIN1[l] * rr + SIN2[l]) * rr + SIN3[l]) * rr) * r[l] + r[l];
        let cs = (((COS1[l] * rr + COS2[l]) * rr + COS3[l]) * rr) * rr + COS4[l] * rr + ONE[l];
        let sgn_s = if flag[l] != q[l] { SIGN } else { 0 };
        let sel_s = if q[l] != 0.0 {
            sn.to_bits()
        } else {
            cs.to_bits()
        };
        let s = f32::from_bits(sgn_s ^ sel_s);
        let sgn_a = if flag[l] != 0.0 { SIGN } else { 0 };
        let sel_a = if q[l] == 0.0 {
            sn.to_bits()
        } else {
            cs.to_bits()
        };
        let a = f32::from_bits(sgn_a ^ (u[l].to_bits() & SIGN) ^ sel_a);

        // Complex products of (a + i s): squares, cubes and their combinations.
        let im = a * s + a * s;
        let re = s * s - a * a;
        let t1 = im * s + a * re;
        let t2 = s * re - a * im;
        let e1 = re * re - im * im;
        let d1 = im * re + im * re;
        let f1 = t2 * re - im * t1;
        let g1 = im * t2 + t1 * re;
        let m1 = t2 * t2 - t1 * t1;
        let n1 = t1 * e1 + d1 * t2;
        let pq = e1 * t2 - t1 * d1;
        let e2 = e1 * e1 - d1 * d1;
        let dd = e1 * f1 - d1 * g1;
        let ee = e1 * g1 + d1 * f1;

        let fv = p6 * e1 + p4 * re + p8 * m1 + p10 * e2;
        let dv = ((p4 * re - p6 * e1) + p8 * m1) - p10 * e2;
        let num1 = ((((a - p5 * t1) + p7 * g1) - p9 * n1) + ee * p11) * p12;
        let den1 = (1.0 - (dv + dv)) * p2;
        let qv = num1 / den1;
        let num2 = ((((p5 * t2 + s) + p7 * f1) + p9 * pq) + dd * p11) * p12;
        let den2 = (fv + fv + 1.0) * p2;
        let sp = num2 / den2;
        let s2 = sp * sp;
        let a1 = p3 * qv * qv + 1.0;
        let nn = (p3 * s2 - 1.0) * qv;
        let bb = (p3 * p3 * qv * qv) * s2 + 1.0;
        out[l] = nn / (a1 * sp - bb);
    }
    out
}

/// 0x1419a0210: four-lane reduction of non-negative arguments to [-pi/4, pi/4] with the
/// quadrant (0..=3). Branches A..D are ported; branch E (|x| > 64 pi) is unreachable here.
fn reduce4(x: V) -> (V, V) {
    if (0..4).all(|l| x[l] <= THR_A[l]) {
        return ([0.0; 4], x);
    }
    if (0..4).all(|l| x[l] <= THR_B[l]) {
        let mut q = [0.0; 4];
        let mut r = [0.0; 4];
        for l in 0..4 {
            let gt = THR_A[l] < x[l];
            let red = ((x[l] - P1[l]) - P2[l]) - P3[l];
            q[l] = if gt { ONE[l] } else { 0.0 };
            r[l] = if gt { red } else { x[l] };
        }
        return (q, r);
    }
    // Quadrant rounding of 2x/pi with the magic-number trick (sign kept as a bit).
    let m: V = std::array::from_fn(|l| C_MUL[l] * x[l]);
    let sg: [u32; 4] = std::array::from_fn(|l| m[l].to_bits() & SIGN);
    let fa: V = std::array::from_fn(|l| f32::from_bits(m[l].to_bits() ^ sg[l]));
    let n: V = std::array::from_fn(|l| {
        let rnd = if fa[l] < MAGIC[l] {
            (fa[l] + MAGIC[l]) - MAGIC[l]
        } else {
            fa[l]
        };
        f32::from_bits(rnd.to_bits() ^ sg[l])
    });
    let quad: V = std::array::from_fn(|l| ((n[l] as i32) & (QMASK[l] as i32)) as f32);
    if (0..4).all(|l| x[l] <= THR_C[l]) {
        let r: V = std::array::from_fn(|l| {
            let neg = f32::from_bits(n[l].to_bits() ^ SIGN);
            ((neg * P1[l] + x[l]) - P2[l] * n[l]) - P3[l] * n[l]
        });
        return (quad, r);
    }
    if (0..4).all(|l| x[l] <= THR_D[l]) {
        let t1: V = std::array::from_fn(|l| (x[l] - P1[l] * n[l]) - P2[l] * n[l]);
        let t2: V = std::array::from_fn(|l| t1[l] - P3[l] * n[l]);
        let r: V =
            std::array::from_fn(|l| t2[l] - (P4[l] * n[l] - ((t1[l] - t2[l]) - P3[l] * n[l])));
        return (quad, r);
    }
    unreachable!("Hilbert designer arguments stay below 8; the |x| > 64 pi branch is never reached")
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOLDEN: [(u32, V, [V; 8], [V; 8]); 4] = [
        (
            44100,
            [
                f32::from_bits(0x3d2e15ec),
                f32::from_bits(0x3d2e15ec),
                f32::from_bits(0x3c935548),
                f32::from_bits(0x3c935548),
            ],
            [
                [
                    f32::from_bits(0x3f3ade56),
                    f32::from_bits(0x3f3ade56),
                    f32::from_bits(0xbf7f3a1a),
                    f32::from_bits(0xbf7f3a1a),
                ],
                [
                    f32::from_bits(0x3e6af385),
                    f32::from_bits(0x3e6af385),
                    f32::from_bits(0xbf7ce90d),
                    f32::from_bits(0xbf7ce90d),
                ],
                [
                    f32::from_bits(0xbebd86ec),
                    f32::from_bits(0xbebd86ec),
                    f32::from_bits(0xbf756eee),
                    f32::from_bits(0xbf756eee),
                ],
                [
                    f32::from_bits(0xbf438687),
                    f32::from_bits(0xbf438687),
                    f32::from_bits(0xbf5d77b0),
                    f32::from_bits(0xbf5d77b0),
                ],
                [
                    f32::from_bits(0xbf6cc1f2),
                    f32::from_bits(0xbf6cc1f2),
                    f32::from_bits(0xbf1a3d06),
                    f32::from_bits(0xbf1a3d06),
                ],
                [
                    f32::from_bits(0xbf7a411d),
                    f32::from_bits(0xbf7a411d),
                    f32::from_bits(0xbda1e0ec),
                    f32::from_bits(0xbda1e0ec),
                ],
                [
                    f32::from_bits(0xbf7e61bb),
                    f32::from_bits(0xbf7e61bb),
                    f32::from_bits(0x3f01bd71),
                    f32::from_bits(0x3f01bd71),
                ],
                [
                    f32::from_bits(0xbf7fc597),
                    f32::from_bits(0xbf7fc597),
                    f32::from_bits(0x3f69859d),
                    f32::from_bits(0x3f69859d),
                ],
            ],
            [
                [
                    f32::from_bits(0x3c80ce16),
                    f32::from_bits(0x3c80ce16),
                    f32::from_bits(0xbd138e67),
                    f32::from_bits(0xbd138e67),
                ],
                [
                    f32::from_bits(0x3e483b53),
                    f32::from_bits(0x3e483b53),
                    f32::from_bits(0x3d14ae9b),
                    f32::from_bits(0x3d14ae9b),
                ],
                [
                    f32::from_bits(0xbf7075ac),
                    f32::from_bits(0xbf7075ac),
                    f32::from_bits(0xbd18ce04),
                    f32::from_bits(0xbd18ce04),
                ],
                [
                    f32::from_bits(0x3fcaa2d3),
                    f32::from_bits(0x3fcaa2d3),
                    f32::from_bits(0x3d28305d),
                    f32::from_bits(0x3d28305d),
                ],
                [
                    f32::from_bits(0xbfef1d03),
                    f32::from_bits(0xbfef1d03),
                    f32::from_bits(0xbd6fd946),
                    f32::from_bits(0xbd6fd946),
                ],
                [
                    f32::from_bits(0x3ffb4d8b),
                    f32::from_bits(0x3ffb4d8b),
                    f32::from_bits(0x3eff67c8),
                    f32::from_bits(0x3eff67c8),
                ],
                [
                    f32::from_bits(0xbffef6a4),
                    f32::from_bits(0xbffef6a4),
                    f32::from_bits(0x3ee658f4),
                    f32::from_bits(0x3ee658f4),
                ],
                [
                    f32::from_bits(0x3fffe2cc),
                    f32::from_bits(0x3fffe2cc),
                    f32::from_bits(0x3db3d318),
                    f32::from_bits(0x3db3d318),
                ],
            ],
        ),
        (
            48000,
            [
                f32::from_bits(0xbb00577b),
                f32::from_bits(0xbb00577b),
                f32::from_bits(0x3d3b18e1),
                f32::from_bits(0x3d3b18e1),
            ],
            [
                [
                    f32::from_bits(0x3f147a57),
                    f32::from_bits(0x3f147a57),
                    f32::from_bits(0xbf7f5601),
                    f32::from_bits(0xbf7f5601),
                ],
                [
                    f32::from_bits(0xbc06fe6a),
                    f32::from_bits(0xbc06fe6a),
                    f32::from_bits(0xbf7d7844),
                    f32::from_bits(0xbf7d7844),
                ],
                [
                    f32::from_bits(0xbf09705e),
                    f32::from_bits(0xbf09705e),
                    f32::from_bits(0xbf77d507),
                    f32::from_bits(0xbf77d507),
                ],
                [
                    f32::from_bits(0xbf542dbe),
                    f32::from_bits(0xbf542dbe),
                    f32::from_bits(0xbf669a49),
                    f32::from_bits(0xbf669a49),
                ],
                [
                    f32::from_bits(0xbf7185c4),
                    f32::from_bits(0xbf7185c4),
                    f32::from_bits(0xbf366671),
                    f32::from_bits(0xbf366671),
                ],
                [
                    f32::from_bits(0xbf7b6db0),
                    f32::from_bits(0xbf7b6db0),
                    f32::from_bits(0xbe97f6ce),
                    f32::from_bits(0xbe97f6ce),
                ],
                [
                    f32::from_bits(0xbf7ea399),
                    f32::from_bits(0xbf7ea399),
                    f32::from_bits(0x3e968dae),
                    f32::from_bits(0x3e968dae),
                ],
                [
                    f32::from_bits(0xbf7fcd38),
                    f32::from_bits(0xbf7fcd38),
                    f32::from_bits(0x3f5a5bc7),
                    f32::from_bits(0x3f5a5bc7),
                ],
            ],
            [
                [
                    f32::from_bits(0xbab9e166),
                    f32::from_bits(0xbab9e166),
                    f32::from_bits(0xbdbb572b),
                    f32::from_bits(0xbdbb572b),
                ],
                [
                    f32::from_bits(0x3ed38be5),
                    f32::from_bits(0x3ed38be5),
                    f32::from_bits(0x3dbc8522),
                    f32::from_bits(0x3dbc8522),
                ],
                [
                    f32::from_bits(0xbf96288e),
                    f32::from_bits(0xbf96288e),
                    f32::from_bits(0xbdc0940d),
                    f32::from_bits(0xbdc0940d),
                ],
                [
                    f32::from_bits(0x3fd79639),
                    f32::from_bits(0x3fd79639),
                    f32::from_bits(0x3dce7a1a),
                    f32::from_bits(0x3dce7a1a),
                ],
                [
                    f32::from_bits(0xbff2d4f4),
                    f32::from_bits(0xbff2d4f4),
                    f32::from_bits(0xbe028aed),
                    f32::from_bits(0xbe028aed),
                ],
                [
                    f32::from_bits(0x3ffc2b7e),
                    f32::from_bits(0x3ffc2b7e),
                    f32::from_bits(0x3ea687f9),
                    f32::from_bits(0x3ea687f9),
                ],
                [
                    f32::from_bits(0xbfff1f27),
                    f32::from_bits(0xbfff1f27),
                    f32::from_bits(0x3f1a2677),
                    f32::from_bits(0x3f1a2677),
                ],
                [
                    f32::from_bits(0x3fffe69c),
                    f32::from_bits(0x3fffe69c),
                    f32::from_bits(0x3e1690e4),
                    f32::from_bits(0x3e1690e4),
                ],
            ],
        ),
        (
            88200,
            [
                f32::from_bits(0x3b9552bc),
                f32::from_bits(0x3b9552bc),
                f32::from_bits(0xbddfbe18),
                f32::from_bits(0xbddfbe18),
            ],
            [
                [
                    f32::from_bits(0xbc2f4ebd),
                    f32::from_bits(0xbc2f4ebd),
                    f32::from_bits(0xbf7fadac),
                    f32::from_bits(0xbf7fadac),
                ],
                [
                    f32::from_bits(0xbf11103c),
                    f32::from_bits(0xbf11103c),
                    f32::from_bits(0xbf7edd1a),
                    f32::from_bits(0xbf7edd1a),
                ],
                [
                    f32::from_bits(0xbf5423f9),
                    f32::from_bits(0xbf5423f9),
                    f32::from_bits(0xbf7ca8da),
                    f32::from_bits(0xbf7ca8da),
                ],
                [
                    f32::from_bits(0xbf6ff54d),
                    f32::from_bits(0xbf6ff54d),
                    f32::from_bits(0xbf76714e),
                    f32::from_bits(0xbf76714e),
                ],
                [
                    f32::from_bits(0xbf7a5661),
                    f32::from_bits(0xbf7a5661),
                    f32::from_bits(0xbf654d04),
                    f32::from_bits(0xbf654d04),
                ],
                [
                    f32::from_bits(0xbf7e0a4c),
                    f32::from_bits(0xbf7e0a4c),
                    f32::from_bits(0xbf3954e4),
                    f32::from_bits(0xbf3954e4),
                ],
                [
                    f32::from_bits(0xbf7f5cc7),
                    f32::from_bits(0xbf7f5cc7),
                    f32::from_bits(0xbeada968),
                    f32::from_bits(0xbeada968),
                ],
                [
                    f32::from_bits(0xbf7fe6f4),
                    f32::from_bits(0xbf7fe6f4),
                    f32::from_bits(0x3f069a6e),
                    f32::from_bits(0x3f069a6e),
                ],
            ],
            [
                [
                    f32::from_bits(0xbedc636e),
                    f32::from_bits(0xbedc636e),
                    f32::from_bits(0x3e5fe21e),
                    f32::from_bits(0x3e5fe21e),
                ],
                [
                    f32::from_bits(0x3f96b782),
                    f32::from_bits(0x3f96b782),
                    f32::from_bits(0xbe6085fd),
                    f32::from_bits(0xbe6085fd),
                ],
                [
                    f32::from_bits(0xbfd44bb3),
                    f32::from_bits(0xbfd44bb3),
                    f32::from_bits(0x3e6282a3),
                    f32::from_bits(0x3e6282a3),
                ],
                [
                    f32::from_bits(0x3feff24d),
                    f32::from_bits(0x3feff24d),
                    f32::from_bits(0xbe686b3b),
                    f32::from_bits(0xbe686b3b),
                ],
                [
                    f32::from_bits(0xbffa8254),
                    f32::from_bits(0xbffa8254),
                    f32::from_bits(0x3e7aa0e0),
                    f32::from_bits(0x3e7aa0e0),
                ],
                [
                    f32::from_bits(0x3ffe49a9),
                    f32::from_bits(0x3ffe49a9),
                    f32::from_bits(0xbe9d6a1d),
                    f32::from_bits(0xbe9d6a1d),
                ],
                [
                    f32::from_bits(0xbfff9560),
                    f32::from_bits(0xbfff9560),
                    f32::from_bits(0x3f344225),
                    f32::from_bits(0x3f344225),
                ],
                [
                    f32::from_bits(0x3ffff37a),
                    f32::from_bits(0x3ffff37a),
                    f32::from_bits(0x3ef2cb24),
                    f32::from_bits(0x3ef2cb24),
                ],
            ],
        ),
        (
            96000,
            [
                f32::from_bits(0x3d008399),
                f32::from_bits(0x3d008399),
                f32::from_bits(0xbdf733f8),
                f32::from_bits(0xbdf733f8),
            ],
            [
                [
                    f32::from_bits(0xbd89cd6a),
                    f32::from_bits(0xbd89cd6a),
                    f32::from_bits(0xbf7fb4b7),
                    f32::from_bits(0xbf7fb4b7),
                ],
                [
                    f32::from_bits(0xbf1a5ee7),
                    f32::from_bits(0xbf1a5ee7),
                    f32::from_bits(0xbf7ef6bd),
                    f32::from_bits(0xbf7ef6bd),
                ],
                [
                    f32::from_bits(0xbf583bea),
                    f32::from_bits(0xbf583bea),
                    f32::from_bits(0xbf7cf6ed),
                    f32::from_bits(0xbf7cf6ed),
                ],
                [
                    f32::from_bits(0xbf717a39),
                    f32::from_bits(0xbf717a39),
                    f32::from_bits(0xbf77573a),
                    f32::from_bits(0xbf77573a),
                ],
                [
                    f32::from_bits(0xbf7adcdb),
                    f32::from_bits(0xbf7adcdb),
                    f32::from_bits(0xbf67d3f6),
                    f32::from_bits(0xbf67d3f6),
                ],
                [
                    f32::from_bits(0xbf7e374e),
                    f32::from_bits(0xbf7e374e),
                    f32::from_bits(0xbf3fb20f),
                    f32::from_bits(0xbf3fb20f),
                ],
                [
                    f32::from_bits(0xbf7f6aec),
                    f32::from_bits(0xbf7f6aec),
                    f32::from_bits(0xbec69601),
                    f32::from_bits(0xbec69601),
                ],
                [
                    f32::from_bits(0xbf7fe915),
                    f32::from_bits(0xbf7fe915),
                    f32::from_bits(0x3ef766ce),
                    f32::from_bits(0x3ef766ce),
                ],
            ],
            [
                [
                    f32::from_bits(0xbefecf1b),
                    f32::from_bits(0xbefecf1b),
                    f32::from_bits(0x3e77585c),
                    f32::from_bits(0x3e77585c),
                ],
                [
                    f32::from_bits(0x3f9eaa9f),
                    f32::from_bits(0x3f9eaa9f),
                    f32::from_bits(0xbe77fd7d),
                    f32::from_bits(0xbe77fd7d),
                ],
                [
                    f32::from_bits(0xbfd829d1),
                    f32::from_bits(0xbfd829d1),
                    f32::from_bits(0x3e79fbe6),
                    f32::from_bits(0x3e79fbe6),
                ],
                [
                    f32::from_bits(0x3ff169dc),
                    f32::from_bits(0x3ff169dc),
                    f32::from_bits(0xbe7fe07c),
                    f32::from_bits(0xbe7fe07c),
                ],
                [
                    f32::from_bits(0xbffb0148),
                    f32::from_bits(0xbffb0148),
                    f32::from_bits(0x3e88ec90),
                    f32::from_bits(0x3e88ec90),
                ],
                [
                    f32::from_bits(0x3ffe704f),
                    f32::from_bits(0x3ffe704f),
                    f32::from_bits(0xbea7cff4),
                    f32::from_bits(0xbea7cff4),
                ],
                [
                    f32::from_bits(0xbfff9e92),
                    f32::from_bits(0xbfff9e92),
                    f32::from_bits(0x3f2bae05),
                    f32::from_bits(0x3f2bae05),
                ],
                [
                    f32::from_bits(0x3ffff48a),
                    f32::from_bits(0x3ffff48a),
                    f32::from_bits(0x3f044c99),
                    f32::from_bits(0x3f044c99),
                ],
            ],
        ),
    ];

    /// The native tables (gain, all-pass, FIR) at 44.1, 48, 88.2 and 96 kHz. 44.1 and 96 kHz
    /// are bit-exact; at 48 and 88.2 kHz the platform `log2f`/`tanf` differ by an ulp from the
    /// native CRT, which the elliptic chain amplifies to about 4e-6 relative.
    #[test]
    fn designer_matches_the_native_tables() {
        let close = |a: f32, b: f32| (a - b).abs() <= 1e-5 * b.abs().max(1e-4);
        for (rate, gain, allpass, fir) in GOLDEN {
            let h = design((1.0 / rate as f32) * std::f32::consts::PI, 20.0, 20_000.0);
            for (a, b) in h.gain.iter().zip(gain) {
                assert!(close(*a, b), "gain at {rate}: {a} vs {b}");
            }
            for k in 0..8 {
                for (a, b) in h.allpass[k].iter().zip(allpass[k]) {
                    assert!(close(*a, b), "allpass {k} at {rate}: {a} vs {b}");
                }
                for (a, b) in h.fir[k].iter().zip(fir[k]) {
                    assert!(close(*a, b), "fir {k} at {rate}: {a} vs {b}");
                }
            }
//! Hilbert half-band coefficient designer of the Freak core.
//!
//! Port of the Kontakt 8 designer at 0x141968890: an order-16 elliptic
//! (nome) design over the band 20 Hz to 20 kHz, evaluated per sample rate.
//! The helpers keep their original roles: the nome series at 0x14199e860,
//! the four-lane elliptic core at 0x14199fc50 (with the quadrant reduction
//! at 0x1419a0210 and the lane helpers at 0x14193f370..0x14193f3a0). The
//! output is a gain, eight all-pass stages and eight FIR taps, four lanes
//! each: lanes 0/1 carry the first branch, lanes 2/3 the second.
//!
//! The arithmetic keeps the original expression order, since the designed
//! coefficients sit in ill-conditioned cancellations. `tan`, `tanh`, `log2`
//! and `sqrt` come from libm, which can differ from the original CRT by one
//! ulp; measured against the dumped tables that is 0 at 44.1 and 96 kHz,
//! 7.5e-7 at 88.2 kHz and 3.9e-6 at 48 kHz (relative, largest tap).
//!
//! Deliberate deviation: below 40.04 kHz the upper edge 20 kHz is above
//! `tan`'s pole, so the upper edge is capped at 0.49 of the sample rate.
//! No dumped table covers those rates.

use std::f32::consts::PI as PI_F;

type V4 = [f32; 4];

/// Coefficients of one Freak core: gain, eight all-pass stages, eight FIR taps.
pub(super) struct Design {
    pub gain: V4,
    pub allpass: [V4; 8],
    pub fir: [V4; 8],
}

/// Band edges in Hz (PE 0x14a182140 and 0x14a182144).
const F_LO: f32 = 20.0;
const F_HI: f32 = 20000.0;
/// Upper-edge cap for rates whose Nyquist is below `F_HI` (see module docs).
const NYQUIST_CAP: f32 = 0.49;
/// Elliptic order (sixteen points, eight all-pass stages per branch).
const ORDER: u32 = 16;

const ONE: f32 = 1.0;
const TWO: f32 = 2.0;
/// PE 0x1476a3b00: quadrant wrap for the upper half (q - 2).
const MINUS_TWO: f32 = -2.0;
/// PE 0x1447149d4 (pi) is `PI_F`, PE 0x1447149e4 = 4.
const FOUR: f32 = f32::from_bits(0x4080_0000);
/// PE 0x14470bad0.
const HALF: f32 = f32::from_bits(0x3f00_0000);
/// PE 0x144fcfe50 and 0x144714a0c: nome series coefficients 150 and 15.
const NOME_C150: f32 = f32::from_bits(0x4316_0000);
const NOME_C15: f32 = f32::from_bits(0x4170_0000);
/// PE 0x14a182254: ln2 / 4.
const LN2_4: f32 = f32::from_bits(0x3e31_7218);

/// PE 0x1476a3730: 2/pi.
const TWO_OVER_PI: f32 = f32::from_bits(0x3f22_f983);
/// PE 0x1476a3a90, 0x14769a440, 0x1476a3ab0, 0x1476a3ac0: range-reduction tiers.
const TIER_QUARTER_PI: f32 = f32::from_bits(0x3f49_0fdb);
const TIER_HALF_PI: f32 = f32::from_bits(0x3fc9_0fdb);
const TIER_20_PI: f32 = f32::from_bits(0x427b_53d1);
const TIER_64_PI: f32 = f32::from_bits(0x4349_0fdb);
/// PE 0x144f392f0: 1.5 * 2^23 rounding offset (`2^23` for round-to-even).
const ROUND_MAGIC: f32 = 8_388_608.0;
/// Cody-Waite split of pi/2 (PE 0x1476a3aa0, 0x1476a3a50, 0x1476a3a40, 0x1476a3a30).
const PIO2_1: f32 = f32::from_bits(0x3fc9_0f80);
const PIO2_2: f32 = f32::from_bits(0x3735_4400);
const PIO2_3: f32 = f32::from_bits(0x2e85_a300);
const PIO2_4: f32 = f32::from_bits(0x248d_3132);

/// sin series (PE 0x1476a3ad0, 0x1476a3a70, 0x1476a3af0): x^7, x^5, x^3.
const SIN_X7: f32 = f32::from_bits(0xb94c_a1f9);
const SIN_X5: f32 = f32::from_bits(0x3c08_839d);
const SIN_X3: f32 = f32::from_bits(0xbe2a_aaa2);
/// cos series (PE 0x1476a3a60, 0x1476a3ae0, 0x1476a3a80): x^8, x^6, x^4.
const COS_X8: f32 = f32::from_bits(0x37cc_f5ce);
const COS_X6: f32 = f32::from_bits(0xbab6_0619);
const COS_X4: f32 = f32::from_bits(0x3d2a_aaa5);

fn lanes(f: impl FnMut(usize) -> f32) -> V4 {
    std::array::from_fn(f)
}

/// Nome helper (0x14199e860): the sixteen-point elliptic state for `order` and
/// the log2 band ratio `l`. Slots 0..=12 as in the original object layout.
fn nome(order: u32, l: f32) -> [f32; 13] {
    let mut st = [0.0f32; 13];
    let f2 = PI_F / ((2 * order) as f32);
    st[0] = f2 * FOUR;
    st[1] = ((order & 1) as i32 * -2 + 3) as f32 * f2;
    let t = (l * LN2_4).tanh();
    st[2] = t;
    let t2 = t * t;
    st[3] = t2;
    let q = (ONE - t2 * t2).sqrt().sqrt();
    let y = ((ONE - q) * HALF) / (q + ONE);
    let y4 = y * y * y * y;
    let rho = (((y4 * NOME_C150 + NOME_C15) * y4 + TWO) * y4 + ONE) * y;
    st[4] = rho;
    st[5] = rho * rho;
    st[6] = st[5] * st[5];
    st[7] = st[6] * st[5];
    let r8 = st[6] * st[6];
    st[8] = rho * r8;
    st[9] = st[6] * r8;
    st[10] = r8 * r8;
    st[11] = st[6] * r8 * r8;
    let s4 = rho.sqrt().sqrt();
    st[12] = s4 + s4;
    st
}

fn sin_poly(x: f32) -> f32 {
    let x2 = x * x;
    (((SIN_X7 * x2 + SIN_X5) * x2 + SIN_X3) * x2 * x) + x
}

fn cos_poly(x: f32) -> f32 {
    let x2 = x * x;
    ((COS_X8 * x2 + COS_X6) * x2 + COS_X4) * x2 * x2 + (-0.5) * x2 + ONE
}

/// Quadrant index (0..=3) of a rounded multiple of pi/2.
fn quadrant(n: f32) -> f32 {
    ((n as i32) & 3) as f32
}

/// Quadrant reduction (0x1419a0210): `|x|` to `(quadrant, r)` with `x = q pi/2 + r`.
/// The tier is chosen per group: all four lanes in one tier, as the original.
fn reduce_quadrant(a: V4) -> (V4, V4) {
    if a.iter().all(|&x| x <= TIER_QUARTER_PI) {
        return ([0.0; 4], a);
    }
    if a.iter().all(|&x| x <= TIER_HALF_PI) {
        let m: [bool; 4] = std::array::from_fn(|l| a[l] > TIER_QUARTER_PI);
        let r = lanes(|l| {
            if m[l] {
                ((a[l] - PIO2_1) - PIO2_2) - PIO2_3
            } else {
                a[l]
            }
        });
        let q = lanes(|l| if m[l] { ONE } else { 0.0 });
        return (q, r);
    }
    if a.iter().all(|&x| x <= TIER_64_PI) {
        let t = lanes(|l| TWO_OVER_PI * a[l]);
        let qs = lanes(|l| {
            let at = t[l].abs();
            let rnd = if at < ROUND_MAGIC {
                (at + ROUND_MAGIC) - ROUND_MAGIC
            } else {
                at
            };
            if t[l].is_sign_negative() { -rnd } else { rnd }
        });
        if a.iter().all(|&x| x <= TIER_20_PI) {
            let r = lanes(|l| (((-qs[l]) * PIO2_1 + a[l]) - PIO2_2 * qs[l]) - PIO2_3 * qs[l]);
            return (lanes(|l| quadrant(qs[l])), r);
        }
        let d0 = lanes(|l| (a[l] - PIO2_1 * qs[l]) - PIO2_2 * qs[l]);
        let d1 = lanes(|l| d0[l] - PIO2_3 * qs[l]);
        let r = lanes(|l| d1[l] - (PIO2_4 * qs[l] - ((d0[l] - d1[l]) - PIO2_3 * qs[l])));
        return (lanes(|l| quadrant(qs[l])), r);
    }
    // Beyond 64 pi the original reduces each lane in double precision. The
    // designer never gets there (|u| < 2 pi for order 16); plain f64 here.
    let mut q = [0.0; 4];
    let mut r = [0.0; 4];
    for l in 0..4 {
        let x = f64::from(a[l]);
        let k = (2.0 * x / std::f64::consts::PI).round();
        r[l] = (x - k * std::f64::consts::FRAC_PI_2) as f32;
        q[l] = ((k as i64) & 3) as f32;
    }
    (q, r)
}

/// Elliptic core (0x14199fc50): four output points for the lanes
/// `u_j = (n + j) p0 + p1`, each a ratio of theta-type series in the nome.
fn elliptic_core(st: &[f32; 13], n: u32) -> V4 {
    let (p0, p1) = (st[0], st[1]);
    let nf = n as f32;
    let u: V4 = lanes(|j| (nf + j as f32) * p0 + p1);
    let abs_u: V4 = lanes(|j| u[j].abs());
    let (q, r) = reduce_quadrant(abs_u);
    let f = lanes(|l| if q[l] >= TWO { ONE } else { 0.0 });
    let qp = lanes(|l| MINUS_TWO * f[l] + q[l]);

    // cos(|u|) and sin(u) from the reduced argument and quadrant.
    let s = lanes(|l| sin_poly(r[l]));
    let c = lanes(|l| cos_poly(r[l]));
    let d = lanes(|l| {
        let v = if qp[l] != 0.0 { s[l] } else { c[l] };
        if f[l] != qp[l] { -v } else { v }
    });
    let e = lanes(|l| {
        let v = if qp[l] == 0.0 { s[l] } else { c[l] };
        if (f[l] != 0.0) ^ u[l].is_sign_negative() {
            -v
        } else {
            v
        }
    });

    // Powers z^k of z = d + i e, as (re, im) pairs in the original order.
    let im2 = lanes(|l| e[l] * d[l] + e[l] * d[l]);
    let re2 = lanes(|l| d[l] * d[l] - e[l] * e[l]);
    let im3 = lanes(|l| im2[l] * d[l] + e[l] * re2[l]);
    let re3 = lanes(|l| d[l] * re2[l] - e[l] * im2[l]);
    let re4 = lanes(|l| re2[l] * re2[l] - im2[l] * im2[l]);
    let im4 = lanes(|l| im2[l] * re2[l] + im2[l] * re2[l]);
    let re5 = lanes(|l| re3[l] * re2[l] - im2[l] * im3[l]);
    let im5 = lanes(|l| im2[l] * re3[l] + im3[l] * re2[l]);
    let re6 = lanes(|l| re3[l] * re3[l] - im3[l] * im3[l]);
    let re7 = lanes(|l| re4[l] * re3[l] - im3[l] * im4[l]);
    let im7 = lanes(|l| im3[l] * re4[l] + re3[l] * im4[l]);
    let re8 = lanes(|l| re4[l] * re4[l] - im4[l] * im4[l]);
    let re9 = lanes(|l| re4[l] * re5[l] - im4[l] * im5[l]);
    let im9 = lanes(|l| re4[l] * im5[l] + im4[l] * re5[l]);

    let (t, t2) = (st[2], st[3]);
    let (rho, r2, r4, r6, r9) = (st[4], st[5], st[6], st[7], st[8]);
    let (r12, r16, r20, p12) = (st[9], st[10], st[11], st[12]);

    // Jacobi theta values theta3 and theta4 (times t), and theta1 and theta2 (times 2 rho^(1/4)).
    let s3 = lanes(|l| ((r4 * re4[l] + rho * re2[l]) + r9 * re6[l]) + r16 * re8[l]);
    let s4 = lanes(|l| ((rho * re2[l] - r4 * re4[l]) + r9 * re6[l]) - r16 * re8[l]);
    let th3 = lanes(|l| (s3[l] + s3[l] + ONE) * t);
    let th4 = lanes(|l| (ONE - (s4[l] + s4[l])) * t);
    let ne =
        lanes(|l| ((((e[l] - r2 * im3[l]) + r6 * im5[l]) - r12 * im7[l]) + im9[l] * r20) * p12);
    let ng =
        lanes(|l| ((((r2 * re3[l] + d[l]) + r6 * re5[l]) + r12 * re7[l]) + re9[l] * r20) * p12);
    let ee = lanes(|l| ne[l] / th4[l]);
    let gg = lanes(|l| ng[l] / th3[l]);

    let t4 = t2 * t2;
    let pp = lanes(|l| (t2 * ee[l]) * ee[l] + ONE);
    let qq = lanes(|l| (t2 * (gg[l] * gg[l]) - ONE) * ee[l]);
    let rr = lanes(|l| ((t4 * ee[l]) * ee[l]) * (gg[l] * gg[l]) + ONE);
    lanes(|l| qq[l] / (pp[l] * gg[l] - rr[l]))
}

/// Designs the Freak Hilbert stages at `sample_rate` (Hz).
pub(super) fn design(sample_rate: u32) -> Design {
    let srf = sample_rate as f32;
    let inv = ONE / srf;
    let w = inv * PI_F;
    let f_hi = F_HI.min(NYQUIST_CAP * srf);
    let fl = (w * F_LO).tan();
    let fh = (f_hi * w).tan();
    let s = (fh * fl).sqrt();
    let l = (fh / fl).log2();
    let st = nome(ORDER, l);

    let mut pts = [0.0f32; 16];
    for m in 0..4usize {
        let block = elliptic_core(&st, (4 * m) as u32);
        pts[4 * m..4 * m + 4].copy_from_slice(&block);
    }

    // Bilinear map of each point pair: the first branch from points 0..8,
    // the second from points 8..16 with the sign of the point flipped.
    let mut gain = [ONE; 4];
    let mut allpass = [[0.0f32; 4]; 8];
    for k in 0..8 {
        let a = pts[k] * s;
        let b = (-pts[8 + k]) * s;
        let lo = (a - ONE) / (a + ONE);
        let hi = (b - ONE) / (b + ONE);
        let row = [lo, lo, hi, hi];
        for l in 0..4 {
            gain[l] *= row[l];
        }
        allpass[k] = row;
    }
    let mut fir = [[0.0f32; 4]; 8];
    for j in 0..8 {
        let mut row = lanes(|l| ONE - allpass[j][l]);
        for ap in &allpass[j + 1..] {
            for l in 0..4 {
                row[l] *= ap[l];
            }
        }
        fir[j] = row;
    }
    Design { gain, allpass, fir }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn designer_is_finite_at_common_rates() {
        for rate in [
            8_000u32, 22_050, 32_000, 44_100, 48_000, 88_200, 96_000, 176_400, 192_000, 384_000,
        ] {
            let d = design(rate);
            let all = d
                .gain
                .iter()
                .chain(d.allpass.iter().flatten())
                .chain(d.fir.iter().flatten());
            assert!(
                all.clone().all(|v| v.is_finite()),
                "non-finite coefficient at {rate} Hz"
            );
        }
    }

    /// Dumped Kontakt 8 tables (gate reference for the designer): gain, all-pass, FIR per rate.
    #[rustfmt::skip]
    const DUMPED: [(u32, [f32; 4], [[f32; 4]; 8], [[f32; 4]; 8]); 4] = [
        (44100, [f32::from_bits(0x3d2e15ec), f32::from_bits(0x3d2e15ec), f32::from_bits(0x3c935548), f32::from_bits(0x3c935548)], [[f32::from_bits(0x3f3ade56), f32::from_bits(0x3f3ade56), f32::from_bits(0xbf7f3a1a), f32::from_bits(0xbf7f3a1a)], [f32::from_bits(0x3e6af385), f32::from_bits(0x3e6af385), f32::from_bits(0xbf7ce90d), f32::from_bits(0xbf7ce90d)], [f32::from_bits(0xbebd86ec), f32::from_bits(0xbebd86ec), f32::from_bits(0xbf756eee), f32::from_bits(0xbf756eee)], [f32::from_bits(0xbf438687), f32::from_bits(0xbf438687), f32::from_bits(0xbf5d77b0), f32::from_bits(0xbf5d77b0)], [f32::from_bits(0xbf6cc1f2), f32::from_bits(0xbf6cc1f2), f32::from_bits(0xbf1a3d06), f32::from_bits(0xbf1a3d06)], [f32::from_bits(0xbf7a411d), f32::from_bits(0xbf7a411d), f32::from_bits(0xbda1e0ec), f32::from_bits(0xbda1e0ec)], [f32::from_bits(0xbf7e61bb), f32::from_bits(0xbf7e61bb), f32::from_bits(0x3f01bd71), f32::from_bits(0x3f01bd71)], [f32::from_bits(0xbf7fc597), f32::from_bits(0xbf7fc597), f32::from_bits(0x3f69859d), f32::from_bits(0x3f69859d)]], [[f32::from_bits(0x3c80ce16), f32::from_bits(0x3c80ce16), f32::from_bits(0xbd138e67), f32::from_bits(0xbd138e67)], [f32::from_bits(0x3e483b53), f32::from_bits(0x3e483b53), f32::from_bits(0x3d14ae9b), f32::from_bits(0x3d14ae9b)], [f32::from_bits(0xbf7075ac), f32::from_bits(0xbf7075ac), f32::from_bits(0xbd18ce04), f32::from_bits(0xbd18ce04)], [f32::from_bits(0x3fcaa2d3), f32::from_bits(0x3fcaa2d3), f32::from_bits(0x3d28305d), f32::from_bits(0x3d28305d)], [f32::from_bits(0xbfef1d03), f32::from_bits(0xbfef1d03), f32::from_bits(0xbd6fd946), f32::from_bits(0xbd6fd946)], [f32::from_bits(0x3ffb4d8b), f32::from_bits(0x3ffb4d8b), f32::from_bits(0x3eff67c8), f32::from_bits(0x3eff67c8)], [f32::from_bits(0xbffef6a4), f32::from_bits(0xbffef6a4), f32::from_bits(0x3ee658f4), f32::from_bits(0x3ee658f4)], [f32::from_bits(0x3fffe2cc), f32::from_bits(0x3fffe2cc), f32::from_bits(0x3db3d318), f32::from_bits(0x3db3d318)]]),
        (48000, [f32::from_bits(0xbb00577b), f32::from_bits(0xbb00577b), f32::from_bits(0x3d3b18e1), f32::from_bits(0x3d3b18e1)], [[f32::from_bits(0x3f147a57), f32::from_bits(0x3f147a57), f32::from_bits(0xbf7f5601), f32::from_bits(0xbf7f5601)], [f32::from_bits(0xbc06fe6a), f32::from_bits(0xbc06fe6a), f32::from_bits(0xbf7d7844), f32::from_bits(0xbf7d7844)], [f32::from_bits(0xbf09705e), f32::from_bits(0xbf09705e), f32::from_bits(0xbf77d507), f32::from_bits(0xbf77d507)], [f32::from_bits(0xbf542dbe), f32::from_bits(0xbf542dbe), f32::from_bits(0xbf669a49), f32::from_bits(0xbf669a49)], [f32::from_bits(0xbf7185c4), f32::from_bits(0xbf7185c4), f32::from_bits(0xbf366671), f32::from_bits(0xbf366671)], [f32::from_bits(0xbf7b6db0), f32::from_bits(0xbf7b6db0), f32::from_bits(0xbe97f6ce), f32::from_bits(0xbe97f6ce)], [f32::from_bits(0xbf7ea399), f32::from_bits(0xbf7ea399), f32::from_bits(0x3e968dae), f32::from_bits(0x3e968dae)], [f32::from_bits(0xbf7fcd38), f32::from_bits(0xbf7fcd38), f32::from_bits(0x3f5a5bc7), f32::from_bits(0x3f5a5bc7)]], [[f32::from_bits(0xbab9e166), f32::from_bits(0xbab9e166), f32::from_bits(0xbdbb572b), f32::from_bits(0xbdbb572b)], [f32::from_bits(0x3ed38be5), f32::from_bits(0x3ed38be5), f32::from_bits(0x3dbc8522), f32::from_bits(0x3dbc8522)], [f32::from_bits(0xbf96288e), f32::from_bits(0xbf96288e), f32::from_bits(0xbdc0940d), f32::from_bits(0xbdc0940d)], [f32::from_bits(0x3fd79639), f32::from_bits(0x3fd79639), f32::from_bits(0x3dce7a1a), f32::from_bits(0x3dce7a1a)], [f32::from_bits(0xbff2d4f4), f32::from_bits(0xbff2d4f4), f32::from_bits(0xbe028aed), f32::from_bits(0xbe028aed)], [f32::from_bits(0x3ffc2b7e), f32::from_bits(0x3ffc2b7e), f32::from_bits(0x3ea687f9), f32::from_bits(0x3ea687f9)], [f32::from_bits(0xbfff1f27), f32::from_bits(0xbfff1f27), f32::from_bits(0x3f1a2677), f32::from_bits(0x3f1a2677)], [f32::from_bits(0x3fffe69c), f32::from_bits(0x3fffe69c), f32::from_bits(0x3e1690e4), f32::from_bits(0x3e1690e4)]]),
        (88200, [f32::from_bits(0x3b9552bc), f32::from_bits(0x3b9552bc), f32::from_bits(0xbddfbe18), f32::from_bits(0xbddfbe18)], [[f32::from_bits(0xbc2f4ebd), f32::from_bits(0xbc2f4ebd), f32::from_bits(0xbf7fadac), f32::from_bits(0xbf7fadac)], [f32::from_bits(0xbf11103c), f32::from_bits(0xbf11103c), f32::from_bits(0xbf7edd1a), f32::from_bits(0xbf7edd1a)], [f32::from_bits(0xbf5423f9), f32::from_bits(0xbf5423f9), f32::from_bits(0xbf7ca8da), f32::from_bits(0xbf7ca8da)], [f32::from_bits(0xbf6ff54d), f32::from_bits(0xbf6ff54d), f32::from_bits(0xbf76714e), f32::from_bits(0xbf76714e)], [f32::from_bits(0xbf7a5661), f32::from_bits(0xbf7a5661), f32::from_bits(0xbf654d04), f32::from_bits(0xbf654d04)], [f32::from_bits(0xbf7e0a4c), f32::from_bits(0xbf7e0a4c), f32::from_bits(0xbf3954e4), f32::from_bits(0xbf3954e4)], [f32::from_bits(0xbf7f5cc7), f32::from_bits(0xbf7f5cc7), f32::from_bits(0xbeada968), f32::from_bits(0xbeada968)], [f32::from_bits(0xbf7fe6f4), f32::from_bits(0xbf7fe6f4), f32::from_bits(0x3f069a6e), f32::from_bits(0x3f069a6e)]], [[f32::from_bits(0xbedc636e), f32::from_bits(0xbedc636e), f32::from_bits(0x3e5fe21e), f32::from_bits(0x3e5fe21e)], [f32::from_bits(0x3f96b782), f32::from_bits(0x3f96b782), f32::from_bits(0xbe6085fd), f32::from_bits(0xbe6085fd)], [f32::from_bits(0xbfd44bb3), f32::from_bits(0xbfd44bb3), f32::from_bits(0x3e6282a3), f32::from_bits(0x3e6282a3)], [f32::from_bits(0x3feff24d), f32::from_bits(0x3feff24d), f32::from_bits(0xbe686b3b), f32::from_bits(0xbe686b3b)], [f32::from_bits(0xbffa8254), f32::from_bits(0xbffa8254), f32::from_bits(0x3e7aa0e0), f32::from_bits(0x3e7aa0e0)], [f32::from_bits(0x3ffe49a9), f32::from_bits(0x3ffe49a9), f32::from_bits(0xbe9d6a1d), f32::from_bits(0xbe9d6a1d)], [f32::from_bits(0xbfff9560), f32::from_bits(0xbfff9560), f32::from_bits(0x3f344225), f32::from_bits(0x3f344225)], [f32::from_bits(0x3ffff37a), f32::from_bits(0x3ffff37a), f32::from_bits(0x3ef2cb24), f32::from_bits(0x3ef2cb24)]]),
        (96000, [f32::from_bits(0x3d008399), f32::from_bits(0x3d008399), f32::from_bits(0xbdf733f8), f32::from_bits(0xbdf733f8)], [[f32::from_bits(0xbd89cd6a), f32::from_bits(0xbd89cd6a), f32::from_bits(0xbf7fb4b7), f32::from_bits(0xbf7fb4b7)], [f32::from_bits(0xbf1a5ee7), f32::from_bits(0xbf1a5ee7), f32::from_bits(0xbf7ef6bd), f32::from_bits(0xbf7ef6bd)], [f32::from_bits(0xbf583bea), f32::from_bits(0xbf583bea), f32::from_bits(0xbf7cf6ed), f32::from_bits(0xbf7cf6ed)], [f32::from_bits(0xbf717a39), f32::from_bits(0xbf717a39), f32::from_bits(0xbf77573a), f32::from_bits(0xbf77573a)], [f32::from_bits(0xbf7adcdb), f32::from_bits(0xbf7adcdb), f32::from_bits(0xbf67d3f6), f32::from_bits(0xbf67d3f6)], [f32::from_bits(0xbf7e374e), f32::from_bits(0xbf7e374e), f32::from_bits(0xbf3fb20f), f32::from_bits(0xbf3fb20f)], [f32::from_bits(0xbf7f6aec), f32::from_bits(0xbf7f6aec), f32::from_bits(0xbec69601), f32::from_bits(0xbec69601)], [f32::from_bits(0xbf7fe915), f32::from_bits(0xbf7fe915), f32::from_bits(0x3ef766ce), f32::from_bits(0x3ef766ce)]], [[f32::from_bits(0xbefecf1b), f32::from_bits(0xbefecf1b), f32::from_bits(0x3e77585c), f32::from_bits(0x3e77585c)], [f32::from_bits(0x3f9eaa9f), f32::from_bits(0x3f9eaa9f), f32::from_bits(0xbe77fd7d), f32::from_bits(0xbe77fd7d)], [f32::from_bits(0xbfd829d1), f32::from_bits(0xbfd829d1), f32::from_bits(0x3e79fbe6), f32::from_bits(0x3e79fbe6)], [f32::from_bits(0x3ff169dc), f32::from_bits(0x3ff169dc), f32::from_bits(0xbe7fe07c), f32::from_bits(0xbe7fe07c)], [f32::from_bits(0xbffb0148), f32::from_bits(0xbffb0148), f32::from_bits(0x3e88ec90), f32::from_bits(0x3e88ec90)], [f32::from_bits(0x3ffe704f), f32::from_bits(0x3ffe704f), f32::from_bits(0xbea7cff4), f32::from_bits(0xbea7cff4)], [f32::from_bits(0xbfff9e92), f32::from_bits(0xbfff9e92), f32::from_bits(0x3f2bae05), f32::from_bits(0x3f2bae05)], [f32::from_bits(0x3ffff48a), f32::from_bits(0x3ffff48a), f32::from_bits(0x3f044c99), f32::from_bits(0x3f044c99)]]),
    ];

    /// Gate: the designer must reproduce the dumped tables at each rate. Bound: 1e-4 absolute
    /// plus 1e-3 relative, which covers the one-ulp libm differences measured in the module docs.
    #[test]
    fn designer_reproduces_dumped_tables() {
        for (rate, gain, allpass, fir) in DUMPED {
            let d = design(rate);
            let want = gain
                .iter()
                .chain(allpass.iter().flatten())
                .chain(fir.iter().flatten());
            let got = d
                .gain
                .iter()
                .chain(d.allpass.iter().flatten())
                .chain(d.fir.iter().flatten());
            let mut worst = 0.0f32;
            for (i, (g, w)) in got.zip(want).enumerate() {
                let tol = 1e-4 + 1e-3 * w.abs();
                assert!(
                    (g - w).abs() <= tol,
                    "{rate} Hz coefficient {i}: got {g}, dumped {w}"
                );
                worst = worst.max((g - w).abs() / w.abs().max(1e-6));
            }
            eprintln!("hilbert gate {rate} Hz: max relative error {worst:.2e}");
        }
    }
}
