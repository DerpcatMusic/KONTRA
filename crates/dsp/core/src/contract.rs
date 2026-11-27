//! Shared functional DSP contract verification: block-size invariance,
//! finite stability, and clean reset. One contract runner for every DSP core.

/// Common RT-safe stereo DSP processor interface.
pub trait DspCore: Send {
    fn process(&mut self, io: &mut [[f32; 2]]);
    fn reset(&mut self);
}

/// Implements [`DspCore`] for types with inherent `process(&mut self, &mut [[f32; 2]])`
/// and `reset(&mut self)` methods.
#[macro_export]
macro_rules! impl_dsp_core {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl $crate::DspCore for $ty {
                #[inline]
                fn process(&mut self, io: &mut [[f32; 2]]) {
                    <$ty>::process(self, io);
                }
                #[inline]
                fn reset(&mut self) {
                    <$ty>::reset(self);
                }
            }
        )+
    };
}

/// Verifies that a DSP processor is block-size invariant across `[1, 7, 64, 256]`,
/// produces only finite samples, and resets cleanly to its initial state.
pub fn assert_block_invariant_and_finite<F, P>(make: F, frames: usize, tol: f32)
where
    F: Fn() -> P,
    P: DspCore,
{
    let input: Vec<[f32; 2]> = (0..frames)
        .map(|i| {
            let t = i as f32;
            [
                0.5 * (t * 0.13).sin() + if i == 0 { 0.4 } else { 0.0 },
                -0.4 * (t * 0.19).cos() + if i == 17 { -0.3 } else { 0.0 },
            ]
        })
        .collect();

    let mut whole = input.clone();
    let mut unit = make();
    unit.process(&mut whole);
    assert!(
        whole.iter().flatten().all(|v| v.is_finite()),
        "non-finite output on whole-block render"
    );

    for chunk in [1usize, 7, 64, 256] {
        unit.reset();
        let mut chunked = input.clone();
        for part in chunked.chunks_mut(chunk) {
            unit.process(part);
        }
        let max_err = whole
            .iter()
            .zip(&chunked)
            .map(|(a, b)| (a[0] - b[0]).abs().max((a[1] - b[1]).abs()))
            .fold(0.0_f32, f32::max);
        assert!(
            max_err <= tol,
            "block-size invariance failed for chunk={chunk}: max_err={max_err:e} > tol={tol:e}"
        );
    }
}
