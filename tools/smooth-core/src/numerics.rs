//! Admitted mathematical programs from separate clean handoffs.
//! Hardware seeds are explicit x86_64 dependencies; runtime dispatch and NaN repair remain
//! unresolved. Outside checked domains/FP modes use named independent library fallbacks.
use crate::fractional_table::FRACTIONAL_TABLE;
use crate::weighting_tables::{VECTOR_EXP_TABLE, VECTOR_LOG_TABLE};

/// Canonical programs require nearest-even, denormals enabled and traps masked. Do not alter the
/// host environment; unsupported architectures/modes select an independent library fallback.
pub fn canonical_estimate_environment() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        #[allow(
            deprecated,
            reason = "MXCSR is the only readback of the FP environment; the intrinsic is deprecated in favour of inline asm"
        )]
        let control = unsafe { std::arch::x86_64::_mm_getcsr() };
        control & 0xe040 == 0 && control & 0x1f80 == 0x1f80
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}

#[cfg(target_arch = "x86_64")]
pub(crate) fn reciprocal_seed(x: f32) -> f32 {
    // SSE is an x86_64 baseline feature. Use the packed primitive, not ideal division.
    unsafe {
        use std::arch::x86_64::{_mm_cvtss_f32, _mm_rcp_ps, _mm_set1_ps};
        _mm_cvtss_f32(_mm_rcp_ps(_mm_set1_ps(x)))
    }
}
#[cfg(target_arch = "x86_64")]
fn reciprocal_sqrt_seed(x: f32) -> f32 {
    unsafe {
        use std::arch::x86_64::{_mm_cvtss_f32, _mm_rsqrt_ps, _mm_set1_ps};
        _mm_cvtss_f32(_mm_rsqrt_ps(_mm_set1_ps(x)))
    }
}
fn reciprocal_domain(x: f32) -> bool {
    (0x00800000..=0x7e7fe800).contains(&x.to_bits())
}

/// Return-only diagnostic: it does not model hardware floating-point exception flags.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KernelLane {
    pub value: f32,
    pub status: u8,
}

/// Checked reciprocal lane. Positive subnormals, negatives and NaNs are not admitted.
pub fn recip32_lane(x: f32) -> Result<KernelLane, &'static str> {
    if !canonical_estimate_environment() {
        return Err("reciprocal requires the canonical x86_64 FP environment");
    }
    let bits = x.to_bits();
    if bits == 0 {
        return Ok(KernelLane {
            value: f32::INFINITY,
            status: 18,
        });
    }
    if (0x7e7fe801..=0x7f800000).contains(&bits) {
        return Ok(KernelLane {
            value: 1.0_f32 / x,
            status: 0,
        });
    }
    #[cfg(target_arch = "x86_64")]
    if reciprocal_domain(x) {
        let seed = reciprocal_seed(x);
        if seed.is_finite() && seed != 0.0 {
            let product = seed * x;
            let correction = product * seed;
            let doubled = seed + seed;
            let result = doubled - correction;
            if result.is_finite() && result > 0.0 {
                return Ok(KernelLane {
                    value: result,
                    status: 0,
                });
            }
        }
    }
    Err("reciprocal input is outside the admitted lane domain")
}

/// Checked source program, or independent scalar division outside its domain/environment.
pub fn recip32(x: f32) -> f32 {
    recip32_lane(x).map_or_else(|_| 1.0 / x, |lane| lane.value)
}

/// Division uses the distinct cubic residual program; it is not A*recip32(D).
pub fn div32(numerator: f32, denominator: f32) -> f32 {
    #[cfg(target_arch = "x86_64")]
    if reciprocal_domain(denominator) && numerator.is_finite() && canonical_estimate_environment() {
        let seed = reciprocal_seed(denominator);
        if seed.is_finite() && seed != 0.0 {
            let product = denominator * seed;
            let residual = 1.0 - product;
            let correction = residual * seed;
            let sum = correction + seed;
            let second = sum * residual;
            let refined = seed + second;
            let result = numerator * refined;
            if result.is_finite() {
                return result;
            }
        }
    }
    numerator / denominator
}

