use sampler_core::{
    ChannelAddress, Duration, Envelope, Error, Expression, Inheritance, Input, Instruction, Limits,
    NotePitch, Pcm, Playback, Prepared, Program, Protocol, Region, Runtime, Sequence,
    SequenceScope, Take, TakePolicy, Velocity,
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

fn limits() -> Limits {
    Limits {
        notes: 8,
        channels: 1,
        families: 8,
        expressions: 8,
        voices: 32,
        decisions: 32,
        commands: 8,
        behaviors: 4,
        behavior_fuel: 64,
        behavior_cells: 0,
    }
}

fn region(sample: usize) -> Region {
    Region {
        sample,
        key_low: 59,
        key_high: 62,
        root_key: None,
        velocity_low: 0.,
        velocity_high: 1.,
        gain: 1.,
        envelope: Envelope::default(),
        playback: Playback::default(),
    }
}

fn sequence(scope: SequenceScope, capacity: usize) -> Sequence {
    Sequence {
        takes: 3,
        policy: TakePolicy::Sequential,
        scope,
        capacity,
    }
}

fn plan(scope: SequenceScope, capacity: usize, microphones: usize, frames: usize) -> Prepared {
    policy_plan(scope, capacity, microphones, frames, TakePolicy::Sequential)
}

fn policy_plan(
    scope: SequenceScope,
    capacity: usize,
    microphones: usize,
    frames: usize,
    policy: TakePolicy,
) -> Prepared {
    let pcm: Vec<_> = (0..3)
        .map(|take| {
            Pcm::new(
                48000,
                vec![[(take + 1) as f32 / 8.; 2]; frames].into_boxed_slice(),
            )
            .unwrap()
        })
        .collect();
    let regions: Vec<_> = (0..3)
        .flat_map(|take| std::iter::repeat_n(region(take), microphones))
        .collect();
    let takes = (0..3)
        .flat_map(|take| {
            std::iter::repeat_n(
                Some(Take {
                    sequence: 0,
                    index: take,
                }),
                microphones,
            )
        })
        .collect();
    let candidates = regions.len() * 4;
    Prepared::new(48000, pcm, regions, candidates)
        .unwrap()
        .with_variation(
            vec![Sequence {
                policy,
                ..sequence(scope, capacity)
            }],
            takes,
            capacity,
            capacity * 3,
        )
        .unwrap()
}

#[test]
fn compiled_selection_matches_independent_scoped_multimic_reference() {
    let specs = [
        sequence(SequenceScope::Global, 1),
        sequence(SequenceScope::Key, 4),
        sequence(SequenceScope::Channel, 32),
        sequence(SequenceScope::ChannelKey, 128),
    ];
    let mut pcm = vec![];
    let mut authored = vec![];
    let mut tags = vec![];
    // Interleave sequence/take/microphone authoring order: compiling groups must
    // not advance independently for microphones or depend on original adjacency.
    for take in 0..3 {
        for mic in 0..4 {
            for (seq, _) in specs.iter().enumerate() {
                let value = ((seq + 1) * 8 + take * 4 + mic + 1) as f32 / 64.;
                let mut r = region(pcm.len());
                if seq == 1 {
                    r.velocity_high = 0.5;
                }
                if seq == 2 {
                    r.velocity_low = 0.5;
                }
                if seq == 3 {
                    r.velocity_low = 0.25;
                    r.velocity_high = 0.75;
                }
                pcm.push(Pcm::new(48000, Box::from([[value; 2]; 4])).unwrap());
                authored.push(r);
                tags.push(Some(Take {
                    sequence: seq,
                    index: take as u32,
                }));
            }
        }
    }
    authored.push(region(pcm.len()));
    tags.push(None);
    pcm.push(Pcm::new(48000, Box::from([[0.0625; 2]; 4])).unwrap());
    let prepared = Prepared::new(48000, pcm.clone(), authored.clone(), authored.len() * 4)
        .unwrap()
        .with_variation(specs.to_vec(), tags.clone(), 165, 0)
        .unwrap();
    let mut rt = Runtime::new(prepared, limits()).unwrap();
    let mut reference: Vec<(usize, Option<ChannelAddress>, Option<u8>, u32)> = vec![];
    let mut seed = 991u64;
    for gesture in 0..400 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let source = Input {
            protocol: [
                Protocol::Native,
                Protocol::Midi1,
                Protocol::Midi2,
                Protocol::Clap,
            ][(seed >> 32) as usize % 4],
            port: ((seed >> 36) % 2) as u16,
            group: ((seed >> 37) % 2) as u8,
            channel: ((seed >> 38) % 2) as u8,
            ..input(gesture)
        };
        let key = 59 + ((seed >> 40) % 4) as u8;
        let velocity = [0., 0.25, 0.5, 0.75, 1.][(seed >> 42) as usize % 5];
        let mut expected_takes = [None; 4];
        for (seq, spec) in specs.iter().enumerate() {
            let eligible = authored.iter().zip(&tags).any(|(r, tag)| {
                tag.is_some_and(|t| t.sequence == seq)
                    && r.key_low <= key
                    && key <= r.key_high
                    && r.velocity_low <= velocity
                    && velocity <= r.velocity_high
            });
            if !eligible {
                continue;
            }
            let address = matches!(
                spec.scope,
                SequenceScope::Channel | SequenceScope::ChannelKey
            )
            .then_some(source.channel_address());
            let scoped_key =
                matches!(spec.scope, SequenceScope::Key | SequenceScope::ChannelKey).then_some(key);
            let position = reference
                .iter()
                .position(|(s, a, k, _)| *s == seq && *a == address && *k == scoped_key);
            let take = position.map_or(0, |i| reference[i].3);
            expected_takes[seq] = Some(take);
            let next = (take + 1) % spec.takes;
            if let Some(i) = position {
                reference[i].3 = next;
            } else {
                reference.push((seq, address, scoped_key, next));
            }
        }
        let mut expected = 0.;
        let mut sources = 0;
        for (r, tag) in authored.iter().zip(&tags) {
            if r.key_low <= key
                && key <= r.key_high
                && r.velocity_low <= velocity
                && velocity <= r.velocity_high
                && tag.is_none_or(|t| expected_takes[t.sequence] == Some(t.index))
            {
                expected += pcm[r.sample].frames()[0][0] * velocity as f32;
                sources += 1;
            }
        }
        support::without_heap(|| {
            let note = rt.trigger(source, key, velocity).unwrap();
            for (seq, expected) in expected_takes.iter().enumerate() {
                assert_eq!(
                    rt.note_take(note, sampler_core::Trigger::Attack, seq),
                    Ok(*expected)
                );
            }
            assert_eq!(rt.decision_count(), expected_takes.iter().flatten().count());
            assert_eq!(rt.voice_count(), sources);
            assert_eq!(rt.family_count(), 1 + rt.decision_count());
            let mut seen = [false; 4];
            for family in rt.note_families(note).unwrap() {
                assert_eq!(rt.family_note(family), Ok(note));
                if let Some(take) = rt.family_take(family).unwrap() {
                    assert!(!seen[take.sequence]);
                    seen[take.sequence] = true;
                    assert_eq!(expected_takes[take.sequence], Some(take.index));
                    assert_eq!(rt.family_voice_count(family), Ok(4));
                }
            }
            let mut audio = [[0.; 2]; 4];
            let split = gesture as usize % 5;
            rt.render(&mut audio[..split]).unwrap();
            rt.render(&mut audio[split..]).unwrap();
            assert_eq!(audio, [[expected; 2]; 4]);
            assert_eq!(rt.note_families(note).unwrap().count(), 0);
            for (seq, expected) in expected_takes.iter().enumerate() {
                assert_eq!(
                    rt.note_take(note, sampler_core::Trigger::Attack, seq),
                    Ok(*expected)
                );
            }
            rt.note_off(source, None).unwrap();
            rt.flush_ended(|_| false);
            assert_eq!(rt.decision_count(), expected_takes.iter().flatten().count());
            rt.flush_ended(|_| true);
            assert_eq!((rt.decision_count(), rt.note_count()), (0, 0));
            assert_eq!(
                rt.note_take(note, sampler_core::Trigger::Attack, 0),
                Err(Error::StaleHandle)
            );
        });
    }
}

