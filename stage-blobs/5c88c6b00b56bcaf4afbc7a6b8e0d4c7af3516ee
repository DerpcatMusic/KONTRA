use sampler_core::{
    Direction, Envelope, Event, Input, Limits, Loop, LoopMode, Pcm, Playback, Prepared, Protocol,
    Region, Runtime,
};
mod support;

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
fn region(playback: Playback) -> Region {
    Region {
        sample: 0,
        key_low: 60,
        key_high: 60,
        root_key: None,
        velocity_low: 0.0,
        velocity_high: 1.0,
        gain: 1.0,
        envelope: Envelope::new(0, 0, 0, 1.0, 8).unwrap(),
        playback,
    }
}
fn prepare(regions: Vec<Region>) -> Result<Prepared, sampler_core::Error> {
    Prepared::new(
        48000,
        vec![Pcm::new(48000, (0..8).map(|i| [i as f32, -(i as f32)]).collect()).unwrap()],
        regions,
        8,
    )
}
fn runtime(playback: Playback) -> Runtime {
    Runtime::new(
        prepare(vec![region(playback)]).unwrap(),
        Limits {
            notes: 4,
            channels: 1,
            performances: 1,
            families: 4,
            decisions: 0,
            expressions: 4,
            voices: 4,
            commands: 4,
            behaviors: 0,
            behavior_fuel: 0,
            behavior_cells: 0,
            note_cells: 0,
        },
    )
    .unwrap()
}

#[test]
fn forward_reverse_and_loops_match_explicit_sequences_at_every_partition() {
    for (direction, loop_range, expected) in [
        (
            Direction::Forward,
            None,
            [1., 2., 3., 4., 5., 6., 0., 0., 0., 0.],
        ),
        (
            Direction::Reverse,
            None,
            [6., 5., 4., 3., 2., 1., 0., 0., 0., 0.],
        ),
        (
            Direction::Forward,
            Some(Loop {
                passes: None,
                start: 2,
                end: 5,
                shape: sampler_core::LoopShape::Wrap,
                mode: LoopMode::Continuous,
            }),
            [1., 2., 3., 4., 2., 3., 4., 2., 3., 4.],
        ),
        (
            Direction::Reverse,
            Some(Loop {
                passes: None,
                start: 2,
                end: 5,
                shape: sampler_core::LoopShape::Wrap,
                mode: LoopMode::Continuous,
            }),
            [6., 5., 4., 3., 2., 4., 3., 2., 4., 3.],
        ),
        (
            Direction::Forward,
            Some(Loop {
                passes: None,
                start: 3,
                end: 4,
                shape: sampler_core::LoopShape::Wrap,
                mode: LoopMode::Continuous,
            }),
            [1., 2., 3., 3., 3., 3., 3., 3., 3., 3.],
        ),
        (
            Direction::Reverse,
            Some(Loop {
                passes: None,
                start: 3,
                end: 4,
                shape: sampler_core::LoopShape::Wrap,
                mode: LoopMode::Continuous,
            }),
            [6., 5., 4., 3., 3., 3., 3., 3., 3., 3.],
        ),
    ] {
        for partition in 1..=10 {
            let mut rt = runtime(Playback {
                start: 1,
                end: Some(7),
                direction,
                loop_range,
                ..Playback::default()
            });
            rt.trigger(input(1), 60, 1.0).unwrap();
            let mut audio = [[0.0; 2]; 10];
            for block in audio.chunks_mut(partition) {
                rt.render(&mut []).unwrap();
                rt.render(block).unwrap();
            }
            assert_eq!(audio.map(|f| f[0]), expected);
            assert!(audio.iter().all(|f| f[0] == -f[1]));
            assert_eq!(rt.voice_count(), usize::from(loop_range.is_some()));
            assert_eq!(rt.note_count(), 1); // EOF does not invent key-up.
        }
    }
}

#[test]
fn release_at_loop_boundary_exits_without_extra_cycle_in_both_directions() {
    for (direction, at, expected) in [
        (
            Direction::Forward,
            5,
            [0., 1., 2., 3., 4., 5., 5.25, 5.25, 0., 0.],
        ),
        (
            Direction::Reverse,
            6,
            [7., 6., 5., 4., 3., 2., 1., 0., 0., 0.],
        ),
    ] {
        for partition in 1..=10 {
            let mut rt = runtime(Playback {
                direction,
                loop_range: Some(Loop {
                    passes: None,
                    start: 2,
                    end: 5,
                    shape: sampler_core::LoopShape::Wrap,
                    mode: LoopMode::UntilRelease,
                }),
                ..Playback::default()
            });
            let n = rt.trigger(input(1), 60, 1.0).unwrap();
            rt.schedule_event(at, Event::KeyUp(n, None)).unwrap();
            let mut audio = [[0.; 2]; 10];
            for block in audio.chunks_mut(partition) {
                rt.render(block).unwrap();
            }
            assert_eq!(audio.map(|f| f[0]), expected);
            assert_eq!((rt.voice_count(), rt.family_count()), (0, 0));
            rt.flush_ended(|_| true);
            assert_eq!(rt.note_count(), 0);
        }
    }
}

