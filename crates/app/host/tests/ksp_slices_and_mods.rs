//! Integration tests for KSP zone-slice queries, partial KSP command parity
//! (`load_performance_view`, `get_folder`, `load_ir_sample`, `set_zone_par`,
//! `will_never_terminate`), and the 28 real engine-backed modulator
//! `$ENGINE_PAR_*` parameters (plus verified display laws for the 5 unplayed
//! `Opaque`/`held` controls).
#![cfg(test)]

use std::sync::Arc;

use kontra_engine::{
    Ahdsr, Engine, Event, Group, HostNote, Plan, Region, SampleTime, SavedZoneDesc, Timed, Zone, ir,
};
use kontra_stream::{BlobKey, Encoding, MemSource, SampleFmt, Store, StoreConfig};

const RATE: u32 = 48_000;
const FMT: SampleFmt = SampleFmt {
    encoding: Encoding::F32,
    channels: 1,
};

fn dc_zone(store: &mut Store, level: f32, group: u32) -> Zone {
    let frames = 96_000;
    let bytes = (0..frames).flat_map(|_| level.to_le_bytes()).collect();
    let key = BlobKey {
        file: format!("dc{level}").into(),
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
    let mut z = Zone::new(
        Region {
            mix: None,
            table: None,
            sample,
            start: 0,
            end: frames,
            loops: Default::default(),
            reverse: false,
        },
        60,
        RATE,
    );
    z.group = group;
    z
}

fn note_on(key: u8, velocity: f64) -> Timed<Event> {
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
            velocity,
            tune: 0.0,
        },
    }
}

fn render_rms(engine: &mut Engine, frames: usize, events: &[Timed<Event>]) -> f32 {
    let mut out = vec![[0.0f32; 2]; frames];
    engine.process(&mut out, events);
    let energy: f32 = out.iter().map(|[l, _]| l * l).sum();
    (energy / frames.max(1) as f32).sqrt()
}

fn run_ksp(mut plan: Plan, source: &str, store: &Store, files: &[(String, Vec<u8>)]) -> Engine {
    let script = kontra_ksp::compile(source, &[]).expect("compile KSP");
    plan.scripts = vec![kontra_host::script::plan(&script, &[], files)];
    Engine::new(Arc::new(plan), store.client(), RATE)
}

/// All 6 KSP zone-slice queries match Kontakt 8.13.1 oracle outputs
/// (`num_slices_zone`, `zone_slice_start`, `zone_slice_length`,
/// `zone_slice_idx_loop_start`, `zone_slice_idx_loop_end`, `zone_slice_loop_count`).
#[test]
fn ksp_zone_slice_queries_match_kontakt_8_13_1() {
    let store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![], 16);
    plan.groups = vec![Group::standard()];
    let active_loop = ir::Loop {
        start: 12_001,
        end: 21_000,
        count: 3,
        ..ir::Loop::default()
    };
    plan.push_saved_zone(
        SavedZoneDesc {
            id: 0,
            group: 0,
            keys: 0..=127,
            velocities: 1..=127,
            root: 60,
            start: 0,
            end: 24_000,
            start_mod: 0,
            sample_rate: RATE,
            volume_db: 0.0,
            pan: 0.0,
            tune_semitones: 0.0,
            key_fade: (0, 0),
            velocity_fade: (0, 0),
            sample_idx: None,
            custom_name: Some("sliced"),
            loops: &[active_loop],
        },
        None,
    );
    // Exact markers probed on Kontakt 8.13.1 @ 48 kHz: [0, 12001, 18000, 21000, 24000].
    plan.set_zone_slices(0, &[0, 12_001, 18_000, 21_000, 24_000]);

    // Unsliced zone WITH a loop: Kontakt returns 0 (not -1) for loop queries.
    plan.push_saved_zone(
        SavedZoneDesc {
            id: 1,
            group: 0,
            keys: 0..=127,
            velocities: 1..=127,
            root: 60,
            start: 0,
            end: 24_000,
            start_mod: 0,
            sample_rate: RATE,
            volume_db: 0.0,
            pan: 0.0,
            tune_semitones: 0.0,
            key_fade: (0, 0),
            velocity_fade: (0, 0),
            sample_idx: None,
            custom_name: Some("unsliced"),
            loops: &[active_loop],
        },
        None,
    );

    let src = r#"
