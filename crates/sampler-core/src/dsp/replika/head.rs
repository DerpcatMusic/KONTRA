//! Replika mode-1 head driver (`141ab8c60`, 1240 B): one frame of a head in mode 1, 2 or 3 (`H+0xc0`).
//!
//! The frame, in the native order (decompile `0141ab0000.c` 4937-5104, listing `replika-3b-listings.txt`):
//! 1. the level crossfade (`+0x57c8` towards the target `+0x57d0`), then the trigger (`+0x57e1`): a rebase to the
//!    base source when the target is zero;
//! 2. the modulation: mode 1 and 3 mix the LFO A and B with the glide `+0x57e8`; mode 2 takes the pair at
//!    `+0x5880` and the factor `+0x80` (its speed is clamped at 4). The L and R tap speeds are the speed ratios
//!    `+0x58` and `+0x60` (clamped at 50.5) times (1 + modulation);
//! 3. the two read taps at `x = x_base + base` (`+0x48`), kernel `141ab87d0` (flag 1), scaled by the level;
//!    mode 1 then runs the post filters;
//! 4. the tap positions advance by the speeds (`K+0x00 = idx + frac`, the index wrapped to the mask), and the
//!    two sections `+0x350` and `+0x384` are retuned to 0.8 times the speed (clamped at 0.9999999).
//!
//! The taps are kernel 1 only. The kernel flag `H+0x270` / `H+0x340` is 1 when the lane mode `H+0x8c` is 1..3
//! (`141abbc90` writes it, and index 28 gives lane mode 3); flag 2 (kernel `141ab7fe0`, not ported) is what
//! `141aca4a0` writes for head mode 2, so the native head-mode-2 configuration is not covered by this port. The
//! vectors run mode 2 with flag 1 to check its arithmetic. Flag 0 (kernel `141ab8a00`, void in the text) is not
//! ported either.
//!
//! Not ported: the write-only index and rate history of the taps (`K+0x08`, `+0x10`, `+0x1c`, `+0x20`, `+0x40`,
//! `+0x90..+0xa0`), which no reader in this unit uses; the wrapper `141ab7a90` (the gains `+0x70`, `+0x74` and the
//! mixing) and the caller of the speeds.
//!
//! Vectors: `docs/re/engine/vectors/replika-native-mode1-driver.json` (kemu, 4 cases of 128 frames, modes 1, 2, 3, 1).

use super::delay::DelayPos;
use super::mode1::PostFilter;
use super::read::{Tap, retune};
use crate::dsp::tpt_svf::Section;

/// `1479ab4c0`: the clamp of the speed ratios (50.5).
const SPEED_MAX: f64 = f64::from_bits(0x4049_4000_0000_0000);
/// `1447149a8`: the clamp of the mode-2 speed (4.0).
const MODE2_SPEED_MAX: f64 = f64::from_bits(0x4010_0000_0000_0000);
/// `144714950`: the modulation scale (0.1).
const MOD_SCALE: f64 = f64::from_bits(0x3fb9_9999_9999_999a);
/// `1479ab3b8`: the section speed scale (0.8), and `1479ab3f0`: its clamp (0.9999999).
const SECTION_SCALE: f64 = f64::from_bits(0x3fe9_9999_9999_999a);
const SECTION_MAX: f64 = f64::from_bits(0x3fef_ffff_ca50_1acb);
/// `144e79a60`: the quarter of mode 1's base source (0.25).
const QUARTER: f64 = f64::from_bits(0x3fd0_0000_0000_0000);
/// `1447149f0`: the path index scale (5.0).
const PATH_SCALE: f32 = f32::from_bits(0x40a0_0000);

/// `if c <= x { c } else { x }`: the native clamp (`comisd`, then a move), which keeps `x` on NaN.
fn min_native(c: f64, x: f64) -> f64 {
    if c <= x { c } else { x }
}

/// The read position after one frame: the integer part of `pos + rate` wrapped to the mask, plus the fraction.
fn advance(pos: f64, rate: f64, mask: i32) -> f64 {
    let p = pos + rate;
    let t = p as i32;
    f64::from(t & mask) + (p - f64::from(t))
}

