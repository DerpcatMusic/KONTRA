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
fn global_ui_callbacks_run_around_the_local_callback_with_native_context() {
    let mut rt = runtime(
        "on init declare ui_button $a declare ui_button $b
        declare $order declare $global_id declare $local_id declare $update_id
        declare $global_type declare $local_type declare $update_type
        declare $a_id := get_ui_id($a) declare $b_id := get_ui_id($b) end on
        on ui_controls $order := $order*10+1 $global_id := $NI_UI_ID
            $global_type := $NI_CALLBACK_TYPE end on
        on ui_control($a) $order := $order*10+2 $local_id := $NI_UI_ID
            $local_type := $NI_CALLBACK_TYPE end on
        on ui_update $order := $order*10+3 $update_id := $NI_UI_ID
            $update_type := $NI_CALLBACK_TYPE end on",
    );
    let plan = rt.active_plan();
    let cell = |rt: &Runtime, index| rt.script_cell(plan, ScriptInstanceId(0), index).unwrap();
    rt.invoke_control(
        context(&rt),
        plan,
        None,
        ControlWrite {
            id: sampler_ksp::derived_control_id(0, "$a"),
            value: ControlValue::Integer(1),
        },
    )
    .unwrap();
    assert_eq!(cell(&rt, 0), 123);
    assert_eq!(
        (cell(&rt, 1), cell(&rt, 2), cell(&rt, 3)),
        (cell(&rt, 7), cell(&rt, 7), 0)
    );
    assert_eq!((cell(&rt, 4), cell(&rt, 5), cell(&rt, 6)), (13, 7, 8));
    rt.invoke_control(
        context(&rt),
        plan,
        None,
        ControlWrite {
            id: sampler_ksp::derived_control_id(0, "$b"),
            value: ControlValue::Integer(1),
        },
    )
    .unwrap();
    assert_eq!(cell(&rt, 0), 12313);
    assert_eq!(cell(&rt, 1), cell(&rt, 8));
    assert!(rt.take_fault().is_none());
}

#[test]
fn waiting_global_ui_callback_does_not_delay_local_or_update_callbacks() {
    let mut rt = runtime(
        "on init declare ui_button $a declare $order end on
        on ui_controls $order := $order*10+1 wait(1000) $order := $order*10+4 end on
        on ui_control($a) $order := $order*10+2 end on
        on ui_update $order := $order*10+3 end on",
    );
    let plan = rt.active_plan();
    rt.invoke_control(
        context(&rt),
        plan,
        None,
        ControlWrite {
            id: sampler_ksp::derived_control_id(0, "$a"),
            value: ControlValue::Integer(1),
        },
    )
    .unwrap();
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 0), Ok(123));
    let mut audio = [[0.; 2]; 64];
    rt.render(&mut audio).unwrap();
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 0), Ok(1234));
    assert!(rt.take_fault().is_none());
}

#[test]
fn global_ui_callback_receives_typed_widget_interaction_without_a_local_callback() {
    let mut rt = runtime(
        "on init declare ui_xy ?xy[2] declare $calls declare $index
        declare $event declare $id declare $expected := get_ui_id(?xy) end on
        on ui_controls inc($calls) $index := $NI_CONTROL_PAR_IDX
            $event := %EVENT_PAR[2] $id := $NI_UI_ID end on",
    );
    let plan = rt.active_plan();
    rt.invoke_widget(
        context(&rt),
        plan,
        None,
        &[WidgetEdit {
            id: sampler_ksp::derived_control_id(0, "?xy"),
            index: 1,
            interaction: WidgetInteraction {
                event_par: [4, 5, 6, 7],
                ..WidgetInteraction::default()
            },
            value: WidgetValue::Real(0.75),
        }],
    )
    .unwrap();
    let cell = |index| rt.script_cell(plan, ScriptInstanceId(0), index).unwrap();
    assert_eq!((cell(2), cell(3), cell(4)), (1, 1, 6));
    assert_eq!(cell(5), cell(6));
    assert!(rt.take_fault().is_none());
}

