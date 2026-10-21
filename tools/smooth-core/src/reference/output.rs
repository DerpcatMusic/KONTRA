//! Output stage of soothe3's process block, translated from `0x890a00..0x893bd7`.
//!
//! What is here, and where it comes from:
//!
//! - [`GainRecord::apply`]: the gain helper `0x899ab0`. The record is three f32 at
//!   `+0` target `t`, `+4` current `g`, `+8` coefficient `a`. Per sample,
//!   `g = (t - g) * a + g`, then every channel sample is multiplied by `g`. The
//!   near-zero branch zeroes the channels through `0x1800010d0` (zero-fill).
//!   Checked bit-for-bit against the emulated binary (`fixtures/output_ramp.json`).
//! - [`ramp_coefficient`]: the constructor's `a = 1 - expf(-1/0.03 / fs)` (`0x88fa90`,
//!   constant `-33.333336` at `0x18209da5c`). The time base is the sample rate, because
//!   the helper steps once per sample.
//! - [`OutputTargets::compute`]: the five record targets the block writes before any
//!   ramp runs (`0x891e0e..0x891ed8`; bit-exact against the emulated block). Records live at
//!   `obj+0xbf28` (dry), `+0xbf34` (wet), `+0xbf40` (main, 1 unless soloed),
//!   `+0xbf4c` (solo stream, 1 when soloed), `+0xbf58` (out).
//! - [`OutputParams`] and [`wet_gain_from_db`] / [`mix_from_percent`]: the setter
//!   `0x894370` switch cases that write the output fields (see the table on the struct).
//! - [`mid_side_pair`]: the stereo-pair helper `0x8a3b20` (gain 0.5 on encode, 1.0 on
//!   decode), as established in `research/stereo-ms-sidechain-output.md`.
//!
//! Not here, and why: the dry/wet summation and the write to the output table, the
//! band-solo filter and its coefficient smoothing, the sidechain mux and meter. Those
//! sit in the stack-aliased region of `0x890c92..0x893bd7` (about 1,300 decompiled
//! lines) and are not decoded yet. No `process_output_block` is provided until they are.

/// Gate below which a gain is treated as zero or unity. `[0x18209d03c]`.
pub const GATE: f32 = 1.0e-3;

/// Time constant of the gain ramps, in seconds. `[0x18209da5c] = -1/0.03`.
pub const RAMP_TIME_CONSTANT: f32 = -33.333_336;

/// Ramp coefficient for a sample rate, as the constructor computes it (`1 - expf(c / fs)`).
/// The constructor's expf is not bit-identical to `f32::exp` in every case; this value is
/// an input to the ramp, so the fixture takes its own coefficients.
pub fn ramp_coefficient(sample_rate: f32) -> f32 {
    1.0 - (RAMP_TIME_CONSTANT / sample_rate).exp()
}

/// One gain record: target, current gain and per-sample coefficient (`0x899ab0`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct GainRecord {
    pub target: f32,
    pub current: f32,
    pub coeff: f32,
}

impl GainRecord {
    /// Applies one block of the ramp to `channels[..][..n]`.
    ///
    /// Three cases, in the binary's order:
    /// 1. `|t - 1| < GATE` and `|t - g| < GATE`: the gain is settled at unity. Nothing
    ///    is done, and `g` is left where it is (it is not snapped to `t`).
    /// 2. `|t| < GATE` and `|g| < GATE`: both are near zero. The channels are zeroed.
    ///    `g` is not updated.
    /// 3. Otherwise: ramp. `g` steps once per sample and multiplies that sample on
    ///    every channel.
    ///
    /// The comparisons are `>=` on `|x|`. For NaN they are false, as the binary's
    /// `comiss`/`jae` pairs are.
    pub fn apply(&mut self, channels: &mut [&mut [f32]], n: usize) {
        let (t, a) = (self.target, self.coeff);
        if !((t - 1.0).abs() >= GATE || (t - self.current).abs() >= GATE) {
            return;
        }
        if !(t.abs() >= GATE || self.current.abs() >= GATE) {
            for ch in channels.iter_mut() {
                ch[..n].fill(0.0);
            }
            return;
        }
        for i in 0..n {
            self.current = (t - self.current) * a + self.current;
            for ch in channels.iter_mut() {
                ch[i] *= self.current;
            }
        }
    }
}

