//! Kolor notify lambdas A (`14193fab0`) and B (`1419421c0`): the P/P2 blocks and the activity phase.
//!
//! The notify object is `this = core + 0x480`. Its six 0x40-byte blocks (16 floats each) are reached
//! through pointers: `this[0]` = P, `[3]` = D (delta), `[6]` = X (fresh input), `[9]` = S (source),
//! `[0xc]` = P2, `[0xf]` = Z (zeros). Phase is `this+0x90` (= core `+0x510`), flag `this+0x95`, value `this+0x98`.
//!
//! Notify A: P := X, P2 := X, D := 0, Z := 0, phase := 4, flag := 0, then tail-calls B.
//!
//! Notify B, per lane `i` (16 lanes, the decompile loop `lVar27 = 0..0x40 step 0x10` unrolled):
//! 1. X := S, P2 := P, Z := 0, D := X - P (float subtract).
//! 2. `a = (float)(|P| bits as u32)`, `e = (float)(|D| bits as u32)`: the bit magnitudes are converted as
//!    integers, not reinterpreted. `T = 0` if `a == 0`, else `(float)((int)a + 1) - a`.
//! 3. If `e < T`: D := 0, P := X, P2 := X (snap). `sum += e` for every lane, before the snap.
//! 4. `sum <= 0`: phase := 4. Otherwise phase := 2, flag := 1, value := this[0x9c] (= `f9c`).
//!
//! `T` is 1.0 for denormal P and 0 for any normal P (`(int)a + 1` rounds back to `a` above 2^24), so the
//! snap fires only on denormal P lanes. That is what the native vectors show.
//!
//! Vectors: `tools/re/fixtures/kolor_notify_b.json` (160 random block sets, 16 per mode, from
//! `tools/re/kolor_notify_b_probe.py`), checked by `notify_b_matches_native`.
//!
//! Not ported: who writes S (`this[9]`). The control side fills it per knob and per mode; that writer is
//! not traced yet, so the live path still passes audio through.

use super::control::Notifier;

pub const LANES: usize = 16;

/// The notify object's blocks and the phase fields. Pointers are replaced by owned arrays.
#[derive(Clone, Debug, PartialEq)]
pub struct Blocks {
    /// `this[0]`: P, the held value per lane.
    pub p: [f32; LANES],
    /// `this[3]`: D, the delta `X - P` before the snap, zero after a snap.
    pub d: [f32; LANES],
    /// `this[6]`: X, the fresh input (copied from S by B).
    pub x: [f32; LANES],
    /// `this[9]`: S, the source written by the control side.
    pub s: [f32; LANES],
    /// `this[0xc]`: P2, a copy of P taken at each B, overwritten by X on a snap.
    pub p2: [f32; LANES],
    /// `this[0xf]`: Z, zeros.
    pub z: [f32; LANES],
    /// `this+0x90`: phase, 2 when active and 4 when idle.
    pub phase: i32,
    /// `this+0x95`: set when B sees activity.
    pub flag: u8,
    /// `this+0x98`: the value copied from `this+0x9c` on activity.
    pub value: f32,
    /// `this+0x9c`: the value B copies to `value` when active.
    pub value_src: f32,
}

impl Default for Blocks {
    fn default() -> Self {
        Self {
            p: [0.0; LANES],
            d: [0.0; LANES],
            x: [0.0; LANES],
            s: [0.0; LANES],
            p2: [0.0; LANES],
            z: [0.0; LANES],
            phase: 4,
            flag: 0,
            value: 0.0,
            value_src: 0.0,
        }
    }
}

/// Native magnitude of a float: the bit pattern with the sign cleared, converted to float as an integer.
fn magnitude_bits(x: f32) -> f32 {
    (x.to_bits() & 0x7fff_ffff) as f32
}

