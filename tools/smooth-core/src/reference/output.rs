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
//! - [`process_output_block`]: the output sum per 512-sample sub-block, including the band-solo
//!   path (`solo.rs`, `0x89a1a0`) on the wet + dry sum, with the main and solo gain records
//!   (`+0xbf40`, `+0xbf4c`).
//!   Order and gate are from the emulated trace: `research/output-stage-translation.md`, "Solo path".
//!
//! Not here: the sidechain listen and the meter, and the wet path itself (the FFT processing
//! feeding `wet`). The wet and dry roles are the record assignment from the code order; a
//! non-identity wet path has not been emulated (see the notes).

use super::solo::{self, SoloBand, SoloChannel};

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

/// Longest sub-block the output stage runs at once. The binary calls the ramp once per
/// sub-block of at most 512 samples (`n = min(512, remaining)`), so the gate decisions are
/// taken per sub-block, not per block.
pub const SUB_BLOCK: usize = 512;

/// What the ramp helper `0x899ab0` does for one sub-block, decided from the record's state
/// before the sub-block starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GainPlan {
    /// Settled at unity: no multiply, `g` kept.
    Skip,
    /// Both under the gate: the channels are zeroed (`0x1800010d0`), `g` not stepped.
    Zero,
    /// Ramp: `g` stepped once per sample; the per-sample gains are in the caller's buffer.
    Ramp,
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
    /// Decides the branch for a sub-block of `n` samples. On [`GainPlan::Ramp`] it steps `g`
    /// once per sample and writes the gains into `gains[..n]`. Other branches leave `gains`
    /// alone. `n` must be at most `gains.len()`.
    ///
    /// Three cases, in the binary's order:
    /// 1. `|t - 1| < GATE` and `|t - g| < GATE`: settled at unity ([`GainPlan::Skip`]).
    ///    `g` is left where it is (it is not snapped to `t`).
    /// 2. `|t| < GATE` and `|g| < GATE`: both near zero ([`GainPlan::Zero`]).
    /// 3. Otherwise: [`GainPlan::Ramp`].
    ///
    /// The comparisons are `>=` on `|x|`, so for NaN they are false, as the binary's
    /// `comiss`/`jae` pairs are.
    pub fn plan(&mut self, n: usize, gains: &mut [f32]) -> GainPlan {
        let (t, a) = (self.target, self.coeff);
        if !((t - 1.0).abs() >= GATE || (t - self.current).abs() >= GATE) {
            return GainPlan::Skip;
        }
        if !(t.abs() >= GATE || self.current.abs() >= GATE) {
            return GainPlan::Zero;
        }
        for g in gains[..n].iter_mut() {
            self.current = (t - self.current) * a + self.current;
            *g = self.current;
        }
        GainPlan::Ramp
    }

    /// Applies one block of the ramp to `channels[..][..n]` (n at most [`SUB_BLOCK`]).
    /// Same branches as [`GainRecord::plan`]; every channel sees the same per-sample gain.
    pub fn apply(&mut self, channels: &mut [&mut [f32]], n: usize) {
        let mut gains = [0.0f32; SUB_BLOCK];
        let plan = self.plan(n, &mut gains);
        for ch in channels.iter_mut() {
            apply_planned(plan, &gains[..n], &mut ch[..n]);
        }
    }
}

/// Applies a planned branch to one channel's sub-block.
fn apply_planned(plan: GainPlan, gains: &[f32], x: &mut [f32]) {
    match plan {
        GainPlan::Skip => {}
        GainPlan::Zero => x.fill(0.0),
        GainPlan::Ramp => {
            for (v, g) in x.iter_mut().zip(gains) {
                *v *= g;
            }
        }
    }
}

/// The five gain records of the output sum: dry (`+0xbf28`), wet (`+0xbf34`), main (`+0xbf40`),
/// solo (`+0xbf4c`) and out (`+0xbf58`), plus the solo filter state (`obj+0xb7a8`, two channels).
/// Their state persists across blocks, so the caller keeps one per instance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutputStage {
    pub dry: GainRecord,
    pub wet: GainRecord,
    pub main: GainRecord,
    pub solo: GainRecord,
    pub out: GainRecord,
    pub solo_states: [SoloChannel; 2],
}

