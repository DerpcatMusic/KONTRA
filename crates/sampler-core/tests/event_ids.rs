use sampler_core::*;
mod support;
fn limits(notes: usize) -> Limits {
    Limits {
        notes,
        channels: 0,
        performances: 1,
        expressions: notes,
        families: notes,
        voices: notes,
        decisions: 0,
        commands: notes,
        behaviors: notes,
        behavior_fuel: 64,
        behavior_cells: notes * 4,
        note_cells: 0,
    }
}
fn plan() -> Prepared {
    Prepared::new(48000, vec![], vec![], 0).unwrap()
}
fn input() -> Input {
    Input {
        protocol: Protocol::Clap,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(99),
    }
}

#[test]
fn replacing_note_deadlines_is_atomic_at_capacity_and_keeps_equal_time_pedal_order() {
    let mut capacity = limits(3);
    capacity.commands = 3;
    capacity.channels = 1;
    let mut rt = Runtime::new(plan(), capacity).unwrap();
    support::without_heap(|| {
        let a = rt.note_on(input(), 60, 1.).unwrap();
        let b = rt
            .note_on(
                Input {
                    external_id: Some(100),
                    ..input()
                },
                60,
                1.,
            )
            .unwrap();
        let channel = rt.register_channel(input().channel_address()).unwrap();
        rt.release_at(a, 10).unwrap();
        rt.release_at(a, 12).unwrap();
        rt.schedule_event(5, Event::Sustain(channel, true)).unwrap();
        assert_eq!(rt.replace_key_up_at(b, 5, None), Err(Error::Capacity));
        assert_eq!(
            rt.replace_key_up_at(a, 5, Some(f64::NAN)),
            Err(Error::InvalidInput)
        );
        rt.replace_key_up_at(a, 5, Some(0.25)).unwrap();
        assert_eq!(rt.pending_commands(), 2);
        rt.render(&mut [[0.; 2]; 1]).unwrap();
        assert_eq!(rt.replace_key_up_at(a, 0, None), Err(Error::PastEvent));
        rt.replace_key_up_at(b, 1, None).unwrap(); // Immediate release needs no slot.
        rt.render(&mut [[0.; 2]; 4]).unwrap();
        assert!(rt.key_down(a).unwrap()); // Exclusive end stays pending.
        rt.render(&mut []).unwrap();
        assert!(!rt.key_down(a).unwrap());
        assert!(rt.note(a).unwrap().2); // Existing pedal command ran first.
        assert_eq!(
            rt.release_context(a).unwrap().key,
            Some(KeyRelease {
                at: 5,
                velocity: Some(0.25),
                cause: ReleaseCause::KeyUp,
            })
        );
        assert_eq!(rt.pending_commands(), 0);
        assert_eq!(rt.replace_key_up_at(a, 6, None), Err(Error::ClosedNote));
        rt.sustain(channel, false).unwrap();
        rt.flush_ended(|_| true);
        assert_eq!(rt.note_count(), 0);
        assert_eq!(rt.replace_key_up_at(a, 6, None), Err(Error::StaleHandle));
    });
}

