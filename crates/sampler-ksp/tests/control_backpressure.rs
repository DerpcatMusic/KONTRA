//! Authored, asset-free admission probes at the ordinary host script limits.
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;

use sampler_core::*;

fn fixture() -> (Runtime, ControlClient, ControlId) {
    let script = sampler_ksp::compile(
        "on init
           declare ui_menu $mode
           add_menu_item($mode, \"First\", 10)
           add_menu_item($mode, \"Second\", 20)
           $mode := 10
           declare ui_label $status(1,1)
           declare $seen := 0
           set_listener($NI_SIGNAL_TIMER_MS,1000)
         end on
         on listener set_text($status, \"tick\") end on
         on ui_control($mode) $seen := $mode end on",
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
    )
    .unwrap();
    let plan = script
        .bind(Prepared::new(48000, vec![], vec![], 0).unwrap())
        .unwrap();
    let limits = Limits::for_plan(&plan, 4, 0);
    assert_eq!(limits.behaviors, 129);
    assert_eq!(limits.commands, 256);
    let (rt, client) = Runtime::new(plan, limits)
        .unwrap()
        .with_control_updates(4, 1)
        .unwrap();
    let id = rt.widget_id(rt.active_plan(), 0, 32768).unwrap();
    (rt, client, id)
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

fn request(rt: &Runtime, id: ControlId, value: WidgetValue) -> ControlRequest {
    ControlRequest {
        plan: rt.active_plan(),
        expected_revision: Some(rt.control_revision(rt.active_plan()).unwrap()),
        operation: ControlOperation::InvokeWidget(
            context(rt),
            vec![WidgetEdit {
                id,
                index: 0,
                interaction: WidgetInteraction::default(),
                value,
            }],
        ),
    }
}

fn step(rt: &mut Runtime) {
    rt.render(&mut [[0.; 2]; 48]).unwrap();
    rt.flush_behaviors(|_, _, outcome| {
        assert_eq!(outcome, Outcome::Finished);
        true
    });
}

#[test]
fn undrained_effects_bound_one_timer_but_repeated_ui_callbacks_fill_the_arena() {
    let (mut rt, mut client, id) = fixture();
    let command = request(&rt, id, WidgetValue::Integer(20));
    let plan = rt.active_plan();
    let revision = rt.control_revision(plan).unwrap();
    let mut rejected = None;
    support::without_heap(|| {
        for _ in 0..400 {
            step(&mut rt);
        }
        let mut callbacks = 0;
        let mut waiting = 0;
        rt.visit_behavior_progress(|progress| {
            callbacks += 1;
            waiting += usize::from(progress.waiting);
            assert_eq!(progress.owner, BehaviorOwner::Plan(plan));
            assert_eq!(progress.outcome, None);
        });
        assert_eq!(callbacks, 2, "the stalled body also parks its timer driver");
        assert_eq!(waiting, 0, "the driver is queued behind its stalled body");
        assert!(rt.stalled_effects() > 0);
        assert!(rt.pending_commands() < 256);
        assert_eq!(rt.note_count(), 0);
        assert_eq!(rt.voice_count(), 0);
        assert_eq!(rt.family_count(), 0);
        assert_eq!(rt.expression_count(), 0);
        // Independent user interactions still acquire callback ownership. Their
        // bodies queue behind the stalled effect, even for a tiny menu handler.
        for _ in 0..127 {
            rt.invoke_widget(
                context(&rt),
                plan,
                None,
                &[WidgetEdit {
                    id,
                    index: 0,
                    interaction: WidgetInteraction::default(),
                    value: WidgetValue::Integer(10),
                }],
            )
            .unwrap();
        }
        assert_eq!(rt.control_revision(plan), Ok(revision + 127));
    });
    let revision = rt.control_revision(plan).unwrap();
    let command = ControlRequest {
        expected_revision: Some(revision),
        ..command
    };
    support::without_heap(|| {
        let sequence = client.submit(command).unwrap();
        assert_eq!(rt.poll_control_update(), Ok(Some(sequence)));
        rejected = client.reply();
        let reply = rejected.as_ref().unwrap();
        assert_eq!(reply.result, Err(Error::Capacity));
        assert_eq!(reply.behavior, None);
        let failure = reply.admission_failure.as_ref().unwrap();
        assert_eq!(failure.required_callbacks, 1);
        assert_eq!(
            (
                failure.used,
                failure.capacity,
                failure.available,
                failure.reserved
            ),
            (129, 129, 0, 0)
        );
        assert_eq!((failure.scanned, failure.scan_truncated), (129, false));
        assert_eq!(
            (
                failure.note_owned,
                failure.plan_owned,
                failure.waiting,
                failure.terminal
            ),
            (0, 129, 0, 0)
        );
        assert_eq!(failure.yielded_queue, 129);
        assert_eq!((failure.outbox_used, failure.outbox_capacity), (256, 256));
        assert!(failure.stalled_effects > 0);
        assert_eq!(failure.owners_omitted, 121);
        assert!(
            failure
                .owners
                .iter()
                .all(|sample| sample.is_some_and(|owner| owner.plan == plan))
        );
        assert_eq!(rt.widget_value(plan, id, 0), Ok(WidgetValue::Integer(10)));
        assert_eq!(rt.control_revision(plan), Ok(revision));
    });
    let command = rejected.take().unwrap().command;
    let mut accepted = None;
    support::without_heap(|| {
        // An ordinary finite service gap must recover without dropping callback
        // bodies, changing timer cadence, or increasing any arena/outbox limit.
        for _ in 0..8 {
            rt.drain_effects(|_| true);
            step(&mut rt);
        }
        let mut callbacks = 0;
        rt.visit_behavior_progress(|progress| {
            callbacks += 1;
            assert!(progress.waiting);
        });
        assert_eq!(callbacks, 1);
        let sequence = client.submit(command).unwrap();
        assert_eq!(rt.poll_control_update(), Ok(Some(sequence)));
        accepted = client.reply();
        let reply = accepted.as_ref().unwrap();
        assert_eq!(reply.result, Ok((1, revision + 1)));
        assert_eq!(reply.admission_failure, None);
        assert_eq!(
            rt.behavior_outcome(reply.behavior.unwrap()),
            Ok(Some(Outcome::Finished))
        );
        assert_eq!(rt.widget_value(plan, id, 0), Ok(WidgetValue::Integer(20)));
    });
    let invalid = request(&rt, id, WidgetValue::Real(20.));
    let mut rejected_type = None;
    support::without_heap(|| {
        let sequence = client.submit(invalid).unwrap();
        assert_eq!(rt.poll_control_update(), Ok(Some(sequence)));
        rejected_type = client.reply();
        assert_eq!(
            rejected_type.as_ref().unwrap().result,
            Err(Error::InvalidInput)
        );
        assert_eq!(rejected_type.as_ref().unwrap().admission_failure, None);
        assert_eq!(rt.widget_value(plan, id, 0), Ok(WidgetValue::Integer(20)));
    });
}

#[test]
fn bounded_effect_service_keeps_timer_and_repeated_menu_callbacks_progressing() {
    let (mut rt, mut client, id) = fixture();
    let plan = rt.active_plan();
    let mut pending = Some(request(&rt, id, WidgetValue::Integer(20)));
    support::without_heap(|| {
        for index in 0..1000 {
            let mut drained = 0;
            rt.drain_effects(|_| {
                if drained == 16 {
                    return false;
                }
                drained += 1;
                true
            });
            step(&mut rt);
            let mut command = pending.take().unwrap();
            let revision = rt.control_revision(plan).unwrap();
            command.expected_revision = Some(revision);
            let ControlOperation::InvokeWidget(_, edits) = &mut command.operation else {
                unreachable!()
            };
            let value = if index % 2 == 0 { 20 } else { 10 };
            edits[0].value = WidgetValue::Integer(value);
            let sequence = client.submit(command).unwrap();
            assert_eq!(rt.poll_control_update(), Ok(Some(sequence)));
            let reply = client.reply().unwrap();
            assert_eq!(reply.result, Ok((1, revision + 1)));
            assert_eq!(reply.admission_failure, None);
            assert_eq!(
                rt.behavior_outcome(reply.behavior.unwrap()),
                Ok(Some(Outcome::Finished))
            );
            assert_eq!(
                rt.widget_value(plan, id, 0),
                Ok(WidgetValue::Integer(value))
            );
            pending = Some(reply.command);
        }
        assert_eq!(rt.stalled_effects(), 0);
    });
}
