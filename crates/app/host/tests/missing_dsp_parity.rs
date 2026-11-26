//! End-to-end live KSP (`$EFFECT_TYPE_*` dynamic slot load + live
//! `set_engine_par`/`get_engine_par`/`get_engine_par_disp`), numerical
//! transfer/timing verification, and rapid-drag RT-safety tests for:
//! Ring Modulator (0x64), Stereo Tune (0x70), Cry Wah (0x58),
//! Twin Delay (0x6b), and Plate Reverb (0x57).
#![cfg(test)]

use std::f32::consts::TAU;
use std::sync::Arc;

use kontra_engine::{Engine, Event, HostNote, Plan, Region, SampleTime, Timed, Zone};
use kontra_stream::{BlobKey, Encoding, MemSource, SampleFmt, Store, StoreConfig};

const RATE: u32 = 48_000;
const FMT: SampleFmt = SampleFmt {
    encoding: Encoding::F32,
    channels: 1,
};

fn zone_from(store: &mut Store, label: &str, samples: Vec<f32>) -> Zone {
    let frames = samples.len() as u64;
    let bytes: Vec<u8> = samples.into_iter().flat_map(f32::to_le_bytes).collect();
    let key = BlobKey {
        file: label.into(),
        offset: 0,
        fmt: FMT,
    };
    let sample = store
        .load(
            &key,
            Arc::new(MemSource::new(FMT, bytes)),
            frames,
            std::slice::from_ref(&(0..frames)),
        )
        .unwrap();
    let region = Region {
        mix: None,
        table: None,
        sample,
        start: 0,
        end: frames,
        loops: Default::default(),
        reverse: false,
    };
    Zone::new(region, 60, RATE)
}

fn sine_zone(store: &mut Store, hz: f32, amp: f32, frames: usize) -> Zone {
    let samples = (0..frames)
        .map(|n| amp * (TAU * hz * n as f32 / RATE as f32).sin())
        .collect();
    zone_from(store, &format!("sine_{hz}_{amp}_{frames}"), samples)
}

fn impulse_zone(store: &mut Store, amp: f32, frames: usize) -> Zone {
    let mut samples = vec![0.0f32; frames];
    if let Some(first) = samples.first_mut() {
        *first = amp;
    }
    zone_from(store, &format!("imp_{amp}_{frames}"), samples)
}

fn build_engine(store: &Store, zones: Vec<Zone>, source: &str) -> Engine {
    let script = kontra_ksp::compile(source, &[]).unwrap();
    let mut plan = Plan::new(zones, 16);
    for g in &mut plan.groups {
        g.mods.clear();
    }
    plan.inserts = vec![None; 8];
    plan.slot_shapes.types = kontra_format_kontakt::slot_types();
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    Engine::new(Arc::new(plan), store.client(), RATE)
}

fn note_on(key: u8) -> Timed<Event> {
    Timed {
        at: SampleTime(0),
        body: Event::NoteOn {
            note: HostNote {
                port: 0,
                channel: 0,
                key,
                id: -1,
                clap: true,
            },
            velocity: 1.0,
            tune: 0.0,
        },
    }
}

fn cc(controller: u8, value: u8) -> Timed<Event> {
    Timed {
        at: SampleTime(0),
        body: Event::midi1(0xB0, controller, value),
    }
}

fn render(engine: &mut Engine, events: &[Timed<Event>], frames: usize) -> Vec<[f32; 2]> {
    let mut out = vec![[0.0f32; 2]; frames];
    let mut first = true;
    for chunk in out.chunks_mut(256) {
        let ev = if first {
            first = false;
            events
        } else {
            &[]
        };
        engine.process(chunk, ev);
    }
    out
}

fn rapid_cc1_drag(engine: &mut Engine, steps: usize) -> Vec<[f32; 2]> {
    let mut out = Vec::with_capacity(steps * 64);
    for step in 0..steps {
        let val = ((step * 127) / (steps.saturating_sub(1).max(1))) as u8;
        let mut block = [[0.0f32; 2]; 64];
        engine.process(&mut block, &[cc(1, val)]);
        out.extend_from_slice(&block);
    }
    out
}

fn dft_mag(samples: &[[f32; 2]], ch: usize, hz: f32) -> f32 {
    let n = samples.len();
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for (i, f) in samples.iter().enumerate() {
        let w = f64::from(TAU * hz) * i as f64 / f64::from(RATE);
        re += f64::from(f[ch]) * w.cos();
        im += f64::from(f[ch]) * w.sin();
    }
    (2.0 * (re * re + im * im).sqrt() / n as f64) as f32
}

fn rms(samples: &[[f32; 2]], ch: usize) -> f32 {
    let sum: f32 = samples.iter().map(|f| f[ch] * f[ch]).sum();
    (sum / samples.len().max(1) as f32).sqrt()
}

fn energy(samples: &[[f32; 2]]) -> f32 {
    samples.iter().map(|[l, r]| l * l + r * r).sum()
}

#[test]
fn ring_modulator_live_ksp_sidebands_and_smooth_drags() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let z = sine_zone(&mut store, 2000.0, 0.25, 96_000);
    // Fast mode=1, freq = 4/7 (~571429) => 0.625 * 128^(4/7) * 50 = 500 Hz,
    // ring = 100% => dry = -1.6, wet = 3.2 => ±500 Hz sidebands (~0.20), carrier suppressed.
    let src = r#"
on init
    set_engine_par($ENGINE_PAR_EFFECT_TYPE, $EFFECT_TYPE_RINGMOD, -1, 0, 1)
    set_engine_par($ENGINE_PAR_RINGMOD_FAST_MODE, 1, -1, 0, 1)
    set_engine_par($ENGINE_PAR_RINGMOD_FREQUENCY, 571429, -1, 0, 1)
    set_engine_par($ENGINE_PAR_RINGMOD_RING, 1000000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_RINGMOD_FM, 0, -1, 0, 1)
    set_engine_par($ENGINE_PAR_RINGMOD_EDGE, 0, -1, 0, 1)
    set_engine_par($ENGINE_PAR_RINGMOD_LFO_AMOUNT, 0, -1, 0, 1)
    set_engine_par($ENGINE_PAR_RINGMOD_LFO_WAVE, 1, -1, 0, 1)
    set_engine_par($ENGINE_PAR_RINGMOD_LFO_WAVE, 500000, -1, 0, 1)
    declare $w1
    $w1 := get_engine_par($ENGINE_PAR_RINGMOD_LFO_WAVE, -1, 0, 1)
    set_engine_par($ENGINE_PAR_RINGMOD_LFO_WAVE, 0, -1, 0, 1)
    set_engine_par($ENGINE_PAR_RINGMOD_LFO_WAVE, -1, -1, 0, 1)
    message(get_engine_par($ENGINE_PAR_EFFECT_TYPE, -1, 0, 1) & " " & get_engine_par($ENGINE_PAR_RINGMOD_RING, -1, 0, 1) & " " & get_engine_par_disp($ENGINE_PAR_RINGMOD_FREQUENCY, -1, 0, 1) & " " & $w1 & " " & get_engine_par($ENGINE_PAR_RINGMOD_LFO_WAVE, -1, 0, 1))
