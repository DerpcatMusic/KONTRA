//! Conformance: `on ui_control`. Kontakt names the first problem with
//! `on ui_control: Variable "<name>" was not declared!` (no such variable) or
//! `on ui_control: Variable "<name>" is not a UI control!` (a plain variable).
use sampler_ksp::Limits;

fn compile(source: &str) -> Result<sampler_ksp::Script, sampler_ksp::Error> {
    sampler_ksp::compile(source, 48000, Limits::LIBRARY, &[])
}

#[test]
fn on_ui_control_rejects_an_undeclared_variable_with_kontakt_text() {
    let source = "on init declare ui_knob $k(0,127,1) end on on ui_control($nope) end on";
    let Err(err) = compile(source) else {
        panic!("undeclared variable accepted")
    };
    assert!(
        err.message
            .contains("on ui_control: Variable \"$nope\" was not declared!"),
        "{err}"
    );
}

#[test]
fn on_ui_control_rejects_a_variable_that_is_not_a_ui_control_with_kontakt_text() {
    let source = "on init declare $plain end on on ui_control($plain) end on";
    let Err(err) = compile(source) else {
        panic!("non-UI variable accepted")
    };
    assert!(
        err.message
            .contains("on ui_control: Variable \"$plain\" is not a UI control!"),
        "{err}"
    );
}

#[test]
fn on_ui_control_accepts_a_declared_ui_control() {
    let source = "on init declare ui_knob $k(0,127,1) end on on ui_control($k) end on";
    assert!(compile(source).is_ok());
}
