//! Audit probes pin observed gaps, not the desired Kontakt behavior.
use sampler_core::*;
use sampler_ksp::model::{Value, WidgetValue};

fn compile(source: &str, env: &sampler_ksp::Environment) -> sampler_ksp::Script {
    sampler_ksp::compile_with(
        source,
        48000,
        sampler_ksp::Limits {
            source_bytes: 65536,
            instructions: 4096,
            variables: 64,
            array_cells: 64,
        },
        &[],
        env,
    )
    .unwrap()
}

fn runtime(script: sampler_ksp::Script) -> Runtime {
    let plan = script
        .bind(Prepared::new(48000, vec![], vec![], 0).unwrap())
        .unwrap();
    let behavior_cells = plan.behavior_local_count() * 4;
    Runtime::new(
        plan,
        Limits {
            notes: 4,
            channels: 0,
            performances: 1,
            families: 4,
            expressions: 4,
            voices: 0,
            decisions: 0,
            commands: 16,
            behaviors: 4,
            behavior_fuel: 4096,
            behavior_cells,
            note_cells: 0,
        },
    )
    .unwrap()
}

fn click(rt: &mut Runtime) {
    let context = ControlContext {
        performance: rt.performance(0).unwrap(),
        origin: ChannelAddress {
            protocol: Protocol::Native,
            port: 0,
            group: 0,
            channel: 0,
        },
        channels: 1,
    };
    rt.invoke_control(
        context,
        rt.active_plan(),
        None,
        ControlWrite {
            id: sampler_ksp::derived_control_id(0, "$B"),
            value: ControlValue::Integer(1),
        },
    )
    .unwrap();
}

#[test]
fn audit_real_scalar_properties_are_integer_at_init() {
    let script = compile(
        "on init declare ui_xy ?P[2]\nset_control_par_real(get_ui_id(?P), $CONTROL_PAR_RANGE_MIN, 0.25)\nend on",
        &Default::default(),
    );
    assert_eq!(
        script.model().interface.widgets[0].properties["$CONTROL_PAR_RANGE_MIN"],
        Value::Int(0)
    );
}

#[test]
fn audit_read_persistent_var_restores_the_same_saved_entry_twice() {
    let env = sampler_ksp::Environment {
        persisted: [("$K".into(), Value::Int(7))].into(),
        ..Default::default()
    };
    let script = compile(
        "on init declare ui_knob $K(0,100,1) make_persistent($K)\nread_persistent_var($K)\n$K := 9\nend on",
        &env,
    );
    // The RE pending-entry model expects 9; this evaluator restores 7 again.
    assert_eq!(
        script.model().interface.widgets[0].value,
        WidgetValue::Int(7)
    );
}

#[test]
fn audit_runtime_alias_effects_are_emitted_but_unhandled() {
    let script = compile(
        "on init declare ui_button $B declare ui_label $L(1,1)\nend on\non ui_control($B)\nset_text($L, \"new\")\nhide_part($L,$HIDE_WHOLE_CONTROL)\nmove_control_px($L,40,50)\nend on",
        &Default::default(),
    );
    let mut view = script.view();
    let before = view.model().clone();
    let mut rt = runtime(script);
    click(&mut rt);
    let mut effects = 0;
    rt.drain_effects(|effect| {
        effects += 1;
        assert!(!view.apply_ui_effect(effect));
        true
    });
    assert_eq!(effects, 3);
    assert_eq!(view.model(), &before);
}

#[test]
fn audit_indexed_readback_ignores_index_and_does_not_seed_init_cells() {
    let script = compile(
        "on init declare ui_button $B declare ui_knob $R(0,100,1)\ndeclare ui_knob $S(0,100,1) declare ui_table %T[4](2,2,100)\nset_control_par_arr(get_ui_id(%T),$CONTROL_PAR_VALUE,7,2)\nend on\non ui_control($B)\n$R := get_control_par_arr(get_ui_id(%T),$CONTROL_PAR_VALUE,2)\nset_control_par_arr(get_ui_id(%T),$CONTROL_PAR_VALUE,11,1)\nset_control_par_arr(get_ui_id(%T),$CONTROL_PAR_VALUE,22,2)\n$S := get_control_par_arr(get_ui_id(%T),$CONTROL_PAR_VALUE,1)\nend on",
        &Default::default(),
    );
    let mut rt = runtime(script);
    click(&mut rt);
    for (name, observed) in [("$R", 0), ("$S", 22)] {
        assert_eq!(
            rt.control_value(rt.active_plan(), sampler_ksp::derived_control_id(0, name)),
            Ok(ControlValue::Integer(observed))
        );
    }
}

#[test]
fn audit_runtime_string_and_real_getters_are_ignored() {
    let script = compile(
        "on init declare ui_button $B declare @s declare ~r\nend on\non ui_control($B)\n@s := get_control_par_str(get_ui_id($B),$CONTROL_PAR_TEXT)\n~r := get_control_par_real(get_ui_id($B),$CONTROL_PAR_VALUE)\nend on",
        &Default::default(),
    );
    for name in ["get_control_par_str", "get_control_par_real"] {
        assert!(
            script
                .coverage()
                .iter()
                .any(|(n, c, _)| *n == name && *c == sampler_ksp::Coverage::Ignored)
        );
    }
}

#[test]
fn audit_alignment_only_is_not_projected_without_font_type() {
    let script = compile(
        "on init declare ui_label $L(1,1)\nset_control_par(get_ui_id($L),$CONTROL_PAR_TEXT_ALIGNMENT,2)\nend on",
        &Default::default(),
    );
    let ui = script.ui(&|_| None).unwrap();
    assert_eq!(ui.widgets[0].style, None);
    assert!(ui.unsupported.is_empty());
}
