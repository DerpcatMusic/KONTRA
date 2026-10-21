//! Checked value-only projection preparation from the clean 0.1.5 mathematical handoff.
//! The seed and scalar logarithm are declared external mappings. No FP control is written.
use crate::numerics::canonical_estimate_environment;
use crate::projection_log2_table::PACKED_LOG2_TABLE;

pub const ROWS: usize = 16;
pub const ANCHORS: [f32; ROWS] = [
    f32::from_bits(0x41a00000),
    f32::from_bits(0x4232399a),
    f32::from_bits(0x42a2586b),
    f32::from_bits(0x4307c5bb),
    f32::from_bits(0x43592ff7),
    f32::from_bits(0x43a94b2b),
    f32::from_bits(0x4401e705),
    f32::from_bits(0x4445625a),
    f32::from_bits(0x44950105),
    f32::from_bits(0x44e00679),
    f32::from_bits(0x4527f21a),
    f32::from_bits(0x457b5952),
    f32::from_bits(0x45bbdb55),
    f32::from_bits(0x460c4a07),
    f32::from_bits(0x46516b3e),
    f32::from_bits(0x469c4000),
];
pub const MINIMUM_RATE: f32 = f32::from_bits(0x472c4400);
const RATE_FACTOR: f32 = f32::from_bits(0x37be37c6);
const GRID_HALF: f32 = f32::from_bits(0x3f000000);
const FRACTION_MASK: u64 = 0x000fffffffffffff;
const ONE_WORD: u64 = 0x3ff0000000000000;
const ROUND_OR_WORD: u64 = 0x000003ff000003ff;
const ROUND_BIAS: f64 = f64::from_bits(0x42c0000000000000);
const A6: f64 = f64::from_bits(0xbfcec7316e9f33a2);
const A5: f64 = f64::from_bits(0x3fd277801a1a2762);
const A4: f64 = f64::from_bits(0xbfd715476521ed82);
const A3: f64 = f64::from_bits(0x3fdec709dc32988b);
const A2: f64 = f64::from_bits(0xbfe71547652b82ff);
const A1: f64 = f64::from_bits(0x3ff71547652b82fe);
const WIDTH_HALF: f64 = f64::from_bits(0x3fe0000000000000);
const SCALAR_ONE: f64 = f64::from_bits(0x3ff0000000000000);
const SIGMA_SCALE_1: f64 = f64::from_bits(0x3ff99999a0000000);
const SIGMA_SCALE_2: f64 = f64::from_bits(0x3fe3403c5b2e4bcb);
const SIGMA_SCALE_3: f64 = f64::from_bits(0x3fe6a09e667f3bcc);
const ROW_NEGATIVE_HALF: f64 = f64::from_bits(0xbfe0000000000000);

fn environment() -> Result<(), &'static str> {
    if canonical_estimate_environment() {
        Ok(())
    } else {
        Err(
            "projection preparation requires checked nearest-even x86_64 gradual-underflow masked-trap arithmetic",
        )
    }
}

