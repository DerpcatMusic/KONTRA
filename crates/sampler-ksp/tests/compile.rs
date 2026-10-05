use sampler_core::{
    Envelope, Event, Expression, Input, Limits as CoreLimits, Outcome, Pcm, Playback, Prepared,
    Protocol, Region, Runtime,
};
use sampler_ksp::{Limits, compile};
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;
fn limits() -> Limits {
    Limits {
        source_bytes: 4096,
        instructions: 32,
        variables: 8,
    }
}

#[test]
fn authored_script_uses_new_native_ownership_after_input_release_without_heap() {
    let source = "{ UTF-8 comment: café } on note ignore_event($EVENT_ID) wait(125) play_note($EVENT_NOTE + 1, 64, 0, 125) end on";
    for (rate, start) in [(44100, 6), (48000, 6), (96000, 12)] {
        for block in 1..=16 {
            let program = compile(source, rate, limits()).unwrap();
            let plan = Prepared::new(
                rate,
                vec![Pcm::new(rate, vec![[1.; 2]; 64].into_boxed_slice()).unwrap()],
                vec![Region {
                    sample: 0,
                    key_low: 61,
                    key_high: 61,
                    root_key: None,
                    velocity_low: 0.,
                    velocity_high: 1.,
                    gain: 1.,
                    envelope: Envelope::default(),
                    playback: Playback::default(),
                }],
                1,
            )
            .unwrap();
            let plan = program.bind(plan).unwrap();
            let mut rt = Runtime::new(
                plan,
                CoreLimits {
                    notes: 4,
                    channels: 1,
                    performances: 1,
                    families: 4,
                    decisions: 0,
                    expressions: 4,
                    voices: 4,
                    commands: 4,
                    behaviors: 1,
                    behavior_fuel: 8,
                    behavior_cells: 0,
                    note_cells: 0,
                },
            )
            .unwrap();
            let input = Input {
                protocol: Protocol::Native,
                port: 7,
                group: 2,
                channel: 3,
                key: 60,
                external_id: Some(-42),
            };
            let mut audio = [[0.; 2]; 48];
            support::without_heap(|| {
                // A fixed generated velocity and independent expression must not
                // accidentally inherit the silent originating event's values.
                let note = rt.note_on(input, 60, 0.).unwrap();
                rt.set_expression(
                    rt.expression_id(note).unwrap(),
                    Expression {
                        gain: 0.,
                        ..Expression::default()
                    },
                )
                .unwrap();
                let callback = rt.start_behavior(note, 0).unwrap();
                rt.schedule_event(2, Event::Release(note)).unwrap();
                for chunk in audio.chunks_mut(block) {
                    rt.render(chunk).unwrap();
                }
                assert_eq!(rt.behavior_outcome(callback), Ok(Some(Outcome::Finished)));
                rt.flush_ended(|_| panic!("completion retains source identity"));
                rt.flush_behaviors(|_, _, _| true);
                let mut ends = 0;
                rt.flush_ended(|ended| {
                    assert_eq!(ended, input);
                    ends += 1;
                    true
                });
                assert_eq!(
                    (
                        ends,
                        rt.note_count(),
                        rt.expression_count(),
                        rt.pending_commands()
                    ),
                    (1, 0, 0, 0)
                );
            });
            for (frame, value) in audio.iter().enumerate() {
                let expected = if (start..start * 2).contains(&frame) {
                    64.0_f32 / 127.
                } else {
                    0.
                };
                assert_eq!(
                    *value, [expected; 2],
                    "rate {rate}, block {block}, frame {frame}"
                );
            }
        }
    }
}

