use sampler_core::{
    Envelope, Event, Input, Outcome, Pcm, Playback, Prepared, Protocol, Region, Runtime,
};
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;

fn runtime(source: &str, rate: u32) -> Runtime {
    let script = sampler_ksp::compile(
        source,
        rate,
        sampler_ksp::Limits {
            source_bytes: 4096,
            instructions: 128,
            variables: 4,
        },
        &[],
    )
    .unwrap();
    let prepared = script
        .bind(
            Prepared::new(
                rate,
                vec![Pcm::new(rate, Box::from([[1.; 2]; 256])).unwrap()],
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
    let cells = prepared.behavior_local_count() * 2;
    Runtime::new(
        prepared,
        sampler_core::Limits {
            notes: 4,
            channels: 0,
            performances: 1,
            families: 4,
            expressions: 4,
            voices: 4,
            decisions: 0,
            commands: 8,
            behaviors: 2,
            behavior_fuel: 64,
            behavior_cells: cells,
            note_cells: 4,
        },
    )
    .unwrap()
}
fn input(id: i32) -> Input {
    Input {
        protocol: Protocol::Clap,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(id),
    }
}

#[test]
fn expressions_drive_note_pitch_velocity_duration_and_waits_after_key_release() {
    let source = "on init declare polyphonic $delay end on
        on note
            ignore_event($EVENT_ID)
            $delay := 125 * (1 + $EVENT_NOTE - 60)
            wait($delay)
            play_note(60 + ($EVENT_NOTE mod 2), $EVENT_VELOCITY / (1 + 1), 0, $delay)
            wait($delay - $delay)
        end on";
    for (rate, delay) in [(44100, 6usize), (48000, 6), (96000, 12)] {
        for block in [1, 7, 64] {
            let mut rt = runtime(source, rate);
            support::without_heap(|| {
                let a = rt.trigger(input(1), 60, 126. / 127.).unwrap();
                let b = rt.trigger(input(2), 61, 64. / 127.).unwrap();
                rt.schedule_event(1, Event::KeyUp(a, None)).unwrap();
                rt.schedule_event(1, Event::KeyUp(b, None)).unwrap();
                let mut audio = [[0.; 2]; 64];
                for chunk in audio.chunks_mut(block) {
                    rt.render(chunk).unwrap();
                    rt.render(&mut []).unwrap();
                }
                for (frame, value) in audio.iter().enumerate() {
                    // 250 us rounds independently: 12/12/24 frames.
                    let expected = if (delay..2 * delay).contains(&frame) {
                        63_f32 / 127.
                    } else if (2 * delay..4 * delay).contains(&frame) {
                        32_f32 / 127.
                    } else {
                        0.
                    };
                    assert_eq!(
                        *value, [expected; 2],
                        "{rate} Hz, block {block}, frame {frame}"
                    );
                }
                let mut completed = 0;
                rt.flush_behaviors(|_, _, outcome| {
                    assert_eq!(outcome, Outcome::Finished);
                    completed += 1;
                    true
                });
                rt.flush_ended(|_| true);
                assert_eq!(
                    (
                        completed,
                        rt.note_count(),
                        rt.voice_count(),
                        rt.pending_commands()
                    ),
                    (2, 0, 0, 0)
                );
            });
        }
    }
}

#[test]
fn evaluated_invalid_arguments_fault_without_partial_notes_or_timers() {
    for (body, rate, expected) in [
        ("wait(1 - 2)", 48000, sampler_core::Error::InvalidInput),
        (
            "wait(2147483647)",
            u32::MAX,
            sampler_core::Error::ArithmeticOverflow,
        ),
        (
            "play_note($EVENT_NOTE + 128, 127, 0, 1)",
            48000,
            sampler_core::Error::InvalidInput,
        ),
        (
            "play_note(60, $EVENT_VELOCITY - 127, 0, 1)",
            48000,
            sampler_core::Error::InvalidInput,
        ),
        (
            "play_note(60, 128, 0, 1)",
            48000,
            sampler_core::Error::InvalidInput,
        ),
        (
            "play_note(60, 127, 0, 0)",
            48000,
            sampler_core::Error::InvalidInput,
        ),
        (
            "play_note(60, 127, 0, -1)",
            48000,
            sampler_core::Error::InvalidInput,
        ),
    ] {
        let mut rt = runtime(
            &format!("on note ignore_event($EVENT_ID) {body} end on"),
            rate,
        );
        support::without_heap(|| {
            rt.trigger(input(1), 60, 1.).unwrap();
            let mut count = 0;
            rt.flush_behaviors(|_, _, outcome| {
                assert_eq!(outcome, Outcome::Fault(expected), "{body}");
                count += 1;
                true
            });
            rt.flush_ended(|_| true);
            assert_eq!(
                (
                    count,
                    rt.note_count(),
                    rt.voice_count(),
                    rt.pending_commands()
                ),
                (1, 0, 0, 0)
            );
        });
    }
    assert!(
        sampler_ksp::compile(
            "on init declare $EVENT_VELOCITY end on",
            48000,
            sampler_ksp::Limits {
                source_bytes: 4096,
                instructions: 128,
                variables: 4
            },
            &[]
        )
        .is_err()
    );
}

#[test]
fn note_callbacks_forward_the_original_once_at_wait_exit_or_completion() {
    for (body, key, audible) in [
        ("", 60, true),
        ("wait(125) wait(125)", 60, true),
        ("exit ignore_event($EVENT_ID)", 60, true),
        ("wait(0) ignore_event($EVENT_ID)", 60, true),
        ("wait(1) ignore_event($EVENT_ID)", 60, true),
        (
            "if ($EVENT_NOTE = 60) ignore_event($EVENT_ID) end if",
            60,
            false,
        ),
        (
            "if ($EVENT_NOTE = 60) ignore_event($EVENT_ID) end if",
            61,
            true,
        ),
        ("ignore_event($EVENT_ID) wait(125)", 60, false),
    ] {
        for block in [1, 7, 64] {
            let source = format!("on note {body} end on");
            let mut rt = runtime(&source, 48000);
            support::without_heap(|| {
                let velocity = 0.345678912345;
                let note = rt
                    .trigger_with_expression(
                        input(1),
                        key,
                        velocity,
                        sampler_core::Expression {
                            gain: 0.25,
                            ..sampler_core::Expression::default()
                        },
                    )
                    .unwrap();
                assert_eq!(
                    rt.note_count(),
                    1,
                    "{body}: original identity, not a generated child"
                );
                assert_eq!(rt.note(note).unwrap().1, velocity);
                rt.schedule_event(2, Event::KeyUp(note, None)).unwrap();
                let mut audio = [[0.; 2]; 16];
                for chunk in audio.chunks_mut(block) {
                    rt.render(chunk).unwrap();
                    rt.render(&mut []).unwrap();
                }
                for (frame, value) in audio.iter().enumerate() {
                    assert_eq!(
                        *value,
                        [if audible && frame < 2 {
                            velocity as f32 * 0.25
                        } else {
                            0.
                        }; 2],
                        "{body}, {block}, {frame}"
                    );
                }
                let mut completed = 0;
                rt.flush_behaviors(|_, _, outcome| {
                    assert_eq!(outcome, Outcome::Finished, "{body}");
                    completed += 1;
                    true
                });
                rt.flush_ended(|_| true);
                assert_eq!(
                    (
                        completed,
                        rt.note_count(),
                        rt.voice_count(),
                        rt.pending_commands()
                    ),
                    (1, 0, 0, 0)
                );
            });
        }
    }
}
