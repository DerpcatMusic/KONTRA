//! Runtime strings: names built while a callback runs reach group lookup,
//! `find_mod` and `set_text`, in bounded text cells.
use sampler_core::{
    Envelope, Input, Limits, Pcm, Playback, Prepared, Protocol, Region, Runtime, ScriptInstanceId,
};

fn run(source: &str, groups: &[&str]) -> Runtime {
    let environment = sampler_ksp::Environment {
        groups: groups.iter().map(|g| (*g).to_owned()).collect(),
        ..Default::default()
    };
    run_in(source, environment)
}

fn run_in(source: &str, environment: sampler_ksp::Environment) -> Runtime {
    let script = sampler_ksp::compile_with(
        source,
        48000,
        sampler_ksp::Limits {
            source_bytes: 65536,
            instructions: 4096,
            variables: 16,
            array_cells: 16,
        },
        &[],
        &environment,
    )
    .unwrap();
    let pcm = vec![Pcm::new(48000, Box::from([[1.0; 2]; 4800])).unwrap()];
    let region = Region {
        sample: 0,
        key_low: 0,
        key_high: 127,
        root_key: None,
        velocity_low: 0.,
        velocity_high: 1.,
        gain: 1.,
        envelope: Envelope::default(),
        playback: Playback::default(),
    };
    let prepared = Prepared::new(48000, pcm, vec![region], 128).unwrap();
    let note_cells = script.note_cells() * 8;
    let plan = script.bind(prepared).unwrap();
    let behavior_cells = plan.behavior_local_count() * 8;
    let mut rt = Runtime::new(
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
    .unwrap();
    let input = Input {
        protocol: Protocol::Clap,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(60),
    };
    rt.trigger(input, 60, 1.).unwrap();
    rt.render(&mut [[0.; 2]; 64]).unwrap();
    rt
}

fn cell(rt: &Runtime, n: u32) -> i64 {
    rt.script_cell(rt.active_plan(), ScriptInstanceId(0), n)
        .unwrap()
}

#[test]
fn group_names_and_lookups_take_computed_indices_and_names() {
    let rt = run(
        "on init declare $a declare $b declare $c declare $d declare $i declare @n end on
         on note
           $i := 2
           @n := group_name($i)
           $a := find_group(@n)
           $b := find_group(\"gr\" & (($i - 1) mod 3 + 1))
           $c := find_mod(0, \"ENV_\" & \"AHDSR\")
           $d := find_group(\"missing\")
         end on",
        &["gr1", "gr2", "gr3"],
    );
    assert_eq!(cell(&rt, 0), 2);
    assert_eq!(cell(&rt, 1), 1);
    assert_eq!(
        cell(&rt, 2),
        i64::from(sampler_core::name_index("ENV_AHDSR"))
    );
    assert_eq!(cell(&rt, 3), -1);
}

#[test]
fn text_longer_than_a_cell_is_cut_and_counted() {
    let rt = run(
        "on init declare $a declare @n end on
         on note
           @n := \"x\"
           $a := 0
           while ($a < 40)
             @n := @n & \"0123456789\"
             inc($a)
           end while
         end on",
        &[],
    );
    assert!(rt.truncated_texts() > 0);
}

/// `set_controller` in `on init` is the controller's value before any input.
#[test]
fn set_controller_in_init_sets_the_starting_controller_value() {
    let rt = run(
        "on init declare $a set_controller(111, 100) end on
         on note $a := %CC[111] end on",
        &[],
    );
    assert_eq!(cell(&rt, 0), 100);
}

/// Saved persistent arrays come back before the first callback, as Solo's
/// articulation on/off switches do: without them every note was gated off.
#[test]
fn saved_persistent_arrays_are_restored() {
    use sampler_ksp::model::Value;
    let environment = sampler_ksp::Environment {
        persisted_arrays: [(
            "%on".to_owned(),
            [3, 0, 7, 9].map(Value::Int).to_vec(), // one more than declared
        )]
        .into(),
        ..Default::default()
    };
    let rt = run_in(
        "on init declare $a declare %on[3] make_persistent(%on) read_persistent_var(%on) end on
         on note $a := %on[0] + %on[2] * 10 end on",
        environment,
    );
    assert_eq!(cell(&rt, 0), 73);
}

/// Kontakt ignores a script's call on a note that already ended: a gate-linked
/// note played after its parent was released is dropped, not a fault.
#[test]
fn playing_from_a_released_note_is_ignored_not_a_fault() {
    let mut rt = run(
        "on init declare $a end on
         on note
           wait(100000)
           $a := play_note(60, 100, 0, -1)
         end on",
        &[],
    );
    rt.note_off(
        sampler_core::Input {
            protocol: sampler_core::Protocol::Clap,
            port: 0,
            group: 0,
            channel: 0,
            key: 60,
            external_id: Some(60),
        },
        None,
    )
    .unwrap();
    let mut faults = Vec::new();
    for _ in 0..200 {
        rt.render(&mut [[0.; 2]; 64]).unwrap();
        rt.flush_behaviors(|_, _, outcome| {
            if !matches!(
                outcome,
                sampler_core::Outcome::Finished | sampler_core::Outcome::Cancelled
            ) {
                faults.push(format!("{outcome:?}"));
            }
            true
        });
    }
    assert_eq!(faults, Vec::<String>::new());
}
