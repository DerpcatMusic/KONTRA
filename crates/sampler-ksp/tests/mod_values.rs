use sampler_core::{
    Envelope, Input, Limits, ModProgram, ModRoute, ModSource, ModTarget, Pcm, Playback, Prepared,
    Protocol, Region, Runtime, ScriptInstanceId,
};

/// `set_event_par_arr($EVENT_PAR_MOD_VALUE_ID)` feeds the event's "from
/// script" modulator: 500000 at id 1 halves a Script(1) -> volume route.
#[test]
fn from_script_modulator_values_drive_voice_modulation() {
    let script = sampler_ksp::compile(
        "on init declare $read end on
         on note
           set_event_par_arr($EVENT_ID, $EVENT_PAR_MOD_VALUE_ID, 500000, 1)
           $read := get_event_par_arr($EVENT_ID, $EVENT_PAR_MOD_VALUE_ID, 1)
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
    let prepared = Prepared::new(48000, pcm, vec![region], 128)
        .unwrap()
        .with_voice_modulation(
            vec![ModProgram {
                controls: vec![],
                breakpoints: vec![],
                sources: vec![ModSource::Script(1)],
                routes: vec![ModRoute::new(0, ModTarget::Attenuate, 1.)],
                shapes: vec![],
            }],
            vec![Some(0)],
            vec![0],
        )
        .unwrap();
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
    let mut audio = [[0.; 2]; 256];
    rt.render(&mut audio).unwrap();
    let plan = rt.active_plan();
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 0), Ok(500000));
    // Written in the note callback before the voice starts: no ramp from 0.
    assert!(
        audio.iter().all(|f| (f[0] - 0.5).abs() < 1e-6),
        "{:?}",
        &audio[..4]
    );
}
