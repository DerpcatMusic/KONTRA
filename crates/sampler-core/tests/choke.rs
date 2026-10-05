use sampler_core::{
    Envelope, Error, Event, Expression, Input, Limits, Loop, LoopMode, Pcm, Playback, Prepared,
    Protocol, Runtime,
};
mod support;

fn input() -> Input {
    Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: None,
    }
}

fn runtime(frames: usize, commands: usize) -> Runtime {
    Runtime::new(
        Prepared::new(
            48000,
            vec![Pcm::new(48000, vec![[1.; 2]; frames].into_boxed_slice()).unwrap()],
            vec![],
            0,
        )
        .unwrap(),
        Limits {
            notes: 2,
            channels: 1,
            performances: 1,
            expressions: 2,
            families: 2,
            decisions: 0,
            voices: 4,
            commands,
            behaviors: 0,
            behavior_fuel: 0,
            behavior_cells: 0,
            note_cells: 0,
        },
    )
    .unwrap()
}

#[test]
fn family_choke_captures_each_layer_and_preserves_siblings_at_every_partition() {
    for blocks in [&[1][..], &[16], &[4, 0, 2, 7, 3], &[3, 5, 2]] {
        let mut rt = runtime(64, 4);
        support::without_heap(|| {
            let note = rt.note_on(input(), 60, 1.).unwrap();
            let family = rt.create_family(note).unwrap();
            for (attack, gain) in [(4, 0.25), (8, 0.5)] {
                rt.start_family(
                    family,
                    0,
                    0,
                    gain,
                    Envelope::new(attack, 0, 0, 1., 32).unwrap(),
                    Playback::default(),
                )
                .unwrap();
            }
            rt.start_family(family, 0, 12, 1., Envelope::default(), Playback::default())
                .unwrap();
            let sibling = rt.create_family(note).unwrap();
            rt.start_family(
                sibling,
                0,
                0,
                0.125,
                Envelope::default(),
                Playback::default(),
            )
            .unwrap();
            rt.schedule_event(4, Event::ChokeFamily(family, 4)).unwrap();
            rt.schedule_event(6, Event::ChokeFamily(family, 100))
                .unwrap();
            let mut audio = [[0.; 2]; 16];
            let mut at = 0;
            let mut block = 0;
            while at < audio.len() {
                let count = blocks[block % blocks.len()].min(audio.len() - at);
                rt.render(&mut audio[at..at + count]).unwrap();
                at += count;
                block += 1;
            }
            assert_eq!(
                audio[..8],
                [0.125, 0.25, 0.375, 0.5, 0.625, 0.5, 0.375, 0.25].map(|v| [v; 2])
            );
            assert_eq!(audio[8..], [[0.125; 2]; 8]);
            assert!(audio.iter().all(|f| f[0] == f[1]));
            assert_eq!(rt.family_note(family), Err(Error::StaleHandle));
            assert_eq!(rt.family_voice_count(sibling), Ok(1));
            assert_eq!(rt.pending_commands(), 0);
            assert!(rt.note(note).unwrap().2 && rt.key_down(note).unwrap());
            rt.stop_family(sibling).unwrap();
            rt.flush_ended(|_| panic!("choking sources must not release their physical key"));
            rt.note_off(input(), None).unwrap();
            rt.flush_ended(|_| false);
            assert_eq!(rt.note_count(), 1);
            rt.flush_ended(|_| true);
            assert_eq!((rt.note_count(), rt.expression_count()), (0, 0));
        });
    }
}

