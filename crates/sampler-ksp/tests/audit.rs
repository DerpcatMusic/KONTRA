//! Kontakt 8 contract probes. Known failures are opt-in so this audit branch
//! adds no runtime changes and does not break the normal test suite.
//! Run: cargo test -p sampler-ksp --test audit -- --ignored
use sampler_core::*;
use sampler_ksp::{Environment, Script};

fn compile_in(source: &str, environment: &Environment) -> Script {
    sampler_ksp::compile_with(
        source,
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
        environment,
    )
    .unwrap()
}

fn compile(source: &str) -> Script {
    compile_in(
        source,
        &Environment {
            groups: vec!["Group 0".into()],
            ..Environment::default()
        },
    )
}

fn runtime_scripts(scripts: Vec<Script>) -> Runtime {
    let prepared = Prepared::new(
        48000,
        vec![Pcm::new(48000, Box::from([[1.; 2]; 48000])).unwrap()],
        vec![Region {
            sample: 0,
            key_low: 0,
            key_high: 127,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope: Envelope::default(),
            playback: Playback::default(),
        }],
        128,
    )
    .unwrap()
    .with_groups(1, vec![Some(0)])
    .unwrap();
    let plan = sampler_ksp::bind_modules(scripts, prepared).unwrap();
    let limits = Limits {
        notes: 16,
        channels: 1,
        performances: 1,
        families: 16,
        voices: 16,
        expressions: 16,
        decisions: 0,
        commands: 64,
        behaviors: 32,
        behavior_fuel: 16384,
        behavior_cells: plan.behavior_local_count() * 32,
        note_cells: plan.note_cell_count() * 16,
    };
    Runtime::new(plan, limits).unwrap()
}

fn runtime(source: &str) -> Runtime {
    runtime_scripts(vec![compile(source)])
}

fn input(id: i32) -> Input {
    Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(id),
    }
}

fn cell(rt: &Runtime, instance: u16, cell: u32) -> i64 {
    rt.script_cell(rt.active_plan(), ScriptInstanceId(instance), cell)
        .unwrap()
}

fn note(rt: &mut Runtime) -> NoteId {
    rt.trigger(input(1), 60, 1.).unwrap()
}

macro_rules! contract {
    ($name:ident, $reason:literal, $source:literal, $cell:expr, $expected:expr) => {
        #[test]
        #[ignore = $reason]
        fn $name() {
            let mut rt = runtime($source);
            note(&mut rt);
            assert_eq!(cell(&rt, 0, $cell), $expected);
        }
    };
}