#[test]
fn failed_admission_does_not_advance_or_publish_partial_mics() {
    let mut rt = Runtime::new(
        plan(SequenceScope::Global, 1, 4, 1),
        Limits {
            voices: 4,
            ..limits()
        },
    )
    .unwrap();
    support::without_heap(|| {
        let blocker = rt.note_on(input(0), 60, 1.).unwrap();
        rt.start(blocker, 0, 0, 1.).unwrap();
        assert_eq!(rt.trigger(input(1), 60, 1.), Err(Error::Capacity));
        assert_eq!(
            (rt.note_count(), rt.voice_count(), rt.decision_count()),
            (1, 1, 0)
        );
        rt.release(blocker).unwrap();
        rt.flush_ended(|_| true);
        let first = rt.trigger(input(1), 60, 1.).unwrap();
        assert_eq!(
            rt.note_take(first, sampler_core::Trigger::Attack, 0),
            Ok(Some(0))
        );
        rt.render(&mut [[0.; 2]; 1]).unwrap();
        assert_eq!(rt.trigger(input(1), 60, 1.), Err(Error::DuplicateInput));
        assert_eq!(
            rt.trigger(
                Input {
                    group: 16,
                    ..input(2)
                },
                60,
                1.
            ),
            Err(Error::InvalidInput)
        );
        assert_eq!(
            rt.trigger_with_expression(
                input(2),
                60,
                1.,
                Expression {
                    pitch_semitones: 100.,
                    ..Expression::default()
                }
            ),
            Err(Error::InvalidInput)
        );
        assert_eq!(
            (rt.note_count(), rt.voice_count(), rt.decision_count()),
            (1, 0, 1)
        );
        rt.release(first).unwrap();
        rt.flush_ended(|_| true);
        let second = rt.trigger(input(2), 60, 1.).unwrap();
        assert_eq!(
            rt.note_take(second, sampler_core::Trigger::Attack, 0),
            Ok(Some(1))
        );
        rt.panic();
        rt.flush_ended(|_| true);
        assert_eq!(rt.decision_count(), 0);
    });
}

