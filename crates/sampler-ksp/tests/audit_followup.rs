//! Authored follow-up probes. Ignored failures are measurements, not implementation fixes.
use sampler_ksp::{
    Environment, Limits,
    model::{Value, WidgetValue},
};
fn compile(source: &str, env: &Environment) -> sampler_ksp::Script {
    sampler_ksp::compile_with(source, 48000, Limits::LIBRARY, &[], env).unwrap()
}
#[test]
fn native_menu_position_and_compressed_tail_are_already_restored() {
    let script = compile(
        "on init declare ui_menu $m add_menu_item($m,\"a\",20) add_menu_item($m,\"b\",80) make_persistent($m) declare %a[5] make_persistent(%a) declare ui_label $l(1,1) end on on persistence_changed set_text($l,%a[4]) end on",
        &Environment {
            persisted: [("$m".into(), Value::Int(1))].into(),
            persisted_arrays: [("%a".into(), vec![Value::Int(7), Value::Int(-3)])].into(),
            ..Default::default()
        },
    );
    assert_eq!(
        script.model().interface.widgets[0].value,
        WidgetValue::Int(80)
    );
    assert_eq!(
        script.model().interface.widgets[1].text("$CONTROL_PAR_TEXT"),
        Some("-3")
    );
}
#[test]
#[ignore = "audit: explicit persistent read is restored again at init completion"]
fn explicit_persistent_read_consumes_the_pending_value() {
    let script = compile(
        "on init declare ui_knob $k(0,100,1) make_persistent($k) read_persistent_var($k) $k:=99 end on",
        &Environment {
            persisted: [("$k".into(), Value::Int(5))].into(),
            ..Default::default()
        },
    );
    assert_eq!(
        script.model().interface.widgets[0].value,
        WidgetValue::Int(99)
    );
}
#[test]
#[ignore = "audit: pending menu index is lost before menu items are constructed"]
fn persistent_menu_read_before_item_construction_preserves_the_index() {
    let script = compile(
        "on init declare ui_menu $m make_persistent($m) read_persistent_var($m) add_menu_item($m,\"a\",20) add_menu_item($m,\"b\",80) declare ui_label $l(1,1) set_text($l,$m) end on",
        &Environment {
            persisted: [("$m".into(), Value::Int(1))].into(),
            ..Default::default()
        },
    );
    assert_eq!(
        script.model().interface.widgets[1].text("$CONTROL_PAR_TEXT"),
        Some("80")
    );
}
#[test]
#[ignore = "audit: invalid saved menu position becomes an unrelated semantic value"]
fn invalid_native_menu_index_selects_first_item() {
    let script = compile(
        "on init declare ui_menu $m add_menu_item($m,\"a\",20) add_menu_item($m,\"b\",80) make_persistent($m) end on",
        &Environment {
            persisted: [("$m".into(), Value::Int(99))].into(),
            ..Default::default()
        },
    );
    assert_eq!(
        script.model().interface.widgets[0].value,
        WidgetValue::Int(20)
    );
}
