use sampler_core::*;
#[path = "support/mod.rs"]
mod support;

const LEVEL: ControlId = ControlId(100);
const MODE: ControlId = ControlId(200);
const ENABLED: ControlId = ControlId(300);
fn definitions() -> Vec<ControlDefinition> {
    vec![
        ControlDefinition {
            id: LEVEL,
            domain: ControlDomain::Real {
                min: -120.,
                max: 24.,
            },
            default: ControlValue::Real(-6.),
        },
        ControlDefinition {
            id: MODE,
            domain: ControlDomain::Integer {
                min: i64::MIN,
                max: i64::MAX,
            },
            default: ControlValue::Integer(3),
        },
        ControlDefinition {
            id: ENABLED,
            domain: ControlDomain::Toggle,
            default: ControlValue::Toggle(true),
        },
    ]
}
fn plan(mut definitions: Vec<ControlDefinition>) -> Prepared {
    definitions.reverse();
    Prepared::new(48000, vec![], vec![], 0)
        .unwrap()
        .with_controls(definitions)
        .unwrap()
}
fn limits() -> Limits {
    Limits {
        notes: 2,
        channels: 0,
        performances: 1,
        families: 0,
        expressions: 2,
        voices: 0,
        decisions: 0,
        commands: 2,
        behaviors: 2,
        behavior_fuel: 16,
        behavior_cells: 4,
        note_cells: 0,
    }
}
fn write(id: ControlId, value: ControlValue) -> ControlWrite {
    ControlWrite { id, value }
}

#[test]
fn typed_edits_and_complete_recall_are_atomic_and_identity_survives_reordering() {
    let (mut rt, mut transfer) =
        Runtime::with_plan_updates(plan(definitions()), limits(), 2, 1).unwrap();
    let original = rt.active_plan();
    let empty = write(ControlId(0), ControlValue::Integer(0));
    let mut snapshot = [empty; 4];
    let edits = [
        write(LEVEL, ControlValue::Real(-12.)),
        write(MODE, ControlValue::Integer(i64::MIN)),
    ];
    support::without_heap(|| {
        assert_eq!(rt.edit_controls(original, Some(0), &edits), Ok(1));
        assert_eq!(rt.capture_controls(original, &mut snapshot), Ok((3, 1)));
        assert_eq!(snapshot[3], empty);
        let mut short = [empty; 2];
        assert_eq!(
            rt.capture_controls(original, &mut short),
            Err(Error::Capacity)
        );
        assert_eq!(short, [empty; 2]);
        for invalid in [
            [
                write(LEVEL, ControlValue::Real(0.)),
                write(MODE, ControlValue::Real(1.)),
            ],
            [write(LEVEL, ControlValue::Real(f64::NAN)), edits[1]],
            [write(LEVEL, ControlValue::Real(f64::INFINITY)), edits[1]],
            [write(LEVEL, ControlValue::Real(25.)), edits[1]],
            [edits[1], edits[0]],
            [edits[0], edits[0]],
            [edits[0], write(ControlId(999), ControlValue::Integer(0))],
        ] {
            assert_eq!(
                rt.edit_controls(original, Some(1), &invalid),
                Err(Error::InvalidInput)
            );
            assert_eq!(rt.control_revision(original), Ok(1));
            assert_eq!(
                rt.control_value(original, LEVEL),
                Ok(ControlValue::Real(-12.))
            );
        }
        assert_eq!(
            rt.edit_controls(original, Some(0), &[]),
            Err(Error::RevisionConflict)
        );
        assert_eq!(rt.edit_controls(original, Some(1), &[]), Ok(1));
        assert_eq!(
            rt.recall_controls(original, Some(1), &edits),
            Err(Error::InvalidInput)
        );
    });
    transfer.submit(Box::new(plan(definitions()))).unwrap();
    support::without_heap(|| {
        rt.poll_plan_update().unwrap();
        let new = rt.active_plan();
        assert_ne!(original, new);
        assert_eq!(rt.control_value(original, MODE), Err(Error::StaleHandle));
        assert_eq!(rt.control_value(new, MODE), Ok(ControlValue::Integer(3)));
        assert_eq!(rt.recall_controls(new, Some(0), &snapshot[..3]), Ok(1));
        assert_eq!(
            rt.control_value(new, MODE),
            Ok(ControlValue::Integer(i64::MIN))
        );
        assert_eq!(rt.control_value(new, LEVEL), Ok(ControlValue::Real(-12.)));
        assert_eq!(
            rt.edit_controls(original, None, &edits),
            Err(Error::StaleHandle)
        );
    });
    drop(transfer.retired().unwrap()); // Includes the old value storage, off audio.
    for definition in [
        ControlDefinition {
            default: ControlValue::Real(f64::NAN),
            ..definitions()[0]
        },
        ControlDefinition {
            domain: ControlDomain::Real {
                min: -f64::INFINITY,
                max: 0.,
            },
            ..definitions()[0]
        },
        ControlDefinition {
            domain: ControlDomain::Integer { min: 1, max: 0 },
            ..definitions()[1]
        },
    ] {
        assert!(
            Prepared::new(48000, vec![], vec![], 0)
                .unwrap()
                .with_controls(vec![definition])
                .is_err()
        );
    }
    assert!(
        Prepared::new(48000, vec![], vec![], 0)
            .unwrap()
            .with_controls(vec![definitions()[0]; 2])
            .is_err()
    );
}

