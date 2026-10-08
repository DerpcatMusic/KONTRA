#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;
use sampler_core::*;

fn setup() -> (Runtime, ControlClient, ScriptStateBuffer) {
    let sources = [
        r#"on init
        declare ui_knob $k(0,127,1) make_persistent($k)
        declare $i := 11 make_persistent($i)
        declare ~r := 1.25 make_persistent(~r)
        declare %nums[3] := (2,3,4) make_persistent(%nums)
        declare ?reals[2] := (0.25,0.75) make_persistent(?reals)
        declare @text := "first" make_persistent(@text)
        declare !texts[2] := ("a","b") make_persistent(!texts)
        declare $calls declare $done
        end on
        on ui_control($k)
        inc($i) ~r := ~r + 0.5 %nums[1] := %nums[1] + 1
        ?reals[1] := ?reals[1] + 0.25 @text := "edited" !texts[1] := "edited array"
        end on
        on persistence_changed
        inc($calls)
        if ($k = 99) wait(1000) end if
        inc($done)
        end on"#,
        "on init declare ui_knob $fault(0,127,1) make_persistent($fault) declare $q end on on persistence_changed if ($fault = 99) wait(1 - 2) end if end on",
    ];
    let scripts: Vec<_> = sources
        .iter()
        .enumerate()
        .map(|(slot, source)| {
            sampler_ksp::compile_with(
                source,
                48000,
                sampler_ksp::Limits::LIBRARY,
                &[],
                &sampler_ksp::Environment {
                    slot: slot as u8,
                    ..Default::default()
                },
            )
            .unwrap()
        })
        .collect();
    let views: Vec<_> = scripts.iter().map(|script| script.view()).collect();
    let state = sampler_ksp::persistent_state_buffer(&views).unwrap();
    let plan = sampler_ksp::bind_modules(scripts, Prepared::new(48000, vec![], vec![], 0).unwrap())
        .unwrap();
    let mut limits = Limits::for_plan(&plan, 4, 4);
    limits.behaviors = 16;
    limits.behavior_cells = plan.behavior_local_count() * 16;
    let (runtime, client) = Runtime::new(plan, limits)
        .unwrap()
        .with_control_updates(4, 4096)
        .unwrap();
    (runtime, client, state)
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
fn live_all_sigil_capture_restore_and_callback_outcomes_use_owned_transport_without_heap() {
    let (mut rt, mut client, mut state) = setup();
    let plan = rt.active_plan();
    rt.capture_script_state(plan, &mut state).unwrap();
    let initial: Vec<_> = state.values.iter().map(|e| e.value).collect();
    let id = sampler_ksp::derived_control_id(0, "$k");
    rt.invoke_widget(
        context(&rt),
        plan,
        None,
        &[WidgetEdit {
            id,
            index: 0,
            interaction: Default::default(),
            value: WidgetValue::Integer(99),
        }],
    )
    .unwrap();
    rt.capture_script_state(plan, &mut state).unwrap();
    assert_ne!(
        state.values.iter().map(|e| e.value).collect::<Vec<_>>(),
        initial
    );
    assert!(
        state
            .values
            .iter()
            .any(|e| e.value == ScriptStateValue::Text(Text::new("edited array")))
    );
    assert!(
        state
            .values
            .iter()
            .any(|e| e.value == ScriptStateValue::Cell(real_bits(1.0)))
    );
    let fault_id = sampler_ksp::derived_control_id(1, "$fault");
    state
        .values
        .iter_mut()
        .find(|e| e.address == ScriptStateAddress::Control(fault_id))
        .unwrap()
        .value = ScriptStateValue::Control(ControlValue::Integer(99));
    let saved: Vec<_> = state.values.iter().map(|e| e.value).collect();
    rt.invoke_widget(
        context(&rt),
        plan,
        None,
        &[WidgetEdit {
            id,
            index: 0,
            interaction: Default::default(),
            value: WidgetValue::Integer(2),
        }],
    )
    .unwrap();
    let revision = rt.control_revision(plan).unwrap();
    client
        .submit(ControlRequest {
            plan,
            expected_revision: Some(revision),
            operation: ControlOperation::RestoreScriptState(state),
        })
        .unwrap();
    support::without_heap(|| {
        rt.poll_control_update().unwrap();
    });
    let reply = client.reply().unwrap();
    assert!(reply.result.is_ok());
    let ControlOperation::RestoreScriptState(mut state) = reply.command.operation else {
        panic!()
    };
    assert_eq!(state.callbacks[0].outcome, None);
    assert!(matches!(
        state.callbacks[1].outcome,
        Some(Outcome::Fault(_))
    ));
    rt.capture_script_state(plan, &mut state).unwrap();
    assert_eq!(
        state.values.iter().map(|e| e.value).collect::<Vec<_>>(),
        saved
    );
    let mut audio = [Frame::default(); 64];
    support::without_heap(|| {
        rt.render(&mut audio).unwrap();
    });
    rt.flush_behaviors(|_, _, _| true);
    client
        .submit(ControlRequest {
            plan,
            expected_revision: None,
            operation: ControlOperation::CaptureScriptState(state),
        })
        .unwrap();
    support::without_heap(|| {
        rt.poll_control_update().unwrap();
    });
    let reply = client.reply().unwrap();
    let ControlOperation::CaptureScriptState(state) = reply.command.operation else {
        panic!()
    };
    assert_eq!(state.callbacks[0].outcome, Some(Outcome::Finished));
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 7), Ok(2));
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 8), Ok(2));
    assert!(matches!(
        state.callbacks[1].outcome,
        Some(Outcome::Fault(_))
    ));
}

