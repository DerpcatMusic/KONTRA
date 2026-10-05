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
        families: 8,
        expressions: 8,
        voices: 8,
        commands: 8,
        behaviors: 4,
        behavior_fuel: 8,
        behavior_cells: 16,
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
        vec![Pcm {
            rate: 48000,
            frames: vec![[1.; 2]; 64].into_boxed_slice(),
        }],
        vec![Region {
            sample: 0,
            key_low: 60,
            key_high: 61,
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
        velocity_scale: 0.5,
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
                ..limits()
            },
            Outcome::FuelExhausted,
        ),
        (
            vec![Instruction::Play {
                transpose: 127,
                velocity_scale: 1.,
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
            ..limits()
        },
    );
    let note = rt.note_on(input(), 60, 1.).unwrap();
    let id = rt.start_behavior(note, 0).unwrap();
    assert_eq!(rt.behavior_outcome(id), Ok(Some(Outcome::Finished)));
    assert!(
        Program::new(vec![Instruction::Play {
            transpose: 0,
            velocity_scale: f64::NAN,
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
                velocity_scale: 1.,
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
                        velocity_scale: 0.5,
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
                velocity_scale: 1.,
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
                velocity_scale: 1.,
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