end on
on controller
    if (%CC_TOUCHED[1] = 1)
        set_engine_par($ENGINE_PAR_RINGMOD_RING, %CC[1] * 7874, -1, 0, 1)
        set_engine_par($ENGINE_PAR_RINGMOD_FM, %CC[1] * 7874, -1, 0, 1)
        set_engine_par($ENGINE_PAR_RINGMOD_FREQUENCY, %CC[1] * 7874, -1, 0, 1)
        set_engine_par($ENGINE_PAR_RINGMOD_EDGE, %CC[1] * 7874, -1, 0, 1)
    end if
end on
"#;
    let mut eng = build_engine(&store, vec![z], src);
    assert_eq!(eng.last_message(), "50 1000000 500.0 1 0");

    let audio = render(&mut eng, &[note_on(60)], 48_000);
    assert!(audio.iter().flatten().all(|v| v.is_finite()));
    let tail = &audio[16_000..];
    let up = dft_mag(tail, 0, 2500.0);
    let down = dft_mag(tail, 0, 1500.0);
    let carrier = dft_mag(tail, 0, 2000.0);
    assert!(
        up > 0.14 && down > 0.14,
        "expected 500 Hz sidebands (~0.20): up={up} down={down}"
    );
    assert!(
        carrier < 0.015,
        "pure ring mod must suppress carrier: {carrier}"
    );

    let drag = rapid_cc1_drag(&mut eng, 64);
    assert!(
        drag.iter()
            .flatten()
            .all(|v| v.is_finite() && v.abs() < 2.0)
    );
}

#[test]
fn stereo_tune_live_ksp_widens_highs_and_drags_smoothly() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let z = sine_zone(&mut store, 1200.0, 0.3, 96_000);
    let src = r#"
on init
    set_engine_par($ENGINE_PAR_EFFECT_TYPE, $EFFECT_TYPE_STEREO_TUNE, -1, 0, 1)
    set_engine_par($ENGINE_PAR_STEREOTUNE_SPLIT, 100000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_STEREOTUNE_DRIFT, 900000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_STEREOTUNE_SPREAD, 1000000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_STEREOTUNE_MIX, 0, -1, 0, 1)
    message(get_engine_par($ENGINE_PAR_EFFECT_TYPE, -1, 0, 1) & " " & get_engine_par($ENGINE_PAR_STEREOTUNE_DRIFT, -1, 0, 1))
end on
on controller
    if (%CC_TOUCHED[1] = 1)
        set_engine_par($ENGINE_PAR_STEREOTUNE_MIX, %CC[1] * 7874, -1, 0, 1)
        set_engine_par($ENGINE_PAR_STEREOTUNE_SPREAD, %CC[1] * 7874, -1, 0, 1)
    end if
end on
"#;
    let mut eng = build_engine(&store, vec![z], src);
    assert_eq!(eng.last_message(), "61 900000");

    let dry = render(&mut eng, &[note_on(60)], 8192);
    let dry_side: f32 = dry[2048..]
        .iter()
        .map(|[l, r]| (l - r).abs())
        .fold(0.0, f32::max);
    assert!(dry_side < 1e-4, "mix=0 should stay mono, side={dry_side}");

    let wet = render(&mut eng, &[cc(1, 127)], 16_384);
    assert!(wet.iter().flatten().all(|v| v.is_finite()));
    let wet_side = (wet[8192..]
        .iter()
        .map(|[l, r]| (l - r).powi(2))
        .sum::<f32>()
        / 8192.0)
        .sqrt();
    assert!(
        wet_side > 0.05,
        "live Stereo Tune mix=100% must widen stereo: side_rms={wet_side}"
    );

    let drag = rapid_cc1_drag(&mut eng, 64);
    assert!(
        drag.iter()
            .flatten()
            .all(|v| v.is_finite() && v.abs() < 2.0)
    );
}

#[test]
fn cry_wah_live_ksp_sweeps_peak_frequency_and_drags_smoothly() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let z = sine_zone(&mut store, 1600.0, 0.25, 96_000);
    let src = r#"
on init
    set_engine_par($ENGINE_PAR_EFFECT_TYPE, $EFFECT_TYPE_CRYWAH, -1, 0, 1)
    set_engine_par($ENGINE_PAR_CW_PEDAL, 0, -1, 0, 1)
    set_engine_par($ENGINE_PAR_CW_MONO, 0, -1, 0, 1)
    message(get_engine_par($ENGINE_PAR_EFFECT_TYPE, -1, 0, 1) & " " & get_engine_par($ENGINE_PAR_CW_PEDAL, -1, 0, 1))
end on
on controller
    if (%CC_TOUCHED[1] = 1)
        set_engine_par($ENGINE_PAR_CW_PEDAL, %CC[1] * 7874, -1, 0, 1)
    end if
end on
"#;
    let mut eng = build_engine(&store, vec![z], src);
    assert_eq!(eng.last_message(), "37 0");

    let heel = render(&mut eng, &[note_on(60)], 12_288);
    let heel_rms = rms(&heel[4096..], 0);

    let toe = render(&mut eng, &[cc(1, 127)], 12_288);
    let toe_rms = rms(&toe[4096..], 0);
    assert!(
        toe_rms > 2.5 * heel_rms,
        "toe pedal must boost 1600 Hz resonance: heel={heel_rms} toe={toe_rms}"
    );

    let drag = rapid_cc1_drag(&mut eng, 64);
    assert!(drag.iter().flatten().all(|v| v.is_finite()));
    let slew = drag
        .windows(2)
        .map(|w| (w[1][0] - w[0][0]).abs().max((w[1][1] - w[0][1]).abs()))
        .fold(0.0f32, f32::max);
    assert!(slew < 1.0, "wah pedal drag slew too sharp: {slew}");
}

#[test]
fn twin_delay_live_ksp_echo_timing_width_and_smooth_drags() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let z = impulse_zone(&mut store, 0.5, 48_000);
    // Pre L = 0.005 (10 ms), Time L = 0.1 (20.72 ms) => L echo at 30.72 ms (~1474 frames).
    // Pre R = 0.0, Time R = 0.2 (80.48 ms) => R echo at 80.48 ms (~3863 frames).
    let src = r#"
