//! W7 failing-first selection/lifecycle contracts, adapted from the ranked KSP audit.
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