#[test]
fn delayed_programs_keep_their_controls_and_due_work_precedes_immediate_edits() {
    let code = vec![
        Instruction::ReadControl {
            local: 0,
            control: MODE,
        },
        Instruction::Wait(5),
        Instruction::ReadControl {
            local: 1,
            control: MODE,
        },
        Instruction::SetLocal { local: 0, value: 9 },
        Instruction::WriteControl {
            control: MODE,
            local: 0,
        },
        Instruction::End,
    ];
    let prepared = plan(definitions())
        .with_programs(vec![Program::new(code.clone()).unwrap()], None)
        .unwrap();
    let (mut rt, mut transfer) = Runtime::with_plan_updates(prepared, limits(), 2, 1).unwrap();
    let old = rt.active_plan();
    let input = Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: None,
    };
    let mut note = None;
    let mut callback = None;
    support::without_heap(|| {
        note = Some(rt.note_on(input, 60, 1.).unwrap());
        callback = Some(rt.start_behavior(note.unwrap(), 0).unwrap());
        assert_eq!(rt.behavior_local(callback.unwrap(), 0), Ok(3));
        rt.edit_controls(old, Some(0), &[write(MODE, ControlValue::Integer(7))])
            .unwrap();
    });
    transfer.submit(Box::new(plan(definitions()))).unwrap();
    support::without_heap(|| {
        rt.poll_plan_update().unwrap();
        let new = rt.active_plan();
        assert_eq!(rt.control_value(new, MODE), Ok(ControlValue::Integer(3)));
        rt.render(&mut [[0.; 2]; 5]).unwrap();
        // At the exclusive block end the callback is still due. It first reads 7
        // and writes 9; the stale UI revision then rejects the entire edit.
        assert_eq!(
            rt.edit_controls(old, Some(1), &[write(MODE, ControlValue::Integer(11))]),
            Err(Error::RevisionConflict)
        );
        assert_eq!(rt.behavior_local(callback.unwrap(), 1), Ok(7));
        assert_eq!(rt.control_value(old, MODE), Ok(ControlValue::Integer(9)));
        assert_eq!(rt.control_value(new, MODE), Ok(ControlValue::Integer(3)));
        rt.release(note.unwrap()).unwrap();
        rt.flush_behaviors(|_, _, outcome| {
            assert_eq!(outcome, Outcome::Finished);
            true
        });
        rt.flush_ended(|_| true);
        assert_eq!(rt.collect_retired_plans(), 1);
    });
    drop(transfer.retired().unwrap());
    // Resolution and operand type are checked during preparation, before admission.
    assert!(
        Prepared::new(48000, vec![], vec![], 0)
            .unwrap()
            .with_programs(vec![Program::new(code).unwrap()], None)
            .is_err()
    );
    assert!(
        plan(definitions())
            .with_programs(
                vec![
                    Program::new(vec![Instruction::ReadControl {
                        local: 0,
                        control: LEVEL
                    }])
                    .unwrap()
                ],
                None
            )
            .is_err()
    );
}