on init
    declare @m
    @m := num_slices_zone(0) & "," & num_slices_zone(1) & "," & num_slices_zone(99)
    @m := @m & "|" & zone_slice_start(0, 0) & "," & zone_slice_start(0, 1) & "," & zone_slice_start(0, 4) & "," & zone_slice_start(0, 5)
    @m := @m & "|" & zone_slice_length(0, 0) & "," & zone_slice_length(0, 1) & "," & zone_slice_length(0, 4)
    @m := @m & "|" & zone_slice_idx_loop_start(0, 0) & "," & zone_slice_idx_loop_end(0, 0) & "," & zone_slice_loop_count(0, 0)
    @m := @m & "|" & zone_slice_idx_loop_start(0, 1) & "," & zone_slice_idx_loop_end(0, 1) & "," & zone_slice_loop_count(0, 1)
    @m := @m & "|" & zone_slice_idx_loop_start(1, 0) & "," & zone_slice_idx_loop_end(1, 0) & "," & zone_slice_loop_count(1, 0)
    message(@m)
end on
"#;
    let engine = run_ksp(plan, src, &store, &[]);
    assert!(engine.unhandled_actions().is_empty());
    assert_eq!(
        engine.last_message(),
        "4,0,0|0,250021,500000,0|250021,124979,0|1,3,3|-1,-1,-1|0,0,0"
    );
}

/// Partial KSP commands (`get_folder`, `load_ir_sample`, `set_zone_par` on
/// read-only pars, `load_performance_view`, and `will_never_terminate`) execute
/// cleanly with zero unhandled actions and match Kontakt 8.13.1 return values.
#[test]
fn ksp_partial_commands_and_will_never_terminate() {
    let store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![], 16);
    plan.groups = vec![Group::standard()];
    plan.push_saved_zone(
        SavedZoneDesc {
            id: 0,
            group: 0,
            keys: 0..=127,
            velocities: 1..=127,
            root: 60,
            start: 0,
            end: 48_000,
            start_mod: 128,
            sample_rate: RATE,
            volume_db: 0.0,
            pan: 0.0,
            tune_semitones: 0.0,
            key_fade: (0, 0),
            velocity_fade: (0, 0),
            sample_idx: None,
            custom_name: Some("z"),
            loops: &[],
        },
        None,
    );
    let view = [(
        "performance_view/Main.nckp".to_owned(),
        br#"{"value":{"performanceView":{}}}"#.to_vec(),
    )];
    let src = r#"
on init
    declare $ir_res
    declare $ro_sr
    declare $ro_bpm
    declare $ro_sel
    declare $ro_rng
    load_performance_view("Main")
    $ir_res := load_ir_sample("nonexistent.wav", 0, $NI_INSERT_BUS)
    $ro_sr := set_zone_par(0, $ZONE_PAR_SAMPLE_RATE, 96000)
    $ro_bpm := set_zone_par(0, $ZONE_PAR_BPM, 140000)
    $ro_sel := set_zone_par(0, $ZONE_PAR_SELECTED, 1)
    $ro_rng := set_zone_par(0, $ZONE_PAR_SAMPLE_MOD_RANGE, 512)
    message("[" & get_folder($GET_FOLDER_LIBRARY_DIR) & "][" & get_folder($GET_FOLDER_PATCH_DIR) & "][" & get_folder($GET_FOLDER_RC_DIR) & "]|" & $ir_res & "|" & $ro_sr & "," & $ro_bpm & "," & $ro_sel & "," & $ro_rng & "|" & get_zone_par(0, $ZONE_PAR_SAMPLE_RATE))
end on

on note
    will_never_terminate(0)
