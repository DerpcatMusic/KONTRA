//! Kolor control tick `14197ab90` and the mode table, decoded from `Kontakt8.exe`.
//!
//! Mode: `14197a2c0` and `141977d20` read the raw knob 4 float (`+0x430`) as
//! `clamp((int)(knob * 10.0), 0, 9)`. The shaper selector of object `n = mode*2 + k`
//! (`+0x6f0`, read by the dispatch `1419749f0`) is the mode itself: the Kolor constructor writes
//! `0,0,1,1,...,9,9` for `n = 0..19`, and kemu confirms the same bytes after every mode
//! (`tools/re/kolor_mode_table_probe.py`), with the dispatch case that runs equal to the mode.
//!
//! Tick: one control step per 32-sample segment. Fields are the core-relative offsets from the
//! decompile. The two notify lambdas (`14193fab0` = A, `1419421c0` = B) fill the P/P2 blocks and
//! set the phase (`+0x510`) to 2 or 4 from data that is not ported yet. A tail-calls B. The tick
//! takes them as a [`Notifier`]; `seed` is the per-mode reset `14197acc0`.
//!
//! Vectors: `tools/re/fixtures/kolor_control_tick.json` (400 random states, from
//! `tools/re/kolor_control_tick_probe.py`), checked by `tick_matches_native`.

/// Raw knob 4 to the mode index. `(int)(knob * 10.0)` truncates toward zero, then clamps to 0..=9.
pub fn mode_from_knob(knob: f32) -> usize {
    ((knob * 10.0) as i32).clamp(0, 9) as usize
}

/// Shaper selector of object `n = mode*2 + k` (the byte at `+0x6f0`). Identity on the mode.
pub fn shaper_selector(mode: usize) -> u8 {
    mode as u8
}

/// Host-side effects of the control tick that are not ported yet.
pub trait Notifier {
    /// Notify A (`14193fab0`): fills the P block. Its tail call to B is made by the tick.
    fn notify_a(&mut self);
    /// Notify B (`1419421c0`): fills P2. Returns the new phase (2 when active, 4 when idle).
    fn notify_b(&mut self) -> i32;
    /// Per-mode seed (`14197acc0`): clears the shaper state for `mode`.
    fn seed(&mut self, mode: usize);
}

/// The control fields of the Kolor core (offsets are from the decompile).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Control {
    /// `+0x510`: countdown phase.
    pub phase: i32,
    /// `+0x514`: one-shot, runs notify B.
    pub run_b: bool,
    /// `+0x518`: countdown. Reaching 0 runs notify A.
    pub count: i32,
    /// `+0x7f1`: pending apply. The high byte of the word at `+0x7f0`; the low byte is `flag7f0`.
    pub pending: bool,
    /// `+0x7e8`: ramp target, set to 1.0 by the tick.
    pub target: f32,
    /// `+0x7e4`: smoothed value.
    pub smoothed: f32,
    /// `+0x7f0`: low byte of the word, set to 1 when `smoothed != 1.0` at the apply.
    pub flag7f0: u8,
    /// `+0x7f4`: mode changed, reseed on the next apply.
    pub dirty: bool,
    /// `+0x7dc`: current mode.
    pub mode: i32,
}

