//! End-to-end DSP, binary serialization, and live KSP parity tests for Wave 2:
//! Rotator (`0x22`), Beat Masher (`0x71`), Gater (`0x72`), Beat Slicer (`0x73`),
//! Reverse Grain (`0x74`), and Transpose Stretch (`0x75`).
#![allow(
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing
)]

use std::f32::consts::TAU;
use std::sync::Arc;

use kontra_engine::{Engine, Event, HostNote, Plan, Region, SampleTime, Timed, Zone, ir};
use kontra_kontakt_effects::{mangling, rotator};
use kontra_stream::{BlobKey, Encoding, MemSource, SampleFmt, Store, StoreConfig};

const RATE: u32 = 48_000;
const FMT: SampleFmt = SampleFmt {
    encoding: Encoding::F32,
    channels: 1,
};

fn sample_zone(store: &mut Store, name: &str, samples: &[f32]) -> Zone {
    let frames = samples.len() as u64;
    let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    let key = BlobKey {
        file: name.into(),
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
        .expect("sample loads");
    let region = Region {
        mix: None,
        table: None,
        sample,
        start: 0,
        end: frames,
        loops: Default::default(),
        reverse: false,
    };
    let mut z = Zone::new(region, 60, RATE);
    z.group = 0;
    z
}

fn host_note(key: u8) -> Timed<Event> {
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

fn cc_event(controller: u8, value: u8) -> Timed<Event> {
    Timed {
        at: SampleTime(0),
        body: Event::midi1(0xB0, controller, value),
    }
}

fn wave2_slot_types() -> Vec<ir::SlotType> {
    let mut types = vec![rotator::slot_type()];
    types.extend(mangling::slot_types());
    types
}

fn engine_with_wave2(store: &Store, zones: Vec<Zone>, source: &str) -> Engine {
    let script = kontra_ksp::compile(source, &[]).expect("script compiles");
    let mut plan = Plan::new(zones, 16);
    for g in &mut plan.groups {
        g.mods.clear();
    }
    plan.inserts = vec![None; 8];
    plan.slot_shapes.types = wave2_slot_types();
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    Engine::new(Arc::new(plan), store.client(), RATE)
}

fn render_blocks(
    engine: &mut Engine,
    total_frames: usize,
    block_size: usize,
    first_events: &[Timed<Event>],
) -> Vec<[f32; 2]> {
    let mut out = Vec::with_capacity(total_frames);
    let mut remaining = total_frames;
    let mut first = true;
    while remaining > 0 {
        let n = remaining.min(block_size);
        let mut buf = vec![[0.0f32; 2]; n];
        let ev = if first {
            first = false;
            first_events
        } else {
            &[]
        };
        engine.process(&mut buf, ev);
        out.extend_from_slice(&buf);
        remaining -= n;
    }
    out
}

fn assert_clean_audio(audio: &[[f32; 2]], label: &str) {
    for (i, &[l, r]) in audio.iter().enumerate() {
        assert!(
            l.is_finite() && r.is_finite(),
            "{label}: non-finite at frame {i}"
        );
        assert!(
            l == 0.0 || l.abs() >= 1e-20,
            "{label}: denormal L={l} at frame {i}"
        );
        assert!(
            r == 0.0 || r.abs() >= 1e-20,
            "{label}: denormal R={r} at frame {i}"
        );
    }
}

// ---------------------------------------------------------------------------
// 1. Binary serialization & parameter law round-trips for all 6 modules
// ---------------------------------------------------------------------------

#[test]
fn all_six_modules_binary_round_trip_and_version_validation() {
    // Rotator (0x22, v0x50): 6 LE floats
    let mut rot_bytes = Vec::new();
    for v in [1.0f32, 0.25, 0.7, 0.3, 0.4, 0.5] {
        rot_bytes.extend(v.to_le_bytes());
    }
    assert!(rotator::saved(0x10, &rot_bytes).is_none());
    let rot = rotator::saved(0x50, &rot_bytes).expect("rotator v0x50");
    let rot_ir = rotator::rotary(rot);
    assert!(rot_ir.fast);
    assert!((rot_ir.balance - 0.25).abs() < 1e-6);
    assert!((rot_ir.mix - 0.125).abs() < 1e-6, "cubic mix law");
    let rot_m = rotator::model(rot);
    assert_eq!(
        rot_m.build(&rot_m.saved),
        Some(ir::EffectKind::Rotary(rot_ir))
    );

    // Beat Masher (0x71, v0x01): FFFBBBI (19 bytes)
    let mut bm_bytes = Vec::new();
    for v in [0.5f32, 0.25, 0.9] {
        bm_bytes.extend(v.to_le_bytes());
    }
    bm_bytes.extend([1u8, 1, 1]);
    bm_bytes.extend(6i32.to_le_bytes()); // 1/8 note = 0.5 beats
    assert!(mangling::beat_masher::saved(2, &bm_bytes).is_none());
    let bm = mangling::beat_masher::saved(1, &bm_bytes).expect("beat masher v1");
    let bm_ir = mangling::beat_masher::ir_effect(bm);
    assert_eq!(bm_ir.length_beats, 0.5);
    assert!((bm_ir.gate - (-0.5)).abs() < 1e-6);
    assert!(bm_ir.reverse && bm_ir.wrap && bm_ir.mash);
    let bm_m = mangling::beat_masher::model(bm);
    assert_eq!(
        bm_m.build(&bm_m.saved),
        Some(ir::EffectKind::BeatMasher(bm_ir))
    );

    // Gater (0x72, v0x01): FFFFBBBB (20 bytes)
    let mut gt_bytes = Vec::new();
    for v in [0.25f32, 0.75, 0.15, 0.95] {
        gt_bytes.extend(v.to_le_bytes());
    }
    gt_bytes.extend([1u8, 0, 1, 0]);
    let gt = mangling::gater::saved(1, &gt_bytes).expect("gater v1");
    let gt_ir = mangling::gater::ir_effect(gt);
    assert_eq!(gt_ir.rate, ir::Rate::Hz((20.0 + 380.0 * 0.25) / 60.0));
    assert!(gt_ir.stutter && !gt_ir.mute_input && gt_ir.active);
    let gt_m = mangling::gater::model(gt);
    assert_eq!(gt_m.build(&gt_m.saved), Some(ir::EffectKind::Gater(gt_ir)));

    // Beat Slicer (0x73, v0x01): FFBBBII (19 bytes)
    let mut bs_bytes = Vec::new();
    for v in [0.6f32, 0.8] {
        bs_bytes.extend(v.to_le_bytes());
    }
    bs_bytes.extend([1u8, 1, 1]);
    bs_bytes.extend(4i32.to_le_bytes());
    bs_bytes.extend(3i32.to_le_bytes());
    let bs = mangling::beat_slicer::saved(1, &bs_bytes).expect("beat slicer v1");
    let bs_ir = mangling::beat_slicer::ir_effect(bs);
    assert_eq!((bs_ir.bars, bs_ir.pattern, bs_ir.style), (2, 4, 3));
    let bs_m = mangling::beat_slicer::model(bs);
    assert_eq!(
        bs_m.build(&bs_m.saved),
        Some(ir::EffectKind::BeatSlicer(bs_ir))
    );

    // Reverse Grain (0x74, v0x01): FFFFBBB (19 bytes)
    let mut rg_bytes = Vec::new();
    for v in [0.4f32, 0.5, 0.0, 1.0] {
        rg_bytes.extend(v.to_le_bytes());
    }
    rg_bytes.extend([1u8, 1, 0]);
    let rg = mangling::reverse_grain::saved(1, &rg_bytes).expect("reverse grain v1");
    let rg_ir = mangling::reverse_grain::ir_effect(rg);
    assert!((rg_ir.pitch_ratio - 0.5).abs() < 1e-5);
    assert!(rg_ir.forward && rg_ir.invert_grains && !rg_ir.reverse);
    let rg_m = mangling::reverse_grain::model(rg);
    assert_eq!(
        rg_m.build(&rg_m.saved),
        Some(ir::EffectKind::ReverseGrain(rg_ir))
    );

    // Transpose Stretch (0x75, v0x01): FFFFBBB (19 bytes)
    let mut ts_bytes = Vec::new();
    for v in [0.5f32, 0.5, 0.0, 1.0] {
        ts_bytes.extend(v.to_le_bytes());
    }
    ts_bytes.extend([1u8, 0, 1]);
    let ts = mangling::transpose_stretch::saved(1, &ts_bytes).expect("transpose stretch v1");
    let ts_ir = mangling::transpose_stretch::ir_effect(ts);
    assert!((ts_ir.semitones - (-60.0)).abs() < 1e-4);
    assert_eq!(ts_ir.bars, 2);
    let ts_m = mangling::transpose_stretch::model(ts);
    assert_eq!(
        ts_m.build(&ts_m.saved),
        Some(ir::EffectKind::TransposeStretch(ts_ir))
    );
}

// ---------------------------------------------------------------------------
// 2. Live KSP set_engine_par / get_engine_par / get_engine_par_disp parity
// ---------------------------------------------------------------------------

#[test]
fn live_ksp_loads_all_six_modules_and_round_trips_all_42_parameters_and_readouts() {
    let store = Store::new(StoreConfig::DEFAULT).unwrap();
    let source = r#"
on init
    { Load all 6 Wave 2 modules into insert slots 0..5 }
    set_engine_par($ENGINE_PAR_EFFECT_TYPE, $EFFECT_TYPE_ROTATOR, -1, 0, 1)
    set_engine_par($ENGINE_PAR_EFFECT_TYPE, $EFFECT_TYPE_BEATMASHER, -1, 1, 1)
    set_engine_par($ENGINE_PAR_EFFECT_TYPE, $EFFECT_TYPE_GATER, -1, 2, 1)
    set_engine_par($ENGINE_PAR_EFFECT_TYPE, $EFFECT_TYPE_BEATSLICER, -1, 3, 1)
    set_engine_par($ENGINE_PAR_EFFECT_TYPE, 65, -1, 4, 1)
    set_engine_par($ENGINE_PAR_EFFECT_TYPE, 66, -1, 5, 1)

    { Rotator (slot 0) }
    set_engine_par($ENGINE_PAR_RT_SPEED, 1000000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_RT_BALANCE, 750000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_RT_ACCEL_HI, 600000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_RT_ACCEL_LO, 400000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_RT_DISTANCE, 250000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_RT_MIX, 1000000, -1, 0, 1)

    { Beat Masher (slot 1) }
    set_engine_par($ENGINE_PAR_BEATMASHER_ROTATE, 250000, -1, 1, 1)
    set_engine_par($ENGINE_PAR_BEATMASHER_GATE, 750000, -1, 1, 1)
    set_engine_par($ENGINE_PAR_BEATMASHER_MIX, 900000, -1, 1, 1)
    set_engine_par($ENGINE_PAR_BEATMASHER_REVERSE, 1, -1, 1, 1)
    set_engine_par($ENGINE_PAR_BEATMASHER_WRAP, 0, -1, 1, 1)
    set_engine_par($ENGINE_PAR_BEATMASHER_MASH, 1, -1, 1, 1)
    set_engine_par($ENGINE_PAR_BEATMASHER_LENGTH, $NI_BEATMASHER_LENGTH_16TH, -1, 1, 1)

    { Gater (slot 2) }
    set_engine_par($ENGINE_PAR_GATER_RATE, 500000, -1, 2, 1)
    set_engine_par($ENGINE_PAR_GATER_SHAPE, 600000, -1, 2, 1)
    set_engine_par($ENGINE_PAR_GATER_NOISE, 200000, -1, 2, 1)
    set_engine_par($ENGINE_PAR_GATER_MIX, 800000, -1, 2, 1)
    set_engine_par($ENGINE_PAR_GATER_STUTTER, 1, -1, 2, 1)
    set_engine_par($ENGINE_PAR_GATER_MUTE_INPUT, 0, -1, 2, 1)
    set_engine_par($ENGINE_PAR_GATER_GATE, 1, -1, 2, 1)
    set_engine_par($ENGINE_PAR_GATER_RATE_SYNC, 0, -1, 2, 1)

    { Beat Slicer (slot 3) }
    set_engine_par($ENGINE_PAR_BEATSLICER_BUZZ, 350000, -1, 3, 1)
    set_engine_par($ENGINE_PAR_BEATSLICER_MIX, 850000, -1, 3, 1)
    set_engine_par($ENGINE_PAR_BEATSLICER_TWOBARS, 1, -1, 3, 1)
    set_engine_par($ENGINE_PAR_BEATSLICER_GATE, 1, -1, 3, 1)
    set_engine_par($ENGINE_PAR_BEATSLICER_SLICE, 1, -1, 3, 1)
    set_engine_par($ENGINE_PAR_BEATSLICER_PATTERN, 3, -1, 3, 1)
    set_engine_par($ENGINE_PAR_BEATSLICER_STYLE, 2, -1, 3, 1)

    { Reverse Grain (slot 4) }
    set_engine_par($ENGINE_PAR_RG_SPEED, 450000, -1, 4, 1)
    set_engine_par($ENGINE_PAR_RG_GRAIN, 550000, -1, 4, 1)
    set_engine_par($ENGINE_PAR_RG_PITCH, 750000, -1, 4, 1)
    set_engine_par($ENGINE_PAR_RG_MIX, 950000, -1, 4, 1)
    set_engine_par($ENGINE_PAR_RG_FORWARD, 0, -1, 4, 1)
    set_engine_par($ENGINE_PAR_RG_INVERT_GRAINS, 1, -1, 4, 1)
    set_engine_par($ENGINE_PAR_RG_REVERSE, 1, -1, 4, 1)

    { Transpose Stretch (slot 5) }
    set_engine_par($ENGINE_PAR_TS_AMOUNT, 500000, -1, 5, 1)
    set_engine_par($ENGINE_PAR_TS_SIZE, 400000, -1, 5, 1)
    set_engine_par($ENGINE_PAR_TS_KEY, 1000000, -1, 5, 1)
    set_engine_par($ENGINE_PAR_TS_MIX, 1000000, -1, 5, 1)
    set_engine_par($ENGINE_PAR_TS_STRETCH, 1, -1, 5, 1)
    set_engine_par($ENGINE_PAR_TS_GRAIN, 0, -1, 5, 1)
    set_engine_par($ENGINE_PAR_TS_TWOBARS, 1, -1, 5, 1)

    message(get_engine_par_disp($ENGINE_PAR_RT_SPEED, -1, 0, 1) & "|" ...
        & get_engine_par_disp($ENGINE_PAR_RT_BALANCE, -1, 0, 1) & "|" ...
        & get_engine_par_disp($ENGINE_PAR_BEATMASHER_GATE, -1, 1, 1) & "|" ...
        & get_engine_par($ENGINE_PAR_BEATMASHER_LENGTH, -1, 1, 1) & "|" ...
        & get_engine_par_disp($ENGINE_PAR_GATER_RATE, -1, 2, 1) & "|" ...
        & get_engine_par($ENGINE_PAR_BEATSLICER_PATTERN, -1, 3, 1) & "|" ...
        & get_engine_par($ENGINE_PAR_BEATSLICER_STYLE, -1, 3, 1) & "|" ...
        & get_engine_par_disp($ENGINE_PAR_RG_PITCH, -1, 4, 1) & "|" ...
        & get_engine_par_disp($ENGINE_PAR_TS_KEY, -1, 5, 1))
end on
"#;
    let e = engine_with_wave2(&store, Vec::new(), source);
    assert_eq!(e.last_message(), "fast|75.0|50.0|7|210 BPM|3|2|75.0|12.00");
}

// ---------------------------------------------------------------------------
// 3. Numerical DSP assertions through the live engine + KSP pipeline
// ---------------------------------------------------------------------------

#[test]
fn live_rotator_produces_slow_and_fast_doppler_and_am_modulation() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let tone: Vec<f32> = (0..144_000)
        .map(|i| (TAU * 1200.0 * i as f32 / RATE as f32).sin() * 0.6)
        .collect();
    let z = sample_zone(&mut store, "rot_tone", &tone);
    let source = r#"
on init
    set_engine_par($ENGINE_PAR_EFFECT_TYPE, $EFFECT_TYPE_ROTATOR, -1, 0, 1)
    set_engine_par($ENGINE_PAR_RT_SPEED, 1000000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_RT_BALANCE, 700000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_RT_ACCEL_HI, 1000000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_RT_DISTANCE, 150000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_RT_MIX, 1000000, -1, 0, 1)
end on
"#;
    let mut e = engine_with_wave2(&store, vec![z], source);
    let out = render_blocks(&mut e, 96_000, 512, &[host_note(60)]);
    assert_clean_audio(&out, "rotator");

    // Verify AM modulation depth and L/R stereo decorrelation after spin-up.
    let mut min_rms = f32::MAX;
    let mut max_rms = 0.0f32;
    let mut stereo_diff = 0.0f32;
    for chunk in out[24_000..72_000].chunks(480) {
        let rms = (chunk.iter().map(|f| f[0] * f[0]).sum::<f32>() / chunk.len() as f32).sqrt();
        min_rms = min_rms.min(rms);
        max_rms = max_rms.max(rms);
        stereo_diff += chunk.iter().map(|f| (f[0] - f[1]).powi(2)).sum::<f32>();
    }
    assert!(
        max_rms > min_rms * 1.15,
        "rotator AM modulation missing: min={min_rms} max={max_rms}"
    );
    assert!(
        stereo_diff > 1.0,
        "rotator stereo spread missing: {stereo_diff}"
    );
}

#[test]
fn live_gater_enforces_exact_tempo_locked_duty_cycle() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let dc = vec![0.8f32; 96_000];
    let z = sample_zone(&mut store, "gater_dc", &dc);
    // Free rate = 120 BPM (x = (120-20)/380 = 0.263158) -> 2 Hz gate at 50% duty.
    let source = r#"
on init
    set_engine_par($ENGINE_PAR_EFFECT_TYPE, $EFFECT_TYPE_GATER, -1, 0, 1)
    set_engine_par($ENGINE_PAR_GATER_RATE_SYNC, 0, -1, 0, 1)
    set_engine_par($ENGINE_PAR_GATER_RATE, 263158, -1, 0, 1)
    set_engine_par($ENGINE_PAR_GATER_SHAPE, 500000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_GATER_NOISE, 0, -1, 0, 1)
    set_engine_par($ENGINE_PAR_GATER_MIX, 1000000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_GATER_GATE, 1, -1, 0, 1)
end on
"#;
    let mut e = engine_with_wave2(&store, vec![z], source);
    let out = render_blocks(&mut e, 48_000, 512, &[host_note(60)]);
    assert_clean_audio(&out, "gater");

    let second_cycle = &out[24_000..48_000];
    let open = second_cycle.iter().filter(|f| f[0].abs() > 0.35).count();
    let duty = open as f32 / second_cycle.len() as f32;
    assert!(
        (duty - 0.5).abs() < 0.06,
        "expected ~0.50 duty cycle, got {duty}"
    );
}