impl Blocks {
    /// Notify B (`1419421c0`): see the module docs for the per-lane rule.
    pub fn notify_b(&mut self) -> i32 {
        let mut sum = 0.0f32;
        for i in 0..LANES {
            self.x[i] = self.s[i];
            self.p2[i] = self.p[i];
            self.z[i] = 0.0;
            self.d[i] = self.x[i] - self.p[i];
            let a = magnitude_bits(self.p[i]);
            // (int)a of a float at or above 2^31 is INT_MIN on x86 (cvttss2si), not saturated.
            let ia = if a >= 2_147_483_648.0 {
                i32::MIN
            } else {
                a as i32
            };
            let t = if a == 0.0 {
                0.0
            } else {
                ia.wrapping_add(1) as f32 - a
            };
            let e = magnitude_bits(self.d[i]);
            if e < t {
                self.d[i] = 0.0;
                self.p[i] = self.x[i];
                self.p2[i] = self.x[i];
            }
            sum += e;
        }
        if sum <= 0.0 {
            self.phase = 4;
        } else {
            self.phase = 2;
            self.flag = 1;
            self.value = self.value_src;
        }
        self.phase
    }

    /// Notify A (`14193fab0`): the tail call to B is made by the caller (`control::Control::tick`).
    pub fn notify_a(&mut self) {
        self.p = self.x;
        self.p2 = self.x;
        self.d = [0.0; LANES];
        self.z = [0.0; LANES];
        self.phase = 4;
        self.flag = 0;
    }
}

/// The real notifier: the tick's notify calls act on the blocks. `seed` (`14197acc0`) resets the shaper
/// state, which the shaper lane owns, so it is a no-op here.
impl Notifier for Blocks {
    fn notify_a(&mut self) {
        Blocks::notify_a(self);
    }
    fn notify_b(&mut self) -> i32 {
        Blocks::notify_b(self)
    }
    fn seed(&mut self, _mode: usize) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const CASES: &str = include_str!("../../../../../../tools/re/fixtures/kolor_notify_b.json");

    fn floats(v: &Value) -> [f32; LANES] {
        let mut out = [0.0f32; LANES];
        for (o, x) in out.iter_mut().zip(v.as_array().unwrap()) {
            *o = x.as_f64().unwrap() as f32;
        }
        out
    }

    #[test]
    fn notify_b_matches_native() {
        let v: Value = serde_json::from_str(CASES).unwrap();
        let cases = v["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 160);
        let mut snapped = 0;
        let mut silent = 0;
        for case in cases {
            let b = &case["before"];
            let a = &case["after"];
            let mut n = Blocks {
                p: floats(&b["P"]),
                s: floats(&b["S"]),
                d: floats(&b["D"]),
                p2: floats(&b["P2"]),
                z: floats(&b["Z"]),
                value_src: 0.75,
                ..Blocks::default()
            };
            // Lanes with S == P are the snap candidates: the test passes only for a zero delta.
            let equal_lanes = (0..LANES).filter(|&i| n.s[i] == n.p[i]).count();
            let phase = n.notify_b();
            assert_eq!(phase as i64, a["phase"].as_i64().unwrap(), "phase");
            assert_eq!(n.flag as i64, a["flag95"].as_i64().unwrap(), "flag");
            for (name, got) in [("P", n.p), ("D", n.d), ("X", n.x), ("P2", n.p2), ("Z", n.z)] {
                let want = floats(&a[name]);
                for i in 0..LANES {
                    assert_eq!(got[i].to_bits(), want[i].to_bits(), "{name}[{i}]");
                }
            }
            if phase == 2 {
                assert_eq!(n.value, 0.75, "value copied from +0x9c");
            }
            snapped += equal_lanes;
            silent += (phase == 4) as usize;
        }
        // Both phases and equal-lane snaps are exercised by the fixture. A snap needs |D| bits < T, and
        // T is 1 for every P that is an integer below 2^24 (only denormal P), so only zero deltas snap.
        assert!(silent > 0 && silent < cases.len());
        assert!(snapped > 0);
    }

    #[test]
    fn notify_a_copies_x_then_b_is_idle_on_equal_blocks() {
        // A copies X into P and P2 and idles the phase. With S equal to P, B finds no activity.
        let mut n = Blocks {
            x: [1.5; LANES],
            ..Blocks::default()
        };
        n.notify_a();
        assert_eq!((n.p, n.p2, n.phase), ([1.5; LANES], [1.5; LANES], 4));
        n.s = [1.5; LANES];
        assert_eq!(n.notify_b(), 4);
        assert_eq!(n.flag, 0);
    }
}