#[test]
fn queued_note_ends_retain_silent_generated_notes_and_cancel_without_leaking_pins() {
    for forced in [false, true] {
        let plan = plan()
            .with_programs(
                vec![
                    Program::new(vec![
                        Instruction::SetLocal {
                            local: 0,
                            value: 60,
                        },
                        Instruction::SetLocal {
                            local: 1,
                            value: 127,
                        },
                        Instruction::PlayMidi {
                            key: 0,
                            velocity: 1,
                            inheritance: Inheritance::Independent,
                            duration: DurationValue::Fixed(Duration::UntilSilent),
                            result: Some(0),
                        },
                    ])
                    .unwrap(),
                ],
                None,
            )
            .unwrap();
        let mut rt = Runtime::new(plan, limits(3)).unwrap();
        support::without_heap(|| {
            let parent = rt.note_on(input(), 60, 1.).unwrap();
            let callback = rt.start_behavior(parent, 0).unwrap();
            let alias = rt.behavior_local(callback, 0).unwrap() as i32;
            let child = rt
                .resolve_source_event(rt.active_plan(), alias)
                .unwrap()
                .unwrap();
            if forced {
                rt.release_at(child, 6).unwrap();
            } else {
                rt.schedule_event(6, Event::KeyUp(child, None)).unwrap();
            }
            rt.flush_behaviors(|_, _, outcome| {
                assert_eq!(outcome, Outcome::Finished);
                true
            });
            rt.flush_ended(|_| true);
            assert_eq!(rt.note_count(), 2); // No source, but its queued end owns it.
            rt.replace_key_up_at(child, 8, None).unwrap();
            rt.render(&mut [[0.; 2]; 7]).unwrap();
            rt.flush_ended(|_| true);
            assert_eq!(rt.note_count(), 2);
            if forced {
                rt.panic();
            } else {
                rt.render(&mut [[0.; 2]; 2]).unwrap();
                assert_eq!(rt.release_context(child).unwrap().key.unwrap().at, 8);
            }
            assert_eq!(rt.pending_commands(), 0);
            rt.flush_ended(|_| true);
            assert_eq!(rt.note_count(), usize::from(!forced));
            rt.panic();
            rt.flush_ended(|_| true);
            assert_eq!(rt.note_count(), 0);
        });
    }
}
#[test]
fn source_ids_are_not_host_ids_and_never_alias_reused_note_slots_or_panic() {
    let mut rt = Runtime::new(plan(), limits(1)).unwrap();
    support::without_heap(|| {
        let plan = rt.active_plan();
        let mut previous = 0;
        for _ in 0..300 {
            let note = rt.note_on(input(), 60, 1.).unwrap();
            let id = rt.source_event_id(note).unwrap();
            assert!(id > previous);
            assert_eq!(rt.source_event_id(note), Ok(id));
            assert_eq!(rt.resolve_source_event(plan, id), Ok(Some(note)));
            assert_eq!(rt.resolve_source_event(plan, previous), Ok(None));
            for invalid in [0, -1, i32::MIN, i32::MAX] {
                assert_eq!(rt.resolve_source_event(plan, invalid), Ok(None));
            }
            rt.panic();
            rt.flush_ended(|_| false);
            assert_eq!(rt.resolve_source_event(plan, id), Ok(Some(note))); // Terminal owner still retained.
            rt.flush_ended(|_| true);
            assert_eq!(rt.resolve_source_event(plan, id), Ok(None));
            assert_eq!(rt.source_event_id(note), Err(Error::StaleHandle));
            previous = id;
        }
    });
}
#[test]
fn source_id_lookup_requires_the_original_generation_and_runtime() {
    let (mut rt, mut control) = Runtime::with_plan_updates(plan(), limits(4), 2, 1).unwrap();
    let mut other = Runtime::new(plan(), limits(1)).unwrap();
    control.submit(Box::new(plan())).unwrap();
    support::without_heap(|| {
        let a = rt.note_on(input(), 60, 1.).unwrap();
        let id = rt.source_event_id(a).unwrap();
        let old = rt.active_plan();
        rt.poll_plan_update().unwrap();
        let new = rt.active_plan();
        assert_eq!(rt.resolve_source_event(old, id), Ok(Some(a)));
        assert_eq!(rt.resolve_source_event(new, id), Ok(None));
        let child = rt
            .child(a, 61, 1., false, Inheritance::Independent)
            .unwrap();
        let child_id = rt.source_event_id(child).unwrap();
        let b = rt
            .note_on(
                Input {
                    external_id: Some(100),
                    ..input()
                },
                60,
                1.,
            )
            .unwrap();
        let b_id = rt.source_event_id(b).unwrap();
        assert_eq!(rt.resolve_source_event(old, child_id), Ok(Some(child)));
        assert_eq!(rt.resolve_source_event(new, child_id), Ok(None));
        assert_eq!(rt.resolve_source_event(old, b_id), Ok(None));
        assert_eq!(rt.resolve_source_event(new, b_id), Ok(Some(b)));
        assert_eq!(other.resolve_source_event(old, id), Err(Error::StaleHandle));
        let foreign = other.note_on(input(), 60, 1.).unwrap();
        assert_eq!(rt.source_event_id(foreign), Err(Error::StaleHandle));
        rt.panic();
        rt.flush_ended(|_| true);
        rt.collect_retired_plans();
        assert_eq!(rt.resolve_source_event(old, id), Err(Error::StaleHandle));
        assert_eq!(rt.resolve_source_event(new, b_id), Ok(None));
    });
    drop(control.retired().unwrap());
}

