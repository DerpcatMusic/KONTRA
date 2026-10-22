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
//! The tap object also keeps the driver's write side (`K+0x08` index, `+0x10` fraction, `+0x1c` step, `+0x20` and
//! `+0x40` of the previous frame, `+0x90..+0xa0` the rate history): the deposit `141accfb0` reads them as its
//! index, offset, count and rates, so they are part of this port, and the deposit's history `+0x84..+0x8c` is `Tap::hist`.
//! The write stage `141acd110` is in `write.rs`. Not ported: the wrapper `141ab7a90` (the gains `+0x70`, `+0x74` and the mixing) and the caller of
//! the speeds. The mode writer `141aca4a0` (k = 0, 1, 2, 4, `Head::set_mode`) is ported; its k = 3 is blocked.
//! The index-28 selector `141aca670` (`Heads::select`) applies the mode writers to the three heads and the
//! selector's own state effects that are modelled (see there).
//!
//! Vectors: `docs/re/engine/vectors/replika-native-mode1-driver.json` (kemu, 4 cases of 128 frames, modes 1, 2, 3, 1).

use super::ReplikaState;
use super::delay::DelayPos;
use super::mode1::{PostFilter, WriteFilter};
use super::read::{Tap, retune};
use crate::dsp::tpt_svf::{Section, State};

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

/// The tap position after one frame (`K+0x00`, `K+0x08..0x20`, `K+0x40`, `K+0x90..0xa0`): `t = trunc(pos + rate)`,
/// the index `t & mask` and the fraction. The old index and fraction, the step `t - old index` and the rate history
/// shift in the order of the native stores.
fn advance(tap: &mut Tap, rate: f64) {
    let p = tap.pos + rate;
    let t = p as i32;
    tap.prev_frac = tap.frac;
    tap.prev_wr = tap.wr;
    tap.delta = t.wrapping_sub(tap.wr);
    tap.wr = t & tap.mask;
    tap.frac = p - f64::from(t);
    tap.pos = f64::from(tap.wr) + tap.frac;
    tap.rates = [rate, tap.rates[0], tap.rates[1]];
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
    /// `+0x57f8`: the glide length, the divisor of the step. `+0x5800`: the count a new glide starts from.
    pub div: f32,
    pub count_init: i32,
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
    /// `+0x8c`: the lane mode. `141abbc90` takes it, and its tail sets the kernel flags from it (k = 1).
    pub lane: i32,
    /// `+0x68`, `+0x69`, `+0x6a`: the glide targets of head modes 0, 1 and 3 (set: 1.0, clear: 0.0).
    pub mode_flags: [bool; 3],
    /// `+0x270` (L) and `+0x340` (R): the kernel flags, which are the taps' `+0xc8` (1 = kernel 1, 2 = kernel 2).
    pub kernel: [i32; 2],
    /// `+0x158` (L) and `+0x194` (R): the mode-0 ring write index; `+0x15c` (L) and `+0x190` (R) its wrap
    /// (`141acd110`; the writers are not traced). The R pair is read in the mirrored order, see `write.rs`.
    pub ring_pos: [i32; 2],
    pub ring_wrap: [i32; 2],
    /// `+0x6c`: hold. With the fade clear the write stage stores its inputs and writes nothing.
    pub hold: bool,
    /// `+0x5824`: the fade is on (`141acc600`, not ported); `+0x5808`: its amount, the inputs scale by `1 - amount`.
    pub fade_on: bool,
    pub fade_amount: f32,
    /// `+0x348` (L) and `+0x34c` (R): the inputs of the last write that stored them (hold or fade).
    pub stored: [f32; 2],
    /// `+0x2dc0` (L) and `+0x4240` (R): the mode-1 write filters (`141aa3c60`).
    pub write_filter: [WriteFilter; 2],
    /// `+0x350` and `+0x384`: the state of the write sections; their coefficients are `sec`.
    pub sec_state: [State; 2],
    /// `+0x368` (L) and `+0x39c` (R): the lowpass output of the write sections, the deposit's input.
    pub sec_out: [f32; 2],
}