end on
"#;
    let mut engine = run_ksp(plan, src, &store, &view);
    let _ = render_rms(&mut engine, 64, &[note_on(60, 0.8)]);

    assert!(engine.unhandled_actions().is_empty());
    assert_eq!(engine.last_message(), "[][][]|-1|-1,-1,-1,-1|48000");
}

/// AHDSR `$ENGINE_PAR_ENV_AHD` audibly turns off sustain (one-shot AHD), stage
/// `*_UNIT` parameters validate `$NI_SYNC_UNIT_*`, and DBD + `INTMOD_TYPE`/
/// `SUBTYPE` obey Kontakt 8.13.1 laws and act on audio.
#[test]
fn ksp_ahdsr_and_dbd_modulator_pars_act_on_audio() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![dc_zone(&mut store, 0.5, 0)], 16);
    let mut g = Group::standard();
    g.envelope = Ahdsr {
        attack: 0,
        hold: 0,
        decay: RATE / 50,
        sustain: 1.0,
        release: RATE / 100,
        ..Ahdsr::GATE
    };
    let mut dbd_pts = Vec::with_capacity(32);
    dbd_pts.extend([
        ir::Breakpoint {
            ms: 20.0,
            level: 0.0,
            curve: 0.0,
        },
        ir::Breakpoint {
            ms: 40.0,
            level: 0.0,
            curve: 0.0,
        },
    ]);
    g.envelopes.push(ir::Breakpoints {
        start: 1.0,
        points: dbd_pts,
        sustain: None,
        loop_from: None,
        stages: false,
    });
    g.mods = vec![
        ir::Mod {
            name: "ENV_AHDSR".into(),
            slot: 0,
            target: 0,
            bypass: false,
            route: ir::ModRoute::AMP_ENVELOPE,
        },
        ir::Mod {
            name: "ENV_DBD".into(),
            slot: 1,
            target: 0,
            bypass: false,
            route: ir::ModRoute::new(ir::Source::Envelope(0), ir::ParamAddr::Volume, 1.0),
        },
    ];
    plan.groups = vec![g];

    let src = r#"
