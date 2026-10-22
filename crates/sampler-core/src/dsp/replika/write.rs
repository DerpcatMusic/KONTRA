//! Replika head write stage (`141acd110`, 597 B): one frame of the write side of a head (piece 3c part 3).
//!
//! Native order (decompile `0141ac0000.c` 8436-8500, listing `docs/re/disasm/141acd110.txt`):
//! 1. hold (`H+0x6c`) set and the fade (`H+0x5824`) clear: store the inputs at `H+0x348` / `H+0x34c`, return.
//! 2. fade set: store the inputs, then scale both by `1 - f32[H+0x5808]`.
//! 3. mode 0: write the scaled inputs into the mode-0 rings at the slots of `H+0x158` / `H+0x15c` (L) and
//!    `H+0x194` / `H+0x190` (R), return. The R slot is not the L slot mirrored: see [`slot_r`].
//! 4. mode 1: the write filter of each channel (`141aa3c60`, `H+0x2dc0` L, `H+0x4240` R).
//! 5. the write sections (`140d947a0`, `H+0x350` and `H+0x384`), then the deposit `141accfb0` of each tap: K1
//!    (`H+0x1a8`) with x = the section's lowpass (`H+0x368`), K2 (`H+0x278`) with `H+0x39c`. The deposit writes the
//!    ring behind `K+0xc0`, which is the mode-0 ring of the same channel (identity closed under kemu).
//!
//! Modes 2 and 3 return `Err(Blocked)` before anything is written. Mode 2 calls `14199c870` (not ported). Mode 3 runs
//! the one-pole `141aa31f0` on the objects at `H+0x3d0` / `H+0x430` with the one-pole states `H+0x3c0` / `H+0x420` and
//! coefficients `H+0x3c4` / `H+0x424`; none of that is modelled on [`Head`]. Modes other than 0..3 take step 5 alone, as
//! the native does (no filter).
//!
//! Vectors: `docs/re/engine/vectors/replika-native-head-write-{mode1,hold,fade,mode0}.json` (kemu,
//! `tools/re/replika_write_kemu.py`).

use super::deposit::Deposit;
use super::head::{Blocked, Head};
use super::read::Tap;

/// Mode-0 slot of the L ring (`141acd110`): with `e = i` clipped to 0 when `i < w`, `r = i - e`; a negative `r` takes
/// `+ w`. For `0 <= i`, `w >= 0` the slot is `i` when `i < w`, else `i - w`.
fn slot_l(i: i32, w: i32) -> i32 {
    let e = if i < w { 0 } else { w };
    let r = i.wrapping_sub(e);
    (if r >= 0 { 0 } else { w }).wrapping_add(r)
}

/// Mode-0 slot of the R ring (`141acd110`, the R pair `H+0x194` / `H+0x190`): `r = w - e` with `e = i` clipped to 0
/// when `w < i`; a negative `r` takes `+ i`. The native takes the wrap minus the index here, not the index minus the wrap.
fn slot_r(i: i32, w: i32) -> i32 {
    let e = if w < i { 0 } else { i };
    let r = w.wrapping_sub(e);
    (if r >= 0 { 0 } else { i }).wrapping_add(r)
}

/// `141accfb0` on `tap`: the deposit reads the tap's `+0x08` (write), `+0x10` (offset), `+0x18` (mask), `+0x1c` (count),
/// `+0x90` / `+0x98` (rates) and its history `+0x84..+0x8c`, which it shifts. The fields are written back: only the history.
fn deposit(tap: &mut Tap, ring: &mut [f32], x: f32) {
    let mut d = Deposit {
        write: tap.wr,
        off: tap.frac,
        mask: tap.mask,
        count: tap.delta,
        hist: tap.hist,
        a: tap.rates[0],
        b: tap.rates[1],
    };
    d.push(ring, x);
    tap.hist = d.hist;
}