impl OutputStage {
    /// The constructor's state, as read from the emulated object (`fixtures/output_solo.json`):
    /// dry `(0, 0)`, wet `(1, 1)`, main `(1, 1)`, solo `(0, 0)`, out `(1, 1)`, each with the
    /// ramp coefficient. The solo filter record starts at 1000 Hz, k = 1, mode 0, with its
    /// smoothing rate `1 - exp(-1/(0.03 fs))` in f64 (the gain records use the f32 constant).
    pub fn new(sample_rate: f32) -> Self {
        let a = ramp_coefficient(sample_rate);
        let fs = sample_rate as f64;
        let mut solo_state = SoloChannel {
            fs,
            mode: 0,
            target_r: 1000.0,
            target_k: 1.0,
            cur_r: 1000.0,
            cur_k: 1.0,
            tau_ms: 30.0,
            rate: 1.0 - (-1.0 / (0.03 * fs)).exp(),
            ..SoloChannel::default()
        };
        solo::svf_coefficients(&mut solo_state);
        Self {
            dry: GainRecord {
                target: 0.0,
                current: 0.0,
                coeff: a,
            },
            wet: GainRecord {
                target: 1.0,
                current: 1.0,
                coeff: a,
            },
            main: GainRecord {
                target: 1.0,
                current: 1.0,
                coeff: a,
            },
            solo: GainRecord {
                target: 0.0,
                current: 0.0,
                coeff: a,
            },
            out: GainRecord {
                target: 1.0,
                current: 1.0,
                coeff: a,
            },
            solo_states: [solo_state; 2],
        }
    }
}