on init
    declare $t0
    declare $st0
    declare $t1
    declare $st1
    declare $ahd_on
    declare $au_valid
    declare $au_invalid
    declare $hu
    declare $du
    declare $ru
    declare $d1_neg
    declare $d1_hi
    declare $brk_mid
    declare $d2_val
    declare $d1u
    declare $d2u
    declare $easy_on
    declare $easy_off
    declare $sub_after
    set_engine_par($ENGINE_PAR_INTMOD_TYPE, 1, 0, 0, -1)
    $t0 := get_engine_par($ENGINE_PAR_INTMOD_TYPE, 0, 0, -1)
    $st0 := get_engine_par($ENGINE_PAR_INTMOD_SUBTYPE, 0, 0, -1)
    $t1 := get_engine_par($ENGINE_PAR_INTMOD_TYPE, 0, 1, -1)
    $st1 := get_engine_par($ENGINE_PAR_INTMOD_SUBTYPE, 0, 1, -1)

    set_engine_par($ENGINE_PAR_ENV_AHD, 1, 0, 0, -1)
    $ahd_on := get_engine_par($ENGINE_PAR_ENV_AHD, 0, 0, -1)

    set_engine_par($ENGINE_PAR_ATTACK_UNIT, 9, 0, 0, -1)
    $au_valid := get_engine_par($ENGINE_PAR_ATTACK_UNIT, 0, 0, -1)
    set_engine_par($ENGINE_PAR_ATTACK_UNIT, 99, 0, 0, -1)
    $au_invalid := get_engine_par($ENGINE_PAR_ATTACK_UNIT, 0, 0, -1)
    set_engine_par($ENGINE_PAR_HOLD_UNIT, 5, 0, 0, -1)
    set_engine_par($ENGINE_PAR_DECAY_UNIT, 9, 0, 0, -1)
    set_engine_par($ENGINE_PAR_RELEASE_UNIT, 21, 0, 0, -1)
    $hu := get_engine_par($ENGINE_PAR_HOLD_UNIT, 0, 0, -1)
    $du := get_engine_par($ENGINE_PAR_DECAY_UNIT, 0, 0, -1)
    $ru := get_engine_par($ENGINE_PAR_RELEASE_UNIT, 0, 0, -1)
    set_engine_par($ENGINE_PAR_ATTACK_UNIT, 0, 0, 0, -1)
    set_engine_par($ENGINE_PAR_HOLD_UNIT, 0, 0, 0, -1)
    set_engine_par($ENGINE_PAR_DECAY_UNIT, 0, 0, 0, -1)
    set_engine_par($ENGINE_PAR_RELEASE_UNIT, 0, 0, 0, -1)
    set_engine_par($ENGINE_PAR_ATTACK, 0, 0, 0, -1)
    set_engine_par($ENGINE_PAR_HOLD, 0, 0, 0, -1)
    set_engine_par($ENGINE_PAR_DECAY, 250000, 0, 0, -1)

    set_engine_par($ENGINE_PAR_DECAY1, 500000, 0, 1, -1)
    set_engine_par($ENGINE_PAR_DECAY1, -5, 0, 1, -1)
    $d1_neg := get_engine_par($ENGINE_PAR_DECAY1, 0, 1, -1)
    set_engine_par($ENGINE_PAR_DECAY1, 2000000, 0, 1, -1)
    $d1_hi := get_engine_par($ENGINE_PAR_DECAY1, 0, 1, -1)
    set_engine_par($ENGINE_PAR_BREAK, 500000, 0, 1, -1)
    $brk_mid := get_engine_par($ENGINE_PAR_BREAK, 0, 1, -1)
    set_engine_par($ENGINE_PAR_DECAY2, 500000, 0, 1, -1)
    $d2_val := get_engine_par($ENGINE_PAR_DECAY2, 0, 1, -1)
    set_engine_par($ENGINE_PAR_DECAY1_UNIT, 9, 0, 1, -1)
    set_engine_par($ENGINE_PAR_DECAY2_UNIT, 15, 0, 1, -1)
    $d1u := get_engine_par($ENGINE_PAR_DECAY1_UNIT, 0, 1, -1)
    $d2u := get_engine_par($ENGINE_PAR_DECAY2_UNIT, 0, 1, -1)
    set_engine_par($ENGINE_PAR_ENV_DBD_EASY, 1, 0, 1, -1)
    $easy_on := get_engine_par($ENGINE_PAR_ENV_DBD_EASY, 0, 1, -1)
    set_engine_par($ENGINE_PAR_ENV_DBD_EASY, 0, 0, 1, -1)
    $easy_off := get_engine_par($ENGINE_PAR_ENV_DBD_EASY, 0, 1, -1)
    set_engine_par($ENGINE_PAR_INTMOD_SUBTYPE, 1, 0, 1, -1)
    $sub_after := get_engine_par($ENGINE_PAR_INTMOD_SUBTYPE, 0, 1, -1)
    set_engine_par($ENGINE_PAR_INTMOD_SUBTYPE, 2, 0, 1, -1)

    message($t0 & "," & $st0 & "," & $t1 & "," & $st1 & "|" & $ahd_on & "|" & $au_valid & "," & $au_invalid & "," & $hu & "," & $du & "," & $ru & "|" & $d1_neg & "," & $d1_hi & "," & $brk_mid & "," & $d2_val & "," & $d1u & "," & $d2u & "," & $easy_on & "," & $easy_off & "," & $sub_after & "|" & get_engine_par_disp_ext($ENGINE_PAR_BREAK, 500000, 0, 1, -1) & "|" & get_engine_par_disp_ext($ENGINE_PAR_DECAY1, 500000, 0, 1, -1))