#[test]
fn unsupported_and_malformed_source_fails_explicitly_with_a_valid_offset() {
    let bodies = [
        "",
        "on init end on",
        "on note end on",
        "on release ignore_event($EVENT_ID) end on",
        "on note ignore_event($EVENT_ID) message(1) end on",
        "on note ignore_event($EVENT_ID) wait($x) end on",
        "on note ignore_event($EVENT_ID) wait(-1) end on",
        "on note ignore_event($EVENT_ID) wait(2147483648) end on",
        "on note ignore_event($EVENT_ID) wait(18446744073709551616) end on",
        "on note ignore_event($EVENT_ID) wait(18446744073709551615) end on",
        "on note ignore_event($EVENT_ID) wait(1)",
        "on note ignore_event($EVENT_ID) end on on note end on",
        "on note ignore_event($OTHER_ID) end on",
        "{ unclosed",
        "{ { nested } }",
        "on note ignore_event($EVENT_ID) play_note(60, 100, 0, 1000) end on",
        "on note ignore_event($EVENT_ID) play_note($EVENT_NOTE + 128, 100, 0, 1000) end on",
        "on note ignore_event($EVENT_ID) play_note($EVENT_NOTE, $EVENT_VELOCITY, 0, 1000) end on",
        "on note ignore_event($EVENT_ID) play_note($EVENT_NOTE, 0, 0, 1000) end on",
        "on note ignore_event($EVENT_ID) play_note($EVENT_NOTE, 128, 0, 1000) end on",
        "on note ignore_event($EVENT_ID) play_note($EVENT_NOTE, 127, 1, 1000) end on",
        "on note ignore_event($EVENT_ID) play_note($EVENT_NOTE, 127, 0, 0) end on",
        "on note ignore_event($EVENT_ID) play_note($EVENT_NOTE, 127, 0, -1) end on",
        "on note ignore_event($EVENT_ID) play_note($EVENT_NOTE, 127, 0, 1000 end on",
        "🎹",
    ];
    for source in bodies {
        let Err(error) = compile(source, 48000, limits()) else {
            panic!("accepted unsupported source: {source}");
        };
        assert!(error.offset <= source.len());
        assert!(source.is_char_boundary(error.offset));
        assert!(!error.message.is_empty());
    }
    let source = "on note ignore_event($EVENT_ID) wait($x) end on";
    let error = compile(source, 48000, limits()).err().unwrap();
    assert_eq!(error.offset, source.find("$x").unwrap());
    assert!(error.message.contains("dynamic expressions"));
}

#[test]
fn compilation_obeys_source_instruction_and_clock_budgets_without_panics() {
    let source = "on note ignore_event($EVENT_ID) wait(0) end on";
    assert!(compile(source, 0, limits()).is_err());
    assert!(
        compile(
            "on note ignore_event($EVENT_ID) wait(2147483647) end on",
            48000,
            limits()
        )
        .is_ok()
    );
    assert!(
        compile(
            "on note ignore_event($EVENT_ID) wait(2147483647) end on",
            u32::MAX,
            limits()
        )
        .is_err()
    );
    assert!(
        compile(
            source,
            48000,
            Limits {
                source_bytes: source.len() - 1,
                ..limits()
            }
        )
        .is_err()
    );
    assert!(
        compile(
            source,
            48000,
            Limits {
                instructions: 1,
                ..limits()
            }
        )
        .is_err()
    );
    assert!(
        compile(
            source,
            48000,
            Limits {
                instructions: 2,
                ..limits()
            }
        )
        .is_ok()
    );
    assert!(compile(include_str!("fixtures/delayed-note.ksp"), 48000, limits()).is_ok());
    assert!(
        compile(
            "on note ignore_event($EVENT_ID) play_note($EVENT_NOTE - 1, 1, 0, 1) end on",
            48000,
            limits()
        )
        .is_ok()
    );
    let mut seed = 1u64;
    for len in 0..512 {
        let text: String = (0..len)
            .map(|_| {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                char::from((seed >> 32) as u8)
            })
            .collect();
        let _ = compile(&text, 48000, limits());
    }
}

