use sampler_core::{
    ChannelScope, ControllerCondition, Envelope, Error, Event, Expression, Inheritance, Input,
    Limits, NotePitch, Pcm, Playback, Prepared, Protocol, Region, ReleaseOptions, Runtime,
    SelectionPolicy, Sequence, SequenceScope, Take, TakePolicy, Trigger,
};
mod support;

fn limits() -> Limits {
    Limits {
        notes: 8,
        performances: 3,
        channels: 0,
        voices: 0,
        families: 0,
        expressions: 8,
        decisions: 0,
        commands: 2,
        behaviors: 0,
        behavior_cells: 0,
        behavior_fuel: 0,
    }
}

fn region(sample: usize) -> Region {
    Region {
        sample,
        key_low: 60,
        key_high: 60,
        root_key: None,
        velocity_low: 0.,
        velocity_high: 1.,
        gain: 1.,
        envelope: Envelope::default(),
        playback: Playback::default(),
    }
}

fn conditioned_plan(policy: SelectionPolicy) -> Prepared {
    let mut regions = Vec::new();
    let mut pcm = Vec::new();
    let mut conditions = Vec::new();
    let mut phases = Vec::new();
    let mut takes = Vec::new();
    let mut sequences = Vec::new();
    for (sequence, trigger) in [Trigger::Attack, Trigger::KeyRelease, Trigger::GateRelease]
        .into_iter()
        .enumerate()
    {
        sequences.push(Sequence {
            takes: 2,
            scope: SequenceScope::Global,
            capacity: 1,
            policy: TakePolicy::Sequential,
        });
        for take in 0..2 {
            for group in 0..4 {
                for mic in 0..2 {
                    let sample = pcm.len();
                    pcm.push(
                        Pcm::new(48000, Box::from([[sample_value(group, take, mic); 2]; 2]))
                            .unwrap(),
                    );
                    regions.push(region(sample));
                    conditions.push(match group {
                        0 => vec![],
                        1 => vec![ControllerCondition {
                            controller: 1,
                            low: 0,
                            high: 0x7fff_ffff,
                        }],
                        2 => vec![ControllerCondition {
                            controller: 1,
                            low: 0x8000_0000,
                            high: u32::MAX,
                        }],
                        _ => vec![
                            ControllerCondition {
                                controller: 2,
                                low: 123,
                                high: 123,
                            },
                            ControllerCondition {
                                controller: 64,
                                low: 0,
                                high: 0,
                            },
                        ],
                    });
                    takes.push(Some(Take {
                        sequence,
                        index: take,
                    }));
                    phases.push(trigger);
                }
            }
        }
    }
    Prepared::new(48000, pcm, regions, 48)
        .unwrap()
        .with_controllers(conditions, 48)
        .unwrap()
        .with_variation(sequences, takes, 3, 0)
        .unwrap()
        .with_releases(phases, ReleaseOptions::default(), ReleaseOptions::default())
        .unwrap()
        .with_release_selection(policy, policy)
}
fn sample_value(group: u32, take: u32, mic: u32) -> f32 {
    (1 + group * 4 + take * 2 + mic) as f32 / 128.
}
fn linear_reference(cc1: u32, cc2: u32, pedal: u32, take: u32) -> f32 {
    let mut sum = 0.;
    for group in 0..4 {
        if match group {
            0 => true,
            1 => cc1 < 0x8000_0000,
            2 => cc1 >= 0x8000_0000,
            _ => cc2 == 123 && pedal == 0,
        } {
            for mic in 0..2 {
                sum += sample_value(group, take, mic);
            }
        }
    }
    sum
}

