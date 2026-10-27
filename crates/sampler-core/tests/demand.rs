use sampler_core::*;
mod support;

fn input() -> Input {
    Input {
        protocol: Protocol::Clap,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(1),
    }
}
fn runtime(frames: usize) -> Runtime {
    Runtime::new(
        Prepared::new(
            48000,
            vec![Pcm::new(48000, (0..frames).map(|i| [i as f32; 2]).collect()).unwrap()],
            vec![],
            0,
        )
        .unwrap(),
        Limits {
            notes: 2,
            channels: 0,
            performances: 1,
            families: 2,
            voices: 2,
            expressions: 2,
            decisions: 0,
            commands: 4,
            behaviors: 0,
            behavior_cells: 0,
            behavior_fuel: 0,
            note_cells: 0,
        },
    )
    .unwrap()
}
fn start(rt: &mut Runtime, at: u64, envelope: Envelope, playback: Playback) -> (NoteId, VoiceId) {
    let note = rt.note_on(input(), 60, 1.).unwrap();
    let family = rt.create_family(note).unwrap();
    let voice = rt
        .start_family(family, 0, at, 1., envelope, playback)
        .unwrap();
    rt.finish_family(family).unwrap();
    (note, voice)
}

#[test]
fn demand_keeps_source_direction_deadlines_and_backpressure_without_advancing_audio() {
    let mut rt = runtime(16);
    support::without_heap(|| {
        let (_, voice) = start(
            &mut rt,
            0,
            Envelope::default(),
            Playback {
                start: 2,
                end: Some(6),
                direction: Direction::Reverse,
                ..Playback::default()
            },
        );
        let mut count = 0;
        assert!(
            rt.visit_voice_demand(voice, 100, |demand| {
                assert_eq!(demand.frames, (5 - count)..(6 - count));
                assert_eq!(demand.deadline, count as u64);
                count += 1;
                true
            })
            .unwrap()
        );
        assert_eq!(count, 4);
        assert!(!rt.visit_voice_demand(voice, 100, |_| false).unwrap());
        assert_eq!(rt.now(), 0);
        let mut audio = [[0.; 2]; 2];
        rt.render(&mut audio).unwrap();
        assert_eq!(audio, [[5.; 2], [4.; 2]]);
        let mut count = 0;
        rt.visit_voice_demand(voice, 4, |demand| {
            assert_eq!(demand.frames, (3 - count)..(4 - count));
            assert_eq!(demand.deadline, 2 + count as u64);
            count += 1;
            true
        })
        .unwrap();
        assert_eq!(count, 2);
        rt.render(&mut audio).unwrap();
        assert_eq!(audio, [[3.; 2], [2.; 2]]);
        assert_eq!(
            rt.visit_voice_demand(voice, 1, |_| true),
            Err(Error::StaleHandle)
        );
    });
}

#[test]
fn pending_onsets_and_envelope_completion_bound_demand_even_when_muted() {
    let mut rt = runtime(16);
    support::without_heap(|| {
        let (note, voice) = start(&mut rt, 3, Envelope::one_shot(0, 2, 0), Playback::default());
        rt.set_expression(
            rt.expression_id(note).unwrap(),
            Expression {
                gain: 0.,
                ..Expression::default()
            },
        )
        .unwrap();
        assert!(
            rt.visit_voice_demand(voice, 3, |_| panic!("exclusive horizon"))
                .unwrap()
        );
        let mut count = 0;
        assert!(
            rt.visit_voice_demand(voice, 100, |demand| {
                assert_eq!(demand.frames, count..count + 1);
                assert_eq!(demand.deadline, 3 + count as u64);
                count += 1;
                true
            })
            .unwrap()
        );
        assert_eq!(count, 2);
        let mut audio = [[1.; 2]; 8];
        rt.render(&mut audio).unwrap();
        assert_eq!(audio, [[0.; 2]; 8]);
        assert_eq!(rt.voice_count(), 0);
    });
}

#[test]
fn fractional_and_high_rate_demand_includes_the_actual_two_sided_kernel() {
    for (transpose, radius) in [(-12., 48usize), (12., 96), (48., 768)] {
        for direction in [Direction::Forward, Direction::Reverse] {
            let mut rt = runtime(2048);
            support::without_heap(|| {
                let (_, voice) = start(
                    &mut rt,
                    0,
                    Envelope::default(),
                    Playback {
                        direction,
                        transpose_semitones: transpose,
                        ..Playback::default()
                    },
                );
                let mut count = 0;
                rt.visit_voice_demand(voice, 1, |demand| {
                    let expected = if direction == Direction::Forward {
                        0..radius + 1
                    } else {
                        2047 - radius..2048
                    };
                    assert_eq!(demand.frames, expected);
                    assert_eq!(demand.deadline, 0);
                    count += 1;
                    true
                })
                .unwrap();
                assert_eq!(count, 1);
            });
        }
    }
}

#[test]
fn crossfade_demands_both_legs_and_release_changes_only_future_topology() {
    for direction in [Direction::Forward, Direction::Reverse] {
        let mut rt = runtime(12);
        support::without_heap(|| {
            let (note, voice) = start(
                &mut rt,
                0,
                Envelope::new(0, 0, 0, 1., 32).unwrap(),
                Playback {
                    direction,
                    loop_range: Some(Loop {
                        start: 4,
                        end: 8,
                        passes: None,
                        mode: LoopMode::UntilRelease,
                        shape: LoopShape::Crossfade { frames: 4 },
                    }),
                    ..Playback::default()
                },
            );
            // Both directions enter the crossfade at traversal offset four.
            rt.render(&mut [[0.; 2]; 5]).unwrap();
            let mut ranges = [None, None];
            let mut count = 0;
            rt.visit_voice_demand(voice, 1, |demand| {
                ranges[count] = Some(demand.frames);
                count += 1;
                true
            })
            .unwrap();
            let expected = if direction == Direction::Forward {
                [Some(5..6), Some(1..2)]
            } else {
                [Some(6..7), Some(10..11)]
            };
            assert_eq!(ranges, expected);
            rt.key_up(note, None).unwrap();
            rt.render(&mut [[0.; 2]; 7]).unwrap();
            let mut count = 0;
            rt.visit_voice_demand(voice, 1, |demand| {
                assert_eq!(
                    demand.frames,
                    if direction == Direction::Forward {
                        8..9
                    } else {
                        3..4
                    }
                );
                count += 1;
                true
            })
            .unwrap();
            assert_eq!(count, 1);
        });
    }
}
