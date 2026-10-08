#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;
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
fn init_indexed_values_reach_typed_bank_readback() {
    let rt = runtime(
        "on init declare ui_table %t[4](1,1,100) declare ui_xy ?xy[2] set_control_par_arr(get_ui_id(%t),$CONTROL_PAR_VALUE,42,2) set_control_par_real_arr(get_ui_id(?xy),$CONTROL_PAR_VALUE,0.75,1) end on",
    );
    let plan = rt.active_plan();
    assert_eq!(
        rt.widget_value(plan, sampler_ksp::derived_control_id(0, "%t"), 2),
        Ok(WidgetValue::Integer(42))
    );
    assert_eq!(
        rt.widget_value(plan, sampler_ksp::derived_control_id(0, "?xy"), 1),
        Ok(WidgetValue::Real(0.75))
    );
}

#[test]
fn unsupported_init_address_is_observable_and_later_volume_write_changes_audio() {
    let script = sampler_ksp::compile_with("on init set_engine_par($ENGINE_PAR_ATTACK,123,0,9,-1) set_engine_par($ENGINE_PAR_VOLUME,500000,0,-1,-1) end on",48000,KspLimits::LIBRARY,&[],&Environment::default()).unwrap();
    let plan = script
        .bind(
            Prepared::new(
                48000,
                vec![Pcm::new(48000, vec![[0.5; 2]; 1024].into_boxed_slice()).unwrap()],
                vec![Region {
                    sample: 0,
                    key_low: 60,
                    key_high: 60,
                    root_key: None,
                    velocity_low: 0.,
                    velocity_high: 1.,
                    gain: 1.,
                    envelope: Envelope::default(),
                    playback: Playback::default(),
                }],
                1,
            )
            .unwrap()
            .with_groups(1, vec![Some(0)])
            .unwrap(),
        )
        .unwrap();
    let mut limits = Limits::for_plan(&plan, 4, 4);
    limits.behaviors = 4;
    limits.behavior_cells = plan.behavior_local_count() * 4;
    let mut rt = Runtime::new(plan, limits).unwrap();
    let unknown = EngineParameterAddress {
        parameter: engine_parameter_id("ENGINE_PAR_ATTACK").unwrap(),
        group: 0,
        slot: 9,
        generic: -1,
    };
    assert_eq!(
        rt.set_engine_parameter(unknown, 123),
        Err(Error::InvalidInput)
    );
    assert!(rt.take_fault().is_none());
    let mut outcomes = vec![];
    while let Some(outcome) = rt.take_engine_parameter_outcome() {
        outcomes.push(outcome);
    }
    assert!(
        outcomes
            .iter()
            .any(|o| o.address == Some(unknown) && o.write && o.result == Err(Error::InvalidInput))
    );
    assert!(outcomes.iter().any(|o| o.write && o.result == Ok(())));
    rt.trigger(
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
    let mut out = [[0.; 2]; 64];
    rt.render(&mut out).unwrap();
    assert!(
        (out[32][0] - 0.2506).abs() < 0.003,
        "authored volume must change rendered level"
    );
}

#[test]
fn typed_toggle_preserves_native_domain_validation() {
    let id = ControlId(42);
    let plan = Prepared::new(48000, vec![], vec![], 1)
        .unwrap()
        .with_controls(vec![ControlDefinition {
            id,
            domain: ControlDomain::Toggle,
            default: ControlValue::Toggle(false),
        }])
        .unwrap()
        .with_script_instances(vec![vec![]])
        .unwrap()
        .with_widgets(vec![WidgetDefinition {
            drop: None,
            id,
            source_slot: 0,
            ui_id: 1,
            instance: ScriptInstanceId(0),
            storage: WidgetStorage::Control(id),
            program: None,
            stage: 0,
        }])
        .unwrap();
    let limits = Limits::for_plan(&plan, 1, 1);
    let mut rt = Runtime::new(plan, limits).unwrap();
    let plan = rt.active_plan();
    let mut edit = WidgetEdit {
        id,
        index: 0,
        value: WidgetValue::Integer(1),
        interaction: WidgetInteraction::default(),
    };
    rt.invoke_widget(context(&rt), plan, None, &[edit]).unwrap();
    assert_eq!(rt.control_value(plan, id), Ok(ControlValue::Toggle(true)));
    assert_eq!(rt.widget_value(plan, id, 0), Ok(WidgetValue::Integer(1)));
    let revision = rt.control_revision(plan).unwrap();
    edit.value = WidgetValue::Integer(2);
    assert_eq!(
        rt.invoke_widget(context(&rt), plan, None, &[edit]),
        Err(Error::InvalidInput)
    );
    assert_eq!(rt.control_revision(plan), Ok(revision));
    assert_eq!(rt.control_value(plan, id), Ok(ControlValue::Toggle(true)));
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

#[test]
fn selected_file_keeps_selector_handle_and_fires_native_callback() {
    let mut rt = runtime(
        "on init declare ui_file_selector $files declare @name declare @stem declare @path declare $event end on on ui_control($files) @name:=fs_get_filename(get_ui_id($files),1) @stem:=fs_get_filename(get_ui_id($files),0) @path:=fs_get_filename(get_ui_id($files),2) $event:=($NI_MOUSE_EVENT_TYPE=$NI_MOUSE_EVENT_TYPE_DROP) end on",
    );
    let plan = rt.active_plan();
    let id = rt.widget_id(plan, 0, 32768).unwrap();
    let before = rt.script_cell(plan, ScriptInstanceId(0), 0).unwrap();
    let selected = Text::try_new("/library/MIDI/Phrase.mid").unwrap();
    rt.invoke_widget(
        context(&rt),
        plan,
        None,
        &[WidgetEdit {
            id,
            index: 0,
            value: WidgetValue::Text(selected),
            interaction: WidgetInteraction {
                event: WidgetEventType::Drop as i32,
                ..Default::default()
            },
        }],
    )
    .unwrap();
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 0), Ok(before));
    assert_eq!(
        rt.script_text(plan, ScriptInstanceId(0), 0)
            .unwrap()
            .as_str(),
        "Phrase.mid"
    );
    assert_eq!(
        rt.script_text(plan, ScriptInstanceId(0), 1)
            .unwrap()
            .as_str(),
        "Phrase"
    );
    assert_eq!(rt.script_text(plan, ScriptInstanceId(0), 2), Ok(selected));
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 1), Ok(1));
    let (mut rt, mut client) = rt.with_control_updates(1, WIDGET_EDIT_CAPACITY).unwrap();
    let request = client
        .submit(ControlRequest {
            plan,
            expected_revision: None,
            operation: ControlOperation::CaptureWidget(vec![WidgetEdit {
                id,
                index: 0,
                value: WidgetValue::Integer(0),
                interaction: Default::default(),
            }]),
        })
        .unwrap();
    assert_eq!(rt.poll_control_update(), Ok(Some(request)));
    let reply = client.reply().unwrap();
    assert_eq!(reply.result, Ok((1, 1)));
    let ControlOperation::CaptureWidget(values) = reply.command.operation else {
        panic!()
    };
    assert_eq!(values[0].value, WidgetValue::Text(selected));
    assert_eq!(
        Text::try_new(&"x".repeat(TEXT_CAPACITY + 1)),
        Err(Error::Capacity)
    );
}

