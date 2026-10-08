use sampler_core::*;
use sampler_ksp::{Environment, Limits as KspLimits};
fn runtime(source: &str) -> Runtime {
    let script = sampler_ksp::compile_with(
        source,
        48000,
        KspLimits::LIBRARY,
        &[],
        &Environment::default(),
    )
    .unwrap();
    let plan = script
        .bind(Prepared::new(48000, vec![], vec![], 1).unwrap())
        .unwrap();
    let cells = plan.behavior_local_count() * 16;
    let mut limits = Limits::for_plan(&plan, 16, 16);
    limits.behaviors = 16;
    limits.behavior_cells = cells;
    Runtime::new(plan, limits).unwrap()
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
#[test]
fn table_mouse_move_has_one_callback_and_overflow_is_atomic() {
    let mut rt = runtime(
        "on init declare ui_table %t[5000](1,1,100) declare $calls declare $index declare $event end on on ui_control(%t) inc($calls) $index := $NI_CONTROL_PAR_IDX $event := %EVENT_PAR[2] end on",
    );
    let id = sampler_ksp::derived_control_id(0, "%t");
    let plan = rt.active_plan();
    let edits: Vec<_> = (0..16)
        .map(|index| WidgetEdit {
            id,
            index,
            interaction: WidgetInteraction {
                event_par: [4, 5, 6, 7],
                ..WidgetInteraction::default()
            },
            value: WidgetValue::Integer(7),
        })
        .collect();
    rt.invoke_widget(context(&rt), plan, None, &edits).unwrap();
    assert_eq!(rt.widget_value(plan, id, 15), Ok(WidgetValue::Integer(7)));
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 5000), Ok(1));
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 5001), Ok(0));
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 5002), Ok(6));
    let overflow: Vec<_> = (0..=WIDGET_EDIT_CAPACITY as u32)
        .map(|index| WidgetEdit {
            id,
            index,
            interaction: WidgetInteraction::default(),
            value: WidgetValue::Integer(9),
        })
        .collect();
    let revision = rt.control_revision(plan).unwrap();
    assert_eq!(
        rt.invoke_widget(context(&rt), plan, None, &overflow),
        Err(Error::InvalidInput)
    );
    assert_eq!(rt.control_revision(plan), Ok(revision));
    assert_eq!(rt.widget_value(plan, id, 15), Ok(WidgetValue::Integer(7)));
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 5000), Ok(1));
}
#[test]
fn typed_text_and_xy_values_are_live() {
    let mut rt = runtime("on init declare ui_xy ?xy[2] declare ui_text_edit @text end on");
    let id = sampler_ksp::derived_control_id(0, "?xy");
    let plan = rt.active_plan();
    let edits = [
        WidgetEdit {
            id,
            index: 0,
            interaction: WidgetInteraction::default(),
            value: WidgetValue::Real(0.25),
        },
        WidgetEdit {
            id,
            index: 1,
            interaction: WidgetInteraction::default(),
            value: WidgetValue::Real(0.75),
        },
    ];
    rt.invoke_widget(context(&rt), plan, None, &edits).unwrap();
    assert_eq!(rt.widget_value(plan, id, 1), Ok(WidgetValue::Real(0.75)));
    let invalid = [
        edits[0],
        WidgetEdit {
            id,
            index: 1,
            interaction: WidgetInteraction::default(),
            value: WidgetValue::Real(f64::NAN),
        },
    ];
    assert_eq!(
        rt.invoke_widget(context(&rt), plan, None, &invalid),
        Err(Error::InvalidInput)
    );
    assert_eq!(rt.widget_value(plan, id, 0), Ok(WidgetValue::Real(0.25)));
    let text = sampler_ksp::derived_control_id(0, "@text");
    rt.invoke_widget(
        context(&rt),
        plan,
        None,
        &[WidgetEdit {
            id: text,
            index: 0,
            interaction: WidgetInteraction::default(),
            value: WidgetValue::Text(Text::new("hello")),
        }],
    )
    .unwrap();
    assert_eq!(
        rt.widget_value(plan, text, 0),
        Ok(WidgetValue::Text(Text::new("hello")))
    );
}

#[test]
fn native_widget_reply_retains_payload_and_reports_authoritative_revision() {
    let rt = runtime("on init declare ui_xy ?xy[2] end on");
    let context = context(&rt);
    let plan = rt.active_plan();
    let id = rt.widget_id(plan, 0, 32768).unwrap();
    let (mut rt, mut client) = rt.with_control_updates(2, WIDGET_EDIT_CAPACITY).unwrap();
    let edits = vec![
        WidgetEdit {
            id,
            index: 0,
            value: WidgetValue::Real(0.25),
            interaction: WidgetInteraction::default(),
        },
        WidgetEdit {
            id,
            index: 1,
            value: WidgetValue::Real(0.75),
            interaction: WidgetInteraction::default(),
        },
    ];
    let request = client
        .submit(ControlRequest {
            plan,
            expected_revision: Some(0),
            operation: ControlOperation::InvokeWidget(context, edits),
        })
        .unwrap();
    assert_eq!(rt.poll_control_update(), Ok(Some(request)));
    let reply = client.reply().unwrap();
    assert_eq!(reply.result, Ok((2, 1)));
    assert!(matches!(reply.command.operation,ControlOperation::InvokeWidget(_,v) if v.len()==2));
    assert_eq!(rt.widget_value(plan, id, 1), Ok(WidgetValue::Real(0.75)));
    let invalid = [WidgetEdit {
        id,
        index: 0,
        value: WidgetValue::Real(0.9),
        interaction: WidgetInteraction::default(),
    }];
    assert_eq!(
        rt.invoke_widget(context, plan, Some(0), &invalid),
        Err(Error::RevisionConflict)
    );
    assert_eq!(rt.widget_value(plan, id, 0), Ok(WidgetValue::Real(0.25)));
}