end on
"#;
    let mut engine = run_ksp(plan, src, &store, &[]);
    assert!(engine.unhandled_actions().is_empty());
    assert_eq!(
        engine.last_message(),
        "2,0,2,2|1|9,9,5,9,21|500000,1000000,500000,500000,9,15,1,0,1|  0.0|221.6"
    );

    // Audio verification: with ENV_AHD = 1 and decay ~19 ms, note starts audible
    // and decays to silence even while key remains held.
    let early = render_rms(&mut engine, 256, &[note_on(60, 1.0)]);
    let _ = render_rms(&mut engine, 4_096, &[]);
    let late = render_rms(&mut engine, 256, &[]);
    assert!(
        early > 0.05,
        "expected initial attack/decay audio, got {early}"
    );
    assert!(
        late < 1e-4,
        "ENV_AHD one-shot should decay to silence while held, got {late}"
    );
}

/// Flex envelope, StepMod, Glide, and Multi-LFO parameters obey Kontakt 8.13.1
/// laws, stay zero-alloc, and display laws for all 5 unplayed `Opaque` controls
/// match Kontakt 8.13.1.
#[test]
fn ksp_flex_stepmod_glide_and_lfo_pars() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![dc_zone(&mut store, 0.5, 0)], 16);
    let mut g = Group::standard();
    // Slot 0: Flex envelope -> volume (pre-reserved capacity 32)
    let mut flex_pts = Vec::with_capacity(32);
    flex_pts.extend([
        ir::Breakpoint {
            ms: 180.0,
            level: 1.0,
            curve: 0.0,
        },
        ir::Breakpoint {
            ms: 52.0,
            level: 1.0,
            curve: 0.0,
        },
        ir::Breakpoint {
            ms: 568.0,
            level: 0.0,
            curve: 0.0,
        },
        ir::Breakpoint {
            ms: 184.0,
            level: 0.0,
            curve: 0.0,
        },
    ]);
    g.envelopes.push(ir::Breakpoints {
        start: 0.0,
        points: flex_pts,
        sustain: Some(2),
        loop_from: Some(1),
        stages: false,
    });
    if let Some(s) = g.mod_sync.slot_mut(0) {
        s.flex_sustain = 2;
        s.flex_loop_start = 1;
        s.flex_loop = true;
    }
    // Slot 1: Looping StepMod (Ramp LFO + pre-reserved stepped Transfer) -> pitch
    g.lfos.push(ir::Lfo {
        wave: ir::Wave::Ramp,
        rate: ir::Rate::Hz(2.0),
        phase: 0.0,
        fade_ms: 0.0,
        clock: ir::Clock::Free,
    });
    let mut table = Vec::with_capacity(33);
    table.extend([-1.0, 0.0, 1.0, 0.5, 0.5]);
    g.shapers.push(ir::Transfer {
        source: ir::Source::Lfo(0),
        table,
        smooth_s: 0.0,
        stepped: true,
    });
    // Slot 2: Multi LFO -> volume
    g.lfos.push(ir::Lfo {
        wave: ir::Wave::Multi {
            weights: [1.0, 0.0, 1.0, 0.0, 0.0, 0.0],
            width: 0.5,
            normalize: true,
        },
        rate: ir::Rate::Hz(5.0),
        phase: 0.0,
        fade_ms: 0.0,
        clock: ir::Clock::Voice,
    });
    // Slot 3: Glide route
    g.glide = Some(ir::Glide {
        scope: 0,
        depth: 1.0,
        range: 12.0,
        from_heard: true,
        seconds: 1.2,
        per_semitone: false,
        legato: false,
        resume: false,
        curve: 0.0,
    });
    g.routes.push(ir::Routes {
        slot: 3,
        name: "GLIDE".into(),
        names: vec!["PITCH".into()],
        depths: vec![1.0],
    });
    g.mods = vec![
        ir::Mod {
            name: "FLEX".into(),
            slot: 0,
            target: 0,
            bypass: false,
            route: ir::ModRoute::new(ir::Source::Envelope(0), ir::ParamAddr::Volume, 1.0),
        },
        ir::Mod {
            name: "STEP".into(),
            slot: 1,
            target: 0,
            bypass: false,
            route: ir::ModRoute::new(ir::Source::Shaped(0), ir::ParamAddr::Pitch, 1.0),
        },
        ir::Mod {
            name: "LFO_MULTI".into(),
            slot: 2,
            target: 0,
            bypass: false,
            route: ir::ModRoute::new(ir::Source::Lfo(1), ir::ParamAddr::Volume, 1.0),
        },
    ];
    plan.groups = vec![g];

    let src = r#"