/// The output-side fields of the DSP object, named as the setter switch writes them.
///
/// | field | object offset | setter case (param id) | value |
/// |---|---|---|---|
/// | `wet_gain` | `+0x94` | `0x5d` (93) | `10^(dB * 0.05)` (`powf(10, dB*0.05)`) |
/// | `mix` | `+0x98` | `0x5e` (94) | percent `* 0.01` |
/// | `out_gain` | `+0x9c` | `0x5f` (95) | `10^(dB * 0.05)` |
/// | `delta` | `+0xa8` | `0x60` (96) | `value >= 0.5` |
/// | `bypass` | `+0xa9` | `0x61` (97) | `value >= 0.5` |
/// | `sidechain` | `+0xa5` | `0x0e` (14) | `value >= 0.5` |
/// | `stereo_ms` | `+0xa6` | `0x10` (16) | `value >= 0.5` |
/// | `sidechain_listen` | `+0xa7` | `0x0f` (15) | `value >= 0.5` (use not decoded) |
/// | `solo_active` | `+0xaa` | derived per block | band-solo flag from the record at `+0x10c + 0x20*idx` |
/// | `solo_flag_236` | `+0x236` | flag setter `0x88f660` | not decoded |
/// | `band_flags_any` | bytes `+0x212 + 8k`, `k < 5` | per block | read only when channels > 2 |
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutputParams {
    pub wet_gain: f32,
    pub mix: f32,
    pub out_gain: f32,
    pub delta: bool,
    pub bypass: bool,
    pub solo_active: bool,
    pub solo_flag_236: bool,
    pub band_flags_any: bool,
    pub channels: usize,
}

impl Default for OutputParams {
    fn default() -> Self {
        Self {
            wet_gain: 1.0,
            mix: 1.0,
            out_gain: 1.0,
            delta: false,
            bypass: false,
            solo_active: false,
            solo_flag_236: false,
            band_flags_any: false,
            channels: 2,
        }
    }
}

/// `0x5d` / `0x5f`: `powf(10, dB * 0.05)`, with the multiply done in f32 first.
pub fn wet_gain_from_db(db: f32) -> f32 {
    10.0f32.powf(db * 0.05)
}

/// `0x5e`: the mix value is a percentage, stored as a fraction.
pub fn mix_from_percent(percent: f32) -> f32 {
    percent * 0.01
}

/// The five gain-record targets the block writes before the ramps run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutputTargets {
    /// `+0xbf28`: dry gain.
    pub dry: f32,
    /// `+0xbf34`: wet gain.
    pub wet: f32,
    /// `+0xbf40`: main (non-solo) gain, 1.0 unless soloed.
    pub main: f32,
    /// `+0xbf4c`: solo-stream gain, 1.0 when soloed.
    pub solo: f32,
    /// `+0xbf58`: output gain.
    pub out: f32,
}

impl OutputTargets {
    /// Targets for one block, as written at `0x891e0e..0x891ed8` (verified against the
    /// emulated block, `fixtures/output_targets.json`). The `delta` term is [`delta_mode`].
    pub fn compute(p: &OutputParams) -> Self {
        let delta_mode = delta_mode(p);
        let wm = p.wet_gain * p.mix;
        let wet = if p.bypass {
            0.0
        } else if delta_mode {
            wm * -1.0
        } else {
            wm
        };
        let dry = if p.bypass {
            1.0
        } else if delta_mode {
            wm
        } else {
            1.0 - p.mix
        };
        let out = if p.bypass { 1.0 } else { p.out_gain };
        let (main, solo) = if p.solo_active { (0.0, 1.0) } else { (1.0, 0.0) };
        Self { dry, wet, main, solo, out }
    }
}

/// The block's `cl` flag at `0x891dd2..0x891e08`.
pub fn delta_mode(p: &OutputParams) -> bool {
    p.delta || (p.channels > 2 && p.band_flags_any) || (p.solo_active && p.solo_flag_236)
}

