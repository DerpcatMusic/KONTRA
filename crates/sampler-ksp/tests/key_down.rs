use sampler_core::{
    Envelope, Input, Limits, Pcm, Playback, Prepared, Protocol, Region, Runtime, ScriptInstanceId,
};

/// `%KEY_DOWN` reads the held input keys: legato scripts clear their note
/// queue when `search(%KEY_DOWN, 1)` finds nothing, so the sounding note must
/// count.
#[test]
fn key_down_array_reads_and_searches_held_keys() {
    let script = sampler_ksp::compile(
        "on init declare $first declare $held declare $other declare $none end on
         on note
           $first := search(%KEY_DOWN, 1)
           $held := %KEY_DOWN[60]
           $other := %KEY_DOWN[61]
           $none := search(%KEY_DOWN, 1, 61, 127)
         end on",
        48000,
        sampler_ksp::Limits {
            source_bytes: 65536,
            instructions: 4096,
            variables: 16,
            array_cells: 16,
        },
        &[],
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
    let plan = rt.active_plan();
    let cell = |i| rt.script_cell(plan, ScriptInstanceId(0), i);
    assert_eq!(
        [cell(0), cell(1), cell(2), cell(3)],
        [Ok(60), Ok(1), Ok(0), Ok(-1)]
    );
}
