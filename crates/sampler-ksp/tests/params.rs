//! Script voice parameters reach the audio: change_vol/pan, fades,
//! set_engine_par group volume and purge_group.
use sampler_core::{
    Envelope, GroupParams, Input, Limits, Pcm, Playback, Prepared, Protocol, Region, Runtime,
};

fn runtime(source: &str) -> Runtime {
    authored(source, GroupParams::default())
}

fn compile(source: &str) -> sampler_ksp::Script {
    sampler_ksp::compile(
        source,
        48000,
        sampler_ksp::Limits {
            source_bytes: 65536,
            instructions: 4096,
            variables: 16,
            array_cells: 16,
        },
        &[],
    )
    .unwrap()
}

/// Group 0 authored at `group0`, baked into its region's gain as lowering does.
fn authored(source: &str, group0: GroupParams) -> Runtime {
    modules(vec![compile(source)], group0)
}

fn modules(scripts: Vec<sampler_ksp::Script>, group0: GroupParams) -> Runtime {
    let note_cells = scripts.iter().map(|s| s.note_cells()).sum::<usize>() * 8;
    let pcm = [0.5, 0.25].map(|v| Pcm::new(48000, Box::from([[v; 2]; 48000])).unwrap());
    let regions = (0..2)
        .map(|sample| Region {
            sample,
            key_low: 60 + sample as u8,
            key_high: 60 + sample as u8,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: if sample == 0 {
                10f32.powf(group0.decibels as f32 / 20.)
            } else {
                1.
            },
            envelope: Envelope::default(),
            playback: Playback::default(),
        })
        .collect();
    let plan = sampler_ksp::bind_modules(
        scripts,
        Prepared::new(48000, pcm.into(), regions, 2)
            .unwrap()
            .with_groups(2, vec![Some(0), Some(1)])
            .and_then(|p| p.with_group_params(vec![group0, GroupParams::default()]))
            .unwrap(),
    )
    .unwrap();
    let behavior_cells = plan.behavior_local_count() * 8;
    Runtime::new(
        plan,
        Limits {
            notes: 8,
            channels: 1,
            performances: 1,
            families: 8,
            voices: 8,
            expressions: 8,
            decisions: 0,
            commands: 16,
            behaviors: 8,
            behavior_fuel: 4096,
            behavior_cells,
            note_cells,
        },
    )
    .unwrap()
}

fn input(key: u8) -> Input {
    Input {
        protocol: Protocol::Clap,
        port: 0,
        group: 0,
        channel: 0,
        key,
        external_id: Some(i32::from(key)),
    }
}

/// Last frame of a 256-frame render.
fn level(rt: &mut Runtime) -> [f32; 2] {
    let mut audio = [[0.; 2]; 256];
    rt.render(&mut audio).unwrap();
    audio[255]
}

