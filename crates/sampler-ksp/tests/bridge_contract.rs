//! Engine-bridge contracts for the note-event commands, straight from the KSP
//! manual (event-commands: change_tune, change_vol, change_pan).
use sampler_core::*;
use sampler_ksp::{Environment, Script};

fn compile(source: &str, environment: &Environment) -> Script {
    sampler_ksp::compile_with(
        source,
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
        environment,
    )
    .unwrap()
}

fn runtime(source: &str) -> Runtime {
    runtime_in(source, &Environment {
        groups: vec!["Group 0".into()],
        ..Environment::default()
    })
}

fn runtime_in(source: &str, environment: &Environment) -> Runtime {
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
            envelope: Envelope::GATE,
            playback: Playback::default(),
        }],
        128,
    )
    .unwrap()
    .with_groups(1, vec![Some(0)])
    .unwrap()
    .with_engine_parameters(vec![], environment.engine_lookups.clone())
    .unwrap();
    let plan = sampler_ksp::bind_modules(vec![compile(source, environment)], prepared).unwrap();
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

fn cell(rt: &Runtime, cell: u32) -> i64 {
    rt.script_cell(rt.active_plan(), ScriptInstanceId(0), cell)
        .unwrap()
}

fn play(source: &str) -> i64 {
    let mut rt = runtime(source);
    rt.trigger(input(1), 60, 1.).unwrap();
    cell(&rt, 0)
}

macro_rules! tune_contract {
    ($name:ident, $source:literal, $expected:expr) => {
        #[test]
        fn $name() {
            assert_eq!(play($source), $expected);
        }
    };
}

// Tune is in millicents (100000 = one semitone), absolute unless mode is 1.
tune_contract!(
    change_tune_absolute_sets_millicents,
    "on init declare $out end on
     on note change_tune($EVENT_ID, 100000, 0)
     $out := get_event_par($EVENT_ID, $EVENT_PAR_TUNE) end on",
    100000
);
tune_contract!(
    change_tune_relative_adds,
    "on init declare $out end on
     on note change_tune($EVENT_ID, 100000, 0)
     change_tune($EVENT_ID, 50000, 1)
     $out := get_event_par($EVENT_ID, $EVENT_PAR_TUNE) end on",
    150000
);
tune_contract!(
    change_tune_relative_negative_from_zero,
    "on init declare $out end on
     on note change_tune($EVENT_ID, -50000, 1)
     $out := get_event_par($EVENT_ID, $EVENT_PAR_TUNE) end on",
    -50000
);
tune_contract!(
    change_tune_mode_two_is_absolute,
    "on init declare $out end on
     on note change_tune($EVENT_ID, 100000, 2)
     change_tune($EVENT_ID, -50000, 2)
     $out := get_event_par($EVENT_ID, $EVENT_PAR_TUNE) end on",
    -50000
);
tune_contract!(
    change_tune_dynamic_mode_selects_add,
    "on init declare $out declare $mode := 1 end on
     on note change_tune($EVENT_ID, 30000, $mode)
     change_tune($EVENT_ID, 30000, $mode)
     $out := get_event_par($EVENT_ID, $EVENT_PAR_TUNE) end on",
    60000
);

#[test]
fn engine_lookups_match_owner_and_target_kind() {
    let environment = Environment {
        groups: vec!["Group 0".into()],
        engine_lookups: vec![
            EngineLookup { group: 0, owner: -1, target: false, name: "ENV_AHDSR".into(), index: 5 },
            EngineLookup { group: 0, owner: 2, target: true, name: "MOD_X".into(), index: 7 },
        ],
        ..Environment::default()
    };
    let mut rt = runtime_in(
        "on init declare $a declare $b declare $c declare $d end on
         on note
           $a := get_mod_idx(0, \"ENV_AHDSR\")
           $b := get_mod_idx(0, \"env_ahdsr\")
           $c := find_mod(0, \"ENV_AHDSR\")
           $d := find_mod(0, \"env_ahdsr\")
         end on",
        &environment,
    );
    rt.trigger(input(1), 60, 1.).unwrap();
    assert_eq!([cell(&rt, 0), cell(&rt, 1), cell(&rt, 2), cell(&rt, 3)], [5, 5, 5, 5]);
    let mut rt = runtime_in(
        "on init declare $a declare $b declare $c declare $d end on
         on note
           $a := get_target_idx(0, 2, \"MOD_X\")
           $b := get_target_idx(0, 3, \"MOD_X\")
           $c := get_mod_idx(0, \"MOD_X\")
           $d := find_target(0, 2, \"mod_x\")
         end on",
        &environment,
    );
    rt.trigger(input(1), 60, 1.).unwrap();
    assert_eq!([cell(&rt, 0), cell(&rt, 1), cell(&rt, 2), cell(&rt, 3)], [7, -1, -1, 7]);
}