on init
    set_engine_par($ENGINE_PAR_EFFECT_TYPE, $EFFECT_TYPE_TWINDELAY, -1, 0, 1)
    set_engine_par($ENGINE_PAR_SEND_EFFECT_DRY_LEVEL, 0, -1, 0, 1)
    set_engine_par($ENGINE_PAR_TDL_PREDELAY_L, 5000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_TDL_TIME_L, 100000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_TDL_FEEDBACK_L, 400000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_TDL_LEVEL_L, 1000000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_TDL_PREDELAY_R, 0, -1, 0, 1)
    set_engine_par($ENGINE_PAR_TDL_TIME_R, 200000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_TDL_FEEDBACK_R, 400000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_TDL_LEVEL_R, 1000000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_TDL_CROSS_FEEDBACK, 0, -1, 0, 1)
    set_engine_par($ENGINE_PAR_TDL_WIDTH, 1000000, -1, 0, 1)
    message(get_engine_par($ENGINE_PAR_EFFECT_TYPE, -1, 0, 1) & " " & get_engine_par($ENGINE_PAR_TDL_WIDTH, -1, 0, 1))
end on
on controller
    if (%CC_TOUCHED[1] = 1)
        set_engine_par($ENGINE_PAR_TDL_TIME_L, %CC[1] * 7874, -1, 0, 1)
        set_engine_par($ENGINE_PAR_TDL_WIDTH, %CC[1] * 7874, -1, 0, 1)
    end if
end on
"#;
    let mut eng = build_engine(&store, vec![z], src);
    assert_eq!(eng.last_message(), "57 1000000");

    let audio = render(&mut eng, &[note_on(60)], 8192);
    assert!(audio.iter().flatten().all(|v| v.is_finite()));
    let l_win_peak = audio[1350..1600]
        .iter()
        .map(|f| f[0].abs())
        .fold(0.0f32, f32::max);
    let r_win_peak = audio[1350..1600]
        .iter()
        .map(|f| f[1].abs())
        .fold(0.0f32, f32::max);
    assert!(
        l_win_peak > 0.1,
        "expected L echo at ~30.7ms, got {l_win_peak}"
    );
    assert!(
        r_win_peak < 0.01,
        "expected R silent at ~30.7ms, got {r_win_peak}"
    );
    let r_late_peak = audio[3700..4050]
        .iter()
        .map(|f| f[1].abs())
        .fold(0.0f32, f32::max);
    assert!(
        r_late_peak > 0.1,
        "expected R echo at ~80.5ms, got {r_late_peak}"
    );

    let drag = rapid_cc1_drag(&mut eng, 64);
    assert!(
        drag.iter()
            .flatten()
            .all(|v| v.is_finite() && v.abs() < 2.0)
    );
}

#[test]
fn plate_reverb_live_ksp_decay_stereo_and_smooth_drags() {
    let run = |decay: i32, stereo: i32| -> Vec<[f32; 2]> {
        let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
        let z = impulse_zone(&mut store, 0.8, 48_000);
        let src = format!(
            r#"
on init
    set_engine_par($ENGINE_PAR_EFFECT_TYPE, $EFFECT_TYPE_PLATEREVERB, -1, 0, 1)
    set_engine_par($ENGINE_PAR_SEND_EFFECT_DRY_LEVEL, 0, -1, 0, 1)
    set_engine_par($ENGINE_PAR_PR_DECAY, {decay}, -1, 0, 1)
    set_engine_par($ENGINE_PAR_PR_LOWSHELF, 500000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_PR_HIDAMP, 300000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_PR_PREDELAY, 0, -1, 0, 1)
    set_engine_par($ENGINE_PAR_PR_STEREO, {stereo}, -1, 0, 1)
    message(get_engine_par($ENGINE_PAR_EFFECT_TYPE, -1, 0, 1))
end on
"#
        );
        let mut eng = build_engine(&store, vec![z], &src);
        assert_eq!(eng.last_message(), "38");
        let out = render(&mut eng, &[note_on(60)], 48_000);
        assert!(out.iter().flatten().all(|v| v.is_finite()));
        out
    };

    let short = run(0, 1_000_000);
    let long = run(1_000_000, 1_000_000);
    let mono = run(500_000, 0);

    assert!(
        energy(&long[24_000..]) > 5.0 * energy(&short[24_000..]),
        "PR_DECAY=100% must sustain far longer tail than PR_DECAY=0%"
    );
    let mono_diff: f32 = mono[2048..]
        .iter()
        .map(|[l, r]| (l - r).abs())
        .fold(0.0, f32::max);
    assert!(
        mono_diff < 1e-5,
        "PR_STEREO=0 must be mono, got diff={mono_diff}"
    );
    let wide_diff: f32 = long[4096..16_384]
        .iter()
        .map(|[l, r]| (l - r).abs())
        .fold(0.0, f32::max);
    assert!(
        wide_diff > 1e-3,
        "PR_STEREO=100% must be stereo, got diff={wide_diff}"
    );

    // Rapid live drag of predelay/stereo/hidamp preserves tail and stays finite.
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let z = impulse_zone(&mut store, 0.8, 48_000);
    let src = r#"
on init
    set_engine_par($ENGINE_PAR_EFFECT_TYPE, $EFFECT_TYPE_PLATEREVERB, -1, 0, 1)
    set_engine_par($ENGINE_PAR_SEND_EFFECT_DRY_LEVEL, 0, -1, 0, 1)
    set_engine_par($ENGINE_PAR_PR_DECAY, 800000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_PR_STEREO, 1000000, -1, 0, 1)
end on
on controller
    if (%CC_TOUCHED[1] = 1)
        set_engine_par($ENGINE_PAR_PR_PREDELAY, %CC[1] * 7874, -1, 0, 1)
        set_engine_par($ENGINE_PAR_PR_STEREO, %CC[1] * 7874, -1, 0, 1)
        set_engine_par($ENGINE_PAR_PR_HIDAMP, %CC[1] * 7874, -1, 0, 1)
    end if
end on
"#;
    let mut eng = build_engine(&store, vec![z], src);
    let _ = render(&mut eng, &[note_on(60)], 4096);
    let drag = rapid_cc1_drag(&mut eng, 64);
    assert!(
        drag.iter()
            .flatten()
            .all(|v| v.is_finite() && v.abs() < 4.0)
    );
    assert!(
        energy(&drag) > 1e-5,
        "plate tail must not be wiped during rapid drags"
    );
}

fn complex_tone_zone(store: &mut Store, label: &str, frames: usize) -> Zone {
    let samples = (0..frames)
        .map(|n| {
            let t = n as f32 / RATE as f32;
            0.18 * (TAU * 110.0 * t).sin()
                + 0.14 * (TAU * 440.0 * t).sin()
                + 0.12 * (TAU * 1760.0 * t).sin()
                + 0.08 * (TAU * 4800.0 * t).sin()
        })
        .collect();
    zone_from(store, label, samples)
}

