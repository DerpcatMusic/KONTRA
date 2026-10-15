//! Portable binary32 log10 for the native dynamics laws.
//!
//! Direct safe-Rust translation of CORE-MATH `cr_log10f`, commit
//! `8ea8ea35c518d06b72de4cf1d528e7a2d16f104c` (Alexei Sibidanov).
//! Source: <https://gitlab.inria.fr/core-math/core-math/-/blob/8ea8ea35c518d06b72de4cf1d528e7a2d16f104c/src/binary32/log10/log10f.c>.
//! C source SHA-256: `1a6c83703332c8d5516b73a2bada5553dee35ff35c9757b7e1184debee56b37d`.
//!
//! The numerical contract is round-to-nearest, ties-to-even, with IEEE binary64
//! intermediate operations rounded separately. Keep this operation order and do
//! not fuse multiply-adds or substitute platform logarithms. The upstream
//! algorithm supports additional C rounding modes; this Rust port does not expose
//! an FP-environment API. It returns the same special-value classes without
//! promising errno, exception flags, or NaN payload identity.
//!
//! Positive subnormal inputs are normalized with integer bits, before floating
//! arithmetic. The result for a positive finite input is zero or a normal f32;
//! the kernel needs no subnormal floating arithmetic, allocation, I/O, or locks.

/*
Copyright (c) 2022-2026 Alexei Sibidanov <sibid@uvic.ca>.

This file is part of the CORE-MATH project
(https://core-math.gitlabpages.inria.fr/).

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
*/

// Reciprocals of 1 + i/64, rounded to 29 bits (upstream tr).
const RECIPROCAL_BITS: [u64; 65] = [
    0x3ff0_0000_0000_0000, 0x3fef_81f8_2000_0000, 0x3fef_07c1_f000_0000, 0x3fee_9131_ac00_0000,
    0x3fee_1e1e_1e00_0000, 0x3fed_ae60_7700_0000, 0x3fed_41d4_1d00_0000, 0x3fec_d856_8900_0000,
    0x3fec_71c7_1c00_0000, 0x3fec_0e07_0400_0000, 0x3feb_acf9_1500_0000, 0x3feb_4e81_b500_0000,
    0x3fea_f286_bd00_0000, 0x3fea_98ef_6000_0000, 0x3fea_41a4_1a00_0000, 0x3fe9_ec8e_9500_0000,
    0x3fe9_9999_9a00_0000, 0x3fe9_48b0_fd00_0000, 0x3fe8_f9c1_9000_0000, 0x3fe8_acb9_0f00_0000,
    0x3fe8_6186_1800_0000, 0x3fe8_1818_1800_0000, 0x3fe7_d05f_4100_0000, 0x3fe7_8a4c_8100_0000,
    0x3fe7_45d1_7400_0000, 0x3fe7_02e0_5c00_0000, 0x3fe6_c16c_1700_0000, 0x3fe6_8168_1700_0000,
    0x3fe6_42c8_5900_0000, 0x3fe6_0581_6000_0000, 0x3fe5_c988_2c00_0000, 0x3fe5_8ed2_3100_0000,
    0x3fe5_5555_5500_0000, 0x3fe5_1d07_eb00_0000, 0x3fe4_e5e0_a700_0000, 0x3fe4_afd6_a000_0000,
    0x3fe4_7ae1_4800_0000, 0x3fe4_46f8_6500_0000, 0x3fe4_1414_1400_0000, 0x3fe3_e22c_bd00_0000,
    0x3fe3_b13b_1400_0000, 0x3fe3_8138_1400_0000, 0x3fe3_521c_fb00_0000, 0x3fe3_23e3_4a00_0000,
    0x3fe2_f684_be00_0000, 0x3fe2_c9fb_4e00_0000, 0x3fe2_9e41_2a00_0000, 0x3fe2_7350_b900_0000,
    0x3fe2_4924_9200_0000, 0x3fe2_1fb7_8100_0000, 0x3fe1_f704_7e00_0000, 0x3fe1_cf06_ae00_0000,
    0x3fe1_a7b9_6100_0000, 0x3fe1_8118_1200_0000, 0x3fe1_5b1e_5f00_0000, 0x3fe1_35c8_1100_0000,
    0x3fe1_1111_1100_0000, 0x3fe0_ecf5_6c00_0000, 0x3fe0_c971_5000_0000, 0x3fe0_a681_0a00_0000,
    0x3fe0_8421_0800_0000, 0x3fe0_624d_d300_0000, 0x3fe0_4104_1000_0000, 0x3fe0_2040_8100_0000,
    0x3fe0_0000_0000_0000,
];