#[test]
fn bounded_ui_handoff_retains_payloads_and_acknowledges_conflicts_without_heap() {
    let (mut rt, mut client) = Runtime::new(plan(definitions()), limits())
        .unwrap()
        .with_control_updates(1, 3)
        .unwrap();
    let generation = rt.active_plan();
    let request = |expected, operation| ControlRequest {
        plan: generation,
        expected_revision: expected,
        operation,
    };
    let first = request(
        Some(0),
        ControlOperation::Edit(Box::from([write(MODE, ControlValue::Integer(5))])),
    );
    assert_eq!(client.submit(first).unwrap(), 1);
    let invalid = request(
        None,
        ControlOperation::Capture(Box::from([write(MODE, ControlValue::Integer(0)); 4])),
    );
    assert_eq!(
        client.submit(invalid).unwrap_err().reason,
        ControlQueueError::PayloadLimit
    );
    let rejected = request(None, ControlOperation::Edit(Box::from([])));
    assert_eq!(
        client.submit(rejected).unwrap_err().reason,
        ControlQueueError::Capacity
    );
    support::without_heap(|| {
        assert_eq!(rt.poll_control_update(), Ok(Some(1)));
    });
    // Queue a stale edit while its predecessor's reply is retained. The audio
    // owner must not apply it or drain due work until a reply slot is available.
    client
        .submit(request(
            Some(0),
            ControlOperation::Edit(Box::from([write(MODE, ControlValue::Integer(9))])),
        ))
        .unwrap();
    support::without_heap(|| {
        assert_eq!(rt.poll_control_update(), Err(ControlQueueError::Capacity));
        assert_eq!(
            rt.control_value(generation, MODE),
            Ok(ControlValue::Integer(5))
        );
    });
    let reply = client.reply().unwrap();
    assert_eq!((reply.request, reply.result), (1, Ok((1, 1))));
    drop(reply);
    support::without_heap(|| {
        assert_eq!(rt.poll_control_update(), Ok(Some(2)));
    });
    let reply = client.reply().unwrap();
    assert_eq!(
        (reply.request, reply.result),
        (2, Err(Error::RevisionConflict))
    );
    let ControlOperation::Edit(payload) = reply.command.operation else {
        panic!()
    };
    assert_eq!(payload[0].value, ControlValue::Integer(9)); // Exact rejected intent returned.
    drop(payload);
    let output: Box<[ControlWrite]> = Box::from([write(ControlId(0), ControlValue::Integer(0)); 3]);
    let pointer = output.as_ptr();
    client
        .submit(request(None, ControlOperation::Capture(output)))
        .unwrap();
    support::without_heap(|| {
        assert_eq!(rt.poll_control_update(), Ok(Some(3)));
    });
    let reply = client.reply().unwrap();
    assert_eq!(reply.result, Ok((3, 1)));
    let ControlOperation::Capture(output) = reply.command.operation else {
        panic!()
    };
    assert_eq!(output.as_ptr(), pointer); // No backing-store copy or destructor on audio.
    assert_eq!(output[1], write(MODE, ControlValue::Integer(5)));
    client
        .submit(request(Some(1), ControlOperation::Recall(output)))
        .unwrap();
    support::without_heap(|| {
        assert_eq!(rt.poll_control_update(), Ok(Some(4)));
    });
    assert_eq!(client.reply().unwrap().result, Ok((3, 2)));
    client
        .submit(request(
            None,
            ControlOperation::Edit(Box::from([write(MODE, ControlValue::Integer(13))])),
        ))
        .unwrap();
    drop(client);
    support::without_heap(|| {
        assert_eq!(
            rt.poll_control_update(),
            Err(ControlQueueError::Disconnected)
        );
        assert_eq!(
            rt.control_value(generation, MODE),
            Ok(ControlValue::Integer(5))
        );
    });
    drop(rt); // Unconsumed payloads destroyed off audio after processing stops.
}