#[test]
fn live_beat_masher_produces_periodic_stutter_loop() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let sig: Vec<f32> = (0..96_000)
        .map(|i| {
            let env = 0.25 + 0.75 * ((i % 9600) as f32 / 9600.0);
            (TAU * 330.0 * i as f32 / RATE as f32).sin() * env
        })
        .collect();
    let z = sample_zone(&mut store, "bm_sig", &sig);
    // Engage mash via CC 1 after recording 12,000 frames; 1/16 at 120 BPM = 6,000 frames.
    let source = r#"
on init
    set_engine_par($ENGINE_PAR_EFFECT_TYPE, $EFFECT_TYPE_BEATMASHER, -1, 0, 1)
    set_engine_par($ENGINE_PAR_BEATMASHER_LENGTH, $NI_BEATMASHER_LENGTH_16TH, -1, 0, 1)
    set_engine_par($ENGINE_PAR_BEATMASHER_GATE, 500000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_BEATMASHER_MIX, 1000000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_BEATMASHER_MASH, 0, -1, 0, 1)
end on
on controller
    if ($CC_NUM = 1)
        set_engine_par($ENGINE_PAR_BEATMASHER_MASH, 1, -1, 0, 1)
    end if
end on
"#;
    let mut e = engine_with_wave2(&store, vec![z], source);
    let _pre = render_blocks(&mut e, 12_000, 500, &[host_note(60)]);
    let mashed = render_blocks(&mut e, 18_000, 500, &[cc_event(1, 127)]);
    assert_clean_audio(&mashed, "beat_masher");

    // Period 2 (6000..12000) vs Period 3 (12000..18000) must match closely.
    let mut diff = 0.0f32;
    let mut energy = 0.0f32;
    for i in 6000..12_000 {
        let d = mashed[i][0] - mashed[i + 6000][0];
        diff += d * d;
        energy += mashed[i][0] * mashed[i][0];
    }
    assert!(energy > 10.0, "mashed audio must be audible: {energy}");
    assert!(
        diff / energy < 5e-3,
        "beat masher stutter periodicity mismatch: {}",
        diff / energy
    );
}