/// The level crossfade of the driver (`H+0x57c0..0x57e1`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fade {
    /// `+0x57c0`: smoothing coefficient.
    pub coef: f64,
    /// `+0x57c8`: the level; `+0x57d0` its target; `+0x57d8` the threshold.
    pub level: f64,
    pub target: f64,
    pub thr: f64,
    /// `+0x57e0`: a settle onto the target is pending; `+0x57e1`: the trigger (a rebase when the target is 0).
    pub pending: bool,
    pub trigger: bool,
}

/// The LFO glide of the driver (`H+0x57e8..0x5805`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glide {
    /// `+0x57e8` current, `+0x57ec` target, `+0x57f0` step, `+0x57f4` threshold, `+0x57fc` count.
    pub cur: f32,
    pub target: f32,
    pub step: f32,
    pub thr: f32,
    pub count: i32,
    /// `+0x5804`: a settle is pending; `+0x5805`: set when the glide settles.
    pub pending: bool,
    pub fresh: bool,
}

/// The modulation sources of one frame: A (`H+0x5868`), B (`H+0x5870`) and the pair at `H+0x5880`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Lfo {
    pub a: f32,
    pub b: f32,
    pub pair: [f32; 2],
}

/// One head in mode 1..3. Field comments give the offsets in the head object H.
#[derive(Clone, Debug)]
pub struct Head {
    /// `+0xc0`: the head mode (2 changes the speed and modulation law, 1 runs the post filters).
    pub mode: i32,
    /// `+0x80` (f32): the mode-2 factor. `+0x98`: s, the modulation depth.
    pub factor: f32,
    pub s: f64,
    /// `+0x50`: the base source, copied to the base `+0x48` on a rebase.
    pub base_src: f64,
    /// `+0x48` base, `+0x58` and `+0x60` speed ratios, `+0xc8` and `+0xd0` delay positions (`141abd0b0`).
    pub delay: DelayPos,
    /// `+0x1a8` (L) and `+0x278` (R): the read taps.
    pub taps: [Tap; 2],
    pub fade: Fade,
    pub glide: Glide,
    /// Mode-1 post filters: L at `+0x56c0`, R at `+0x5724`.
    pub post: [PostFilter; 2],
    /// `+0x350` and `+0x384`: the sections the driver retunes; `rate` is their rate factor (preset).
    pub sec: [Section; 2],
    pub rate: f32,
    /// `+0xa0..+0xac`: the time of each head mode (f32); `141ac4510` takes the one of `mode` as `s`.
    pub time: [f32; 4],
    /// `+0x00`, `+0x38`, `+0x40`: the anchors of the base change (`141abd190`; writers not traced).
    pub anchor: [f64; 3],
}

/// `141aca670`: the Replika path (k) from the raw index-28 value: `min(4, floor(5 v))`. Path k sets the head
/// mode `k` for k = 1, 2, 3 (`141aca4a0`).
pub fn path_for_index(v: f32) -> i32 {
    ((v * PATH_SCALE).floor() as i32).min(4)
}