fn close(a: [f32; 2], b: [f32; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3
}

#[test]
fn note_volume_pan_and_fades_shape_the_voice() {
    let mut rt = runtime(
        "on note
           change_vol($EVENT_ID, -6021, 0)
           change_pan($EVENT_ID, 1000, 0)
           wait(10000)
           change_vol($EVENT_ID, 6021, 1)
           set_event_par($EVENT_ID, $EVENT_PAR_PAN, 0)
           wait(10000)
           fade_out($EVENT_ID, 2000, 1)
         end on",
    );
    rt.trigger(input(60), 60, 1.).unwrap();
    // -6 dB and hard right: balance law silences the left.
    assert!(close(level(&mut rt), [0.0, 0.25]));
    level(&mut rt);
    // Frames 512..768: back to 0 dB and centre (waits are 480 frames).
    assert!(close(level(&mut rt), [0.5, 0.5]));
    level(&mut rt);
    // Faded out and stopped: the voice is gone.
    assert_eq!(level(&mut rt), [0.0; 2]);
    assert_eq!(rt.voice_count(), 0);
}

#[test]
fn engine_volume_and_purge_address_one_group() {
    let mut rt = runtime(
        "on init
           set_engine_par($ENGINE_PAR_VOLUME, 0, 0, -1, -1)
         end on
         on note
           if ($EVENT_NOTE = 61)
             set_engine_par($ENGINE_PAR_VOLUME, 500000, 0, -1, -1)
             set_engine_par($ENGINE_PAR_TUNE, 500000, 1, -1, -1)
             purge_group(1, 0)
           end if
         end on",
    );
    rt.trigger(input(60), 60, 1.).unwrap();
    // on init's write is not a runtime callback; group 0 plays at 0 dB.
    assert!(close(level(&mut rt), [0.5; 2]));
    rt.trigger(input(61), 61, 1.).unwrap();
    // 500000 is -6.02 dB for group 0 (both of its notes); group 1 is purged.
    assert!(close(level(&mut rt), [0.2506; 2]), "{:?}", level(&mut rt));
}

#[test]
fn volume_envelope_attack_applies_to_voices_that_start_after_it() {
    let mut rt = runtime(
        "on note
           { 200809 is 10 ms (480 frames) on Kontakt's attack law. }
           set_engine_par($ENGINE_PAR_ATTACK, 200809, 0, find_mod(0, \"ENV_AHDSR\"), -1)
           { Another modulator's attack is left to the host. }
           set_engine_par($ENGINE_PAR_ATTACK, 0, 1, find_mod(1, \"ENV_FILTER\"), -1)
         end on",
    );
    rt.trigger(input(60), 60, 1.).unwrap();
    let level = level(&mut rt)[0];
    assert!((level - 0.5 * 256. / 480.).abs() < 0.01, "{level}");
}

#[test]
fn engine_volume_reads_the_authored_value_and_sets_it_absolutely() {
    let mut rt = authored(
        "on note
           { The authored -6 dB group reads back as 500000. }
           if (abs(get_engine_par($ENGINE_PAR_VOLUME, 0, -1, -1) - 500000) < 20)
             { 629960 is 0 dB, not +6 dB on top of the authored value. }
             set_engine_par($ENGINE_PAR_VOLUME, 629960, 0, -1, -1)
           end if
         end on",
        GroupParams {
            decibels: -6.0,
            ..GroupParams::default()
        },
    );
    rt.trigger(input(60), 60, 1.).unwrap();
    assert!(close(level(&mut rt), [0.5; 2]), "{:?}", level(&mut rt));
}

#[test]
fn timer_listener_plays_notes_on_its_period() {
    let mut rt = runtime(
        "on init
           declare $n
           set_listener($NI_SIGNAL_TIMER_MS, 10000)
         end on
         on listener
           if ($NI_SIGNAL_TYPE = $NI_SIGNAL_TIMER_MS)
             inc($n)
             if ($n = 2)
               { The second tick stops the timer. }
               change_listener_par($NI_SIGNAL_TIMER_MS, 0)
             end if
             play_note(60, 100, 0, 100000)
           end if
         end on",
    );
    // 10 ms is 480 frames: silent before the first tick, sounding after.
    assert_eq!(level(&mut rt), [0.0; 2]);

    level(&mut rt);
    // Velocity 100: 0.5 · 100/127.
    assert!(close(level(&mut rt), [0.3937; 2]));
    for _ in 0..25 {
        level(&mut rt);
    }
    // Two 100 ms notes, then no more ticks.
    assert_eq!(level(&mut rt), [0.0; 2]);
    assert_eq!(rt.voice_count(), 0);
}

#[test]
fn pgs_writes_reach_every_slot_and_run_pgs_changed() {
    let writer = compile(
        "on note
           ignore_event($EVENT_ID)
           pgs_set_key_val(SHARED, 0, 5)
         end on",
    );
    let reader = compile(
        "on init
           pgs_create_key(SHARED, 1)
         end on
         on pgs_changed
           if (pgs_key_exists(SHARED) and pgs_get_key_val(SHARED, 0) = 5)
             play_note(61, 127, 0, 100000)
           end if
         end on",
    );
    let mut rt = modules(vec![writer, reader], GroupParams::default());
    rt.trigger(input(60), 60, 1.).unwrap();
    // Only the reader's note 61 sounds.
    assert!(close(level(&mut rt), [0.25; 2]), "{:?}", level(&mut rt));
}

#[test]
fn sort_and_array_equal_run_at_runtime() {
    let mut rt = runtime(
        "on init
           declare %a[4] := (59, 61, 58, 60)
           declare %b[4] := (61, 60, 59, 58)
         end on
         on note
           ignore_event($EVENT_ID)
           sort(%a, 1)
           if (array_equal(%a, %b))
             sort(%a, 0, 1, 3)
             { 61, 58, 59, 60 }
             if (%a[1] = 58 and %a[3] = 60)
               play_note(61, 127, 0, 100000)
             end if
           end if
         end on",
    );
    rt.trigger(input(60), 60, 1.).unwrap();
    assert!(close(level(&mut rt), [0.25; 2]), "{:?}", level(&mut rt));
}

#[test]
fn runtime_ui_requests_update_the_model() {
    let source = "on init
           declare ui_label $offline(1, 1)
           set_listener($NI_SIGNAL_TIMER_MS, 10000)
         end on
         on listener
           set_control_par(get_ui_id($offline), $CONTROL_PAR_HIDE, $HIDE_WHOLE_CONTROL)
           set_control_par_str(get_ui_id($offline), $CONTROL_PAR_PICTURE, \"online\")
           set_key_color(60, $KEY_COLOR_CYAN)
         end on";
    let mut model = compile(source);
    let mut rt = runtime(source);
    for _ in 0..3 {
        level(&mut rt);
    }
    let mut view = model.view();
    let mut applied = 0;
    rt.drain_effects(|e| {
        applied += usize::from(model.apply_ui_effect(e));
        assert!(view.apply_ui_effect(e));
        true
    });
    assert_eq!(view.ui(&|_| None).unwrap(), model.ui(&|_| None).unwrap());
    let w = &model.model().interface.widgets[0];
    assert_eq!(applied, 3);
    assert_eq!(view.model().interface.keys[60].color, Some(8));
    assert_eq!(w.int("$CONTROL_PAR_HIDE"), Some(16));
    assert_eq!(
        w.properties.get("$CONTROL_PAR_PICTURE"),
        Some(&sampler_ksp::model::Value::Text("online".into()))
    );
}

#[test]
fn key_down_reads_held_input_keys() {
    let mut rt = runtime(
        "on note
           ignore_event($EVENT_ID)
           if (%KEY_DOWN[60] = 1 and %KEY_DOWN[61] = 0)
             play_note(61, 127, 0, 100000)
           end if
         end on",
    );
    rt.trigger(input(60), 60, 1.).unwrap();
    assert!(close(level(&mut rt), [0.25; 2]), "{:?}", level(&mut rt));
}