contract!(
    init_engine_read_matches_authored_center_pan,
    "audit: init engine readback",
    "on init declare $out := get_engine_par($ENGINE_PAR_PAN, 0, -1, -1) end on",
    0,
    500000
);
contract!(
    init_engine_write_reaches_runtime,
    "audit: init engine writes",
    "on init declare $out set_engine_par($ENGINE_PAR_PAN, 1000000, 0, -1, -1) end on
     on note $out := get_engine_par($ENGINE_PAR_PAN, 0, -1, -1) end on",
    0,
    1000000
);
contract!(
    missing_modulator_returns_not_found,
    "audit: fabricated modulator index",
    "on init declare $out end on on note $out := find_mod(0, \"missing\") end on",
    0,
    -1
);
contract!(
    purge_state_reports_loaded_samples,
    "audit: purge readback",
    "on init declare $out end on on note $out := get_purge_state(0) end on",
    0,
    1
);
contract!(
    runtime_time_conversion_uses_microseconds,
    "audit: ms_to_ticks 1000x units",
    "on init declare $out end on on note $out := ms_to_ticks(500000) end on",
    0,
    960
);
contract!(
    runtime_tick_conversion_returns_microseconds,
    "audit: ticks_to_ms 1000x units",
    "on init declare $out end on on note $out := ticks_to_ms(960) end on",
    0,
    500000
);
contract!(
    active_event_status_is_note_queue,
    "audit: event status",
    "on init declare $out end on on note $out := event_status($EVENT_ID) end on",
    0,
    1
);
contract!(
    get_event_ids_contains_current_event,
    "audit: event enumeration",
    "on init declare %ids[16] declare $out end on
     on note get_event_ids(%ids) $out := search(%ids, $EVENT_ID) end on",
    16,
    0
);
contract!(
    event_mark_is_readable,
    "audit: marks",
    "on init declare $out end on on note set_event_mark($EVENT_ID, $MARK_1)
     $out := get_event_mark($EVENT_ID, $MARK_1) end on",
    0,
    1
);
contract!(
    current_event_group_allow_state_is_readable,
    "audit: event group readback",
    "on init declare $out end on on note
     $out := get_event_par_arr($EVENT_ID, $EVENT_PAR_ALLOW_GROUP, 0) end on",
    0,
    1
);
contract!(
    custom_event_array_parameters_roundtrip,
    "audit: custom event parameters",
    "on init declare $out end on on note
     set_event_par_arr($EVENT_ID, $EVENT_PAR_CUSTOM, 42, 15)
     $out := get_event_par_arr($EVENT_ID, $EVENT_PAR_CUSTOM, 15) end on",
    0,
    42
);
contract!(
    thirteen_script_modulator_ids_roundtrip,
    "audit: per-note modulator store drops IDs after twelve",
    "on init declare $out declare $i end on
     on note $i := 0 while ($i < 13)
     set_event_par_arr($EVENT_ID, $EVENT_PAR_MOD_VALUE_ID, 42, $i)
     inc($i) end while
     $out := get_event_par_arr($EVENT_ID, $EVENT_PAR_MOD_VALUE_ID, 12) end on",
    0,
    42
);
contract!(
    affected_groups_have_dynamic_size,
    "audit: GROUPS_AFFECTED",
    "on init declare $out end on on note $out := num_elements(%GROUPS_AFFECTED) end on",
    0,
    1
);
contract!(
    relative_mode_two_is_absolute,
    "audit: change_vol mode 2",
    "on init declare $out end on on note change_vol($EVENT_ID, -6000, 2)
     change_vol($EVENT_ID, -3000, 2)
     $out := get_event_par($EVENT_ID, $EVENT_PAR_VOLUME) end on",
    0,
    -3000
);
contract!(
    table_value_setter_updates_script_array,
    "audit: UI table state",
    "on init declare ui_table %table[2] (1, 1, 100) declare $out end on
     on note set_control_par_arr(get_ui_id(%table), $CONTROL_PAR_VALUE, 7, 1)
     $out := %table[1] end on",
    2,
    7
);
contract!(
    pan_mode_two_is_absolute,
    "audit: change_pan mode 2 accumulates",
    "on init declare $out end on on note change_pan($EVENT_ID, 1000, 2)
     change_pan($EVENT_ID, -1000, 2)
     $out := get_event_par($EVENT_ID, $EVENT_PAR_PAN) end on",
    0,
    -1000
);
contract!(
    indexed_control_properties_do_not_alias,
    "audit: indexed UI mirror",
    "on init declare ui_table %table[2] (1, 1, 100) declare $out end on
     on note set_control_par_arr(get_ui_id(%table), $CONTROL_PAR_VALUE, 7, 0)
     set_control_par_arr(get_ui_id(%table), $CONTROL_PAR_VALUE, 9, 1)
     $out := get_control_par_arr(get_ui_id(%table), $CONTROL_PAR_VALUE, 0) end on",
    2,
    7
);
contract!(
    real_array_sort_executes,
    "audit: real sort faults",
    "on init declare ?a[2] := (2.0, 1.0) declare $out end on
     on note sort(?a, 0) $out := int(?a[0]) end on",
    2,
    1
);

#[test]
#[ignore = "audit: current slot at init"]
fn init_current_script_slot_uses_environment() {
    let rt = runtime_scripts(vec![compile_in(
        "on init declare $out := $CURRENT_SCRIPT_SLOT end on",
        &Environment {
            slot: 3,
            ..Default::default()
        },
    )]);
    assert_eq!(cell(&rt, 0, 0), 3);
}

