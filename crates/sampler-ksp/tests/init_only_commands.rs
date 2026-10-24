//! Conformance: commands the KSP manual restricts to `on init` (add_menu_item,
//! make_persistent, set_script_title, set_ui_height_px). Kontakt rejects them in any
//! other callback at load time with `<name>() can only be used in "on init" callback!`.
use sampler_ksp::Limits;

fn compile(source: &str) -> Result<sampler_ksp::Script, sampler_ksp::Error> {
    sampler_ksp::compile(source, 48000, Limits::LIBRARY, &[])
}

const CALLS: [(&str, &str); 4] = [
    ("add_menu_item", "add_menu_item($m, \"a\", 1)"),
    ("make_persistent", "make_persistent($v)"),
    ("set_script_title", "set_script_title(\"title\")"),
    ("set_ui_height_px", "set_ui_height_px(300)"),
];

#[test]
fn init_only_commands_load_in_on_init() {
    for (name, call) in CALLS {
        let source = format!("on init declare ui_menu $m declare $v {call} end on");
        assert!(compile(&source).is_ok(), "{name} in on init");
    }
}

#[test]
fn init_only_commands_reject_other_callbacks_with_kontakt_text() {
    for (name, call) in CALLS {
        for callback in ["on note", "on release", "on controller"] {
            let source =
                format!("on init declare ui_menu $m declare $v end on {callback} {call} end on");
            let Err(err) = compile(&source) else {
                panic!("{name} accepted in {callback}")
            };
            let want = format!("{name}() can only be used in \"on init\" callback!");
            assert!(err.message.contains(&want), "{callback}: {err}");
        }
    }
}

#[test]
fn init_only_commands_reached_through_functions_follow_their_caller() {
    // Kontakt rejects `call` in on init (docs/re/KSP_CONFORMANCE.md), so the init-only
    // command cannot be reached this way from on init. Reaching it from other callbacks
    // is covered by the callback tests above.
    let source = "on init declare ui_menu $m declare $v call setup end on function setup add_menu_item($m, \"a\", 1) end function";
    let Err(err) = compile(source) else {
        panic!("call in on init accepted")
    };
    assert!(err.message.contains("call is not allowed in on init"), "{err}");
}