pub fn fractional_domain(x: f32, exponent: f32) -> bool {
    let upper = if exponent.to_bits() == 0x3f000000 {
        0x7f7ff000
    } else {
        0x7f7ffffe
    };
    (0x00800000..=upper).contains(&x.to_bits())
        && (0x3dccccd0..=0x3f19999a).contains(&exponent.to_bits())
}

/// Table-assisted, compensated fractional transform, with an explicit packed rsqrt half branch.
/// Unsupported domains/FP modes/architectures use independent double-library power.
pub fn pow32(x: f32, exponent: f32) -> f32 {
    if !fractional_domain(x, exponent) || !canonical_estimate_environment() {
        return (x as f64).powf(exponent as f64) as f32;
    }
    power_program(x, exponent)
}

/// The weighting scalar admission is broader in exponent but narrower in base than Pow32.
pub fn scalar_power32(x: f32, exponent: f32) -> f32 {
    if exponent == 4.0 && x.is_finite() && x >= 0.0 {
        return power_four32(x);
    }
    if x.to_bits() == 0 && exponent.is_finite() && exponent > 0.0 && exponent != 0.5 {
        return 0.0; // Supplied positive-zero repair; half-zero remains independent fallback.
    }
    if !(0x1f800000..=0x5f800000).contains(&x.to_bits())
        || !(0x3c23d70b..=0x3f7d70a3).contains(&exponent.to_bits())
        || !canonical_estimate_environment()
    {
        return (x as f64).powf(exponent as f64) as f32;
    }
    power_program(x, exponent)
}

pub fn power_four32(x: f32) -> f32 {
    if !canonical_estimate_environment() {
        return (x as f64).powf(4.0) as f32;
    }
    let square = x * x;
    let cube = square * x;
    cube * x
}

