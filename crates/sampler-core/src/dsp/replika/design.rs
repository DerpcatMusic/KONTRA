//! Mode-1 design chain of the Kontakt 8 Replika head: the setup that turns the mode (`H+0x8c`) and the sample
//! rate into the coefficients of the two write lanes and the two post filters (`141abbc90`).
//!
//! Native units (`docs/re/engine/disasm/replika-mode1-listings.txt`): the lane setup `141ac3fd0` takes the mode
//! from edx and the rate from `lane+0x1450`. Its section helpers are `1419529e0` (g = fifth-order tangent series
//! of pi f / fs), `141ac96c0` (k = 2 - 2 arg) and `14199dbf0` (k = r - 1/r with r = 2^(bw/2), the bandwidth in
//! octaves). Each helper ends in the `Section::new(g, k)` algebra. The envelope smoothing is exp(C / fs) with
//! C = -75 pi for the lanes (`1479ab720`) and C = -50 pi for the post filters (`1479ab718`). The post loop
//! writes its gain (the damping k) at `+0x54`, the field `PostFilter` calls the gain.
//!
//! `pow` and `exp` are the libm calls `1443d10b4` and `1443d10cc`; here they run in f64 and round to f32.
//!
//! Not ported here: the tail of `141abbc90` (the kernel flags `K+0xc8` = 1 for lane modes 1..3, else 2, and the
//! resets `141abaab0` of both taps), which `Head::set_mode` applies for k = 1, and the per-mode read and speed
//! selection `141abd190` and `141ac4510` (piece 3b).
//!
//! Vectors: `docs/re/engine/vectors/replika-native-mode1-design.json` (kemu, rates 44.1/48/96 kHz, modes 0..3),
//! plus the core-run mode-3 vectors `replika-native-mode1-write-l.json` and `replika-native-mode1-postfilter-l.json`.

use crate::dsp::tpt_svf::Section;

/// `1479ab720`: -75 pi, the lane envelope exponent.
const LANE_EXP: f64 = f64::from_bits(0xc06d_73d2_86bf_e4d8);
/// `1479ab718`: -50 pi, the post filter envelope exponent.
const POST_EXP: f64 = f64::from_bits(0xc063_a28c_59d5_433b);
/// `144760108`, `144fcfce0`: the 1/3 and 2/15 of the tangent series (f32 bit patterns).
const TAN_THIRD: f32 = f32::from_bits(0x3eaa_aaab);
const TAN_2_15: f32 = f32::from_bits(0x3e08_8889);

/// The g part of `1419529e0`: tan(x) to fifth order (`x + x^3/3 + 2x^5/15`), in the native f32 order. Shared
/// with the tap section `14199dcc0` (`replika::read`).
pub(super) fn tan_poly(x: f32) -> f32 {
    let x2 = x * x;
    let x3 = x2 * x;
    let x5 = x3 * x2;
    x3 * TAN_THIRD + x + x5 * TAN_2_15
}

/// `1419529e0` with x = pi f / fs, the argument rounded to f32 as the native `cvtpd2ps`.
fn tan_series(f: f32, fs: f32) -> f32 {
    tan_poly((f64::from(f) * std::f64::consts::PI / f64::from(fs)) as f32)
}

/// `141ac96c0`: the damping k = 2 - 2 arg.
fn damping_linear(arg: f32) -> f32 {
    2. - (arg + arg)
}

/// `14199dbf0`: the damping k = r - 1/r with r = 2^(bw / 2), `bw` in octaves.
fn damping_octaves(bw: f32) -> f32 {
    let r = 2f64.powf(f64::from(bw * 0.5)) as f32;
    (r * r - 1.) / r
}

/// One write lane after `141ac3fd0` (modes 0..3). Mode 4 and up leave the lane untouched in the native code.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LaneDesign {
    /// `+0x54`: envelope smoothing, exp(-75 pi / fs).
    pub env_coef: f32,
    /// Sections A (`+0x58`) and B (`+0x8c`).
    pub sec_a: Section,
    pub sec_b: Section,
    /// `+0x1464`, `+0x1468`, `+0x146c`, `+0x1470`.
    pub in_scale: f32,
    pub out_gain: f32,
    pub adaa_k: f32,
    pub dither_gain: f32,
}

/// The design of one write lane for `mode` at sample rate `fs`. `None` for modes 4 and up.
pub fn lane(fs: f32, mode: u32) -> Option<LaneDesign> {
    // (F_A, F_B, k arg A, k arg B, in_scale, out_gain, adaa_k, dither_gain), one row per branch of 141ac3fd0.
    let (fa, fb, ka, kb, in_scale, out_gain, adaa_k, dither_gain): (
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
    ) = match mode {
        0 => (9000., 8500., 0.75, 0.35, 0.5, 2., 0.04, 0.005),
        1 => (4000., 3500., 0.75, 0.4, 0.5, 2., 0.08, 0.015),
        2 => (2700., 2500., 0.85, 0.35, 0.5, 2., 0.15, 0.035),
        3 => (8000., 7000., 0.75, 0.35, 0.6, 1.6, 0.2, 0.01),
        _ => return None,
    };
    Some(LaneDesign {
        env_coef: (LANE_EXP / f64::from(fs)).exp() as f32,
        sec_a: Section::new(tan_series(fa, fs), damping_linear(ka)),
        sec_b: Section::new(tan_series(fb, fs), damping_linear(kb)),
        in_scale,
        out_gain,
        adaa_k,
        dither_gain,
    })
}