#[test]
fn scope_and_retained_decision_capacities_fail_without_advancing_other_owners() {
    let mut rt = Runtime::new(
        plan(SequenceScope::Key, 1, 1, 1),
        Limits {
            decisions: 1,
            ..limits()
        },
    )
    .unwrap();
    support::without_heap(|| {
        let first = rt.trigger(input(0), 60, 1.).unwrap();
        rt.render(&mut [[0.; 2]; 1]).unwrap();
        rt.release(first).unwrap();
        rt.flush_ended(|_| false);
        assert_eq!(rt.trigger(input(1), 60, 1.), Err(Error::Capacity));
        rt.flush_ended(|_| true);
        assert_eq!(rt.trigger(input(2), 61, 1.), Err(Error::Capacity));
        assert_eq!((rt.note_count(), rt.decision_count()), (0, 0));
        let next = rt
            .trigger_pitched(
                Input { key: 0, ..input(3) },
                NotePitch::Absolute(60.25),
                1.,
                Expression::default(),
            )
            .unwrap();
        assert_eq!(
            rt.note_take(next, sampler_core::Trigger::Attack, 0),
            Ok(Some(1))
        );
        rt.panic();
        rt.flush_ended(|_| true);
    });
}

#[test]
fn missing_takes_advance_but_ineligible_gestures_do_not() {
    let r = Region {
        velocity_high: 0.5,
        ..region(0)
    };
    let plan = Prepared::new(
        48000,
        vec![Pcm::new(48000, Box::from([[1.; 2]; 1])).unwrap()],
        vec![r],
        4,
    )
    .unwrap()
    .with_variation(
        vec![sequence(SequenceScope::Global, 1)],
        vec![Some(Take {
            sequence: 0,
            index: 0,
        })],
        1,
        0,
    )
    .unwrap();
    let mut rt = Runtime::new(plan, limits()).unwrap();
    support::without_heap(|| {
        for i in 0..9 {
            let silent = rt.trigger(input(i), 60, 1.).unwrap();
            assert_eq!(
                rt.note_take(silent, sampler_core::Trigger::Attack, 0),
                Ok(None)
            );
            assert_eq!((rt.voice_count(), rt.decision_count()), (0, 0));
            rt.release(silent).unwrap();
            rt.flush_ended(|_| true);
            let note = rt.trigger(input(i), 60, 0.5).unwrap();
            assert_eq!(
                rt.note_take(note, sampler_core::Trigger::Attack, 0),
                Ok(Some(i as u32 % 3))
            );
            assert_eq!(rt.voice_count(), usize::from(i % 3 == 0));
            let mut audio = [[0.; 2]; 1];
            rt.render(&mut audio).unwrap();
            assert_eq!(audio, [[if i % 3 == 0 { 0.5 } else { 0. }; 2]]);
            rt.release(note).unwrap();
            rt.flush_ended(|_| true);
            assert_eq!(rt.decision_count(), 0);
        }
    });
}