#[test]
fn plan_callbacks_are_independent_of_notes_and_retain_generations_until_acknowledged() {
    let code = vec![
        Instruction::ReadControl {
            local: 0,
            control: MODE,
        },
        Instruction::Wait(5),
        Instruction::WriteControl {
            control: MODE,
            local: 0,
        },
        Instruction::End,
    ];
    let prepared = plan(definitions())
        .with_programs(
            vec![
                Program::new(code)
                    .unwrap()
                    .with_wait_lifetime(WaitLifetime::Callback),
            ],
            None,
        )
        .unwrap()
        .with_control_programs(vec![(MODE, 0)])
        .unwrap();
    let (rt, mut transfer) = Runtime::with_plan_updates(prepared, limits(), 2, 1).unwrap();
    let (mut rt, mut client) = rt.with_control_updates(1, 1).unwrap();
    let old = rt.active_plan();
    client
        .submit(ControlRequest {
            plan: old,
            expected_revision: Some(0),
            operation: ControlOperation::Invoke(write(MODE, ControlValue::Integer(11))),
        })
        .unwrap();
    support::without_heap(|| {
        assert_eq!(rt.poll_control_update(), Ok(Some(1)));
    });
    let reply = client.reply().unwrap();
    let first = reply.behavior.unwrap();
    assert_eq!(reply.result, Ok((1, 1)));
    drop(reply);
    let mut second = None;
    support::without_heap(|| {
        second = rt
            .invoke_control(old, Some(1), write(MODE, ControlValue::Integer(12)))
            .unwrap()
            .1;
        assert_eq!(rt.note_count(), 0);
        assert_eq!(rt.expression_count(), 0);
        assert_eq!(rt.behavior_local(first, 0), Ok(11));
        assert_eq!(rt.behavior_local(second.unwrap(), 0), Ok(12));
        // Saturation cannot change the value even though the value itself is valid.
        assert_eq!(
            rt.invoke_control(old, None, write(MODE, ControlValue::Integer(99))),
            Err(Error::Capacity)
        );
        assert_eq!(rt.control_value(old, MODE), Ok(ControlValue::Integer(12)));
    });
    transfer.submit(Box::new(plan(definitions()))).unwrap();
    support::without_heap(|| {
        rt.poll_plan_update().unwrap();
        assert_eq!(rt.plan_count(), 2);
        // MIDI channel cleanup cannot accidentally cancel a UI callback.
        rt.all_sound_off(ChannelAddress {
            protocol: Protocol::Native,
            port: 0,
            group: 0,
            channel: 0,
        })
        .unwrap();
        rt.render(&mut [[0.; 2]; 6]).unwrap();
        assert_eq!(rt.behavior_outcome(first), Ok(Some(Outcome::Finished)));
        assert_eq!(
            rt.behavior_outcome(second.unwrap()),
            Ok(Some(Outcome::Finished))
        );
        assert_eq!(rt.control_value(old, MODE), Ok(ControlValue::Integer(12)));
        assert_eq!(rt.control_revision(old), Ok(4));
        assert_eq!(
            rt.control_value(rt.active_plan(), MODE),
            Ok(ControlValue::Integer(3))
        );
        assert_eq!(rt.collect_retired_plans(), 0);
        rt.flush_behaviors(|_, owner, _| {
            assert_eq!(owner, BehaviorOwner::Plan(old));
            false
        });
        assert_eq!(rt.collect_retired_plans(), 0);
        rt.flush_behaviors(|_, _, _| true);
        assert_eq!(rt.collect_retired_plans(), 1);
        assert_eq!(rt.control_value(old, MODE), Err(Error::StaleHandle));
        assert_eq!(rt.behavior_local(first, 0), Err(Error::StaleHandle));
    });
    drop(transfer.retired().unwrap());
}