fn power_program(x: f32, exponent: f32) -> f32 {
    #[cfg(target_arch = "x86_64")]
    if exponent.to_bits() == 0x3f000000 {
        let seed = reciprocal_sqrt_seed(x);
        let scaled = x * seed;
        let square = scaled * scaled;
        let residual = x - square;
        let half_seed = 0.5 * seed;
        let correction = half_seed * residual;
        let result = scaled + correction;
        return if result.is_finite() { result } else { x.sqrt() };
    }
    let bits = x.to_bits();
    let index = (((bits & 0x007f0000) + 0x00010000) >> 17) as usize;
    let [high_bits, low_bits, reciprocal_bits] = FRACTIONAL_TABLE[index];
    let table_high = f32::from_bits(high_bits);
    let table_low = f32::from_bits(low_bits);
    let table_reciprocal = f32::from_bits(reciprocal_bits);
    let binary_exponent = ((bits >> 23) as i32 - 127) as f32;
    let mantissa = f32::from_bits((bits & 0x007fffff) | 0x3f800000);
    let mantissa_high = f32::from_bits(mantissa.to_bits() & 0x7fffc000);
    let mantissa_low = mantissa - mantissa_high;
    let high_product = mantissa_high * table_reciprocal;
    let z_high = high_product - 1.0;
    let z_low = mantissa_low * table_reciprocal;
    let z = z_high + z_low;
    let j0 = -0.25 * z;
    let j1 = j0 + f32::from_bits(0x3eaaaaab);
    let j2 = j1 * z;
    let j3 = j2 + -0.5;
    let z_square = z * z;
    let j4 = j3 * z_square;
    let a0 = table_high + z_high;
    let exponent_high = binary_exponent * f32::from_bits(0x3f317000);
    let a1 = a0 + exponent_high;
    let exponent_low = binary_exponent * f32::from_bits(0x3805fdf4);
    let d0 = table_low + exponent_low;
    let d1 = j4 + d0;
    let near_one = bits > 0x3f7e0000 && bits < 0x3f820000;
    let a = a1 + if near_one { z_low } else { 0.0 };
    let d = d1 + if near_one { 0.0 } else { z_low };
    let sum = a + d;
    let split = f32::from_bits(0x45800800) * sum;
    let combined = sum + split;
    let high = combined - split;
    let residual = a - high;
    let low = residual + d;
    let split_exponent = f32::from_bits(0x45800800) * exponent;
    let delta_exponent = split_exponent - exponent;
    let exponent_high = split_exponent - delta_exponent;
    let exponent_low = exponent - exponent_high;
    let product_high = high * exponent_high;
    let q0 = low * exponent_low;
    let q1 = high * exponent_low;
    let q2 = q0 + q1;
    let q3 = low * exponent_high;
    let product_low = q2 + q3;
    let scaled = product_high * f32::from_bits(0x3fb8aa3b);
    let biased = scaled + f32::from_bits(0x4b400000);
    let power = biased - f32::from_bits(0x4b400000);
    let upper = power * f32::from_bits(0x3f317200);
    let r0 = product_high - upper;
    let lower = power * f32::from_bits(0x35bfbe8e);
    let r1 = r0 - lower;
    let remainder = r1 + product_low;
    let w0 = f32::from_bits(0x3c07d9fe) * remainder;
    let w1 = w0 + f32::from_bits(0x3d2b8392);
    let w2 = w1 * remainder;
    let w3 = w2 + f32::from_bits(0x3e2aacac);
    let w4 = w3 * remainder;
    let w5 = w4 + f32::from_bits(0x3effff34);
    let w6 = w5 * remainder;
    let w7 = w6 + f32::from_bits(0x3f7ffffe);
    let w8 = remainder * w7;
    let polynomial = w8 + 1.0;
    f32::from_bits(
        biased
            .to_bits()
            .wrapping_shl(23)
            .wrapping_add(polynomial.to_bits()),
    )
}

/// Separate mixed64/32 weighting program with an unrefined packed reciprocal seed.
pub fn vector_power32(x: f32, exponent: f32) -> f32 {
    if x.to_bits() == 0 && exponent.is_finite() && exponent > 0.0 {
        return 0.0;
    }
    let fallback = || (x as f64).powf(exponent as f64) as f32;
    if !(0x37800000..=0x47800000).contains(&x.to_bits())
        || !(1.0..=5.0).contains(&exponent)
        || !canonical_estimate_environment()
    {
        return fallback();
    }
    #[cfg(target_arch = "x86_64")]
    {
        let original = (x as f64).to_bits();
        let biased_exponent = (original >> 52) as i32;
        let mantissa = f64::from_bits((original & 0x000fffffffffffff) | 0x3f50000000000000);
        let seed = reciprocal_seed(mantissa as f32) as f64;
        let bias = f64::from_bits(0x4338000000000000);
        let snapped = seed + bias;
        let reciprocal = snapped - bias;
        let product = mantissa * reciprocal;
        let z = product - 1.0;
        if !(512.0..=1024.0).contains(&reciprocal)
            || reciprocal.fract() != 0.0
            || z.abs() > 1.0 / 128.0
        {
            return fallback();
        }
        let exponent_base = if reciprocal > 724.0 { 1023 } else { 1022 };
        let binary_exponent = (biased_exponent - exponent_base) as f64;
        let j0 = f64::from_bits(0x3fd55555555543c5) * z;
        let j1 = j0 + f64::from_bits(0xbfdffffffffff81f);
        let square = z * z;
        let j2 = j1 * square;
        let j3 = z + j2;
        let l0 = f64::from_bits(VECTOR_LOG_TABLE[reciprocal as usize - 512]) + j3;
        let l1 = binary_exponent * f64::from_bits(0x3fe62e42fefa39ef);
        let logarithm = l1 + l0;
        let q = exponent as f64 * logarithm;
        let t = q * f64::from_bits(0x40a71547652b82fe);
        let half = t - 0.5;
        let magic = half + bias;
        let n = magic - bias;
        let fraction = t - n;
        let index = (magic.to_bits() & 0x7ff) as usize;
        let table = f64::from_bits(VECTOR_EXP_TABLE[index]);
        let d0 = fraction * f64::from_bits(0x3f362f3904051fa1);
        let d1 = d0 * table;
        let v = table + d1;
        let scale = (magic.to_bits() >> 11).wrapping_shl(52);
        let result = f64::from_bits(v.to_bits().wrapping_add(scale)) as f32;
        return if result.is_normal() && result > 0.0 {
            result
        } else {
            fallback()
        };
    }
    #[cfg(not(target_arch = "x86_64"))]
    fallback()
}

