use sampler_ksp::{
    Environment, Limits,
    model::{PerformanceControl, Value, WidgetKind},
};

#[test]
fn initialization_uses_authored_control_ranges_and_is_consumed_at_the_host_rate() {
    let mut control = PerformanceControl::assumed("$Dial", WidgetKind::Slider);
    control.params = vec![0, 7];
    control.properties = vec![
        ("$CONTROL_PAR_POS_X".into(), Value::Int(91)),
        ("$CONTROL_PAR_POS_Y".into(), Value::Int(42)),
    ];
    let mut env = Environment::default();
    env.performance_view.controls.push(control);
    let source = r#"on init
        load_performance_view("test")
        set_engine_par($ENGINE_PAR_VOLUME, get_control_par(get_ui_id($Dial), $CONTROL_PAR_MAX_VALUE), -1, -1, -1)
        end on"#;
    let initialized = sampler_ksp::initialize(source, Limits::LIBRARY, &env).unwrap();
    assert_eq!(initialized.engine_pars()[0].value, 7);
    assert!(!initialized.writes_effect_slots());
    let script =
        sampler_ksp::compile_initialized(source, 44100, Limits::LIBRARY, &[], initialized).unwrap();
    let w = &script.model().interface.widgets[0];
    assert_eq!(w.kind, WidgetKind::Slider);
    assert_eq!(w.range, Some((0, 7)));
    assert_eq!(
        (w.int("$CONTROL_PAR_POS_X"), w.int("$CONTROL_PAR_POS_Y")),
        (Some(91), Some(42))
    );
}