#[test]
fn plan_callback_faults_cancel_without_touching_musical_owners_or_leaking_work() {
    let programs = vec![
        Program::new(vec![Instruction::Wait(9), Instruction::End])
            .unwrap()
            .with_wait_lifetime(WaitLifetime::Callback),
        Program::new(vec![Instruction::Jump { target: 0 }])
            .unwrap()
            .with_wait_lifetime(WaitLifetime::Callback),
        Program::new(vec![
            Instruction::SetLocal {
                local: 0,
                value: i64::MAX,
            },
            Instruction::AddLocal { local: 0, value: 1 },
        ])
        .unwrap()
        .with_wait_lifetime(WaitLifetime::Callback),
        Program::new(vec![Instruction::ReadKey { local: 0 }])
            .unwrap()
            .with_wait_lifetime(WaitLifetime::Callback),
        Program::new(vec![Instruction::Wait(9)]).unwrap(),
    ];
    let prepared = plan(definitions()).with_programs(programs, None).unwrap();
    let mut rt = Runtime::new(prepared, limits()).unwrap();
    let generation = rt.active_plan();
    support::without_heap(|| {
        let note = rt
            .note_on(
                Input {
                    protocol: Protocol::Native,
                    port: 0,
                    group: 0,
                    channel: 0,
                    key: 60,
                    external_id: None,
                },
                60,
                1.,
            )
            .unwrap();
        for program in [3, 4, 5] {
            assert_eq!(
                rt.start_plan_behavior(generation, program),
                Err(Error::InvalidInput)
            );
        }
        for (program, expected) in [
            (1, Outcome::FuelExhausted),
            (2, Outcome::Fault(Error::ArithmeticOverflow)),
        ] {
            let id = rt.start_plan_behavior(generation, program).unwrap();
            assert_eq!(rt.behavior_outcome(id), Ok(Some(expected)));
            assert!(rt.note(note).unwrap().2);
            rt.flush_behaviors(|_, owner, outcome| {
                assert_eq!(owner, BehaviorOwner::Plan(generation));
                assert_eq!(outcome, expected);
                true
            });
        }
        let id = rt.start_plan_behavior(generation, 0).unwrap();
        assert_eq!(rt.pending_commands(), 1);
        rt.cancel_behavior(id).unwrap();
        assert_eq!(rt.pending_commands(), 0);
        assert_eq!(rt.behavior_outcome(id), Ok(Some(Outcome::Cancelled)));
        assert!(rt.note(note).unwrap().2);
        rt.flush_behaviors(|_, _, _| true);
        let next = rt.start_plan_behavior(generation, 0).unwrap();
        assert_ne!(next, id);
        rt.panic();
        assert_eq!(rt.pending_commands(), 0);
        assert_eq!(rt.behavior_outcome(next), Ok(Some(Outcome::Cancelled)));
        rt.flush_behaviors(|_, _, _| true);
        rt.flush_ended(|_| true);
        assert_eq!(rt.note_count(), 0);
    });
}

