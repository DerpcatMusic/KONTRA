//! W7 failing-first selection/lifecycle contracts, adapted from the ranked KSP audit.
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;
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
    runtime_prepared(scripts, prepared)
}

fn runtime_prepared(scripts: Vec<Script>, prepared: Prepared) -> Runtime {
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
        behavior_fuel: 65536,
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

#[test]
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
fn event_source_reports_creating_script_slot() {
    let scripts = [
        (
            3,
            "on note ignore_event($EVENT_ID) play_note(60, 127, 0, 100000) end on",
        ),
        (
            4,
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
    assert_eq!(cell(&rt, 1, 0), 3);
}
#[test]
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
fn current_event_allow_group_changes_current_selection() {
    let mut rt = runtime(
        "on note set_event_par_arr($EVENT_ID, $EVENT_PAR_ALLOW_GROUP, 0, $ALL_GROUPS) end on",
    );
    note(&mut rt);
    let mut output = [[0.; 2]; 64];
    rt.render(&mut output).unwrap();
    assert!(
        output.iter().flatten().all(|x| *x == 0.),
        "current event sounded despite its mask"
    );
}

#[test]
fn release_counter_reset_uses_current_clock_and_freezes_after_key_up() {
    let mut rt = runtime(
        "on init declare $id end on on note $id := $EVENT_ID end on on controller reset_rls_trig_counter($id) end on",
    );
    let n = note(&mut rt);
    rt.render(&mut [[0.; 2]; 480]).unwrap();
    assert_eq!(rt.release_counter_frames(n).unwrap(), 480);
    let domain = rt.performance(0).unwrap();
    rt.dispatch_controller(domain, input(1).channel_address(), 1, 1, 1)
        .unwrap();
    assert_eq!(rt.release_counter_frames(n).unwrap(), 0);
    rt.render(&mut [[0.; 2]; 48]).unwrap();
    rt.key_up(n, None).unwrap();
    rt.render(&mut [[0.; 2]; 48]).unwrap();
    assert_eq!(rt.release_counter_frames(n).unwrap(), 48);
}

#[test]
fn stop_wait_can_disable_later_waits_without_leaving_a_stale_resume() {
    let mut rt = runtime(
        "on init declare $id declare $out end on on note $id := $NI_CALLBACK_ID wait(1000000) $out := 1 wait(1000000) $out := 2 end on on controller stop_wait($id, 1) end on",
    );
    note(&mut rt);
    let p = rt.performance(0).unwrap();
    rt.dispatch_controller(p, input(1).channel_address(), 1, 1, 1)
        .unwrap();
    rt.render(&mut [[0.; 2]; 1]).unwrap();
    assert_eq!(cell(&rt, 0, 1), 2);
    assert_eq!(rt.pending_commands(), 0);
}

macro_rules! event_contract {
    ($name:ident, $reason:literal, $source:literal, $cell:expr, $expected:expr) => {
        #[test]
        fn $name() {
            let mut rt = runtime($source);
            note(&mut rt);
            assert_eq!(cell(&rt, 0, $cell), $expected);
        }
    };
}
event_contract!(
    active_event_status_is_note_queue,
    "audit: event status",
    "on init declare $out end on on note $out := event_status($EVENT_ID) end on",
    0,
    1
);
event_contract!(
    get_event_ids_contains_current_event,
    "audit: event enumeration",
    "on init declare %ids[16] declare $out end on
     on note get_event_ids(%ids) $out := search(%ids, $EVENT_ID) end on",
    16,
    0
);
event_contract!(
    event_mark_is_readable,
    "audit: marks",
    "on init declare $out end on on note set_event_mark($EVENT_ID, $MARK_1)
     $out := get_event_mark($EVENT_ID, $MARK_1) end on",
    0,
    1
);
event_contract!(
    current_event_group_allow_state_is_readable,
    "audit: event group readback",
    "on init declare $out end on on note
     $out := get_event_par_arr($EVENT_ID, $EVENT_PAR_ALLOW_GROUP, 0) end on",
    0,
    1
);
event_contract!(
    custom_event_array_parameters_roundtrip,
    "audit: custom event parameters",
    "on init declare $out end on on note
     set_event_par_arr($EVENT_ID, $EVENT_PAR_CUSTOM, 42, 15)
     $out := get_event_par_arr($EVENT_ID, $EVENT_PAR_CUSTOM, 15) end on",
    0,
    42
);
event_contract!(
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
event_contract!(
    affected_groups_have_dynamic_size,
    "audit: GROUPS_AFFECTED",
    "on init declare $out end on on note $out := num_elements(%GROUPS_AFFECTED) end on",
    0,
    1
);
event_contract!(
    relative_mode_two_is_absolute,
    "audit: change_vol mode 2",
    "on init declare $out end on on note change_vol($EVENT_ID, -6000, 2)
     change_vol($EVENT_ID, -3000, 2)
     $out := get_event_par($EVENT_ID, $EVENT_PAR_VOLUME) end on",
    0,
    -3000
);
event_contract!(
    pan_mode_two_is_absolute,
    "audit: change_pan mode 2 accumulates",
    "on init declare $out end on on note change_pan($EVENT_ID, 1000, 2)
     change_pan($EVENT_ID, -1000, 2)
     $out := get_event_par($EVENT_ID, $EVENT_PAR_PAN) end on",
    0,
    -1000
);

#[test]
fn event_queries_and_arrays_run_without_heap_and_retire_with_the_event() {
    let mut rt = runtime(
        "on init
        declare %ids[4] := (9,9,9,9)
        declare $id declare $status declare $marked declare $custom
        end on
        on note
        get_event_ids(%ids)
        $id := $EVENT_ID
        set_event_mark($id, $MARK_28)
        set_event_par_arr($id, $EVENT_PAR_CUSTOM, -2147483648, 15)
        $status := event_status($id)
        $marked := get_event_mark($id, $MARK_28)
        $custom := get_event_par_arr($id, $EVENT_PAR_CUSTOM, 15)
        end on
        on controller
        get_event_ids(%ids)
        $status := event_status($id)
        $marked := get_event_mark($id, $MARK_28)
        $custom := get_event_par_arr($id, $EVENT_PAR_CUSTOM, 15)
        end on",
    );
    support::without_heap(|| {
        note(&mut rt);
    });
    assert_eq!(cell(&rt, 0, 0), cell(&rt, 0, 4));
    assert_eq!(
        [cell(&rt, 0, 1), cell(&rt, 0, 2), cell(&rt, 0, 3)],
        [0, 9, 9]
    );
    assert_eq!(
        [cell(&rt, 0, 5), cell(&rt, 0, 6), cell(&rt, 0, 7)],
        [1, 1, i64::from(i32::MIN)]
    );
    rt.panic();
    rt.flush_behaviors(|_, _, _| true);
    rt.flush_ended(|_| true);
    let domain = rt.performance(0).unwrap();
    support::without_heap(|| {
        rt.dispatch_controller(domain, input(1).channel_address(), 1, 1, 1)
            .unwrap();
    });
    assert_eq!(cell(&rt, 0, 0), 0);
    assert_eq!(
        [cell(&rt, 0, 5), cell(&rt, 0, 6), cell(&rt, 0, 7)],
        [0, 0, 0]
    );
}

#[test]
fn custom_parameters_share_standard_indices_and_do_not_alias_modulators() {
    let mut rt = runtime(
        "on init declare $a declare $b declare $c declare $d declare $e end on
        on note
        set_event_par($EVENT_ID, $EVENT_PAR_0, 19)
        $a := get_event_par_arr($EVENT_ID, $EVENT_PAR_CUSTOM, 0)
        set_event_par_arr($EVENT_ID, $EVENT_PAR_CUSTOM, 23 + 7, 3)
        $b := get_event_par($EVENT_ID, $EVENT_PAR_3)
        set_event_par_arr($EVENT_ID, $EVENT_PAR_MOD_VALUE_ID, 2000000, 1000)
        set_event_par_arr($EVENT_ID, $EVENT_PAR_MOD_VALUE_ID, 88, 1001)
        set_event_par_arr($EVENT_ID, $EVENT_PAR_CUSTOM, 77, -1)
        set_event_par_arr($EVENT_ID, $EVENT_PAR_CUSTOM, 66, 16)
        $c := get_event_par_arr($EVENT_ID, $EVENT_PAR_MOD_VALUE_ID, 1000)
        $d := get_event_par_arr($EVENT_ID, $EVENT_PAR_CUSTOM, 0)
        $e := get_event_par_arr($EVENT_ID, $EVENT_PAR_CUSTOM, 16)
        end on",
    );
    note(&mut rt);
    assert_eq!(
        (0..5).map(|i| cell(&rt, 0, i)).collect::<Vec<_>>(),
        vec![19, 30, 1000000, 19, 0]
    );
}

#[test]
fn event_mark_deletion_preserves_other_marks_and_note_reuse_clears_them() {
    let mut rt = runtime(
        "on init declare $a declare $b declare $c end on
        on note
        $c := get_event_mark($EVENT_ID, $MARK_28)
        set_event_mark($EVENT_ID, $MARK_1 + $MARK_28)
        delete_event_mark($EVENT_ID, $MARK_1)
        $a := get_event_mark($EVENT_ID, $MARK_1)
        $b := get_event_mark($EVENT_ID, $MARK_28)
        end on",
    );
    for _ in 0..2 {
        note(&mut rt);
        assert_eq!(
            [cell(&rt, 0, 0), cell(&rt, 0, 1), cell(&rt, 0, 2)],
            [0, 1, 0]
        );
        rt.panic();
        rt.flush_behaviors(|_, _, _| true);
        rt.flush_ended(|_| true);
    }
}

#[test]
fn dynamic_relative_mode_only_adds_for_one() {
    let mut rt = runtime(
        "on init declare $mode := 2 declare $a declare $b end on
        on note
        change_vol($EVENT_ID,-6000,$mode)
        change_vol($EVENT_ID,-3000,$mode)
        $a := get_event_par($EVENT_ID,$EVENT_PAR_VOLUME)
        $mode := 1
        change_vol($EVENT_ID,-1000,$mode)
        $b := get_event_par($EVENT_ID,$EVENT_PAR_VOLUME)
        end on",
    );
    note(&mut rt);
    assert_eq!([cell(&rt, 0, 1), cell(&rt, 0, 2)], [-3000, -4000]);
}

#[test]
fn affected_groups_keep_physical_holes_and_ignore_script_group_disallow() {
    let prepared = Prepared::new(
        48000,
        vec![Pcm::new(48000, Box::from([[1.; 2]; 64])).unwrap()],
        vec![Region {
            sample: 0,
            key_low: 60,
            key_high: 60,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope: Envelope::default(),
            playback: Playback::default(),
        }],
        1,
    )
    .unwrap()
    .with_groups(5, vec![Some(3)])
    .unwrap();
    let script = compile_in(
        "on init declare $size declare $group declare $allowed declare $after end on
        on note
        disallow_group($ALL_GROUPS)
        $size := num_elements(%GROUPS_AFFECTED)
        $group := %GROUPS_AFFECTED[0]
        $allowed := get_event_par_arr($EVENT_ID,$EVENT_PAR_ALLOW_GROUP,3)
        allow_group(3)
        $after := get_event_par_arr($EVENT_ID,$EVENT_PAR_ALLOW_GROUP,3)
        end on",
        &Environment {
            groups: (0..5).map(|g| format!("Group {g}")).collect(),
            ..Default::default()
        },
    );
    let mut rt = runtime_prepared(vec![script], prepared);
    support::without_heap(|| {
        note(&mut rt);
    });
    assert_eq!(
        (0..4).map(|i| cell(&rt, 0, i)).collect::<Vec<_>>(),
        vec![1, 3, 0, 1]
    );
    rt.panic();
    rt.trigger(input(2), 61, 1.).unwrap();
    assert_eq!(cell(&rt, 0, 0), 0);
    assert_eq!(cell(&rt, 0, 1), -1);
}

#[test]
fn release_reads_its_group_view_and_event_tags() {
    let mut rt = runtime(
        "on init declare $a declare $b declare $c declare $d end on
        on note
        set_event_mark($EVENT_ID,$MARK_1)
        set_event_par_arr($EVENT_ID,$EVENT_PAR_CUSTOM,42,15)
        end on
        on release
        disallow_group($ALL_GROUPS)
        $a := get_event_par_arr($EVENT_ID,$EVENT_PAR_ALLOW_GROUP,0)
        allow_group(0)
        $b := get_event_par_arr($EVENT_ID,$EVENT_PAR_ALLOW_GROUP,0)
        $c := get_event_mark($EVENT_ID,$MARK_1)
        $d := get_event_par_arr($EVENT_ID,$EVENT_PAR_CUSTOM,15)
        end on",
    );
    let n = note(&mut rt);
    support::without_heap(|| {
        rt.key_up(n, None).unwrap();
    });
    assert_eq!(
        (0..4).map(|i| cell(&rt, 0, i)).collect::<Vec<_>>(),
        vec![0, 1, 1, 42]
    );
}

#[test]
fn all_native_modulator_ids_and_custom_parameters_roundtrip_together() {
    let mut rt = runtime(
        "on init declare $i declare $sum declare $custom_sum end on
        on note
        $i := 0
        while ($i <= 1000)
            set_event_par_arr($EVENT_ID,$EVENT_PAR_MOD_VALUE_ID,$i,$i)
            inc($i)
        end while
        $i := 0
        while ($i < 16)
            set_event_par_arr($EVENT_ID,$EVENT_PAR_CUSTOM,$i + 1,$i)
            inc($i)
        end while
        $i := 0
        while ($i <= 1000)
            $sum := $sum + get_event_par_arr($EVENT_ID,$EVENT_PAR_MOD_VALUE_ID,$i)
            inc($i)
        end while
        $i := 0
        while ($i < 16)
            $custom_sum := $custom_sum + get_event_par_arr($EVENT_ID,$EVENT_PAR_CUSTOM,$i)
            inc($i)
        end while
        end on",
    );
    support::without_heap(|| {
        note(&mut rt);
    });
    assert_eq!([cell(&rt, 0, 1), cell(&rt, 0, 2)], [500500, 136]);
}

#[test]
fn generated_event_pending_group_state_is_readable() {
    let mut rt = runtime(
        "on init declare $id declare $before declare $after end on
        on note
        $id := play_note(60,127,0,100000)
        $before := get_event_par_arr($id,$EVENT_PAR_ALLOW_GROUP,0)
        set_event_par_arr($id,$EVENT_PAR_ALLOW_GROUP,0,0)
        $after := get_event_par_arr($id,$EVENT_PAR_ALLOW_GROUP,0)
        end on",
    );
    note(&mut rt);
    assert_eq!([cell(&rt, 0, 1), cell(&rt, 0, 2)], [1, 0]);
}
#[test]
fn affected_group_search_uses_dynamic_length() {
    let mut rt = runtime(
        "on init declare $a declare $b end on on note
        disallow_group($ALL_GROUPS)
        $a := search(%GROUPS_AFFECTED,0)
        $b := search(%GROUPS_AFFECTED,99)
        end on",
    );
    support::without_heap(|| {
        note(&mut rt);
    });
    assert_eq!([cell(&rt, 0, 0), cell(&rt, 0, 1)], [0, -1]);
}