#[test]
fn source_id_exhaustion_is_explicit_and_does_not_exhaust_native_note_ownership() {
    assert!(plan().with_source_event_limit(0).is_err());
    assert!(plan().with_source_event_limit(-1).is_err());
    let mut rt = Runtime::new(plan().with_source_event_limit(2).unwrap(), limits(2)).unwrap();
    support::without_heap(|| {
        let a = rt.note_on(input(), 60, 1.).unwrap();
        let first = rt.source_event_id(a).unwrap();
        let b = rt
            .child(a, 60, 1., false, Inheritance::Independent)
            .unwrap();
        let last = rt.source_event_id(b).unwrap();
        assert_eq!((first, last), (1, 2));
        rt.key_up(b, None).unwrap();
        rt.flush_ended(|_| true);
        let c = rt
            .child(a, 60, 1., false, Inheritance::Independent)
            .unwrap();
        assert_eq!(rt.source_event_id(c), Err(Error::Capacity));
        assert_eq!(rt.source_event_id(a), Ok(first));
        assert_eq!(rt.resolve_source_event(rt.active_plan(), last), Ok(None));
        assert_eq!(rt.note_count(), 2);
        rt.panic();
        rt.flush_ended(|_| true);
        let d = rt.note_on(input(), 60, 1.).unwrap();
        assert_eq!(rt.source_event_id(d), Err(Error::Capacity));
        assert_eq!(rt.resolve_source_event(rt.active_plan(), first), Ok(None));
    });
}

#[test]
fn generated_result_aliases_are_preflighted_and_do_not_pin_completed_children() {
    let program = || {
        Program::new(vec![
            Instruction::SetLocal {
                local: 0,
                value: 60,
            },
            Instruction::SetLocal {
                local: 1,
                value: 127,
            },
            Instruction::PlayMidi {
                key: 0,
                velocity: 1,
                duration: DurationValue::Fixed(Duration::UntilSilent),
                inheritance: Inheritance::Independent,
                result: Some(0),
            },
            Instruction::ReadEventId { local: 1 },
            Instruction::End,
        ])
        .unwrap()
    };
    let make = |maximum| {
        plan()
            .with_programs(vec![program()], None)
            .unwrap()
            .with_source_event_limit(maximum)
            .unwrap()
    };
    let mut rt = Runtime::new(make(1), limits(3)).unwrap();
    support::without_heap(|| {
        let n = rt.note_on(input(), 60, 1.).unwrap();
        rt.source_event_id(n).unwrap();
        let callback = rt.start_behavior(n, 0).unwrap();
        assert_eq!(
            rt.behavior_outcome(callback),
            Ok(Some(Outcome::Fault(Error::Capacity)))
        );
        assert_eq!(rt.behavior_local(callback, 0), Ok(60));
        assert_eq!(
            (
                rt.note_count(),
                rt.expression_count(),
                rt.voice_count(),
                rt.pending_commands()
            ),
            (1, 1, 0, 0)
        );
    });
    let mut rt = Runtime::new(make(10), limits(3)).unwrap();
    support::without_heap(|| {
        let n = rt.note_on(input(), 60, 1.).unwrap();
        let callback = rt.start_behavior(n, 0).unwrap();
        assert_eq!(rt.behavior_outcome(callback), Ok(Some(Outcome::Finished)));
        let child_id = rt.behavior_local(callback, 0).unwrap() as i32;
        let parent_id = rt.behavior_local(callback, 1).unwrap() as i32;
        assert_ne!(child_id, parent_id);
        assert_eq!(
            rt.resolve_source_event(rt.active_plan(), parent_id),
            Ok(Some(n))
        );
        assert!(
            rt.resolve_source_event(rt.active_plan(), child_id)
                .unwrap()
                .is_some()
        );
        rt.flush_ended(|_| false);
        assert_eq!(
            rt.resolve_source_event(rt.active_plan(), child_id),
            Ok(None)
        );
        assert_eq!(rt.note_count(), 1);
    });
    let program = Program::new(vec![
        Instruction::End,
        Instruction::ReadEventId { local: 4 },
    ])
    .unwrap();
    assert!(program.requires_note());
    assert!(matches!(
        Runtime::new(
            plan().with_programs(vec![program], None).unwrap(),
            limits(1)
        ),
        Err(Error::Capacity)
    ));
}