#[test]
#[ignore = "audit: persistence callback context"]
fn persistence_callback_has_its_own_type() {
    let rt = runtime(
        "on init declare $out end on
        on persistence_changed $out := $NI_CALLBACK_TYPE end on",
    );
    assert_eq!(cell(&rt, 0, 0), 11);
}

#[test]
#[ignore = "audit: callback IDs alias event IDs"]
fn note_and_release_have_distinct_callback_ids() {
    let mut rt = runtime(
        "on init declare $a declare $b end on
        on note $a := $NI_CALLBACK_ID end on
        on release $b := $NI_CALLBACK_ID end on",
    );
    let n = note(&mut rt);
    rt.key_up(n, None).unwrap();
    assert_ne!(cell(&rt, 0, 0), cell(&rt, 0, 1));
}

#[test]
#[ignore = "audit: stop_wait is an unconsumed effect"]
fn stop_wait_resumes_suspended_callback() {
    let mut rt = runtime(
        "on init declare $id declare $out end on
        on note $id := $NI_CALLBACK_ID wait(1000000) $out := 1 end on
        on controller stop_wait($id, 0) end on",
    );
    note(&mut rt);
    let domain = rt.performance(0).unwrap();
    rt.dispatch_controller(domain, input(1).channel_address(), 1, 1, 1)
        .unwrap();
    rt.render(&mut [[0.; 2]; 48]).unwrap();
    assert_eq!(cell(&rt, 0, 1), 1);
}

#[test]
#[ignore = "audit: reset_ksp_timer has no state effect"]
fn reset_ksp_timer_resets_readback() {
    let mut rt = runtime(
        "on init declare $out end on
        on note reset_ksp_timer $out := $KSP_TIMER end on",
    );
    rt.render(&mut [[0.; 2]; 480]).unwrap();
    note(&mut rt);
    assert!(cell(&rt, 0, 0) < 1000);
}

#[test]
#[ignore = "audit: by_marks note_off is dropped"]
fn note_off_by_marks_releases_matching_event() {
    let mut rt = runtime(
        "on note set_event_mark($EVENT_ID, $MARK_1) end on
        on controller note_off(by_marks($MARK_1)) end on",
    );
    let n = note(&mut rt);
    let domain = rt.performance(0).unwrap();
    rt.dispatch_controller(domain, input(1).channel_address(), 1, 1, 1)
        .unwrap();
    assert_eq!(rt.key_down(n), Ok(false));
}

#[test]
#[ignore = "audit: release velocity readback missing"]
fn release_velocity_is_event_parameter() {
    let mut rt = runtime(
        "on init declare $out end on
        on release $out := get_event_par($EVENT_ID, $EVENT_PAR_REL_VELOCITY) end on",
    );
    let n = note(&mut rt);
    rt.key_up(n, Some(64. / 127.)).unwrap();
    assert_eq!(cell(&rt, 0, 0), 64);
}

#[test]
#[ignore = "audit: creator slot lost across modules"]
fn event_source_reports_creating_script_slot() {
    let scripts = [
        (
            0,
            "on note ignore_event($EVENT_ID) play_note(60, 127, 0, 100000) end on",
        ),
        (
            1,
            "on init declare $out end on on note
        $out := get_event_par($EVENT_ID, $EVENT_PAR_SOURCE) end on",
        ),
    ]
    .into_iter()
    .map(|(slot, source)| {
        compile_in(
            source,
            &Environment {
                slot,
                ..Default::default()
            },
        )
    })
    .collect();
    let mut rt = runtime_scripts(scripts);
    note(&mut rt);
    assert_eq!(cell(&rt, 1, 0), 0);
}

#[test]
#[ignore = "audit: dynamic engine parameter address goes to mirror"]
fn dynamic_engine_parameter_write_changes_audio() {
    let mut rt = runtime(
        "on init declare $p := $ENGINE_PAR_VOLUME end on
        on note set_engine_par($p, 0, 0, -1, -1) end on",
    );
    note(&mut rt);
    let mut audio = [[0.; 2]; 256];
    rt.render(&mut audio).unwrap();
    assert_eq!(audio[255], [0.; 2]);
}