#[test]
fn choke_shortens_a_release_without_restarting_or_extending_it() {
    let mut rt = runtime(64, 4);
    support::without_heap(|| {
        let note = rt.note_on(input(), 60, 1.).unwrap();
        let family = rt.create_family(note).unwrap();
        rt.start_family(
            family,
            0,
            0,
            1.,
            Envelope::new(0, 0, 0, 1., 8).unwrap(),
            Playback::default(),
        )
        .unwrap();
        rt.schedule_event(2, Event::Release(note)).unwrap();
        rt.schedule_event(4, Event::ChokeFamily(family, 2)).unwrap();
        rt.schedule_event(5, Event::ChokeFamily(family, u32::MAX))
            .unwrap();
        let mut audio = [[0.; 2]; 8];
        rt.render(&mut audio[..4]).unwrap();
        rt.flush_ended(|_| panic!("the release tail still retains its note"));
        rt.render(&mut []).unwrap(); // Exclusive-end choke executes at the next boundary.
        assert_eq!(rt.family_voice_count(family), Ok(1));
        assert_eq!(
            rt.start_family(
                family,
                0,
                rt.now(),
                1.,
                Envelope::default(),
                Playback::default()
            ),
            Err(Error::ClosedFamily)
        );
        rt.release(note).unwrap(); // Ordinary gate cleanup cannot restart a choke.
        rt.render(&mut audio[4..]).unwrap();
        assert_eq!(
            audio.map(|f| f[0]),
            [1., 1., 1., 0.875, 0.75, 0.375, 0., 0.]
        );
        assert_eq!(
            (rt.voice_count(), rt.family_count(), rt.pending_commands()),
            (0, 0, 0)
        );
        rt.flush_ended(|_| true);
        assert_eq!((rt.note_count(), rt.expression_count()), (0, 0));
    });
}

#[test]
fn choke_preserves_loop_phase_and_muted_tail_duration() {
    for gain in [0., 1.] {
        let plan = Prepared::new(
            48000,
            vec![
                Pcm::new(
                    48000,
                    Box::from([[0.; 2], [1.; 2], [2.; 2], [3.; 2], [4.; 2]]),
                )
                .unwrap(),
            ],
            vec![],
            0,
        )
        .unwrap();
        let mut rt = Runtime::new(
            plan,
            Limits {
                notes: 1,
                channels: 0,
                performances: 1,
                expressions: 1,
                families: 1,
                decisions: 0,
                voices: 1,
                commands: 1,
                behaviors: 0,
                behavior_fuel: 0,
                behavior_cells: 0,
                note_cells: 0,
            },
        )
        .unwrap();
        support::without_heap(|| {
            let note = rt.note_on(input(), 60, 1.).unwrap();
            let family = rt.create_family(note).unwrap();
            rt.start_family(
                family,
                0,
                0,
                gain,
                Envelope::default(),
                Playback {
                    loop_range: Some(Loop {
                        start: 0,
                        end: 5,
                        mode: LoopMode::UntilRelease,
                    }),
                    ..Playback::default()
                },
            )
            .unwrap();
            rt.schedule_event(3, Event::ChokeFamily(family, 4)).unwrap();
            let mut audio = [[0.; 2]; 8];
            rt.render(&mut audio[..6]).unwrap();
            assert_eq!(rt.voice_count(), 1);
            rt.render(&mut audio[6..]).unwrap();
            assert_eq!(
                audio,
                [0., 1., 2., 3., 3., 0., 0.25, 0.].map(|v| [v * gain; 2])
            );
            assert_eq!((rt.voice_count(), rt.family_count()), (0, 0));
            rt.note_off(input(), None).unwrap();
            rt.flush_ended(|_| true);
            assert_eq!(rt.note_count(), 0);
        });
    }
}

#[test]
fn natural_completion_cancels_scheduled_choke_without_pinning_or_retargeting() {
    let mut rt = runtime(2, 1);
    support::without_heap(|| {
        let note = rt.note_on(input(), 60, 1.).unwrap();
        let old = rt.create_family(note).unwrap();
        rt.start_family(
            old,
            0,
            0,
            1.,
            Envelope::new(0, 0, 0, 1., 32).unwrap(),
            Playback::default(),
        )
        .unwrap();
        rt.finish_family(old).unwrap();
        rt.schedule_event(8, Event::ChokeFamily(old, 0)).unwrap();
        rt.note_off(input(), None).unwrap();
        rt.render(&mut [[0.; 2]; 2]).unwrap();
        assert_eq!(rt.family_count(), 0);
        rt.flush_ended(|_| true);
        assert_eq!((rt.note_count(), rt.pending_commands()), (0, 0)); // EOF frees the scheduled choke.
        let note = rt.note_on(input(), 60, 1.).unwrap();
        let replacement = rt.create_family(note).unwrap();
        rt.start_family(
            replacement,
            0,
            2,
            1.,
            Envelope::default(),
            Playback {
                loop_range: Some(Loop {
                    start: 0,
                    end: 2,
                    mode: LoopMode::Continuous,
                }),
                ..Playback::default()
            },
        )
        .unwrap();
        rt.finish_family(replacement).unwrap();
        assert_eq!(rt.choke_family(old, 0), Err(Error::StaleHandle));
        let mut audio = [[0.; 2]; 12];
        rt.render(&mut audio).unwrap();
        assert_eq!(audio, [[1.; 2]; 12]);
        assert_eq!(rt.family_voice_count(replacement), Ok(1));
        assert_eq!(rt.pending_commands(), 0);
        rt.choke_family(replacement, 0).unwrap();
        assert_eq!(rt.family_count(), 0);
        rt.note_off(input(), None).unwrap();
        rt.flush_ended(|_| true);
        assert_eq!(rt.note_count(), 0);
    });
}