#[test]
fn polyphonic_source_state_reaches_release_callbacks_without_heap_or_note_aliasing() {
    let source = include_str!("fixtures/polyphonic-release.ksp");
    for (rate, delay) in [(44100, 6), (48000, 6), (96000, 12)] {
        for block in [1, 3, 7, 16] {
            let script = compile(source, rate, limits()).unwrap();
            assert_eq!(script.note_cells(), 3);
            let plan = script
                .bind(
                    Prepared::new(
                        rate,
                        vec![Pcm::new(rate, Box::from([[1.; 2]; 64])).unwrap()],
                        vec![Region {
                            sample: 0,
                            key_low: 60,
                            key_high: 61,
                            root_key: None,
                            velocity_low: 0.,
                            velocity_high: 1.,
                            gain: 1.,
                            envelope: Envelope::default(),
                            playback: Playback::default(),
                        }],
                        2,
                    )
                    .unwrap(),
                )
                .unwrap();
            let mut rt = Runtime::new(
                plan,
                CoreLimits {
                    notes: 4,
                    channels: 1,
                    performances: 1,
                    families: 4,
                    expressions: 4,
                    voices: 4,
                    decisions: 0,
                    commands: 8,
                    behaviors: 4,
                    behavior_fuel: 16,
                    behavior_cells: 4,
                    note_cells: 12,
                },
            )
            .unwrap();
            let input = Input {
                protocol: Protocol::Native,
                port: 0,
                group: 0,
                channel: 0,
                key: 60,
                external_id: None,
            };
            let mut audio = [[0.; 2]; 48];
            support::without_heap(|| {
                // The physical key is deliberately the same; identity is per note,
                // while logical pitch/state differs on these two overlapping inputs.
                let a = rt.trigger(input, 60, 0.).unwrap();
                let b = rt.trigger(input, 61, 0.).unwrap();
                assert_eq!((rt.note_cell(a, 0), rt.note_cell(b, 0)), (Ok(60), Ok(61)));
                assert_eq!(rt.note_cell(a, 2), Ok(i64::from(i32::MIN)));
                let channel = rt.register_channel(input.channel_address()).unwrap();
                rt.sustain(channel, true).unwrap();
                rt.schedule_event(2, Event::KeyUp(a, None)).unwrap();
                rt.schedule_event(4, Event::KeyUp(b, None)).unwrap();
                rt.schedule_event(5, Event::Sustain(channel, false))
                    .unwrap();
                for chunk in audio.chunks_mut(block) {
                    rt.render(chunk).unwrap();
                    rt.render(&mut []).unwrap();
                }
                assert_eq!((rt.note_cell(a, 1), rt.note_cell(b, 1)), (Ok(60), Ok(61)));
                assert_eq!(rt.note_cell(a, 2), Ok(i64::from(i32::MAX)));
                assert_eq!(rt.note_cell(b, 2), Ok(i64::from(i32::MAX)));
                rt.flush_ended(|_| panic!("unaccepted callbacks retain the two inputs"));
                let mut callbacks = 0;
                rt.flush_behaviors(|_, _, outcome| {
                    assert_eq!(outcome, Outcome::Finished);
                    callbacks += 1;
                    true
                });
                assert_eq!(callbacks, 4);
                let mut ends = 0;
                rt.flush_ended(|_| {
                    ends += 1;
                    true
                });
                assert_eq!(
                    (
                        ends,
                        rt.note_count(),
                        rt.voice_count(),
                        rt.pending_commands()
                    ),
                    (2, 0, 0, 0)
                );
            });
            for (frame, actual) in audio.iter().enumerate() {
                let layers = usize::from((2 + delay..2 + 2 * delay).contains(&frame))
                    + usize::from((4 + delay..4 + 2 * delay).contains(&frame));
                assert_eq!(
                    *actual,
                    [layers as f32 * (64. / 127.); 2],
                    "rate {rate}, block {block}, frame {frame}"
                );
            }
        }
    }
}

#[test]
fn polyphonic_declarations_and_callback_tables_reject_invalid_scopes_and_budgets() {
    let init = "on init declare polyphonic $a end on";
    for source in [
        format!("{init} on note ignore_event($EVENT_ID) $a := 2147483648 end on"),
        format!("{init} on release $a := -2147483649 end on"),
        format!("{init} on release $a := 1 + 2 end on"),
        format!("{init} on release $a : = 1 end on"),
        format!("{init} on release $a := $missing end on"),
        format!("{init} on release $missing := 1 end on"),
        format!("{init} on release end on on release end on"),
        format!("{init} on note ignore_event($EVENT_ID) end on {init}"),
        "on init declare polyphonic $a declare polyphonic $a end on on release end on".into(),
        "on init declare polyphonic $NI_bad end on on release end on".into(),
        "on init declare polyphonic $EVENT_ID end on on release end on".into(),
        "on init declare polyphonic $a $a := 1 end on on release end on".into(),
        "on init declare polyphonic $a := 1 end on on release end on".into(),
        "on init declare $a end on on release end on".into(),
        "on init declare polyphonic $ end on on release end on".into(),
    ] {
        let error = compile(&source, 48000, limits())
            .err()
            .expect("invalid source accepted");
        assert!(source.is_char_boundary(error.offset));
    }
    let source = format!("{init} on release $a := -2147483648 end on");
    assert!(
        compile(
            &source,
            48000,
            Limits {
                variables: 0,
                ..limits()
            }
        )
        .is_err()
    );
    assert!(
        compile(
            &source,
            48000,
            Limits {
                variables: 1,
                instructions: 3,
                ..limits()
            }
        )
        .is_ok()
    );
    assert!(
        compile(
            &source,
            48000,
            Limits {
                instructions: 2,
                ..limits()
            }
        )
        .is_err()
    );
    let source = "on release end on on note ignore_event($EVENT_ID) end on";
    assert!(
        compile(
            source,
            48000,
            Limits {
                instructions: 1,
                ..limits()
            }
        )
        .is_err()
    );
    let script = compile(
        source,
        48000,
        Limits {
            instructions: 2,
            ..limits()
        },
    )
    .unwrap();
    assert!(
        script
            .bind(Prepared::new(44100, vec![], vec![], 0).unwrap())
            .is_err()
    );
}
