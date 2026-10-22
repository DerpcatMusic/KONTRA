//! Conformance: read_persistent_var. Kontakt accepts it only in `on init` and rejects
//! other callbacks with `read_persistent_var() can only be used in "on init" callback!`
//! (decompile: the same text is raised at both sites that check the callback).
use sampler_ksp::Limits;

fn compile(source: &str) -> Result<sampler_ksp::Script, sampler_ksp::Error> {
    sampler_ksp::compile(source, 48000, Limits::LIBRARY, &[])
}

#[test]
fn read_persistent_var_loads_in_on_init() {
    let source =
        "on init declare ui_knob $k(0,100,1) make_persistent($k) read_persistent_var($k) end on";
    assert!(compile(source).is_ok());
}

#[test]
fn read_persistent_var_rejects_other_callbacks_with_kontakt_text() {
    for callback in [
        "on note",
        "on release",
        "on controller",
        "on persistence_changed",
    ] {
        let source = format!(
            "on init declare ui_knob $k(0,100,1) make_persistent($k) end on {callback} read_persistent_var($k) end on"
        );
        let Err(err) = compile(&source) else {
            panic!("read_persistent_var accepted in {callback}")
        };
        let want = "read_persistent_var() can only be used in \"on init\" callback!";
        assert!(err.message.contains(want), "{callback}: {err}");
    }
}
