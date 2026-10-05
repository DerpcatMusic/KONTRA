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
        vec![Pcm {
            rate: 48000,
            frames: (0..8).map(|i| [i as f32, -(i as f32)]).collect(),
        }],
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
            families: 4,
            expressions: 4,
            voices: 4,
            commands: 4,
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
                start: 2,
                end: 5,
                mode: LoopMode::Continuous,
            }),
            [1., 2., 3., 4., 2., 3., 4., 2., 3., 4.],
        ),
        (
            Direction::Reverse,
            Some(Loop {
                start: 2,
                end: 5,
                mode: LoopMode::Continuous,
            }),
            [6., 5., 4., 3., 2., 4., 3., 2., 4., 3.],
        ),
        (
            Direction::Forward,
            Some(Loop {
                start: 3,
                end: 4,
                mode: LoopMode::Continuous,
            }),
            [1., 2., 3., 3., 3., 3., 3., 3., 3., 3.],
        ),
        (
            Direction::Reverse,
            Some(Loop {
                start: 3,
                end: 4,
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
                    start: 2,
                    end: 5,
                    mode: LoopMode::UntilRelease,
                }),
                ..Playback::default()
            });
            let n = rt.trigger(input(1), 60, 1.0).unwrap();
            rt.schedule_event(at, Event::KeyUp(n)).unwrap();
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
            start: 2,
            end: 5,
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
            families: 2,
            expressions: 2,
            voices: 4,
            commands: 4,
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
                start: 4,
                end: 4,
                mode: LoopMode::Continuous,
            }),
            ..Playback::default()
        },
        Playback {
            start: 3,
            loop_range: Some(Loop {
                start: 2,
                end: 5,
                mode: LoopMode::Continuous,
            }),
            ..Playback::default()
        },
        Playback {
            end: Some(5),
            loop_range: Some(Loop {
                start: 2,
                end: 6,
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
