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
    }
}
fn runtime(code: Vec<Instruction>, limits: Limits) -> Runtime {
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
    .with_programs(vec![Program::new(code).unwrap()], None)
    .unwrap();
    Runtime::new(plan, limits).unwrap()
}
fn play(duration: u32) -> Instruction {
    Instruction::Play {
        transpose: 0,
        velocity_scale: 0.5,
        duration,
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
                ..limits()
            },
            Outcome::FuelExhausted,
        ),
        (
            vec![Instruction::Play {
                transpose: 127,
                velocity_scale: 1.,
                duration: 1,
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
            duration: 1
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
                duration: 1,
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
