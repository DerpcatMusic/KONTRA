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
        }
    }
}
