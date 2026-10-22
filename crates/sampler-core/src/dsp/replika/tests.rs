//! Native vector tests for the Replika core (piece 1). The vectors are the emulated output of the original
//! Kontakt 8 code (kemu, `tools/re/replika_kemu_vectors.py`), 4096 frames at 48 kHz, 32-frame quanta. They
//! are native reference output, not a Kontakt render. Tolerance: 1e-5 absolute.
use serde_json::Value;

use super::*;

const N: usize = 4096;
const TOLERANCE: f32 = 1e-5;
/// The wrapper's native block: setters are applied per 32-frame quantum (`replika_kemu_vectors.py`).
const QUANTUM: usize = 32;

const VECTOR_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/re/engine/vectors/");

fn vector(name: &str) -> Value {
    let text = match name {
        "default" => include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/re/engine/vectors/replika-native-default.json"
        )),
        "time-v0p01" => include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/re/engine/vectors/replika-native-time-v0p01.json"
        )),
        "time-v0p03" => include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/re/engine/vectors/replika-native-time-v0p03.json"
        )),
        "feedback-v0p8" => include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/re/engine/vectors/replika-native-feedback-v0p8.json"
        )),
        "saturation-v0p5" => include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/re/engine/vectors/replika-native-saturation-v0p5.json"
        )),
        "lowcut-v0p5" => include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/re/engine/vectors/replika-native-lowcut-v0p5.json"
        )),
        "highcut-v0p5" => include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/re/engine/vectors/replika-native-highcut-v0p5.json"
        )),
        "ducking-amount0" => include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/re/engine/vectors/replika-native-ducking-amount0.json"
        )),
        "ducking-amount0p5" => include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/re/engine/vectors/replika-native-ducking-amount0p5.json"
        )),
        "ducking-amount0p5-release1" => include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/re/engine/vectors/replika-native-ducking-amount0p5-release1.json"
        )),
        other => panic!("no vector {other} in {VECTOR_DIR}"),
    };
    serde_json::from_str(text).expect("vector json")
}

fn pairs(v: &Value) -> Vec<(usize, f32)> {
    v.as_array()
        .expect("pair list")
        .iter()
        .map(|p| {
            (
                p[0].as_u64().expect("index") as usize,
                p[1].as_f64().expect("value") as f32,
            )
        })
        .collect()
}

fn floats(v: &Value) -> Vec<f32> {
    v.as_array()
        .expect("samples")
        .iter()
        .map(|x| x.as_f64().expect("sample") as f32)
        .collect()
}

/// Run a vector through the core and return (max abs error over both channels, first frame over tolerance).
fn check(name: &str) -> (f32, Option<usize>) {
    let v = vector(name);
    assert_eq!(v["rate"].as_u64(), Some(48_000));
    assert_eq!(v["block"].as_u64(), Some(QUANTUM as u64));
    let rate = 48_000.;
    let mut core = ReplikaCore::new(rate);
    for (i, value) in pairs(&v["defaults"]) {
        core.set_param(i, value);
    }
    let writes: Vec<Vec<(usize, f32)>> = v["writes"]
        .as_array()
        .expect("writes")
        .iter()
        .map(pairs)
        .collect();
    let stim = v["stim"].as_array().expect("stim");
    let (stim_l, stim_r): (Vec<f64>, Vec<f64>) = (
        floats(&stim[0]).iter().map(|&x| f64::from(x)).collect(),
        floats(&stim[1]).iter().map(|&x| f64::from(x)).collect(),
    );
    let mut state = ReplikaState::new(rate);
    let mut storage = vec![[0.; 2]; storage_len(rate)];
    let (mut out_l, mut out_r) = (vec![0.; N], vec![0.; N]);
    for (q, quantum) in writes.iter().enumerate() {
        for &(i, value) in quantum {
            core.set_param(i, value);
        }
        let span = q * QUANTUM..(q + 1) * QUANTUM;
        core.process(
            &mut state,
            &mut storage,
            [&stim_l[span.clone()], &stim_r[span.clone()]],
            [&mut out_l[span.clone()], &mut out_r[span]],
        );
    }
    let (want_l, want_r) = (floats(&v["outL"]), floats(&v["outR"]));
    let mut max = 0f32;
    let mut first = None;
    for i in 0..N {
        let e = (out_l[i] as f32 - want_l[i])
            .abs()
            .max((out_r[i] as f32 - want_r[i]).abs());
        if e > TOLERANCE && first.is_none() {
            first = Some(i);
        }
        max = max.max(e);
    }
    (max, first)
}

fn assert_case(name: &str) {
    let (max, first) = check(name);
    eprintln!("replika {name}: max abs error {max:.3e} (first frame over 1e-5: {first:?})");
    assert!(
        max < TOLERANCE,
        "{name}: max abs error {max:.3e}, first frame over tolerance {first:?}"
    );
}

#[test]
fn default_impulse_matches_native_vector() {
    assert_case("default");
}