#[test]
fn generation_counters_and_delayed_children_keep_original_take_policy() {
    for policy in [
        TakePolicy::Sequential,
        TakePolicy::Random { seed: 23 },
        TakePolicy::NoRepeat { seed: 23 },
        TakePolicy::Shuffle { seed: 23 },
    ] {
        let mut reference = Runtime::new(
            policy_plan(SequenceScope::Global, 1, 1, 1, policy),
            limits(),
        )
        .unwrap();
        let mut choices = [0; 2];
        for (id, choice) in choices.iter_mut().enumerate() {
            let n = reference.trigger(input(id as i32), 60, 1.).unwrap();
            *choice = reference
                .note_take(n, sampler_core::Trigger::Attack, 0)
                .unwrap()
                .unwrap();
            reference.release(n).unwrap();
            reference.flush_ended(|_| true);
        }

        let old = policy_plan(SequenceScope::Global, 1, 1, 1, policy)
            .with_programs(
                vec![
                    Program::new(vec![
                        Instruction::Wait(4),
                        Instruction::Play {
                            transpose: 0,
                            velocity: Velocity::Fixed(1.),
                            inheritance: Inheritance::Independent,
                            duration: Duration::Frames(1),
                        },
                        Instruction::End,
                    ])
                    .unwrap(),
                ],
                None,
            )
            .unwrap();
        let (mut rt, mut control) = Runtime::with_plan_updates(old, limits(), 2, 1).unwrap();
        control
            .submit(Box::new(policy_plan(
                SequenceScope::Global,
                1,
                4,
                1,
                policy,
            )))
            .unwrap();
        let old_plan = rt.active_plan();
        support::without_heap(|| {
            let root = rt.trigger(input(0), 60, 1.).unwrap();
            rt.start_behavior(root, 0).unwrap();
            let mut audio = [[0.; 2]; 6];
            rt.render(&mut audio[..1]).unwrap();
            assert_eq!(rt.poll_plan_update(), Ok(Some(1)));
            let first = rt.trigger(input(1), 60, 1.).unwrap();
            assert_eq!(
                rt.note_take(first, sampler_core::Trigger::Attack, 0),
                Ok(Some(choices[0]))
            );
            rt.render(&mut audio[1..2]).unwrap();
            rt.release(first).unwrap();
            rt.flush_ended(|_| true);
            let second = rt.trigger(input(2), 60, 1.).unwrap();
            assert_eq!(
                rt.note_take(second, sampler_core::Trigger::Attack, 0),
                Ok(Some(choices[1]))
            );
            rt.render(&mut audio[2..]).unwrap();
            assert_eq!(
                audio.map(|f| f[0]),
                [
                    (choices[0] + 1) as f32 / 8.,
                    (choices[0] + 1) as f32 / 2.,
                    (choices[1] + 1) as f32 / 2.,
                    0.,
                    (choices[1] + 1) as f32 / 8.,
                    0.
                ]
            );
            assert_eq!(
                rt.note_take(root, sampler_core::Trigger::Attack, 0),
                Ok(Some(choices[0]))
            );
            assert_eq!(rt.note_plan(root), Ok(old_plan));
            rt.flush_behaviors(|_, _, _| true);
            rt.panic();
            rt.flush_ended(|_| false);
            assert_eq!(rt.collect_retired_plans(), 0);
            rt.flush_ended(|_| true);
            assert_eq!(rt.decision_count(), 0);
            assert_eq!(rt.collect_retired_plans(), 1);
        });
        drop(control.retired().unwrap()); // Mutable sequence storage is destroyed on control too.
    }
}

#[test]
fn preparation_rejects_invalid_sequence_tables_and_bounds() {
    let prepare = || {
        Prepared::new(
            48000,
            vec![Pcm::new(48000, Box::from([[1.; 2]; 1])).unwrap()],
            vec![region(0)],
            4,
        )
        .unwrap()
    };
    let tag = Some(Take {
        sequence: 0,
        index: 0,
    });
    for spec in [
        Sequence {
            takes: 0,
            ..sequence(SequenceScope::Global, 1)
        },
        sequence(SequenceScope::Key, 0),
    ] {
        assert!(matches!(
            prepare().with_variation(vec![spec], vec![tag], 128, 0),
            Err(Error::InvalidInput)
        ));
    }
    for tags in [
        vec![],
        vec![tag, tag],
        vec![Some(Take {
            sequence: 1,
            index: 0,
        })],
        vec![Some(Take {
            sequence: 0,
            index: 3,
        })],
    ] {
        assert!(matches!(
            prepare().with_variation(vec![sequence(SequenceScope::Global, 1)], tags, 1, 0),
            Err(Error::InvalidInput)
        ));
    }
    assert!(matches!(
        prepare().with_variation(vec![sequence(SequenceScope::Key, 2)], vec![tag], 1, 0),
        Err(Error::Capacity)
    ));
    assert!(matches!(
        prepare().with_variation(
            vec![sequence(SequenceScope::Channel, usize::MAX)],
            vec![tag],
            usize::MAX,
            0
        ),
        Err(Error::Capacity)
    ));
    assert!(matches!(
        prepare().with_variation(
            vec![
                sequence(SequenceScope::Channel, usize::MAX),
                sequence(SequenceScope::Global, 1)
            ],
            vec![tag],
            usize::MAX,
            0
        ),
        Err(Error::Capacity)
    ));
    // Finite domains reserve only their possible owners, regardless of a larger ceiling.
    assert!(
        prepare()
            .with_variation(
                vec![sequence(SequenceScope::Global, usize::MAX)],
                vec![tag],
                1,
                0
            )
            .is_ok()
    );
    assert!(
        prepare()
            .with_variation(
                vec![sequence(SequenceScope::Key, usize::MAX)],
                vec![tag],
                128,
                0
            )
            .is_ok()
    );
}