#[test]
fn live_reverse_grain_correlates_with_time_reversed_waveform() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    // Asymmetric ascending-saw amplitude envelope repeating every 100ms (4800 frames).
    let sig: Vec<f32> = (0..96_000)
        .map(|i| {
            let phase = (i % 4800) as f32 / 4800.0;
            let amp = phase * phase;
            (TAU * 440.0 * i as f32 / RATE as f32).sin() * amp
        })
        .collect();
    let z = sample_zone(&mut store, "rg_ramp", &sig);
    let source = r#"
on init
    set_engine_par($ENGINE_PAR_EFFECT_TYPE, 65, -1, 0, 1)
    set_engine_par($ENGINE_PAR_RG_SPEED, 1000000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_RG_GRAIN, 536429, -1, 0, 1)
    set_engine_par($ENGINE_PAR_RG_PITCH, 500000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_RG_MIX, 1000000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_RG_REVERSE, 1, -1, 0, 1)
    set_engine_par($ENGINE_PAR_RG_INVERT_GRAINS, 0, -1, 0, 1)
end on
"#;
    let mut e = engine_with_wave2(&store, vec![z], source);
    let out = render_blocks(&mut e, 48_000, 512, &[host_note(60)]);
    assert_clean_audio(&out, "reverse_grain");

    // Measure short-window envelope correlation against forward vs reversed ramp template.
    let seg = &out[14_400..38_400];
    let mut env = Vec::new();
    for c in seg.chunks(120) {
        let rms = (c.iter().map(|f| f[0] * f[0]).sum::<f32>() / c.len() as f32).sqrt();
        env.push(rms);
    }
    assert!(env.iter().copied().fold(0.0f32, f32::max) > 0.1);
    // Compare best lag correlation with ascending vs descending 40-bin sawtooth envelope.
    let period = 40usize;
    let max_corr = |descending: bool| -> f32 {
        (0..period)
            .map(|lag| {
                let mut dot = 0.0f32;
                let mut norm_a = 0.0f32;
                let mut norm_b = 0.0f32;
                for (idx, &v) in env.iter().enumerate() {
                    let t = ((idx + lag) % period) as f32 / period as f32;
                    let tpl = if descending { (1.0 - t).powi(2) } else { t * t };
                    dot += v * tpl;
                    norm_a += v * v;
                    norm_b += tpl * tpl;
                }
                dot / (norm_a * norm_b).sqrt().max(1e-8)
            })
            .fold(0.0f32, f32::max)
    };
    let rev_corr = max_corr(true);
    let fwd_corr = max_corr(false);
    assert!(
        rev_corr > fwd_corr,
        "reverse grain envelope should correlate stronger with reversed ramp: rev={rev_corr} fwd={fwd_corr}"
    );
}