#[test]
fn conditioned_multimic_selection_matches_independent_reference_at_each_release_phase() {
    for policy in [SelectionPolicy::Onset, SelectionPolicy::Current] {
        let mut budget = limits();
        budget.channels = 1;
        budget.voices = 24;
        budget.families = 3;
        budget.decisions = 3;
        let mut rt = Runtime::new(conditioned_plan(policy), budget).unwrap();
        support::without_heap(|| {
            let domain = rt.performance(0).unwrap();
            let scope = ChannelScope {
                protocol: Protocol::Clap,
                port: 0,
                group: 0,
                channels: 1,
            };
            for gesture in 0..128u32 {
                let take = gesture % 2;
                let onset = if gesture % 3 == 0 {
                    0x7fff_ffff
                } else {
                    0x8000_0000
                };
                let release = onset ^ u32::MAX;
                let cc2 = if gesture % 4 == 0 { 123 } else { 124 };
                rt.set_controller(domain, 1, onset).unwrap();
                rt.set_controller(domain, 2, cc2).unwrap();
                rt.set_pedal_controller(domain, scope, 64, u32::MAX)
                    .unwrap();
                let note = rt.trigger(input(gesture as i32), 60, 1.).unwrap();
                let mut out = [[0.; 2]; 2];
                rt.render(&mut out).unwrap();
                assert_eq!(out, [[linear_reference(onset, cc2, u32::MAX, take); 2]; 2]);
                assert_eq!(rt.voice_count(), 0);
                rt.set_controller(domain, 1, release).unwrap();
                rt.key_up(note, None).unwrap();
                rt.render(&mut out).unwrap();
                let key_cc = if policy == SelectionPolicy::Onset {
                    onset
                } else {
                    release
                };
                assert_eq!(out, [[linear_reference(key_cc, cc2, u32::MAX, take); 2]; 2]);
                rt.set_controller(domain, 2, 123).unwrap();
                // Gate selection must see CC64=0 in the same accepted operation.
                rt.set_pedal_controller(domain, scope, 64, 0).unwrap();
                rt.render(&mut out).unwrap();
                let expected = if policy == SelectionPolicy::Onset {
                    linear_reference(onset, cc2, u32::MAX, take)
                } else {
                    linear_reference(release, 123, 0, take)
                };
                assert_eq!(out, [[expected; 2]; 2]);
                for (sequence, trigger) in
                    [Trigger::Attack, Trigger::KeyRelease, Trigger::GateRelease]
                        .into_iter()
                        .enumerate()
                {
                    assert_eq!(
                        rt.note_take(note, trigger, sequence).unwrap().unwrap(),
                        take
                    );
                }
                assert_eq!(rt.note_controller(note, 1).unwrap(), onset);
                assert_eq!(rt.note_controller(note, 64).unwrap(), u32::MAX);
                rt.flush_ended(|_| true);
                assert_eq!(rt.note_count(), 0);
                assert_eq!(rt.release_reserve().voices, 0);
            }
        });
    }
}