#[test]
fn later_sequence_failure_does_not_advance_an_earlier_sequence() {
    for policy in [
        TakePolicy::Sequential,
        TakePolicy::Random { seed: 23 },
        TakePolicy::NoRepeat { seed: 23 },
        TakePolicy::Shuffle { seed: 23 },
    ] {
        let mut reference = Runtime::new(
            policy_plan(SequenceScope::Global, 1, 1, 1, policy),
            limits(),
        )
        .unwrap();
        let mut choices = [0; 2];
        for (id, choice) in choices.iter_mut().enumerate() {
            let n = reference.trigger(input(id as i32), 60, 1.).unwrap();
            *choice = reference
                .note_take(n, sampler_core::Trigger::Attack, 0)
                .unwrap()
                .unwrap();
            reference.release(n).unwrap();
            reference.flush_ended(|_| true);
        }

        let prepared = Prepared::new(
            48000,
            vec![Pcm::new(48000, Box::from([[1.; 2]; 1])).unwrap()],
            vec![region(0); 2],
            8,
        )
        .unwrap()
        .with_variation(
            vec![
                Sequence {
                    policy,
                    ..sequence(SequenceScope::Global, 1)
                },
                sequence(SequenceScope::Channel, 1),
            ],
            vec![
                Some(Take {
                    sequence: 0,
                    index: 0,
                }),
                Some(Take {
                    sequence: 1,
                    index: 0,
                }),
            ],
            2,
            3,
        )
        .unwrap();
        let mut rt = Runtime::new(prepared, limits()).unwrap();
        support::without_heap(|| {
            let first = rt.trigger(input(0), 60, 1.).unwrap();
            rt.release(first).unwrap();
            rt.flush_ended(|_| true);
            assert_eq!(
                rt.trigger(
                    Input {
                        port: 1,
                        ..input(1)
                    },
                    60,
                    1.
                ),
                Err(Error::Capacity)
            );
            assert_eq!((rt.note_count(), rt.decision_count()), (0, 0));
            let second = rt.trigger(input(2), 60, 1.).unwrap();
            assert_eq!(
                rt.note_take(second, sampler_core::Trigger::Attack, 0),
                Ok(Some(choices[1]))
            );
            assert_eq!(
                rt.note_take(second, sampler_core::Trigger::Attack, 1),
                Ok(Some(1))
            );
            rt.panic();
            rt.flush_ended(|_| true);
        });
    }
}

#[test]
fn generated_notes_reclaim_decisions_without_consuming_host_terminals() {
    let play = Instruction::Play {
        transpose: 0,
        velocity: Velocity::Fixed(1.),
        inheritance: Inheritance::Linked,
        duration: Duration::Frames(1),
    };
    let program = Program::new(vec![
        play,
        Instruction::Wait(2),
        play,
        Instruction::Wait(2),
        play,
        Instruction::End,
    ])
    .unwrap();
    let prepared = plan(SequenceScope::Channel, 1, 1, 1)
        .with_programs(vec![program], Some(0))
        .unwrap();
    let mut rt = Runtime::new(
        prepared,
        Limits {
            decisions: 1,
            ..limits()
        },
    )
    .unwrap();
    support::without_heap(|| {
        let root = rt
            .trigger(
                Input {
                    protocol: Protocol::Midi2,
                    port: 19,
                    group: 15,
                    channel: 14,
                    ..input(0)
                },
                60,
                1.,
            )
            .unwrap();
        assert_eq!(
            rt.note_take(root, sampler_core::Trigger::Attack, 0),
            Ok(None)
        ); // Bound program suppressed its own selection.
        let mut audio = [[0.; 2]; 6];
        rt.render(&mut audio).unwrap();
        assert_eq!(audio.map(|f| f[0]), [0.125, 0., 0.25, 0., 0.375, 0.]);
        assert_eq!(rt.decision_count(), 1);
        rt.flush_behaviors(|_, _, _| true);
        rt.flush_ended(|_| panic!("the held physical root must survive internal reclamation"));
        assert_eq!((rt.note_count(), rt.decision_count()), (1, 0));
        rt.release(root).unwrap();
        let mut ended = 0;
        rt.flush_ended(|_| {
            ended += 1;
            true
        });
        assert_eq!((ended, rt.note_count()), (1, 0));
    });
}