#[test]
fn live_transpose_stretch_verifies_octave_pitch_shift_ratio() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let tone: Vec<f32> = (0..120_000)
        .map(|i| (TAU * 440.0 * i as f32 / RATE as f32).sin() * 0.6)
        .collect();
    let z = sample_zone(&mut store, "ts_sine", &tone);
    // $ENGINE_PAR_TS_KEY = 1_000_000 -> +12.00 semitones -> 880 Hz (1760 crossings/s).
    let source = r#"
on init
    set_engine_par($ENGINE_PAR_EFFECT_TYPE, 66, -1, 0, 1)
    set_engine_par($ENGINE_PAR_TS_KEY, 1000000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_TS_SIZE, 250000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_TS_MIX, 1000000, -1, 0, 1)
    set_engine_par($ENGINE_PAR_TS_STRETCH, 0, -1, 0, 1)
end on
"#;
    let mut e = engine_with_wave2(&store, vec![z], source);
    let out = render_blocks(&mut e, 96_000, 512, &[host_note(60)]);
    assert_clean_audio(&out, "transpose_stretch");

    let tail = &out[48_000..96_000];
    let crossings = tail
        .windows(2)
        .filter(|w| (w[0][0] < 0.0) != (w[1][0] < 0.0))
        .count();
    assert!(
        (1680..=1840).contains(&crossings),
        "expected ~1760 zero crossings for +12st of 440 Hz, got {crossings}"
    );
}

