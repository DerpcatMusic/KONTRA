//! Conformance: read_persistent_var. Kontakt accepts it only in `on init`
//! (`read_persistent_var() can only be used in "on init" callback!`) and rejects an
//! undeclared name with `read_persistent_var(): Variable "<name>" was not declared!`.
use sampler_ksp::Limits;

fn compile(source: &str) -> Result<sampler_ksp::Script, sampler_ksp::Error> {
    sampler_ksp::compile(source, 48000, Limits::LIBRARY, &[])
}

#[test]
fn read_persistent_var_loads_in_on_init_for_a_declared_variable() {
    let source = "on init declare $v make_persistent($v) read_persistent_var($v) end on";
    assert!(compile(source).is_ok());
}

#[test]
fn read_persistent_var_rejects_other_callbacks_with_kontakt_text() {
    for callback in ["on note", "on release", "on controller"] {
        let source = format!("on init declare $v end on {callback} read_persistent_var($v) end on");
        let Err(err) = compile(&source) else {
            panic!("accepted in {callback}")
        };
        assert!(
            err.message
                .contains("read_persistent_var() can only be used in \"on init\" callback!"),
            "{callback}: {err}"
        );
    }
}

#[test]
fn read_persistent_var_rejects_an_undeclared_variable_with_kontakt_text() {
    let source = "on init read_persistent_var($nope) end on";
    let Err(err) = compile(source) else {
        panic!("undeclared variable accepted")
    };
    assert!(
        err.message
            .contains("read_persistent_var(): Variable \"$nope\" was not declared!"),
        "{err}"
    );
}