/// One post filter after the `141abbc90` loop body (modes 0..3).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PostDesign {
    /// `+0x24`: envelope smoothing, exp(-50 pi / fs).
    pub env_coef: f32,
    /// The section at `+0x28`. Its `r` is the gain at `+0x54`.
    pub sec: Section,
}

/// The design of one post filter for `mode` at sample rate `fs`. `None` for modes 4 and up.
pub fn post(fs: f32, mode: u32) -> Option<PostDesign> {
    // (frequency, bandwidth in octaves) per branch of the post loop.
    let (f, bw): (f32, f32) = match mode {
        0 => (150., 7.),
        1 => (150., 5.2),
        2 => (140., 4.),
        3 => (150., 8.),
        _ => return None,
    };
    Some(PostDesign {
        env_coef: (POST_EXP / f64::from(fs)).exp() as f32,
        sec: Section::new(tan_series(f, fs), damping_octaves(bw)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const TOLERANCE: f32 = 1e-5;

    fn vector(name: &str) -> Value {
        let path = format!(
            "{}/../../docs/re/engine/vectors/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(path).expect("vector file"))
            .expect("vector json")
    }

    fn word(v: &Value) -> u32 {
        v.as_u64().expect("u32 word") as u32
    }

    fn close(label: &str, got: f32, want: f32) {
        assert!(
            (got - want).abs() <= TOLERANCE,
            "{label}: got {got} want {want}"
        );
    }

    /// Lane words are windows: `lane_w` from `+0x50`, `lane_p` from `+0x1450`.
    fn check_lane(d: &LaneDesign, lane_w: &[Value], lane_p: &[Value], mode: u32, fs: f32) {
        let w = |off: usize| f32::from_bits(word(&lane_w[(off - 0x50) / 4]));
        let p = |off: usize| f32::from_bits(word(&lane_p[(off - 0x1450) / 4]));
        assert_eq!(p(0x1450), fs, "lane rate");
        assert_eq!(word(&lane_p[1]), mode, "lane mode at +0x1454");
        close("env", d.env_coef, w(0x54));
        let (c2, c3, c4, r) = d.sec_a.parts();
        close("A.c2", c2, w(0x60));
        close("A.c3", c3, w(0x64));
        close("A.c4", c4, w(0x68));
        close("A.k", r, w(0x84));
        let (c2, c3, c4, r) = d.sec_b.parts();
        close("B.c2", c2, w(0x94));
        close("B.c3", c3, w(0x98));
        close("B.c4", c4, w(0x9c));
        close("B.k", r, w(0xb8));
        close("in_scale", d.in_scale, p(0x1464));
        close("out_gain", d.out_gain, p(0x1468));
        close("adaa_k", d.adaa_k, p(0x146c));
        close("dither_gain", d.dither_gain, p(0x1470));
    }

    #[test]
    fn design_matches_native_vectors_for_every_rate_and_mode() {
        let doc = vector("replika-native-mode1-design.json");
        let cases = doc["cases"].as_array().expect("cases");
        assert_eq!(cases.len(), 12);
        for case in cases {
            let mode = case["mode"].as_u64().expect("mode") as u32;
            let fs = case["fs"].as_f64().expect("fs") as f32;
            let lanes = case["lanes"].as_array().expect("lanes");
            let posts = case["posts"].as_array().expect("posts");
            let d = lane(fs, mode).expect("modes 0..3");
            for l in lanes {
                check_lane(
                    &d,
                    l["w"].as_array().unwrap(),
                    l["p"].as_array().unwrap(),
                    mode,
                    fs,
                );
            }
            let post_d = post(fs, mode).expect("modes 0..3");
            for q in posts {
                let w = q["w"].as_array().expect("post words");
                let pw = |off: usize| f32::from_bits(word(&w[(off - 0x20) / 4]));
                assert_eq!(pw(0x5c), fs, "post rate");
                assert_eq!(word(&w[(0x60 - 0x20) / 4]), mode, "post mode at +0x60");
                close("post env", post_d.env_coef, pw(0x24));
                let (c2, c3, c4, r) = post_d.sec.parts();
                close("post c2", c2, pw(0x30));
                close("post c3", c3, pw(0x34));
                close("post c4", c4, pw(0x38));
                close("post gain", r, pw(0x54));
            }
        }
    }

    #[test]
    fn design_matches_the_core_run_mode_three_vectors() {
        // The write and post vectors were captured from a full core run at 48 kHz with the head in mode 3.
        let write = vector("replika-native-mode1-write-l.json");
        let words = write["words"].as_array().expect("write words");
        let d = lane(48000., 3).expect("mode 3");
        check_lane(
            &d,
            &words[0x50 / 4..0xc0 / 4],
            &words[0x1450 / 4..0x1480 / 4],
            3,
            48000.,
        );

        let post_doc = vector("replika-native-mode1-postfilter-l.json");
        let init = &post_doc["init"];
        let pd = post(48000., 3).expect("mode 3");
        let (c2, c3, c4, r) = pd.sec.parts();
        close(
            "core post env",
            pd.env_coef,
            init["coef"].as_f64().unwrap() as f32,
        );
        close("core post c2", c2, init["c2"].as_f64().unwrap() as f32);
        close("core post c3", c3, init["c3"].as_f64().unwrap() as f32);
        close("core post c4", c4, init["c4"].as_f64().unwrap() as f32);
        close("core post gain", r, init["gain"].as_f64().unwrap() as f32);
    }

    #[test]
    fn modes_four_and_up_leave_the_chain_alone() {
        assert!(lane(48000., 4).is_none());
        assert!(post(48000., 4).is_none());
    }
}