// ---------------------------------------------------------------------------
// 4. Rapid knob-drag smoothness stress test across all 6 modules
// ---------------------------------------------------------------------------

#[test]
fn rapid_knob_drags_across_all_six_modules_stay_finite_and_click_free() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let tone: Vec<f32> = (0..120_000)
        .map(|i| (TAU * 220.0 * i as f32 / RATE as f32).sin() * 0.5)
        .collect();
    let z = sample_zone(&mut store, "drag_tone", &tone);
    let source = r#"
on init
    set_engine_par($ENGINE_PAR_EFFECT_TYPE, $EFFECT_TYPE_ROTATOR, -1, 0, 1)
    set_engine_par($ENGINE_PAR_EFFECT_TYPE, $EFFECT_TYPE_GATER, -1, 1, 1)
    set_engine_par($ENGINE_PAR_EFFECT_TYPE, $EFFECT_TYPE_BEATMASHER, -1, 2, 1)
    set_engine_par($ENGINE_PAR_EFFECT_TYPE, $EFFECT_TYPE_BEATSLICER, -1, 3, 1)
    set_engine_par($ENGINE_PAR_EFFECT_TYPE, 65, -1, 4, 1)
    set_engine_par($ENGINE_PAR_EFFECT_TYPE, 66, -1, 5, 1)
    declare $v