fn diff_rms(a: &[[f32; 2]], b: &[[f32; 2]]) -> f32 {
    let n = a.len().min(b.len()).max(1);
    let sum: f32 = a
        .iter()
        .zip(b)
        .map(|(x, y)| {
            let dl = x[0] - y[0];
            let dr = x[1] - y[1];
            0.5 * (dl * dl + dr * dr)
        })
        .sum();
    (sum / n as f32).sqrt()
}

fn assert_finite_bounded(samples: &[[f32; 2]], limit: f32, label: &str) {
    for (i, &[l, r]) in samples.iter().enumerate() {
        assert!(
            l.is_finite() && r.is_finite() && l.abs() <= limit && r.abs() <= limit,
            "{label}: sample {i} out of bounds ({l}, {r})"
        );
    }
}

#[test]
fn twang_live_ksp_bright_tone_and_smooth_drags() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let z_dry = complex_tone_zone(&mut store, "twang_dry", 48_000);
    let mut dry_eng = build_engine(&store, vec![z_dry], "on init\nend on\n");
    let dry = render(&mut dry_eng, &[note_on(60)], 16_384);

    let z = complex_tone_zone(&mut store, "twang_wet", 96_000);
    let src = r#"
on init
    set_engine_par($ENGINE_PAR_EFFECT_TYPE, $EFFECT_TYPE_TWANG, -1, 0, 1)
    set_engine_par($ENGINE_PAR_TW_VOLUME, 700000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_TW_TREBLE, 600000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_TW_MID, 500000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_TW_BASS, 500000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_TW_BRIGHT, 0, -1, 0, 1)
    set_engine_par($ENGINE_PAR_TW_MONO, 0, -1, 0, 1)
    message(get_engine_par($ENGINE_PAR_EFFECT_TYPE, -1, 0, 1) & " " & get_engine_par($ENGINE_PAR_TW_VOLUME, -1, 0, 1))
end on
on controller
    if (%CC_TOUCHED[1] = 1)
        set_engine_par($ENGINE_PAR_TW_VOLUME, %CC[1] * 7874, -1, 0, 1)
        set_engine_par($ENGINE_PAR_TW_TREBLE, %CC[1] * 7874, -1, 0, 1)
        set_engine_par($ENGINE_PAR_TW_BRIGHT, %CC[1] / 64, -1, 0, 1)
    end if
end on
"#;
    let mut eng = build_engine(&store, vec![z], src);
    assert_eq!(eng.last_message(), "23 700000");
    let dark = render(&mut eng, &[note_on(60)], 16_384);
    assert_finite_bounded(&dark, 4.0, "twang dark");
    assert!(diff_rms(&dark[4096..], &dry[4096..]) > 0.05);

    let bright = render(&mut eng, &[cc(1, 127)], 16_384);
    assert_finite_bounded(&bright, 4.0, "twang bright");
    assert!(
        dft_mag(&bright[4096..], 0, 4800.0) > dft_mag(&dark[4096..], 0, 4800.0) * 1.25,
        "Twang bright + treble must lift high harmonics"
    );

    let drag = rapid_cc1_drag(&mut eng, 64);
    assert_finite_bounded(&drag, 4.0, "twang drag");
}

#[test]
fn cabinet_live_ksp_models_size_air_and_bipolar_tone() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let z_dry = complex_tone_zone(&mut store, "cab_dry", 48_000);
    let mut dry_eng = build_engine(&store, vec![z_dry], "on init\nend on\n");
    let dry = render(&mut dry_eng, &[note_on(60)], 16_384);

    let z = complex_tone_zone(&mut store, "cab_wet", 96_000);
    let src = r#"
on init
    set_engine_par($ENGINE_PAR_EFFECT_TYPE, $EFFECT_TYPE_CABINET, -1, 0, 1)
    set_engine_par($ENGINE_PAR_CABINET_TYPE, 0, -1, 0, 1)
    set_engine_par($ENGINE_PAR_CB_SIZE, 500000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_CB_AIR, 0, -1, 0, 1)
    set_engine_par($ENGINE_PAR_CB_BASS, 500000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_CB_TREBLE, 500000, -1, 0, 1)
    message(get_engine_par($ENGINE_PAR_EFFECT_TYPE, -1, 0, 1) & " " & get_engine_par($ENGINE_PAR_CABINET_TYPE, -1, 0, 1))
end on
on controller
    if (%CC_TOUCHED[1] = 1)
        set_engine_par($ENGINE_PAR_CB_AIR, %CC[1] * 7874, -1, 0, 1)
        set_engine_par($ENGINE_PAR_CB_SIZE, %CC[1] * 7874, -1, 0, 1)
        set_engine_par($ENGINE_PAR_CB_BASS, %CC[1] * 7874, -1, 0, 1)
        set_engine_par($ENGINE_PAR_CB_TREBLE, %CC[1] * 7874, -1, 0, 1)
        set_engine_par($ENGINE_PAR_CABINET_TYPE, %CC[1] / 12, -1, 0, 1)
    end if
end on
"#;
    let mut eng = build_engine(&store, vec![z], src);
    assert_eq!(eng.last_message(), "24 0");

    let close = render(&mut eng, &[note_on(60)], 16_384);
    assert_finite_bounded(&close, 4.0, "cabinet close");
    assert!(diff_rms(&close[4096..], &dry[4096..]) > 0.05);
    let close_side: f32 = close[4096..]
        .iter()
        .map(|[l, r]| (l - r).abs())
        .fold(0.0, f32::max);
    assert!(close_side < 1e-4, "air=0 should be mono, side={close_side}");

    let airy = render(&mut eng, &[cc(1, 127)], 16_384);
    assert_finite_bounded(&airy, 4.0, "cabinet airy");
    let airy_side: f32 = airy[4096..]
        .iter()
        .map(|[l, r]| (l - r).abs())
        .fold(0.0, f32::max);
    assert!(
        airy_side > 0.01,
        "air=100% must add stereo room reflections, side={airy_side}"
    );
    assert!(diff_rms(&airy[4096..], &close[4096..]) > 0.03);

    let drag = rapid_cc1_drag(&mut eng, 64);
    assert_finite_bounded(&drag, 4.0, "cabinet drag");
}

