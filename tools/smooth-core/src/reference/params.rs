//! The reference's parameter setter: `0x894370`, entered through the setParam
//! thunk `0x88f4e0` (wrapper slot +0x08).
//!
//! Evidence: the disassembly of `0x894370` (the decompile misreads its clamps
//! and the double-typed descriptor fields), the decompile corpus, and the
//! Unicorn differential fixture `research/oracle/params_setter_fixture.tsv`
//! written by `research/re-tools/emulate/setter_oracle.py`. The test at the
//! bottom replays that fixture and compares every byte the setter writes.
//!
//! Calling convention, from the disassembly:
//! - thunk `0x88f4e0`: `rcx = wrapper`, `obj = [wrapper]`, `xmm2 = (double) value`,
//!   then `cvtsd2ss` and `jmp 0x894370`.
//! - `0x894370`: `rdx` is a host descriptor: `u32 index` at +0, `f64 min` at
//!   +0x28, `f64 max` at +0x30, and a function pointer at +0x50. The function is
//!   called as `f64 fn(desc, f64 x)` and its result is `cvtsd2ss`ed.
//!
//! The host's mapping function is not in this image, so [`ReferenceParams::set`]
//! uses the identity map on [0, 1]. [`ReferenceParams::set_described`] takes the
//! host's range and map explicitly.
//!
//! Field names are `r{index}_{offset}`. `index` is the setter index (0..=142)
//! and `offset` is where the DSP object keeps the value. The map from each index
//! to a SMOOTH parameter is in `research/reference-params.md`. Mapped indices
//! are a candidate reading unless that file says otherwise.
//!
//! Return value: `0x894370` leaves `rax` holding a mix of the previous value and
//! the dispatch byte, depending on the case. Nothing in this translation reads
//! it, so it is not reproduced; the fixture records it but the test ignores it.

use std::collections::BTreeMap;

use crate::dsp::Settings;

/// Indices the setter accepts. Anything above is a no-op (`cmp eax, 0x8e; ja`).
pub const PARAM_COUNT: u32 = 143;

/// `ln 6` as stored at `0x18209d3f0` (f32 bits `0x3fe55860`).
const LN6: f32 = f32::from_bits(0x3fe5_5860);
/// `1 / ln 2` as stored at `0x18209d4b8` (f64). Converts `asinh` to octaves.
const LOG2_E: f64 = 2.885_390_081_777_927;

/// Dirty-flag groups: each is four bytes at the offsets in [`DIRTY_BASE`], one per
/// slot. Slots are A=0 (index 2), B=1 (index 4), C=2 (indices 5, 8, 9), D=3
/// (indices 6, 7, 10..=13), F=4 (band fields), E=5 (indices 1, 14, 15).
const DIRTY_BASE: [u32; 4] = [0x3240, 0x5e88, 0x8b18, 0xb760];
const SLOT_A: usize = 0;
const SLOT_B: usize = 1;
const SLOT_C: usize = 2;
const SLOT_D: usize = 3;
const SLOT_F: usize = 4;
const SLOT_E: usize = 5;

/// One band's eight fields, at `0x10c + 32 * band` (`obj+0x10c` in `0x894370`).
/// Index `19 + 9 * band + field` sets field `field`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Band {
    /// `+0x00` (`0x10c`), index `f0`: bool, stored as `x >= 0.5`.
    pub used: bool,
    /// `+0x01` (`0x10d`), index `f1`: bool.
    pub enabled: bool,
    /// `+0x04` (`0x110`), index `f2`: the host's plain value, clamped to the range.
    pub frequency: f32,
    /// `+0x08` (`0x114`), index `f3`: `x * 24 - 12`, the reference's `1.2 * dB`.
    pub depth: f32,
    /// `+0x0c` (`0x118`), index `f4`: `log2(e) * asinh(1 / (2 * y))`, half the
    /// bandwidth in octaves, from the plain value `y` (the band Q). Stored as f32 bits.
    pub half_bandwidth_oct: f32,
    /// `+0x10` (`0x11c`), index `f5`: `expf(ln6 + x * (ln128 - ln6))`, a slope in
    /// dB/oct over 6..128. Stored as f32 bits.
    pub slope: f32,
    /// `+0x14` (`0x120`), index `f6`: the host's plain value (balance).
    pub focus: f32,
    /// `+0x18` (`0x124`), index `f7`: `roundf(y)` as an integer (shape).
    pub shape: u32,
    /// `+0x1c` (`0x128`), index `f8`: `roundf(y)` as an integer. SMOOTH's
    /// `BandRecord::selector`, which SMOOTH always writes as 0.
    pub selector: u32,
}