#[test]
fn script_note_end_and_callback_fault_preserve_anonymous_fifo_and_external_key_ownership() {
    for fault in [false, true] {
        let code = if fault {
            vec![
                Instruction::SetLocal {
                    local: 0,
                    value: 128,
                },
                Instruction::WriteEventKey { local: 0 },
            ]
        } else {
            vec![
                Instruction::ReadEventId { local: 0 },
                Instruction::KeyUpEvent {
                    event: 0,
                    delay: None,
                },
            ]
        };
        let p = plan()
            .with_programs(vec![Program::new(code).unwrap()], Some(0))
            .unwrap();
        let mut rt = Runtime::new(p, limits(3)).unwrap();
        support::without_heap(|| {
            let raw = Input {
                protocol: Protocol::Midi1,
                external_id: None,
                ..input()
            };
            let old = rt.trigger(raw, 72, 1.).unwrap();
            assert!(!rt.key_down(old).unwrap());
            assert!(rt.input_held(old).unwrap());
            let original = rt.release_context(old).unwrap();
            assert_eq!(
                original.key.unwrap().cause,
                if fault {
                    ReleaseCause::BehaviorFault
                } else {
                    ReleaseCause::Script
                }
            );
            rt.flush_behaviors(|_, _, outcome| {
                assert_eq!(
                    outcome,
                    if fault {
                        Outcome::Fault(Error::InvalidInput)
                    } else {
                        Outcome::Finished
                    }
                );
                true
            });
            rt.flush_ended(|_| panic!("the host still owns its key pairing"));
            let fresh = rt.note_on(raw, 84, 1.).unwrap();
            assert_eq!(rt.note_off(raw, Some(0.75)), Ok(old));
            assert!(!rt.input_held(old).unwrap());
            assert!(rt.input_held(fresh).unwrap() && rt.key_down(fresh).unwrap());
            assert_eq!(rt.release_context(old), Ok(original));
            let mut ends = 0;
            rt.flush_ended(|address| {
                assert_eq!(address, raw);
                ends += 1;
                true
            });
            assert_eq!((ends, rt.note_count()), (1, 1));
            assert_eq!(rt.note_off(raw, None), Ok(fresh));
            rt.flush_ended(|_| true);
            assert_eq!(rt.note_count(), 0);
        });
    }
}

