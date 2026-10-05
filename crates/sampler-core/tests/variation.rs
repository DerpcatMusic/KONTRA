use sampler_core::{
    ChannelAddress, Duration, Envelope, Error, Expression, Inheritance, Input, Instruction, Limits,
    NotePitch, Pcm, Playback, Prepared, Program, Protocol, Region, Runtime, Sequence,
    SequenceScope, Take, Velocity,
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
        scope,
        capacity,
    }
}

fn plan(scope: SequenceScope, capacity: usize, microphones: usize, frames: usize) -> Prepared {
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
        .with_variation(vec![sequence(scope, capacity)], takes, capacity)
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
        .with_variation(specs.to_vec(), tags.clone(), 165)
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
                assert_eq!(rt.note_take(note, seq), Ok(*expected));
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
                assert_eq!(rt.note_take(note, seq), Ok(*expected));
            }
            rt.note_off(source).unwrap();
            rt.flush_ended(|_| false);
            assert_eq!(rt.decision_count(), expected_takes.iter().flatten().count());
            rt.flush_ended(|_| true);
            assert_eq!((rt.decision_count(), rt.note_count()), (0, 0));
            assert_eq!(rt.note_take(note, 0), Err(Error::StaleHandle));
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
        assert_eq!(rt.note_take(first, 0), Ok(Some(0)));
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
        assert_eq!(rt.note_take(second, 0), Ok(Some(1)));
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
        assert_eq!(rt.note_take(next, 0), Ok(Some(1)));
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
    )
    .unwrap();
    let mut rt = Runtime::new(plan, limits()).unwrap();
    support::without_heap(|| {
        for i in 0..9 {
            let silent = rt.trigger(input(i), 60, 1.).unwrap();
            assert_eq!(rt.note_take(silent, 0), Ok(None));
            assert_eq!((rt.voice_count(), rt.decision_count()), (0, 0));
            rt.release(silent).unwrap();
            rt.flush_ended(|_| true);
            let note = rt.trigger(input(i), 60, 0.5).unwrap();
            assert_eq!(rt.note_take(note, 0), Ok(Some(i as u32 % 3)));
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
    let old = plan(SequenceScope::Global, 1, 1, 1)
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
        .submit(Box::new(plan(SequenceScope::Global, 1, 4, 1)))
        .unwrap();
    let old_plan = rt.active_plan();
    support::without_heap(|| {
        let root = rt.trigger(input(0), 60, 1.).unwrap();
        rt.start_behavior(root, 0).unwrap();
        let mut audio = [[0.; 2]; 6];
        rt.render(&mut audio[..1]).unwrap();
        assert_eq!(rt.poll_plan_update(), Ok(Some(1)));
        let first = rt.trigger(input(1), 60, 1.).unwrap();
        assert_eq!(rt.note_take(first, 0), Ok(Some(0)));
        rt.render(&mut audio[1..2]).unwrap();
        rt.release(first).unwrap();
        rt.flush_ended(|_| true);
        let second = rt.trigger(input(2), 60, 1.).unwrap();
        assert_eq!(rt.note_take(second, 0), Ok(Some(1)));
        rt.render(&mut audio[2..]).unwrap();
        assert_eq!(audio.map(|f| f[0]), [0.125, 0.5, 1., 0., 0.25, 0.]);
        assert_eq!(rt.note_take(root, 0), Ok(Some(0)));
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
            prepare().with_variation(vec![spec], vec![tag], 128),
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
            prepare().with_variation(vec![sequence(SequenceScope::Global, 1)], tags, 1),
            Err(Error::InvalidInput)
        ));
    }
    assert!(matches!(
        prepare().with_variation(vec![sequence(SequenceScope::Key, 2)], vec![tag], 1),
        Err(Error::Capacity)
    ));
    assert!(matches!(
        prepare().with_variation(
            vec![sequence(SequenceScope::Channel, usize::MAX)],
            vec![tag],
            usize::MAX
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
            usize::MAX
        ),
        Err(Error::Capacity)
    ));
    // Finite domains reserve only their possible owners, regardless of a larger ceiling.
    assert!(
        prepare()
            .with_variation(
                vec![sequence(SequenceScope::Global, usize::MAX)],
                vec![tag],
                1
            )
            .is_ok()
    );
    assert!(
        prepare()
            .with_variation(
                vec![sequence(SequenceScope::Key, usize::MAX)],
                vec![tag],
                128
            )
            .is_ok()
    );
}

#[test]
fn later_sequence_failure_does_not_advance_an_earlier_sequence() {
    let prepared = Prepared::new(
        48000,
        vec![Pcm::new(48000, Box::from([[1.; 2]; 1])).unwrap()],
        vec![region(0); 2],
        8,
    )
    .unwrap()
    .with_variation(
        vec![
            sequence(SequenceScope::Global, 1),
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
        assert_eq!(rt.note_take(second, 0), Ok(Some(1)));
        assert_eq!(rt.note_take(second, 1), Ok(Some(1)));
        rt.panic();
        rt.flush_ended(|_| true);
    });
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
        assert_eq!(rt.note_take(root, 0), Ok(None)); // Bound program suppressed its own selection.
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
        assert_eq!(rt.note_take(note, 0), Ok(Some(0)));
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
        assert_eq!(rt.note_take(first, 0), Ok(Some(0)));
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
        assert_eq!(rt.note_take(next, 0), Ok(Some(1)));
        rt.panic();
        rt.flush_ended(|_| true);
    });
}