#[test]
fn condition_validation_and_zero_match_do_not_consume_sequence_or_source_ownership() {
    let prepare = || {
        Prepared::new(
            48000,
            vec![Pcm::new(48000, Box::from([[1.; 2]; 2])).unwrap()],
            vec![region(0)],
            1,
        )
        .unwrap()
    };
    let condition = ControllerCondition {
        controller: 1,
        low: 100,
        high: 200,
    };
    assert!(matches!(
        prepare().with_controllers(vec![], 1),
        Err(Error::InvalidInput)
    ));
    assert!(matches!(
        prepare().with_controllers(vec![vec![condition]], 0),
        Err(Error::Capacity)
    ));
    for invalid in [
        ControllerCondition {
            controller: 128,
            ..condition
        },
        ControllerCondition {
            low: 201,
            ..condition
        },
    ] {
        assert!(matches!(
            prepare().with_controllers(vec![vec![invalid]], 1),
            Err(Error::InvalidInput)
        ));
    }
    assert!(matches!(
        prepare().with_controllers(
            vec![vec![
                condition,
                ControllerCondition {
                    low: 201,
                    high: 255,
                    ..condition
                }
            ]],
            2
        ),
        Err(Error::InvalidInput)
    ));
    let plan = prepare()
        .with_controllers(
            vec![vec![
                condition,
                ControllerCondition {
                    low: 150,
                    high: u32::MAX,
                    ..condition
                },
            ]],
            2,
        )
        .unwrap()
        .with_variation(
            vec![Sequence {
                takes: 2,
                scope: SequenceScope::Global,
                capacity: 1,
                policy: TakePolicy::Sequential,
            }],
            vec![Some(Take {
                sequence: 0,
                index: 0,
            })],
            1,
            0,
        )
        .unwrap();
    let mut budget = limits();
    budget.voices = 1;
    budget.families = 1;
    budget.decisions = 1;
    let mut rt = Runtime::new(plan, budget).unwrap();
    support::without_heap(|| {
        let domain = rt.performance(0).unwrap();
        for value in [0, 100, 149, 201, u32::MAX] {
            rt.set_controller(domain, 1, value).unwrap();
            let note = rt.trigger(input(0), 60, 1.).unwrap();
            assert_eq!(rt.voice_count(), 0);
            assert_eq!(rt.note_take(note, Trigger::Attack, 0).unwrap(), None);
            rt.key_up(note, None).unwrap();
            rt.flush_ended(|_| true);
        }
        for (value, take, voices) in [(150, 0, 1), (200, 1, 0), (175, 0, 1)] {
            rt.set_controller(domain, 1, value).unwrap();
            let note = rt.trigger(input(0), 60, 1.).unwrap();
            assert_eq!(rt.voice_count(), voices);
            assert_eq!(
                rt.note_take(note, Trigger::Attack, 0).unwrap().unwrap(),
                take
            );
            rt.render(&mut [[0.; 2]; 2]).unwrap();
            rt.key_up(note, None).unwrap();
            rt.flush_ended(|_| true);
        }
    });
}

#[test]
fn pending_release_pitch_checks_use_known_onset_controllers_and_all_possible_current_values() {
    for policy in [SelectionPolicy::Onset, SelectionPolicy::Current] {
        let mut high = region(0);
        high.playback.transpose_semitones = 48.;
        let plan = Prepared::new(
            48000,
            vec![Pcm::new(48000, Box::from([[0.25; 2]; 2])).unwrap()],
            vec![region(0), high],
            2,
        )
        .unwrap()
        .with_releases(
            vec![Trigger::GateRelease; 2],
            ReleaseOptions::default(),
            ReleaseOptions::default(),
        )
        .unwrap()
        .with_controllers(
            vec![
                vec![ControllerCondition {
                    controller: 1,
                    low: 0,
                    high: 0,
                }],
                vec![ControllerCondition {
                    controller: 1,
                    low: 1,
                    high: 1,
                }],
            ],
            2,
        )
        .unwrap()
        .with_release_selection(policy, policy);
        let mut budget = limits();
        budget.voices = 2;
        budget.families = 1;
        let mut rt = Runtime::new(plan, budget).unwrap();
        support::without_heap(|| {
            let note = rt.trigger(input(0), 60, 1.).unwrap();
            let owner = rt.expression_id(note).unwrap();
            let expression = Expression {
                pitch_semitones: 12.,
                ..Expression::default()
            };
            let expected = if policy == SelectionPolicy::Onset {
                Ok(())
            } else {
                Err(Error::InvalidInput)
            };
            assert_eq!(rt.set_expression(owner, expression), expected);
            assert_eq!(rt.set_expressions(&[(owner, expression)]), expected);
            assert_eq!(
                rt.schedule_event(1, Event::Expression(note, expression)),
                expected
            );
            rt.set_controller(rt.performance(0).unwrap(), 1, 1).unwrap();
            rt.key_up(note, None).unwrap();
            assert_eq!(
                rt.release_status(note, Trigger::GateRelease),
                Ok(sampler_core::ReleaseStatus::Selected)
            );
            rt.panic();
            rt.flush_ended(|_| true);
        });
    }
}