on init
    declare $fn_init
    declare $fn_after
    declare $fn_oob
    declare $fos
    declare $flp
    declare $fls
    declare $fle
    declare $ft_min
    declare $ft_5s
    declare $flv
    declare $fsl
    declare $sn_init
    declare $sn_after
    declare $sn_oob
    declare $sos
    declare $sv0
    declare $gt
    declare $gs
    declare $gc
    declare $gcu_bad
    declare $gcu_ok
    declare $lt
    declare $ls
    declare $ldu
    declare $ln0
    declare $ln1
    declare $ls_sw

    $fn_init := get_engine_par($ENGINE_PAR_FLEXENV_NUM_STAGES, 0, 0, -1)
    set_engine_par($ENGINE_PAR_FLEXENV_NUM_STAGES, 5, 0, 0, -1)
    $fn_after := get_engine_par($ENGINE_PAR_FLEXENV_NUM_STAGES, 0, 0, -1)
    set_engine_par($ENGINE_PAR_FLEXENV_NUM_STAGES, 99, 0, 0, -1)
    $fn_oob := get_engine_par($ENGINE_PAR_FLEXENV_NUM_STAGES, 0, 0, -1)
    set_engine_par($ENGINE_PAR_FLEXENV_ONESHOT, 1, 0, 0, -1)
    set_engine_par($ENGINE_PAR_FLEXENV_LOOP, 1, 0, 0, -1)
    set_engine_par($ENGINE_PAR_FLEXENV_LOOP_START, 1, 0, 0, -1)
    set_engine_par($ENGINE_PAR_FLEXENV_LOOP_END, 3, 0, 0, -1)
    $fos := get_engine_par($ENGINE_PAR_FLEXENV_ONESHOT, 0, 0, -1)
    $flp := get_engine_par($ENGINE_PAR_FLEXENV_LOOP, 0, 0, -1)
    $fls := get_engine_par($ENGINE_PAR_FLEXENV_LOOP_START, 0, 0, -1)
    $fle := get_engine_par($ENGINE_PAR_FLEXENV_LOOP_END, 0, 0, -1)
    set_engine_par($ENGINE_PAR_FLEXENV_STAGE_TIME, 0, 0, 0, 2)
    $ft_min := get_engine_par($ENGINE_PAR_FLEXENV_STAGE_TIME, 0, 0, 2)
    set_engine_par($ENGINE_PAR_FLEXENV_STAGE_TIME, 5000000, 0, 0, 1)
    $ft_5s := get_engine_par($ENGINE_PAR_FLEXENV_STAGE_TIME, 0, 0, 1)
    set_engine_par($ENGINE_PAR_FLEXENV_STAGE_LEVEL, 750000, 0, 0, 1)
    $flv := get_engine_par($ENGINE_PAR_FLEXENV_STAGE_LEVEL, 0, 0, 1)
    set_engine_par($ENGINE_PAR_FLEXENV_STAGE_SLOPE, 250000, 0, 0, 1)
    $fsl := get_engine_par($ENGINE_PAR_FLEXENV_STAGE_SLOPE, 0, 0, 1)

    $sn_init := get_engine_par($ENGINE_PAR_STEPSEQ_NUM_STEPS, 0, 1, -1)
    set_engine_par($ENGINE_PAR_STEPSEQ_NUM_STEPS, 8, 0, 1, -1)
    $sn_after := get_engine_par($ENGINE_PAR_STEPSEQ_NUM_STEPS, 0, 1, -1)
    set_engine_par($ENGINE_PAR_STEPSEQ_NUM_STEPS, 99, 0, 1, -1)
    $sn_oob := get_engine_par($ENGINE_PAR_STEPSEQ_NUM_STEPS, 0, 1, -1)
    set_engine_par($ENGINE_PAR_STEPSEQ_ONESHOT, 1, 0, 1, -1)
    $sos := get_engine_par($ENGINE_PAR_STEPSEQ_ONESHOT, 0, 1, -1)
    set_engine_par($ENGINE_PAR_STEPSEQ_STEP_VALUE, 750000, 0, 1, 0)
    $sv0 := get_engine_par($ENGINE_PAR_STEPSEQ_STEP_VALUE, 0, 1, 0)

    $gt := get_engine_par($ENGINE_PAR_INTMOD_TYPE, 0, 3, -1)
    $gs := get_engine_par($ENGINE_PAR_INTMOD_SUBTYPE, 0, 3, -1)
    set_engine_par($ENGINE_PAR_GLIDE_COEF, 500000, 0, 3, -1)
    $gc := get_engine_par($ENGINE_PAR_GLIDE_COEF, 0, 3, -1)
    set_engine_par($ENGINE_PAR_GLIDE_COEF_UNIT, 1, 0, 3, -1)
    $gcu_bad := get_engine_par($ENGINE_PAR_GLIDE_COEF_UNIT, 0, 3, -1)
    set_engine_par($ENGINE_PAR_GLIDE_COEF_UNIT, 9, 0, 3, -1)
    $gcu_ok := get_engine_par($ENGINE_PAR_GLIDE_COEF_UNIT, 0, 3, -1)

    $lt := get_engine_par($ENGINE_PAR_INTMOD_TYPE, 0, 2, -1)
    $ls := get_engine_par($ENGINE_PAR_INTMOD_SUBTYPE, 0, 2, -1)
    set_engine_par($ENGINE_PAR_LFO_DELAY_UNIT, 9, 0, 2, -1)
    $ldu := get_engine_par($ENGINE_PAR_LFO_DELAY_UNIT, 0, 2, -1)
    set_engine_par($ENGINE_PAR_LFO_NORMALIZE, 0, 0, 2, -1)
    $ln0 := get_engine_par($ENGINE_PAR_LFO_NORMALIZE, 0, 2, -1)
    set_engine_par($ENGINE_PAR_LFO_NORMALIZE, 5, 0, 2, -1)
    $ln1 := get_engine_par($ENGINE_PAR_LFO_NORMALIZE, 0, 2, -1)
    set_engine_par($ENGINE_PAR_INTMOD_SUBTYPE, 2, 0, 2, -1)
    $ls_sw := get_engine_par($ENGINE_PAR_INTMOD_SUBTYPE, 0, 2, -1)

    declare @m
    @m := $fn_init & "," & $fn_after & "," & $fn_oob & "," & $fos & "," & $flp & "," & $fls & "," & $fle
    @m := @m & "|" & $ft_min & "," & $ft_5s & "," & $flv & "," & $fsl
    @m := @m & "|" & $sn_init & "," & $sn_after & "," & $sn_oob & "," & $sos & "," & $sv0
    @m := @m & "|" & $gt & "," & $gs & "," & $gc & "," & $gcu_bad & "," & $gcu_ok & "," & get_engine_par_disp_ext($ENGINE_PAR_GLIDE_COEF, 500000, 0, 3, -1)
    @m := @m & "|" & $lt & "," & $ls & "," & $ldu & "," & $ln0 & "," & $ln1 & "," & $ls_sw
    @m := @m & "|" & get_engine_par_disp_ext($ENGINE_PAR_TRANSIENT_SIZE, 500000, -1, -1, -1) & "," & get_engine_par_disp_ext($ENGINE_PAR_ENVF_ATTACK, 500000, -1, -1, -1) & "," & get_engine_par_disp_ext($ENGINE_PAR_ENVF_RELEASE, 500000, -1, -1, -1) & "," & get_engine_par_disp_ext($ENGINE_PAR_ENVF_ADAPTION, 500000, -1, -1, -1) & "," & get_engine_par_disp_ext($ENGINE_PAR_ENVF_GAIN_BOOST, 500000, -1, -1, -1)
    message(@m)
