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
    assert_eq!(cell(&rt, 2), i64::from(sampler_core::name_index("ENV_AHDSR")));
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