/// Index `98 + 5 * k` stores `x` at `0x20c + 8k`. Indices `+1..=+4` store the four
/// flags at `0x210 + 8k ..= 0x213 + 8k`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ExtGroup {
    /// `0x20c + 8k`, index `98 + 5k`: normalized value.
    pub norm: f32,
    /// `0x210 + 8k ..= 0x213 + 8k`, indices `99 + 5k ..= 102 + 5k`.
    pub flags: [bool; 4],
}

/// The reference's parameter state for one DSP object. Every field is at the
/// object offset in its doc comment. Zero-filled on reset, like the host's zeroed
/// object in the fixture.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReferenceParams {
    /// `0x54`, index 0: normalized `x`.
    pub r000_norm_54: f32,
    /// `0xab`, index 1: bool. Zero-latency mode (`reference_plan.rs`). Block E.
    pub r001_flag_ab: bool,
    /// `0x104`, index 2: `roundf(y)` as u32. Block A on change.
    pub r002_int_104: u32,
    /// `0x50`, index 3: `roundf(y)` as u32.
    pub r003_int_50: u32,
    /// `0xac`, index 4: bool. Block B on change, and it sets `0x4a`.
    pub r004_flag_ac: bool,
    /// `0x58`, index 5: normalized `x`. Block C.
    pub r005_norm_58: f32,
    /// `0x5c`, index 6: normalized `x`. Block D.
    pub r006_norm_5c: f32,
    /// `0x60`, index 7: normalized `x`. Block D.
    pub r007_norm_60: f32,
    /// `0x64`, index 8: plain `y`. Block C.
    pub r008_plain_64: f32,
    /// `0x68`, index 9: plain `y`. Block C.
    pub r009_plain_68: f32,
    /// `0x6c`, index 10: plain `y`. Block D.
    pub r010_plain_6c: f32,
    /// `0x70`, index 11: plain `y`. Block D.
    pub r011_plain_70: f32,
    /// `0x74`, index 12: plain `y`. Block D.
    pub r012_plain_74: f32,
    /// `0x78`, index 13: plain `y`. Block D.
    pub r013_plain_78: f32,
    /// `0xa5`, index 14: bool. Block E.
    pub r014_flag_a5: bool,
    /// `0xa7`, index 15: bool. Block E.
    pub r015_flag_a7: bool,
    /// `0xa6`, index 16: bool.
    pub r016_flag_a6: bool,
    /// `0x7c`, index 17: normalized `x`.
    pub r017_norm_7c: f32,
    /// `0x90`, index 18: normalized `x`.
    pub r018_norm_90: f32,
    /// Indices 19..=90: eight bands of nine fields.
    pub bands: [Band; 8],
    /// `0x100`, index 91: `min(roundf(y) - 1, 7)` as i32. Host value 0 stores -1
    /// (`0xffff_ffff`), which is "no band soloed".
    pub r091_solo_100: u32,
    /// `0xa0`, index 92: plain `y`, or 60 when `0.999 <= x`.
    pub r092_plain_a0: f32,
    /// `0x94`, index 93: `powf(10, y * 0.05)`. The wet-trim gain, f32 bits.
    pub r093_gain_94: f32,
    /// `0x98`, index 94: `y * 0.01`.
    pub r094_scaled_98: f32,
    /// `0x9c`, index 95: `powf(10, y * 0.05)`. The output-trim gain, f32 bits.
    pub r095_gain_9c: f32,
    /// `0xa8`, index 96: bool.
    pub r096_flag_a8: bool,
    /// `0xa9`, index 97: bool.
    pub r097_flag_a9: bool,
    /// Indices 98..=122: five groups of normalized value plus four flags.
    pub ext: [ExtGroup; 5],
    /// `0x80`, index 123: normalized `x`.
    pub r123_norm_80: f32,
    /// `0x84`, index 124: normalized `x`.
    pub r124_norm_84: f32,
    /// `0x88`, index 125: normalized `x`.
    pub r125_norm_88: f32,
    /// `0x8c`, index 126: normalized `x`.
    pub r126_norm_8c: f32,
    /// `0xf0..=0xff`, indices 127..=142: `flags[i - 127]` is bool byte `0x71 + i`.
    pub flags_f0: [bool; 16],
    /// The dirty bytes, `dirty[g][slot]` at `DIRTY_BASE[g] + slot`. Set to 1 by the
    /// setter, and read by the process path (not translated yet).
    pub dirty: [[u8; 6]; 4],
    /// `0x4a`: set to 1 when index 4 changes.
    pub flag_4a: u8,
}