/// Pure metadata admission; no allocation and no upstream host/base binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProjectionLayout {
    logical_bins: usize,
    padded_stride: usize,
}
impl ProjectionLayout {
    pub fn checked(logical_bins: usize) -> Result<Self, &'static str> {
        if !(2..=2_147_483_644).contains(&logical_bins) {
            return Err("projection logical count outside signed32 padding admission");
        }
        let padded_stride = logical_bins
            .checked_add(3)
            .and_then(|n| (n / 4).checked_mul(4))
            .ok_or("projection padding overflow")?;
        if padded_stride > i32::MAX as usize {
            return Err("projection padded count outside signed32 admission");
        }
        // Check every prospective matrix byte count before callers allocate anything.
        let bytes = padded_stride
            .checked_mul(ROWS)
            .and_then(|n| n.checked_mul(4))
            .ok_or("projection bank byte size overflow")?;
        if bytes > isize::MAX as usize {
            return Err("projection bank exceeds pointer size");
        }
        Ok(Self {
            logical_bins,
            padded_stride,
        })
    }
    pub fn logical_bins(self) -> usize {
        self.logical_bins
    }
    pub fn padded_stride(self) -> usize {
        self.padded_stride
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeclaredProjectionGeometry {
    rate: f32,
    rate_multiplier: i32,
    m: i32,
    transform_length: u64,
    layout: ProjectionLayout,
}
impl DeclaredProjectionGeometry {
    pub fn checked(incoming_rate: f32, base: i32) -> Result<Self, &'static str> {
        environment()?;
        if !incoming_rate.is_finite() || base <= 0 {
            return Err(
                "declared geometry requires finite incoming rate and positive signed32 base",
            );
        }
        let rate = incoming_rate.max(MINIMUM_RATE);
        let product = rate * RATE_FACTOR; // RN32 precedes truncation; no exact-real rate ratio.
        if !product.is_finite() || !(1.0..2_147_483_648.0).contains(&(product as f64)) {
            return Err("declared rate multiplier does not fit positive signed32");
        }
        let rate_multiplier = product as i32;
        let m64 = i64::from(base) * i64::from(rate_multiplier);
        if !(1..=2_147_483_643).contains(&m64) {
            return Err("declared positive size product exceeds signed32/padding admission");
        }
        // The admitted positive transform length is retained in u64, without signed32 wrapping.
        let transform_length = (m64 as u64)
            .checked_mul(2)
            .ok_or("transform size overflow")?;
        let bins = usize::try_from((transform_length >> 1) + 1).map_err(|_| "bin size overflow")?;
        let layout = ProjectionLayout::checked(bins)?;
        Ok(Self {
            rate,
            rate_multiplier,
            m: m64 as i32,
            transform_length,
            layout,
        })
    }
    pub fn rate(self) -> f32 {
        self.rate
    }
    pub fn rate_multiplier(self) -> i32 {
        self.rate_multiplier
    }
    pub fn m(self) -> i32 {
        self.m
    }
    pub fn transform_length(self) -> u64 {
        self.transform_length
    }
    pub fn layout(self) -> ProjectionLayout {
        self.layout
    }
}

/// Checked point evaluation also permits large-k rounding tests without huge buffers.
pub fn projection_coordinate(
    rate: f32,
    layout: ProjectionLayout,
    k: usize,
) -> Result<(f32, f32), &'static str> {
    environment()?;
    if !rate.is_finite() || rate < MINIMUM_RATE || k >= layout.logical_bins {
        return Err("coordinate rate/index outside admission");
    }
    let inverse_m = SCALAR_ONE / (layout.logical_bins - 1) as f64;
    let rounded_index = k as f32;
    let product = rounded_index as f64 * inverse_m;
    let normalized = product as f32;
    let half_rate = rate * GRID_HALF;
    let coordinate = normalized * half_rate;
    if !normalized.is_finite() || !coordinate.is_finite() {
        return Err("nonfinite projection coordinate");
    }
    Ok((normalized, coordinate))
}

fn decompose(x: f64) -> (usize, f64, f64, f64) {
    let u = x.to_bits();
    let exponent = ((u >> 52) & 2047) as i32 - 1023;
    let mantissa = f64::from_bits((u & FRACTION_MASK) | ONE_WORD);
    let index = (((((u >> 42) & 1020) + 4) & 2040) >> 3) as usize;
    let forced = f64::from_bits(mantissa.to_bits() | ROUND_OR_WORD);
    let biased = forced + ROUND_BIAS;
    let center = biased - ROUND_BIAS;
    (index, mantissa, center, exponent as f64)
}

fn packed_log2_lane(x: f64) -> f64 {
    let (j, m, c, exponent) = decompose(x);
    let [high, low, reciprocal] = PACKED_LOG2_TABLE[j].map(f64::from_bits);
    let difference = m - c;
    let z = difference * reciprocal;
    let h = exponent + high;
    let product = z * A6;
    let mut v = product + A5;
    let product = v * z;
    v = product + A4;
    let product = v * z;
    v = product + A3;
    let product = v * z;
    v = product + A2;
    v *= z;
    v *= z;
    v += low;
    let linear = z * A1;
    v += linear;
    v + h
}