// Logarithms of the reciprocals, including the upstream offset (tl).
const LOGARITHM_BITS: [u64; 65] = [
    0xbd12_0000_0000_0000, 0x3f7b_9476_8931_0dfa, 0x3f8b_5e90_9c96_d11b, 0x3f94_5f4f_59ed_1e08,
    0x3f9a_f5f9_2cbd_8bc1, 0x3fa0_ba01_a606_dcde, 0x3fa3_ed11_9b9a_29cd, 0x3fa7_1483_4298_ed14,
    0x3faa_30a9_d983_564c, 0x3fad_41d5_1267_0665, 0x3fb0_2428_c0f6_5442, 0x3fb1_a234_44ee_cb67,
    0x3fb3_1b30_543f_4bdd, 0x3fb4_8f3e_d39b_fc2d, 0x3fb5_fe80_49a0_e34b, 0x3fb7_6914_0a6a_9f30,
    0x3fb8_cf18_36c9_8bdb, 0x3fba_30a9_d555_40ca, 0x3fbb_8de4_d1ee_8167, 0x3fbc_e6e4_202c_a20f,
    0x3fbe_3bc1_acca_cd30, 0x3fbf_8c96_83b5_aafd, 0x3fc0_6cbd_68ca_9a03, 0x3fc1_1142_f19d_f6c5,
    0x3fc1_b3e7_1fa7_a913, 0x3fc2_54b4_d37a_4677, 0x3fc2_f3b6_912c_be9c, 0x3fc3_90f6_8311_581a,
    0x3fc4_2c7e_7fff_c53d, 0x3fc4_c658_08c7_8cd1, 0x3fc5_5e8c_5075_1bea, 0x3fc5_f524_45de_c36c,
    0x3fc6_8a28_8c3f_1195, 0x3fc7_1da1_7bdf_0cad, 0x3fc7_af97_3608_af6e, 0x3fc8_4011_952a_250e,
    0x3fc8_cf18_37a7_e9f4, 0x3fc9_5cb2_891e_436a, 0x3fc9_e8e7_b0f8_6974, 0x3fca_73be_aa5d_b121,
    0x3fca_fd3e_3945_5867, 0x3fcb_856c_f060_d985, 0x3fcc_0c51_34de_1f90, 0x3fcc_91f1_371b_c934,
    0x3fcd_1652_ffcd_3ee8, 0x3fcd_997c_6f63_5e09, 0x3fce_1b73_3ab9_0ed0, 0x3fce_9c3c_eada_c7eb,
    0x3fcf_1bde_ec43_a29a, 0x3fcf_9a5e_7a5f_a392, 0x3fd0_0be0_5ac0_2ef5, 0x3fd0_4a05_4d81_a29e,
    0x3fd0_87a0_8359_57c5, 0x3fd0_c4b4_5709_94e1, 0x3fd1_0143_1aa1_fe1b, 0x3fd1_3d4f_08b9_8da2,
    0x3fd1_78da_53ed_b85c, 0x3fd1_b3e7_1e9f_9d22, 0x3fd1_ee77_7def_deb7, 0x3fd2_288d_7b48_e205,
    0x3fd2_622b_0f52_e469, 0x3fd2_9b52_2a4c_62de, 0x3fd2_d404_b0e3_0f4a, 0x3fd3_0c44_78f3_fbaf,
    0x3fd3_4413_509f_78df,
];

// Upstream hash table st: exact binary32 powers of ten, with empty slots.
const POWER_OF_TEN_BITS: [u32; 16] = [
    0x5015_02f9, 0x4120_0000, 0x42c8_0000, 0x0000_0000,
    0x447a_0000, 0x0000_0000, 0x461c_4000, 0x47c3_5000,
    0x0000_0000, 0x4974_2400, 0x0000_0000, 0x4b18_9680,
    0x4cbe_bc20, 0x0000_0000, 0x4e6e_6b28, 0x3f80_0000,
];

const FAST: [f64; 3] = [
    f64::from_bits(0x3fdb_cb7b_15d3_5067),
    f64::from_bits(0xbfcb_cbb1_cd29_cbaf),
    f64::from_bits(0x3fc2_870e_2624_ce4e),
];