#[test]
fn same_time_controller_and_release_order_is_audible_and_partition_invariant() {
    for controller_first in [false, true] {
        for block in [1, 2, 4, 6] {
            let plan = Prepared::new(
                48000,
                vec![Pcm::new(48000, Box::from([[0.5; 2]])).unwrap()],
                vec![region(0)],
                1,
            )
            .unwrap()
            .with_controllers(
                vec![vec![ControllerCondition {
                    controller: 1,
                    low: 9,
                    high: 9,
                }]],
                1,
            )
            .unwrap()
            .with_releases(
                vec![Trigger::GateRelease],
                ReleaseOptions::default(),
                ReleaseOptions::default(),
            )
            .unwrap()
            .with_release_selection(SelectionPolicy::Onset, SelectionPolicy::Current);
            let mut budget = limits();
            budget.voices = 1;
            budget.families = 1;
            let mut rt = Runtime::new(plan, budget).unwrap();
            support::without_heap(|| {
                let domain = rt.performance(0).unwrap();
                let note = rt.trigger(input(0), 60, 1.).unwrap();
                let update = Event::Controller(domain, 1, 9);
                let release = Event::KeyUp(note, None);
                let events = if controller_first {
                    [update, release]
                } else {
                    [release, update]
                };
                for event in events {
                    rt.schedule_event(2, event).unwrap();
                }
                let mut out = [[0.; 2]; 6];
                for chunk in out.chunks_mut(block) {
                    rt.render(&mut []).unwrap();
                    rt.render(chunk).unwrap();
                }
                let mut expected = [[0.; 2]; 6];
                if controller_first {
                    expected[2] = [0.5; 2];
                }
                assert_eq!(out, expected);
                assert_eq!(rt.controller(domain, 1).unwrap(), 9);
                assert_eq!(rt.note_controller(note, 1).unwrap(), 0);
                rt.flush_ended(|_| true);
                assert_eq!(rt.note_count(), 0);
            });
        }
    }
}
fn runtime() -> Runtime {
    Runtime::new(Prepared::new(48000, vec![], vec![], 0).unwrap(), limits()).unwrap()
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
fn full_note_capacity_and_continuous_changes_keep_snapshots_without_heap_work() {
    let mut rt = runtime();
    support::without_heap(|| {
        for cycle in 0..100u32 {
            let mut notes = [None; 8];
            for (i, slot) in notes.iter_mut().enumerate() {
                let domain = rt.performance(i % 3).unwrap();
                let value = cycle * 1000 + i as u32;
                rt.set_articulation(domain, value).unwrap();
                for cc in 0..128 {
                    rt.set_controller(domain, cc, value + u32::from(cc))
                        .unwrap();
                }
                *slot = Some(
                    rt.note_on_pitched_in(
                        domain,
                        input(i as i32),
                        NotePitch::Key(60),
                        1.,
                        Expression::default(),
                    )
                    .unwrap(),
                );
            }
            // Failed admission must not acquire a version owner.
            assert_eq!(rt.note_on(input(999), 60, 1.), Err(Error::Capacity));
            for step in 0..512 {
                let domain = rt.performance(step % 3).unwrap();
                rt.set_controller(domain, 1, u32::MAX - step as u32)
                    .unwrap();
                rt.set_articulation(domain, u32::MAX).unwrap();
            }
            for (i, note) in notes.into_iter().enumerate() {
                let note = note.unwrap();
                let value = cycle * 1000 + i as u32;
                assert_eq!(rt.note_selection(note).unwrap().articulation, value);
                for cc in 0..128 {
                    assert_eq!(rt.note_controller(note, cc).unwrap(), value + u32::from(cc));
                }
                rt.key_up(note, None).unwrap();
                rt.flush_ended(|_| false);
                assert_eq!(rt.note_controller(note, 1).unwrap(), value + 1);
            }
            rt.flush_ended(|_| true);
            assert_eq!(rt.note_count(), 0);
            assert_eq!(
                rt.note_controller(notes[0].unwrap(), 0),
                Err(Error::StaleHandle)
            );
        }
    });
}

#[test]
fn controller_timeline_preserves_boundaries_precision_and_rejection_atomicity() {
    let mut rt = runtime();
    let other = runtime();
    support::without_heap(|| {
        let domain = rt.performance(1).unwrap();
        let foreign = other.performance(1).unwrap();
        assert_eq!(rt.set_controller(foreign, 1, 5), Err(Error::StaleHandle));
        assert_eq!(rt.set_controller(domain, 128, 5), Err(Error::InvalidInput));
        assert_eq!(rt.controller(domain, 255), Err(Error::InvalidInput));
        rt.schedule_event(2, Event::Controller(domain, 1, 0x12345678))
            .unwrap();
        rt.schedule_event(2, Event::Controller(domain, 1, 0x12345679))
            .unwrap();
        assert_eq!(
            rt.schedule_event(3, Event::Controller(domain, 1, 42)),
            Err(Error::Capacity)
        );
        rt.render(&mut [[0.; 2]; 2]).unwrap();
        assert_eq!(rt.controller(domain, 1).unwrap(), 0);
        rt.render(&mut []).unwrap();
        assert_eq!(rt.controller(domain, 1).unwrap(), 0x12345679);
        let note = rt
            .note_on_pitched_in(
                domain,
                input(0),
                NotePitch::Key(60),
                1.,
                Expression::default(),
            )
            .unwrap();
        assert_eq!(rt.note_controller(note, 1).unwrap(), 0x12345679);
        rt.set_controller(domain, 1, 0x1234567a).unwrap();
        assert_eq!(rt.note_controller(note, 1).unwrap(), 0x12345679);
        assert_eq!(rt.controller(rt.performance(0).unwrap(), 1).unwrap(), 0);
        assert_eq!(
            rt.schedule_event(1, Event::Controller(domain, 1, 42)),
            Err(Error::PastEvent)
        );
        rt.schedule_event(3, Event::Controller(domain, 1, 42))
            .unwrap();
        rt.panic();
        rt.render(&mut [[0.; 2]; 4]).unwrap();
        assert_eq!(rt.controller(domain, 1).unwrap(), 0x1234567a);
        rt.flush_ended(|_| true);
    });
}

#[test]
fn children_capture_current_domain_controllers_independently_of_expression_inheritance() {
    let mut rt = runtime();
    support::without_heap(|| {
        let domain = rt.performance(2).unwrap();
        rt.set_controller(domain, 7, 17).unwrap();
        let parent = rt
            .note_on_pitched_in(
                domain,
                input(0),
                NotePitch::Key(60),
                1.,
                Expression::default(),
            )
            .unwrap();
        for (i, inheritance) in [
            Inheritance::Independent,
            Inheritance::Snapshot,
            Inheritance::Linked,
        ]
        .into_iter()
        .enumerate()
        {
            let value = 100 + i as u32;
            rt.set_controller(domain, 7, value).unwrap();
            let child = rt.child(parent, 60, 1., true, inheritance).unwrap();
            assert_eq!(rt.note_controller(child, 7).unwrap(), value);
            assert_eq!(rt.note_controller(parent, 7).unwrap(), 17);
            rt.pin(child).unwrap();
            rt.release(child).unwrap();
            rt.flush_ended(|_| true);
            assert_eq!(rt.note_controller(child, 7).unwrap(), value);
            rt.unpin(child).unwrap();
        }
        rt.key_up(parent, None).unwrap();
        rt.flush_ended(|_| true);
        assert_eq!(rt.note_count(), 0);
    });
}