#[test]
fn jump_van51_ac_box_hot_solo_cat_live_ksp_and_smooth_drags() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let z_dry = complex_tone_zone(&mut store, "amps_dry", 48_000);
    let mut dry_eng = build_engine(&store, vec![z_dry], "on init\nend on\n");
    let dry = render(&mut dry_eng, &[note_on(60)], 16_384);

    // Jump (31)
    {
        let z = complex_tone_zone(&mut store, "jump_wet", 96_000);
        let src = r#"
on init
    set_engine_par($ENGINE_PAR_EFFECT_TYPE, $EFFECT_TYPE_JUMP, -1, 0, 1)
    set_engine_par($ENGINE_PAR_JMP_PREAMP, 400000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_JMP_MASTER, 600000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_JMP_HIGAIN, 0, -1, 0, 1)
    message(get_engine_par($ENGINE_PAR_EFFECT_TYPE, -1, 0, 1) & " " & get_engine_par($ENGINE_PAR_JMP_PREAMP, -1, 0, 1))
end on
on controller
    if (%CC_TOUCHED[1] = 1)
        set_engine_par($ENGINE_PAR_JMP_PREAMP, %CC[1] * 7874, -1, 0, 1)
        set_engine_par($ENGINE_PAR_JMP_PRESENCE, %CC[1] * 7874, -1, 0, 1)
        set_engine_par($ENGINE_PAR_JMP_HIGAIN, %CC[1] / 64, -1, 0, 1)
    end if
end on
"#;
        let mut eng = build_engine(&store, vec![z], src);
        assert_eq!(eng.last_message(), "31 400000");
        let low = render(&mut eng, &[note_on(60)], 16_384);
        assert_finite_bounded(&low, 4.0, "jump low");
        assert!(diff_rms(&low[4096..], &dry[4096..]) > 0.05);
        let high = render(&mut eng, &[cc(1, 127)], 16_384);
        assert_finite_bounded(&high, 4.0, "jump high");
        assert!(diff_rms(&high[4096..], &low[4096..]) > 0.05);
        assert_finite_bounded(&rapid_cc1_drag(&mut eng, 48), 4.0, "jump drag");
    }

    // Van51 (32)
    {
        let z = complex_tone_zone(&mut store, "van51_wet", 96_000);
        let src = r#"
on init
    set_engine_par($ENGINE_PAR_EFFECT_TYPE, $EFFECT_TYPE_VAN51, -1, 0, 1)
    set_engine_par($ENGINE_PAR_V5_PREGAINRHYTHM, 300000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_V5_PREGAINLEAD, 900000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_V5_LEADCHANNEL, 0, -1, 0, 1)
    set_engine_par($ENGINE_PAR_V5_CRUNCH, 0, -1, 0, 1)
    message(get_engine_par($ENGINE_PAR_EFFECT_TYPE, -1, 0, 1) & " " & get_engine_par($ENGINE_PAR_V5_PREGAINLEAD, -1, 0, 1))
end on
on controller
    if (%CC_TOUCHED[1] = 1)
        set_engine_par($ENGINE_PAR_V5_LEADCHANNEL, %CC[1] / 64, -1, 0, 1)
        set_engine_par($ENGINE_PAR_V5_HIGAIN, %CC[1] / 64, -1, 0, 1)
        set_engine_par($ENGINE_PAR_V5_RESONANCE, %CC[1] * 7874, -1, 0, 1)
    end if
end on
"#;
        let mut eng = build_engine(&store, vec![z], src);
        assert_eq!(eng.last_message(), "32 900000");
        let rhythm = render(&mut eng, &[note_on(60)], 16_384);
        assert_finite_bounded(&rhythm, 4.0, "van51 rhythm");
        assert!(diff_rms(&rhythm[4096..], &dry[4096..]) > 0.05);
        let lead = render(&mut eng, &[cc(1, 127)], 16_384);
        assert_finite_bounded(&lead, 4.0, "van51 lead");
        assert!(diff_rms(&lead[4096..], &rhythm[4096..]) > 0.05);
        assert_finite_bounded(&rapid_cc1_drag(&mut eng, 48), 4.0, "van51 drag");
    }

    // AC Box (33) — verify tremolo + tone cut respond live
    {
        let z = sine_zone(&mut store, 1000.0, 0.25, 96_000);
        let src = r#"
on init
    set_engine_par($ENGINE_PAR_EFFECT_TYPE, $EFFECT_TYPE_ACBOX, -1, 0, 1)
    set_engine_par($ENGINE_PAR_AC_NORMALVOLUME, 600000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_AC_BRILLIANTVOLUME, 600000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_AC_TONECUT, 100000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_AC_TREMOLOSPEED, 700000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_AC_TREMOLODEPTH, 0, -1, 0, 1)
    message(get_engine_par($ENGINE_PAR_EFFECT_TYPE, -1, 0, 1) & " " & get_engine_par($ENGINE_PAR_AC_TONECUT, -1, 0, 1))
end on
on controller
    if (%CC_TOUCHED[1] = 1)
        set_engine_par($ENGINE_PAR_AC_TREMOLODEPTH, %CC[1] * 7874, -1, 0, 1)
        set_engine_par($ENGINE_PAR_AC_TONECUT, %CC[1] * 7874, -1, 0, 1)
    end if
end on
"#;
        let mut eng = build_engine(&store, vec![z], src);
        assert_eq!(eng.last_message(), "33 100000");
        let steady = render(&mut eng, &[note_on(60)], 16_384);
        assert_finite_bounded(&steady, 4.0, "acbox steady");
        let trem = render(&mut eng, &[cc(1, 127)], 24_000);
        assert_finite_bounded(&trem, 4.0, "acbox tremolo");
        // Measure block-to-block RMS swing under deep tremolo.
        let env: Vec<f32> = trem[4096..].chunks(512).map(|c| rms(c, 0)).collect();
        let lo = env.iter().copied().fold(f32::INFINITY, f32::min);
        let hi = env.iter().copied().fold(0.0f32, f32::max);
        assert!(
            hi > lo * 1.4,
            "AC Box tremolo must modulate amplitude: lo={lo} hi={hi}"
        );
        assert_finite_bounded(&rapid_cc1_drag(&mut eng, 48), 4.0, "acbox drag");
    }

    // Hot Solo (34)
    {
        let z = complex_tone_zone(&mut store, "hotsolo_wet", 96_000);
        let src = r#"
on init
    set_engine_par($ENGINE_PAR_EFFECT_TYPE, $EFFECT_TYPE_HOTSOLO, -1, 0, 1)
    set_engine_par($ENGINE_PAR_HS_PRENORMAL, 300000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_HS_PREOVERDRIVE, 900000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_HS_OVERDRIVE, 0, -1, 0, 1)
    message(get_engine_par($ENGINE_PAR_EFFECT_TYPE, -1, 0, 1) & " " & get_engine_par($ENGINE_PAR_HS_PREOVERDRIVE, -1, 0, 1))
end on
on controller
    if (%CC_TOUCHED[1] = 1)
        set_engine_par($ENGINE_PAR_HS_OVERDRIVE, %CC[1] / 64, -1, 0, 1)
        set_engine_par($ENGINE_PAR_HS_DEPTH, %CC[1] * 7874, -1, 0, 1)
    end if
end on
"#;
        let mut eng = build_engine(&store, vec![z], src);
        assert_eq!(eng.last_message(), "34 900000");
        let norm = render(&mut eng, &[note_on(60)], 16_384);
        assert_finite_bounded(&norm, 4.0, "hotsolo norm");
        assert!(diff_rms(&norm[4096..], &dry[4096..]) > 0.05);
        let od = render(&mut eng, &[cc(1, 127)], 16_384);
        assert_finite_bounded(&od, 4.0, "hotsolo od");
        assert!(diff_rms(&od[4096..], &norm[4096..]) > 0.05);
        assert_finite_bounded(&rapid_cc1_drag(&mut eng, 48), 4.0, "hotsolo drag");
    }

    // Cat (35)
    {
        let z = complex_tone_zone(&mut store, "cat_wet", 96_000);
        let src = r#"
on init
    set_engine_par($ENGINE_PAR_EFFECT_TYPE, $EFFECT_TYPE_CAT, -1, 0, 1)
    set_engine_par($ENGINE_PAR_CT_DISTORTION, 800000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_CT_FILTER, 50000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_CT_VOLUME, 600000, -1, 0, 1)
    message(get_engine_par($ENGINE_PAR_EFFECT_TYPE, -1, 0, 1) & " " & get_engine_par($ENGINE_PAR_CT_DISTORTION, -1, 0, 1))
end on
on controller
    if (%CC_TOUCHED[1] = 1)
        set_engine_par($ENGINE_PAR_CT_FILTER, %CC[1] * 7874, -1, 0, 1)
        set_engine_par($ENGINE_PAR_CT_BALLS, %CC[1] * 7874, -1, 0, 1)
    end if
end on
"#;
        let mut eng = build_engine(&store, vec![z], src);
        assert_eq!(eng.last_message(), "35 800000");
        let open = render(&mut eng, &[note_on(60)], 16_384);
        assert_finite_bounded(&open, 4.0, "cat open");
        assert!(diff_rms(&open[4096..], &dry[4096..]) > 0.05);
        let dark = render(&mut eng, &[cc(1, 127)], 16_384);
        assert_finite_bounded(&dark, 4.0, "cat dark");
        assert!(
            dft_mag(&dark[4096..], 0, 4800.0) < dft_mag(&open[4096..], 0, 4800.0) * 0.5,
            "Cat reverse filter at max must darken highs"
        );
        assert_finite_bounded(&rapid_cc1_drag(&mut eng, 48), 4.0, "cat drag");
    }
}