const ACCURATE: [f64; 7] = [
    f64::from_bits(0x3fdb_cb7b_1526_e50e),
    f64::from_bits(0xbfcb_cb7b_1526_e53d),
    f64::from_bits(0x3fc2_87a7_636f_3fa2),
    f64::from_bits(0xbfbb_cb7b_146a_14b3),
    f64::from_bits(0x3fb6_3c62_7d52_19cb),
    f64::from_bits(0xbfb2_8807_36c8_762d),
    f64::from_bits(0x3faf_c1ec_f913_961a),
];

const LOG10_2: f64 = f64::from_bits(0x3fd3_4413_509f_79ff);
const LOG10_2_HI: f64 = f64::from_bits(0x3fd3_4413_509f_7a00);
const LOG10_2_LO: f64 = f64::from_bits(0xbc90_cee0_ed4c_a7e9);
const FAST_ERROR: f64 = f64::from_bits(0x3dda_f23f_0000_0000);

/// Base-10 logarithm with the pinned binary32 rounding algorithm.
///
/// Returns -infinity for either zero, +infinity for +infinity, and NaN for
/// negative values and NaNs. The caller must keep the f32 result boundary before
/// subsequent dB multiplication.
#[inline]
pub(super) fn log10(value: f32) -> f32 {
    let mut bits = value.to_bits();
    if bits >= 0x7f80_0000 {
        return special(value);
    }

    // Exact powers of ten need no polynomial. This hash is upstream st/je.
    if bits == POWER_OF_TEN_BITS[((bits >> 24) & 0xf) as usize] {
        let exponent = (bits >> 23).wrapping_sub(126);
        return (exponent.wrapping_mul(0x04d1_04d4) >> 28) as f32;
    }

    if bits < 0x0080_0000 {
        if bits == 0 {
            return special(value);
        }
        let shift = bits.leading_zeros() - 8;
        // The subtraction deliberately wraps for negative binary exponents.
        bits = (bits << shift).wrapping_sub(shift << 23);
    }

    let exponent = ((bits as i32) >> 23) - 127;
    let mantissa = u64::from(bits & 0x007f_ffff);
    // For 23 mantissa bits this rounded index is always in 0..=64.
    let index = ((mantissa + (1 << 16)) >> 17) as usize;
    let reciprocal = f64::from_bits(RECIPROCAL_BITS[index]);
    let logarithm = f64::from_bits(LOGARITHM_BITS[index]);
    let normalized = f64::from_bits((mantissa << 29) | (0x3ff_u64 << 52));
    let exponent = f64::from(exponent);
    let z = normalized * reciprocal - 1.0;
    let z2 = z * z;
    let mut estimate = ((exponent * LOG10_2 + logarithm) + z * FAST[0])
        + z2 * (FAST[1] + z * FAST[2]);
    let mut rounded = estimate as f32;
    let other_bound = (estimate + FAST_ERROR) as f32;

    if rounded != other_bound {
        // Preserve the accurate polynomial and each subsequent addition from C.
        let mut correction = z
            * ((ACCURATE[0] + z * ACCURATE[1])
                + z2
                    * ((ACCURATE[2] + z * ACCURATE[3])
                        + z2 * (ACCURATE[4] + z * ACCURATE[5] + z2 * ACCURATE[6])));
        correction += LOG10_2_LO * exponent;
        correction += logarithm - f64::from_bits(LOGARITHM_BITS[0]);
        let high = exponent * LOG10_2_HI;
        estimate = high + correction;
        rounded = estimate as f32;

        if estimate.to_bits() & ((1_u64 << 28) - 1) == 0 {
            // A rounded binary64 midpoint needs the residual before f32 rounding.
            let residual = (high - estimate) + correction;
            estimate += residual * 32.0;
            rounded = estimate as f32;
        }
    }
    rounded
}

#[cold]
#[inline(never)]
fn special(value: f32) -> f32 {
    let bits = value.to_bits();
    let magnitude = bits << 1;
    if magnitude == 0 {
        return f32::NEG_INFINITY;
    }
    if bits == 0x7f80_0000 {
        return value;
    }
    if magnitude > 0xff00_0000 {
        return value + value;
    }
    f32::from_bits(0xffc0_0000)
}

#[cfg(test)]
mod tests {
    use super::log10;