#[test]
fn time_matches_native_vectors_at_two_settings() {
    assert_case("time-v0p01");
    assert_case("time-v0p03");
}

#[test]
fn feedback_matches_native_vector() {
    assert_case("feedback-v0p8");
}

#[test]
fn lowcut_and_highcut_match_native_vectors() {
    assert_case("lowcut-v0p5");
    assert_case("highcut-v0p5");
}

#[test]
fn saturation_matches_native_vector() {
    assert_case("saturation-v0p5");
}

/// Ducker (gate of `141aa9480`): 1 kHz burst of 0.5 for 2048 frames, sense 0.8 (threshold 0.2), release 0
/// (200 ms). The amount-0 case is the same run with target 0 dB; the 0.5 case ducks the echo to about 0.08;
/// the release-1 case (7500 ms) keeps the duck through the window.
#[test]
fn ducking_matches_native_vectors_at_amount_zero_and_half() {
    assert_case("ducking-amount0");
    assert_case("ducking-amount0p5");
    assert_case("ducking-amount0p5-release1");
}

/// The measured input law (index 0 at the reset value is 1.0000119; at 1.0 it is 3.98113).
#[test]
fn input_level_law_matches_the_measured_points() {
    assert!(
        (level(0.762_405) - 1.000_011_9).abs() < 2e-6,
        "{}",
        level(0.762_405)
    );
    assert!((level(1.) / 3.981_133_7 - 1.).abs() < 2e-6, "{}", level(1.));
    assert!((level(0.) - 1.980_545_8e-7).abs() < 1e-7, "{}", level(0.));
}

/// The head delays of the default impulse (`H+0xd8`, `H+0xe0`, bias -2 after the first update).
#[test]
fn default_head_delays_are_the_measured_bases() {
    let core = ReplikaCore::new(48_000.);
    assert_eq!(core.delays(), [[1742, 2102], [2282, 1562], [1562, 2282]]);
}

/// The feedback gain is 1.2 x the setter value (`1479ab410`, `141ac9420`); 0.8 gives 0.96.
#[test]
fn feedback_gain_is_one_point_two_times_the_setter() {
    let mut core = ReplikaCore::new(48_000.);
    core.set_param(index::FEEDBACK, 0.8);
    assert!(
        (core.feedback_gain - 0.96).abs() < 1e-6,
        "{}",
        core.feedback_gain
    );
}

/// Setters clamp to 0..=1 as the wrapper does; NaN reads as 0.
#[test]
fn setters_clamp_to_the_unit_interval() {
    let mut core = ReplikaCore::new(48_000.);
    core.set_param(index::SATURATION, 1.7);
    assert_eq!(core.saturation, 1.);
    core.set_param(index::SATURATION, -0.3);
    assert_eq!(core.saturation, 0.);
    core.set_param(index::SATURATION, f32::NAN);
    assert_eq!(core.saturation, 0.);
}

/// The slab packing restores the state exactly: a run resumed from the packed state equals the unbroken
/// run, and zero slots are the reset state.
#[test]
fn packed_state_resumes_the_run_exactly() {
    let rate = 48_000.;
    let controls = ReplikaControls {
        feedback: 0.8,
        saturation: 0.5,
        ..ReplikaControls::default()
    };
    let core = ReplikaCore::from_controls(rate, &controls);
    assert_eq!(
        ReplikaState::load(rate, &[[0.; 2]; STATE_SLOTS]),
        ReplikaState::new(rate)
    );
    let x: Vec<f64> = (0..600).map(|i| (0.1 * f64::from(i)).sin()).collect();
    let (mut want_l, mut want_r) = (vec![0.; 600], vec![0.; 600]);
    let mut storage = vec![[0.; 2]; storage_len(rate)];
    core.process(
        &mut ReplikaState::new(rate),
        &mut storage,
        [&x[..], &x[..]],
        [&mut want_l[..], &mut want_r[..]],
    );

    let mut storage = vec![[0.; 2]; storage_len(rate)];
    let (mut got_l, mut got_r) = (vec![0.; 600], vec![0.; 600]);
    let mut st = ReplikaState::new(rate);
    core.process(
        &mut st,
        &mut storage,
        [&x[..300], &x[..300]],
        [&mut got_l[..300], &mut got_r[..300]],
    );
    let mut slots = [[0.; 2]; STATE_SLOTS];
    st.store(&mut slots);
    let mut st = ReplikaState::load(rate, &slots);
    core.process(
        &mut st,
        &mut storage,
        [&x[300..], &x[300..]],
        [&mut got_l[300..], &mut got_r[300..]],
    );
    assert_eq!(got_l, want_l);
    assert_eq!(got_r, want_r);
}

/// The ring holds the longest read: TIME 2000 ms (the clamp of `0.5 x v x 4000`) plus the largest offset.
#[test]
fn ring_holds_the_longest_head_read() {
    let longest = 96_000 + 540 + 2;
    assert!(
        ring_len(48_000.) > longest,
        "ring {} vs read {longest}",
        ring_len(48_000.)
    );
}