#[test]
fn bass_pro_and_skreamer_deluxe_live_ksp_and_modes() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let z_dry = complex_tone_zone(&mut store, "bp_sk_dry", 48_000);
    let mut dry_eng = build_engine(&store, vec![z_dry], "on init\nend on\n");
    let dry = render(&mut dry_eng, &[note_on(60)], 16_384);

    // Bass Pro (47) with 9-band GEQ toggle
    {
        let z = complex_tone_zone(&mut store, "basspro_wet", 96_000);
        let src = r#"
on init
    set_engine_par($ENGINE_PAR_EFFECT_TYPE, $EFFECT_TYPE_BASSPRO, -1, 0, 1)
    set_engine_par($ENGINE_PAR_BASSPRO_GAIN, 600000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_BASSPRO_DRIVE, 500000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_BASSPRO_GEQ, 0, -1, 0, 1)
    set_engine_par($ENGINE_PAR_BASSPRO_GEQ_1K, 1000000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_BASSPRO_GEQ_4K, 1000000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_BASSPRO_GEQ_VOLUME, 750000, -1, 0, 1)
    message(get_engine_par($ENGINE_PAR_EFFECT_TYPE, -1, 0, 1) & " " & get_engine_par($ENGINE_PAR_BASSPRO_GEQ_1K, -1, 0, 1))
end on
on controller
    if (%CC_TOUCHED[1] = 1)
        set_engine_par($ENGINE_PAR_BASSPRO_GEQ, %CC[1] / 64, -1, 0, 1)
        set_engine_par($ENGINE_PAR_BASSPRO_ULTRALO, %CC[1] / 64, -1, 0, 1)
        set_engine_par($ENGINE_PAR_BASSPRO_MIDFREQ, %CC[1] * 7874, -1, 0, 1)
    end if
end on
"#;
        let mut eng = build_engine(&store, vec![z], src);
        assert_eq!(eng.last_message(), "47 1000000");
        let geq_off = render(&mut eng, &[note_on(60)], 16_384);
        assert_finite_bounded(&geq_off, 4.0, "basspro geq off");
        assert!(diff_rms(&geq_off[4096..], &dry[4096..]) > 0.05);
        let geq_on = render(&mut eng, &[cc(1, 127)], 16_384);
        assert_finite_bounded(&geq_on, 4.0, "basspro geq on");
        assert!(diff_rms(&geq_on[4096..], &geq_off[4096..]) > 0.04);
        assert_finite_bounded(&rapid_cc1_drag(&mut eng, 48), 4.0, "basspro drag");
    }

    // Skreamer Deluxe (69) across all 3 clipping modes
    let mut mode_outs = Vec::new();
    for mode in 0..3 {
        let z = complex_tone_zone(&mut store, &format!("skdlx_{mode}"), 48_000);
        let src = format!(
            r#"
on init
    set_engine_par($ENGINE_PAR_EFFECT_TYPE, $EFFECT_TYPE_SKREAMERDLX, -1, 0, 1)
    set_engine_par($ENGINE_PAR_SKDLX_MODE, {mode}, -1, 0, 1)
    set_engine_par($ENGINE_PAR_SKDLX_DRIVE, 850000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_SKDLX_TONE, 600000, -1, 0, 1)
    message(get_engine_par($ENGINE_PAR_EFFECT_TYPE, -1, 0, 1) & " " & get_engine_par($ENGINE_PAR_SKDLX_MODE, -1, 0, 1))
end on
"#
        );
        let mut eng = build_engine(&store, vec![z], &src);
        assert_eq!(eng.last_message(), format!("69 {mode}"));
        let out = render(&mut eng, &[note_on(60)], 16_384);
        assert_finite_bounded(&out, 4.0, "skdlx");
        assert!(diff_rms(&out[4096..], &dry[4096..]) > 0.05);
        mode_outs.push(out);
    }
    assert!(diff_rms(&mode_outs[0][4096..], &mode_outs[1][4096..]) > 0.01);
    assert!(diff_rms(&mode_outs[1][4096..], &mode_outs[2][4096..]) > 0.01);
}

