#![allow(unsafe_code)]

/// Prepared CPU choice for the paired-channel fold, like v1's native kernels.
/// The private capability checks AVX2 once, outside the per-sample loop.
pub struct StereoFold {
    wide: bool,
}

impl StereoFold {
    pub fn new() -> Self {
        Self {
            wide: crate::level() == crate::Level::V3,
        }
    }

    #[inline(always)]
    pub fn fold(&self, sum: [[f32; 4]; 2]) -> [f32; 2] {
        #[cfg(target_arch = "x86_64")]
        if self.wide {
            // SAFETY: the private capability is created only after CPU detection.
            return unsafe { wide(sum) };
        }
        baseline(sum)
    }
}

#[inline(always)]
fn baseline(sum: [[f32; 4]; 2]) -> [f32; 2] {
    #[cfg(target_arch = "x86_64")]
    {
        use std::arch::x86_64::*;
        // SAFETY: SSE2 is baseline on x86_64; both loads read four owned lanes.
        unsafe {
            let half = _mm_add_ps(_mm_loadu_ps(sum[0].as_ptr()), _mm_loadu_ps(sum[1].as_ptr()));
            let stereo = _mm_add_ps(half, _mm_movehl_ps(half, half));
            let mut out = [0.; 2];
            _mm_storel_epi64(out.as_mut_ptr().cast(), _mm_castps_si128(stereo));
            return out;
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let half: [f32; 4] = std::array::from_fn(|k| sum[0][k] + sum[1][k]);
        [half[0] + half[2], half[1] + half[3]]
    }
}

// Keep the complete eight-lane accumulator live through v1's AVX paired lanes.
// Reducing to SSE before the fold narrows the enclosing dot loop to 128 bits.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn wide(sum: [[f32; 4]; 2]) -> [f32; 2] {
    use std::arch::x86_64::*;
    // SAFETY: caller's private capability guarantees AVX2. The nested array
    // owns eight contiguous f32 lanes; the output owns the two stored lanes.
    unsafe {
        let packed = _mm256_loadu_ps(sum.as_ptr().cast());
        let half = _mm256_add_ps(packed, _mm256_permute2f128_ps::<1>(packed, packed));
        let stereo = _mm256_add_ps(half, _mm256_permute_ps::<0x4e>(half));
        let mut out = [0.; 2];
        _mm_storel_epi64(
            out.as_mut_ptr().cast(),
            _mm_castps_si128(_mm256_castps256_ps128(stereo)),
        );
        out
    }
}