#[test]
fn family_capacity_failure_preserves_take_and_selected_pitch_preflight_is_atomic() {
    let mut rt = Runtime::new(
        plan(SequenceScope::Global, 1, 4, 1),
        Limits {
            families: 1,
            ..limits()
        },
    )
    .unwrap();
    support::without_heap(|| {
        let holder = rt.note_on(input(0), 60, 1.).unwrap();
        let empty = rt.create_family(holder).unwrap();
        assert_eq!(rt.trigger(input(1), 60, 1.), Err(Error::Capacity));
        assert_eq!((rt.decision_count(), rt.voice_count()), (0, 0));
        rt.finish_family(empty).unwrap();
        let note = rt.trigger(input(1), 60, 1.).unwrap();
        assert_eq!(
            rt.note_take(note, sampler_core::Trigger::Attack, 0),
            Ok(Some(0))
        );
        rt.panic();
        rt.flush_ended(|_| true);
    });
    let high = Region {
        playback: Playback {
            transpose_semitones: 48.,
            ..Playback::default()
        },
        ..region(0)
    };
    let plan = Prepared::new(
        48000,
        vec![Pcm::new(48000, Box::from([[1.; 2]; 1])).unwrap()],
        vec![region(0), high],
        8,
    )
    .unwrap()
    .with_variation(
        vec![Sequence {
            takes: 2,
            ..sequence(SequenceScope::Global, 1)
        }],
        vec![
            Some(Take {
                sequence: 0,
                index: 0,
            }),
            Some(Take {
                sequence: 0,
                index: 1,
            }),
        ],
        1,
        0,
    )
    .unwrap();
    let mut rt = Runtime::new(plan, limits()).unwrap();
    support::without_heap(|| {
        let raised = Expression {
            pitch_semitones: 1.,
            ..Expression::default()
        };
        let first = rt
            .trigger_with_expression(input(0), 60, 1., raised)
            .unwrap();
        assert_eq!(
            rt.note_take(first, sampler_core::Trigger::Attack, 0),
            Ok(Some(0))
        );
        rt.release(first).unwrap();
        rt.flush_ended(|_| true);
        assert_eq!(
            rt.trigger_with_expression(input(1), 60, 1., raised),
            Err(Error::InvalidInput)
        );
        assert_eq!(
            (
                rt.decision_count(),
                rt.note_count(),
                rt.family_count(),
                rt.voice_count()
            ),
            (0, 0, 0, 0)
        );
        let next = rt.trigger(input(1), 60, 1.).unwrap();
        assert_eq!(
            rt.note_take(next, sampler_core::Trigger::Attack, 0),
            Ok(Some(1))
        );
        rt.panic();
        rt.flush_ended(|_| true);
    });
}

