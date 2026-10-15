use sampler_core::{
    ChannelAddress, ControlContext, ControlValue, ControlWrite, Limits, Prepared, Protocol,
    Runtime, ScriptInstanceId,
};

#[test]
fn runtime_scalar_value_properties_are_consumed_without_invoking_target_callbacks() {
    let source = r#"on init
        make_perfview
        declare ui_button $apply
        declare ui_button $button
        declare ui_switch $switch
        declare ui_knob $knob(0,100,1)
        declare ui_menu $menu
        add_menu_item($menu,"First",30)
        add_menu_item($menu,"Last",99)
        declare ui_knob $calls(0,100,1)
        declare ui_button $bad
        declare ui_button $outside
        declare %ids[6]
        %ids[0] := get_ui_id($button)
        %ids[1] := get_ui_id($switch)
        %ids[2] := get_ui_id($knob)
        %ids[3] := get_ui_id($menu)
        %ids[4] := get_ui_id($bad)
        %ids[5] := get_ui_id($outside)
    end on
    on ui_control($apply)
        set_control_par(%ids[0],$CONTROL_PAR_VALUE,1)
        set_control_par(%ids[1],$CONTROL_PAR_VALUE,1)
        set_control_par(%ids[2],$CONTROL_PAR_VALUE,42)
        set_control_par(%ids[3],$CONTROL_PAR_VALUE,99)
        set_control_par_str(%ids[4],$CONTROL_PAR_VALUE,"bad")
        set_control_par(%ids[5],$CONTROL_PAR_VALUE,5)
    end on
    on ui_control($button)
        inc($calls)
    end on"#;
    let script = sampler_ksp::compile(source, 48000, sampler_ksp::Limits::LIBRARY, &[]).unwrap();
    let mut view = script.view();
    let prepared = script
        .bind(Prepared::new(48000, vec![], vec![], 1).unwrap())
        .unwrap();
    let mut limits = Limits::for_plan(&prepared, 8, 0);
    limits.behaviors = 16;
    limits.behavior_cells = prepared.behavior_local_count() * 16;
    let mut runtime = Runtime::new(prepared, limits).unwrap();
    let plan = runtime.active_plan();
    let context = ControlContext {
        performance: runtime.performance(0).unwrap(),
        origin: ChannelAddress {
            protocol: Protocol::Native,
            port: 0,
            group: 0,
            channel: 0,
        },
        channels: 1,
    };
    runtime
        .invoke_control(
            context,
            plan,
            None,
            ControlWrite {
                id: sampler_ksp::derived_control_id(0, "$apply"),
                value: ControlValue::Integer(1),
            },
        )
        .unwrap();
    runtime.flush_behaviors(|_, _, _| true);
    let mut effects = 0;
    runtime.drain_effects(|effect| {
        effects += 1;
        assert!(view.apply_ui_effect_for(plan, ScriptInstanceId(0), effect));
        true
    });
    assert_eq!(effects, 6);
    assert_eq!(
        runtime.control_value(plan, sampler_ksp::derived_control_id(0, "$calls")),
        Ok(ControlValue::Integer(0))
    );
    let face = view.ui(&|_| None).unwrap();
    for (name, value) in [("$button", 1), ("$switch", 1), ("$knob", 42), ("$menu", 99)] {
        let (index, widget) = face
            .widgets
            .iter()
            .enumerate()
            .find(|(_, w)| w.name == name)
            .unwrap();
        assert_eq!(widget.value, Some(sampler_ui_ir::Value::Integer(value)));
        assert!(
            !face
                .unsupported
                .iter()
                .any(|gap| gap.widget == Some(sampler_ui_ir::WidgetRef(index))
                    && gap.feature == "$CONTROL_PAR_VALUE"),
            "{name}"
        );
    }
    for name in ["$bad", "$outside"] {
        let index = face.widgets.iter().position(|w| w.name == name).unwrap();
        assert!(
            face.unsupported
                .iter()
                .any(|gap| gap.widget == Some(sampler_ui_ir::WidgetRef(index))
                    && gap.feature == "$CONTROL_PAR_VALUE"),
            "{name}"
        );
    }
    assert!(runtime.take_fault().is_none());
}