/// Stereo-pair helper `0x8a3b20` for one pair `(s0, s1)`: `dst = (g(s0+s1), g(s0-s1))`.
///
/// Encode runs with `g = 0.5` (`0x1808928a3`, the analysis input). Decode runs with
/// `g = 1.0` (`0x18089a5bc`, the wet path). Emulated against the binary in
/// `research/re-tools/emulate/stereo_pair_probe.py`, not bit-compared.
pub fn mid_side_pair(s0: f32, s1: f32, gain: f32) -> (f32, f32) {
    (gain * (s0 + s1), gain * (s0 - s1))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("fixtures/output_ramp.json");

    fn f32s(v: &serde_json::Value) -> Vec<f32> {
        v.as_array()
            .expect("array of f32 bit patterns")
            .iter()
            .map(|b| f32::from_bits(b.as_u64().expect("u32") as u32))
            .collect()
    }

    #[test]
    fn ramp_matches_emulated_helper_bit_for_bit() {
        let fixture: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert!(cases.len() >= 500, "fixture should cover the gate branches and rates");
        for (idx, case) in cases.iter().enumerate() {
            let n = case["n"].as_u64().unwrap() as usize;
            let rec_in = f32s(&case["rec_in"]);
            let rec_out = f32s(&case["rec_out"]);
            let mut rec = GainRecord { target: rec_in[0], current: rec_in[1], coeff: rec_in[2] };
            let mut bufs: Vec<Vec<f32>> = case["in"].as_array().unwrap().iter().map(f32s).collect();
            let mut refs: Vec<&mut [f32]> = bufs.iter_mut().map(|b| b.as_mut_slice()).collect();
            rec.apply(&mut refs, n);

            assert_eq!(rec.target.to_bits(), rec_out[0].to_bits(), "case {idx} target");
            assert_eq!(rec.current.to_bits(), rec_out[1].to_bits(), "case {idx} current");
            assert_eq!(rec.coeff.to_bits(), rec_out[2].to_bits(), "case {idx} coeff");
            for (c, expected) in case["out"].as_array().unwrap().iter().enumerate() {
                for (i, (g, e)) in refs[c].iter().zip(f32s(expected)).enumerate() {
                    assert_eq!(g.to_bits(), e.to_bits(), "case {idx} ch {c} sample {i}");
                }
            }
        }
    }

    #[test]
    fn targets_match_the_emulated_block_bit_for_bit() {
        const TARGETS: &str = include_str!("fixtures/output_targets.json");
        let fixture: serde_json::Value = serde_json::from_str(TARGETS).unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert!(cases.len() >= 32, "fixture should cover identity, delta, bypass and solo");
        let bits = |v: &serde_json::Value| f32::from_bits(v.as_u64().unwrap() as u32);
        for (idx, case) in cases.iter().enumerate() {
            let p = &case["params"];
            let params = OutputParams {
                wet_gain: bits(&p["wet_gain"]),
                mix: bits(&p["mix"]),
                out_gain: bits(&p["out_gain"]),
                delta: p["delta"].as_bool().unwrap(),
                bypass: p["bypass"].as_bool().unwrap(),
                solo_active: p["solo_active"].as_bool().unwrap(),
                solo_flag_236: p["solo_flag_236"].as_bool().unwrap(),
                band_flags_any: false,
                channels: 2,
            };
            let got = OutputTargets::compute(&params);
            let want = &case["targets"];
            for (name, value) in [
                ("dry", got.dry),
                ("wet", got.wet),
                ("main", got.main),
                ("solo", got.solo),
                ("out", got.out),
            ] {
                assert_eq!(value.to_bits(), bits(&want[name]).to_bits(), "case {idx} {name}");
            }
        }
    }

    #[test]
    fn unity_and_zero_gates_leave_their_documented_state() {
        let mut settled = GainRecord { target: 1.0, current: 1.0 - 5e-4, coeff: 0.1 };
        let mut buf = vec![0.25f32; 4];
        settled.apply(&mut [buf.as_mut_slice()], 4);
        assert_eq!(buf, vec![0.25; 4], "settled at unity: no multiply");
        assert_eq!(settled.current, 1.0 - 5e-4, "settled at unity: g is kept, not snapped");

        let mut mute = GainRecord { target: 0.0, current: 5e-4, coeff: 0.1 };
        mute.apply(&mut [buf.as_mut_slice()], 4);
        assert_eq!(buf, vec![0.0; 4], "both under the gate: zeroed");
        assert_eq!(mute.current, 5e-4, "zeroed path does not step g");
    }

    #[test]
    fn bypass_and_delta_targets_match_the_block() {
        let p = OutputParams { wet_gain: 2.0, mix: 0.25, out_gain: 0.5, ..OutputParams::default() };
        let t = OutputTargets::compute(&p);
        assert_eq!((t.dry, t.wet, t.out), (0.75, 0.5, 0.5));
        assert_eq!((t.main, t.solo), (1.0, 0.0));

        let d = OutputParams { delta: true, ..p };
        let t = OutputTargets::compute(&d);
        assert_eq!((t.dry, t.wet), (0.5, -0.5), "delta: output = wm * (dry - wet)");

        let b = OutputParams { bypass: true, delta: true, ..p };
        let t = OutputTargets::compute(&b);
        assert_eq!((t.dry, t.wet, t.out), (1.0, 0.0, 1.0), "bypass ignores trims and mix");
    }

    #[test]
    fn solo_engages_delta_and_routes_the_solo_stream() {
        let p = OutputParams { solo_active: true, solo_flag_236: true, ..OutputParams::default() };
        assert!(delta_mode(&p));
        let t = OutputTargets::compute(&p);
        assert_eq!((t.main, t.solo), (0.0, 1.0));
        let no_flag = OutputParams { solo_active: true, ..OutputParams::default() };
        assert!(!delta_mode(&no_flag));
    }

    #[test]
    fn mid_side_round_trips_at_unity_after_half_on_encode() {
        let (m, s) = mid_side_pair(0.6, 0.2, 0.5);
        let (l, r) = mid_side_pair(m, s, 1.0);
        assert!((l - 0.6).abs() < 1e-7 && (r - 0.2).abs() < 1e-7);
    }
}
