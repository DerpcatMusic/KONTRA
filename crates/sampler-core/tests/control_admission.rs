//! Rejection-time ownership and typed validation; no audio assets or host needed.
use sampler_core::*;
#[path = "support/mod.rs"]
mod support;

const MODE: ControlId = ControlId(100);

fn prepared(code: Vec<Instruction>) -> Prepared {
    Prepared::new(48000, vec![], vec![], 0)
        .unwrap()
        .with_controls(vec![ControlDefinition {
            id: MODE,
            domain: ControlDomain::Integer { min: 0, max: 20 },
            default: ControlValue::Integer(10),
        }])
        .unwrap()
        .with_script_instances(vec![vec![0]])
        .unwrap()
        .with_programs(
            vec![
                Program::new(code)
                    .unwrap()
                    .with_source_slot(3)
                    .with_script_instance(ScriptInstanceId(0))
                    .with_wait_lifetime(WaitLifetime::Callback),
            ],
            None,
        )
        .unwrap()
        .with_control_programs(vec![ControlCallback {
            control: MODE,
            program: 0,
            stage: 0,
        }])
        .unwrap()
        .with_widgets(vec![WidgetDefinition {
            id: MODE,
            source_slot: 3,
            ui_id: 32768,
            instance: ScriptInstanceId(0),
            storage: WidgetStorage::Control(MODE),
            drop: None,
            program: Some(0),
            stage: 0,
        }])
        .unwrap()
        .with_automation_bindings(vec![AutomationBinding {
            source: AutomationSource::HostParameter(7),
            source_slot: 3,
            ui_id: 32768,
            low: 0.,
            high: 1.,
            soft_takeover: false,
        }])
        .unwrap()
}

fn runtime(plan: Prepared, capacity: usize) -> (Runtime, ControlClient) {
    let limits = Limits {
        behaviors: capacity,
        behavior_cells: plan.behavior_local_count() * capacity,
        ..Limits::for_plan(&plan, 4, 0)
    };
    Runtime::new(plan, limits)
        .unwrap()
        .with_control_updates(1, 1)
        .unwrap()
}

fn context(rt: &Runtime) -> ControlContext {
    ControlContext {
        performance: rt.performance(0).unwrap(),
        origin: ChannelAddress {
            protocol: Protocol::Native,
            port: 0,
            group: 0,
            channel: 0,
        },
        channels: 1,
    }
}

fn command(rt: &Runtime, operation: ControlOperation) -> ControlRequest {
    ControlRequest {
        plan: rt.active_plan(),
        expected_revision: Some(rt.control_revision(rt.active_plan()).unwrap()),
        operation,
    }
}

fn invoke(rt: &Runtime, value: ControlValue) -> ControlOperation {
    ControlOperation::Invoke(context(rt), ControlWrite { id: MODE, value })
}

fn widget(rt: &Runtime, value: WidgetValue) -> ControlOperation {
    ControlOperation::InvokeWidget(
        context(rt),
        vec![WidgetEdit {
            id: MODE,
            index: 0,
            value,
            interaction: WidgetInteraction::default(),
        }],
    )
}

fn exchange(rt: &mut Runtime, client: &mut ControlClient, request: ControlRequest) -> ControlReply {
    let sequence = client.submit(request).unwrap();
    assert_eq!(rt.poll_control_update(), Ok(Some(sequence)));
    client.reply().unwrap()
}

#[test]
fn full_arena_rejects_valid_edits_but_keeps_type_domain_and_revision_errors() {
    let (mut rt, mut client) = runtime(prepared(vec![Instruction::Wait(100)]), 16);
    let plan = rt.active_plan();
    support::without_heap(|| {
        for _ in 0..16 {
            rt.start_plan_behavior(plan, 0).unwrap();
        }
    });
    let mut requests = [
        Some(command(&rt, invoke(&rt, ControlValue::Real(20.)))),
        Some(command(&rt, invoke(&rt, ControlValue::Integer(21)))),
        Some(command(&rt, widget(&rt, WidgetValue::Real(20.)))),
        Some(command(&rt, widget(&rt, WidgetValue::Integer(21)))),
        Some(ControlRequest {
            expected_revision: Some(99),
            ..command(&rt, invoke(&rt, ControlValue::Integer(20)))
        }),
        Some(command(&rt, invoke(&rt, ControlValue::Integer(20)))),
        Some(command(&rt, widget(&rt, WidgetValue::Integer(20)))),
        Some(command(
            &rt,
            ControlOperation::HostParameter(context(&rt), 7, 1.),
        )),
    ];
    let expected = [
        Error::InvalidInput,
        Error::InvalidInput,
        Error::InvalidInput,
        Error::InvalidInput,
        Error::RevisionConflict,
        Error::Capacity,
        Error::Capacity,
        Error::Capacity,
    ];
    let mut replies: [Option<ControlReply>; 8] = std::array::from_fn(|_| None);
    support::without_heap(|| {
        for ((request, reply), error) in requests.iter_mut().zip(replies.iter_mut()).zip(expected) {
            *reply = Some(exchange(&mut rt, &mut client, request.take().unwrap()));
            let reply = reply.as_ref().unwrap();
            assert_eq!(reply.result, Err(error));
            assert_eq!(reply.behavior, None);
            assert_eq!(reply.admission_failure.is_some(), error == Error::Capacity);
            if let Some(failure) = reply.admission_failure {
                assert_eq!(
                    (
                        failure.used,
                        failure.capacity,
                        failure.reserved,
                        failure.available
                    ),
                    (16, 16, 0, 0)
                );
                assert_eq!(
                    (
                        failure.plan_owned,
                        failure.note_owned,
                        failure.waiting,
                        failure.terminal
                    ),
                    (16, 0, 16, 0)
                );
                assert_eq!(failure.pending_commands, 16);
                assert_eq!(failure.required_callbacks, 1);
                assert_eq!(failure.outbox_used, 0);
                assert!(!failure.scan_truncated);
                assert!(
                    failure
                        .owners
                        .iter()
                        .all(|owner| owner.is_some_and(|owner| owner.plan == plan
                            && owner.source_slot == 3
                            && owner.program == 0
                            && owner.stage == 0))
                );
            }
            assert_eq!(rt.control_value(plan, MODE), Ok(ControlValue::Integer(10)));
            assert_eq!(rt.control_revision(plan), Ok(0));
            assert_eq!(rt.pending_commands(), 16);
        }
        rt.render(&mut [[0.; 2]; 101]).unwrap();
        rt.flush_behaviors(|_, _, outcome| {
            assert_eq!(outcome, Outcome::Finished);
            true
        });
        // The returned snapshot survives the pressure it describes.
        assert_eq!(
            replies[5]
                .as_ref()
                .unwrap()
                .admission_failure
                .unwrap()
                .waiting,
            16
        );
        let mut count = 0;
        rt.visit_behavior_progress(|_| count += 1);
        assert_eq!(count, 0);
    });
}

