//! Original projection frontend with checked packed width preparation and owned aligned rows.
//! Host/base geometry, input scratch producer, FFT/window and runtime dispatch remain unclosed.
use crate::numerics::div32;
pub use crate::numerics::{exp32, ln32};
pub use crate::projection_math::{ANCHORS, ROWS};
use crate::projection_math::{
    DeclaredProjectionGeometry, ProjectionLayout, ProjectionMappings, ProjectionWidths,
    projection_coordinate,
};
use crate::reduction::{accurate_sum32, dot32, phase32};
use std::alloc::{Layout, alloc_zeroed, dealloc};
use std::ptr::NonNull;

/// Independent allocation policy, not a limit on the mathematical geometry metadata.
pub const MAX_OWNED_PROJECTION_BINS: usize = 65_537;

/// Fully initialized, uniquely owned allocation. Each row has its own 64-byte aligned base;
/// this keeps the required padded stride 4*ceil(B/4), even when that stride is not 64-byte sized.
pub(crate) struct AlignedF32 {
    pointer: NonNull<f32>,
    length: usize,
    layout: Layout,
}
// Ownership moves with the allocation; shared access is immutable and mutation requires &mut.
unsafe impl Send for AlignedF32 {}
unsafe impl Sync for AlignedF32 {}
impl AlignedF32 {
    pub(crate) fn zeroed(length: usize) -> Result<Self, &'static str> {
        let bytes = length
            .checked_mul(std::mem::size_of::<f32>())
            .ok_or("aligned size overflow")?;
        if bytes == 0 {
            return Err("zero aligned allocation");
        }
        let layout = Layout::from_size_align(bytes, 64).map_err(|_| "invalid aligned size")?;
        let pointer = NonNull::new(unsafe { alloc_zeroed(layout) }.cast::<f32>())
            .ok_or("aligned projection allocation failed")?;
        Ok(Self {
            pointer,
            length,
            layout,
        })
    }
    pub(crate) fn values(&self) -> &[f32] {
        unsafe { std::slice::from_raw_parts(self.pointer.as_ptr(), self.length) }
    }
    pub(crate) fn values_mut(&mut self) -> &mut [f32] {
        unsafe { std::slice::from_raw_parts_mut(self.pointer.as_ptr(), self.length) }
    }
}
impl Drop for AlignedF32 {
    fn drop(&mut self) {
        unsafe { dealloc(self.pointer.as_ptr().cast(), self.layout) };
    }
}

struct AlignedBank {
    rows: [AlignedF32; ROWS],
    layout: ProjectionLayout,
}
impl AlignedBank {
    fn zeroed(layout: ProjectionLayout) -> Result<Self, &'static str> {
        let mut rows = Vec::new();
        rows.try_reserve_exact(ROWS)
            .map_err(|_| "row metadata allocation failed")?;
        for _ in 0..ROWS {
            rows.push(AlignedF32::zeroed(layout.padded_stride())?);
        }
        let rows = rows.try_into().map_err(|_| "invalid fixed row count")?;
        Ok(Self { rows, layout })
    }
    fn row(&self, i: usize) -> &[f32] {
        &self.rows[i].values()[..self.layout.logical_bins()]
    }
    fn row_mut(&mut self, i: usize) -> &mut [f32] {
        &mut self.rows[i].values_mut()[..self.layout.logical_bins()]
    }
}

pub struct ProjectionCoordinates {
    normalized: AlignedF32,
    coordinates: AlignedF32,
    layout: ProjectionLayout,
}
impl ProjectionCoordinates {
    pub fn checked(rate: f32, layout: ProjectionLayout) -> Result<Self, &'static str> {
        if layout.logical_bins() > MAX_OWNED_PROJECTION_BINS {
            return Err("projection exceeds independent owned preparation allocation cap");
        }
        projection_coordinate(rate, layout, 0)?; // All rate/FP/size admission before allocation.
        let mut normalized = AlignedF32::zeroed(layout.padded_stride())?;
        let mut coordinates = AlignedF32::zeroed(layout.padded_stride())?;
        for k in 0..layout.logical_bins() {
            let (unit, value) = projection_coordinate(rate, layout, k)?;
            normalized.values_mut()[k] = unit;
            coordinates.values_mut()[k] = value;
        }
        Ok(Self {
            normalized,
            coordinates,
            layout,
        })
    }
    pub fn normalized(&self) -> &[f32] {
        &self.normalized.values()[..self.layout.logical_bins()]
    }
    pub fn coordinates(&self) -> &[f32] {
        &self.coordinates.values()[..self.layout.logical_bins()]
    }
}

