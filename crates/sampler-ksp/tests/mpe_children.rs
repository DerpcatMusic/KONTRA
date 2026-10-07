use sampler_core::*;
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;

fn runtime(source: Option<&str>) -> Runtime {
    let (rate, asset_rate, playback) = (48000, 48000, Playback::default());
    let pcm = (0..192)
        .map(|i| {
            [
                (i as f32 * 0.23).sin() * 0.25,
                (i as f32 * 0.17).cos() * 0.25,
            ]
        })
        .collect::<Vec<_>>();
    let mut plan = Prepared::new(
        rate,
        vec![Pcm::new(asset_rate, pcm.into_boxed_slice()).unwrap()],
        vec![Region {
            sample: 0,
            key_low: 60,
            key_high: 60,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope: Envelope::default(),
            playback,
        }],
        1,
    )
    .unwrap();
    if let Some(source) = source {
        plan = sampler_ksp::compile(
            source,
            rate,
            sampler_ksp::Limits {
                source_bytes: 4096,
                instructions: 128,
                variables: 2,
                array_cells: 0,
            },
            &[],
        )
        .unwrap()
        .bind(plan)
        .unwrap();
    }
    let cells = plan.behavior_local_count();
    Runtime::new(
        plan,
        Limits {
            notes: 2,
            channels: 0,
            performances: 1,
            families: 1,
            voices: 1,
            expressions: 2,
            decisions: 0,
            commands: 2,
            behaviors: 1,
            behavior_cells: cells,
            behavior_fuel: 128,
            note_cells: 0,
        },
    )
    .unwrap()
}

fn input() -> Input {
    Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(1),
    }
}

/// A note a script plays with `play_note` carries its parent's per-note
/// expression (an MPE bend or pressure on the member channel): with the
/// parent muted, the child sounds exactly like the parent played directly.
#[test]
#[ignore = "play_note children are Inheritance::Independent by design (compile.rs asserts a parent's gain is not inherited); inheriting pitch/pressure/timbre only needs a core policy"]
fn script_played_notes_follow_the_parents_expression() {
    let source = "on note ignore_event($EVENT_ID)
                  play_note(60, 127, 0, 0) end on";
    for expression in [
        Expression {
            pitch_semitones: 12.0,
            ..Expression::default()
        },
        Expression {
            pressure: u32::MAX,
            ..Expression::default()
        },
    ] {
        let mut reference = runtime(None);
        reference
            .trigger_with_expression(input(), 60, 1., expression)
            .unwrap();
        let mut expected = [[0.; 2]; 96];
        reference.render(&mut expected).unwrap();
        let mut rt = runtime(Some(source));
        rt.trigger_with_expression(input(), 60, 1., expression)
            .unwrap();
        let mut audio = [[0.; 2]; 96];
        rt.render(&mut audio).unwrap();
        assert_eq!(audio, expected, "{expression:?}");
    }
}
