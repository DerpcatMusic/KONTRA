use sampler_core::{
    Envelope, Error, Event, Expression, Input, Limits, Loop, LoopMode, Pcm, Playback, Prepared,
    Protocol, Region, ReleaseOptions, ReleaseReserve, ReleaseStatus, ReleaseVelocity, Runtime,
    Sequence, SequenceScope, Take, TakePolicy, Trigger,
};
mod support;

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
fn limits() -> Limits {
    Limits {
        notes: 8,
        channels: 1,
        performances: 1,
        families: 16,
        expressions: 8,
        voices: 32,
        decisions: 24,
        commands: 16,
        behaviors: 0,
        behavior_fuel: 0,
        behavior_cells: 0,
        note_cells: 0,
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
fn prepared(
    regions: Vec<Region>,
    triggers: Vec<Trigger>,
    options: ReleaseOptions,
    frames: usize,
) -> Prepared {
    let candidates = regions.len();
    Prepared::new(
        48000,
        vec![Pcm::new(48000, vec![[1.; 2]; frames].into_boxed_slice()).unwrap()],
        regions,
        candidates,
    )
    .unwrap()
    .with_releases(triggers, options, options)
    .unwrap()
}
fn phases(shared: bool) -> Prepared {
    let mut regions = Vec::new();
    let mut tags = Vec::new();
    let mut triggers = Vec::new();
    let pcm = (1..=3)
        .map(|n| Pcm::new(48000, vec![[n as f32 / 8.; 2]; 8].into_boxed_slice()).unwrap())
        .collect();
    for (phase, trigger) in [Trigger::Attack, Trigger::KeyRelease, Trigger::GateRelease]
        .into_iter()
        .enumerate()
    {
        for take in 0..3 {
            for _ in 0..2 {
                regions.push(region(take));
                tags.push(Some(Take {
                    sequence: if shared { 0 } else { phase },
                    index: take as u32,
                }));
                triggers.push(trigger);
            }
        }
    }
    let count = if shared { 1 } else { 3 };
    Prepared::new(48000, pcm, regions, 18)
        .unwrap()
        .with_releases(
            triggers,
            ReleaseOptions::default(),
            ReleaseOptions::default(),
        )
        .unwrap()
        .with_variation(
            vec![
                Sequence {
                    takes: 3,
                    policy: TakePolicy::Sequential,
                    scope: SequenceScope::Global,
                    capacity: 1
                };
                count
            ],
            tags,
            count,
            0,
        )
        .unwrap()
}

#[test]
fn full_capacity_pedal_burst_preserves_complete_mics_and_independent_phase_history() {
    let mut rt = Runtime::new(
        phases(false),
        Limits {
            voices: 12,
            families: 6,
            decisions: 6,
            ..limits()
        },
    )
    .unwrap();
    support::without_heap(|| {
        let channel = rt.register_channel(input(0).channel_address()).unwrap();
        rt.sustain(channel, true).unwrap();
        let a = rt.trigger(input(1), 60, 1.).unwrap();
        let b = rt.trigger(input(2), 60, 1.).unwrap();
        assert_eq!(
            rt.release_reserve(),
            ReleaseReserve {
                voices: 8,
                families: 4,
                decisions: 4,
                commands: 0
            }
        );
        assert_eq!(rt.trigger(input(3), 60, 1.), Err(Error::Capacity));
        assert_eq!(rt.create_family(a), Err(Error::Capacity));
        let mut block = [[0.; 2]; 8];
        rt.render(&mut block).unwrap();
        assert_eq!(block, [[0.75; 2]; 8]); // two mics at takes 0 and 1
        assert_eq!(rt.voice_count(), 0); // attack EOF does not consume release reservations
        rt.key_up(a, Some(0.25)).unwrap();
        rt.key_up(b, Some(0.75)).unwrap();
        assert_eq!(rt.voice_count(), 4);
        assert_eq!(
            rt.release_status(a, Trigger::GateRelease),
            Ok(ReleaseStatus::Pending)
        );
        rt.render(&mut block[..2]).unwrap();
        assert_eq!(&block[..2], &[[0.75; 2]; 2]); // onset velocity policy
        rt.sustain(channel, false).unwrap();
        assert_eq!(rt.voice_count(), 8); // key layers survive gate release
        assert_eq!(rt.release_reserve(), ReleaseReserve::default());
        rt.render(&mut block).unwrap();
        assert_eq!(
            block,
            [
                [1.5; 2], [1.5; 2], [1.5; 2], [1.5; 2], [1.5; 2], [1.5; 2], [0.75; 2], [0.75; 2]
            ]
        );
        for (note, expected) in [(a, 0), (b, 1)] {
            for (sequence, phase) in [Trigger::Attack, Trigger::KeyRelease, Trigger::GateRelease]
                .into_iter()
                .enumerate()
            {
                assert_eq!(rt.note_take(note, phase, sequence), Ok(Some(expected)));
            }
        }
        rt.flush_ended(|_| false);
        assert_eq!(rt.decision_count(), 6);
        rt.flush_ended(|_| true);
        assert_eq!(
            (
                rt.note_count(),
                rt.family_count(),
                rt.decision_count(),
                rt.expression_count()
            ),
            (0, 0, 0, 0)
        );
    });
}

#[test]
fn shared_sequence_is_explicit_and_decisions_remain_phase_qualified() {
    let mut rt = Runtime::new(phases(true), limits()).unwrap();
    support::without_heap(|| {
        let note = rt.trigger(input(0), 60, 1.).unwrap();
        rt.key_up(note, None).unwrap();
        for (take, phase) in [Trigger::Attack, Trigger::KeyRelease, Trigger::GateRelease]
            .into_iter()
            .enumerate()
        {
            assert_eq!(rt.note_take(note, phase, 0), Ok(Some(take as u32)));
        }
        let mut block = [[0.; 2]; 8];
        rt.render(&mut block).unwrap();
        assert_eq!(block, [[1.25; 2]; 8]); // attack closed; two release mics of takes 1 and 2
        rt.flush_ended(|_| true);
    });
}

#[test]
fn velocity_sweep_reserves_overlap_not_all_takes_or_disjoint_layers() {
    // +0 and -0 are equal eligibility endpoints, as are the two halves at 0.5.
    let mut regions = vec![region(0); 6];
    for (r, (lo, hi)) in regions.iter_mut().zip([
        (-0., -0.),
        (0., 0.5),
        (0.5, 1.),
        (0., 1.),
        (0., 0.5),
        (0.5, 1.),
    ]) {
        r.velocity_low = lo;
        r.velocity_high = hi;
    }
    let tags = vec![
        None,
        None,
        None,
        Some(Take {
            sequence: 0,
            index: 0,
        }),
        Some(Take {
            sequence: 0,
            index: 1,
        }),
        Some(Take {
            sequence: 0,
            index: 1,
        }),
    ];
    let plan = prepared(
        regions,
        vec![Trigger::GateRelease; 6],
        ReleaseOptions {
            duration: Some(100),
            ..ReleaseOptions::default()
        },
        1,
    )
    .with_variation(
        vec![Sequence {
            takes: 2,
            policy: TakePolicy::Sequential,
            scope: SequenceScope::Global,
            capacity: 1,
        }],
        tags,
        1,
        0,
    )
    .unwrap();
    let mut rt = Runtime::new(
        plan,
        Limits {
            voices: 4,
            families: 2,
            decisions: 1,
            commands: 2,
            ..limits()
        },
    )
    .unwrap();
    support::without_heap(|| {
        for (id, velocity, voices) in [(0, 0., 3), (1, 0.5, 4), (2, 1., 2)] {
            let note = rt.trigger(input(id), 60, velocity).unwrap();
            assert_eq!(
                rt.release_reserve(),
                ReleaseReserve {
                    voices: 4,
                    families: 2,
                    decisions: 1,
                    commands: 2
                }
            );
            assert_eq!(
                rt.schedule_event(rt.now() + 1, Event::KeyUp(note, None)),
                Err(Error::Capacity)
            );
            rt.key_up(note, None).unwrap();
            assert_eq!(rt.voice_count(), voices);
            assert_eq!(rt.pending_commands(), 2);
            rt.render(&mut [[0.; 2]; 1]).unwrap();
            assert_eq!(rt.pending_commands(), 0); // EOF frees future family jobs immediately
            rt.flush_ended(|_| true);
        }
    });
}

#[test]
fn key_velocity_unknown_and_zero_are_distinct_and_synthetic_key_uses_fallback() {
    let options = ReleaseOptions {
        velocity: ReleaseVelocity::KeyUp { fallback: 0.75 },
        duration: None,
    };
    let mut rt = Runtime::new(
        prepared(
            vec![region(0); 2],
            vec![Trigger::KeyRelease, Trigger::GateRelease],
            options,
            2,
        ),
        limits(),
    )
    .unwrap();
    support::without_heap(|| {
        for (id, velocity, expected) in [(0, None, 1.5), (1, Some(0.), 0.), (2, Some(0.25), 0.5)] {
            let note = rt.trigger(input(id), 60, 1.).unwrap();
            rt.key_up(note, velocity).unwrap();
            let mut block = [[0.; 2]; 2];
            rt.render(&mut block).unwrap();
            assert_eq!(block, [[expected; 2]; 2]);
            rt.flush_ended(|_| true);
        }
        let note = rt.trigger(input(3), 60, 1.).unwrap();
        rt.release(note).unwrap(); // explicit closure suppresses physical key phase
        assert_eq!(
            rt.release_status(note, Trigger::KeyRelease),
            Ok(ReleaseStatus::Suppressed)
        );
        let mut block = [[0.; 2]; 2];
        rt.render(&mut block).unwrap();
        assert_eq!(block, [[0.75; 2]; 2]);
        rt.flush_ended(|_| true);
    });
}

#[test]
fn looped_release_owns_its_duration_and_works_at_empty_and_exclusive_end_boundaries() {
    let mut r = region(0);
    r.playback.loop_range = Some(Loop {
        passes: None,
        start: 0,
        end: 1,
        shape: sampler_core::LoopShape::Wrap,
        mode: LoopMode::Continuous,
    });
    r.envelope = Envelope::new(0, 0, 0, 1., 2).unwrap();
    let raw = || {
        Prepared::new(
            48000,
            vec![Pcm::new(48000, Box::from([[1.; 2]])).unwrap()],
            vec![r],
            1,
        )
        .unwrap()
    };
    assert!(matches!(
        raw().with_releases(
            vec![Trigger::GateRelease],
            ReleaseOptions::default(),
            ReleaseOptions::default()
        ),
        Err(Error::InvalidInput)
    ));
    for duration in [0, 3] {
        let plan = raw()
            .with_releases(
                vec![Trigger::GateRelease],
                ReleaseOptions::default(),
                ReleaseOptions {
                    duration: Some(duration),
                    ..ReleaseOptions::default()
                },
            )
            .unwrap();
        let mut rt = Runtime::new(plan, limits()).unwrap();
        support::without_heap(|| {
            let note = rt.trigger(input(0), 60, 1.).unwrap();
            rt.schedule_event(2, Event::KeyUp(note, None)).unwrap();
            let mut silent = [[9.; 2]; 2];
            rt.render(&mut silent).unwrap();
            assert_eq!(silent, [[0.; 2]; 2]);
            assert_eq!(
                rt.release_status(note, Trigger::GateRelease),
                Ok(ReleaseStatus::Pending)
            );
            rt.render(&mut []).unwrap();
            assert_eq!(
                rt.release_status(note, Trigger::GateRelease),
                Ok(ReleaseStatus::Selected)
            );
            let mut out = [[0.; 2]; 6];
            rt.render(&mut out).unwrap();
            assert_eq!(
                out,
                if duration == 0 {
                    [[1.; 2], [0.5; 2], [0.; 2], [0.; 2], [0.; 2], [0.; 2]]
                } else {
                    [[1.; 2], [1.; 2], [1.; 2], [1.; 2], [0.5; 2], [0.; 2]]
                }
            );
            assert_eq!((rt.voice_count(), rt.pending_commands()), (0, 0));
            rt.flush_ended(|_| true);
        });
    }
}

#[test]
fn dormant_pitch_is_validated_without_rejecting_unreachable_onset_velocity_layers() {
    let mut high = region(0);
    high.velocity_low = 0.5;
    high.playback.transpose_semitones = 48.; // maximum supported source ratio already
    let mut low = region(0);
    low.velocity_high = 0.49;
    for velocity_policy in [
        ReleaseVelocity::Onset,
        ReleaseVelocity::KeyUp { fallback: 0.25 },
    ] {
        let plan = prepared(
            vec![low, high],
            vec![Trigger::GateRelease; 2],
            ReleaseOptions {
                velocity: velocity_policy,
                duration: None,
            },
            4,
        );
        let mut rt = Runtime::new(plan, limits()).unwrap();
        support::without_heap(|| {
            let note = rt.trigger(input(0), 60, 0.25).unwrap();
            let owner = rt.expression_id(note).unwrap();
            let expression = Expression {
                pitch_semitones: 12.,
                ..Expression::default()
            };
            let expected = if velocity_policy == ReleaseVelocity::Onset {
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
            let channel = rt.register_channel(input(0).channel_address()).unwrap();
            rt.sustain(channel, true).unwrap();
            rt.key_up(note, Some(0.25)).unwrap();
            // Future gate layer now has a known physical velocity even for KeyUp policy.
            rt.set_expression(owner, expression).unwrap();
            rt.sustain(channel, false).unwrap();
            assert_eq!(
                rt.release_status(note, Trigger::GateRelease),
                Ok(ReleaseStatus::Selected)
            );
            rt.panic();
            rt.flush_ended(|_| true);
        });
    }
}

#[test]
fn hard_cleanup_returns_every_reservation_and_late_key_up_cannot_resurrect_layers() {
    let mut rt = Runtime::new(phases(false), limits()).unwrap();
    support::without_heap(|| {
        for id in 0..100 {
            let note = rt.trigger(input(id), 60, 1.).unwrap();
            if id % 2 == 0 {
                rt.all_sound_off(input(id).channel_address()).unwrap();
                assert!(rt.key_down(note).unwrap());
                rt.key_up(note, Some(1.)).unwrap();
            } else {
                rt.panic();
            }
            for phase in [Trigger::KeyRelease, Trigger::GateRelease] {
                assert_eq!(
                    rt.release_status(note, phase),
                    Ok(ReleaseStatus::Suppressed)
                );
                assert_eq!(
                    rt.note_take(
                        note,
                        phase,
                        if phase == Trigger::KeyRelease { 1 } else { 2 }
                    ),
                    Ok(None)
                );
            }
            assert_eq!(rt.release_reserve(), ReleaseReserve::default());
            assert_eq!((rt.voice_count(), rt.pending_commands()), (0, 0));
            rt.flush_ended(|_| true);
            assert_eq!(
                (
                    rt.note_count(),
                    rt.family_count(),
                    rt.decision_count(),
                    rt.expression_count()
                ),
                (0, 0, 0, 0)
            );
        }
    });
}

#[test]
fn release_scope_is_claimed_at_admission_without_drawing_and_failed_admission_is_atomic() {
    let plan = prepared(
        vec![region(0); 2],
        vec![Trigger::Attack, Trigger::GateRelease],
        ReleaseOptions::default(),
        1,
    )
    .with_variation(
        vec![
            Sequence {
                takes: 3,
                policy: TakePolicy::Sequential,
                scope: SequenceScope::Global,
                capacity: 1,
            },
            Sequence {
                takes: 2,
                policy: TakePolicy::Sequential,
                scope: SequenceScope::Channel,
                capacity: 1,
            },
        ],
        vec![
            Some(Take {
                sequence: 0,
                index: 0,
            }),
            Some(Take {
                sequence: 1,
                index: 1,
            }),
        ],
        2,
        0,
    )
    .unwrap();
    let mut rt = Runtime::new(plan, limits()).unwrap();
    support::without_heap(|| {
        let a = rt.trigger(input(0), 60, 1.).unwrap();
        // Admission claims channel 0 even before any release draw occurs.
        assert_eq!(
            rt.trigger(
                Input {
                    channel: 1,
                    ..input(1)
                },
                60,
                1.
            ),
            Err(Error::Capacity)
        );
        assert_eq!(rt.note_count(), 1);
        rt.key_up(a, None).unwrap(); // selected unmapped take 0 still advances/records once
        assert_eq!(rt.note_take(a, Trigger::GateRelease, 1), Ok(Some(0)));
        assert_eq!(rt.voice_count(), 0);
        assert_eq!(
            rt.release_status(a, Trigger::GateRelease),
            Ok(ReleaseStatus::Selected)
        );
        rt.flush_ended(|_| true);
        let b = rt.trigger(input(2), 60, 1.).unwrap();
        assert_eq!(rt.note_take(b, Trigger::Attack, 0), Ok(Some(1))); // rejected channel never advanced attack
        rt.key_up(b, None).unwrap();
        assert_eq!(rt.note_take(b, Trigger::GateRelease, 1), Ok(Some(1)));
        let mut out = [[0.; 2]; 1];
        rt.render(&mut out).unwrap();
        assert_eq!(out, [[1.; 2]]);
        rt.flush_ended(|_| true);
    });
}

#[test]
fn prepared_replacement_keeps_old_release_mapping_and_reserved_budget_alive() {
    let old = prepared(
        vec![region(0); 2],
        vec![Trigger::GateRelease; 2],
        ReleaseOptions::default(),
        2,
    );
    let new = prepared(
        vec![region(0)],
        vec![Trigger::GateRelease],
        ReleaseOptions::default(),
        2,
    );
    let (mut rt, mut control) = Runtime::with_plan_updates(old, limits(), 2, 1).unwrap();
    control.submit(Box::new(new)).unwrap();
    support::without_heap(|| {
        let a = rt.trigger(input(0), 60, 1.).unwrap();
        let old_id = rt.note_plan(a).unwrap();
        rt.poll_plan_update().unwrap();
        let b = rt.trigger(input(1), 60, 1.).unwrap();
        assert_ne!(rt.note_plan(b), Ok(old_id));
        assert_eq!(rt.release_reserve().voices, 3);
        rt.key_up(a, None).unwrap();
        rt.key_up(b, None).unwrap();
        let mut out = [[0.; 2]; 2];
        rt.render(&mut out).unwrap();
        assert_eq!(out, [[3.; 2]; 2]);
        rt.flush_ended(|_| false);
        assert_eq!(rt.collect_retired_plans(), 0);
        rt.flush_ended(|_| true);
        assert_eq!(rt.collect_retired_plans(), 1);
        assert_eq!(rt.release_reserve(), ReleaseReserve::default());
    });
    drop(control.retired().unwrap());
}

#[test]
fn generated_duration_and_release_commands_cannot_consume_each_others_capacity() {
    use sampler_core::{Duration, Inheritance, Instruction, Outcome, Program, Velocity};
    for capacity in [1, 2] {
        let plan = prepared(
            vec![region(0)],
            vec![Trigger::GateRelease],
            ReleaseOptions {
                duration: Some(2),
                ..ReleaseOptions::default()
            },
            8,
        )
        .with_programs(
            vec![
                Program::new(vec![
                    Instruction::Play {
                        transpose: 0,
                        velocity: Velocity::Fixed(1.),
                        inheritance: Inheritance::Linked,
                        duration: Duration::Frames(1),
                    },
                    Instruction::End,
                ])
                .unwrap(),
            ],
            None,
        )
        .unwrap();
        let mut rt = Runtime::new(
            plan,
            Limits {
                commands: capacity,
                behaviors: 1,
                behavior_fuel: 4,
                ..limits()
            },
        )
        .unwrap();
        support::without_heap(|| {
            let root = rt.note_on(input(0), 60, 1.).unwrap();
            let behavior = rt.start_behavior(root, 0).unwrap();
            if capacity == 1 {
                assert_eq!(
                    rt.behavior_outcome(behavior),
                    Ok(Some(Outcome::Fault(Error::Capacity)))
                );
                assert_eq!((rt.note_count(), rt.pending_commands()), (1, 0));
                assert_eq!(rt.release_reserve(), ReleaseReserve::default());
            } else {
                assert_eq!(rt.behavior_outcome(behavior), Ok(Some(Outcome::Finished)));
                assert_eq!(
                    (
                        rt.note_count(),
                        rt.pending_commands(),
                        rt.release_reserve().commands
                    ),
                    (2, 1, 1)
                );
                let mut out = [[0.; 2]; 4];
                rt.render(&mut out).unwrap();
                assert_eq!(out, [[0.; 2], [1.; 2], [1.; 2], [0.; 2]]);
                assert_eq!(rt.release_reserve(), ReleaseReserve::default());
                assert_eq!(rt.pending_commands(), 0);
            }
            rt.flush_behaviors(|_, _, _| true);
            rt.panic();
            rt.flush_ended(|_| true);
            assert_eq!(rt.note_count(), 0);
        });
    }
}

#[test]
fn cancelled_and_faulted_parents_suppress_descendant_release_audio() {
    use sampler_core::{Duration, Inheritance, Instruction, Outcome, Program, Velocity};
    for fault in [false, true] {
        let plan = prepared(
            vec![region(0)],
            vec![Trigger::GateRelease],
            ReleaseOptions::default(),
            8,
        )
        .with_programs(
            vec![
                Program::new(vec![
                    Instruction::Play {
                        transpose: 0,
                        velocity: Velocity::Fixed(1.),
                        inheritance: Inheritance::Linked,
                        duration: Duration::Gate,
                    },
                    Instruction::Wait(1),
                    Instruction::Jump { target: 2 },
                ])
                .unwrap(),
            ],
            None,
        )
        .unwrap();
        let mut rt = Runtime::new(
            plan,
            Limits {
                behaviors: 1,
                behavior_fuel: 4,
                ..limits()
            },
        )
        .unwrap();
        support::without_heap(|| {
            let root = rt.trigger(input(0), 60, 1.).unwrap();
            let behavior = rt.start_behavior(root, 0).unwrap();
            assert_eq!(rt.release_reserve().voices, 2);
            if fault {
                // v2: preempted every block; ends as a runaway after a second.
                for _ in 0..12 {
                    rt.render(&mut [[0.; 2]; 4800]).unwrap();
                }
            } else {
                rt.cancel_behavior(behavior).unwrap();
            }
            assert_eq!(
                rt.behavior_outcome(behavior),
                Ok(Some(if fault {
                    Outcome::FuelExhausted
                } else {
                    Outcome::Cancelled
                }))
            );
            assert_eq!(
                rt.release_status(root, Trigger::GateRelease),
                Ok(ReleaseStatus::Suppressed)
            );
            assert_eq!(rt.release_reserve(), ReleaseReserve::default());
            assert_eq!((rt.voice_count(), rt.pending_commands()), (0, 0));
            rt.flush_behaviors(|_, _, _| true);
            assert!(rt.input_held(root).unwrap());
            assert_eq!(rt.note_off(input(0), None), Ok(root));
            rt.flush_ended(|_| true);
            assert_eq!(rt.note_count(), 0);
        });
    }
}

#[test]
fn compiled_reserves_match_an_independent_grid_reference_across_varied_layers() {
    let mut seed = 7u64;
    for _ in 0..64 {
        let mut regions = Vec::new();
        let mut tags = Vec::new();
        for i in 0..24 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let a = ((seed >> 32) % 5) as f64 / 4.;
            let b = ((seed >> 40) % 5) as f64 / 4.;
            let mut r = region(0);
            r.velocity_low = a.min(b);
            r.velocity_high = a.max(b);
            regions.push(r);
            tags.push(if i < 8 {
                None
            } else {
                Some(Take {
                    sequence: i / 8 - 1,
                    index: (i % 3) as u32,
                })
            });
        }
        // Explicitly enumerate the complete eligibility partition; no endpoint sweep
        // or production selector is used to calculate this oracle.
        let mut expected = ReleaseReserve::default();
        for grid in 0..=8 {
            let velocity = grid as f64 / 8.;
            let mut counts = [[0usize; 3]; 3];
            for (r, tag) in regions.iter().zip(&tags) {
                if r.velocity_low <= velocity && velocity <= r.velocity_high {
                    let (group, take) = tag.map_or((0, 0), |t| (t.sequence + 1, t.index as usize));
                    counts[group][take] += 1;
                }
            }
            let maxima = counts.map(|c| *c.iter().max().unwrap());
            expected.voices = expected.voices.max(maxima.iter().sum());
            expected.families = expected
                .families
                .max(maxima.iter().filter(|&&v| v > 0).count());
            expected.decisions = expected
                .decisions
                .max(maxima[1..].iter().filter(|&&v| v > 0).count());
            expected.commands = expected.families;
        }
        let plan = prepared(
            regions,
            vec![Trigger::GateRelease; 24],
            ReleaseOptions {
                duration: Some(10),
                ..ReleaseOptions::default()
            },
            1,
        )
        .with_variation(
            vec![
                Sequence {
                    takes: 3,
                    policy: TakePolicy::Sequential,
                    scope: SequenceScope::Global,
                    capacity: 1
                };
                2
            ],
            tags,
            2,
            0,
        )
        .unwrap();
        let mut rt = Runtime::new(
            plan,
            Limits {
                voices: expected.voices,
                families: expected.families,
                decisions: expected.decisions,
                commands: expected.commands,
                ..limits()
            },
        )
        .unwrap();
        support::without_heap(|| {
            for grid in 0..=8 {
                let note = rt.trigger(input(grid), 60, grid as f64 / 8.).unwrap();
                assert_eq!(rt.release_reserve(), expected);
                rt.key_up(note, None).unwrap();
                assert_eq!(
                    rt.release_status(note, Trigger::GateRelease),
                    Ok(ReleaseStatus::Selected)
                );
                rt.render(&mut [[0.; 2]; 1]).unwrap();
                rt.flush_ended(|_| true);
                assert_eq!((rt.note_count(), rt.pending_commands()), (0, 0));
            }
        });
    }
}

#[test]
fn manual_family_release_and_choke_cancel_owned_timers_without_releasing_siblings() {
    let mut r = region(0);
    r.playback.loop_range = Some(Loop {
        passes: None,
        start: 0,
        end: 1,
        shape: sampler_core::LoopShape::Wrap,
        mode: LoopMode::Continuous,
    });
    r.envelope = Envelope::new(0, 0, 0, 1., 2).unwrap();
    for choke in [false, true] {
        let plan = prepared(
            vec![r],
            vec![Trigger::KeyRelease],
            ReleaseOptions {
                duration: Some(100),
                ..ReleaseOptions::default()
            },
            1,
        );
        let mut rt = Runtime::new(plan, limits()).unwrap();
        support::without_heap(|| {
            let channel = rt.register_channel(input(0).channel_address()).unwrap();
            rt.sustain(channel, true).unwrap();
            let a = rt.trigger(input(0), 60, 1.).unwrap();
            let b = rt.trigger(input(1), 60, 1.).unwrap();
            rt.key_up(a, None).unwrap();
            rt.key_up(b, None).unwrap();
            let family = rt.note_families(a).unwrap().next().unwrap();
            assert_eq!(rt.family_trigger(family), Ok(Trigger::KeyRelease));
            if choke {
                rt.choke_family(family, 1).unwrap();
            } else {
                rt.release_family(family).unwrap();
                assert_eq!(rt.release_family(family), Err(Error::ClosedFamily));
            }
            let mut out = [[0.; 2]; 3];
            rt.render(&mut out).unwrap();
            assert_eq!(
                out,
                if choke {
                    [[2.; 2], [1.; 2], [1.; 2]]
                } else {
                    [[2.; 2], [1.5; 2], [1.; 2]]
                }
            );
            assert_eq!(rt.pending_commands(), 1); // sibling's automatic timer is still owned
            assert_eq!(rt.release_family(family), Err(Error::StaleHandle));
            assert!(rt.note(a).unwrap().2); // family closure cannot consume the held gate
            rt.panic();
            rt.flush_ended(|_| true);
        });
    }
}

#[test]
fn counted_release_sources_need_no_duration_command_and_retire_at_natural_eof() {
    for shape in [
        sampler_core::LoopShape::Wrap,
        sampler_core::LoopShape::PingPong,
    ] {
        let mut r = region(0);
        r.playback.loop_range = Some(Loop {
            start: 0,
            end: 1,
            mode: LoopMode::Continuous,
            shape,
            passes: std::num::NonZeroU32::new(3),
        });
        let p = prepared(
            vec![r],
            vec![Trigger::GateRelease],
            ReleaseOptions::default(),
            1,
        );
        let mut rt = Runtime::new(
            p,
            Limits {
                commands: 0,
                ..limits()
            },
        )
        .unwrap();
        support::without_heap(|| {
            let note = rt.trigger(input(0), 60, 1.).unwrap();
            rt.key_up(note, None).unwrap();
            assert_eq!(rt.pending_commands(), 0);
            let mut output = [[0.; 2]; 5];
            rt.render(&mut output).unwrap();
            assert_eq!(output, [[1.; 2], [1.; 2], [1.; 2], [0.; 2], [0.; 2]]);
            assert_eq!((rt.voice_count(), rt.family_count()), (0, 0));
            rt.flush_ended(|_| true);
            assert_eq!(rt.note_count(), 0);
        });
    }
}

#[test]
fn velocity_curves_shape_each_phase_without_changing_selection_or_note_velocity() {
    use sampler_core::VelocityCurve;
    let mut regions = vec![region(0); 3];
    for region in &mut regions {
        region.gain = 0.5;
    }
    regions[2].velocity_low = 0.25;
    let p = prepared(
        regions,
        vec![Trigger::Attack, Trigger::KeyRelease, Trigger::GateRelease],
        ReleaseOptions {
            velocity: ReleaseVelocity::KeyUp { fallback: 0.75 },
            duration: None,
        },
        2,
    )
    .with_velocity_curves(vec![
        VelocityCurve::Linear,
        VelocityCurve::Constant,
        VelocityCurve::Power(2.0),
    ])
    .unwrap();
    let mut rt = Runtime::new(p, limits()).unwrap();
    support::without_heap(|| {
        for (id, velocity) in [0.0, 0.125, 0.25, 0.500_000_000_3, 1.0]
            .into_iter()
            .enumerate()
        {
            let note = rt.trigger(input(id as i32), 60, velocity).unwrap();
            assert_eq!(rt.note(note).unwrap().1, velocity);
            let mut output = [[0.; 2]; 2];
            rt.render(&mut output).unwrap();
            assert_eq!(output, [[0.5 * velocity as f32; 2]; 2]);
            assert_eq!(rt.voice_count(), 0);
            rt.key_up(note, Some(velocity)).unwrap();
            // Constant key release remains audible at zero. The gate layer is
            // selected by raw velocity, then independently applies its square.
            let expected = if velocity < 0.25 {
                0.5
            } else {
                0.5 + 0.5 * (velocity * velocity) as f32
            };
            rt.render(&mut output).unwrap();
            assert_eq!(output, [[expected; 2]; 2]);
            rt.flush_ended(|_| true);
            assert_eq!(
                (rt.note_count(), rt.voice_count(), rt.family_count()),
                (0, 0, 0)
            );
        }
    });
    for curve in [
        VelocityCurve::Power(f64::NAN),
        VelocityCurve::Power(f64::INFINITY),
        VelocityCurve::Power(f64::NEG_INFINITY),
        VelocityCurve::Power(-1.0),
        VelocityCurve::Power(0.0),
    ] {
        assert!(matches!(
            prepared(
                vec![region(0)],
                vec![Trigger::Attack],
                ReleaseOptions::default(),
                1
            )
            .with_velocity_curves(vec![curve]),
            Err(Error::InvalidInput)
        ));
    }
    for curves in [vec![], vec![VelocityCurve::Linear; 2]] {
        assert!(matches!(
            prepared(
                vec![region(0)],
                vec![Trigger::Attack],
                ReleaseOptions::default(),
                1
            )
            .with_velocity_curves(curves),
            Err(Error::InvalidInput)
        ));
    }
}

#[test]
fn deferred_release_owns_layer_reserves_and_replaces_full_timeline_deadlines() {
    use sampler_core::{Instruction, Outcome, Program};
    let make = || {
        prepared(
            vec![
                Region {
                    gain: 0.25,
                    ..region(0)
                },
                Region {
                    gain: 0.5,
                    ..region(0)
                },
                Region {
                    gain: 1.,
                    ..region(0)
                },
            ],
            vec![Trigger::Attack, Trigger::KeyRelease, Trigger::GateRelease],
            ReleaseOptions::default(),
            16,
        )
        .with_programs(
            vec![
                Program::new(vec![Instruction::SuppressRelease])
                    .unwrap()
                    .with_wait_lifetime(sampler_core::WaitLifetime::Callback),
            ],
            None,
        )
        .unwrap()
        .with_release_program(0)
        .unwrap()
    };
    for hard in [false, true] {
        let mut rt = Runtime::new(
            make(),
            Limits {
                commands: 1,
                behaviors: 2,
                behavior_fuel: 8,
                ..limits()
            },
        )
        .unwrap();
        support::without_heap(|| {
            let a = rt.trigger(input(0), 60, 1.).unwrap();
            let b = rt.trigger(input(1), 60, 1.).unwrap();
            assert_eq!(rt.suppress_release(a), Err(Error::InvalidInput));
            rt.key_up(a, Some(0.5)).unwrap();
            rt.all_notes_off(input(0).channel_address()).unwrap();
            assert!(!rt.key_down(a).unwrap() && !rt.key_down(b).unwrap());
            assert!(rt.note(a).unwrap().2 && rt.note(b).unwrap().2);
            assert_eq!(
                rt.release_status(a, Trigger::KeyRelease),
                Ok(ReleaseStatus::Pending)
            );
            rt.replace_release_forward_at(a, 8).unwrap();
            assert_eq!(rt.replace_release_forward_at(b, 4), Err(Error::Capacity));
            rt.replace_release_forward_at(a, 5).unwrap();
            let mut audio = [[0.; 2]; 6];
            rt.render(&mut audio).unwrap();
            assert_eq!(audio[..5], [[0.5; 2]; 5]);
            assert_eq!(audio[5], [1.75; 2]);
            assert_eq!(rt.release_context(a).unwrap().key.unwrap().at, 0);
            assert_eq!(rt.release_context(a).unwrap().gate.unwrap().at, 5);
            assert!(!rt.suppress_release(a).unwrap());
            assert!(!rt.resume_release(a).unwrap());
            if hard {
                rt.replace_release_forward_at(b, 10).unwrap();
                rt.panic();
            } else {
                rt.release(b).unwrap(); // Explicit forced closure returns a held key-release reserve.
                assert_eq!(
                    rt.release_status(b, Trigger::KeyRelease),
                    Ok(ReleaseStatus::Suppressed)
                );
                rt.render(&mut [[0.; 2]; 32]).unwrap();
            }
            assert_eq!(rt.release_reserve(), ReleaseReserve::default());
            rt.flush_behaviors(|_, _, outcome| {
                assert_eq!(outcome, Outcome::Finished);
                true
            });
            rt.flush_ended(|_| true);
            assert_eq!((rt.note_count(), rt.pending_commands()), (0, 0));
        });
    }
}