#[test]
fn restore_rejection_and_capture_bad_address_leave_buffers_and_live_state_untouched() {
    let (mut rt, mut client, mut state) = setup();
    let plan = rt.active_plan();
    rt.capture_script_state(plan, &mut state).unwrap();
    let initial = state.values.clone();
    let revision = rt.control_revision(plan).unwrap();
    state.values[0].value = ScriptStateValue::Control(ControlValue::Integer(200));
    assert_eq!(
        rt.restore_script_state(plan, Some(revision), &mut state),
        Err(Error::InvalidInput)
    );
    assert_eq!(rt.control_revision(plan), Ok(revision));
    assert!(state.callbacks.iter().all(|c| c.behavior.is_none()));
    rt.capture_script_state(plan, &mut state).unwrap();
    assert_eq!(state.values, initial);
    assert_eq!(
        rt.restore_script_state(plan, Some(revision + 1), &mut state),
        Err(Error::RevisionConflict)
    );
    let original_program = state.callbacks[1].program;
    state.callbacks[1].program = usize::MAX;
    assert_eq!(
        rt.restore_script_state(plan, Some(revision), &mut state),
        Err(Error::InvalidInput)
    );
    assert!(state.callbacks.iter().all(|c| c.behavior.is_none()));
    assert_eq!(rt.control_revision(plan), Ok(revision));
    state.callbacks[1].program = original_program;
    let last = state.values.len() - 1;
    state.values[last].address = ScriptStateAddress::Text {
        instance: ScriptInstanceId(0),
        index: u32::MAX,
    };
    let invalid = state.values.clone();
    assert_eq!(
        rt.capture_script_state(plan, &mut state),
        Err(Error::InvalidInput)
    );
    assert_eq!(state.values, invalid);
    state.values[0].value = ScriptStateValue::Control(ControlValue::Integer(88));
    assert_eq!(
        rt.restore_script_state(plan, Some(revision), &mut state),
        Err(Error::InvalidInput)
    );
    assert_eq!(
        rt.control_value(plan, sampler_ksp::derived_control_id(0, "$k")),
        Ok(ControlValue::Integer(0))
    );
    let mut too_large = ScriptStateBuffer::default();
    too_large.values.resize(4097, initial[0]);
    assert_eq!(
        client
            .submit(ControlRequest {
                plan,
                expected_revision: None,
                operation: ControlOperation::CaptureScriptState(too_large)
            })
            .unwrap_err()
            .reason,
        ControlQueueError::PayloadLimit
    );
}