impl Head {
    /// `141acd110(H, x_l, x_r)`: one frame of the write stage. `rings` are the L and R rings behind `H+0x150` and
    /// `H+0x188` (the mode-0 rings, and the deposit's rings through `K+0xc0`). Returns `Err(Blocked)` for modes 2 and 3,
    /// with nothing written.
    ///
    /// Mode-0 slots are not range checked, as in the native: an out-of-range slot panics here (the native writes past
    /// the ring). The vectors keep every slot inside the ring.
    pub fn write(&mut self, x: [f32; 2], rings: [&mut [f32]; 2]) -> Result<(), Blocked> {
        if matches!(self.mode, 2 | 3) {
            return Err(Blocked);
        }
        let [ring_l, ring_r] = rings;
        let mut x = x;
        if self.fade_on {
            self.stored = x;
            let gain = 1.0 - self.fade_amount;
            x = [x[0] * gain, x[1] * gain];
        } else if self.hold {
            self.stored = x;
            return Ok(());
        }

        match self.mode {
            0 => {
                ring_l[slot_l(self.ring_pos[0], self.ring_wrap[0]) as usize] = x[0];
                ring_r[slot_r(self.ring_pos[1], self.ring_wrap[1]) as usize] = x[1];
                return Ok(());
            }
            1 => {
                x[0] = self.write_filter[0].tick(x[0]);
                x[1] = self.write_filter[1].tick(x[1]);
            }
            _ => {}
        }

        let (_, lp_l) = self.sec[0].tick(&mut self.sec_state[0], x[0]);
        let (_, lp_r) = self.sec[1].tick(&mut self.sec_state[1], x[1]);
        self.sec_out = [lp_l, lp_r];
        deposit(&mut self.taps[0], ring_l, lp_l);
        deposit(&mut self.taps[1], ring_r, lp_r);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::replika::delay::DelayPos;
    use crate::dsp::replika::head::{Fade, Glide};
    use crate::dsp::replika::mode1::{PostFilter, WriteFilter};
    use crate::dsp::tpt_svf::{Section, State};
    use serde_json::Value;

    const RING: usize = 64;

    fn vector(name: &str) -> Value {
        let path = format!(
            "{}/../../docs/re/engine/vectors/replika-native-head-write-{name}.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(path).expect("vector file"))
            .expect("vector json")
    }

    fn fl(v: &Value) -> f32 {
        v.as_f64().expect("f32") as f32
    }

    fn floats(v: &Value) -> Vec<f32> {
        v.as_array().expect("array").iter().map(fl).collect()
    }

    fn int(v: &Value) -> i32 {
        v.as_i64().expect("int") as i32
    }

    /// `1e-5` relative to the native value, with an absolute floor of `1e-5` for the values near zero.
    fn close(label: &str, got: f64, want: f64) {
        let tol = 1e-5 * want.abs().max(1.0);
        assert!((got - want).abs() <= tol, "{label}: got {got} want {want}");
    }

    /// The head as the native snapshot has it before the case's first frame. The driver fields are idle (zero).
    fn head(doc: &Value, init: &Value) -> Head {
        let lane = |side: &str| -> Vec<u32> {
            doc["lane_words"][side]
                .as_array()
                .expect("lane words")
                .iter()
                .map(|w| w.as_u64().expect("u32") as u32)
                .collect()
        };
        let tap = |t: &Value| {
            let mut k = Tap::new(0., int(&t["mask"]), 0., 0, 0., 0., 0., 1.);
            k.wr = int(&t["wr"]);
            k.frac = t["frac"].as_f64().expect("frac");
            k.delta = int(&t["delta"]);
            k.rates = [
                t["rate0"].as_f64().expect("rate0"),
                t["rate1"].as_f64().expect("rate1"),
                0.,
            ];
            let h = floats(&t["hist"]);
            k.hist = [h[0], h[1], h[2]];
            k
        };
        let sec = |s: &Value| Section::from_coefficients(fl(&s["c2"]), fl(&s["c3"]), fl(&s["c4"]));
        let state = |s: &Value| State {
            s0: fl(&s["s0"]),
            s1: fl(&s["s1"]),
        };
        let secs = &init["sec"];
        let taps = &init["taps"];
        Head {
            mode: int(&init["mode"]),
            factor: 0.,
            s: 0.,
            base_src: 0.,
            delay: DelayPos {
                lo: 0,
                hi: 0,
                mode: 0,
                base: 0.,
                r58: 0.,
                r60: 0.,
                a: 0.,
                b: 0.,
                flag: false,
            },
            taps: [tap(&taps[0]), tap(&taps[1])],
            fade: Fade {
                coef: 0.,
                level: 0.,
                target: 0.,
                thr: 0.,
                pending: false,
                trigger: false,
            },
            glide: Glide {
                cur: 0.,
                target: 0.,
                step: 0.,
                thr: 0.,
                count: 0,
                pending: false,
                fresh: false,
                div: 1.,
                count_init: 0,
            },
            post: [PostFilter {
                env: 0.,
                coef: 0.,
                s0: 0.,
                s1: 0.,
                c2: 0.,
                c3: 0.,
                c4: 0.,
                gain: 0.,
            }; 2],
            sec: [sec(&secs[0]), sec(&secs[1])],
            rate: 1.,
            time: [0.; 4],
            anchor: [0.; 3],
            lane: 0,
            mode_flags: [false; 3],
            kernel: [1, 1],
            ring_pos: [int(&init["ring_pos"][0]), int(&init["ring_pos"][1])],
            ring_wrap: [int(&init["ring_wrap"][0]), int(&init["ring_wrap"][1])],
            hold: int(&init["hold"]) != 0,
            fade_on: int(&init["fade_on"]) != 0,
            fade_amount: fl(&init["fade_amount"]),
            stored: [fl(&init["stored"][0]), fl(&init["stored"][1])],
            write_filter: [
                WriteFilter::from_object(&lane("l")),
                WriteFilter::from_object(&lane("r")),
            ],
            sec_state: [state(&secs[0]), state(&secs[1])],
            sec_out: [fl(&init["sec_out"][0]), fl(&init["sec_out"][1])],
        }
    }

    /// The fields one frame writes, against the native snapshot after that frame.
    fn check(label: &str, h: &Head, rings: [&[f32]; 2], after: &Value) {
        for i in 0..2 {
            close(
                &format!("{label} stored[{i}]"),
                f64::from(h.stored[i]),
                f64::from(fl(&after["stored"][i])),
            );
            close(
                &format!("{label} sec_out[{i}]"),
                f64::from(h.sec_out[i]),
                f64::from(fl(&after["sec_out"][i])),
            );
            let s = &after["sec"][i];
            close(
                &format!("{label} sec[{i}].s0"),
                f64::from(h.sec_state[i].s0),
                f64::from(fl(&s["s0"])),
            );
            close(
                &format!("{label} sec[{i}].s1"),
                f64::from(h.sec_state[i].s1),
                f64::from(fl(&s["s1"])),
            );
            let t = &after["taps"][i];
            assert_eq!(h.taps[i].wr, int(&t["wr"]), "{label} tap[{i}].wr");
            close(
                &format!("{label} tap[{i}].frac"),
                h.taps[i].frac,
                t["frac"].as_f64().unwrap(),
            );
            for (j, want) in floats(&t["hist"]).into_iter().enumerate() {
                close(
                    &format!("{label} tap[{i}].hist[{j}]"),
                    f64::from(h.taps[i].hist[j]),
                    f64::from(want),
                );
            }
            let wf = floats(&after["wf"][i]);
            for (j, got) in h.write_filter[i].probe().into_iter().enumerate() {
                close(
                    &format!("{label} wf[{i}][{j}]"),
                    f64::from(got),
                    f64::from(wf[j]),
                );
            }
        }
        for (j, want) in floats(&after["ring_l"]).into_iter().enumerate() {
            close(
                &format!("{label} ring_l[{j}]"),
                f64::from(rings[0][j]),
                f64::from(want),
            );
        }
        for (j, want) in floats(&after["ring_r"]).into_iter().enumerate() {
            close(
                &format!("{label} ring_r[{j}]"),
                f64::from(rings[1][j]),
                f64::from(want),
            );
        }
    }

    /// Replays every case of `name` frame by frame: `Head::write` on the head of `init`, checked after each frame.
    /// Returns the number of frames checked.
    fn replay(name: &str) -> usize {
        let doc = vector(name);
        let mut frames = 0;
        for case in doc["cases"].as_array().expect("cases") {
            let init = &case["init"];
            let mut h = head(&doc, init);
            let mut ring_l = floats(&init["ring_l"]);
            let mut ring_r = floats(&init["ring_r"]);
            assert_eq!(ring_l.len(), RING);
            for (k, frame) in case["frames"]
                .as_array()
                .expect("frames")
                .iter()
                .enumerate()
            {
                let x = [fl(&frame["x"][0]), fl(&frame["x"][1])];
                h.write(x, [&mut ring_l[..], &mut ring_r[..]])
                    .expect("modes 0 and 1 are ported");
                let label = format!("{name} seed {} frame {k}", case["seed"]);
                check(&label, &h, [&ring_l[..], &ring_r[..]], &frame["after"]);
                frames += 1;
            }
        }
        frames
    }

    /// Mode 1 with hold and fade clear: WriteFilter, the write sections, the deposit into both rings.
    #[test]
    fn write_mode1_matches_native_vectors() {
        assert!(replay("mode1") > 0);
    }

    /// Hold set, fade clear: the inputs are stored, and the rings, filters and sections are left alone.
    #[test]
    fn write_hold_stores_inputs_and_writes_nothing() {
        let doc = vector("hold");
        for case in doc["cases"].as_array().expect("cases") {
            for frame in case["frames"].as_array().expect("frames") {
                assert_eq!(frame["after"]["ring_l"], case["init"]["ring_l"]);
                assert_eq!(frame["after"]["ring_r"], case["init"]["ring_r"]);
                assert_eq!(frame["after"]["sec"], case["init"]["sec"]);
                assert_eq!(frame["after"]["wf"], case["init"]["wf"]);
                assert_eq!(frame["after"]["stored"], frame["x"]);
            }
        }
        assert!(replay("hold") > 0);
    }

    /// Fade set: the inputs are stored, and the write runs on them scaled by `1 - f32[H+0x5808]`.
    #[test]
    fn write_fade_scales_inputs_by_one_minus_amount() {
        let doc = vector("fade");
        for case in doc["cases"].as_array().expect("cases") {
            assert_eq!(int(&case["init"]["fade_on"]), 1);
            for frame in case["frames"].as_array().expect("frames") {
                assert_eq!(frame["after"]["stored"], frame["x"]);
            }
        }
        assert!(replay("fade") > 0);
    }

    /// Mode 0 (the "as is" write): the scaled inputs into the mode-0 ring slots, with hold and fade in every combination.
    #[test]
    fn write_mode0_slots_match_native_vectors() {
        assert!(replay("mode0") > 0);
    }

    /// Modes 2 and 3 are blocked before anything is written.
    #[test]
    fn modes_two_and_three_are_blocked_and_write_nothing() {
        let doc = vector("mode1");
        let init = &doc["cases"][0]["init"];
        for mode in [2, 3] {
            let mut h = head(&doc, init);
            h.mode = mode;
            let mut ring_l = floats(&init["ring_l"]);
            let mut ring_r = floats(&init["ring_r"]);
            let (stored, sec_state, sec_out) = (h.stored, h.sec_state, h.sec_out);
            let wf = h.write_filter[0].probe();
            assert_eq!(
                h.write([0.25, -0.25], [&mut ring_l[..], &mut ring_r[..]]),
                Err(Blocked)
            );
            assert_eq!(h.stored, stored);
            assert_eq!(h.sec_state, sec_state);
            assert_eq!(h.sec_out, sec_out);
            assert_eq!(h.write_filter[0].probe(), wf);
            assert_eq!(ring_l, floats(&init["ring_l"]));
            assert_eq!(ring_r, floats(&init["ring_r"]));
        }
    }
}
