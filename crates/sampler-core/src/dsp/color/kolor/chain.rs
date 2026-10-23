//! Kolor dry path: the per-channel gain smoother and the wet mix, measured from `Kontakt8.exe`
//! through the kemu harness (`tools/re/kolor_kemu_probe.py`).
//!
//! With the shaper at zero (modes 0, 1, 3, 4, 6..9 at default drive), the output is the input times
//! `(1 - wet) * s[n]`, where `s` is a one-pole smoother toward 1.0: `s[n] = A * s[n-1] + (1 - A)`.
//! Measured: the wet knob scales linearly (`wet` 0, 0.25, 0.5, 1 give ratios 1, 0.75, 0.5, 0 at modes 6
//! and 9), and the fitted `A = 0.99947651` reproduces the native step response. The left channel starts
//! one half step lower than the right (`s[-1] = -(1 - A) / (2A)` against `0` for the right).
//!
//! Not in this module: the shaper output of modes 2 and 5 (post-shaper sections, unmapped), the drive
//! shaper cases at non-default drive, and the level detector. The stage keeps its state in the
//! storage of the chain it sits in (see [`Dry::FRAMES`]), so a block boundary does not reset it.

use crate::dsp::Planar;

/// One-pole smoother coefficient, fitted from the native step responses.
const A: f64 = 0.999_476_5;

/// Frames of delay storage: frame 0 holds the two smoother states, frame 1 the started flag.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Dry {
    /// `1 - wet`, the dry share (`wet` is Kolor knob 7).
    dry: f64,
}

impl Dry {
    /// Frames of delay storage the state occupies.
    pub(crate) const FRAMES: usize = 2;

    /// `wet` is Kolor knob 7 in `0..=1`.
    pub(crate) fn new(wet: f64) -> Self {
        Self { dry: 1. - wet }
    }

    /// `1 - wet`, for traces.
    pub(crate) fn dry(&self) -> f64 {
        self.dry
    }

    /// Scale `block` in place. `storage[0] = [left, right]` smoother states, `storage[1][0]` is 0 until the
    /// first block has run.
    pub(crate) fn process(&self, storage: &mut [[f64; 2]], block: &mut Planar, len: usize) {
        if storage[1][0] == 0. {
            // Fresh state: the left smoother starts one half step below the right.
            storage[0] = [-(1. - A) / (2. * A), 0.];
            storage[1][0] = 1.;
        }
        let mut state = storage[0];
        for i in 0..len {
            for (channel, s) in block.iter_mut().zip(state.iter_mut()) {
                *s = A * *s + (1. - A);
                channel[i] *= self.dry * *s;
            }
        }
        storage[0] = state;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::BLOCK;

    /// Modes whose shaper contributes nothing on the native vectors (steady and moving).
    const DRY_ONLY_MODES: [u64; 8] = [0, 1, 3, 4, 6, 7, 8, 9];

    fn fixture() -> serde_json::Value {
        serde_json::from_str(include_str!("../../../../../../tools/re/fixtures/kolor_native_vectors.json")).unwrap()
    }

    fn floats(v: &serde_json::Value) -> Vec<f64> {
        v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()
    }

    /// Input of `tools/re/kolor_kemu_probe.py::signal(512)`.
    fn signal() -> (Vec<f64>, Vec<f64>) {
        let rate = 48_000.0_f64;
        let tau = std::f64::consts::TAU;
        let left = (0..512).map(|i| {
            let t = i as f64 / rate;
            0.5 * (tau * 220.0 * t).sin() + 0.2 * (tau * 3100.0 * t).sin()
        });
        let right = (0..512).map(|i| {
            let t = i as f64 / rate;
            0.45 * (tau * 330.0 * t + 0.3).sin()
        });
        (left.collect(), right.collect())
    }

    fn run(wet: f64, left: &[f64], right: &[f64]) -> (Vec<f64>, Vec<f64>) {
        let mut block: Planar = [[0.; BLOCK]; 2];
        let mut storage = [[0.; 2]; Dry::FRAMES];
        let (mut out_l, mut out_r) = (Vec::new(), Vec::new());
        for (l, r) in left.chunks(BLOCK).zip(right.chunks(BLOCK)) {
            block[0][..l.len()].copy_from_slice(l);
            block[1][..r.len()].copy_from_slice(r);
            Dry::new(wet).process(&mut storage, &mut block, l.len());
            out_l.extend_from_slice(&block[0][..l.len()]);
            out_r.extend_from_slice(&block[1][..r.len()]);
        }
        (out_l, out_r)
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
            // The fixture is at default knobs, so wet is 0.5. Moving automation only changes knob 0,
            // which the dry path does not read.
            for (name, want_l, want_r) in [
                ("steady", floats(&case["steady_L"]), floats(&case["steady_R"])),
                ("moving", floats(&case["moving_L"]), floats(&case["moving_R"])),
            ] {
                let (l, r) = run(0.5, &left, &right);
                for n in 0..512 {
                    assert!((l[n] - want_l[n]).abs() < 1e-5, "mode {mode} {name} L n={n}: {} vs {}", l[n], want_l[n]);
                    assert!((r[n] - want_r[n]).abs() < 1e-5, "mode {mode} {name} R n={n}: {} vs {}", r[n], want_r[n]);
                }
            }
        }
    }