impl Head {
    /// One frame. `rings` are the L and R tap rings (`mask + 1` samples each). Returns the L and R outputs.
    pub fn drive(&mut self, lfo: Lfo, rings: [&mut [f32]; 2]) -> [f32; 2] {
        let [ring_l, ring_r] = rings;

        // Level crossfade: settle on the target inside the threshold, else step towards it.
        let gap = (self.fade.target - self.fade.level).abs();
        if gap <= self.fade.thr {
            if self.fade.pending {
                self.fade.level = self.fade.target;
                self.fade.pending = false;
                self.fade.trigger = true;
            }
        } else {
            self.fade.level =
                (1.0 - self.fade.coef) * self.fade.target + self.fade.coef * self.fade.level;
        }

        // Trigger: clear it, and with a zero target rebase onto the base source.
        if self.fade.trigger {
            self.fade.trigger = false;
            if self.fade.target == 0.0 {
                self.delay.base = self.base_src;
                self.fade.target = 1.0;
                if self.fade.level != 1.0 {
                    self.fade.pending = true;
                }
                self.delay.r58 = self.delay.base / self.delay.a;
                self.delay.r60 = self.delay.base / self.delay.b;
            }
        }

        // Modulation of the speeds and the x offset of the taps.
        let (x_base, mod_l, mod_r) = if self.mode == 2 {
            let p = lfo.pair;
            let m = f64::from(p[0]) * self.s * MOD_SCALE;
            let speed = min_native(MODE2_SPEED_MAX, self.delay.r58);
            let x = f64::from((self.factor + self.factor) * p[1]) * speed;
            (x, m, m)
        } else {
            self.glide_step();
            let s = self.s * MOD_SCALE;
            let g = self.glide.cur;
            let a = f64::from(lfo.a);
            let mod_l = s * a;
            let mod_r = (f64::from(lfo.b) * f64::from(g) + f64::from(1.0 - g) * a) * s;
            (1.0, mod_l, mod_r)
        };
        let rate_l = min_native(SPEED_MAX, self.delay.r58) * (mod_l + 1.0);
        let rate_r = min_native(SPEED_MAX, self.delay.r60) * (mod_r + 1.0);

        // Taps: x = x_base + base, scaled by the level. The offset of each tap is this frame's speed.
        let x = x_base + self.delay.base;
        let level = self.fade.level as f32;
        let mut y = [
            level * self.taps[0].read(ring_l, x),
            level * self.taps[1].read(ring_r, x),
        ];
        self.taps[0].offset = rate_l;
        self.taps[1].offset = rate_r;

        if self.mode == 1 {
            y[0] = self.post[0].tick(y[0]);
            y[1] = self.post[1].tick(y[1]);
        }

        self.taps[0].pos = advance(self.taps[0].pos, rate_l, self.taps[0].mask);
        self.taps[1].pos = advance(self.taps[1].pos, rate_r, self.taps[1].mask);

        let sec_l = min_native(SECTION_MAX, rate_l * SECTION_SCALE) as f32;
        let sec_r = min_native(SECTION_MAX, rate_r * SECTION_SCALE) as f32;
        self.sec[0] = retune(self.sec[0], sec_l, self.rate);
        self.sec[1] = retune(self.sec[1], sec_r, self.rate);
        y
    }

    /// `141ac4510`: `s` (`+0x98`) = the time of the head mode (`+0xa0 + 4 mode`, modes 0..3). Other modes keep `s`.
    pub fn sample_time(&mut self) {
        if let Some(&t) = usize::try_from(self.mode)
            .ok()
            .and_then(|m| self.time.get(m))
        {
            self.s = f64::from(t);
        }
    }

    /// `141abd190`: the base change request. The source `+0x50` is anchor-dependent (mode 1: `a0 * 0.25 * a38`,
    /// mode 2: `a0`, mode 3: `a40 * a0`), clamped at 1. The target goes to 0 (the level fades out). When the level
    /// is nonzero a settle is pending and the trigger is cleared, so the next rebase runs from `drive`.
    pub fn rebase_request(&mut self) {
        let [a0, a38, a40] = self.anchor;
        match self.mode {
            1 => self.base_src = a0 * QUARTER * a38,
            2 => self.base_src = a0,
            3 => self.base_src = a40 * a0,
            _ => {}
        }
        if self.base_src <= 1.0 {
            self.base_src = 1.0;
        }
        self.fade.target = 0.0;
        if self.fade.level != 0.0 {
            self.fade.pending = true;
            self.fade.trigger = false;
        }
    }