impl ReferenceParams {
    /// setParam with the identity host map on [0, 1]. Equivalent to `0x894370` with
    /// `desc = { min: 0, max: 1, map: identity }`. Indices above 142 do nothing.
    pub fn set(&mut self, index: u16, value: f32) {
        self.set_described(u32::from(index), value, 0.0, 1.0, |x| x);
    }

    /// `0x894370` with the host's descriptor. `value` is the f32 the thunk passes
    /// after `cvtsd2ss`. `min` and `max` are the descriptor's f64 fields, converted
    /// to f32 as `cvtpd2ps` does.
    pub fn set_described(
        &mut self,
        index: u32,
        value: f32,
        min: f64,
        max: f64,
        map: impl Fn(f64) -> f64,
    ) {
        // Prologue, in the order of 0x894370. `x` is the clamped normalized value,
        // `y` the mapped and clamped plain value, `u` the truncated `roundf(y)`.
        let x = unit_clamp(value);
        let y = clamp_plain(map(f64::from(x)) as f32, min as f32, max as f32);
        let u = cvtt_u32(y.round());
        let on = x >= 0.5;

        match index {
            0 => self.r000_norm_54 = x,
            1 => {
                if self.r001_flag_ab == on {
                    return;
                }
                self.r001_flag_ab = on;
                self.mark(SLOT_E);
            }
            2 => {
                if self.r002_int_104 != u {
                    self.r002_int_104 = u;
                    self.mark(SLOT_A);
                }
            }
            3 => self.r003_int_50 = u,
            4 => {
                if self.r004_flag_ac != on {
                    self.r004_flag_ac = on;
                    self.mark(SLOT_B);
                    self.flag_4a = 1;
                }
            }
            5 => {
                self.r005_norm_58 = x;
                self.mark(SLOT_C);
            }
            6 => {
                self.r006_norm_5c = x;
                self.mark(SLOT_D);
            }
            7 => {
                self.r007_norm_60 = x;
                self.mark(SLOT_D);
            }
            8 => {
                self.r008_plain_64 = y;
                self.mark(SLOT_C);
            }
            9 => {
                self.r009_plain_68 = y;
                self.mark(SLOT_C);
            }
            10 => {
                self.r010_plain_6c = y;
                self.mark(SLOT_D);
            }
            11 => {
                self.r011_plain_70 = y;
                self.mark(SLOT_D);
            }
            12 => {
                self.r012_plain_74 = y;
                self.mark(SLOT_D);
            }
            13 => {
                self.r013_plain_78 = y;
                self.mark(SLOT_D);
            }
            14 => {
                if self.r014_flag_a5 == on {
                    return;
                }
                self.r014_flag_a5 = on;
                self.mark(SLOT_E);
            }
            15 => {
                if self.r015_flag_a7 == on {
                    return;
                }
                self.r015_flag_a7 = on;
                self.mark(SLOT_E);
            }
            16 => self.r016_flag_a6 = on,
            17 => self.r017_norm_7c = x,
            18 => self.r018_norm_90 = x,
            19..=90 => {
                let rel = index - 19;
                let band = &mut self.bands[(rel / 9) as usize];
                match rel % 9 {
                    0 => band.used = on,
                    1 => band.enabled = on,
                    2 => band.frequency = y,
                    3 => band.depth = x * 24.0 - 12.0,
                    4 => band.half_bandwidth_oct = half_bandwidth_oct(y),
                    5 => band.slope = slope_from_norm(x),
                    6 => band.focus = y,
                    7 => band.shape = u,
                    _ => band.selector = u,
                }
                self.mark(SLOT_F);
            }
            // Signed compare (`cmp eax, ecx; cmovg`): host value 0 gives -1, which is "no solo".
            91 => self.r091_solo_100 = (u.wrapping_sub(1) as i32).min(7) as u32,
            92 => self.r092_plain_a0 = if 0.999 <= f64::from(x) { 60.0 } else { y },
            93 => self.r093_gain_94 = 10.0_f32.powf(y * 0.05),
            94 => self.r094_scaled_98 = y * 0.01,
            95 => self.r095_gain_9c = 10.0_f32.powf(y * 0.05),
            96 => self.r096_flag_a8 = on,
            97 => self.r097_flag_a9 = on,
            98..=122 => {
                let rel = index - 98;
                let group = &mut self.ext[(rel / 5) as usize];
                match rel % 5 {
                    0 => group.norm = x,
                    r => group.flags[(r - 1) as usize] = on,
                }
            }
            123 => self.r123_norm_80 = x,
            124 => self.r124_norm_84 = x,
            125 => self.r125_norm_88 = x,
            126 => self.r126_norm_8c = x,
            127..=142 => self.flags_f0[(index - 127) as usize] = on,
            _ => {}
        }
    }