#[test]
fn command_exhaustion_inside_an_admitted_callback_is_not_an_admission_rejection() {
    let (mut rt, mut client) = runtime(prepared(vec![Instruction::Wait(100)]), 16);
    let plan = rt.active_plan();
    let request = command(&rt, invoke(&rt, ControlValue::Integer(20)));
    let mut reply = None;
    support::without_heap(|| {
        for _ in 0..256 {
            rt.schedule_event(1000, Event::Articulation(rt.performance(0).unwrap(), 0))
                .unwrap();
        }
        reply = Some(exchange(&mut rt, &mut client, request));
        let reply = reply.as_ref().unwrap();
        assert_eq!(reply.result, Ok((1, 1)));
        assert_eq!(reply.admission_failure, None);
        assert_eq!(
            rt.behavior_outcome(reply.behavior.unwrap()),
            Ok(Some(Outcome::Fault(Error::Capacity)))
        );
        assert_eq!(rt.control_value(plan, MODE), Ok(ControlValue::Integer(20)));
    });
}

#[test]
fn rejected_admission_counts_note_owners_and_reserved_controller_slots_separately() {
    // Keep UI/note waiters separate from the controller-only forward operand.
    let plan = prepared(vec![Instruction::Wait(100)])
        .with_programs(
            vec![
                Program::new(vec![Instruction::Wait(100)])
                    .unwrap()
                    .with_source_slot(3)
                    .with_script_instance(ScriptInstanceId(0))
                    .with_wait_lifetime(WaitLifetime::Callback),
                Program::new(vec![Instruction::Wait(100), Instruction::ForwardController])
                    .unwrap()
                    .with_wait_lifetime(WaitLifetime::Callback),
            ],
            None,
        )
        .unwrap()
        .with_controller_programs(vec![1, 1])
        .unwrap()
        .with_control_programs(vec![ControlCallback {
            control: MODE,
            program: 0,
            stage: 0,
        }])
        .unwrap();
    let (mut rt, mut client) = runtime(plan, 4);
    let request = command(&rt, invoke(&rt, ControlValue::Integer(20)));
    let mut reply = None;
    support::without_heap(|| {
        let context = context(&rt);
        rt.dispatch_controller(context.performance, context.origin, 1, 1, u32::MAX)
            .unwrap()
            .unwrap();
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
        rt.start_behavior(note, 0).unwrap();
        rt.start_plan_behavior(rt.active_plan(), 0).unwrap();
        reply = Some(exchange(&mut rt, &mut client, request));
        let reply = reply.as_ref().unwrap();
        assert_eq!(reply.result, Err(Error::Capacity));
        let failure = reply.admission_failure.unwrap();
        assert_eq!(
            (
                failure.used,
                failure.capacity,
                failure.reserved,
                failure.available
            ),
            (3, 4, 1, 0)
        );
        assert_eq!(
            (failure.note_owned, failure.plan_owned, failure.waiting),
            (1, 2, 3)
        );
    });
}

#[test]
fn rejection_diagnostics_cap_their_traversal_and_report_omitted_owners() {
    let (mut rt, mut client) = runtime(prepared(vec![Instruction::Jump { target: 0 }]), 4097);
    let request = command(&rt, invoke(&rt, ControlValue::Integer(20)));
    let mut reply = None;
    support::without_heap(|| {
        rt.set_behavior_block_fuel(1);
        for _ in 0..4097 {
            rt.start_plan_behavior(rt.active_plan(), 0).unwrap();
        }
        reply = Some(exchange(&mut rt, &mut client, request));
        let failure = reply.as_ref().unwrap().admission_failure.unwrap();
        assert_eq!((failure.used, failure.capacity), (4097, 4097));
        assert_eq!(failure.scanned, 4096);
        assert!(failure.scan_truncated);
        assert_eq!(failure.plan_owned, 4096);
        assert_eq!(failure.owners_omitted, 4089);
        assert_eq!(failure.yielded_queue, 4097);
    });
}