#[test]
fn seeded_policies_are_reproducible_and_enforce_their_distinct_contracts() {
    for policy in [
        TakePolicy::Random { seed: 42 },
        TakePolicy::NoRepeat { seed: 42 },
        TakePolicy::Shuffle { seed: 42 },
    ] {
        let mut a = Runtime::new(
            policy_plan(SequenceScope::Global, 1, 4, 4, policy),
            limits(),
        )
        .unwrap();
        let mut b = Runtime::new(
            policy_plan(SequenceScope::Global, 1, 4, 4, policy),
            limits(),
        )
        .unwrap();
        let mut history = [0; 300];
        support::without_heap(|| {
            for (i, take) in history.iter_mut().enumerate() {
                let na = a.trigger(input(i as i32), 60, 1.).unwrap();
                let nb = b.trigger(input(i as i32), 60, 1.).unwrap();
                *take = a
                    .note_take(na, sampler_core::Trigger::Attack, 0)
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    b.note_take(nb, sampler_core::Trigger::Attack, 0),
                    Ok(Some(*take))
                );
                assert!(*take < 3);
                assert_eq!((a.voice_count(), a.family_count()), (4, 1));
                for family in a.note_families(na).unwrap() {
                    assert_eq!(
                        a.family_take(family),
                        Ok(Some(Take {
                            sequence: 0,
                            index: *take
                        }))
                    );
                }
                let mut whole = [[0.; 2]; 4];
                let mut split = whole;
                a.render(&mut whole).unwrap();
                for frame in &mut split {
                    b.render(std::slice::from_mut(frame)).unwrap();
                }
                assert_eq!(whole, split);
                assert_eq!(whole, [[(*take + 1) as f32 / 2.; 2]; 4]);
                a.release(na).unwrap();
                b.release(nb).unwrap();
                a.flush_ended(|_| true);
                b.flush_ended(|_| true);
            }
        });
        assert!(history.contains(&0) && history.contains(&1) && history.contains(&2));
        match policy {
            TakePolicy::NoRepeat { .. } => assert!(history.windows(2).all(|w| w[0] != w[1])),
            TakePolicy::Shuffle { .. } => {
                for bag in history.as_chunks::<3>().0 {
                    assert_eq!(bag.iter().fold(0, |mask, take| mask | 1 << take), 7);
                }
                // Shuffle is allowed to repeat across the boundary of two complete bags.
                assert!(
                    history
                        .windows(2)
                        .enumerate()
                        .any(|(i, w)| i % 3 == 2 && w[0] == w[1])
                );
            }
            TakePolicy::Random { .. } => assert!(history.windows(2).any(|w| w[0] == w[1])),
            TakePolicy::Sequential => unreachable!(),
        }
    }
}

#[test]
fn random_streams_follow_scope_identity_not_cell_assignment_or_other_owners() {
    for policy in [
        TakePolicy::Random { seed: 921 },
        TakePolicy::NoRepeat { seed: 921 },
        TakePolicy::Shuffle { seed: 921 },
    ] {
        for scope in [
            SequenceScope::Key,
            SequenceScope::Channel,
            SequenceScope::ChannelKey,
        ] {
            let owners = if scope == SequenceScope::Key { 4 } else { 8 };
            let mut a = Runtime::new(policy_plan(scope, owners, 1, 1, policy), limits()).unwrap();
            let mut b = Runtime::new(policy_plan(scope, owners, 1, 1, policy), limits()).unwrap();
            let mut expected = [[0; 8]; 60];
            let address = |owner: usize| {
                let owner = if scope == SequenceScope::ChannelKey {
                    owner / 4
                } else {
                    owner
                };
                Input {
                    protocol: [
                        Protocol::Native,
                        Protocol::Midi1,
                        Protocol::Midi2,
                        Protocol::Clap,
                        Protocol::Vst3,
                    ][owner % 5],
                    port: if owner == 5 { u16::MAX } else { 0 },
                    group: if owner == 6 { 15 } else { 0 },
                    channel: if owner == 7 { 15 } else { 0 },
                    ..input(0)
                }
            };
            support::without_heap(|| {
                // Claim cells in ascending order and fully consume each owner's history.
                for owner in 0..owners {
                    for step in &mut expected {
                        let n = a
                            .trigger(address(owner), 59 + (owner % 4) as u8, 1.)
                            .unwrap();
                        step[owner] = a
                            .note_take(n, sampler_core::Trigger::Attack, 0)
                            .unwrap()
                            .unwrap();
                        a.release(n).unwrap();
                        a.flush_ended(|_| true);
                    }
                }
                // Claim in reverse order; interleave every owner at each step.
                for step in &expected {
                    for owner in (0..owners).rev() {
                        let n = b
                            .trigger(address(owner), 59 + (owner % 4) as u8, 1.)
                            .unwrap();
                        assert_eq!(
                            b.note_take(n, sampler_core::Trigger::Attack, 0),
                            Ok(Some(step[owner]))
                        );
                        b.release(n).unwrap();
                        b.flush_ended(|_| true);
                    }
                }
            });
            assert!(expected.iter().any(|step| step[0] != step[1]));
        }
    }
}

