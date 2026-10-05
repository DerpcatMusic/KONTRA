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
