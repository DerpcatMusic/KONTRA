//! Conformance: get_ui_id. Kontakt rejects an undeclared name or one that is not a
//! UI widget with `get_ui_id(): "<name>" was not declared or is not allowed here!`.
use sampler_ksp::Limits;

fn compile(source: &str) -> Result<sampler_ksp::Script, sampler_ksp::Error> {
    sampler_ksp::compile(source, 48000, Limits::LIBRARY, &[])
}

#[test]
fn get_ui_id_rejects_undeclared_and_non_ui_names_with_kontakt_text() {
    let undeclared = "on init declare $x end on on note $x := get_ui_id($nope) end on";
    let Err(err) = compile(undeclared) else {
        panic!("undeclared name accepted")
    };
    assert!(
        err.message
            .contains("get_ui_id(): \"$nope\" was not declared or is not allowed here!"),
        "{err}"
    );
    let plain = "on init declare $x declare $plain end on on note $x := get_ui_id($plain) end on";
    let Err(err) = compile(plain) else {
        panic!("non-UI name accepted")
    };
    assert!(
        err.message
            .contains("get_ui_id(): \"$plain\" was not declared or is not allowed here!"),
        "{err}"
    );
}

#[test]
fn get_ui_id_accepts_a_declared_ui_widget() {
    let source =
        "on init declare $x declare ui_knob $k(0,127,1) end on on note $x := get_ui_id($k) end on";
    assert!(compile(source).is_ok());
}