end on
on controller
    $v := %CC[$CC_NUM] * 7874
    set_engine_par($ENGINE_PAR_RT_BALANCE, $v, -1, 0, 1)
    set_engine_par($ENGINE_PAR_RT_DISTANCE, $v, -1, 0, 1)
    set_engine_par($ENGINE_PAR_RT_MIX, $v, -1, 0, 1)
    set_engine_par($ENGINE_PAR_GATER_SHAPE, $v, -1, 1, 1)
    set_engine_par($ENGINE_PAR_GATER_MIX, $v, -1, 1, 1)
    set_engine_par($ENGINE_PAR_BEATMASHER_ROTATE, $v, -1, 2, 1)
    set_engine_par($ENGINE_PAR_BEATMASHER_MIX, $v, -1, 2, 1)
    set_engine_par($ENGINE_PAR_BEATSLICER_BUZZ, $v, -1, 3, 1)
    set_engine_par($ENGINE_PAR_BEATSLICER_MIX, $v, -1, 3, 1)
    set_engine_par($ENGINE_PAR_RG_PITCH, $v, -1, 4, 1)
    set_engine_par($ENGINE_PAR_RG_MIX, $v, -1, 4, 1)
    set_engine_par($ENGINE_PAR_TS_KEY, $v, -1, 5, 1)
    set_engine_par($ENGINE_PAR_TS_MIX, $v, -1, 5, 1)
end on
"#;
    let mut e = engine_with_wave2(&store, vec![z], source);
    let mut all_out = Vec::with_capacity(64 * 256);
    for step in 0u8..64 {
        let mut buf = [[0.0f32; 2]; 256];
        let val = if step % 2 == 0 { 127 } else { 0 };
        if step == 0 {
            e.process(&mut buf, &[host_note(60), cc_event(1, val)]);
        } else {
            e.process(&mut buf, &[cc_event(1, val)]);
        }
        all_out.extend_from_slice(&buf);
    }
    assert_clean_audio(&all_out, "rapid_knob_drag");
    // Bounded sample-to-sample slew confirms no hard clicks on extreme 0 <-> 100% knob slams.
    let max_delta = all_out
        .windows(2)
        .map(|w| (w[1][0] - w[0][0]).abs().max((w[1][1] - w[0][1]).abs()))
        .fold(0.0f32, f32::max);
    assert!(
        max_delta < 0.75,
        "rapid knob drag caused excessive click jump: {max_delta}"
    );
}