/// Both lanes are admitted before evaluation. No ideal log2 or seed occurs in this program.
pub fn packed_log2_normal_pair(x: [f64; 2]) -> Result<[f64; 2], &'static str> {
    environment()?;
    if x.iter().any(|v| !v.is_normal() || *v <= 0.0) {
        return Err("packed log2 requires two positive normal finite binary64 values");
    }
    Ok(x.map(packed_log2_lane))
}

/// These function mappings may affect last bits; labels are provenance, not equivalence proofs.
#[derive(Clone, Copy, Debug)]
pub struct ProjectionMappings {
    pub approximate_reciprocal32: fn(f32) -> f32,
    pub scalar_library_log2_64: fn(f64) -> f64,
    pub reciprocal_mapping: &'static str,
    pub scalar_log2_mapping: &'static str,
}
fn current_cpu_seed(x: f32) -> f32 {
    #[cfg(target_arch = "x86_64")]
    {
        crate::numerics::reciprocal_seed(x)
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = x;
        f32::NAN
    }
}
impl ProjectionMappings {
    pub fn current_cpu_and_rust_library() -> Self {
        Self {
            approximate_reciprocal32: current_cpu_seed,
            scalar_library_log2_64: f64::log2,
            reciprocal_mapping: "current x86_64 packed SSE RCPPS; upstream CPU/dispatch unproved",
            scalar_log2_mapping: "Rust f64::log2 current std/runtime; upstream CRT last bits unproved",
        }
    }
}

fn average_spacing(mappings: ProjectionMappings) -> Result<f64, &'static str> {
    environment()?;
    let ratio = ANCHORS[15] as f64 / ANCHORS[0] as f64;
    let logarithm = (mappings.scalar_library_log2_64)(ratio);
    environment()?; // External mappings must preserve the admitted FP mode.
    let q = logarithm / 16.0_f64;
    if !q.is_finite() || !(0.5..=1.0).contains(&q) {
        return Err("scalar log2 mapping produced out-of-admission average spacing");
    }
    Ok(q)
}

pub fn projection_row_coefficients(widths: [f32; ROWS]) -> Result<[f32; ROWS], &'static str> {
    environment()?;
    let mut coefficients = [0.0; ROWS];
    for (width, coefficient) in widths.into_iter().zip(&mut coefficients) {
        let sigma1 = width as f64 * SIGMA_SCALE_1;
        let sigma2 = sigma1 * SIGMA_SCALE_2;
        let sigma = sigma2 * SIGMA_SCALE_3;
        let squared = sigma * sigma;
        if !sigma.is_finite() || sigma == 0.0 || !squared.is_finite() || squared == 0.0 {
            return Err("row sigma/square outside finite nonzero admission");
        }
        let divided = ROW_NEGATIVE_HALF / squared;
        *coefficient = divided as f32;
        if !coefficient.is_finite() || *coefficient == 0.0 {
            return Err("row coefficient outside finite nonzero preparation admission");
        }
    }
    Ok(coefficients)
}

