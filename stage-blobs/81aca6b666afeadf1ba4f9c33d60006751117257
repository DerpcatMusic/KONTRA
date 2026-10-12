//! Runtime CPU dispatch for the sampler's hot kernels: one binary, kernels
//! compiled for the baseline and for x86-64-v3 (AVX2 + FMA), the level picked
//! once from the running CPU.
//!
//! [`dispatch`] runs a closure inside a function compiled with the wider
//! target features. The closure (and everything `#[inline(always)]` beneath
//! it) is inlined there and vectorized for those features; anything it calls
//! out of line keeps the baseline. Rust never contracts `a * b + c` into a
//! fused multiply-add, so both paths compute bit-identical results; only the
//! vector width changes.
//!
//! aarch64 always has NEON in its baseline, so it needs no dispatch.

mod v1_section;
pub use v1_section::process as filter_section_v1;

/// The instruction set [`dispatch`] runs kernels with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// The build's baseline target features.
    Baseline,
    /// x86-64-v3: AVX2 and FMA.
    V3,
}

/// The level of the running CPU, detected once and cached.
pub fn level() -> Level {
    dispatch::level()
}

/// Run `kernel` compiled for [`level()`]. Mark closures `#[inline(always)]`
/// and keep their callees `#[inline(always)]` so they compile in the wider
/// context.
#[inline(always)]
pub fn dispatch<R>(kernel: impl FnOnce() -> R) -> R {
    dispatch::run(kernel)
}

/// Run `wide` (compiled for x86-64-v3) on a CPU with AVX2 and FMA, else
/// `narrow`. For kernels that fuse multiply-adds on the wide path only:
/// results differ between the two in the last bits, each is deterministic.
/// Both take `context`, so they can borrow the same state mutably.
#[inline(always)]
pub fn dispatch_fused<C, R>(
    context: C,
    narrow: impl FnOnce(C) -> R,
    wide: impl FnOnce(C) -> R,
) -> R {
    dispatch::run_fused(context, narrow, wide)
}

#[allow(unsafe_code)]
mod dispatch {
    use super::Level;

    #[cfg(target_arch = "x86_64")]
    pub(super) fn level() -> Level {
        use std::sync::atomic::{AtomicU8, Ordering};
        // 0 unknown, 1 baseline, 2 v3. Detection is idempotent: racing
        // initializers store the same value.
        static LEVEL: AtomicU8 = AtomicU8::new(0);
        match LEVEL.load(Ordering::Relaxed) {
            1 => Level::Baseline,
            2 => Level::V3,
            _ => {
                let v3 = std::arch::is_x86_feature_detected!("avx2")
                    && std::arch::is_x86_feature_detected!("fma");
                LEVEL.store(if v3 { 2 } else { 1 }, Ordering::Relaxed);
                if v3 { Level::V3 } else { Level::Baseline }
            }
        }
    }

    #[cfg(not(target_arch = "x86_64"))]
    pub(super) fn level() -> Level {
        Level::Baseline
    }

    #[cfg(target_arch = "x86_64")]
    #[inline(always)]
    pub(super) fn run<R>(kernel: impl FnOnce() -> R) -> R {
        if level() == Level::V3 {
            // SAFETY: `v3` only requires AVX2 and FMA, which `level()` has
            // just confirmed on the running CPU. It has no other preconditions.
            unsafe { v3(kernel) }
        } else {
            kernel()
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[inline(always)]
    pub(super) fn run_fused<C, R>(
        context: C,
        narrow: impl FnOnce(C) -> R,
        wide: impl FnOnce(C) -> R,
    ) -> R {
        if level() == Level::V3 {
            // SAFETY: as in `run`.
            unsafe { v3(|| wide(context)) }
        } else {
            narrow(context)
        }
    }

    #[cfg(not(target_arch = "x86_64"))]
    #[inline(always)]
    pub(super) fn run_fused<C, R>(
        context: C,
        narrow: impl FnOnce(C) -> R,
        _wide: impl FnOnce(C) -> R,
    ) -> R {
        narrow(context)
    }

    #[cfg(not(target_arch = "x86_64"))]
    #[inline(always)]
    pub(super) fn run<R>(kernel: impl FnOnce() -> R) -> R {
        kernel()
    }

    /// # Safety
    /// The CPU must support AVX2 and FMA.
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2,fma")]
    #[inline]
    unsafe fn v3<R>(kernel: impl FnOnce() -> R) -> R {
        kernel()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_matches_the_baseline_bit_for_bit() {
        let x: Vec<f32> = (0..1000).map(|i| (i as f32 * 0.37).sin()).collect();
        let sum = |x: &[f32]| {
            let mut lanes = [0.0f32; 8];
            for chunk in x.as_chunks::<8>().0 {
                for k in 0..8 {
                    lanes[k] += chunk[k] * 1.5 + 0.25;
                }
            }
            lanes
        };
        assert_eq!(dispatch(|| sum(&x)), sum(&x));
        assert_eq!(level(), level());
    }
}