#[test]
fn random_admission_failures_preserve_future_history_and_publish_nothing() {
    for policy in [
        TakePolicy::Random { seed: 194 },
        TakePolicy::NoRepeat { seed: 194 },
        TakePolicy::Shuffle { seed: 194 },
    ] {
        // Voice, family, retained decision, duplicate identity and invalid input/pitch.
        for failure in 0..6 {
            let mut capacity = limits();
            match failure {
                0 => capacity.voices = 4,
                1 => capacity.families = 1,
                2 => capacity.decisions = 1,
                _ => {}
            }
            let mut rt = Runtime::new(
                policy_plan(SequenceScope::Global, 1, 4, 1, policy),
                capacity,
            )
            .unwrap();
            let mut reference = Runtime::new(
                policy_plan(SequenceScope::Global, 1, 4, 1, policy),
                limits(),
            )
            .unwrap();
            support::without_heap(|| {
                let first = rt.trigger(input(0), 60, 1.).unwrap();
                let expected = reference.trigger(input(0), 60, 1.).unwrap();
                assert_eq!(
                    rt.note_take(first, sampler_core::Trigger::Attack, 0),
                    reference.note_take(expected, sampler_core::Trigger::Attack, 0)
                );
                if failure >= 2 {
                    rt.render(&mut [[0.; 2]; 1]).unwrap();
                }
                let before = (
                    rt.note_count(),
                    rt.voice_count(),
                    rt.family_count(),
                    rt.decision_count(),
                );
                let result = match failure {
                    3 => rt.trigger(input(0), 60, 1.),
                    4 => rt.trigger(
                        Input {
                            group: 16,
                            ..input(1)
                        },
                        60,
                        1.,
                    ),
                    5 => rt.trigger_with_expression(
                        input(1),
                        60,
                        1.,
                        Expression {
                            pitch_semitones: 100.,
                            ..Expression::default()
                        },
                    ),
                    _ => rt.trigger(input(1), 60, 1.),
                };
                assert_eq!(
                    result,
                    Err(match failure {
                        3 => Error::DuplicateInput,
                        4 | 5 => Error::InvalidInput,
                        _ => Error::Capacity,
                    })
                );
                assert_eq!(
                    before,
                    (
                        rt.note_count(),
                        rt.voice_count(),
                        rt.family_count(),
                        rt.decision_count()
                    )
                );
                rt.release(first).unwrap();
                rt.flush_ended(|_| true);
                reference.release(expected).unwrap();
                reference.flush_ended(|_| true);
                for id in 1..61 {
                    let n = rt.trigger(input(id), 60, 1.).unwrap();
                    let expected = reference.trigger(input(id), 60, 1.).unwrap();
                    assert_eq!(
                        rt.note_take(n, sampler_core::Trigger::Attack, 0),
                        reference.note_take(expected, sampler_core::Trigger::Attack, 0)
                    );
                    rt.release(n).unwrap();
                    rt.flush_ended(|_| true);
                    reference.release(expected).unwrap();
                    reference.flush_ended(|_| true);
                }
            });
        }
    }
}

#[test]
fn random_preparation_rejects_impossible_no_repeat_and_unbudgeted_shuffle() {
    let prepare = |spec: Sequence, states, entries| {
        Prepared::new(
            48000,
            vec![Pcm::new(48000, Box::from([[1.; 2]])).unwrap()],
            vec![region(0)],
            4,
        )
        .unwrap()
        .with_variation(
            vec![spec],
            vec![Some(Take {
                sequence: 0,
                index: 0,
            })],
            states,
            entries,
        )
    };
    let mut spec = Sequence {
        policy: TakePolicy::Shuffle { seed: 0 },
        ..sequence(SequenceScope::Channel, 2)
    };
    assert!(matches!(prepare(spec, 2, 5), Err(Error::Capacity)));
    assert!(prepare(spec, 2, 6).is_ok());
    spec.capacity = usize::MAX;
    assert!(matches!(
        prepare(spec, usize::MAX, usize::MAX),
        Err(Error::Capacity)
    ));
    spec.capacity = 1;
    spec.takes = 1;
    spec.policy = TakePolicy::NoRepeat { seed: 0 };
    assert!(matches!(prepare(spec, 1, 1), Err(Error::InvalidInput)));
    for policy in [
        TakePolicy::Random { seed: u64::MAX },
        TakePolicy::Shuffle { seed: u64::MAX },
    ] {
        spec.policy = policy;
        let mut rt = Runtime::new(prepare(spec, 1, 1).unwrap(), limits()).unwrap();
        support::without_heap(|| {
            for id in 0..8 {
                let n = rt.trigger(input(id), 60, 1.).unwrap();
                assert_eq!(
                    rt.note_take(n, sampler_core::Trigger::Attack, 0),
                    Ok(Some(0))
                );
                rt.release(n).unwrap();
                rt.flush_ended(|_| true);
            }
        });
    }
}