#[test]
fn script_deadline_replacement_cannot_consume_scheduled_physical_keyups() {
    for commands in [1, 2] {
        let mut rt = Runtime::new(
            plan(),
            Limits {
                commands,
                ..limits(2)
            },
        )
        .unwrap();
        support::without_heap(|| {
            let n = rt.note_on(input(), 60, 1.).unwrap();
            rt.schedule_event(12, Event::KeyUp(n, Some(0.5))).unwrap();
            if commands == 1 {
                assert_eq!(rt.replace_script_key_up_at(n, 8), Err(Error::Capacity));
                assert!(rt.key_down(n).unwrap());
                rt.replace_script_key_up_at(n, 0).unwrap();
            } else {
                rt.replace_script_key_up_at(n, 8).unwrap();
                rt.replace_script_key_up_at(n, 4).unwrap();
                rt.render(&mut [[0.; 2]; 5]).unwrap();
            }
            assert!(!rt.key_down(n).unwrap() && rt.input_held(n).unwrap());
            assert_eq!(rt.pending_commands(), 1);
            assert_eq!(rt.note_on(input(), 60, 1.), Err(Error::DuplicateInput));
            rt.flush_ended(|_| panic!("logical release cannot consume the host key"));
            rt.render(&mut [[0.; 2]; 12]).unwrap();
            rt.render(&mut []).unwrap();
            assert!(!rt.input_held(n).unwrap());
            assert_eq!(rt.pending_commands(), 0);
            rt.flush_ended(|_| true);
            let fresh = rt.note_on(input(), 60, 1.).unwrap();
            assert!(rt.input_held(fresh).unwrap());
            assert_eq!(rt.input_held(n), Err(Error::StaleHandle));
            rt.panic();
            rt.flush_ended(|_| true);
            assert_eq!(rt.note_count(), 0);
        });
    }
}

#[test]
fn thousands_of_nested_release_callbacks_use_reserved_frames_on_a_small_thread_stack() {
    const COUNT: usize = 4096;
    let release = Program::new(vec![
        Instruction::ReadEventId { local: 0 },
        Instruction::AddLocal { local: 0, value: 1 },
        Instruction::KeyUpEvent {
            event: 0,
            delay: None,
        },
        Instruction::ReadScriptCell { local: 1, cell: 0 },
        Instruction::AddLocal { local: 1, value: 1 },
        Instruction::WriteScriptCell { cell: 0, local: 1 },
    ])
    .unwrap()
    .with_script_instance(ScriptInstanceId(0))
    .with_wait_lifetime(WaitLifetime::Callback);
    let p = plan()
        .with_script_instances(vec![vec![0]])
        .unwrap()
        .with_programs(vec![release], None)
        .unwrap()
        .with_release_program(0)
        .unwrap();
    let mut rt = Runtime::new(
        p,
        Limits {
            behavior_fuel: 6,
            ..limits(COUNT)
        },
    )
    .unwrap();
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(move || {
            let mut notes = Vec::with_capacity(COUNT);
            support::without_heap(|| {
                for index in 0..COUNT {
                    let n = rt
                        .trigger(
                            Input {
                                external_id: Some(index as i32),
                                ..input()
                            },
                            60,
                            1.,
                        )
                        .unwrap();
                    assert_eq!(rt.source_event_id(n).unwrap(), index as i32 + 1);
                    notes.push(n);
                }
                rt.key_up(notes[0], None).unwrap();
                assert_eq!(
                    rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 0),
                    Ok(COUNT as i64)
                );
                let mut callbacks = 0;
                rt.flush_behaviors(|_, _, outcome| {
                    assert_eq!(outcome, Outcome::Finished);
                    callbacks += 1;
                    true
                });
                assert_eq!(callbacks, COUNT);
                let mut ends = 0;
                rt.flush_ended(|_| {
                    ends += 1;
                    true
                });
                assert_eq!(
                    (ends, rt.note_count(), rt.pending_commands()),
                    (1, COUNT - 1, 0)
                );
                for &note in &notes[1..] {
                    assert!(!rt.key_down(note).unwrap() && rt.input_held(note).unwrap());
                    rt.key_up(note, None).unwrap();
                }
                rt.flush_ended(|_| true);
                assert_eq!((rt.note_count(), rt.expression_count()), (0, 0));
                let fresh = rt.trigger(input(), 60, 1.).unwrap();
                rt.key_up(fresh, None).unwrap();
                rt.flush_behaviors(|_, _, outcome| {
                    assert_eq!(outcome, Outcome::Finished);
                    true
                });
                rt.flush_ended(|_| true);
                assert_eq!(
                    rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 0),
                    Ok(COUNT as i64 + 1)
                );
            });
        })
        .unwrap()
        .join()
        .unwrap();
}