    #[test]
    fn wet_sweep_matches_native_for_every_dry_mode() {
        // `tools/re/kolor_kemu_probe.py --wet-sweep`: native output per (mode, wet), 256 frames.
        let fx: serde_json::Value =
            serde_json::from_str(include_str!("../../../../../../tools/re/fixtures/kolor_wet_sweep.json")).unwrap();
        let (left, right) = signal();
        for case in fx["cases"].as_array().unwrap() {
            let (mode, wet) = (case["mode"].as_u64().unwrap(), case["wet"].as_f64().unwrap());
            let (l, r) = run(wet, &left[..256], &right[..256]);
            for (name, got, want) in [("L", &l, floats(&case["L"])), ("R", &r, floats(&case["R"]))] {
                for n in 0..256 {
                    assert!((got[n] - want[n]).abs() < 1e-5, "mode {mode} wet {wet} {name} n={n}: {} vs {}", got[n], want[n]);
                }
            }
        }
    }

    /// `tools/re/kolor_drive_sweep.py`: the kemu block at drive 0..1 (knob 3) and wet 0.5, for the
    /// three inputs sine, impulse and noise. Measured: the drive knob does nothing for the shaper-free
    /// modes on these inputs: the native output is the dry path at every drive. The shaper readout weights
    /// `w` and bias `c` (shaper P[152..161]) are zero in every GRU mode, so the shaper output is zero even
    /// with the static-init blocks loaded (`tools/re/kolor_static_init.py`). Modes 2 and 5 are checked in
    /// their own test.
    #[test]
    fn drive_sweep_matches_native_for_dry_modes() {
        let fx: serde_json::Value =
            serde_json::from_str(include_str!("../../../../../../tools/re/fixtures/kolor_drive_sweep.json")).unwrap();
        let n = 256;
        let (sine_l, sine_r) = signal();
        let mut seed = |mut x: u64| -> Vec<f64> {
            (0..n).map(|_| {
                x = (1_103_515_245 * x + 12_345) & 0x7FFF_FFFF;
                0.5 * (x as f64 / 0x7FFF_FFFF as f64 * 2.0 - 1.0)
            }).collect()
        };
        let inputs = [
            ("sine", sine_l[..n].to_vec(), sine_r[..n].to_vec()),
            ("impulse", [vec![1.0], vec![0.0; n - 1]].concat(), vec![0.0; n]),
            ("noise", seed(12345), seed(777)),
        ];
        for case in fx["cases"].as_array().unwrap() {
            let mode = case["mode"].as_u64().unwrap();
            if !DRY_ONLY_MODES.contains(&mode) {
                continue;
            }
            let name = case["signal"].as_str().unwrap();
            let (_, left, right) = inputs.iter().find(|(s, _, _)| *s == name).unwrap();
            let (l, r) = run(fx["wet"].as_f64().unwrap(), left, right);
            let (want_l, want_r) = (floats(&case["L"]), floats(&case["R"]));
            for i in 0..n {
                let drive = case["drive"].as_f64().unwrap();
                assert!((l[i] - want_l[i]).abs() < 1e-5, "mode {mode} drive {drive} {name} L n={i}");
                assert!((r[i] - want_r[i]).abs() < 1e-5, "mode {mode} drive {drive} {name} R n={i}");
            }
        }
    }

    #[test]
    fn smoother_settles_at_one_and_starts_at_its_measured_values() {
        // Left first gain (1-A)/2, right first gain (1-A), with wet 0 so the dry share is 1.
        let mut first: Planar = [[1.; BLOCK]; 2];
        let mut storage = [[0.; 2]; Dry::FRAMES];
        Dry::new(0.).process(&mut storage, &mut first, 1);
        assert!((first[0][0] - (1. - A) / 2.).abs() < 1e-12);
        assert!((first[1][0] - (1. - A)).abs() < 1e-12);
        let mut settled: Planar = [[1.; BLOCK]; 2];
        for _ in 0..200 {
            settled = [[1.; BLOCK]; 2];
            Dry::new(0.).process(&mut storage, &mut settled, BLOCK);
        }
        assert!((settled[0][BLOCK - 1] - 1.).abs() < 0.1);
    }
}