/// Positive finite normal lanes use the supplied bit decomposition and f32 polynomial.
pub fn ln32(x: f32) -> f32 {
    if !canonical_estimate_environment() || !(0x00800000..=0x7f7fffff).contains(&x.to_bits()) {
        return (x as f64).ln() as f32;
    }
    let offset = x.to_bits().wrapping_sub(0x3f2aaaab) as i32;
    let exponent = offset >> 23;
    let mantissa = f32::from_bits(((offset as u32) & 0x007fffff) + 0x3f2aaaab);
    let difference = mantissa - 1.0;
    let product = -0.15177205204963684_f32 * difference;
    let mut polynomial = product + 0.16964881122112274_f32;
    let product = polynomial * difference;
    polynomial = product - 0.16462457180023193_f32;
    let product = polynomial * difference;
    polynomial = product + 0.19822503626346588_f32;
    let product = polynomial * difference;
    polynomial = product - 0.25004664063453674_f32;
    let product = polynomial * difference;
    polynomial = product + 0.33336564898490906_f32;
    let product = polynomial * difference;
    polynomial = product - 0.5;
    polynomial *= difference;
    polynomial *= difference;
    let fraction = difference + polynomial;
    let exponent = exponent as f32 * 0.6931471824645996_f32;
    fraction + exponent
}

/// Central finite lanes use separate bias-rounding stages and exponent-field addition.
fn ordinary_exp32(x: f32) -> f32 {
    let product = x * 1.4426950216293335_f32;
    let biased = product + 12582912.0_f32;
    let exponent = biased - 12582912.0_f32;
    let high = exponent * 0.693145751953125_f32;
    let remainder = x - high;
    let low = exponent * 1.428606765330187e-6_f32;
    let remainder = remainder - low;
    let product = 0.008291719481348991_f32 * remainder;
    let mut polynomial = product + 0.041873522102832794_f32;
    let product = polynomial * remainder;
    polynomial = product + 0.16667431592941284_f32;
    let product = polynomial * remainder;
    polynomial = product + 0.4999939203262329_f32;
    let product = polynomial * remainder;
    polynomial = product + 0.9999998807907104_f32;
    let product = polynomial * remainder;
    polynomial = product + 1.0;
    let exponent_field = biased.to_bits().wrapping_shl(23);
    f32::from_bits(polynomial.to_bits().wrapping_add(exponent_field))
}