#[test]
fn equal_time_source_start_and_choke_obey_submission_order() {
    for choke_first in [false, true] {
        let mut rt = runtime(64, 2);
        support::without_heap(|| {
            let note = rt.note_on(input(), 60, 1.).unwrap();
            let family = rt.create_family(note).unwrap();
            if choke_first {
                rt.schedule_event(4, Event::ChokeFamily(family, 4)).unwrap();
            }
            rt.start_family(family, 0, 4, 1., Envelope::default(), Playback::default())
                .unwrap();
            if !choke_first {
                rt.schedule_event(4, Event::ChokeFamily(family, 4)).unwrap();
            }
            let mut audio = [[0.; 2]; 8];
            rt.render(&mut audio[..4]).unwrap();
            assert_eq!((rt.voice_count(), rt.pending_commands()), (1, 2));
            rt.render(&mut audio[4..]).unwrap();
            let expected = if choke_first {
                [0.; 8]
            } else {
                [0., 0., 0., 0., 1., 0.75, 0.5, 0.25]
            };
            assert_eq!(audio, expected.map(|v| [v; 2]));
            assert_eq!(
                (rt.family_count(), rt.voice_count(), rt.pending_commands()),
                (0, 0, 0)
            );
            assert!(rt.note(note).unwrap().2);
            rt.note_off(input(), None).unwrap();
            rt.flush_ended(|_| true);
            assert_eq!(rt.note_count(), 0);
        });
    }
}

#[test]
fn immediate_choke_needs_no_queue_capacity_and_failed_scheduling_is_atomic() {
    let mut rt = runtime(64, 1);
    let mut foreign = runtime(64, 1);
    let other_note = foreign.note_on(input(), 60, 1.).unwrap();
    let other_family = foreign.create_family(other_note).unwrap();
    support::without_heap(|| {
        let note = rt.note_on(input(), 60, 1.).unwrap();
        let family = rt.create_family(note).unwrap();
        rt.start_family(family, 0, 0, 1., Envelope::default(), Playback::default())
            .unwrap();
        rt.schedule_event(100, Event::Expression(note, Expression::default()))
            .unwrap();
        assert_eq!(
            rt.schedule_event(50, Event::ChokeFamily(family, 0)),
            Err(Error::Capacity)
        );
        assert_eq!(rt.choke_family(other_family, 0), Err(Error::StaleHandle));
        // Failed scheduling did not seal the family or stop its admitted source.
        rt.start_family(family, 0, 0, 0.5, Envelope::default(), Playback::default())
            .unwrap();
        rt.choke_family(family, 0).unwrap();
        assert_eq!(
            (rt.voice_count(), rt.family_count(), rt.pending_commands()),
            (0, 0, 1)
        );
        assert!(rt.key_down(note).unwrap());
        let empty = rt.create_family(note).unwrap();
        rt.choke_family(empty, u32::MAX).unwrap();
        assert_eq!(rt.family_count(), 0);
        rt.note_off(input(), None).unwrap();
        rt.flush_ended(|_| true);
        assert_eq!((rt.note_count(), rt.pending_commands()), (0, 0));
    });
}