#[test]
fn kolor_live_ksp_all_ten_modes_bass_saver_boost_and_mix() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let z_dry = complex_tone_zone(&mut store, "kolor_dry", 48_000);
    let mut dry_eng = build_engine(&store, vec![z_dry], "on init\nend on\n");
    let dry = render(&mut dry_eng, &[note_on(60)], 16_384);

    let mut mode_outs = Vec::new();
    for mode in 0..10 {
        let z = complex_tone_zone(&mut store, &format!("kolor_m{mode}"), 48_000);
        let src = format!(
            r#"
on init
    set_engine_par($ENGINE_PAR_EFFECT_TYPE, $EFFECT_TYPE_KOLOR, -1, 0, 1)
    set_engine_par($ENGINE_PAR_KOLOR_MODE, {mode}, -1, 0, 1)
    set_engine_par($ENGINE_PAR_KOLOR_DRIVE, 850000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_KOLOR_BOOST, 1, -1, 0, 1)
    set_engine_par($ENGINE_PAR_KOLOR_BOOST_LEVEL, 500000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_KOLOR_MIX, 1000000, -1, 0, 1)
    message(get_engine_par($ENGINE_PAR_EFFECT_TYPE, -1, 0, 1) & " " & get_engine_par($ENGINE_PAR_KOLOR_MODE, -1, 0, 1))
end on
"#
        );
        let mut eng = build_engine(&store, vec![z], &src);
        assert_eq!(eng.last_message(), format!("70 {mode}"));
        let out = render(&mut eng, &[note_on(60)], 16_384);
        assert_finite_bounded(&out, 4.0, &format!("kolor mode {mode}"));
        assert!(
            diff_rms(&out[4096..], &dry[4096..]) > 0.03,
            "Kolor mode {mode} must audibly saturate input"
        );
        mode_outs.push(out);
    }
    // Check that distinct modes produce distinct waveforms.
    for a in 0..9 {
        assert!(
            diff_rms(&mode_outs[a][4096..], &mode_outs[a + 1][4096..]) > 0.005,
            "Kolor modes {a} and {} should differ",
            a + 1
        );
    }

    // Mix=0 passes dry cleanly, and rapid parameter drags stay finite.
    let z = complex_tone_zone(&mut store, "kolor_drag", 96_000);
    let src = r#"
on init
    set_engine_par($ENGINE_PAR_EFFECT_TYPE, $EFFECT_TYPE_KOLOR, -1, 0, 1)
    set_engine_par($ENGINE_PAR_KOLOR_DRIVE, 900000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_KOLOR_MIX, 0, -1, 0, 1)
end on
on controller
    if (%CC_TOUCHED[1] = 1)
        set_engine_par($ENGINE_PAR_KOLOR_MIX, %CC[1] * 7874, -1, 0, 1)
        set_engine_par($ENGINE_PAR_KOLOR_BASS_SAVER, %CC[1] * 7874, -1, 0, 1)
        set_engine_par($ENGINE_PAR_KOLOR_MID_FREQ, %CC[1] * 7874, -1, 0, 1)
    end if
end on
"#;
    let mut eng = build_engine(&store, vec![z], src);
    let mix0 = render(&mut eng, &[note_on(60)], 16_384);
    assert!(
        diff_rms(&mix0[4096..], &dry[4096..]) < 1e-4,
        "Kolor mix=0 must pass dry cleanly"
    );
    assert_finite_bounded(&rapid_cc1_drag(&mut eng, 64), 4.0, "kolor drag");
}

#[test]
fn dyx_aet_filter_live_ksp_morph_amount_and_core_cell_zero_output() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let z_dry = complex_tone_zone(&mut store, "dyx_dry", 48_000);
    let mut dry_eng = build_engine(&store, vec![z_dry], "on init\nend on\n");
    let dry = render(&mut dry_eng, &[note_on(60)], 16_384);

    // DYX / AET Filter (25)
    let z = complex_tone_zone(&mut store, "dyx_wet", 96_000);
    let src = r#"
on init
    set_engine_par($ENGINE_PAR_EFFECT_TYPE, $EFFECT_TYPE_AET_FILTER, -1, 0, 1)
    set_engine_par($ENGINE_PAR_EXP_FILTER_MORPH, 0, -1, 0, 1)
    set_engine_par($ENGINE_PAR_EXP_FILTER_AMOUNT, 1000000, -1, 0, 1)
    message(get_engine_par($ENGINE_PAR_EFFECT_TYPE, -1, 0, 1) & " " & get_engine_par($ENGINE_PAR_EXP_FILTER_AMOUNT, -1, 0, 1))
end on
on controller
    if (%CC_TOUCHED[1] = 1)
        set_engine_par($ENGINE_PAR_EXP_FILTER_MORPH, %CC[1] * 7874, -1, 0, 1)
    end if
end on
"#;
    let mut eng = build_engine(&store, vec![z], src);
    assert_eq!(eng.last_message(), "25 1000000");

    let dark = render(&mut eng, &[note_on(60)], 16_384);
    assert_finite_bounded(&dark, 4.0, "dyx dark");
    assert!(diff_rms(&dark[4096..], &dry[4096..]) > 0.04);

    let bright = render(&mut eng, &[cc(1, 127)], 16_384);
    assert_finite_bounded(&bright, 4.0, "dyx bright");
    assert!(diff_rms(&bright[4096..], &dark[4096..]) > 0.04);
    assert_finite_bounded(&rapid_cc1_drag(&mut eng, 48), 4.0, "dyx drag");

    // Core Cell (0x5e): saved v0x51 placeholder decodes 10 floats and outputs silence vs bypass.
    let bytes: Vec<u8> = (0..10u32)
        .flat_map(|i| (i as f32 * 0.1).to_le_bytes())
        .collect();
    let cc_saved = kontra_kontakt_effects::core_cell::saved(0x51, &bytes).unwrap();
    let kind = kontra_kontakt_effects::core_cell::model(cc_saved)
        .build(&cc_saved.params)
        .unwrap();
    assert_eq!(kind, kontra_engine::ir::EffectKind::Gain(0.0));

    let z_cc = complex_tone_zone(&mut store, "core_cell_z", 16_384);
    let mut plan = Plan::new(vec![z_cc], 16);
    plan.groups[0].mods.clear();
    plan.inserts = vec![Some(kontra_engine::ir::Effect {
        kind,
        bypass: false,
        output: 1.0,
        dry: 0.0,
    })];
    let mut cc_eng = Engine::new(Arc::new(plan), store.client(), RATE);
    let out = render(&mut cc_eng, &[note_on(60)], 8192);
    assert!(energy(&out) < 1e-10, "Core Cell must zero output");
    assert!(energy(&dry[..8192]) > 1.0, "dry signal must be non-silent");
}