#[derive(Clone, Copy, Debug)]
pub struct ProjectionWidths {
    pub q: f64,
    pub widths: [f32; ROWS],
    pub row_coefficients: [f32; ROWS],
    pub mappings: ProjectionMappings,
}
impl ProjectionWidths {
    pub fn packed(mappings: ProjectionMappings) -> Result<Self, &'static str> {
        let q = average_spacing(mappings)?;
        let mut widths = [0.0_f32; ROWS];
        for first in (1..15).step_by(2) {
            let mut inputs = [0.0_f64; 2];
            for (lane, input) in inputs.iter_mut().enumerate() {
                let k = first + lane;
                let denominator = ANCHORS[k + 1];
                let r = (mappings.approximate_reciprocal32)(denominator);
                environment()?;
                let seed_product = denominator as f64 * r as f64;
                let seed_error = seed_product - 1.0;
                if !r.is_finite() || r <= 0.0 || seed_error.abs() > 0.0009765625 {
                    return Err("projection opaque reciprocal seed violates relative 2^-10 bound");
                }
                // Local square-first refinement, deliberately distinct from standalone Recip32.
                let squared = r * r;
                let twice = r + r;
                let correction = squared * denominator;
                let inverse = twice - correction;
                let x = inverse * ANCHORS[k - 1];
                if !x.is_normal() || !(0.24..=0.45).contains(&x) {
                    return Err(
                        "projection rounded ratio outside positive-normal fixed-anchor admission",
                    );
                }
                *input = x as f64;
            }
            let logarithms = packed_log2_normal_pair(inputs)?;
            for lane in 0..2 {
                let half = logarithms[lane] * WIDTH_HALF;
                let divided = half / q;
                widths[first + lane] = divided as f32;
            }
        }
        widths[0] = f32::from_bits(widths[1].to_bits());
        widths[15] = f32::from_bits(widths[14].to_bits());
        let row_coefficients = projection_row_coefficients(widths)?;
        Ok(Self {
            q,
            widths,
            row_coefficients,
            mappings,
        })
    }

    /// Separate conditional scalar program, never an implicit packed-path substitute.
    pub fn scalar_alternative(mappings: ProjectionMappings) -> Result<Self, &'static str> {
        let q = average_spacing(mappings)?;
        let inverse_q = SCALAR_ONE / q;
        let mut widths = [0.0_f32; ROWS];
        for k in 1..15 {
            let x = ANCHORS[k - 1] / ANCHORS[k + 1];
            let logarithm = (mappings.scalar_library_log2_64)(x as f64);
            environment()?;
            if !logarithm.is_finite() {
                return Err("invalid scalar width logarithm mapping");
            }
            let half = logarithm * WIDTH_HALF;
            let scaled = half * inverse_q;
            widths[k] = scaled as f32;
        }
        widths[0] = f32::from_bits(widths[1].to_bits());
        widths[15] = f32::from_bits(widths[14].to_bits());
        let row_coefficients = projection_row_coefficients(widths)?;
        Ok(Self {
            q,
            widths,
            row_coefficients,
            mappings,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    include!("fixtures/projection/staged-fixtures.rs");

    fn fixed_scalar_log2(x: f64) -> f64 {
        assert_eq!(x, 1000.0);
        f64::from_bits(FIXED_SCALAR_LOG2_1000_WORD)
    }
    fn fixture_seed(d: f32) -> f32 {
        let k = ANCHORS
            .iter()
            .position(|x| x.to_bits() == d.to_bits())
            .unwrap();
        f32::from_bits(FIXTURE_SEEDS[k - 2])
    }
    fn fixture_mappings() -> ProjectionMappings {
        ProjectionMappings {
            approximate_reciprocal32: fixture_seed,
            scalar_library_log2_64: fixed_scalar_log2,
            reciprocal_mapping: "explicit bounded mathematical seed words",
            scalar_log2_mapping: "fixed mathematical binary64 log2(1000) word",
        }
    }

    #[test]
    fn packed_normal_pairs_match_independent_staged_fixtures() {
        for pair in LOG2_CASES.chunks_exact(2) {
            let input = [f64::from_bits(pair[0].0), f64::from_bits(pair[1].0)];
            let expected = [pair[0].1, pair[1].1];
            assert_eq!(
                packed_log2_normal_pair(input).unwrap().map(f64::to_bits),
                expected
            );
        }
        for (input, exponent) in [(1.0, 0.0), (0.5, -1.0), (2.0, 1.0), (1024.0, 10.0)] {
            assert_eq!(
                packed_log2_normal_pair([input, input]).unwrap(),
                [exponent; 2]
            );
        }
    }

    #[test]
    fn packed_pair_and_row_coefficient_domains_reject_invalid_values() {
        for invalid in [
            0.0,
            -0.0,
            -1.0,
            f64::INFINITY,
            f64::NAN,
            f64::from_bits(1),
            f64::from_bits(0x000fffffffffffff),
        ] {
            assert!(packed_log2_normal_pair([1.0, invalid]).is_err());
            assert!(packed_log2_normal_pair([invalid, 1.0]).is_err());
        }
        for invalid in [0.0, f32::INFINITY, f32::NAN, f32::from_bits(1)] {
            let mut widths = [-1.0; ROWS];
            widths[7] = invalid;
            assert!(projection_row_coefficients(widths).is_err());
        }
    }

    #[test]
    fn forced_rounding_grid_and_upper_half_ties_cover_all_table_indices() {
        environment().unwrap();
        for j in 0..=128 {
            let grid = 1.0_f64 + j as f64 / 128.0;
            if j < 128 {
                let (index, _, center, _) = decompose(grid);
                assert_eq!(index, j);
                assert_eq!(center, grid);
                let tie_word = ONE_WORD + ((2 * j as u64 + 1) << 44);
                for (word, expected_index) in
                    [(tie_word - 1, j), (tie_word, j + 1), (tie_word + 1, j + 1)]
                {
                    let (index, _, center, _) = decompose(f64::from_bits(word));
                    assert_eq!(index, expected_index);
                    assert_eq!(center, 1.0 + expected_index as f64 / 128.0);
                }
            }
            let [high, low, reciprocal] = PACKED_LOG2_TABLE[j].map(f64::from_bits);
            assert!(high.is_finite() && low.is_finite() && reciprocal.is_normal());
            assert_eq!((high * 549755813888.0).fract(), 0.0);
        }
    }

    #[test]
    fn packed_widths_and_double_coefficients_match_explicit_mapping_fixtures() {
        let prepared = ProjectionWidths::packed(fixture_mappings()).unwrap();
        assert_eq!(prepared.widths.map(f32::to_bits), FIXTURE_WIDTHS);
        assert_eq!(
            prepared.row_coefficients.map(f32::to_bits),
            FIXTURE_COEFFICIENTS
        );
        assert!(prepared.widths.iter().all(|x| *x < 0.0));
        assert_eq!(prepared.widths[0].to_bits(), prepared.widths[1].to_bits());
        assert_eq!(prepared.widths[15].to_bits(), prepared.widths[14].to_bits());
        let mut distinct = 0;
        for k in 1..15 {
            let d = ANCHORS[k + 1];
            let r = fixture_seed(d);
            let square = r * r;
            let twice = r + r;
            let correction = square * d;
            let inverse = twice - correction;
            let x = inverse * ANCHORS[k - 1];
            assert_eq!(x.to_bits(), FIXTURE_RATIOS[k - 1]);
            let standalone_product = r * d;
            let standalone_correction = standalone_product * r;
            distinct += usize::from(inverse.to_bits() != (twice - standalone_correction).to_bits());
        }
        assert_eq!(
            distinct, 6,
            "fixture must distinguish the two refinement associations"
        );
    }

    #[test]
    fn opaque_seed_and_scalar_library_mappings_have_checked_admission() {
        let invalid_scalars: [fn(f64) -> f64; 3] = [|_| f64::NAN, |_| 0.0, |_| 17.0];
        for scalar in invalid_scalars {
            let mut mappings = fixture_mappings();
            mappings.scalar_library_log2_64 = scalar;
            assert!(ProjectionWidths::packed(mappings).is_err());
        }
        let invalid_seeds: [fn(f32) -> f32; 4] = [|_| f32::NAN, |_| 0.0, |_| -1.0, |_| 1.0];
        for seed in invalid_seeds {
            let mut mappings = fixture_mappings();
            mappings.approximate_reciprocal32 = seed;
            assert!(ProjectionWidths::packed(mappings).is_err());
        }
    }

    #[test]
    fn geometry_conversion_padding_and_size_bounds_need_no_large_allocation() {
        for (rate, multiplier) in [
            (1.0, 1),
            (-1.0, 1),
            (44100.0, 1),
            (48000.0, 1),
            (88200.0, 2),
            (174400.0, 3),
            (192000.0, 4),
        ] {
            let g = DeclaredProjectionGeometry::checked(rate, 2048).unwrap();
            assert_eq!(g.rate_multiplier(), multiplier);
            assert_eq!(g.m(), multiplier * 2048);
            assert_eq!(g.transform_length(), g.m() as u64 * 2);
            assert_eq!(g.layout().logical_bins(), g.m() as usize + 1);
            assert_eq!(
                g.layout().padded_stride(),
                4 * g.layout().logical_bins().div_ceil(4)
            );
        }
        let maximum = DeclaredProjectionGeometry::checked(44100.0, 2_147_483_643).unwrap();
        assert_eq!(maximum.transform_length(), 4_294_967_286);
        assert_eq!(maximum.layout().padded_stride(), 2_147_483_644);
        for (rate, base) in [
            (44100.0, 0),
            (44100.0, -1),
            (44100.0, i32::MAX),
            (192000.0, i32::MAX),
            (f32::MAX, 1),
            (f32::NAN, 1),
            (f32::INFINITY, 1),
        ] {
            assert!(DeclaredProjectionGeometry::checked(rate, base).is_err());
        }
        for bins in [0, 1, 2_147_483_645, usize::MAX] {
            assert!(ProjectionLayout::checked(bins).is_err());
        }
        for bins in 2..10 {
            let l = ProjectionLayout::checked(bins).unwrap();
            assert_eq!(l.padded_stride(), bins.div_ceil(4) * 4);
        }
    }

    #[test]
    fn coordinate_integer_rounding_and_separate_narrowing_match_math_fixtures() {
        for &(rate, bins, k, unit, coordinate) in COORDINATE_CASES {
            let actual = projection_coordinate(
                f32::from_bits(rate),
                ProjectionLayout::checked(bins).unwrap(),
                k,
            )
            .unwrap();
            assert_eq!((actual.0.to_bits(), actual.1.to_bits()), (unit, coordinate));
        }
        let l = ProjectionLayout::checked(2049).unwrap();
        assert_eq!(projection_coordinate(44100.0, l, 0).unwrap(), (0.0, 0.0));
        assert_eq!(
            projection_coordinate(44100.0, l, 2048).unwrap(),
            (1.0, 22050.0)
        );
        assert!(projection_coordinate(44100.0, l, 2049).is_err());
        assert!(projection_coordinate(44099.0, l, 0).is_err());
        assert!(projection_coordinate(f32::NAN, l, 0).is_err());
    }

    #[test]
    fn scalar_alternative_remains_a_distinct_conditional_program() {
        let mappings = ProjectionMappings {
            scalar_library_log2_64: f64::log2,
            ..fixture_mappings()
        };
        let packed = ProjectionWidths::packed(mappings).unwrap();
        let scalar = ProjectionWidths::scalar_alternative(mappings).unwrap();
        assert!(
            packed
                .widths
                .iter()
                .zip(scalar.widths)
                .any(|(a, b)| a.to_bits() != b.to_bits())
        );
        assert!(scalar.widths.iter().all(|x| x.is_finite() && *x < 0.0));
    }

    #[test]
    fn current_cpu_mapping_is_bounded_and_preserves_fp_control() {
        environment().unwrap();
        #[cfg(target_arch = "x86_64")]
        {
            #[allow(deprecated, reason = "reads MXCSR to check the FP control is preserved")]
            let before = unsafe { std::arch::x86_64::_mm_getcsr() } & !0x3f;
            let prepared =
                ProjectionWidths::packed(ProjectionMappings::current_cpu_and_rust_library())
                    .unwrap();
            assert!(prepared.widths.iter().all(|x| x.is_normal() && *x < 0.0));
            #[allow(deprecated, reason = "reads MXCSR to check the FP control is preserved")]
            let after = unsafe { std::arch::x86_64::_mm_getcsr() } & !0x3f;
            assert_eq!(before, after); // Value-only contract excludes sticky exception flags.
        }
    }
}