    /// `or byte [obj + DIRTY_BASE[g] + slot], 1` for the four groups.
    fn mark(&mut self, slot: usize) {
        for group in &mut self.dirty {
            group[slot] = 1;
        }
    }

    /// Every non-zero byte of the object, keyed by offset. Used by the differential
    /// test. The layout is the one in the field docs above.
    pub fn image(&self) -> BTreeMap<u32, u8> {
        let mut m = BTreeMap::new();
        let mut put = |off: u32, bytes: &[u8]| {
            for (i, b) in bytes.iter().enumerate() {
                if *b != 0 {
                    m.insert(off + i as u32, *b);
                }
            }
        };
        let f = |v: f32| v.to_bits().to_le_bytes();
        let u = |v: u32| v.to_le_bytes();
        let b = |v: bool| [v as u8];
        put(0x54, &f(self.r000_norm_54));
        put(0xab, &b(self.r001_flag_ab));
        put(0x104, &u(self.r002_int_104));
        put(0x50, &u(self.r003_int_50));
        put(0xac, &b(self.r004_flag_ac));
        put(0x58, &f(self.r005_norm_58));
        put(0x5c, &f(self.r006_norm_5c));
        put(0x60, &f(self.r007_norm_60));
        put(0x64, &f(self.r008_plain_64));
        put(0x68, &f(self.r009_plain_68));
        put(0x6c, &f(self.r010_plain_6c));
        put(0x70, &f(self.r011_plain_70));
        put(0x74, &f(self.r012_plain_74));
        put(0x78, &f(self.r013_plain_78));
        put(0xa5, &b(self.r014_flag_a5));
        put(0xa7, &b(self.r015_flag_a7));
        put(0xa6, &b(self.r016_flag_a6));
        put(0x7c, &f(self.r017_norm_7c));
        put(0x90, &f(self.r018_norm_90));
        for (i, band) in self.bands.iter().enumerate() {
            let base = 0x10c + 32 * i as u32;
            put(base, &b(band.used));
            put(base + 1, &b(band.enabled));
            put(base + 4, &f(band.frequency));
            put(base + 8, &f(band.depth));
            put(base + 12, &f(band.half_bandwidth_oct));
            put(base + 16, &f(band.slope));
            put(base + 20, &f(band.focus));
            put(base + 24, &u(band.shape));
            put(base + 28, &u(band.selector));
        }
        put(0x100, &u(self.r091_solo_100));
        put(0xa0, &f(self.r092_plain_a0));
        put(0x94, &f(self.r093_gain_94));
        put(0x98, &f(self.r094_scaled_98));
        put(0x9c, &f(self.r095_gain_9c));
        put(0xa8, &b(self.r096_flag_a8));
        put(0xa9, &b(self.r097_flag_a9));
        for (k, g) in self.ext.iter().enumerate() {
            let base = 0x20c + 8 * k as u32;
            put(base, &f(g.norm));
            for (j, flag) in g.flags.iter().enumerate() {
                put(base + 4 + j as u32, &b(*flag));
            }
        }
        put(0x80, &f(self.r123_norm_80));
        put(0x84, &f(self.r124_norm_84));
        put(0x88, &f(self.r125_norm_88));
        put(0x8c, &f(self.r126_norm_8c));
        for (i, flag) in self.flags_f0.iter().enumerate() {
            put(0xf0 + i as u32, &b(*flag));
        }
        for (g, group) in self.dirty.iter().enumerate() {
            put(DIRTY_BASE[g], group);
        }
        put(0x4a, &[self.flag_4a]);
        m
    }