    // Independently rounded Decimal(110) references retained in the CI diagnosis.
    // Includes all 21 LLVM MPFR log10f tricky inputs, subnormal/normal extremes,
    // neighbors of one, and the 0x0efeee7a double-rounding regression.
    // Input source: llvm-project/libc/test/src/math/log10f_test.cpp.
    // Snapshot SHA-256: 099d77be7e581db88cbbff3532b017ae6656111a302c96c38738d2e5d404a5d8.
    const ROUNDING_EDGES: &[(u32, u32)] = &[
        (0x0000_0001, 0xc233_69f4),
        (0x0000_0002, 0xc232_35b3),
        (0x007f_ffff, 0xc217_b818),
        (0x0080_0000, 0xc217_b818),
        (0x0080_0001, 0xc217_b818),
        (0x08ae_a356, 0xc203_e9da),
        (0x0efe_ee7a, 0xc1e9_9d23),
        (0x120b_93dc, 0xc1da_d957),
        (0x13ae_78d3, 0xc1d2_d957),
        (0x1c7d_a337, 0xc1a8_9be8),
        (0x3f5f_de1b, 0xbd6e_9637),
        (0x3f7f_ffff, 0xb2de_5bd9),
        (0x3f80_0000, 0x0000_0000),
        (0x3f80_0001, 0x335e_5bd8),
        (0x3f80_70d8, 0x3ac3_b162),
        (0x4120_0000, 0x3f80_0000),
        (0x42c8_0000, 0x4000_0000),
        (0x447a_0000, 0x4040_0000),
        (0x461c_4000, 0x4080_0000),
        (0x47c3_5000, 0x40a0_0000),
        (0x4974_2400, 0x40c0_0000),
        (0x4b18_9680, 0x40e0_0000),
        (0x4cbe_bc20, 0x4100_0000),
        (0x4e6e_6b28, 0x4110_0000),
        (0x4f13_4f83, 0x4116_498a),
        (0x5015_02f9, 0x4120_0000),
        (0x69c8_c583, 0x41cb_db29),
        (0x7956_ba5e, 0x420b_5f5d),
        (0x7f7f_ffff, 0x421a_209b),
    ];

    #[test]
    fn matches_independent_rounding_edges() {
        for &(input, expected) in ROUNDING_EDGES {
            assert_eq!(
                log10(f32::from_bits(input)).to_bits(),
                expected,
                "input {input:#010x}"
            );
        }
    }

    #[test]
    fn preserves_special_value_classes() {
        for zero in [0.0_f32, -0.0] {
            assert_eq!(log10(zero), f32::NEG_INFINITY);
        }
        assert_eq!(log10(f32::INFINITY), f32::INFINITY);
        for negative in [
            -f32::from_bits(1),
            -f32::MIN_POSITIVE,
            -1.0,
            -f32::MAX,
            f32::NEG_INFINITY,
        ] {
            assert!(log10(negative).is_nan(), "negative input {negative:?}");
        }
        for nan in [
            0x7fc0_0000,
            0xffc0_0000,
            0x7f80_0001,
            0xff80_0001,
            0x7fff_ffff,
        ] {
            assert!(
                log10(f32::from_bits(nan)).is_nan(),
                "NaN bits {nan:#010x}"
            );
        }
        assert_eq!(log10(1.0).to_bits(), 0);
    }

    #[test]
    fn is_finite_and_monotone_across_positive_strata() {
        let mut previous = f32::NEG_INFINITY;
        for step in 0..4096_u64 {
            let bits = 1 + ((u64::from(0x7f7f_ffff_u32 - 1) * step) / 4095) as u32;
            let current = log10(f32::from_bits(bits));
            assert!(
                current.is_finite() && current >= previous,
                "positive input {bits:#010x}"
            );
            previous = current;
        }
    }

    #[test]
    fn is_monotone_across_range_reduction_boundaries() {
        let check = |bits: u32| {
            let before = log10(f32::from_bits(bits - 1));
            let at = log10(f32::from_bits(bits));
            let after = log10(f32::from_bits(bits + 1));
            assert!(before.is_finite() && at.is_finite() && after.is_finite());
            assert!(before <= at && at <= after, "range boundary {bits:#010x}");
        };
        for exponent in 1..255_u32 {
            check(exponent << 23);
            for bin in 0..64_u32 {
                // Index rounding changes at half a 1/64 interval.
                check((exponent << 23) | ((bin << 17) + (1 << 16)));
            }
        }
    }
}