#[test]
fn script_instances_share_only_their_own_generation_across_notes_and_ui_callbacks() {
    let note_code = vec![
        Instruction::ReadKey { local: 0 },
        Instruction::WriteScriptCell { cell: 0, local: 0 },
        Instruction::Wait(4),
        Instruction::ReadScriptCell { local: 1, cell: 0 },
    ];
    let ui_code = vec![
        Instruction::ReadScriptCell { local: 0, cell: 0 },
        Instruction::AddLocal { local: 0, value: 3 },
        Instruction::WriteScriptCell { cell: 0, local: 0 },
        Instruction::Wait(2),
        Instruction::ReadScriptCell { local: 1, cell: 0 },
    ];
    let prepared = plan(vec![])
        .with_script_instances(vec![vec![5], vec![100]])
        .unwrap()
        .with_programs(
            vec![
                Program::new(note_code)
                    .unwrap()
                    .with_script_instance(ScriptInstanceId(0)),
                Program::new(ui_code.clone())
                    .unwrap()
                    .with_script_instance(ScriptInstanceId(0))
                    .with_wait_lifetime(WaitLifetime::Callback),
                Program::new(ui_code)
                    .unwrap()
                    .with_script_instance(ScriptInstanceId(1))
                    .with_wait_lifetime(WaitLifetime::Callback),
            ],
            None,
        )
        .unwrap();
    let (mut rt, mut transfer) = Runtime::with_plan_updates(
        prepared,
        Limits {
            behaviors: 4,
            commands: 4,
            behavior_cells: 8,
            ..limits()
        },
        2,
        1,
    )
    .unwrap();
    let old = rt.active_plan();
    let mut notes = [None; 2];
    let mut callbacks = [None; 4];
    support::without_heap(|| {
        for i in 0..2 {
            let key = 60 + i as u8;
            let input = Input {
                protocol: Protocol::Native,
                port: 0,
                group: 0,
                channel: 0,
                key,
                external_id: Some(i as i32),
            };
            let note = rt.note_on(input, key, 1.).unwrap();
            notes[i] = Some(note);
            callbacks[i] = Some(rt.start_behavior(note, 0).unwrap());
        }
        callbacks[2] = Some(rt.start_plan_behavior(old, 1).unwrap());
        callbacks[3] = Some(rt.start_plan_behavior(old, 2).unwrap());
        assert_eq!(rt.script_cell(old, ScriptInstanceId(0), 0), Ok(64));
        assert_eq!(rt.script_cell(old, ScriptInstanceId(1), 0), Ok(103));
        assert_eq!(
            rt.script_cell(old, ScriptInstanceId(2), 0),
            Err(Error::InvalidInput)
        );
        assert_eq!(
            rt.script_cell(old, ScriptInstanceId(0), 1),
            Err(Error::InvalidInput)
        );
    });
    transfer
        .submit(Box::new(
            plan(vec![])
                .with_script_instances(vec![vec![-1], vec![-2]])
                .unwrap(),
        ))
        .unwrap();
    support::without_heap(|| {
        rt.poll_plan_update().unwrap();
        let new = rt.active_plan();
        rt.render(&mut [[0.; 2]; 5]).unwrap();
        for (i, expected) in [64, 64, 64, 103].into_iter().enumerate() {
            assert_eq!(rt.behavior_local(callbacks[i].unwrap(), 1), Ok(expected));
            assert_eq!(
                rt.behavior_outcome(callbacks[i].unwrap()),
                Ok(Some(Outcome::Finished))
            );
        }
        for (i, expected) in [60, 61, 64, 103].into_iter().enumerate() {
            assert_eq!(rt.behavior_local(callbacks[i].unwrap(), 0), Ok(expected));
        }
        assert_eq!(rt.script_cell(new, ScriptInstanceId(0), 0), Ok(-1));
        assert_eq!(rt.script_cell(new, ScriptInstanceId(1), 0), Ok(-2));
        for note in notes {
            rt.key_up(note.unwrap(), None).unwrap();
        }
        rt.flush_behaviors(|_, _, _| false);
        rt.flush_ended(|_| panic!("script callbacks still pin their notes"));
        assert_eq!(rt.collect_retired_plans(), 0);
        rt.flush_behaviors(|_, _, _| true);
        rt.flush_ended(|_| true);
        assert_eq!(rt.collect_retired_plans(), 1);
        assert_eq!(
            rt.script_cell(old, ScriptInstanceId(0), 0),
            Err(Error::StaleHandle)
        );
        assert_eq!(rt.script_cell(new, ScriptInstanceId(0), 0), Ok(-1));
    });
    drop(transfer.retired().unwrap());
}