    /// Builds the state a host would produce by sending SMOOTH's current values.
    ///
    /// The setter path needs a normalized host value and a host map, and SMOOTH's
    /// own maps (log, OL, PL, SS) are not in the image. So this writes each field
    /// straight from the plain value, using the same stored formulas as the setter
    /// cases above. Every dirty flag is set, as after a full set.
    ///
    /// Left at zero, because `Settings` does not carry them: quality, low latency
    /// and linear phase (indices 2, 1, 4), which SMOOTH keeps in `EngineConfig`;
    /// the detail tilts (17, 18), which SMOOTH never feeds to the engine; and the
    /// attack and release tilts (123..=126), whose normalized form needs SMOOTH's
    /// inverse sym_skewed map, which is not in the image.
    pub fn from_smooth(s: &Settings) -> Self {
        let mut r = Self::default();
        r.r000_norm_54 = s.depth;
        r.r003_int_50 = u32::from(s.hard);
        r.r005_norm_58 = s.focus;
        r.r008_plain_64 = s.stereo_link * 100.0;
        r.r010_plain_6c = s.detail * 10.0;
        r.r011_plain_70 = s.attack * 10.0;
        r.r012_plain_74 = s.release * 10.0;
        r.r014_flag_a5 = s.sidechain;
        r.r015_flag_a7 = s.sidechain_listen;
        r.r016_flag_a6 = s.delta;
        r.r091_solo_100 = s.solo_band.map_or(u32::MAX, |b| b as u32);
        r.r092_plain_a0 = s.max_cut_db;
        r.r093_gain_94 = s.wet_gain;
        r.r094_scaled_98 = s.mix;
        r.r095_gain_9c = 10.0_f32.powf(s.output_db * 0.05);
        r.r096_flag_a8 = s.bypass;
        r.r097_flag_a9 = s.mid_side;
        for (i, band) in s.bands.iter().enumerate() {
            r.bands[i] = Band {
                used: band.used,
                enabled: band.enabled,
                frequency: band.frequency,
                depth: band.depth,
                half_bandwidth_oct: half_bandwidth_oct(band.width),
                slope: band.slope,
                focus: band.focus,
                shape: band.shape as u8 as u32,
                selector: u32::from(band.selector),
            };
        }
        r.dirty = [[1; 6]; 4];
        r
    }
}

/// `0x894370` prologue: `x >= 0 ? x : 0` then `x <= 1 || unordered ? that : 1`.
/// NaN gives 0.
fn unit_clamp(x: f32) -> f32 {
    if x > 1.0 {
        1.0
    } else if x >= 0.0 {
        x
    } else {
        0.0
    }
}

/// `0x894370` clamp of the mapped value: `y >= lo ? y : lo`, then `hi >= y ? that : hi`.
/// NaN gives `hi`.
fn clamp_plain(y: f32, lo: f32, hi: f32) -> f32 {
    let lower = if y >= lo { y } else { lo };
    if hi >= y { lower } else { hi }
}

/// `cvttss2si` of the rounded value, read as u32. Out-of-range and NaN give
/// `0x8000_0000`.
fn cvtt_u32(r: f32) -> u32 {
    let t = r.trunc();
    if t.is_nan() || t >= 2_147_483_648.0 || t < -2_147_483_648.0 {
        0x8000_0000
    } else {
        t as i32 as u32
    }
}

/// Band field `f4` (`0x69da50`): `log2(e) * asinh(1 / (2y))` in f64, from f32
/// `y`. Returns 0 for `y <= 0` and NaN.
pub fn half_bandwidth_oct(y: f32) -> f32 {
    if y.is_nan() || y <= 0.0 {
        return 0.0;
    }
    let t = 1.0_f64 / f64::from(y * 2.0);
    (t.asinh() * LOG2_E) as f32
}