    /// The per-head glide of the LFO mix (`+0x57e8`): settle when the gap is inside the threshold or the count runs out,
    /// else step by `+0x57f0`.
    fn glide_step(&mut self) {
        let g = &mut self.glide;
        let gap = (g.target - g.cur).abs();
        let settled = gap <= g.thr || {
            g.count -= 1;
            g.count < 1
        };
        if !settled {
            g.cur += g.step;
        } else if g.pending {
            g.cur = g.target;
            g.step = 0.0;
            g.count = 0;
            g.fresh = true;
            g.pending = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::tpt_svf::State;
    use serde_json::Value;

    fn vector() -> Value {
        let path = format!(
            "{}/../../docs/re/engine/vectors/replika-native-mode1-driver.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(path).expect("vector file"))
            .expect("vector json")
    }

    fn f(v: &Value, key: &str) -> f64 {
        v[key].as_f64().unwrap_or_else(|| panic!("field {key}"))
    }

    fn head_from_init(init: &Value, rate: f32) -> Head {
        let kernel = |p: &str| {
            let mut t = Tap::new(
                f(init, &format!("{p}_pos")),
                f(init, &format!("{p}_mask")) as i32,
                f(init, &format!("{p}_phase")),
                f(init, &format!("{p}_idx")) as i32,
                f(init, &format!("{p}_xprev")),
                f(init, &format!("{p}_offset")),
                f(init, &format!("{p}_damp")) as f32,
                f(init, &format!("{p}_rate")) as f32,
            );
            t.state = State {
                s0: f(init, &format!("{p}_s0")) as f32,
                s1: f(init, &format!("{p}_s1")) as f32,
            };
            t
        };
        let post = |p: &str| PostFilter {
            env: f(init, &format!("{p}_env")) as f32,
            coef: f(init, &format!("{p}_coef")) as f32,
            s0: f(init, &format!("{p}_s0")) as f32,
            s1: f(init, &format!("{p}_s1")) as f32,
            c2: f(init, &format!("{p}_c2")) as f32,
            c3: f(init, &format!("{p}_c3")) as f32,
            c4: f(init, &format!("{p}_c4")) as f32,
            gain: f(init, &format!("{p}_gain")) as f32,
        };
        Head {
            mode: f(init, "mode") as i32,
            factor: f(init, "factor") as f32,
            s: f(init, "s"),
            base_src: f(init, "base_src"),
            delay: DelayPos {
                lo: 0,
                hi: 0,
                mode: 0,
                base: f(init, "delay_base"),
                r58: f(init, "r58"),
                r60: f(init, "r60"),
                a: f(init, "a"),
                b: f(init, "b"),
                flag: false,
            },
            taps: [kernel("k1"), kernel("k2")],
            fade: Fade {
                coef: f(init, "f_coef"),
                level: f(init, "f_level"),
                target: f(init, "f_target"),
                thr: f(init, "f_thr"),
                pending: f(init, "f_pend") != 0.0,
                trigger: f(init, "f_trig") != 0.0,
            },
            glide: Glide {
                cur: f(init, "g_cur") as f32,
                target: f(init, "g_tgt") as f32,
                step: f(init, "g_step") as f32,
                thr: f(init, "g_thr") as f32,
                count: f(init, "g_count") as i32,
                pending: f(init, "g_pend") != 0.0,
                fresh: f(init, "g_fresh") != 0.0,
            },
            post: [post("pl"), post("pr")],
            sec: [
                Section::new(0., f(init, "sl_k") as f32),
                Section::new(0., f(init, "sr_k") as f32),
            ],
            rate,
            time: [0.; 4],
            anchor: [0.; 3],
        }
    }

    fn words(v: &Value) -> Vec<f32> {
        v.as_array()
            .expect("ring")
            .iter()
            .map(|x| x.as_f64().expect("sample") as f32)
            .collect()
    }

    /// Relative to the native value, with a floor for the values near zero (the lowpassed rings).
    fn close(label: &str, got: f64, want: f64, rel: f64) {
        let tol = rel * want.abs().max(1e-12);
        assert!((got - want).abs() <= tol, "{label}: got {got} want {want}");
    }

    /// The f32 integrators and section coefficients: the native TPT update cancels (`s1' = X - s1`), so single
    /// rounding differences (about 4 ulp per frame at the 0.05 level) are the floor, not a logic error.
    fn close_f32(label: &str, got: f64, want: f64) {
        let tol = 1e-5 * want.abs().max(1e-3);
        assert!((got - want).abs() <= tol, "{label}: got {got} want {want}");
    }

    #[test]
    fn driver_matches_native_frames() {
        let doc = vector();
        for case in doc["cases"].as_array().expect("cases") {
            let init = &case["init"];
            let rate = f(init, "k1_rate") as f32;
            let mut head = head_from_init(init, rate);
            let mut ring_l = words(&case["ring_l"]);
            let mut ring_r = words(&case["ring_r"]);
            let tag = format!("seed {} mode {}", case["seed"], case["mode"]);
            // The outputs are checked against the case's peak level: the rings are lowpassed to tiny values at
            // the 1/fs rate factor, so a per-sample relative bound would test the floating-point noise.
            let frames = case["frames"].as_array().expect("frames");
            let peak = frames
                .iter()
                .flat_map(|fr| [f(&fr["state"], "outL").abs(), f(&fr["state"], "outR").abs()])
                .fold(0.0_f64, f64::max);

            for (n, frame) in frames.iter().enumerate() {
                let inp = &frame["in"];
                let st = &frame["state"];
                head.s = f(inp, "s");
                head.base_src = f(inp, "base_src");
                head.delay.a = f(inp, "c8");
                head.delay.b = f(inp, "d0");
                head.fade.target = f(inp, "target");
                head.fade.trigger = f(inp, "trig") != 0.0;
                head.glide.target = f(inp, "gtgt") as f32;
                let pair = inp["pair"].as_array().expect("pair");
                let lfo = Lfo {
                    a: f(inp, "a") as f32,
                    b: f(inp, "b") as f32,
                    pair: [
                        pair[0].as_f64().unwrap() as f32,
                        pair[1].as_f64().unwrap() as f32,
                    ],
                };
                let [l, r] = head.drive(lfo, [ring_l.as_mut_slice(), ring_r.as_mut_slice()]);
                let at = format!("{tag} frame {n}");
                assert!(
                    (f64::from(l) - f(st, "outL")).abs() <= 1e-5 * peak,
                    "{at} outL: got {l} want {}",
                    f(st, "outL")
                );
                assert!(
                    (f64::from(r) - f(st, "outR")).abs() <= 1e-5 * peak,
                    "{at} outR: got {r} want {}",
                    f(st, "outR")
                );
                close(
                    &format!("{at} k1_pos"),
                    head.taps[0].pos,
                    f(st, "k1_pos"),
                    1e-9,
                );
                close(
                    &format!("{at} k2_pos"),
                    head.taps[1].pos,
                    f(st, "k2_pos"),
                    1e-9,
                );
                close(
                    &format!("{at} k1_offset"),
                    head.taps[0].offset,
                    f(st, "k1_offset"),
                    1e-9,
                );
                close(
                    &format!("{at} k2_offset"),
                    head.taps[1].offset,
                    f(st, "k2_offset"),
                    1e-9,
                );
                close_f32(
                    &format!("{at} k1_s0"),
                    f64::from(head.taps[0].state.s0),
                    f(st, "k1_s0"),
                );
                close_f32(
                    &format!("{at} k1_s1"),
                    f64::from(head.taps[0].state.s1),
                    f(st, "k1_s1"),
                );
                close_f32(
                    &format!("{at} k2_s0"),
                    f64::from(head.taps[1].state.s0),
                    f(st, "k2_s0"),
                );
                close_f32(
                    &format!("{at} k2_s1"),
                    f64::from(head.taps[1].state.s1),
                    f(st, "k2_s1"),
                );
                close(
                    &format!("{at} level"),
                    head.fade.level,
                    f(st, "f_level"),
                    1e-9,
                );
                close(
                    &format!("{at} target"),
                    head.fade.target,
                    f(st, "f_target"),
                    1e-9,
                );
                assert_eq!(head.fade.pending, f(st, "f_pend") != 0.0, "{at} pending");
                assert_eq!(head.fade.trigger, f(st, "f_trig") != 0.0, "{at} trigger");
                close_f32(
                    &format!("{at} g_cur"),
                    f64::from(head.glide.cur),
                    f(st, "g_cur"),
                );
                assert_eq!(
                    i64::from(head.glide.count),
                    f(st, "g_count") as i64,
                    "{at} count"
                );
                assert_eq!(
                    head.glide.pending,
                    f(st, "g_pend") != 0.0,
                    "{at} glide pending"
                );
                assert_eq!(
                    head.glide.fresh,
                    f(st, "g_fresh") != 0.0,
                    "{at} glide fresh"
                );
                close(
                    &format!("{at} base"),
                    head.delay.base,
                    f(st, "delay_base"),
                    1e-9,
                );
                close(&format!("{at} r58"), head.delay.r58, f(st, "r58"), 1e-9);
                close(&format!("{at} r60"), head.delay.r60, f(st, "r60"), 1e-9);
                close_f32(
                    &format!("{at} pl_s0"),
                    f64::from(head.post[0].s0),
                    f(st, "pl_s0"),
                );
                close_f32(
                    &format!("{at} pr_s1"),
                    f64::from(head.post[1].s1),
                    f(st, "pr_s1"),
                );
                for (i, p) in ["sl", "sr"].iter().enumerate() {
                    let (c2, c3, c4, _) = head.sec[i].parts();
                    close_f32(
                        &format!("{at} {p}_c2"),
                        f64::from(c2),
                        f(st, &format!("{p}_c2")),
                    );
                    close_f32(
                        &format!("{at} {p}_c3"),
                        f64::from(c3),
                        f(st, &format!("{p}_c3")),
                    );
                    close_f32(
                        &format!("{at} {p}_c4"),
                        f64::from(c4),
                        f(st, &format!("{p}_c4")),
                    );
                }
            }
        }
    }

    fn units_vector() -> Value {
        let path = format!(
            "{}/../../docs/re/engine/vectors/replika-native-head-units.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(path).expect("vector file"))
            .expect("vector json")
    }

    #[test]
    fn sample_time_and_rebase_match_native_vectors() {
        let doc = units_vector();
        let rate = 1.0 / 48_000.0;
        let driver = vector();
        let init = &driver["cases"][0]["init"];
        for case in doc["sample_time"].as_array().expect("sample_time") {
            let mut head = head_from_init(init, rate);
            head.mode = f(case, "mode") as i32;
            head.s = f(case, "before");
            let times = case["times"].as_array().expect("times");
            for (k, t) in times.iter().enumerate() {
                head.time[k] = t.as_f64().unwrap() as f32;
            }
            head.sample_time();
            assert_eq!(head.s, f(case, "s"), "sample_time mode {}", head.mode);
        }
        for case in doc["rebase"].as_array().expect("rebase") {
            let mut head = head_from_init(init, rate);
            head.mode = f(case, "mode") as i32;
            let anchor = case["anchor"].as_array().expect("anchor");
            for (k, a) in anchor.iter().enumerate() {
                head.anchor[k] = a.as_f64().unwrap();
            }
            head.base_src = f(case, "base_src_in");
            head.fade.level = f(case, "level");
            head.fade.target = f(case, "target_in");
            head.fade.pending = f(case, "pend_in") != 0.0;
            head.fade.trigger = f(case, "trig_in") != 0.0;
            head.rebase_request();
            let tag = format!("rebase mode {}", head.mode);
            close(
                &format!("{tag} src"),
                head.base_src,
                f(case, "base_src"),
                1e-12,
            );
            close(
                &format!("{tag} target"),
                head.fade.target,
                f(case, "target"),
                1e-12,
            );
            assert_eq!(head.fade.pending, f(case, "pend") != 0.0, "{tag} pending");
            assert_eq!(head.fade.trigger, f(case, "trig") != 0.0, "{tag} trigger");
        }
    }

    #[test]
    fn path_is_the_index_times_five_clamped_at_four() {
        let cases = [
            (0.0, 0),
            (0.19, 0),
            (0.2, 1),
            (0.39, 1),
            (0.4, 2),
            (0.6, 3),
            (0.79, 3),
            (0.8, 4),
            (1.0, 4),
        ];
        for (v, want) in cases {
            assert_eq!(path_for_index(v as f32), want, "index {v}");
        }
    }
}