/// The output sum of one block (`0x890a00`, output side, verified by emulation).
///
/// `dry` is the delayed input block, `wet` the processed block, both one slice per channel
/// (1 or 2 channels). `out` receives the result. Per sub-block of at most [`SUB_BLOCK`]
/// samples, as the binary does (`fixtures/output_block.json`, `fixtures/output_solo.json`):
///
/// 1. the targets of [`OutputTargets::compute`] are written to the five records;
/// 2. `out = wet`, then `out *= g_wet` (ramp of the wet record);
/// 3. `scratch = dry`, then `scratch *= g_dry` (ramp of the dry record), `out += scratch`;
/// 4. solo path, when `!bypass && (solo_active || g_solo > GATE)`: the solo filter runs on the
///    the sum `out` (wet + dry, before the main ramp), `S = solo(sum)`, then `S *= g_solo`,
///    `out *= g_main`, `out += S`;
/// 5. `out *= g_out` (ramp of the out record).
///
/// The solo gate is evaluated per sub-block, from the solo record's current gain. Skipped
/// sub-blocks leave the solo filter state and the solo and main records alone.
pub fn process_output_block(
    stage: &mut OutputStage,
    p: &OutputParams,
    dry: &[&[f32]],
    wet: &[&[f32]],
    out: &mut [&mut [f32]],
) {
    let ch = out.len();
    assert!((1..=2).contains(&ch), "output stage handles mono or stereo");
    assert!(
        dry.len() == ch && wet.len() == ch,
        "one dry and one wet slice per channel"
    );
    let n = out[0].len();

    let t = OutputTargets::compute(p);
    stage.dry.target = t.dry;
    stage.wet.target = t.wet;
    stage.main.target = t.main;
    stage.solo.target = t.solo;
    stage.out.target = t.out;

    let mut gains = [0.0f32; SUB_BLOCK];
    let mut scratch = [[0.0f32; SUB_BLOCK]; 2];
    let mut solo_buf = [[0.0f32; SUB_BLOCK]; 2];
    let mut sum = [[0.0f32; SUB_BLOCK]; 2];
    let mut s = 0;
    while s < n {
        let m = (n - s).min(SUB_BLOCK);
        for c in 0..ch {
            out[c][s..s + m].copy_from_slice(&wet[c][s..s + m]);
        }
        let plan = stage.wet.plan(m, &mut gains);
        for c in 0..ch {
            apply_planned(plan, &gains[..m], &mut out[c][s..s + m]);
        }

        for c in 0..ch {
            scratch[c][..m].copy_from_slice(&dry[c][s..s + m]);
        }
        let plan = stage.dry.plan(m, &mut gains);
        for c in 0..ch {
            apply_planned(plan, &gains[..m], &mut scratch[c][..m]);
            for (o, d) in out[c][s..s + m].iter_mut().zip(&scratch[c][..m]) {
                *o += d;
            }
        }

        // Solo path (`0x8923a7..0x892781`): bypass skips it; otherwise it runs while the solo
        // band is active or its gain is still above the gate (`comiss` against `0x18209d03c`).
        // The solo filter reads the sum wet + dry before the main ramp (checked in the trace).
        if !p.bypass && (p.solo_active || !(GATE >= stage.solo.current)) {
            for c in 0..ch {
                sum[c][..m].copy_from_slice(&out[c][s..s + m]);
            }
            let sum_in: [&[f32]; 2] = [&sum[0][..m], &sum[ch - 1][..m]];
            {
                let (s0, s1) = solo_buf.split_at_mut(1);
                let mut solo_out: [&mut [f32]; 2] = [&mut s0[0][..m], &mut s1[0][..m]];
                solo::process(
                    &mut stage.solo_states[..ch],
                    &p.solo_band,
                    &sum_in[..ch],
                    &mut solo_out[..ch],
                    m,
                );
            }
            let plan = stage.solo.plan(m, &mut gains);
            for c in 0..ch {
                apply_planned(plan, &gains[..m], &mut solo_buf[c][..m]);
            }
            let plan = stage.main.plan(m, &mut gains);
            for c in 0..ch {
                apply_planned(plan, &gains[..m], &mut out[c][s..s + m]);
            }
            for c in 0..ch {
                for (o, v) in out[c][s..s + m].iter_mut().zip(&solo_buf[c][..m]) {
                    *o += v;
                }
            }
        }

        let plan = stage.out.plan(m, &mut gains);
        for c in 0..ch {
            apply_planned(plan, &gains[..m], &mut out[c][s..s + m]);
        }
        s += m;
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
    /// The solo band's fields (`obj + 0x10c + 0x20 * idx`), read by the solo filter.
    pub solo_band: SoloBand,
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
            solo_band: SoloBand::default(),
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
        let (main, solo) = if p.solo_active {
            (0.0, 1.0)
        } else {
            (1.0, 0.0)
        };
        Self {
            dry,
            wet,
            main,
            solo,
            out,
        }
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
    /// getLatency at 48 kHz on the stand-in (`fixtures/output_block.json`).
    const DELAY_SAMPLES: usize = 2304;

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
        assert!(
            cases.len() >= 500,
            "fixture should cover the gate branches and rates"
        );
        for (idx, case) in cases.iter().enumerate() {
            let n = case["n"].as_u64().unwrap() as usize;
            let rec_in = f32s(&case["rec_in"]);
            let rec_out = f32s(&case["rec_out"]);
            let mut rec = GainRecord {
                target: rec_in[0],
                current: rec_in[1],
                coeff: rec_in[2],
            };
            let mut bufs: Vec<Vec<f32>> = case["in"].as_array().unwrap().iter().map(f32s).collect();
            let mut refs: Vec<&mut [f32]> = bufs.iter_mut().map(|b| b.as_mut_slice()).collect();
            rec.apply(&mut refs, n);

            assert_eq!(
                rec.target.to_bits(),
                rec_out[0].to_bits(),
                "case {idx} target"
            );
            assert_eq!(
                rec.current.to_bits(),
                rec_out[1].to_bits(),
                "case {idx} current"
            );
            assert_eq!(
                rec.coeff.to_bits(),
                rec_out[2].to_bits(),
                "case {idx} coeff"
            );
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
        assert!(
            cases.len() >= 32,
            "fixture should cover identity, delta, bypass and solo"
        );
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
                solo_band: SoloBand::default(),
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
                assert_eq!(
                    value.to_bits(),
                    bits(&want[name]).to_bits(),
                    "case {idx} {name}"
                );
            }
        }
    }

    #[test]
    fn unity_and_zero_gates_leave_their_documented_state() {
        let mut settled = GainRecord {
            target: 1.0,
            current: 1.0 - 5e-4,
            coeff: 0.1,
        };
        let mut buf = vec![0.25f32; 4];
        settled.apply(&mut [buf.as_mut_slice()], 4);
        assert_eq!(buf, vec![0.25; 4], "settled at unity: no multiply");
        assert_eq!(
            settled.current,
            1.0 - 5e-4,
            "settled at unity: g is kept, not snapped"
        );

        let mut mute = GainRecord {
            target: 0.0,
            current: 5e-4,
            coeff: 0.1,
        };
        mute.apply(&mut [buf.as_mut_slice()], 4);
        assert_eq!(buf, vec![0.0; 4], "both under the gate: zeroed");
        assert_eq!(mute.current, 5e-4, "zeroed path does not step g");
    }

    #[test]
    fn bypass_and_delta_targets_match_the_block() {
        let p = OutputParams {
            wet_gain: 2.0,
            mix: 0.25,
            out_gain: 0.5,
            ..OutputParams::default()
        };
        let t = OutputTargets::compute(&p);
        assert_eq!((t.dry, t.wet, t.out), (0.75, 0.5, 0.5));
        assert_eq!((t.main, t.solo), (1.0, 0.0));

        let d = OutputParams { delta: true, ..p };
        let t = OutputTargets::compute(&d);
        assert_eq!(
            (t.dry, t.wet),
            (0.5, -0.5),
            "delta: output = wm * (dry - wet)"
        );

        let b = OutputParams {
            bypass: true,
            delta: true,
            ..p
        };
        let t = OutputTargets::compute(&b);
        assert_eq!(
            (t.dry, t.wet, t.out),
            (1.0, 0.0, 1.0),
            "bypass ignores trims and mix"
        );
    }

    #[test]
    fn solo_engages_delta_and_routes_the_solo_stream() {
        let p = OutputParams {
            solo_active: true,
            solo_flag_236: true,
            ..OutputParams::default()
        };
        assert!(delta_mode(&p));
        let t = OutputTargets::compute(&p);
        assert_eq!((t.main, t.solo), (0.0, 1.0));
        let no_flag = OutputParams {
            solo_active: true,
            ..OutputParams::default()
        };
        assert!(!delta_mode(&no_flag));
    }

    #[test]
    fn output_block_matches_the_emulated_process_block() {
        const BLOCK: &str = include_str!("fixtures/output_block.json");
        let fixture: serde_json::Value = serde_json::from_str(BLOCK).unwrap();
        let n = fixture["n"].as_u64().unwrap() as usize;
        let rate = fixture["sample_rate"].as_f64().unwrap() as f32;
        let bits = |v: &serde_json::Value| f32::from_bits(v.as_u64().unwrap() as u32);
        let block = |v: &serde_json::Value| -> Vec<Vec<f32>> {
            v.as_array().unwrap().iter().map(f32s).collect()
        };
        let wet = block(&fixture["wet_block"]);
        let dry = block(&fixture["dry_block"]);
        let dry_refs: Vec<&[f32]> = dry.iter().map(|c| c.as_slice()).collect();
        let wet_refs: Vec<&[f32]> = wet.iter().map(|c| c.as_slice()).collect();
        let cases = fixture["cases"].as_array().unwrap();
        assert!(
            cases.len() >= 6 && n > DELAY_SAMPLES,
            "fixture covers the delay and every output mode"
        );

        for (idx, case) in cases.iter().enumerate() {
            let p = &case["params"];
            let params = OutputParams {
                wet_gain: bits(&p["wet_gain"]),
                mix: bits(&p["mix"]),
                out_gain: bits(&p["out_gain"]),
                delta: p["delta"].as_bool().unwrap(),
                bypass: p["bypass"].as_bool().unwrap(),
                solo_active: false,
                solo_flag_236: false,
                band_flags_any: false,
                channels: 2,
                solo_band: SoloBand::default(),
            };
            // Records as the constructor leaves them (dry, wet, out), taken from the emulated object.
            let rec = |name: &str| {
                let r = f32s(&case["rec_in"][name]);
                GainRecord {
                    target: r[0],
                    current: r[1],
                    coeff: r[2],
                }
            };
            let mut stage = OutputStage::new(rate);
            stage.dry = rec("dry");
            stage.wet = rec("wet");
            stage.out = rec("out");
            assert_eq!(
                stage.wet.coeff.to_bits(),
                OutputStage::new(rate).wet.coeff.to_bits(),
                "ramp coefficient"
            );

            let mut bufs: Vec<Vec<f32>> = vec![vec![0.0; n]; 2];
            let mut outs: Vec<&mut [f32]> = bufs.iter_mut().map(|b| b.as_mut_slice()).collect();
            process_output_block(&mut stage, &params, &dry_refs, &wet_refs, &mut outs);

            for (c, expected) in case["out"].as_array().unwrap().iter().enumerate() {
                let want = f32s(expected);
                let peak = want.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-3);
                for (i, (g, e)) in bufs[c].iter().zip(&want).enumerate() {
                    let err = (g - e).abs() / peak;
                    assert!(
                        err <= 1e-6,
                        "case {idx} ch {c} sample {i}: got {g}, emulated {e}"
                    );
                }
            }
        }
    }

    fn solo_state_from_bits(v: &serde_json::Value) -> SoloChannel {
        let d: Vec<u64> = v
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_u64().unwrap())
            .collect();
        let f = |k: usize| f64::from_bits(d[k]);
        SoloChannel {
            fs: f(0),
            mode: d[1] as u32 as i32,
            target_r: f(2),
            target_k: f(3),
            cur_r: f(4),
            cur_k: f(5),
            tau_ms: f(6),
            rate: f(7),
            g: f(8),
            k: f(9),
            a1: f(10),
            a2: f(11),
            a3: f(12),
            ic1: f(13),
            ic2: f(14),
        }
    }

    fn rec_from_bits(v: &serde_json::Value) -> GainRecord {
        let r = f32s(v);
        GainRecord {
            target: r[0],
            current: r[1],
            coeff: r[2],
        }
    }

    /// Relative comparison of two f64 values, bit-exact or within 1e-6 of the larger magnitude.
    fn close64(got: f64, want: f64) -> bool {
        got.to_bits() == want.to_bits() || (got - want).abs() <= 1e-6 * want.abs().max(1e-12)
    }

    /// Replays each emulated block from the record and solo state the emulator had before it.
    /// Checks the output channels to 1e-6 relative to each channel's peak, and the records and
    /// solo state after the block.
    #[test]
    fn solo_output_block_matches_the_emulated_process_block() {
        const SOLO: &str = include_str!("fixtures/output_solo.json");
        let fixture: serde_json::Value = serde_json::from_str(SOLO).unwrap();
        let n = fixture["n"].as_u64().unwrap() as usize;
        let rate = fixture["sample_rate"].as_u64().unwrap() as f32;
        let bits = |v: &serde_json::Value| f32::from_bits(v.as_u64().unwrap() as u32);
        let frames = |v: &serde_json::Value| -> Vec<Vec<f32>> {
            v.as_array().unwrap().iter().map(f32s).collect()
        };

        let mut blocks = 0;
        let mut solo_samples = 0usize;
        for case in fixture["cases"].as_array().unwrap() {
            for (b, block) in case["blocks"].as_array().unwrap().iter().enumerate() {
                let p = &block["params"];
                let band = &block["band"];
                let params = OutputParams {
                    wet_gain: bits(&p["wet_gain"]),
                    mix: bits(&p["mix"]),
                    out_gain: bits(&p["out_gain"]),
                    delta: p["delta"].as_u64().unwrap() != 0,
                    bypass: p["bypass"].as_u64().unwrap() != 0,
                    solo_active: p["solo_active"].as_u64().unwrap() != 0,
                    solo_flag_236: p["flag_236"].as_u64().unwrap() != 0,
                    band_flags_any: false,
                    channels: 2,
                    solo_band: SoloBand {
                        freq: f32::from_bits(band["freq"].as_u64().unwrap() as u32),
                        half_bw_oct: f32::from_bits(band["half_bw"].as_u64().unwrap() as u32),
                        shape: band["shape"].as_u64().unwrap() as u32,
                    },
                    ..OutputParams::default()
                };
                let rec = &block["rec_in"];
                let mut stage = OutputStage::new(rate);
                stage.dry = rec_from_bits(&rec["dry"]);
                stage.wet = rec_from_bits(&rec["wet"]);
                stage.main = rec_from_bits(&rec["main"]);
                stage.solo = rec_from_bits(&rec["solo"]);
                stage.out = rec_from_bits(&rec["out"]);
                let st = block["state_in"].as_array().unwrap();
                stage.solo_states = [solo_state_from_bits(&st[0]), solo_state_from_bits(&st[1])];

                let dry = frames(&block["dry"]);
                let wet = frames(&block["wet"]);
                let dry_refs: Vec<&[f32]> = dry.iter().map(|c| c.as_slice()).collect();
                let wet_refs: Vec<&[f32]> = wet.iter().map(|c| c.as_slice()).collect();
                let mut bufs: Vec<Vec<f32>> = vec![vec![0.0; n]; 2];
                let mut outs: Vec<&mut [f32]> = bufs.iter_mut().map(|v| v.as_mut_slice()).collect();
                process_output_block(&mut stage, &params, &dry_refs, &wet_refs, &mut outs);

                for (c, want) in frames(&block["out"]).iter().enumerate() {
                    let peak = want.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-3);
                    for (i, (g, e)) in bufs[c].iter().zip(want).enumerate() {
                        assert!(
                            (g - e).abs() / peak <= 1e-6,
                            "case {} block {b} ch {c} sample {i}: got {g}, emulated {e}",
                            case["name"]
                        );
                    }
                    solo_samples += want.iter().filter(|v| **v != 0.0).count();
                }

                let want_rec = &block["rec_out"];
                for (name, got) in [
                    ("dry", stage.dry),
                    ("wet", stage.wet),
                    ("main", stage.main),
                    ("solo", stage.solo),
                    ("out", stage.out),
                ] {
                    let w = rec_from_bits(&want_rec[name]);
                    assert_eq!(got.target.to_bits(), w.target.to_bits(), "{name} target");
                    assert!(
                        (got.current - w.current).abs() <= 1e-6 * w.current.abs().max(1e-6),
                        "{name} current: got {}, emulated {}",
                        got.current,
                        w.current
                    );
                }
                let want_st = block["state_out"].as_array().unwrap();
                for (c, want) in want_st.iter().enumerate() {
                    let w = solo_state_from_bits(want);
                    let g = stage.solo_states[c];
                    assert_eq!(g.mode, w.mode, "solo ch {c} mode");
                    for (name, gv, wv) in [
                        ("cur_r", g.cur_r, w.cur_r),
                        ("cur_k", g.cur_k, w.cur_k),
                        ("g", g.g, w.g),
                        ("k", g.k, w.k),
                        ("a1", g.a1, w.a1),
                        ("ic1", g.ic1, w.ic1),
                        ("ic2", g.ic2, w.ic2),
                    ] {
                        assert!(
                            close64(gv, wv),
                            "solo ch {c} {name}: got {gv:e}, emulated {wv:e}"
                        );
                    }
                }
                blocks += 1;
            }
        }
        assert!(
            blocks >= 7,
            "fixture covers solo on, fade-out, trims, delta, bypass and Tilt"
        );
        assert!(
            solo_samples > 1000,
            "the solo path must contribute non-zero output"
        );
    }

    /// The constructor state of [`OutputStage::new`] against the emulated object (first block of a case).
    #[test]
    fn output_stage_new_matches_the_constructor_state() {
        const SOLO: &str = include_str!("fixtures/output_solo.json");
        let fixture: serde_json::Value = serde_json::from_str(SOLO).unwrap();
        let rate = fixture["sample_rate"].as_u64().unwrap() as f32;
        let first = &fixture["cases"][0]["blocks"][0];
        let stage = OutputStage::new(rate);
        assert_eq!(stage.solo.target.to_bits(), 0);
        assert_eq!(stage.main.current.to_bits(), 1.0f32.to_bits());
        let st = first["state_in"].as_array().unwrap();
        for (c, want) in st.iter().enumerate() {
            let w = solo_state_from_bits(want);
            let g = stage.solo_states[c];
            assert_eq!(g.mode, w.mode, "mode");
            for (name, gv, wv) in [
                ("fs", g.fs, w.fs),
                ("target_r", g.target_r, w.target_r),
                ("cur_r", g.cur_r, w.cur_r),
                ("target_k", g.target_k, w.target_k),
                ("cur_k", g.cur_k, w.cur_k),
                ("tau_ms", g.tau_ms, w.tau_ms),
                ("rate", g.rate, w.rate),
                ("g", g.g, w.g),
                ("k", g.k, w.k),
                ("a1", g.a1, w.a1),
                ("a2", g.a2, w.a2),
                ("a3", g.a3, w.a3),
            ] {
                assert!(
                    close64(gv, wv),
                    "ch {c} {name}: got {gv:e}, emulated {wv:e}"
                );
            }
        }
    }

    /// `0x8a3b20` against the emulated helper, bit for bit. Flag 1 is the encode site
    /// (`0x1808928a3`, gain 0.5), flag 0 the decode site (`0x18089a5bc`, gain 1.0). Mode 1 copies
    /// the first channel.
    #[test]
    fn mid_side_helper_matches_the_emulated_helper_bit_for_bit() {
        const MS: &str = include_str!("fixtures/mid_side.json");
        let fixture: serde_json::Value = serde_json::from_str(MS).unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert!(cases.len() >= 50, "fixture covers encode, decode and copy");
        for (idx, case) in cases.iter().enumerate() {
            let src = [f32s(&case["src"][0]), f32s(&case["src"][1])];
            let want = [f32s(&case["out"][0]), f32s(&case["out"][1])];
            let flag = case["flag"].as_u64().unwrap();
            let mode = case["mode"].as_u64().unwrap();
            if mode == 1 {
                assert_eq!(want[0], src[0], "case {idx}: copy of the first channel");
                continue;
            }
            let gain = if flag == 1 { 0.5 } else { 1.0 };
            for (i, (&l, &r)) in src[0].iter().zip(&src[1]).enumerate() {
                let (m, s) = mid_side_pair(l, r, gain);
                assert_eq!(
                    m.to_bits(),
                    want[0][i].to_bits(),
                    "case {idx} sample {i} mid"
                );
                assert_eq!(
                    s.to_bits(),
                    want[1][i].to_bits(),
                    "case {idx} sample {i} side"
                );
            }
        }
    }

    #[test]
    fn mid_side_round_trips_at_unity_after_half_on_encode() {
        let (m, s) = mid_side_pair(0.6, 0.2, 0.5);
        let (l, r) = mid_side_pair(m, s, 1.0);
        assert!((l - 0.6).abs() < 1e-7 && (r - 0.2).abs() < 1e-7);
    }
}