#[test]
fn shared_pcm_views_and_loop_release_are_independent_without_heap_work() {
    let forward = Playback {
        start: 2,
        end: Some(5),
        loop_range: Some(Loop {
            passes: None,
            start: 2,
            end: 5,
            shape: sampler_core::LoopShape::Wrap,
            mode: LoopMode::Continuous,
        }),
        ..Playback::default()
    };
    let reverse = Playback {
        direction: Direction::Reverse,
        ..forward
    };
    let plan = prepare(vec![region(forward), region(reverse)]).unwrap();
    assert_eq!(plan.sample_count(), 1);
    let mut rt = Runtime::new(
        plan,
        Limits {
            notes: 2,
            channels: 1,
            performances: 1,
            families: 2,
            decisions: 0,
            expressions: 2,
            voices: 4,
            commands: 4,
            behaviors: 0,
            behavior_fuel: 0,
            behavior_cells: 0,
            note_cells: 0,
        },
    )
    .unwrap();
    support::without_heap(|| {
        let note = rt.trigger(input(1), 60, 1.0).unwrap();
        let mut held = [[0.; 2]; 7];
        rt.render(&mut held).unwrap();
        assert_eq!(held.map(|f| f[0]), [6.; 7]);
        rt.release(note).unwrap();
        rt.flush_ended(|_| panic!("looping release tail still owns the note"));
        let mut tail = [[0.; 2]; 10];
        rt.render(&mut tail).unwrap();
        assert_eq!(
            tail.map(|f| f[0]),
            [6., 5.25, 4.5, 3.75, 3., 2.25, 1.5, 0.75, 0., 0.]
        );
        assert_eq!((rt.voice_count(), rt.family_count()), (0, 0));
        rt.flush_ended(|_| true);
        assert_eq!((rt.note_count(), rt.expression_count()), (0, 0));
    });
}

#[test]
fn invalid_views_are_rejected_before_any_admission() {
    let invalid = [
        Playback {
            start: 8,
            ..Playback::default()
        },
        Playback {
            start: 4,
            end: Some(4),
            ..Playback::default()
        },
        Playback {
            end: Some(9),
            ..Playback::default()
        },
        Playback {
            loop_range: Some(Loop {
                passes: None,
                start: 4,
                end: 4,
                shape: sampler_core::LoopShape::Wrap,
                mode: LoopMode::Continuous,
            }),
            ..Playback::default()
        },
        Playback {
            start: 3,
            loop_range: Some(Loop {
                passes: None,
                start: 2,
                end: 5,
                shape: sampler_core::LoopShape::Wrap,
                mode: LoopMode::Continuous,
            }),
            ..Playback::default()
        },
        Playback {
            end: Some(5),
            loop_range: Some(Loop {
                passes: None,
                start: 2,
                end: 6,
                shape: sampler_core::LoopShape::Wrap,
                mode: LoopMode::Continuous,
            }),
            ..Playback::default()
        },
        Playback {
            end: Some(usize::MAX),
            ..Playback::default()
        },
    ];
    let mut rt = runtime(Playback::default());
    let note = rt.note_on(input(1), 60, 1.0).unwrap();
    let family = rt.create_family(note).unwrap();
    for playback in invalid {
        assert!(prepare(vec![region(playback)]).is_err());
        assert_eq!(
            rt.start_family(family, 0, 100, 1.0, Envelope::default(), playback),
            Err(sampler_core::Error::InvalidInput)
        );
        assert_eq!((rt.voice_count(), rt.pending_commands()), (0, 0));
        assert_eq!(rt.family_voice_count(family), Ok(0));
    }
}

#[test]
fn ping_pong_visits_each_endpoint_once_in_both_directions_without_heap() {
    use sampler_core::LoopShape;
    for (direction, start, end, expected) in [
        (
            Direction::Forward,
            2,
            5,
            [1., 2., 3., 4., 3., 2., 3., 4., 3., 2.],
        ),
        (
            Direction::Reverse,
            2,
            5,
            [6., 5., 4., 3., 2., 3., 4., 3., 2., 3.],
        ),
        (
            Direction::Forward,
            2,
            4,
            [1., 2., 3., 2., 3., 2., 3., 2., 3., 2.],
        ),
        (
            Direction::Reverse,
            2,
            4,
            [6., 5., 4., 3., 2., 3., 2., 3., 2., 3.],
        ),
        (
            Direction::Forward,
            3,
            4,
            [1., 2., 3., 3., 3., 3., 3., 3., 3., 3.],
        ),
        (
            Direction::Reverse,
            3,
            4,
            [6., 5., 4., 3., 3., 3., 3., 3., 3., 3.],
        ),
    ] {
        for partition in 1..=10 {
            let mut rt = runtime(Playback {
                start: 1,
                end: Some(7),
                direction,
                loop_range: Some(Loop {
                    passes: None,
                    start,
                    end,
                    mode: LoopMode::Continuous,
                    shape: LoopShape::PingPong,
                }),
                ..Playback::default()
            });
            let mut audio = [[0.; 2]; 10];
            support::without_heap(|| {
                rt.trigger(input(1), 60, 1.).unwrap();
                for chunk in audio.chunks_mut(partition) {
                    rt.render(chunk).unwrap();
                    rt.render(&mut []).unwrap();
                }
                rt.panic();
                rt.flush_ended(|_| true);
                assert_eq!(rt.note_count(), 0);
            });
            assert_eq!(audio.map(|f| f[0]), expected);
            assert!(audio.iter().all(|f| f[0] == -f[1]));
        }
    }
}