impl Control {
    /// One tick. `knob4` is the raw float at `+0x430`.
    pub fn tick(&mut self, knob4: f32, n: &mut impl Notifier) {
        if !self.run_b {
            if self.phase != 0 {
                self.count = self.count.wrapping_sub(1);
                if self.count == 0 {
                    n.notify_a();
                    self.phase = n.notify_b();
                } else if self.phase == 4 {
                    self.phase = 0;
                } else if self.phase == 2 {
                    self.phase = 3;
                }
            }
        } else {
            self.run_b = false;
            self.phase = n.notify_b();
        }
        if self.pending {
            self.pending = false;
            if self.target == 0.0 {
                if self.dirty {
                    let mode = mode_from_knob(knob4);
                    self.mode = mode as i32;
                    self.dirty = false;
                    n.seed(mode);
                }
                let changed = 1.0 != self.smoothed;
                self.target = 1.0;
                if changed {
                    self.flag7f0 = 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const TICKS: &str = include_str!("../../../../../../tools/re/fixtures/kolor_control_tick.json");

    /// Replays the native notify results. `notify_b` returns the phase the native run ended with.
    struct Replay {
        phase_after: i32,
        a: u32,
        b: u32,
        seed: u32,
    }

    impl Notifier for Replay {
        fn notify_a(&mut self) {
            self.a += 1;
        }
        fn notify_b(&mut self) -> i32 {
            self.b += 1;
            self.phase_after
        }
        fn seed(&mut self, _: usize) {
            self.seed += 1;
        }
    }

    #[test]
    fn mode_table_and_knob_map() {
        // Selector table as measured under kemu: object n = 2*mode + k has selector n/2.
        let measured = [0u8, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9];
        for (n, sel) in measured.iter().enumerate() {
            assert_eq!(shaper_selector(n / 2), *sel);
        }
        // Host knob -> mode: mode/9 round-trips (apply feeds knob 4 as mode/9).
        for mode in 0..10usize {
            assert_eq!(mode_from_knob(mode as f32 / 9.0), mode, "mode {mode}");
        }
        assert_eq!(mode_from_knob(-0.3), 0);
        assert_eq!(mode_from_knob(1.0), 9);
        assert_eq!(mode_from_knob(0.99), 9);
    }

    #[test]
    fn tick_matches_native() {
        let v: Value = serde_json::from_str(TICKS).unwrap();
        let fields = v["fields"].as_array().unwrap();
        assert_eq!(fields.len(), 10);
        let mut checked = 0;
        for case in v["cases"].as_array().unwrap() {
            let b = &case["before"];
            let a = &case["after"];
            let fired = &case["fired"];
            let mut c = Control {
                phase: b["phase"].as_i64().unwrap() as i32,
                run_b: b["flag514"].as_u64().unwrap() != 0,
                count: b["count518"].as_i64().unwrap() as i32,
                pending: b["pending7f1"].as_u64().unwrap() != 0,
                target: b["gain7e8"].as_f64().unwrap() as f32,
                smoothed: b["gain7e4"].as_f64().unwrap() as f32,
                flag7f0: b["flag7f0"].as_u64().unwrap() as u8,
                dirty: b["dirty7f4"].as_u64().unwrap() != 0,
                mode: b["mode7dc"].as_i64().unwrap() as i32,
            };
            let knob = b["knob430"].as_f64().unwrap() as f32;
            let mut r = Replay {
                phase_after: a["phase"].as_i64().unwrap() as i32,
                a: 0,
                b: 0,
                seed: 0,
            };
            c.tick(knob, &mut r);
            // Notify counts: the native run fires A, B and seed exactly as the port does.
            let count = |k: &str| fired.get(k).and_then(Value::as_u64).unwrap_or(0) as u32;
            assert_eq!((r.a, r.b, r.seed), (count("notify_a"), count("notify_b"), count("seed")));
            assert_eq!(c.count, a["count518"].as_i64().unwrap() as i32, "count");
            assert_eq!(c.run_b as u64, a["flag514"].as_u64().unwrap());
            assert_eq!(c.pending as u64, a["pending7f1"].as_u64().unwrap());
            assert_eq!(c.target, a["gain7e8"].as_f64().unwrap() as f32, "target");
            assert_eq!(c.flag7f0 as u64, a["flag7f0"].as_u64().unwrap(), "flag7f0");
            assert_eq!(c.dirty as u64, a["dirty7f4"].as_u64().unwrap());
            assert_eq!(c.mode as i64, a["mode7dc"].as_i64().unwrap());
            assert_eq!(c.phase, a["phase"].as_i64().unwrap() as i32, "phase");
            checked += 1;
        }
        assert_eq!(checked, 400);
    }
}