#[test]
fn all_new_dsp_modules_allocate_zero_bytes_on_audio_thread() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let cases: &[(&str, &str, &str)] = &[
        (
            "RingMod",
            "$EFFECT_TYPE_RINGMOD",
            "$ENGINE_PAR_RINGMOD_RING",
        ),
        (
            "StereoTune",
            "$EFFECT_TYPE_STEREO_TUNE",
            "$ENGINE_PAR_STEREOTUNE_MIX",
        ),
        ("CryWah", "$EFFECT_TYPE_CRYWAH", "$ENGINE_PAR_CW_PEDAL"),
        (
            "TwinDelay",
            "$EFFECT_TYPE_TWINDELAY",
            "$ENGINE_PAR_TDL_WIDTH",
        ),
        (
            "PlateReverb",
            "$EFFECT_TYPE_PLATEREVERB",
            "$ENGINE_PAR_PR_DECAY",
        ),
        ("Twang", "$EFFECT_TYPE_TWANG", "$ENGINE_PAR_TW_VOLUME"),
        ("Cabinet", "$EFFECT_TYPE_CABINET", "$ENGINE_PAR_CB_AIR"),
        ("Jump", "$EFFECT_TYPE_JUMP", "$ENGINE_PAR_JMP_PREAMP"),
        ("Van51", "$EFFECT_TYPE_VAN51", "$ENGINE_PAR_V5_POSTGAIN"),
        ("ACBox", "$EFFECT_TYPE_ACBOX", "$ENGINE_PAR_AC_TONECUT"),
        ("HotSolo", "$EFFECT_TYPE_HOTSOLO", "$ENGINE_PAR_HS_MASTER"),
        ("Cat", "$EFFECT_TYPE_CAT", "$ENGINE_PAR_CT_DISTORTION"),
        (
            "BassPro",
            "$EFFECT_TYPE_BASSPRO",
            "$ENGINE_PAR_BASSPRO_GAIN",
        ),
        (
            "SkreamerDlx",
            "$EFFECT_TYPE_SKREAMERDLX",
            "$ENGINE_PAR_SKDLX_DRIVE",
        ),
        ("Kolor", "$EFFECT_TYPE_KOLOR", "$ENGINE_PAR_KOLOR_DRIVE"),
        (
            "DYX",
            "$EFFECT_TYPE_AET_FILTER",
            "$ENGINE_PAR_EXP_FILTER_MORPH",
        ),
    ];

    for &(label, fx_ty, par) in cases {
        let z = complex_tone_zone(&mut store, &format!("rt_{label}"), 32_768);
        let src = format!(
            r#"
on init
    set_engine_par($ENGINE_PAR_EFFECT_TYPE, {fx_ty}, -1, 0, 1)
end on
on controller
    if (%CC_TOUCHED[1] = 1)
        set_engine_par({par}, %CC[1] * 7874, -1, 0, 1)
    end if
end on
"#
        );
        let mut eng = build_engine(&store, vec![z], &src);
        let mut block = [[0.0f32; 2]; 256];
        eng.process(&mut block, &[note_on(60)]);
        for step in 0..16u8 {
            eng.process(&mut block, &[cc(1, step * 8)]);
            assert_finite_bounded(&block, 4.0, label);
            eng.process(&mut block, &[]);
            assert_finite_bounded(&block, 4.0, label);
        }
    }
}

#[test]
fn envelope_follower_modulates_live_voice_volume_and_stays_finite() {
    use kontra_engine::ir;

    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();

    // Baseline unmodulated 440 Hz tone at 0.5 amplitude.
    let z_plain = sine_zone(&mut store, 440.0, 0.5, 48_000);
    let mut plain_plan = Plan::new(vec![z_plain], 16);
    plain_plan.groups[0].mods.clear();
    let mut plain_eng = Engine::new(Arc::new(plain_plan), store.client(), RATE);
    let plain_out = render(&mut plain_eng, &[note_on(60)], 24_000);
    let plain_rms = rms(&plain_out[8192..], 0);

    // Same tone with an EnvelopeFollower ducking volume (-0.8 depth).
    let z_loud = sine_zone(&mut store, 440.0, 0.5, 48_000);
    let mut duck_plan = Plan::new(vec![z_loud], 16);
    duck_plan.groups[0].mods.clear();
    duck_plan.groups[0].followers = vec![ir::EnvelopeFollower {
        attack_ms: 2.0,
        release_ms: 40.0,
        adaption_ms: 0.0,
        gain_db: 6.0,
    }];
    duck_plan.groups[0].mods.push(ir::Mod {
        name: "ENV_FOLLOW_VOL".into(),
        slot: 0,
        target: 0,
        bypass: false,
        route: ir::ModRoute::new(ir::Source::EnvelopeFollower(0), ir::ParamAddr::Volume, -0.8)
            .law(ir::RouteLaw::Gain(ir::Curve::Depth { power: 1 })),
    });
    let mut duck_eng = Engine::new(Arc::new(duck_plan), store.client(), RATE);
    let duck_out = render(&mut duck_eng, &[note_on(60)], 24_000);
    assert_finite_bounded(&duck_out, 2.0, "envelope follower loud");
    let duck_rms = rms(&duck_out[8192..], 0);
    assert!(
        duck_rms < plain_rms * 0.75,
        "envelope follower must audibly duck loud signal: plain_rms={plain_rms} duck_rms={duck_rms}"
    );
    assert!(
        duck_rms > 0.01,
        "envelope follower ducked output must remain audible: duck_rms={duck_rms}"
    );

    // Quiet input (0.04 amplitude) ducks proportionally much less than loud input.
    let z_quiet_plain = sine_zone(&mut store, 440.0, 0.04, 48_000);
    let mut qp_plan = Plan::new(vec![z_quiet_plain], 16);
    qp_plan.groups[0].mods.clear();
    let mut qp_eng = Engine::new(Arc::new(qp_plan), store.client(), RATE);
    let qp_out = render(&mut qp_eng, &[note_on(60)], 24_000);
    let qp_rms = rms(&qp_out[8192..], 0);

    let z_quiet_duck = sine_zone(&mut store, 440.0, 0.04, 48_000);
    let mut qd_plan = Plan::new(vec![z_quiet_duck], 16);
    qd_plan.groups[0].mods.clear();
    qd_plan.groups[0].followers = vec![ir::EnvelopeFollower {
        attack_ms: 2.0,
        release_ms: 40.0,
        adaption_ms: 0.0,
        gain_db: 6.0,
    }];
    qd_plan.groups[0].mods.push(ir::Mod {
        name: "ENV_FOLLOW_VOL".into(),
        slot: 0,
        target: 0,
        bypass: false,
        route: ir::ModRoute::new(ir::Source::EnvelopeFollower(0), ir::ParamAddr::Volume, -0.8)
            .law(ir::RouteLaw::Gain(ir::Curve::Depth { power: 1 })),
    });
    let mut qd_eng = Engine::new(Arc::new(qd_plan), store.client(), RATE);
    let qd_out = render(&mut qd_eng, &[note_on(60)], 24_000);
    assert_finite_bounded(&qd_out, 2.0, "envelope follower quiet");
    let qd_rms = rms(&qd_out[8192..], 0);
    assert!(
        qd_rms / qp_rms > (duck_rms / plain_rms) + 0.15,
        "envelope follower must track input amplitude: quiet_ratio={} loud_ratio={}",
        qd_rms / qp_rms,
        duck_rms / plain_rms
    );
}