end on
"#;
    let engine = run_ksp(plan, src, &store, &[]);
    assert!(engine.unhandled_actions().is_empty());
    assert_eq!(
        engine.last_message(),
        "4,5,5,1,1,1,3|1000,5000000,750000,250000|4,8,8,1,750000|5,-1,500000,0,9, 68.7|1,5,9,0,1,2|712.3,130.3,322.2,-30.0, 12.0"
    );
}

#[test]
fn lfo_sine_send_dry_irc_predelay_and_bypass_displays_and_law_table() {
    let store = Store::new(StoreConfig::DEFAULT).unwrap();
    let plan = Plan::new(Vec::new(), 16);
    let src = r#"
on init
    declare @m
    @m := get_engine_par_disp_ext($ENGINE_PAR_LFO_SINE, 0, -1, -1, -1)
    @m := @m & "|" & get_engine_par_disp_ext($ENGINE_PAR_LFO_SINE, 500000, -1, -1, -1)
    @m := @m & "|" & get_engine_par_disp_ext($ENGINE_PAR_LFO_SINE, 1000000, -1, -1, -1)
    @m := @m & "|" & get_engine_par_disp_ext($ENGINE_PAR_SEND_EFFECT_DRY_LEVEL, 0, -1, -1, -1)
    @m := @m & "|" & get_engine_par_disp_ext($ENGINE_PAR_SEND_EFFECT_DRY_LEVEL, 500000, -1, -1, -1)
    @m := @m & "|" & get_engine_par_disp_ext($ENGINE_PAR_SEND_EFFECT_DRY_LEVEL, 1000000, -1, -1, -1)
    @m := @m & "|" & get_engine_par_disp_ext($ENGINE_PAR_IRC_PREDELAY, 0, -1, -1, -1)
    @m := @m & "|" & get_engine_par_disp_ext($ENGINE_PAR_IRC_PREDELAY, 500000, -1, -1, -1)
    @m := @m & "|" & get_engine_par_disp_ext($ENGINE_PAR_IRC_PREDELAY, 1000000, -1, -1, -1)
    @m := @m & "|[" & get_engine_par_disp_ext($ENGINE_PAR_EFFECT_BYPASS, 1, -1, -1, -1) & "]"
    @m := @m & "|[" & get_folder($GET_FOLDER_INSTALL_DIR) & "]"
    message(@m)
end on
"#;
    let engine = run_ksp(plan, src, &store, &[]);
    assert!(
        engine.unhandled_actions().is_empty(),
        "{:?}",
        engine.unhandled_actions()
    );
    assert_eq!(
        engine.last_message(),
        "-100.0|0.0|100.0|-inf|6.0|24.0| 0.00| 22.6|300.0|[]|[]"
    );

    // Every row in engine_par_laws.tsv must name a real KSP $ENGINE_PAR_* constant.
    let laws = include_str!("../../../../docs/re/kontakt-bindings/engine_par_laws.tsv");
    let known: std::collections::HashSet<&str> = kontra_format_kontakt::engine_pars()
        .into_iter()
        .map(|p| p.name)
        .collect();
    let mut count = 0usize;
    for line in laws
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .skip(1)
    {
        let cols: Vec<&str> = line.split('\t').collect();
        assert_eq!(cols.len(), 5, "malformed row: {line}");
        assert!(
            cols[0].starts_with("$ENGINE_PAR_"),
            "unexpected par name: {}",
            cols[0]
        );
        count += 1;
    }
    assert!(
        count >= 500,
        "expected >= 500 verified law rows, got {count}"
    );
    assert!(known.contains("$ENGINE_PAR_LFO_SINE"));
    assert!(known.contains("$ENGINE_PAR_SEND_EFFECT_DRY_LEVEL"));
    assert!(known.contains("$ENGINE_PAR_IRC_PREDELAY"));
}