const EPSILON: f32 = 1.1920928955078125e-7;
const SUPPORT_THRESHOLD: f32 = 0.001;
const RECONSTRUCTION_FLOOR: f32 = 1.0000000116860974e-7;

/// Shared staged owner-rate grid, prepared outside the audio callback.
pub fn frequency_grid(owner_rate: f32, bins: usize) -> Result<Vec<f32>, &'static str> {
    let layout = ProjectionLayout::checked(bins)?;
    let coordinates = ProjectionCoordinates::checked(owner_rate, layout)?;
    let mut grid = Vec::new();
    grid.try_reserve_exact(bins)
        .map_err(|_| "frequency grid allocation failed")?;
    grid.extend_from_slice(coordinates.coordinates());
    Ok(grid)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Support {
    pub start: usize,
    pub end: usize,
}
fn first_support(row: &[f32]) -> Option<Support> {
    let start = row.iter().position(|&value| value >= SUPPORT_THRESHOLD)?;
    let length = row[start..]
        .iter()
        .position(|&value| value < SUPPORT_THRESHOLD)
        .unwrap_or(row.len() - start);
    Some(Support {
        start,
        end: start + length,
    })
}

pub struct Projection {
    layout: ProjectionLayout,
    raw: AlignedBank,
    analysis: AlignedBank,
    reconstruction: AlignedBank,
    coordinates: ProjectionCoordinates,
    preparation: ProjectionWidths,
    scratch_phase: usize,
    denominators: [f32; ROWS],
    supports: [Support; ROWS],
}
impl Projection {
    /// Original engine supplies rate and B independently of the declared base geometry builder.
    /// Phase zero is our separately aligned absolute-value scratch choice, not a native arena claim.
    pub fn new(owner_rate: f32, bins: usize) -> Result<Self, &'static str> {
        Self::with_mappings_and_scratch_phase(
            owner_rate,
            bins,
            ProjectionMappings::current_cpu_and_rust_library(),
            0,
        )
    }
    pub fn with_mappings_and_scratch_phase(
        owner_rate: f32,
        bins: usize,
        mappings: ProjectionMappings,
        scratch_phase: usize,
    ) -> Result<Self, &'static str> {
        Self::prepare(
            owner_rate,
            ProjectionLayout::checked(bins)?,
            mappings,
            scratch_phase,
        )
    }
    pub fn from_declared_geometry(
        geometry: DeclaredProjectionGeometry,
        mappings: ProjectionMappings,
        scratch_phase: usize,
    ) -> Result<Self, &'static str> {
        Self::prepare(geometry.rate(), geometry.layout(), mappings, scratch_phase)
    }
    fn prepare(
        owner_rate: f32,
        layout: ProjectionLayout,
        mappings: ProjectionMappings,
        scratch_phase: usize,
    ) -> Result<Self, &'static str> {
        let bins = layout.logical_bins();
        if bins > MAX_OWNED_PROJECTION_BINS || scratch_phase > 3 {
            return Err("projection allocation cap or scratch phase admission failed");
        }
        let scratch_length = bins
            .checked_add(scratch_phase)
            .ok_or("scratch size overflow")?;
        // All fixed widths/coefficients and coordinate rate admission precede allocation.
        projection_coordinate(owner_rate, layout, 0)?;
        let preparation = ProjectionWidths::packed(mappings)?;
        let coordinates = ProjectionCoordinates::checked(owner_rate, layout)?;
        let mut raw = AlignedBank::zeroed(layout)?;
        let mut analysis = AlignedBank::zeroed(layout)?;
        let mut reconstruction = AlignedBank::zeroed(layout)?;
        let mut scratch_storage = AlignedF32::zeroed(scratch_length)?;
        let scratch = &mut scratch_storage.values_mut()[scratch_phase..];
        if phase32(scratch) != scratch_phase {
            return Err("absolute scratch alignment admission failed");
        }
        let log_gain = (10.0_f64.powf(0.30000000000000004).ln()) as f32;
        let grid = coordinates.coordinates();
        for i in 0..ROWS {
            let kappa = preparation.row_coefficients[i];
            let inverse_anchor = (1.0 / (ANCHORS[i] as f64).max(EPSILON as f64)) as f32;
            for k in 0..bins {
                let shifted = grid[k] + EPSILON;
                let scaled = shifted * inverse_anchor;
                let logarithm = ln32(scaled);
                let log2 = logarithm * 1.4426950216293335_f32;
                let square = log2 * log2;
                let exponent = square * kappa;
                let inner = exp32(exponent);
                let shaped = inner * log_gain;
                raw.row_mut(i)[k] = exp32(shaped) + -1.0_f32;
            }
        }
        let mut peak = 0.0_f32;
        for value in raw.row_mut(0).iter_mut().rev() {
            peak = peak.max(*value);
            *value = peak;
        }
        peak = 0.0;
        for value in raw.row_mut(15) {
            peak = peak.max(*value);
            *value = peak;
        }
        let fallback_threshold = 10.0_f32.powf(-2.8499999046325684_f32);
        for i in 1..15 {
            let mut maximum = raw.row(i).iter().copied().fold(0.0_f32, f32::max);
            if maximum < fallback_threshold {
                for k in 0..bins {
                    raw.row_mut(i)[k] = raw.row(i - 1)[k].max(raw.row(i + 1)[k]);
                }
                maximum = raw.row(i).iter().copied().fold(0.0_f32, f32::max);
            }
            if !maximum.is_finite() || maximum <= 0.0 {
                return Err("nonfinite or zero projection row normalization");
            }
            let inverse = 1.0_f32 / maximum;
            for value in raw.row_mut(i) {
                *value *= inverse;
            }
        }
        if (0..ROWS)
            .flat_map(|i| raw.row(i))
            .any(|value| !value.is_finite() || *value < 0.0)
        {
            return Err("nonfinite or negative projection geometry");
        }
        let mut supports = [Support { start: 0, end: 0 }; ROWS];
        let mut denominators = [0.0_f32; ROWS];
        for i in 0..ROWS {
            let row = raw.row(i);
            supports[i] =
                first_support(row).ok_or("projection row has no finite supported interval")?;
            // AccurateSum32 consumes separate absolute-value scratch, with independent phase.
            for (absolute, value) in scratch.iter_mut().zip(row) {
                *absolute = value.abs();
            }
            let denominator = accurate_sum32(scratch, scratch_phase)?;
            if !denominator.is_finite() || denominator <= 0.0 {
                return Err("nonfinite or zero analysis denominator");
            }
            denominators[i] = denominator;
            let inverse = 1.0_f32 / denominator;
            for k in 0..bins {
                analysis.row_mut(i)[k] = (raw.row(i)[k] * inverse).max(0.0);
            }
        }
        // Reuse initialized scratch only after analysis sums; padding is never consumed.
        let sums = scratch;
        sums.fill(0.0);
        for i in 0..ROWS {
            for k in supports[i].start..supports[i].end {
                sums[k] = raw.row(i)[k] + sums[k];
            }
        }
        for i in 0..ROWS {
            for k in 0..bins {
                reconstruction.row_mut(i)[k] =
                    div32(raw.row(i)[k], sums[k].max(RECONSTRUCTION_FLOOR)).max(0.0);
            }
        }
        if (0..ROWS)
            .flat_map(|i| analysis.row(i).iter().chain(reconstruction.row(i)))
            .any(|value| !value.is_finite())
        {
            return Err("nonfinite projection matrix");
        }
        Ok(Self {
            layout,
            coordinates,
            preparation,
            scratch_phase,
            raw,
            analysis,
            reconstruction,
            denominators,
            supports,
        })
    }
    pub fn layout(&self) -> ProjectionLayout {
        self.layout
    }
    pub fn width_preparation(&self) -> &ProjectionWidths {
        &self.preparation
    }
    pub fn coordinate_preparation(&self) -> &ProjectionCoordinates {
        &self.coordinates
    }
    pub fn absolute_scratch_phase(&self) -> usize {
        self.scratch_phase
    }
    pub fn supports(&self) -> &[Support; ROWS] {
        &self.supports
    }
    pub fn analysis_denominators(&self) -> &[f32; ROWS] {
        &self.denominators
    }
    pub fn raw_row(&self, row: usize) -> &[f32] {
        self.raw.row(row)
    }
    pub fn analyze(&self, input: &[f32]) -> [f32; ROWS] {
        assert!(input.len() >= self.layout.logical_bins());
        let mut coefficients = [0.0_f32; ROWS];
        for i in 0..ROWS {
            let support = self.supports[i];
            let row = &self.analysis.row(i)[support.start..support.end];
            debug_assert_eq!(phase32(row), support.start % 4);
            if let Ok(value) = dot32(&input[support.start..support.end], row, support.start % 4) {
                coefficients[i] = value;
            } // Empty/error calls preserve the caller-initialized zero, not a successful dot.
        }
        coefficients
    }
    pub fn reconstruct(&self, coefficients: &[f32; ROWS], output: &mut [f32]) {
        assert!(output.len() >= self.layout.logical_bins());
        output[..self.layout.logical_bins()].fill(0.0);
        // Increasing row order, separate product and f32 addition; no mul_add.
        for i in 0..ROWS {
            for k in self.supports[i].start..self.supports[i].end {
                let product = self.reconstruction.row(i)[k] * coefficients[i];
                output[k] = product + output[k];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owned_bank_alignment_padding_and_independent_scratch_phases() {
        for bins in 2049..=2052 {
            let p = Projection::new(44100.0, bins).unwrap();
            assert_eq!(p.layout().padded_stride(), bins.div_ceil(4) * 4);
            for bank in [&p.raw, &p.analysis, &p.reconstruction] {
                for i in 0..ROWS {
                    let storage = bank.rows[i].values();
                    assert_eq!(storage.as_ptr() as usize % 64, 0);
                    assert_eq!(storage.len(), p.layout().padded_stride());
                    assert_eq!(bank.row(i).len(), bins);
                    assert!(storage[bins..].iter().all(|v| v.to_bits() == 0));
                }
            }
        }
        for phase in 0..4 {
            let p = Projection::with_mappings_and_scratch_phase(
                44100.0,
                2051,
                ProjectionMappings::current_cpu_and_rust_library(),
                phase,
            )
            .unwrap();
            let mut storage = AlignedF32::zeroed(2051 + phase).unwrap();
            let scratch = &mut storage.values_mut()[phase..];
            assert_eq!(phase32(scratch), phase);
            assert_eq!(p.absolute_scratch_phase(), phase);
            for i in 0..ROWS {
                assert_eq!(phase32(p.raw.row(i)), 0);
                for (x, y) in scratch.iter_mut().zip(p.raw.row(i)) {
                    *x = y.abs();
                }
                assert_eq!(
                    accurate_sum32(scratch, phase).unwrap().to_bits(),
                    p.denominators[i].to_bits()
                );
                let support = p.supports[i];
                let supported = &p.analysis.row(i)[support.start..support.end];
                assert_eq!(phase32(supported), support.start % 4);
            }
        }
    }

    #[test]
    fn matrix_dot_operand_padding_and_logical_reconstruction_contracts() {
        let mut p = Projection::new(48000.0, 2051).unwrap();
        let input: Vec<f32> = (0..2051).map(|k| (k % 17) as f32 * 0.125).collect();
        let before = p.analyze(&input);
        for i in 0..ROWS {
            let support = p.supports[i];
            let second = &p.analysis.row(i)[support.start..support.end];
            let expected = dot32(
                &input[support.start..support.end],
                second,
                support.start % 4,
            )
            .unwrap();
            assert_eq!(before[i].to_bits(), expected.to_bits());
            // Padding is our initialized storage policy only; consumers must ignore poisoned padding.
            for bank in [&mut p.raw, &mut p.analysis, &mut p.reconstruction] {
                bank.rows[i].values_mut()[2051..].fill(f32::NAN);
            }
        }
        assert_eq!(
            p.analyze(&input).map(f32::to_bits),
            before.map(f32::to_bits)
        );
        let coefficients = std::array::from_fn(|i| i as f32 * -0.0625);
        let mut output = vec![123.0; 2055];
        p.reconstruct(&coefficients, &mut output);
        assert!(output[2051..].iter().all(|x| *x == 123.0));
        let mut expected = vec![0.0_f32; 2051];
        for i in 0..ROWS {
            for k in p.supports[i].start..p.supports[i].end {
                let product = p.reconstruction.row(i)[k] * coefficients[i];
                expected[k] = product + expected[k];
            }
        }
        assert_eq!(
            output[..2051]
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>(),
            expected.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn declared_geometry_frontend_and_original_adapter_keep_allocation_bounds() {
        let mappings = ProjectionMappings::current_cpu_and_rust_library();
        let g = DeclaredProjectionGeometry::checked(48000.0, 2048).unwrap();
        let declared = Projection::from_declared_geometry(g, mappings, 0).unwrap();
        let adapter = Projection::new(48000.0, 2049).unwrap();
        assert_eq!(declared.layout(), g.layout());
        assert_eq!(
            declared.width_preparation().widths.map(f32::to_bits),
            adapter.width_preparation().widths.map(f32::to_bits)
        );
        for i in 0..ROWS {
            assert_eq!(declared.raw_row(i), adapter.raw_row(i));
        }
        let coordinate_bank = declared.coordinate_preparation();
        assert_eq!(coordinate_bank.normalized()[2048], 1.0);
        assert_eq!(coordinate_bank.coordinates()[2048], 24000.0);
        let huge = DeclaredProjectionGeometry::checked(44100.0, 2_147_483_643).unwrap();
        assert!(Projection::from_declared_geometry(huge, mappings, 0).is_err());
        assert!(Projection::new(44100.0, MAX_OWNED_PROJECTION_BINS + 1).is_err());
        assert!(Projection::new(44100.0, usize::MAX).is_err());
        assert!(Projection::with_mappings_and_scratch_phase(44100.0, 2049, mappings, 4).is_err());
    }
    #[test]
    fn stored_anchors_and_first_contiguous_support_contract() {
        assert_eq!(
            ANCHORS.map(f32::to_bits),
            [
                0x41a00000, 0x4232399a, 0x42a2586b, 0x4307c5bb, 0x43592ff7, 0x43a94b2b, 0x4401e705,
                0x4445625a, 0x44950105, 0x44e00679, 0x4527f21a, 0x457b5952, 0x45bbdb55, 0x460c4a07,
                0x46516b3e, 0x469c4000
            ]
        );
        assert_eq!(
            first_support(&[0.0, 0.002, 0.003, 0.0001, 0.006]),
            Some(Support { start: 1, end: 3 })
        );
        assert!(first_support(&[0.0; 3]).is_none());
    }
    #[test]
    fn endpoint_tails_and_full_row_denominator_preserve_trimmed_mass_loss() {
        let p = Projection::new(44100.0, 2049).unwrap();
        assert!(p.raw_row(0).windows(2).all(|w| w[0] >= w[1]));
        assert!(p.raw_row(15).windows(2).all(|w| w[0] <= w[1]));
        assert!(p.raw_row(0)[0] > 0.0 && p.raw_row(15)[2048] > 0.9);
        for i in 1..15 {
            assert!((p.raw_row(i).iter().copied().fold(0.0_f32, f32::max) - 1.0).abs() < 2.0e-6);
        }
        let constant = p.analyze(&vec![1.0; 2049]);
        let mut observed_omitted_tails = false;
        for i in 0..ROWS {
            let raw = p.raw_row(i);
            let support = p.supports[i];
            let all = accurate_sum32(raw, p.absolute_scratch_phase()).unwrap();
            assert_eq!(all.to_bits(), p.denominators[i].to_bits());
            let consumed = raw[support.start..support.end].iter().copied().sum::<f32>();
            if consumed < all * 0.99999 {
                observed_omitted_tails = true;
                assert!(constant[i] < 0.99999);
            }
            assert!(constant[i] > 0.0 && constant[i] <= 1.000002);
        }
        assert!(
            observed_omitted_tails,
            "support trimming must not be normalized away"
        );
    }
    #[test]
    fn supported_reconstruction_is_finite_without_forcing_analysis_unity() {
        for (rate, bins) in [
            (44100.0, 257),
            (48000.0, 2049),
            (174400.0, 6145),
            (192000.0, 8193),
        ] {
            let p = Projection::new(rate, bins).unwrap();
            let mut out = vec![0.0; bins];
            p.reconstruct(&[1.0; ROWS], &mut out);
            assert!(
                out.iter()
                    .all(|value| value.is_finite() && (*value - 1.0).abs() < 2.0e-6)
            );
            let coefficients = p
                .analyze(&vec![1.0; bins])
                .map(|value| ln32(value.max(0.01)));
            p.reconstruct(&coefficients, &mut out);
            assert!(out.iter().all(|value| value.is_finite()));
            assert!(out.iter().any(|value| *value < -1.0e-6));
        }
        assert!(Projection::new(f32::NAN, 2049).is_err());
    }
}
