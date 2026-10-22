//! Kolor dry path: the per-channel gain ramp, measured from `Kontakt8.exe` through the kemu harness.
//!
//! With the shaper at zero (modes 0, 1, 3, 4, 6..9 on the native vectors), the output is the input
//! times a one-pole ramp toward 0.5 (`(1 - wet)` at the default wet 0.5):
//! `g[n] = A * g[n-1] + B`, `A = 0.99947651`, `B = 0.5 * (1 - A)`. Measured from the impulse,
//! step and fixture data (`tools/re/kolor_kemu_probe.py`). The left channel's first gain is `B/2`,
//! the right channel's is `B`. The kernel's constants are not decoded yet: `A` and `B` are fitted.
//!
//! Not in this module: the shaper contribution of modes 2 and 5 (the residual is nonzero there, see
//! `docs/re/engine/k-effects-batch-a.md`), knob 0 (shaper drive, no effect on the dry path) and any
//! post-ramp filter.

/// The one-pole coefficient, fitted from the native step responses.
const A: f32 = 0.999_476_5;
/// `0.5 * (1 - A)`: the ramp settles at 0.5.
const B: f32 = 2.617_485e-4;

/// One channel's ramp state.
#[derive(Clone, Copy, Debug)]
pub struct GainRamp {
    g: f32,
}

impl GainRamp {
    /// Left: first sample gain `B / 2`. The state is one step behind so the first `next` lands there.
    pub const LEFT: Self = Self { g: -B / (2.0 * A) };
    /// Right: first sample gain `B`.
    pub const RIGHT: Self = Self { g: 0.0 };

    /// Advance one sample and return its gain.
    pub fn next(&mut self) -> f32 {
        self.g = A * self.g + B;
        self.g
    }
}

/// The dry path for a stereo block. Each sample is scaled by its channel's ramp.
#[derive(Clone, Copy, Debug)]
pub struct DryPath {
    left: GainRamp,
    right: GainRamp,
}

impl Default for DryPath {
    fn default() -> Self {
        Self { left: GainRamp::LEFT, right: GainRamp::RIGHT }
    }
}

impl DryPath {
    /// Scale `left` and `right` in place by their ramps. Both slices must be the same length.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            *l *= self.left.next();
            *r *= self.right.next();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Modes whose shaper contributes nothing on the native vectors (steady and moving).
    const DRY_ONLY_MODES: [u64; 8] = [0, 1, 3, 4, 6, 7, 8, 9];

    fn fixture() -> serde_json::Value {
        serde_json::from_str(include_str!("../../../../../../tools/re/fixtures/kolor_native_vectors.json")).unwrap()
    }

    fn floats(v: &serde_json::Value) -> Vec<f32> {
        v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap() as f32).collect()
    }

    /// Input of `tools/re/kolor_kemu_probe.py::signal(512)`.
    fn signal() -> (Vec<f32>, Vec<f32>) {
        let rate = 48_000.0_f64;
        let tau = std::f64::consts::TAU;
        let left = (0..512).map(|i| {
            let t = i as f64 / rate;
            (0.5 * (tau * 220.0 * t).sin() + 0.2 * (tau * 3100.0 * t).sin()) as f32
        });
        let right = (0..512).map(|i| {
            let t = i as f64 / rate;
            (0.45 * (tau * 330.0 * t + 0.3).sin()) as f32
        });
        (left.collect(), right.collect())
    }

    #[test]
    fn dry_path_matches_native_vectors_for_shaper_free_modes() {
        let fx = fixture();
        assert_eq!(fx["rate"].as_f64(), Some(48_000.0));
        let (left, right) = signal();
        for case in fx["cases"].as_array().unwrap() {
            let mode = case["mode"].as_u64().unwrap();
            if !DRY_ONLY_MODES.contains(&mode) {
                continue;
            }
            for (name, want_l, want_r) in [
                ("steady", floats(&case["steady_L"]), floats(&case["steady_R"])),
                ("moving", floats(&case["moving_L"]), floats(&case["moving_R"])),
            ] {
                // Moving automation changes knob 0 only, which the dry path does not read.
                let mut l = left.clone();
                let mut r = right.clone();
                DryPath::default().process(&mut l, &mut r);
                for n in 0..512 {
                    assert!((l[n] - want_l[n]).abs() < 1e-5, "mode {mode} {name} L n={n}: {} vs {}", l[n], want_l[n]);
                    assert!((r[n] - want_r[n]).abs() < 1e-5, "mode {mode} {name} R n={n}: {} vs {}", r[n], want_r[n]);
                }
            }
        }
    }

    #[test]
    fn ramp_settles_at_half_and_starts_at_its_measured_values() {
        let mut ramp = GainRamp::LEFT;
        assert!((ramp.next() - B / 2.0).abs() < 1e-9);
        let mut last = 0.0;
        for _ in 0..200_000 {
            last = ramp.next();
        }
        assert!((last - 0.5).abs() < 1e-4);
    }
}