/// All signed finite lanes and infinities are admitted under the checked FP environment.
/// The subnormal branch deliberately performs two separately rounded f32 products.
pub fn exp32_lane(x: f32) -> Result<KernelLane, &'static str> {
    if !canonical_estimate_environment() || x.is_nan() {
        return Err("exponential requires a non-NaN lane and canonical x86_64 FP environment");
    }
    if x == f32::INFINITY {
        return Ok(KernelLane {
            value: f32::INFINITY,
            status: 0,
        });
    }
    if x == f32::NEG_INFINITY {
        return Ok(KernelLane {
            value: 0.0,
            status: 0,
        });
    }
    let guard = f32::from_bits(0x42aeac4f);
    if x.abs() <= guard {
        return Ok(KernelLane {
            value: ordinary_exp32(x),
            status: 0,
        });
    }
    if x > f32::from_bits(0x42b17217) {
        let maximum = f32::from_bits(0x7f7fffff);
        return Ok(KernelLane {
            value: maximum * maximum,
            status: 12,
        });
    }
    if x < f32::from_bits(0xc2cff1b4) {
        let minimum = f32::from_bits(1);
        return Ok(KernelLane {
            value: minimum * minimum,
            status: 17,
        });
    }
    let product = x * f32::from_bits(0x3fb8aa3b);
    let biased = product + f32::from_bits(0x4b400000);
    let exponent = biased - f32::from_bits(0x4b400000);
    let integer = (biased.to_bits() as u16) as i16 as i32;
    let high = exponent * f32::from_bits(0x3f317200);
    let remainder = x - high;
    let low = exponent * f32::from_bits(0x35bfbe8e);
    let remainder = remainder - low;
    let product = f32::from_bits(0x3c0950ef) * remainder;
    let mut polynomial = product + f32::from_bits(0x3d2bb1b6);
    let product = polynomial * remainder;
    polynomial = product + f32::from_bits(0x3e2aa9c6);
    let product = polynomial * remainder;
    polynomial = product + f32::from_bits(0x3efffe85);
    let product = polynomial * remainder;
    polynomial = product + f32::from_bits(0x3f800001);
    let product = polynomial * remainder;
    polynomial = f32::from_bits(0x3f800001) + product;
    let (value, status) = if x < -guard {
        let scale = f32::from_bits(((integer + 60 + 127) as u32) << 23);
        let scaled = polynomial * scale;
        (scaled * f32::from_bits(0x21800000), 17)
    } else if integer <= 127 {
        let scale = f32::from_bits(((integer + 127) as u32) << 23);
        (polynomial * scale, 0)
    } else {
        let scaled = polynomial * f32::from_bits(0x7f000000);
        (scaled * 2.0, 0)
    };
    Ok(KernelLane { value, status })
}

/// Checked source program, or independent double-library exponential outside its FP/NaN admission.
pub fn exp32(x: f32) -> f32 {
    exp32_lane(x).map_or_else(|_| (x as f64).exp() as f32, |lane| lane.value)
}

fn map_lanes_in_place(
    values: &mut [f32],
    lane: fn(f32) -> Result<KernelLane, &'static str>,
) -> Result<u8, &'static str> {
    if values.is_empty() {
        return Err("array kernel requires a positive length");
    }
    let mut status = 0;
    for value in values {
        let result = lane(*value)?;
        *value = result.value;
        if result.status != 0 {
            status = result.status;
        }
    }
    Ok(status)
}

/// Increasing-index return-only status accumulation; exact in-place aliasing is supported.
pub fn exp32_in_place(values: &mut [f32]) -> Result<u8, &'static str> {
    if !canonical_estimate_environment() || values.iter().any(|x| x.is_nan()) {
        return Err("exponential array is outside checked admission");
    }
    map_lanes_in_place(values, exp32_lane)
}