#[test]
fn global_ui_dispatch_keeps_program_offsets_and_script_instances_separate() {
    let source = "on init declare ui_button $a declare $order declare $slot end on
        on ui_controls $order := $order*10+1 $slot := $CURRENT_SCRIPT_SLOT end on
        on ui_update $order := $order*10+3 end on";
    let scripts = (0..2)
        .map(|slot| {
            sampler_ksp::compile_with(
                source,
                48000,
                KspLimits::LIBRARY,
                &[],
                &Environment {
                    slot,
                    ..Environment::default()
                },
            )
            .unwrap()
        })
        .collect();
    let plan = sampler_ksp::bind_modules(scripts, Prepared::new(48000, vec![], vec![], 1).unwrap())
        .unwrap();
    let mut limits = Limits::for_plan(&plan, 16, 16);
    limits.behaviors = 16;
    limits.behavior_cells = plan.behavior_local_count() * 16;
    let mut rt = Runtime::new(plan, limits).unwrap();
    let plan = rt.active_plan();
    rt.invoke_control(
        context(&rt),
        plan,
        None,
        ControlWrite {
            id: sampler_ksp::derived_control_id(1, "$a"),
            value: ControlValue::Integer(1),
        },
    )
    .unwrap();
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 0), Ok(0));
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(1), 0), Ok(13));
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(1), 1), Ok(1));
    assert!(rt.take_fault().is_none());
}

#[test]
fn update_without_global_controls_runs_after_local_and_on_widgets_without_local() {
    let mut rt = runtime(
        "on init declare ui_button $a declare ui_button $b declare $order
        declare $id declare $kind end on
        on ui_control($a) $order := $order*10+1 end on
        on ui_update $order := $order*10+2 $id := $NI_UI_ID $kind := $NI_CALLBACK_TYPE end on",
    );
    let plan = rt.active_plan();
    for name in ["$a", "$b"] {
        rt.invoke_control(
            context(&rt),
            plan,
            None,
            ControlWrite {
                id: sampler_ksp::derived_control_id(0, name),
                value: ControlValue::Integer(1),
            },
        )
        .unwrap();
    }
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 0), Ok(122));
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 1), Ok(0));
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 2), Ok(8));
    assert!(rt.take_fault().is_none());
}

/// Acceptance gate, not evidence of implementation. Uses the existing runtime
/// helper without a ScriptView or effect drain: GUI publication cannot own state.
#[test]
#[ignore = "UNIMPLEMENTED: runtime waveform attach/set/get state; NOT_RUN"]
fn waveform_headless_initial_seed_and_runtime_roundtrip_requirement() {
    let mut rt = runtime(
        "on init
         declare $seed_cursor declare $seed_table declare $cursor
         declare $flags declare $table declare $alias declare $midi
         declare ui_waveform $w(1,1) declare ui_button $apply
         attach_zone($w,27,3)
         set_ui_wf_property($w,$UI_WF_PROP_PLAY_CURSOR,0,12000)
         set_ui_wf_property($w,$UI_WF_PROP_TABLE_VAL,3,77)
         end on
         on ui_control($apply)
         $seed_cursor := get_ui_wf_property($w,$UI_WF_PROP_PLAY_CURSOR,0)
         $seed_table := get_ui_wf_property($w,$UI_WF_PROP_TABLE_VAL,3)
         attach_zone($w,27,3)
         set_ui_wf_property($w,$UI_WF_PROP_PLAY_CURSOR,0,24000)
         set_ui_wf_property($w,$UI_WF_PROP_FLAGS,0,11)
         set_ui_wf_property($w,$UI_WF_PROP_TABLE_VAL,3,99)
         set_ui_wf_property($w,$UI_WF_PROP_MIDI_DRAG_START_NOTE,0,65)
         $cursor := get_ui_wf_property($w,$UI_WF_PROP_PLAY_CURSOR,0)
         $flags := get_ui_wf_property($w,$UI_WF_PROP_FLAGS,0)
         $table := get_ui_wf_property($w,$UI_WF_PROP_TABLE_VAL,3)
         $alias := get_ui_wf_property($w,$UI_WF_PROP_TABLE_VAL,77)
         $midi := get_ui_wf_property($w,$UI_WF_PROP_MIDI_DRAG_START_NOTE,0)
         end on",
    );
    let plan = rt.active_plan();
    rt.invoke_control(
        context(&rt),
        plan,
        None,
        ControlWrite {
            id: sampler_ksp::derived_control_id(0, "$apply"),
            value: ControlValue::Integer(1),
        },
    )
    .unwrap();
    for (index, expected) in [12000, 77, 24000, 11, 99, 0, 65].into_iter().enumerate() {
        assert_eq!(
            rt.script_cell(plan, ScriptInstanceId(0), u32::try_from(index).unwrap()),
            Ok(expected)
        );
    }
    assert!(rt.take_fault().is_none());
}