#[test]
fn ping_pong_release_finishes_the_return_leg_then_exits_in_the_initial_direction() {
    use sampler_core::LoopShape;
    for (direction, release, expected) in [
        (
            Direction::Forward,
            5,
            [0., 1., 2., 3., 4., 5., 5.25, 5.25, 0., 0., 0., 0.],
        ),
        (
            Direction::Forward,
            6,
            [0., 1., 2., 3., 4., 3., 2., 2.625, 3., 3.125, 3., 2.625],
        ),
        (
            Direction::Reverse,
            6,
            [7., 6., 5., 4., 3., 2., 1., 0., 0., 0., 0., 0.],
        ),
        (
            Direction::Reverse,
            7,
            [7., 6., 5., 4., 3., 2., 3., 4., 2.625, 1.5, 0.625, 0.],
        ),
    ] {
        for partition in 1..=12 {
            let mut rt = runtime(Playback {
                direction,
                loop_range: Some(Loop {
                    passes: None,
                    start: 2,
                    end: 5,
                    mode: LoopMode::UntilRelease,
                    shape: LoopShape::PingPong,
                }),
                ..Playback::default()
            });
            let mut audio = [[0.; 2]; 12];
            support::without_heap(|| {
                let n = rt.trigger(input(1), 60, 1.).unwrap();
                rt.schedule_event(release, Event::KeyUp(n, None)).unwrap();
                for chunk in audio.chunks_mut(partition) {
                    rt.render(chunk).unwrap();
                }
                rt.flush_ended(|_| true);
                assert_eq!((rt.note_count(), rt.voice_count()), (0, 0));
            });
            assert_eq!(
                audio.map(|f| f[0]),
                expected,
                "{direction:?}, release {release}"
            );
        }
    }
}

#[test]
fn counted_loops_finish_their_tail_without_releasing_the_physical_key() {
    use sampler_core::LoopShape;
    for (direction, shape, expected) in [
        (
            Direction::Forward,
            LoopShape::Wrap,
            vec![0., 1., 2., 3., 4., 2., 3., 4., 5., 6., 7.],
        ),
        (
            Direction::Reverse,
            LoopShape::Wrap,
            vec![7., 6., 5., 4., 3., 2., 4., 3., 2., 1., 0.],
        ),
        (
            Direction::Forward,
            LoopShape::PingPong,
            vec![0., 1., 2., 3., 4., 3., 2., 3., 4., 5., 6., 7.],
        ),
        (
            Direction::Reverse,
            LoopShape::PingPong,
            vec![7., 6., 5., 4., 3., 2., 3., 4., 3., 2., 1., 0.],
        ),
    ] {
        for passes in [1, 2] {
            for block in [1, 3, 16] {
                let mut rt = runtime(Playback {
                    direction,
                    loop_range: Some(Loop {
                        start: 2,
                        end: 5,
                        mode: LoopMode::Continuous,
                        shape,
                        passes: std::num::NonZeroU32::new(passes),
                    }),
                    ..Playback::default()
                });
                let mut output = [[0.; 2]; 16];
                support::without_heap(|| {
                    let note = rt.trigger(input(1), 60, 1.).unwrap();
                    for chunk in output.chunks_mut(block) {
                        rt.render(chunk).unwrap();
                    }
                    assert_eq!(rt.voice_count(), 0);
                    rt.flush_ended(|_| panic!("source EOF must not release the physical key"));
                    assert_eq!(rt.key_down(note), Ok(true));
                    rt.key_up(note, None).unwrap();
                    rt.flush_ended(|_| true);
                    assert_eq!(rt.note_count(), 0);
                });
                let one: Vec<_> = if direction == Direction::Forward {
                    (0..8).map(|i| i as f32).collect()
                } else {
                    (0..8).rev().map(|i| i as f32).collect()
                };
                let reference = if passes == 1 { &one } else { &expected };
                for (i, frame) in output.iter().enumerate() {
                    let value = reference.get(i).copied().unwrap_or(0.);
                    assert_eq!(
                        *frame,
                        [value, -value],
                        "{direction:?} {shape:?} count {passes} block {block} at {i}"
                    );
                }
            }
        }
    }
}
