use sampler_core::{
    Envelope, Error, Event, Input, Instruction, Limits, Outcome, Pcm, Playback, Prepared, Program,
    Protocol, Region, Runtime,
};
mod support;
fn input() -> Input {
    Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(7),
    }
}
fn limits() -> Limits {
    Limits {
        notes: 8,
        channels: 1,
        performances: 1,
        families: 8,
        decisions: 0,
        expressions: 8,
        voices: 8,
        commands: 8,
        behaviors: 4,
        behavior_fuel: 8,
        behavior_cells: 16,
        note_cells: 0,
    }
}
fn runtime(code: Vec<Instruction>, limits: Limits) -> Runtime {
    runtime_with_lifetime(code, limits, sampler_core::WaitLifetime::Gate)
}
fn runtime_with_lifetime(
    code: Vec<Instruction>,
    limits: Limits,
    lifetime: sampler_core::WaitLifetime,
) -> Runtime {
    let plan = Prepared::new(
        48000,
        vec![Pcm::new(48000, vec![[1.; 2]; 64].into_boxed_slice()).unwrap()],
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
    .unwrap()
    .with_programs(
        vec![Program::new(code).unwrap().with_wait_lifetime(lifetime)],
        None,
    )
    .unwrap();
    Runtime::new(plan, limits).unwrap()
}
fn play(duration: u32) -> Instruction {
    Instruction::Play {
        transpose: 0,
        velocity: sampler_core::Velocity::Scale(0.5),
        inheritance: sampler_core::Inheritance::Linked,
        duration: sampler_core::Duration::FramesOrGate(duration),
    }
}

#[test]
fn native_waits_generated_notes_and_terminal_backpressure_are_sample_exact() {
    for block in 1..=32 {
        let mut rt = runtime(
            vec![
                play(4),
                Instruction::Wait(6),
                play(3),
                Instruction::Wait(4),
                Instruction::End,
            ],
            limits(),
        );
        let mut audio = [[0.; 2]; 32];
        support::without_heap(|| {
            // No default trigger: the original note is suppressed, generated notes
            // alone select prepared regions. The behavior retains its input owner.
            let note = rt.note_on(input(), 60, 1.).unwrap();
            let behavior = rt.start_behavior(note, 0).unwrap();
            assert_eq!(rt.behavior_outcome(behavior), Ok(None));
            assert_eq!(rt.unpin(note), Err(Error::InvalidInput));
            for chunk in audio.chunks_mut(block) {
                rt.render(chunk).unwrap();
            }
            assert_eq!(rt.behavior_outcome(behavior), Ok(Some(Outcome::Finished)));
            rt.release(note).unwrap();
            rt.flush_behaviors(|_, _, _| false);
            rt.flush_ended(|_| panic!("unaccepted completion retains original input"));
            let mut completed = 0;
            rt.flush_behaviors(|id, owner, outcome| {
                assert_eq!((id, owner, outcome), (behavior, note, Outcome::Finished));
                completed += 1;
                true
            });
            let mut ends = 0;
            rt.flush_ended(|ended| {
                assert_eq!(ended, input());
                ends += 1;
                true
            });
            assert_eq!(
                (
                    completed,
                    ends,
                    rt.note_count(),
                    rt.voice_count(),
                    rt.pending_commands()
                ),
                (1, 1, 0, 0, 0)
            );
            assert_eq!(rt.behavior_outcome(behavior), Err(Error::StaleHandle));
        });
        for (i, frame) in audio.iter().enumerate() {
            let value = if i < 4 || (6..9).contains(&i) {
                0.5
            } else {
                0.
            };
            assert_eq!(*frame, [value; 2], "block {block}, frame {i}");
        }
    }
}

#[test]
fn wait_cancellation_panic_and_explicit_abort_need_no_queue_space() {
    for action in 0..3 {
        let mut rt = runtime(
            vec![play(20), Instruction::Wait(10), play(4)],
            Limits {
                commands: 2,
                ..limits()
            },
        );
        support::without_heap(|| {
            let note = rt.note_on(input(), 60, 1.).unwrap();
            let behavior = rt.start_behavior(note, 0).unwrap();
            assert_eq!(rt.pending_commands(), 2);
            rt.render(&mut [[0.; 2]; 2]).unwrap();
            match action {
                0 => rt.release(note).unwrap(),
                1 => rt.panic(),
                _ => rt.cancel_behavior(behavior).unwrap(),
            }
            let cause = match action {
                0 => sampler_core::ReleaseCause::Explicit,
                1 => sampler_core::ReleaseCause::Panic,
                _ => sampler_core::ReleaseCause::BehaviorCancelled,
            };
            assert_eq!(
                rt.release_context(note).unwrap().gate,
                Some(sampler_core::GateRelease { at: 2, cause })
            );
            assert_eq!(rt.pending_commands(), 0);
            assert_eq!(rt.behavior_outcome(behavior), Ok(Some(Outcome::Cancelled)));
            let mut audio = [[123.; 2]; 32];
            rt.render(&mut audio).unwrap();
            assert_eq!(audio, [[0.; 2]; 32]);
            rt.flush_ended(|_| panic!("completion owns a private pin"));
            rt.flush_behaviors(|_, _, _| true);
            rt.flush_ended(|_| true);
            assert_eq!((rt.note_count(), rt.expression_count()), (0, 0));
        });
    }
}

#[test]
fn faults_and_fuel_are_observable_and_cannot_leave_partial_owned_work() {
    let cases = [
        (
            vec![play(20), Instruction::Wait(1)],
            Limits {
                commands: 1,
                ..limits()
            },
            Outcome::Fault(Error::Capacity),
        ),
        (
            vec![play(20), Instruction::Wait(0), Instruction::End],
            Limits {
                behavior_fuel: 1,
                behavior_cells: 0,
                note_cells: 0,
                ..limits()
            },
            Outcome::FuelExhausted,
        ),
        (
            vec![Instruction::Play {
                transpose: 127,
                velocity: sampler_core::Velocity::Scale(1.),
                inheritance: sampler_core::Inheritance::Linked,
                duration: sampler_core::Duration::FramesOrGate(1),
            }],
            limits(),
            Outcome::Fault(Error::InvalidInput),
        ),
        (
            vec![play(20)],
            Limits {
                voices: 0,
                ..limits()
            },
            Outcome::Fault(Error::Capacity),
        ),
    ];
    for (code, limits, outcome) in cases {
        let mut rt = runtime(code, limits);
        support::without_heap(|| {
            let note = rt.note_on(input(), 60, 1.).unwrap();
            let id = rt.start_behavior(note, 0).unwrap();
            assert_eq!(rt.behavior_outcome(id), Ok(Some(outcome)));
            assert_eq!(rt.pending_commands(), 0);
            rt.flush_behaviors(|_, owner, result| {
                assert_eq!((owner, result), (note, outcome));
                true
            });
            rt.flush_ended(|_| true);
            assert_eq!(
                (
                    rt.note_count(),
                    rt.voice_count(),
                    rt.family_count(),
                    rt.expression_count()
                ),
                (0, 0, 0, 0)
            );
        });
    }
    let mut rt = runtime(
        vec![Instruction::Wait(0)],
        Limits {
            behavior_fuel: 1,
            behavior_cells: 0,
            note_cells: 0,
            ..limits()
        },
    );
    let note = rt.note_on(input(), 60, 1.).unwrap();
    let id = rt.start_behavior(note, 0).unwrap();
    assert_eq!(rt.behavior_outcome(id), Ok(Some(Outcome::Finished)));
    assert!(
        Program::new(vec![Instruction::Play {
            transpose: 0,
            velocity: sampler_core::Velocity::Scale(f64::NAN),
            inheritance: sampler_core::Inheritance::Linked,
            duration: sampler_core::Duration::FramesOrGate(1)
        }])
        .is_err()
    );
}

#[test]
fn resumed_commands_finish_before_later_equal_time_events_and_end_is_exclusive() {
    let mut rt = runtime(
        vec![Instruction::Wait(4), play(8), Instruction::End],
        limits(),
    );
    let note = rt.note_on(input(), 60, 1.).unwrap();
    let id = rt.start_behavior(note, 0).unwrap();
    rt.schedule_event(4, Event::Release(note)).unwrap();
    rt.render(&mut [[0.; 2]; 4]).unwrap();
    assert_eq!(rt.behavior_outcome(id), Ok(None));
    rt.render(&mut []).unwrap();
    // Without the non-reentrant queue guard, Play's service calls would drain the
    // later release first, turning the callback into an erroneous ClosedNote fault.
    assert_eq!(rt.behavior_outcome(id), Ok(Some(Outcome::Finished)));
    assert_eq!(rt.pending_commands(), 0);
    rt.flush_behaviors(|_, _, _| true);
    rt.flush_ended(|_| true);
    assert_eq!(rt.note_count(), 0);
}

#[test]
fn bound_suppression_and_completion_capacity_do_not_publish_partial_inputs() {
    let plan = Prepared::new(48000, vec![], vec![], 0)
        .unwrap()
        .with_programs(vec![Program::new(vec![]).unwrap()], Some(0))
        .unwrap();
    let mut rt = Runtime::new(
        plan,
        Limits {
            behaviors: 1,
            ..limits()
        },
    )
    .unwrap();
    let mut other = runtime(vec![], limits());
    support::without_heap(|| {
        let note = rt.trigger(input(), 60, 1.).unwrap();
        assert_eq!((rt.note_count(), rt.voice_count()), (1, 0));
        assert_eq!(
            rt.trigger(
                Input {
                    external_id: Some(8),
                    ..input()
                },
                60,
                1.
            ),
            Err(Error::Capacity)
        );
        assert_eq!(rt.note_count(), 1);
        rt.release(note).unwrap();
        rt.flush_ended(|_| panic!("completion retains suppressed input"));
        rt.flush_behaviors(|id, owner, outcome| {
            assert_eq!((owner, outcome), (note, Outcome::Finished));
            assert_eq!(other.behavior_outcome(id), Err(Error::StaleHandle));
            assert_eq!(other.cancel_behavior(id), Err(Error::StaleHandle));
            true
        });
        rt.flush_ended(|_| true);
        assert_eq!(rt.note_count(), 0);
    });
    assert!(
        Prepared::new(48000, vec![], vec![], 0)
            .unwrap()
            .with_programs(vec![], Some(0))
            .is_err()
    );
}

#[test]
fn deferred_fault_is_reported_at_the_resume_boundary() {
    let mut rt = runtime(
        vec![
            Instruction::Wait(4),
            Instruction::Play {
                transpose: 127,
                velocity: sampler_core::Velocity::Scale(1.),
                inheritance: sampler_core::Inheritance::Linked,
                duration: sampler_core::Duration::FramesOrGate(1),
            },
        ],
        limits(),
    );
    support::without_heap(|| {
        let note = rt.note_on(input(), 60, 1.).unwrap();
        let id = rt.start_behavior(note, 0).unwrap();
        rt.render(&mut [[0.; 2]; 4]).unwrap();
        assert_eq!(rt.behavior_outcome(id), Ok(None));
        rt.render(&mut []).unwrap();
        assert_eq!(
            rt.behavior_outcome(id),
            Ok(Some(Outcome::Fault(Error::InvalidInput)))
        );
        assert!(!rt.note(note).unwrap().2);
        assert_eq!(
            rt.release_context(note).unwrap().gate,
            Some(sampler_core::GateRelease {
                at: 4,
                cause: sampler_core::ReleaseCause::BehaviorFault
            })
        );
        assert_eq!(rt.pending_commands(), 0);
        rt.flush_behaviors(|_, _, _| true);
        rt.flush_ended(|_| true);
        assert_eq!(rt.note_count(), 0);
    });
}

#[test]
fn callback_locals_survive_waits_and_remain_polyphonically_isolated() {
    let code = vec![
        Instruction::ReadKey { local: 0 },
        Instruction::AddLocal {
            local: 0,
            value: -59,
        },
        play(2),
        Instruction::Wait(3),
        Instruction::AddLocal {
            local: 0,
            value: -1,
        },
        Instruction::JumpIfZero {
            local: 0,
            target: 7,
        },
        Instruction::Jump { target: 2 },
        Instruction::End,
    ];
    for block in 1..=16 {
        let mut rt = runtime(code.clone(), limits());
        let mut audio = [[0.; 2]; 16];
        support::without_heap(|| {
            let a = rt.note_on(input(), 60, 1.).unwrap();
            let b = rt
                .note_on(
                    Input {
                        external_id: Some(8),
                        ..input()
                    },
                    61,
                    1.,
                )
                .unwrap();
            let x = rt.start_behavior(a, 0).unwrap();
            let y = rt.start_behavior(b, 0).unwrap();
            assert_eq!(
                (rt.behavior_local(x, 0), rt.behavior_local(y, 0)),
                (Ok(1), Ok(2))
            );
            assert_eq!(rt.behavior_local(x, 1), Err(Error::InvalidInput));
            for chunk in audio.chunks_mut(block) {
                rt.render(chunk).unwrap();
            }
            assert_eq!(
                (rt.behavior_local(x, 0), rt.behavior_local(y, 0)),
                (Ok(0), Ok(0))
            );
            assert_eq!(rt.behavior_outcome(x), Ok(Some(Outcome::Finished)));
            assert_eq!(rt.behavior_outcome(y), Ok(Some(Outcome::Finished)));
            rt.panic();
            rt.flush_behaviors(|_, _, _| true);
            assert_eq!(rt.behavior_local(x, 0), Err(Error::StaleHandle));
            let mut ends = 0;
            rt.flush_ended(|_| {
                ends += 1;
                true
            });
            assert_eq!((ends, rt.note_count()), (2, 0));
        });
        for (i, frame) in audio.iter().enumerate() {
            let gain = if i < 2 {
                1.
            } else if (3..5).contains(&i) {
                0.5
            } else {
                0.
            };
            assert_eq!(*frame, [gain; 2], "block {block}, frame {i}");
        }
    }
}

#[test]
fn local_storage_is_budgeted_and_zeroed_on_callback_slot_reuse() {
    let programs = vec![
        Program::new(vec![Instruction::SetLocal {
            local: 0,
            value: 99,
        }])
        .unwrap(),
        Program::new(vec![
            Instruction::Wait(1),
            Instruction::AddLocal { local: 0, value: 1 },
        ])
        .unwrap(),
    ];
    let plan = Prepared::new(48000, vec![], vec![], 0)
        .unwrap()
        .with_programs(programs, None)
        .unwrap();
    let mut rt = Runtime::new(
        plan,
        Limits {
            behaviors: 1,
            behavior_cells: 1,
            note_cells: 0,
            ..limits()
        },
    )
    .unwrap();
    support::without_heap(|| {
        let note = rt.note_on(input(), 60, 1.).unwrap();
        let old = rt.start_behavior(note, 0).unwrap();
        assert_eq!(rt.behavior_local(old, 0), Ok(99));
        rt.flush_behaviors(|_, _, _| true);
        let fresh = rt.start_behavior(note, 1).unwrap();
        assert_ne!(old, fresh);
        assert_eq!(rt.behavior_local(old, 0), Err(Error::StaleHandle));
        assert_eq!(rt.behavior_local(fresh, 0), Ok(0));
        rt.render(&mut [[0.; 2]; 1]).unwrap();
        assert_eq!(rt.behavior_local(fresh, 0), Ok(0));
        rt.render(&mut []).unwrap();
        assert_eq!(rt.behavior_local(fresh, 0), Ok(1));
        assert_eq!(rt.behavior_outcome(fresh), Ok(Some(Outcome::Finished)));
        rt.panic();
        rt.flush_behaviors(|_, _, _| true);
        rt.flush_ended(|_| true);
    });
    let plan = Prepared::new(48000, vec![], vec![], 0)
        .unwrap()
        .with_programs(
            vec![Program::new(vec![Instruction::ReadKey { local: u16::MAX }]).unwrap()],
            None,
        )
        .unwrap();
    assert!(matches!(Runtime::new(plan, limits()), Err(Error::Capacity)));
    assert!(Program::new(vec![Instruction::Jump { target: 2 }]).is_err());
    assert!(
        Program::new(vec![Instruction::JumpIfZero {
            local: 0,
            target: 2
        }])
        .is_err()
    );
}

#[test]
fn branching_loops_obey_fuel_and_integer_arithmetic_never_wraps() {
    let cases = [
        (
            vec![Instruction::Jump { target: 0 }],
            Outcome::FuelExhausted,
        ),
        (
            vec![
                Instruction::SetLocal {
                    local: 0,
                    value: i64::MAX,
                },
                Instruction::AddLocal { local: 0, value: 1 },
            ],
            Outcome::Fault(Error::ArithmeticOverflow),
        ),
        (
            vec![
                Instruction::SetLocal {
                    local: 0,
                    value: i64::MIN,
                },
                Instruction::AddLocal {
                    local: 0,
                    value: -1,
                },
            ],
            Outcome::Fault(Error::ArithmeticOverflow),
        ),
    ];
    for (code, outcome) in cases {
        let mut rt = runtime(code, limits());
        support::without_heap(|| {
            let note = rt.note_on(input(), 60, 1.).unwrap();
            let id = rt.start_behavior(note, 0).unwrap();
            assert_eq!(rt.behavior_outcome(id), Ok(Some(outcome)));
            assert_eq!(rt.pending_commands(), 0);
            rt.flush_behaviors(|_, _, _| true);
            rt.flush_ended(|_| true);
            assert_eq!(rt.note_count(), 0);
        });
    }
    let mut rt = runtime(
        vec![Instruction::Wait(1), Instruction::Jump { target: 0 }],
        limits(),
    );
    support::without_heap(|| {
        let note = rt.note_on(input(), 60, 1.).unwrap();
        let id = rt.start_behavior(note, 0).unwrap();
        rt.render(&mut [[0.; 2]; 64]).unwrap();
        assert_eq!(rt.behavior_outcome(id), Ok(None));
        assert_eq!(rt.pending_commands(), 1);
        rt.release(note).unwrap();
        assert_eq!(rt.behavior_outcome(id), Ok(Some(Outcome::Cancelled)));
        assert_eq!(rt.pending_commands(), 0);
        rt.flush_behaviors(|_, _, _| true);
        rt.flush_ended(|_| true);
    });
}

#[test]
fn callback_retention_can_outlive_input_release_without_orphaning_generated_notes() {
    use sampler_core::{Duration, WaitLifetime};
    for lifetime in [WaitLifetime::Gate, WaitLifetime::Callback] {
        for block in 1..=16 {
            let mut rt = runtime_with_lifetime(
                vec![
                    Instruction::Wait(4),
                    Instruction::Play {
                        transpose: 0,
                        velocity: sampler_core::Velocity::Scale(0.5),
                        inheritance: sampler_core::Inheritance::Linked,
                        duration: Duration::Frames(4),
                    },
                    Instruction::End,
                ],
                limits(),
                lifetime,
            );
            let mut audio = [[0.; 2]; 16];
            support::without_heap(|| {
                let note = rt.note_on(input(), 60, 1.).unwrap();
                let id = rt.start_behavior(note, 0).unwrap();
                rt.release_at(note, 2).unwrap();
                for chunk in audio.chunks_mut(block) {
                    rt.render(chunk).unwrap();
                }
                let expected = if lifetime == WaitLifetime::Gate {
                    Outcome::Cancelled
                } else {
                    Outcome::Finished
                };
                assert_eq!(rt.behavior_outcome(id), Ok(Some(expected)));
                rt.flush_ended(|_| panic!("completion still owns the original note"));
                rt.flush_behaviors(|_, _, _| true);
                let mut ends = 0;
                rt.flush_ended(|ended| {
                    assert_eq!(ended, input());
                    ends += 1;
                    true
                });
                assert_eq!((ends, rt.note_count(), rt.pending_commands()), (1, 0, 0));
            });
            for (frame, value) in audio.iter().enumerate() {
                let gain = if lifetime == WaitLifetime::Callback && (4..8).contains(&frame) {
                    0.5
                } else {
                    0.
                };
                assert_eq!(
                    *value, [gain; 2],
                    "{lifetime:?}, block {block}, frame {frame}"
                );
            }
        }
    }
}

#[test]
fn generated_duration_and_gate_are_independent_policies() {
    use sampler_core::Duration;
    for duration in [
        Duration::Gate,
        Duration::Frames(4),
        Duration::FramesOrGate(4),
    ] {
        let mut rt = runtime(
            vec![Instruction::Play {
                transpose: 0,
                velocity: sampler_core::Velocity::Scale(1.),
                inheritance: sampler_core::Inheritance::Linked,
                duration,
            }],
            limits(),
        );
        support::without_heap(|| {
            let note = rt.note_on(input(), 60, 1.).unwrap();
            rt.start_behavior(note, 0).unwrap();
            rt.release_at(note, 2).unwrap();
            let mut audio = [[0.; 2]; 8];
            rt.render(&mut audio).unwrap();
            let end = if duration == Duration::Frames(4) {
                4
            } else {
                2
            };
            for (i, frame) in audio.iter().enumerate() {
                assert_eq!(*frame, [if i < end { 1. } else { 0. }; 2]);
            }
            rt.flush_behaviors(|_, _, _| true);
            rt.flush_ended(|_| true);
            assert_eq!(rt.note_count(), 0);
        });
    }
}

#[test]
fn retained_callbacks_on_closed_notes_still_cancel_on_panic_or_explicit_abort() {
    use sampler_core::WaitLifetime;
    for panic in [false, true] {
        let mut rt = runtime_with_lifetime(
            vec![Instruction::Wait(10), play(2)],
            limits(),
            WaitLifetime::Callback,
        );
        support::without_heap(|| {
            let note = rt.note_on(input(), 60, 1.).unwrap();
            rt.release(note).unwrap();
            // A release-side caller can start work while it still owns the closed ID.
            let id = rt.start_behavior(note, 0).unwrap();
            assert_eq!(rt.behavior_outcome(id), Ok(None));
            if panic {
                rt.panic();
            } else {
                rt.cancel_behavior(id).unwrap();
            }
            assert_eq!(rt.behavior_outcome(id), Ok(Some(Outcome::Cancelled)));
            assert_eq!(rt.pending_commands(), 0);
            rt.render(&mut [[0.; 2]; 32]).unwrap();
            rt.flush_ended(|_| panic!("cancelled outcome still owns the note"));
            rt.flush_behaviors(|_, _, _| true);
            rt.flush_ended(|_| true);
            assert_eq!(rt.note_count(), 0);
        });
    }
}

#[test]
fn independent_duration_keeps_ownership_after_callback_fault_until_its_release() {
    let mut rt = runtime(
        vec![
            Instruction::Play {
                transpose: 0,
                velocity: sampler_core::Velocity::Scale(1.),
                inheritance: sampler_core::Inheritance::Linked,
                duration: sampler_core::Duration::Frames(4),
            },
            Instruction::Wait(1),
        ],
        Limits {
            commands: 1,
            ..limits()
        },
    );
    support::without_heap(|| {
        let note = rt.note_on(input(), 60, 1.).unwrap();
        let id = rt.start_behavior(note, 0).unwrap();
        assert_eq!(
            rt.behavior_outcome(id),
            Ok(Some(Outcome::Fault(Error::Capacity)))
        );
        rt.flush_behaviors(|_, _, _| true);
        rt.flush_ended(|_| panic!("independent duration still retains its input ancestor"));
        let mut audio = [[0.; 2]; 8];
        rt.render(&mut audio).unwrap();
        assert_eq!(audio[..4], [[1.; 2]; 4]);
        assert_eq!(audio[4..], [[0.; 2]; 4]);
        let mut ends = 0;
        rt.flush_ended(|_| {
            ends += 1;
            true
        });
        assert_eq!((ends, rt.note_count(), rt.pending_commands()), (1, 0, 0));
    });
}

#[test]
fn generated_note_slots_are_reused_inside_a_block_without_consuming_host_terminals() {
    for (block, notes, expressions) in [1, 7, 64, 256]
        .into_iter()
        .flat_map(|block| [(block, 3, 8), (block, 8, 3)])
    {
        let mut budget = limits();
        budget.notes = notes;
        budget.expressions = expressions;
        let mut rt = runtime(
            vec![
                Instruction::SetLocal {
                    local: 0,
                    value: 100,
                },
                Instruction::Play {
                    transpose: 0,
                    velocity: sampler_core::Velocity::Fixed(0.5),
                    inheritance: sampler_core::Inheritance::Independent,
                    duration: sampler_core::Duration::Frames(1),
                },
                Instruction::Wait(2),
                Instruction::AddLocal {
                    local: 0,
                    value: -1,
                },
                Instruction::JumpIfZero {
                    local: 0,
                    target: 6,
                },
                Instruction::Jump { target: 1 },
                Instruction::End,
            ],
            budget,
        );
        let mut audio = [[0.; 2]; 256];
        support::without_heap(|| {
            // A rejected host terminal in the lowest slot must neither be consumed
            // nor prevent reclaiming an unrelated completed generated child.
            let earlier = Input {
                external_id: Some(6),
                ..input()
            };
            let pending = rt.note_on(earlier, 60, 1.).unwrap();
            rt.release(pending).unwrap();
            let root = rt.note_on(input(), 60, 1.).unwrap();
            let behavior = rt.start_behavior(root, 0).unwrap();
            for chunk in audio.chunks_mut(block) {
                rt.render(chunk).unwrap();
                rt.flush_ended(|_| false);
            }
            assert_eq!(rt.behavior_outcome(behavior), Ok(Some(Outcome::Finished)));
            assert!(
                rt.note(pending).is_ok(),
                "host terminal must still own its slot"
            );
            rt.release(root).unwrap();
            let mut completions = 0;
            rt.flush_behaviors(|_, _, outcome| {
                assert_eq!(outcome, Outcome::Finished);
                completions += 1;
                true
            });
            let mut terminals = [None; 2];
            let mut count = 0;
            rt.flush_ended(|origin| {
                terminals[count] = Some(origin);
                count += 1;
                true
            });
            assert_eq!(terminals, [Some(earlier), Some(input())]);
            assert_eq!((completions, count, rt.note_count()), (1, 2, 0));
            assert_eq!(
                (rt.family_count(), rt.expression_count(), rt.voice_count()),
                (0, 0, 0)
            );
        });
        for (frame, actual) in audio.iter().enumerate() {
            let expected = if frame < 200 && frame % 2 == 0 {
                0.5
            } else {
                0.
            };
            assert_eq!(*actual, [expected; 2], "block {block}, frame {frame}");
        }
    }
}

#[test]
fn scoped_hard_silence_cancels_descendants_and_retained_waits_but_keeps_physical_keys() {
    use sampler_core::{Duration, Expression, Inheritance, Velocity, WaitLifetime};
    for block in [1, 3, 16] {
        let generated = Instruction::Play {
            transpose: 0,
            velocity: Velocity::Fixed(0.5),
            inheritance: Inheritance::Independent,
            duration: Duration::Frames(100),
        };
        let mut rt = runtime_with_lifetime(
            vec![generated, Instruction::Wait(8), generated, Instruction::End],
            limits(),
            WaitLifetime::Callback,
        );
        support::without_heap(|| {
            let root = rt.note_on(input(), 60, 1.).unwrap();
            let other_input = Input {
                channel: 1,
                ..input()
            };
            let other = rt.note_on(other_input, 60, 1.).unwrap();
            let own_callback = rt.start_behavior(root, 0).unwrap();
            let other_callback = rt.start_behavior(other, 0).unwrap();
            let tail = rt
                .child(root, 60, 1., false, Inheritance::Independent)
                .unwrap();
            let family = rt.create_family(tail).unwrap();
            rt.start_family(
                family,
                0,
                0,
                1.,
                Envelope::new(0, 0, 0, 1., 32).unwrap(),
                Playback::default(),
            )
            .unwrap();
            rt.finish_family(family).unwrap();
            let delayed = rt
                .child(tail, 60, 1., false, Inheritance::Independent)
                .unwrap();
            rt.start(delayed, 0, 4, 1.).unwrap();
            let channel = rt.register_channel(input().channel_address()).unwrap();
            rt.sustain(channel, true).unwrap();
            rt.schedule_event(10, Event::KeyUp(root, None)).unwrap();
            rt.schedule_event(12, Event::Expression(root, Expression::default()))
                .unwrap();
            rt.schedule_event(12, Event::Expression(other, Expression::default()))
                .unwrap();
            assert_eq!(rt.pending_commands(), 8);
            rt.render(&mut [[0.; 2]; 1]).unwrap();
            rt.release(tail).unwrap();
            assert_eq!(
                rt.voice_count(),
                4,
                "tail and delayed descendant still own voices"
            );
            assert_eq!(rt.all_sound_off(input().channel_address()), Ok(3));
            assert_eq!(rt.all_sound_off(input().channel_address()), Ok(0));
            assert_eq!(rt.voice_count(), 1);
            assert_eq!(
                rt.pending_commands(),
                4,
                "physical key-up survives hard silence"
            );
            assert_eq!(
                rt.behavior_outcome(own_callback),
                Ok(Some(Outcome::Cancelled))
            );
            assert_eq!(rt.behavior_outcome(other_callback), Ok(None));
            rt.flush_behaviors(|id, _, outcome| {
                assert_eq!((id, outcome), (own_callback, Outcome::Cancelled));
                true
            });
            rt.flush_ended(|_| panic!("silenced physical key must remain paired"));
            assert_eq!(rt.key_down(root), Ok(true));
            assert!(!rt.note(root).unwrap().2);
            assert_eq!(rt.pedals(channel), Ok((true, false)));
            let mut audio = [[0.; 2]; 16];
            let mut terminals = 0;
            for chunk in audio.chunks_mut(block) {
                rt.render(chunk).unwrap();
                rt.flush_ended(|origin| {
                    assert_eq!(origin, input());
                    terminals += 1;
                    true
                });
            }
            assert_eq!(terminals, 1);
            for (frame, sample) in audio.iter().enumerate() {
                assert_eq!(*sample, [if frame < 7 { 0.5 } else { 1. }; 2]);
            }
            assert_eq!(
                rt.behavior_outcome(other_callback),
                Ok(Some(Outcome::Finished))
            );
            rt.panic();
            rt.flush_behaviors(|_, _, _| true);
            rt.flush_ended(|origin| {
                assert_eq!(origin, other_input);
                true
            });
            assert_eq!(
                (
                    rt.note_count(),
                    rt.expression_count(),
                    rt.pending_commands()
                ),
                (0, 0, 0)
            );
        });
    }
}

#[test]
fn generated_source_pitch_preflight_preserves_accepted_future_expression() {
    use sampler_core::{Duration, Expression, Inheritance, Velocity};
    for inheritance in [
        Inheritance::Linked,
        Inheritance::Snapshot,
        Inheritance::Independent,
    ] {
        let mut rt = runtime(
            vec![
                Instruction::Wait(1),
                Instruction::Play {
                    transpose: 0,
                    velocity: Velocity::Fixed(1.0),
                    inheritance,
                    duration: Duration::Frames(4),
                },
                Instruction::End,
            ],
            limits(),
        );
        support::without_heap(|| {
            let root = rt.note_on(input(), 60, 1.0).unwrap();
            // With no admitted source, retaining this native value is valid. A
            // subsequently linked source cannot support it and must fail before
            // creating a child/family; snapshots do not inherit future changes.
            rt.schedule_event(
                10,
                Event::Expression(
                    root,
                    Expression {
                        pitch_semitones: 120.0,
                        ..Expression::default()
                    },
                ),
            )
            .unwrap();
            let behavior = rt.start_behavior(root, 0).unwrap();
            let mut audio = [[0.0; 2]; 2];
            rt.render(&mut audio).unwrap();
            let linked = inheritance == Inheritance::Linked;
            assert_eq!(
                rt.behavior_outcome(behavior),
                Ok(Some(if linked {
                    Outcome::Fault(Error::InvalidInput)
                } else {
                    Outcome::Finished
                }))
            );
            assert_eq!(rt.note_count(), if linked { 1 } else { 2 });
            assert_eq!(rt.voice_count(), usize::from(!linked));
            assert_eq!(audio, [[0.0; 2], [if linked { 0.0 } else { 1.0 }; 2]]);
            rt.render(&mut [[0.0; 2]; 16]).unwrap();
            assert_eq!(
                rt.expression(rt.expression_id(root).unwrap())
                    .unwrap()
                    .pitch_semitones,
                if linked { 0.0 } else { 120.0 }
            );
            rt.flush_behaviors(|_, _, _| true);
            rt.key_up(root, None).unwrap();
            rt.flush_ended(|_| true);
            assert_eq!(
                (
                    rt.note_count(),
                    rt.family_count(),
                    rt.voice_count(),
                    rt.pending_commands()
                ),
                (0, 0, 0, 0)
            );
        });
    }
}

#[test]
fn note_cells_outlive_callbacks_and_remain_isolated_until_logical_retirement() {
    let plan = Prepared::new(48000, vec![], vec![], 0)
        .unwrap()
        .with_programs(
            vec![
                Program::new(vec![
                    Instruction::ReadKey { local: 0 },
                    Instruction::WriteNoteCell { cell: 0, local: 0 },
                ])
                .unwrap(),
                Program::new(vec![
                    Instruction::ReadNoteCell { local: 0, cell: 0 },
                    Instruction::Wait(2),
                    Instruction::AddLocal { local: 0, value: 1 },
                    Instruction::WriteNoteCell { cell: 0, local: 0 },
                ])
                .unwrap()
                .with_wait_lifetime(sampler_core::WaitLifetime::Callback),
            ],
            None,
        )
        .unwrap();
    let mut rt = Runtime::new(
        plan,
        Limits {
            notes: 2,
            note_cells: 2,
            ..limits()
        },
    )
    .unwrap();
    let foreign = {
        let mut other = runtime(vec![], limits());
        other.note_on(input(), 60, 1.).unwrap()
    };
    let address = Input {
        external_id: None,
        ..input()
    };
    support::without_heap(|| {
        assert_eq!(rt.note_cell(foreign, 0), Err(Error::StaleHandle));
        for _ in 0..64 {
            let a = rt.note_on(address, 60, 1.).unwrap();
            let b = rt.note_on(address, 60, 1.).unwrap();
            assert_eq!(rt.note_cell(a, 0), Ok(0));
            assert_eq!(rt.note_cell(b, 0), Ok(0));
            assert_eq!(rt.note_cell(a, 1), Err(Error::InvalidInput));
            let writer = rt.start_behavior(a, 0).unwrap();
            assert_eq!(rt.note_cell(a, 0), Ok(60));
            assert_eq!(rt.note_cell(b, 0), Ok(0));
            assert_eq!(rt.note_on(address, 60, 1.), Err(Error::Capacity));
            assert_eq!(rt.note_cell(a, 0), Ok(60));
            rt.flush_behaviors(|_, _, _| true);
            assert_eq!(rt.behavior_local(writer, 0), Err(Error::StaleHandle));
            rt.note_off(address, None).unwrap();
            rt.pin(a).unwrap();
            let reader = rt.start_behavior(a, 1).unwrap();
            let other = rt.start_behavior(b, 1).unwrap();
            assert_eq!(rt.behavior_local(reader, 0), Ok(60));
            assert_eq!(rt.behavior_local(other, 0), Ok(0));
            rt.render(&mut [[0.; 2]; 2]).unwrap();
            assert_eq!(rt.note_cell(a, 0), Ok(60));
            rt.render(&mut []).unwrap();
            assert_eq!(rt.note_cell(a, 0), Ok(61));
            assert_eq!(rt.note_cell(b, 0), Ok(1));
            rt.flush_behaviors(|_, _, _| false);
            assert_eq!(rt.behavior_local(reader, 0), Ok(61));
            rt.flush_behaviors(|_, _, _| true);
            rt.flush_ended(|_| true);
            assert_eq!(rt.note_cell(a, 0), Ok(61));
            rt.unpin(a).unwrap();
            rt.flush_ended(|_| false);
            assert_eq!(rt.note_cell(a, 0), Ok(61));
            rt.flush_ended(|_| true);
            assert_eq!(rt.note_cell(a, 0), Err(Error::StaleHandle));
            rt.note_off(address, None).unwrap();
            rt.flush_ended(|_| true);
            assert_eq!(rt.note_cell(b, 0), Err(Error::StaleHandle));
            assert_eq!(rt.note_count(), 0);
        }
    });
}

#[test]
fn note_cell_layouts_follow_original_plans_and_children_start_with_zero_state() {
    let prepare = |cell| {
        Prepared::new(48000, vec![], vec![], 0)
            .unwrap()
            .with_programs(
                vec![
                    Program::new(vec![
                        Instruction::ReadKey { local: 0 },
                        Instruction::WriteNoteCell { cell, local: 0 },
                    ])
                    .unwrap(),
                ],
                None,
            )
            .unwrap()
    };
    assert!(matches!(
        Runtime::new(prepare(0), limits()),
        Err(Error::Capacity)
    ));
    assert!(matches!(
        Runtime::new(
            prepare(u16::MAX),
            Limits {
                note_cells: 8,
                ..limits()
            }
        ),
        Err(Error::Capacity)
    ));
    assert!(matches!(
        Runtime::new(
            prepare(0),
            Limits {
                note_cells: usize::MAX,
                ..limits()
            }
        ),
        Err(Error::Capacity)
    ));
    let (mut rt, mut control) = Runtime::with_plan_updates(
        prepare(1),
        Limits {
            note_cells: 16,
            ..limits()
        },
        2,
        1,
    )
    .unwrap();
    let rejected = Box::new(prepare(2));
    let pointer = std::ptr::from_ref(rejected.as_ref());
    let rejected = control.submit(rejected).unwrap_err();
    assert_eq!(rejected.reason, sampler_core::PlanError::NoteStateCapacity);
    assert_eq!(std::ptr::from_ref(rejected.prepared.as_ref()), pointer);
    let mut old = None;
    support::without_heap(|| {
        let note = rt.note_on(input(), 60, 1.).unwrap();
        rt.start_behavior(note, 0).unwrap();
        rt.flush_behaviors(|_, _, _| true);
        old = Some(note);
    });
    let old = old.unwrap();
    assert_eq!(control.submit(Box::new(prepare(0))).unwrap(), 1);
    support::without_heap(|| {
        assert_eq!(rt.poll_plan_update(), Ok(Some(1)));
        assert_eq!(rt.note_cell(old, 1), Ok(60));
        let new = rt
            .note_on(
                Input {
                    external_id: Some(8),
                    ..input()
                },
                61,
                1.,
            )
            .unwrap();
        assert_eq!(rt.note_cell(new, 0), Ok(0));
        assert_eq!(rt.note_cell(new, 1), Err(Error::InvalidInput));
        rt.start_behavior(new, 0).unwrap();
        assert_eq!(rt.note_cell(new, 0), Ok(61));
        for inheritance in [
            sampler_core::Inheritance::Independent,
            sampler_core::Inheritance::Snapshot,
            sampler_core::Inheritance::Linked,
        ] {
            let child = rt.child(old, 62, 1., false, inheritance).unwrap();
            assert_eq!(rt.note_plan(child), rt.note_plan(old));
            assert_eq!(rt.note_cell(child, 1), Ok(0));
            rt.start_behavior(child, 0).unwrap();
            assert_eq!(rt.note_cell(child, 1), Ok(62));
            assert_eq!(rt.note_cell(old, 1), Ok(60));
            rt.release(child).unwrap();
            rt.flush_behaviors(|_, _, _| true);
            rt.flush_ended(|_| true);
        }
        rt.release(old).unwrap();
        rt.release(new).unwrap();
        rt.flush_ended(|_| true);
        assert_eq!(rt.note_count(), 0);
        assert_eq!(rt.collect_retired_plans(), 1);
    });
    assert!(control.retired().is_some());
}