pub fn recip32_in_place(values: &mut [f32]) -> Result<u8, &'static str> {
    if !canonical_estimate_environment()
        || values.iter().any(|x| {
            let bits = x.to_bits();
            bits != 0 && !(0x00800000..=0x7f800000).contains(&bits)
        })
    {
        return Err("reciprocal array is outside checked admission");
    }
    map_lanes_in_place(values, recip32_lane)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_arch = "x86_64")]
    #[allow(
        deprecated,
        reason = "the test sets and reads MXCSR directly, the only way to drive the FP environment"
    )]
    fn checked_fp_environment_rejects_modes_and_restores_test_thread() {
        use std::arch::x86_64::{_mm_getcsr, _mm_setcsr};
        struct Restore(u32);
        impl Drop for Restore {
            fn drop(&mut self) {
                unsafe { _mm_setcsr(self.0) };
            }
        }
        let saved = unsafe { _mm_getcsr() };
        let _restore = Restore(saved);
        for control in [
            saved | 0x8000,
            saved | 0x40,
            (saved & !0x6000) | 0x2000,
            (saved & !0x3f) & !0x80,
        ] {
            unsafe { _mm_setcsr(control) };
            assert!(!canonical_estimate_environment());
            assert!(exp32_lane(0.0).is_err());
            assert!(recip32_lane(1.0).is_err());
        }
        unsafe { _mm_setcsr(saved) };
        assert_eq!(unsafe { _mm_getcsr() }, saved);
    }

    #[test]
    fn exponential_repair_boundary_bits_and_last_status() {
        assert!(
            canonical_estimate_environment(),
            "fixture requires admitted x86_64 FP mode"
        );
        let fixtures = [
            (0x00000000, 0x3f800000, 0),
            (0x80000000, 0x3f800000, 0),
            (0x42aeac4e, 0x7e7fff34, 0),
            (0x42aeac4f, 0x7e7fffb4, 0),
            (0x42aeac50, 0x7e80001b, 0),
            (0xc2aeac4e, 0x00800066, 0),
            (0xc2aeac4f, 0x00800026, 0),
            (0xc2aeac50, 0x007fffe7, 17),
            (0x42b17217, 0x7f7fff86, 0),
            (0x42b17218, 0x7f800000, 12),
            (0xc2cff1b4, 0x00000001, 17),
            (0xc2cff1b5, 0x00000000, 17),
            (0xc2b40000, 0x0008ec28, 17),
            (0xc2c80000, 0x0000001b, 17),
            (0x7f800000, 0x7f800000, 0),
            (0xff800000, 0x00000000, 0),
        ];
        for (input, expected, status) in fixtures {
            let result = exp32_lane(f32::from_bits(input)).unwrap();
            assert_eq!(result.value.to_bits(), expected, "input {input:08x}");
            assert_eq!(result.status, status);
        }
        let mut lanes = [100.0, -100.0, 0.75, f32::INFINITY];
        assert_eq!(exp32_in_place(&mut lanes).unwrap(), 17);
        let mut reversed = [-100.0, 100.0, 0.75];
        assert_eq!(exp32_in_place(&mut reversed).unwrap(), 12);
        assert!(exp32_in_place(&mut []).is_err());
        assert!(exp32_lane(f32::NAN).is_err());
    }

    #[test]
    fn reciprocal_large_positive_zero_and_checked_admission() {
        assert!(
            canonical_estimate_environment(),
            "fixture requires admitted x86_64 FP mode"
        );
        for input in [0x7e7fe801, 0x7f000000, 0x7f7fffff, 0x7f800000] {
            let x = f32::from_bits(input);
            let result = recip32_lane(x).unwrap();
            assert_eq!(result.value.to_bits(), (1.0_f32 / x).to_bits());
            assert_eq!(result.status, 0);
        }
        let zero = recip32_lane(0.0).unwrap();
        assert_eq!(zero.value.to_bits(), 0x7f800000);
        assert_eq!(zero.status, 18);
        let mut lanes = [0.0, 2.0, f32::INFINITY];
        assert_eq!(recip32_in_place(&mut lanes).unwrap(), 18);
        assert_eq!(lanes[2].to_bits(), 0);
        let mut rejected = [2.0, f32::from_bits(1)];
        assert!(recip32_in_place(&mut rejected).is_err());
        assert_eq!(rejected[0], 2.0);
        assert!(recip32_lane(-0.0).is_err());
        assert!(recip32_lane(f32::NAN).is_err());
    }

    #[test]
    fn weighting_scalar_and_mixed_precision_golden_bits() {
        assert_eq!(scalar_power32(0.0, 0.75).to_bits(), 0);
        assert_eq!(vector_power32(0.0, 5.0).to_bits(), 0);
        if !canonical_estimate_environment() {
            return;
        }
        for (x, p, expected) in [
            (0x1f800000, 0x3c23d70b, 0x3f24474e),
            (0x5f800000, 0x3f7d70a3, 0x5f244736),
            (0x3f820001, 0x3f400000, 0x3f817f42),
            (0x447a0000, 0x3f7d70a3, 0x44695041),
            (0x3f800000, 0x3c23d70b, 0x3f800000),
        ] {
            assert_eq!(
                scalar_power32(f32::from_bits(x), f32::from_bits(p)).to_bits(),
                expected
            );
        }
        for (x, p, expected) in [
            (0x37800000, 0x3f800000, 0x37800000),
            (0x47800000, 0x40a00000, 0x67800000),
            (0x3f800000, 0x40800000, 0x3f800000),
            (0x40000000, 0x3fd9999a, 0x404fefc6),
            (0x40000000, 0x40555555, 0x41214517),
            (0x3fc00000, 0x40866666, 0x40afaf36),
        ] {
            assert_eq!(
                vector_power32(f32::from_bits(x), f32::from_bits(p)).to_bits(),
                expected
            );
        }
        let x = f32::from_bits(0x3f012345);
        assert_eq!(scalar_power32(x, 4.0).to_bits(), 0x3d849cb4);
        let square = x * x;
        assert_eq!((square * square).to_bits(), 0x3d849cb5);
        assert_eq!(VECTOR_LOG_TABLE[0], 0x8000000000000000);
        assert_eq!(VECTOR_LOG_TABLE[512], 0);
        assert_eq!(VECTOR_EXP_TABLE[0], 0x3feffffffc27dd9e); // Supplied bias; not ideal exp(0).
    }

    #[test]
    fn fractional_table_matches_mathematical_recipe() {
        // The shipped validation additionally regenerates these words with 90-digit decimals.
        for (k, words) in FRACTIONAL_TABLE.iter().enumerate() {
            let reciprocal = (16384.0_f64 / (64 + k) as f64).round_ties_even() / 256.0;
            let logarithm = if k == 0 { 0.0 } else { -reciprocal.ln() };
            let high = (16384.0 * logarithm).floor() / 16384.0;
            assert_eq!(
                *words,
                [
                    (high as f32).to_bits(),
                    ((logarithm - high) as f32).to_bits(),
                    (reciprocal as f32).to_bits()
                ]
            );
        }
    }

    #[test]
    fn fractional_program_bits_and_admission_boundaries() {
        // Independently evaluated clean mathematics; not vendor runtime outputs.
        if canonical_estimate_environment() {
            for (x, p, expected) in [
                (0x00800000, 0x3e2aaaab, 0x34fffff9),
                (0x3f7e0000, 0x3eaaaaab, 0x3f7f54e3),
                (0x3f7e0001, 0x3eaaaaab, 0x3f7f54e3),
                (0x3f81ffff, 0x3f19999a, 0x3f81323f),
                (0x3f820000, 0x3f19999a, 0x3f81323f),
                (0x3f820001, 0x3dccccd0, 0x3f8032d8),
                (0x3f800000, 0x3e2aaaab, 0x3f800000),
                (0x3f800000, 0x3eaaaaab, 0x3f800000),
                (0x40000000, 0x3f19999a, 0x3fc20302),
                (0x447a0000, 0x3dccccd0, 0x3fff64c4),
                (0x7f7ffffe, 0x3f19999a, 0x65dedc84),
            ] {
                assert_eq!(
                    pow32(f32::from_bits(x), f32::from_bits(p)).to_bits(),
                    expected
                );
            }
        }
        assert!(fractional_domain(
            f32::MIN_POSITIVE,
            f32::from_bits(0x3dccccd0)
        ));
        assert!(!fractional_domain(f32::from_bits(0x007fffff), 0.5));
        assert!(!fractional_domain(f32::from_bits(0x7f7fffff), 0.25));
        assert!(fractional_domain(f32::from_bits(0x7f7ff000), 0.5));
        assert!(!fractional_domain(f32::from_bits(0x7f7ff001), 0.5));
        assert_eq!(pow32(0.0, 0.5), 0.0);
        assert_eq!(pow32(4.0, 2.0), 16.0);
    }

    #[test]
    #[cfg(target_arch = "x86_64")]
    fn packed_estimate_refinements_are_distinct_staged_programs() {
        assert!(
            canonical_estimate_environment(),
            "check requires nearest-even, FTZ/DAZ off"
        );
        for bits in [0x00800000, 0x3f800000, 0x40400000, 0x7e7fe800] {
            let d = f32::from_bits(bits);
            let r = reciprocal_seed(d);
            let t = d * r;
            let e = 1.0 - t;
            let p = e * r;
            let q = p + r;
            let v = q * e;
            let refined = r + v;
            assert_eq!(div32(0.375, d).to_bits(), (0.375 * refined).to_bits());
            let first = r * d;
            let second = first * r;
            let doubled = r + r;
            assert_eq!(recip32(d).to_bits(), (doubled - second).to_bits());
        }
        // This host's packed estimate retains a different residual in the one-step program.
        assert_ne!(recip32(1.0).to_bits(), div32(1.0, 1.0).to_bits());
        for bits in [0x00800000, 0x3f820001, 0x447a0000, 0x7f7ff000] {
            let x = f32::from_bits(bits);
            let r = reciprocal_sqrt_seed(x);
            let s = x * r;
            let square = s * s;
            let error = x - square;
            let half = 0.5 * r;
            let delta = half * error;
            assert_eq!(pow32(x, 0.5).to_bits(), (s + delta).to_bits());
        }
        assert_eq!(div32(3.0, -2.0), -1.5); // Independent exceptional-domain fallback.
        assert_eq!(recip32(-2.0), -0.5);
    }

    #[test]
    fn canonical_normal_lane_bits_and_declared_fallback_boundaries() {
        // Golden bits evaluate the clean staged program, not a selected vendor runtime.
        for (input, expected) in [
            (0x00800000, 0xc2aeac50),
            (0x3c23d70a, 0xc0935d8e),
            (0x3f000000, 0xbf317218),
            (0x3f800000, 0x00000000),
            (0x40000000, 0x3f317218),
            (0x40400000, 0x3f8c9f54),
            (0x447a0000, 0x40dd0c55),
            (0x7f7fffff, 0x42b17218),
        ] {
            assert_eq!(ln32(f32::from_bits(input)).to_bits(), expected);
        }
        for (input, expected) in [
            (0x00000000, 0x3f800000),
            (0x3f800000, 0x402df855),
            (0xbf800000, 0x3ebc5ab3),
            (0x3f317218, 0x40000000),
            (0xbf317218, 0x3f000000),
            (0x42ae0000, 0x7e36d808),
            (0xc2ae0000, 0x00b33686),
            (0x42aeac4f, 0x7e7fffb4),
            (0xc2aeac4f, 0x00800026),
        ] {
            assert_eq!(exp32(f32::from_bits(input)).to_bits(), expected);
        }
        assert_eq!(ln32(0.0), f32::NEG_INFINITY);
        assert!(ln32(-1.0).is_nan());
        assert!(ln32(f32::from_bits(1)) < -100.0);
        assert_eq!(exp32(90.0), f32::INFINITY);
        assert!(exp32(-100.0) > 0.0 && exp32(-100.0) < f32::MIN_POSITIVE);
        assert!(exp32(f32::NAN).is_nan());
    }
}