#[test]
fn mouse_area_admits_owned_drop_paths_and_native_enter_leave_metadata() {
    let mut rt = runtime(
        "on init declare ui_mouse_area $area set_control_par(get_ui_id($area),$CONTROL_PAR_DND_ACCEPT_AUDIO,$NI_DND_ACCEPT_MULTIPLE) set_control_par(get_ui_id($area),$CONTROL_PAR_DND_ACCEPT_MIDI,$NI_DND_ACCEPT_ONE) set_control_par(get_ui_id($area),$CONTROL_PAR_RECEIVE_DRAG_EVENTS,1) declare $calls declare $audio_count declare $midi_count declare $inside declare @audio declare @midi end on on ui_control($area) inc($calls) $audio_count := num_elements(!NI_DND_ITEMS_AUDIO) $midi_count := num_elements(!NI_DND_ITEMS_MIDI) $inside := $NI_MOUSE_OVER_CONTROL if ($audio_count > 0) @audio := !NI_DND_ITEMS_AUDIO[0] end if if ($midi_count > 0) @midi := !NI_DND_ITEMS_MIDI[0] end if end on",
    );
    let plan = rt.active_plan();
    let id = rt.widget_id(plan, 0, 32768).unwrap();
    let interaction = WidgetInteraction {
        event: WidgetEventType::DndDrop as i32,
        mouse_over: true,
        ..Default::default()
    };
    let edits = [
        WidgetEdit {
            id,
            index: 0,
            interaction,
            value: WidgetValue::DropPath {
                kind: WidgetDropKind::Audio,
                path: Text::new("/tmp/native-test.wav"),
            },
        },
        WidgetEdit {
            id,
            index: 1,
            interaction,
            value: WidgetValue::DropPath {
                kind: WidgetDropKind::Midi,
                path: Text::new("/tmp/native-test.mid"),
            },
        },
    ];
    support::without_heap(|| {
        rt.invoke_widget(context(&rt), plan, None, &edits).unwrap();
    });
    assert_eq!(
        rt.widget_value(plan, id, 0),
        Ok(WidgetValue::Integer(0)),
        "native handle stays intact"
    );
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 1), Ok(1));
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 2), Ok(1));
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 3), Ok(1));
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 4), Ok(1));
    assert_eq!(
        rt.script_text(plan, ScriptInstanceId(0), 0)
            .unwrap()
            .as_str(),
        "/tmp/native-test.wav"
    );
    assert_eq!(
        rt.script_text(plan, ScriptInstanceId(0), 1)
            .unwrap()
            .as_str(),
        "/tmp/native-test.mid"
    );
    let revision = rt.control_revision(plan).unwrap();
    let oversize: Vec<_> = (0..=WIDGET_DROP_CAPACITY)
        .map(|index| WidgetEdit {
            id,
            index,
            interaction,
            value: edits[0].value,
        })
        .collect();
    assert_eq!(
        rt.invoke_widget(context(&rt), plan, None, &oversize),
        Err(Error::InvalidInput)
    );
    assert_eq!(rt.control_revision(plan), Ok(revision));
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 1), Ok(1));
    rt.invoke_widget(
        context(&rt),
        plan,
        None,
        &[WidgetEdit {
            id,
            index: 0,
            interaction: WidgetInteraction {
                event: WidgetEventType::DndDrag as i32,
                ..Default::default()
            },
            value: WidgetValue::Integer(0),
        }],
    )
    .unwrap();
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 1), Ok(2));
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 2), Ok(0));
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 3), Ok(0));
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 4), Ok(0));
    for event in [
        WidgetEventType::LeftButtonDown,
        WidgetEventType::LeftButtonUp,
    ] {
        rt.invoke_widget(
            context(&rt),
            plan,
            None,
            &[WidgetEdit {
                id,
                index: 0,
                interaction: WidgetInteraction {
                    event: event as i32,
                    ..Default::default()
                },
                value: WidgetValue::Integer(0),
            }],
        )
        .unwrap();
    }
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 1), Ok(4));
    assert_eq!(rt.widget_value(plan, id, 0), Ok(WidgetValue::Integer(0)));
}

#[test]
fn ordinary_mouse_area_buttons_need_no_drop_configuration() {
    let mut rt = runtime(
        "on init declare ui_mouse_area $area declare $calls declare $event end on on ui_control($area) inc($calls) $event := $NI_MOUSE_EVENT_TYPE end on",
    );
    let plan = rt.active_plan();
    let id = rt.widget_id(plan, 0, 32768).unwrap();
    for event in [
        WidgetEventType::LeftButtonDown,
        WidgetEventType::LeftButtonUp,
    ] {
        rt.invoke_widget(
            context(&rt),
            plan,
            None,
            &[WidgetEdit {
                id,
                index: 0,
                value: WidgetValue::Integer(0),
                interaction: WidgetInteraction {
                    event: event as i32,
                    ..Default::default()
                },
            }],
        )
        .unwrap();
        assert_eq!(
            rt.script_cell(plan, ScriptInstanceId(0), 2),
            Ok(event as i64)
        );
    }
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 1), Ok(2));
}