#[test]
fn script_state_layout_and_callback_binding_are_validated_before_activation() {
    let code = || Program::new(vec![Instruction::ReadScriptCell { local: 0, cell: 1 }]).unwrap();
    assert!(matches!(
        plan(vec![]).with_programs(vec![code()], None),
        Err(Error::InvalidInput)
    ));
    for (instance, banks) in [(0, vec![]), (0, vec![vec![1]]), (1, vec![vec![1, 2]])] {
        assert!(matches!(
            plan(vec![])
                .with_script_instances(banks)
                .unwrap()
                .with_programs(
                    vec![code().with_script_instance(ScriptInstanceId(instance))],
                    None
                ),
            Err(Error::InvalidInput)
        ));
    }
    let p = plan(vec![])
        .with_script_instances(vec![vec![1, 2]])
        .unwrap()
        .with_programs(vec![code().with_script_instance(ScriptInstanceId(0))], None)
        .unwrap();
    assert!(matches!(
        p.with_script_instances(vec![vec![1]]),
        Err(Error::InvalidInput)
    ));
    assert!(matches!(
        plan(vec![]).with_script_instances(vec![vec![0; 65537]]),
        Err(Error::Capacity)
    ));
    assert!(matches!(
        plan(vec![]).with_script_instances(vec![vec![]; 65537]),
        Err(Error::Capacity)
    ));
    let p = plan(vec![])
        .with_script_instances(vec![vec![0]])
        .unwrap()
        .with_programs(
            vec![
                Program::new(vec![Instruction::ReadScriptCell { local: 2, cell: 0 }])
                    .unwrap()
                    .with_script_instance(ScriptInstanceId(0)),
            ],
            None,
        )
        .unwrap();
    assert!(matches!(Runtime::new(p, limits()), Err(Error::Capacity)));
}

#[test]
fn script_writes_survive_callback_fault_cancel_and_slot_reuse_without_resetting_the_instance() {
    let programs = vec![
        vec![
            Instruction::SetLocal { local: 0, value: 7 },
            Instruction::WriteScriptCell { cell: 0, local: 0 },
            Instruction::Wait(10),
            Instruction::SetLocal { local: 0, value: 9 },
            Instruction::WriteScriptCell { cell: 0, local: 0 },
        ],
        vec![
            Instruction::ReadScriptCell { local: 0, cell: 1 },
            Instruction::AddLocal { local: 0, value: 1 },
        ],
        vec![Instruction::ReadScriptCell { local: 0, cell: 0 }],
    ]
    .into_iter()
    .map(|code| {
        Program::new(code)
            .unwrap()
            .with_script_instance(ScriptInstanceId(0))
            .with_wait_lifetime(WaitLifetime::Callback)
    })
    .collect();
    let p = plan(vec![])
        .with_script_instances(vec![vec![0, i64::MAX]])
        .unwrap()
        .with_programs(programs, None)
        .unwrap();
    let mut rt = Runtime::new(p, limits()).unwrap();
    support::without_heap(|| {
        let plan = rt.active_plan();
        let writer = rt.start_plan_behavior(plan, 0).unwrap();
        let fault = rt.start_plan_behavior(plan, 1).unwrap();
        assert_eq!(
            rt.behavior_outcome(fault),
            Ok(Some(Outcome::Fault(Error::ArithmeticOverflow)))
        );
        rt.cancel_behavior(writer).unwrap();
        assert_eq!(rt.pending_commands(), 0);
        rt.render(&mut [[0.; 2]; 11]).unwrap();
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 0), Ok(7));
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 1), Ok(i64::MAX));
        rt.flush_behaviors(|_, _, _| true);
        let reader = rt.start_plan_behavior(plan, 2).unwrap();
        assert_eq!(rt.behavior_local(reader, 0), Ok(7));
        assert_eq!(rt.behavior_local(writer, 0), Err(Error::StaleHandle));
        rt.flush_behaviors(|_, _, _| true);
        assert_eq!(rt.note_count(), 0);
    });
}
