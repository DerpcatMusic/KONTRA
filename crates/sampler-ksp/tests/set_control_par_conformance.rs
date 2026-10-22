//! Conformance: set_control_par. Kontakt rejects a constant $CONTROL_PAR_TYPE as the
//! parameter with `The value of $CONTROL_PAR_TYPE cannot be changed! ...`; the value
//! is only readable through get_control_par.
use sampler_ksp::Limits;

fn compile(source: &str) -> Result<sampler_ksp::Script, sampler_ksp::Error> {
    sampler_ksp::compile(source, 48000, Limits::LIBRARY, &[])
}

#[test]
fn set_control_par_rejects_writing_control_par_type_with_kontakt_text() {
    let decl = "on init declare $x declare ui_knob $k(0,127,1) end on";
    for call in [
        "set_control_par(get_ui_id($k), $CONTROL_PAR_TYPE, 1)",
        "set_control_par_str(get_ui_id($k), $CONTROL_PAR_TYPE, \"x\")",
        "set_control_par_real(get_ui_id($k), $CONTROL_PAR_TYPE, 1.0)",
    ] {
        let source = format!("{decl} on note {call} end on");
        let Err(err) = compile(&source) else {
            panic!("{call} accepted")
        };
        assert!(
            err.message.contains(
                "The value of $CONTROL_PAR_TYPE cannot be changed! This control parameter only works with get_control_par()."
            ),
            "{call}: {err}"
        );
    }
}

#[test]
fn control_par_type_is_readable_through_get_control_par() {
    let source = "on init declare $x declare ui_knob $k(0,127,1) end on on note $x := get_control_par(get_ui_id($k), $CONTROL_PAR_TYPE) end on";
    assert!(compile(source).is_ok());
}