#[test]
#[ignore = "audit: load-time persistence purge is never applied"]
fn load_persistence_purge_excludes_group_from_playback() {
    let mut rt = runtime("on persistence_changed purge_group(0, 0) end on");
    note(&mut rt);
    let mut audio = [[0.; 2]; 256];
    rt.render(&mut audio).unwrap();
    assert_eq!(audio[255], [0.; 2]);
}

#[test]
#[ignore = "audit: runtime text property readback empty"]
fn text_property_write_is_visible_to_script() {
    let mut rt = runtime(
        "on init declare ui_label $label (1, 1) declare @out end on
        on note set_control_par_str(get_ui_id($label), $CONTROL_PAR_TEXT, \"new\")
        @out := get_control_par_str(get_ui_id($label), $CONTROL_PAR_TEXT) end on",
    );
    note(&mut rt);
    assert_eq!(
        rt.script_text(rt.active_plan(), ScriptInstanceId(0), 0)
            .unwrap()
            .as_str(),
        "new"
    );
}

#[test]
#[ignore = "audit: engine display queries return empty text"]
fn engine_display_query_returns_pan_text() {
    let mut rt = runtime(
        "on init declare @out end on
        on note @out := get_engine_par_disp($ENGINE_PAR_PAN, 0, -1, -1) end on",
    );
    note(&mut rt);
    assert!(
        !rt.script_text(rt.active_plan(), ScriptInstanceId(0), 0)
            .unwrap()
            .as_str()
            .is_empty()
    );
}

#[test]
#[ignore = "audit: async work cannot complete"]
fn asynchronous_ir_request_has_identity_and_completion() {
    let script = compile(
        "on init declare $id declare $done declare ui_button $load end on
        on ui_control($load)
        $id := load_ir_sample(\"missing.wav\", 0, $NI_SEND_BUS) wait_async($id) end on
        on async_complete $done := 1 end on",
    );
    let id = script.controls()[0].definition.id;
    let mut rt = runtime_scripts(vec![script]);
    rt.invoke_control(
        ControlContext {
            performance: rt.performance(0).unwrap(),
            origin: input(1).channel_address(),
            channels: 1,
        },
        rt.active_plan(),
        Some(0),
        ControlWrite {
            id,
            value: ControlValue::Integer(1),
        },
    )
    .unwrap();
    rt.render(&mut [[0.; 2]; 480]).unwrap();
    assert_eq!(
        cell(&rt, 0, 1),
        1,
        "even failure must complete asynchronously"
    );
}

#[test]
#[ignore = "audit: MIDI file commands absent"]
fn midi_file_buffer_commands_are_supported() {
    assert!(
        sampler_ksp::compile(
            "on init mf_set_buffer_size(16) end on",
            48000,
            sampler_ksp::Limits::LIBRARY,
            &[]
        )
        .is_ok()
    );
}

#[test]
#[ignore = "audit: ui_controls callback absent"]
fn global_ui_callback_is_supported() {
    assert!(
        sampler_ksp::compile(
            "on ui_controls end on",
            48000,
            sampler_ksp::Limits::LIBRARY,
            &[]
        )
        .is_ok()
    );
}

#[test]
#[ignore = "audit: strings truncate before Kontakt's 320 character limit"]
fn string_capacity_covers_320_characters() {
    let mut rt = runtime(
        "on init declare @out declare $i end on
        on note while ($i < 30) @out := @out & \"0123456789\" inc($i) end while end on",
    );
    note(&mut rt);
    assert_eq!(
        rt.script_text(rt.active_plan(), ScriptInstanceId(0), 0)
            .unwrap()
            .as_str()
            .len(),
        300
    );
}

#[test]
fn listener_can_generate_notes_without_input() {
    let mut rt = runtime(
        "on init set_listener($NI_SIGNAL_TIMER_MS, 10000) end on
        on listener play_note(60, 127, 0, 100000) end on",
    );
    rt.render(&mut [[0.; 2]; 1000]).unwrap();
    assert!(
        rt.voice_count() > 0,
        "listener fault: {:?}",
        rt.take_fault()
    );
}

#[test]
#[ignore = "audit: default runtime musical duration is zero"]
fn musical_duration_is_available_in_note_callback() {
    let mut rt = runtime(
        "on init declare $out end on
        on note $out := $DURATION_QUARTER end on",
    );
    note(&mut rt);
    assert_eq!(cell(&rt, 0, 0), 500000);
}

#[test]
#[ignore = "audit: invalid real search accepted"]
fn real_search_is_rejected_as_documented() {
    assert!(
        sampler_ksp::compile(
            "on init declare ?a[1] declare $out end on
        on note $out := search(?a, 0.0) end on",
            48000,
            sampler_ksp::Limits::LIBRARY,
            &[]
        )
        .is_err()
    );
}

#[test]
fn ignore_event_accepts_an_aliased_current_id() {
    let mut rt = runtime(
        "on init declare $id end on
        on note $id := $EVENT_ID ignore_event($id) end on",
    );
    note(&mut rt);
    assert_eq!(rt.voice_count(), 0);
}

#[test]
#[ignore = "audit: controller path rejects virtual pitch bend controller"]
fn pitch_bend_can_trigger_controller_callback() {
    let mut rt = runtime(
        "on init declare $out end on
        on controller inc($out) end on",
    );
    let domain = rt.performance(0).unwrap();
    rt.dispatch_controller(domain, input(1).channel_address(), 1, 128, 0x80000000)
        .unwrap();
    assert_eq!(cell(&rt, 0, 0), 1);
}

#[test]
#[ignore = "audit: set_text effect is not applied by UI consumer"]
fn set_text_updates_the_runtime_ui_model() {
    let script = compile(
        "on init declare ui_label $label (1, 1) set_text($label, \"old\") end on
        on note set_text($label, \"new\") end on",
    );
    let mut view = script.view();
    let mut rt = runtime_scripts(vec![script]);
    note(&mut rt);
    rt.drain_effects(|effect| {
        view.apply_ui_effect(effect);
        true
    });
    assert_eq!(
        view.model().interface.widgets[0].text("$CONTROL_PAR_TEXT"),
        Some("new")
    );
}

#[test]
#[ignore = "audit: init PGS state is not shared across script evaluations"]
fn later_script_init_reads_prior_slot_pgs_state() {
    let rt = runtime_scripts(vec![
        compile("on init pgs_create_key(TEST, 1) pgs_set_key_val(TEST, 0, 42) end on"),
        compile("on init declare $out := pgs_get_key_val(TEST, 0) end on"),
    ]);
    assert_eq!(cell(&rt, 1, 0), 42);
}

#[test]
#[ignore = "audit: current event group edits require deferred generated event"]
fn set_event_group_can_disable_current_note_group() {
    let mut rt = runtime(
        "on note
        set_event_par_arr($EVENT_ID, $EVENT_PAR_ALLOW_GROUP, 0, $ALL_GROUPS) end on",
    );
    note(&mut rt);
    assert_eq!(rt.voice_count(), 0);
}

#[test]
fn integer_and_polyphonic_baseline() {
    let mut rt = runtime(
        "on init declare polyphonic $p declare $sum declare $div end on
        on note $p := $EVENT_VELOCITY $sum := 2147483647 + 1 $div := -7 / 2 end on
        on release $div := $p end on",
    );
    let a = rt.trigger(input(1), 60, 64. / 127.).unwrap();
    let b = rt.trigger(input(2), 60, 96. / 127.).unwrap();
    assert_eq!(cell(&rt, 0, 0), i64::from(i32::MIN));
    assert_eq!(cell(&rt, 0, 1), -3);
    rt.key_up(a, None).unwrap();
    assert_eq!(cell(&rt, 0, 1), 64);
    rt.key_up(b, None).unwrap();
    assert_eq!(cell(&rt, 0, 1), 96);
}