/// Band field `f5`: `expf((logf(128) - ln6) * x + ln6)`, with `x` already clamped.
fn slope_from_norm(x: f32) -> f32 {
    let top = 128.0_f32.ln();
    ((top - LN6) * x + LN6).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../research/oracle/params_setter_fixture.tsv");

    /// Float fields the reference computes with libm (`powf`, `asinh`, `logf`,
    /// `expf`). Compared at 1e-6 relative. Every other field is compared exactly.
    /// Offsets are the starts of each f32-sized field.
    fn is_libm_float(off: u32) -> Option<u32> {
        let mut starts = vec![0x94, 0x9c];
        for b in 0..8 {
            starts.push(0x118 + 32 * b);
            starts.push(0x11c + 32 * b);
        }
        starts.into_iter().find(|s| (*s..s + 4).contains(&off))
    }

    fn word(m: &BTreeMap<u32, u8>, start: u32) -> f32 {
        let b: Vec<u8> = (0..4)
            .map(|i| m.get(&(start + i)).copied().unwrap_or(0))
            .collect();
        f32::from_le_bytes([b[0], b[1], b[2], b[3]])
    }

    fn compare(got: &BTreeMap<u32, u8>, want: &BTreeMap<u32, u8>) -> Result<(), String> {
        let keys: std::collections::BTreeSet<u32> =
            got.keys().chain(want.keys()).copied().collect();
        for k in keys {
            let g = got.get(&k).copied().unwrap_or(0);
            let w = want.get(&k).copied().unwrap_or(0);
            if g == w {
                continue;
            }
            match is_libm_float(k) {
                Some(s) => {
                    let (gv, wv) = (word(got, s), word(want, s));
                    let tol = 1e-6 * gv.abs().max(wv.abs());
                    if gv.to_bits() == wv.to_bits() || (gv - wv).abs() <= tol {
                        continue;
                    }
                    return Err(format!("offset {s:#x}: got {gv:e}, want {wv:e}"));
                }
                None => return Err(format!("offset {k:#x}: got {g:#04x}, want {w:#04x}")),
            }
        }
        Ok(())
    }

    /// Replays every fixture sequence and compares the object after each call.
    #[test]
    fn setter_matches_emulated_reference() {
        let mut calls = 0usize;
        let mut seq_id: Option<u32> = None;
        let mut rp = ReferenceParams::default();
        let mut shadow: BTreeMap<u32, u8> = BTreeMap::new();
        for line in FIXTURE.lines().filter(|l| !l.starts_with('#')) {
            let f: Vec<&str> = line.split('\t').collect();
            let seq: u32 = f[0].parse().unwrap();
            if seq_id != Some(seq) {
                rp = ReferenceParams::default();
                shadow.clear();
                seq_id = Some(seq);
            }
            let index: u32 = f[1].parse().unwrap();
            let bits = |s: &str| f64::from_bits(u64::from_str_radix(s, 16).unwrap());
            let value = bits(f[2]);
            let (lo, hi) = (bits(f[3]), bits(f[4]));
            // The thunk narrows the double with cvtsd2ss before 0x894370 sees it.
            rp.set_described(index, value as f32, lo, hi, |x| x);
            if let Some(changes) = f.get(6).filter(|s| !s.is_empty()) {
                for run in changes.split(',') {
                    let (off, hex) = run.split_once(':').unwrap();
                    let off: u32 = off.parse().unwrap();
                    for (i, pair) in hex.as_bytes().chunks(2).enumerate() {
                        let byte =
                            u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
                        if byte == 0 {
                            shadow.remove(&(off + i as u32));
                        } else {
                            shadow.insert(off + i as u32, byte);
                        }
                    }
                }
            }
            if let Err(e) = compare(&rp.image(), &shadow) {
                panic!(
                    "call {calls}: seq {seq} index {index} value {value:?} min {lo:?} max {hi:?}: {e}"
                );
            }
            calls += 1;
        }
        assert!(calls > 5000, "fixture too short: {calls} calls");
    }

    #[test]
    fn from_smooth_matches_the_setter_for_linear_fields() {
        let mut s = Settings::default();
        s.mix = 0.25;
        s.wet_gain = 0.5;
        s.max_cut_db = 60.0;
        let r = ReferenceParams::from_smooth(&s);

        let mut by_setter = ReferenceParams::default();
        // mix: index 94 stores y * 0.01, so host value 25 on [0, 100] stores 0.25.
        by_setter.set_described(94, 0.25, 0.0, 100.0, |x| x * 100.0);
        assert!((r.r094_scaled_98 - by_setter.r094_scaled_98).abs() < 1e-6);
        // max cut: 0.999 <= x stores 60, as the host's top value does.
        by_setter.set_described(92, 1.0, 0.0, 24.0, |x| x * 24.0);
        assert_eq!(by_setter.r092_plain_a0, 60.0);
        assert_eq!(r.r092_plain_a0, 60.0);
    }

    #[test]
    fn band_index_map_covers_each_field() {
        // Index 19 + 9b + f: band b's field f. Every band field lands in its band.
        let mut rp = ReferenceParams::default();
        rp.set(19 + 9 * 7 + 6, 0.5); // band 7, field 6: focus stores y
        assert_eq!(rp.bands[7].focus, 0.5);
        rp.set(19 + 9 * 3 + 7, 0.99); // band 3, field 7 (shape): round(0.99) = 1
        assert_eq!(rp.bands[3].shape, 1);
    }
}