/// `141aca4a0` with k = 3 calls `141acb100`, which is not ported. Returned before anything is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Blocked;

/// The glide target of a mode flag: 1.0 when set, else 0.0 (the byte test of `141aca4a0`).
fn flag_target(on: bool) -> f32 {
    if on { 1.0 } else { 0.0 }
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

        advance(&mut self.taps[0], rate_l);
        advance(&mut self.taps[1], rate_r);

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

    /// `141aca4a0(H, k, flag)`: the head mode writer for k = 0, 1, 2 and 4 (other k only set the mode). `rings` are the
    /// L and R rings behind the taps' handles (`K+0xc0`, the same as `**(H+0x150)` and `**(H+0x188)`); with `flag` the
    /// taps clear them (`1407e6d80`). The tail is `rebase_request` and `sample_time`. k = 3 calls the unported
    /// `141acb100`: it returns `Err(Blocked)` and changes nothing.
    ///
    /// The design chain `141abbc90` (k = 1: lane and post coefficients) is not applied here; only its tail is, the
    /// kernel flags from the lane mode and the tap resets.
    pub fn set_mode(&mut self, k: i32, flag: bool, rings: [&mut [f32]; 2]) -> Result<(), Blocked> {
        if k == 3 {
            return Err(Blocked);
        }
        self.mode = k;
        let [ring_l, ring_r] = rings;
        match k {
            0 => self.set_glide(flag_target(self.mode_flags[0])),
            1 => {
                self.kernel = [if (1..=3).contains(&self.lane) { 1 } else { 2 }; 2];
                self.reset_taps(flag, ring_l, ring_r);
                self.set_glide(flag_target(self.mode_flags[1]));
            }
            2 => {
                self.kernel = [2; 2];
                self.reset_taps(flag, ring_l, ring_r);
            }
            _ => {}
        }
        self.rebase_request();
        self.sample_time();
        Ok(())
    }

    /// The tap resets of `141aca4a0`: `141abaab0` on both taps, or with `flag` `141aba040(K, 1)` (the reset and the
    /// ring clear; it contains the `141abaab0` part).
    fn reset_taps(&mut self, flag: bool, ring_l: &mut [f32], ring_r: &mut [f32]) {
        if flag {
            self.taps[0].reset_flagged(true, ring_l);
            self.taps[1].reset_flagged(true, ring_r);
        } else {
            self.taps.iter_mut().for_each(Tap::reset_rates);
        }
    }

    /// The glide retarget of `141aca4a0`: `+0x57ec` = target. When it differs from the current `+0x57e8`, the step is
    /// `(target - cur) / +0x57f8`, the count restarts at `+0x5800` and the settle flags are set (`+0x5804` = 1, `+0x5805` = 0).
    fn set_glide(&mut self, target: f32) {
        let g = &mut self.glide;
        g.target = target;
        if target == g.cur {
            return;
        }
        g.count = g.count_init;
        g.step = (target - g.cur) / g.div;
        g.pending = true;
        g.fresh = false;
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

    /// The part of the head reset `141aba100` (run by `141aba800` with flag 1) that the port takes: both taps reset
    /// (`141aba040` with flag 0, [`Tap::reset`]; the ring clear of flag 1 is not ported), then a pending settle lands on
    /// the target now (`+0x57c8` = `+0x57d0`, the pending byte cleared, the trigger `+0x57e1` set). The rest of
    /// `141aba100` is not ported (see [`Heads::select`]).
    fn reset_taps_and_settle(&mut self) {
        self.taps.iter_mut().for_each(Tap::reset);
        if self.fade.pending {
            self.fade.level = self.fade.target;
            self.fade.pending = false;
            self.fade.trigger = true;
        }
    }
}

/// The mode-1 heads (`C+0xac0`, `C+0x6380`, `C+0xbc40`, in this order) and the path the selector last applied:
/// `C+0x830`, and `C+0x604` is the same value. Not in the state slab yet (the heads are not packed).
#[derive(Clone, Debug)]
pub struct Heads {
    /// `C+0x830`. The ctor and prepare leave `0x7fffffff` (measured under kemu), so the first selector call changes the path.
    pub path: i32,
    pub heads: [Head; 3],
}

impl Heads {
    /// `141aca670`, the index-28 selector, for the raw value `v`. Ported, in native order: the path guard (an unchanged
    /// path does nothing), the mode writer `141aca4a0` on each head with flag 0 (`Head::set_mode`; mode k for k = 1, 2,
    /// else 0), then `141aba800` and `141aba680`: each head's taps reset and pending fade settle (the parts of `141aba100`
    /// the port takes), the ducker envelope goes to 0 dB, and the saturator section states to 0.
    ///
    /// Path 3 returns `Err(Blocked)` before anything is written (its mode writer calls the unported `141acb100`).
    ///
    /// Not ported. The native selector writes these too; `changed_bytes` in `replika-native-selector28-*.json` has the
    /// byte runs. The bypass fade `141acc600` (its blocks at `H+0x5808`, `P3+0x60`) changes nothing with the default
    /// enables, so the vectors do not exercise it. The rest of `141aba100` (the ring clear `1407e6d80` of flag 1, sections
    /// `+0x350`/`+0x384`, post filters, lane banks, the words `+0x158`, `+0x190`, `+0x5788..+0x57b0`). The delay caller
    /// `141abd290` (`H+0x58`/`+0x60` speed ratios, `+0xc8`/`+0xd0` positions, the `+0x90` flag, the words `+0xd8..+0xf0`),
    /// with its path callees `141aca380` (path 2, the block `C+0x9a0`) and `141acb160` (path 3, `C+0x14198`). The rotator at
    /// `C+0x980..0x997` (`141aca670` itself, and `141aba700`), the post table `C+0x944..0x967` (path 1), the table
    /// `C+0x14208..` and the flags `C+0x11504`, `C+0x12984` (path 4), the ducking flag `C+0x938`, and `141aba570`.
    /// The lane and post coefficients of `141abbc90` (k = 1) also stay with the mode writer's design chain (not applied).
    pub fn select(&mut self, st: &mut ReplikaState, v: f32) -> Result<(), Blocked> {
        let k = path_for_index(v);
        if k == self.path {
            return Ok(());
        }
        if k == 3 {
            return Err(Blocked);
        }
        self.path = k;
        let mode = if (1..=3).contains(&k) { k } else { 0 };
        for h in &mut self.heads {
            // Flag 0: the taps keep their rings (`reset_taps` then touches only the rates).
            h.set_mode(mode, false, [&mut [], &mut []])
                .expect("modes 0, 1, 2 and 4 are ported");
        }
        for h in &mut self.heads {
            h.reset_taps_and_settle();
        }
        st.env = 0.;
        st.saturator = Default::default();
        Ok(())
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
            t.wr = f(init, &format!("{p}_wr")) as i32;
            t.frac = f(init, &format!("{p}_frac"));
            t.delta = f(init, &format!("{p}_delta")) as i32;
            t.prev_wr = f(init, &format!("{p}_pwr")) as i32;
            t.prev_frac = f(init, &format!("{p}_pfrac"));
            t.rates = [0, 1, 2].map(|k| f(init, &format!("{p}_rate{k}")));
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
                div: 1.0,
                count_init: 0,
            },
            post: [post("pl"), post("pr")],
            sec: [
                Section::new(0., f(init, "sl_k") as f32),
                Section::new(0., f(init, "sr_k") as f32),
            ],
            rate,
            time: [0.; 4],
            anchor: [0.; 3],
            lane: 0,
            mode_flags: [false; 3],
            kernel: [1, 1],
            ring_pos: [0; 2],
            ring_wrap: [0; 2],
            hold: false,
            fade_on: false,
            fade_amount: 0.,
            stored: [0.; 2],
            write_filter: idle_write_filters(),
            sec_state: [State::default(); 2],
            sec_out: [0.; 2],
        }
    }

    /// Write filters with an all-zero lane object (the tests that do not run the write stage).
    fn idle_write_filters() -> [WriteFilter; 2] {
        [
            WriteFilter::from_object(&[0; 1312]),
            WriteFilter::from_object(&[0; 1312]),
        ]
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
                for (i, p) in ["k1", "k2"].iter().enumerate() {
                    let t = &head.taps[i];
                    assert_eq!(
                        i64::from(t.wr),
                        f(st, &format!("{p}_wr")) as i64,
                        "{at} {p}_wr"
                    );
                    assert_eq!(
                        i64::from(t.delta),
                        f(st, &format!("{p}_delta")) as i64,
                        "{at} {p}_delta"
                    );
                    assert_eq!(
                        i64::from(t.prev_wr),
                        f(st, &format!("{p}_pwr")) as i64,
                        "{at} {p}_pwr"
                    );
                    assert!(
                        (t.frac - f(st, &format!("{p}_frac"))).abs() <= 1e-10,
                        "{at} {p}_frac"
                    );
                    assert!(
                        (t.prev_frac - f(st, &format!("{p}_pfrac"))).abs() <= 1e-10,
                        "{at} {p}_pfrac"
                    );
                    for k in 0..3 {
                        close(
                            &format!("{at} {p}_rate{k}"),
                            t.rates[k],
                            f(st, &format!("{p}_rate{k}")),
                            1e-9,
                        );
                    }
                }
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

    fn mode_vector() -> Value {
        let path = format!(
            "{}/../../docs/re/engine/vectors/replika-native-mode-writer.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(path).expect("vector file"))
            .expect("vector json")
    }

    /// The head of a mode-writer case: the fields the writer, its tail and the taps' resets read or write.
    fn mode_head(init: &Value) -> Head {
        let tap = |p: &str| {
            let mut t = Tap::new(
                f(init, &format!("{p}_pos")),
                0,
                f(init, &format!("{p}_phase")),
                f(init, &format!("{p}_idx")) as i32,
                f(init, &format!("{p}_xprev")),
                f(init, &format!("{p}_offset")),
                0.0,
                1.0,
            );
            t.wr = f(init, &format!("{p}_wr")) as i32;
            t.frac = f(init, &format!("{p}_frac"));
            t.delta = f(init, &format!("{p}_delta")) as i32;
            t.prev_wr = f(init, &format!("{p}_pwr")) as i32;
            t.prev_frac = f(init, &format!("{p}_pfrac"));
            t.rates = [0, 1, 2].map(|k| f(init, &format!("{p}_rate{k}")));
            t.state = State {
                s0: f(init, &format!("{p}_s0")) as f32,
                s1: f(init, &format!("{p}_s1")) as f32,
            };
            t
        };
        Head {
            mode: f(init, "mode") as i32,
            factor: 0.,
            s: f(init, "s"),
            base_src: f(init, "base_src"),
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
            taps: [tap("L"), tap("R")],
            fade: Fade {
                coef: 0.,
                level: f(init, "level"),
                target: f(init, "fade_target"),
                thr: 0.,
                pending: f(init, "pend") != 0.0,
                trigger: f(init, "trig") != 0.0,
            },
            glide: Glide {
                cur: f(init, "g_cur") as f32,
                target: f(init, "g_target") as f32,
                step: f(init, "g_step") as f32,
                thr: 0.,
                count: f(init, "g_count") as i32,
                pending: f(init, "g_pend") != 0.0,
                fresh: f(init, "g_fresh") != 0.0,
                div: f(init, "g_div") as f32,
                count_init: f(init, "g_count_init") as i32,
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
            sec: [Section::new(0., 0.); 2],
            rate: 1.,
            time: ["t0", "t1", "t2", "t3"].map(|k| f(init, k) as f32),
            anchor: [f(init, "a0"), f(init, "a38"), f(init, "a40")],
            lane: f(init, "lane") as i32,
            mode_flags: ["flag68", "flag69", "flag6a"].map(|k| f(init, k) != 0.0),
            kernel: [f(init, "kernel_l") as i32, f(init, "kernel_r") as i32],
            ring_pos: [0; 2],
            ring_wrap: [0; 2],
            hold: false,
            fade_on: false,
            fade_amount: 0.,
            stored: [0.; 2],
            write_filter: idle_write_filters(),
            sec_state: [State::default(); 2],
            sec_out: [0.; 2],
        }
    }

    /// `141aca4a0` for k = 0, 1, 2, 4 with flag 0 and 1 (kemu): the head fields, the taps and the two rings after the
    /// call. Floats within 1e-5, integers and flags exact.
    #[test]
    fn mode_writer_matches_native_vectors() {
        let doc = mode_vector();
        for case in doc["cases"].as_array().expect("cases") {
            let k = f(case, "k") as i32;
            let flag = f(case, "flag") != 0.0;
            let (init, end) = (&case["init"], &case["end"]);
            let mut h = mode_head(init);
            let mut ring_l = words(&init["ring_l"]);
            let mut ring_r = words(&init["ring_r"]);
            h.set_mode(k, flag, [&mut ring_l[..], &mut ring_r[..]])
                .expect("k 0, 1, 2 and 4 are ported");
            let tag = format!("k {k} flag {flag} seed {}", f(case, "seed"));
            assert_eq!(h.mode as f64, f(end, "mode"), "{tag} mode");
            assert_eq!(h.kernel[0] as f64, f(end, "kernel_l"), "{tag} kernel L");
            assert_eq!(h.kernel[1] as f64, f(end, "kernel_r"), "{tag} kernel R");
            let g = &h.glide;
            close_f32(&format!("{tag} glide cur"), g.cur as f64, f(end, "g_cur"));
            close_f32(
                &format!("{tag} glide target"),
                g.target as f64,
                f(end, "g_target"),
            );
            close_f32(
                &format!("{tag} glide step"),
                g.step as f64,
                f(end, "g_step"),
            );
            assert_eq!(g.count as f64, f(end, "g_count"), "{tag} glide count");
            assert_eq!(
                g.count_init as f64,
                f(end, "g_count_init"),
                "{tag} glide count init"
            );
            assert_eq!(g.pending, f(end, "g_pend") != 0.0, "{tag} glide pending");
            assert_eq!(g.fresh, f(end, "g_fresh") != 0.0, "{tag} glide fresh");
            close_f32(&format!("{tag} base"), h.base_src, f(end, "base_src"));
            close_f32(&format!("{tag} level"), h.fade.level, f(end, "level"));
            close_f32(
                &format!("{tag} target"),
                h.fade.target,
                f(end, "fade_target"),
            );
            assert_eq!(h.fade.pending, f(end, "pend") != 0.0, "{tag} pend");
            assert_eq!(h.fade.trigger, f(end, "trig") != 0.0, "{tag} trig");
            close_f32(&format!("{tag} s"), h.s, f(end, "s"));
            for (side, tap) in [("L", &h.taps[0]), ("R", &h.taps[1])] {
                let key = |name: &str| format!("{side}_{name}");
                close_f32(&format!("{tag} {side} pos"), tap.pos, f(end, &key("pos")));
                close_f32(
                    &format!("{tag} {side} frac"),
                    tap.frac,
                    f(end, &key("frac")),
                );
                close_f32(
                    &format!("{tag} {side} phase"),
                    tap.phase,
                    f(end, &key("phase")),
                );
                close_f32(
                    &format!("{tag} {side} xprev"),
                    tap.x_prev,
                    f(end, &key("xprev")),
                );
                close_f32(
                    &format!("{tag} {side} pfrac"),
                    tap.prev_frac,
                    f(end, &key("pfrac")),
                );
                close_f32(
                    &format!("{tag} {side} offset"),
                    tap.offset,
                    f(end, &key("offset")),
                );
                assert_eq!(tap.wr as f64, f(end, &key("wr")), "{tag} {side} wr");
                assert_eq!(
                    tap.delta as f64,
                    f(end, &key("delta")),
                    "{tag} {side} delta"
                );
                assert_eq!(tap.prev_wr as f64, f(end, &key("pwr")), "{tag} {side} pwr");
                assert_eq!(tap.idx as f64, f(end, &key("idx")), "{tag} {side} idx");
                close_f32(
                    &format!("{tag} {side} s0"),
                    tap.state.s0 as f64,
                    f(end, &key("s0")),
                );
                close_f32(
                    &format!("{tag} {side} s1"),
                    tap.state.s1 as f64,
                    f(end, &key("s1")),
                );
                for (i, name) in ["rate0", "rate1", "rate2"].iter().enumerate() {
                    close_f32(
                        &format!("{tag} {side} {name}"),
                        tap.rates[i],
                        f(end, &key(name)),
                    );
                }
            }
            assert_eq!(ring_l, words(&end["ring_l"]), "{tag} ring L");
            assert_eq!(ring_r, words(&end["ring_r"]), "{tag} ring R");
        }
    }

    /// k = 3 calls the unported `141acb100`: the writer returns `Blocked` and the head and rings stay as they were.
    #[test]
    fn mode_three_is_blocked_and_writes_nothing() {
        let doc = mode_vector();
        let case = &doc["cases"][0];
        let mut h = mode_head(&case["init"]);
        let before = h.clone();
        let mut ring_l = words(&case["init"]["ring_l"]);
        let mut ring_r = words(&case["init"]["ring_r"]);
        let r = h.set_mode(3, true, [&mut ring_l[..], &mut ring_r[..]]);
        assert_eq!(r, Err(Blocked));
        assert_eq!(h.mode, before.mode);
        assert_eq!(h.glide, before.glide);
        assert_eq!(h.kernel, before.kernel);
        assert_eq!(ring_l, words(&case["init"]["ring_l"]));
    }

    fn selector_vector(name: &str) -> Value {
        let path = format!(
            "{}/../../docs/re/engine/vectors/replika-native-selector28-{name}.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(path).expect("vector file"))
            .expect("vector json")
    }

    /// The selector's state from a vector's `init`: the path, the three heads, the ducker and the saturators.
    fn selector_state(init: &Value) -> (Heads, ReplikaState) {
        let core = &init["core"];
        let heads = Heads {
            path: f(core, "path830") as i32,
            heads: [0, 1, 2].map(|i| mode_head(&init["heads"][i])),
        };
        let mut st = ReplikaState::new(48_000.);
        st.env = f(core, "env");
        for (ch, side) in [(0, "L"), (1, "R")] {
            let sec = |p: &str| State {
                s0: f(core, &format!("sat{side}_{p}_s0")) as f32,
                s1: f(core, &format!("sat{side}_{p}_s1")) as f32,
            };
            st.saturator[ch] = [sec("b"), sec("a")];
        }
        (heads, st)
    }

    /// `141aca670` through the setter (index 28) from the reset path: v = 0.1 (k 0), 0.3 (k 1), 0.5 (k 2), 0.9 (k 4).
    /// The ported fields after the call equal the native ones (floats within 1e-5, integers and flags exact). The
    /// `changed` list of each vector holds what the port does not write (see [`Heads::select`]); it is not checked.
    #[test]
    fn selector_matches_native_vectors() {
        for name in ["v0p1", "v0p3", "v0p5", "v0p9"] {
            let doc = selector_vector(name);
            let (init, end) = (&doc["init"], &doc["end"]);
            let (mut heads, mut st) = selector_state(init);
            heads
                .select(&mut st, f(&doc, "v") as f32)
                .expect("paths 0, 1, 2 and 4 are ported");
            let tag = format!("{name} k {}", f(&doc, "k"));
            assert_eq!(heads.path as f64, f(&end["core"], "path830"), "{tag} path");
            assert_eq!(st.env, f(&end["core"], "env"), "{tag} env");
            for (ch, side) in [(0, "L"), (1, "R")] {
                let got = [
                    ("a_s0", st.saturator[ch][1].s0),
                    ("a_s1", st.saturator[ch][1].s1),
                    ("b_s0", st.saturator[ch][0].s0),
                    ("b_s1", st.saturator[ch][0].s1),
                ];
                for (part, g) in got {
                    let want = f(&end["core"], &format!("sat{side}_{part}"));
                    close_f32(&format!("{tag} sat{side} {part}"), g as f64, want);
                }
            }
            for (i, h) in heads.heads.iter().enumerate() {
                let (e, tag) = (&end["heads"][i], format!("{tag} head {i}"));
                assert_eq!(h.mode as f64, f(e, "mode"), "{tag} mode");
                assert_eq!(h.kernel[0] as f64, f(e, "kernel_l"), "{tag} kernel L");
                assert_eq!(h.kernel[1] as f64, f(e, "kernel_r"), "{tag} kernel R");
                let g = &h.glide;
                close_f32(&format!("{tag} glide cur"), g.cur as f64, f(e, "g_cur"));
                close_f32(
                    &format!("{tag} glide target"),
                    g.target as f64,
                    f(e, "g_target"),
                );
                close_f32(&format!("{tag} glide step"), g.step as f64, f(e, "g_step"));
                assert_eq!(g.count as f64, f(e, "g_count"), "{tag} glide count");
                assert_eq!(
                    g.count_init as f64,
                    f(e, "g_count_init"),
                    "{tag} count init"
                );
                assert_eq!(g.pending, f(e, "g_pend") != 0.0, "{tag} glide pending");
                assert_eq!(g.fresh, f(e, "g_fresh") != 0.0, "{tag} glide fresh");
                close(&format!("{tag} s"), h.s, f(e, "s"), 1e-5);
                close(&format!("{tag} base"), h.base_src, f(e, "base_src"), 1e-5);
                close(
                    &format!("{tag} fade target"),
                    h.fade.target,
                    f(e, "fade_target"),
                    1e-5,
                );
                close(
                    &format!("{tag} fade level"),
                    h.fade.level,
                    f(e, "level"),
                    1e-5,
                );
                assert_eq!(h.fade.pending, f(e, "pend") != 0.0, "{tag} fade pending");
                assert_eq!(h.fade.trigger, f(e, "trig") != 0.0, "{tag} fade trigger");
                for (side, tap) in [("L", &h.taps[0]), ("R", &h.taps[1])] {
                    let key = |n: &str| format!("{side}_{n}");
                    for (k, n) in ["rate0", "rate1", "rate2"].iter().enumerate() {
                        close_f32(&format!("{tag} {side} {n}"), tap.rates[k], f(e, &key(n)));
                    }
                    // The driver fields the tap reset zeroes (`141aba040`), and the write-side fields it leaves.
                    for (n, got) in [
                        ("pos", tap.pos),
                        ("frac", tap.frac),
                        ("phase", tap.phase),
                        ("xprev", tap.x_prev),
                        ("pfrac", tap.prev_frac),
                        ("offset", tap.offset),
                        ("s0", tap.state.s0 as f64),
                        ("s1", tap.state.s1 as f64),
                    ] {
                        close_f32(&format!("{tag} {side} {n}"), got, f(e, &key(n)));
                    }
                    for (n, got) in [
                        ("wr", tap.wr),
                        ("delta", tap.delta),
                        ("pwr", tap.prev_wr),
                        ("idx", tap.idx),
                    ] {
                        assert_eq!(got as f64, f(e, &key(n)), "{tag} {side} {n}");
                    }
                }
            }
        }
    }

    /// Index 0.7 (path 3) returns `Err(Blocked)` with nothing written. A repeat of the same index changes nothing.
    #[test]
    fn selector_path_three_is_blocked_and_repeat_is_a_no_op() {
        let doc = selector_vector("v0p1");
        let (mut heads, mut st) = selector_state(&doc["init"]);
        let (path, glide, kernel) = (heads.path, heads.heads[0].glide, heads.heads[0].kernel);
        let (env, sat) = (st.env, st.saturator);
        assert_eq!(heads.select(&mut st, 0.7), Err(Blocked));
        assert_eq!(heads.path, path);
        assert_eq!(heads.heads[0].glide, glide);
        assert_eq!(heads.heads[0].kernel, kernel);
        assert_eq!(st.env, env);
        assert_eq!(st.saturator, sat);
        heads.select(&mut st, 0.1).expect("path 0");
        let (glide, fade) = (heads.heads[0].glide, heads.heads[0].fade);
        heads.select(&mut st, 0.1).expect("same path");
        assert_eq!(heads.heads[0].glide, glide);
        assert_eq!(heads.heads[0].fade, fade);
    }
}
