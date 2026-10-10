#![allow(unsafe_code)]

/// v1 Section::process_avx keeps paired channels in registers through the fold.
#[inline(always)]
pub fn fold_stereo(sum: [[f32; 4]; 2]) -> [f32; 2] {
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
