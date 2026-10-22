//! First-order shelf `[b0, b1, a1]` of `(b0 + b1·z⁻¹)/(1 + a1·z⁻¹)` with DC
//! gain `g0` and Nyquist gain `gp`. One design for every reverb shelf; the
//! [`Warp`] selects how the corner frequency maps to the coefficients, which
//! is the only thing that differs between the native routines.

use std::f64::consts::PI;

#[derive(Clone, Copy)]
pub(crate) enum Warp {
    /// Tangent warp of a corner in cycles per sample (`0x141649120`, the
    /// spark and generic presets).
    Tan(f64),
    /// Sine warp of a corner `f` Hz at `rate` (`0x141a92480`, Plate and its
    /// line shelves); `sin` is the native's sine (libm or the polynomial).
    Sin { f: f64, rate: f64, sin: fn(f64) -> f64 },
}

pub(crate) fn first_order(warp: Warp, g0: f64, gp: f64) -> [f64; 3] {
    match warp {
        Warp::Tan(fc) => {
            let r = g0 / gp;
            let t = (PI * fc).tan();
            let (lo, hi) = (t / r.sqrt(), t * r.sqrt());
            [gp * (hi + 1.0) / (lo + 1.0), gp * (hi - 1.0) / (lo + 1.0), (lo - 1.0) / (lo + 1.0)]
        }
        Warp::Sin { f, rate, sin } => {
            if g0 == gp {
                return [g0, 0.0, 0.0];
            }
            let q = (gp * gp - g0 * g0) / (g0 * g0 + gp * gp - 2.0 * gp * g0);
            let p = q - q.signum() * (q * q - 1.0).sqrt();
            let b0 = 0.5 * ((g0 - gp) * p + g0 + gp);
            let b1 = 0.5 * ((g0 + gp) * p + g0 - gp);
            let th = PI * (f / rate).min(1.0) / 2.0;
            let k = sin(th - PI / 4.0) / sin(th + PI / 4.0);
            let n = 1.0 / (k * p + 1.0);
            [(k * b1 + b0) * n, (k * b0 + b1) * n, (k + p) * n]
        }
    }
}

/// Bit fingerprints of both warps over a grid of corners, gains and rates,
/// pinned before the duplicate routines were merged into [`first_order`].
#[cfg(test)]
mod fingerprints {
    use super::*;

    fn eat(h: &mut u64, v: u64) {
        for b in v.to_le_bytes() {
            *h = (*h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3);
        }
    }

    #[test]
    fn shelf_designs_are_bit_identical() {
        let mut tan = 0xcbf2_9ce4_8422_2325u64;
        for fc in [250.0 / 48_000.0, 12_000.0 / 48_000.0, 0.25, 0.1, 0.4, 1e-4, 0.499] {
            for (g0, gp) in [(0.5, 1.0), (1.0, 0.5), (0.3, 0.7), (1.0, 1.0), (2.0, 0.1), (1e-3, 4.0)] {
                first_order(Warp::Tan(fc), g0, gp).iter().for_each(|v| eat(&mut tan, v.to_bits()));
            }
        }
        let mut sin = 0xcbf2_9ce4_8422_2325u64;
        for rate in [44_100.0, 48_000.0, 96_000.0, 192_000.0] {
            for f in [1000.0, 160.0, 5600.0, 757.0, 4851.0, 1.0, 0.4 * rate, 0.6 * rate] {
                for (g0, gp) in [(1.659_586_9, 0.5), (1.0, 1.0), (0.7, 1.3), (1e-3, 1e3), (3.0, 0.3)] {
                    first_order(Warp::Sin { f, rate, sin: f64::sin }, g0, gp).iter().for_each(|v| eat(&mut sin, v.to_bits()));
                    first_order(Warp::Sin { f, rate, sin: super::super::plate::psin }, g0, gp).iter().for_each(|v| eat(&mut sin, v.to_bits()));
                }
            }
        }
        assert_eq!([tan, sin], [0x5206_168c_e971_6bd4, 0xc2eb_69db_d758_08db]);
    }
}
