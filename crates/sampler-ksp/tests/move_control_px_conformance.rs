//! Conformance: move_control_px. Kontakt 8 clamps X above 1000 and Y above 750 with a
//! warning, and a negative X or Y hides the control without moving it.
use sampler_ksp::Limits;

fn run(call: &str) -> sampler_ksp::Script {
    let source = format!("on init\n declare ui_button $B\n {call}\nend on");
    sampler_ksp::compile(&source, 48000, Limits::LIBRARY, &[]).unwrap()
}

fn widget(script: &sampler_ksp::Script) -> sampler_ui_ir::Widget {
    let face = script.ui(&|_| None).unwrap();
    face.widgets.into_iter().find(|w| w.name == "$B").unwrap()
}

#[test]
fn move_control_px_clamps_right_and_bottom_edges_with_kontakt_warnings() {
    let script = run("move_control_px($B, 1200, 900)");
    let text: Vec<_> = script.warnings().iter().map(|w| w.message.as_str()).collect();
    assert!(text.contains(&"move_control_px(): X position must be between 0 and 1000 pixels!"), "{text:?}");
    assert!(text.contains(&"move_control_px(): Y position must be between 0 and 750 pixels!"), "{text:?}");
    let w = widget(&script);
    assert_eq!((w.rect.x, w.rect.y, w.hidden), (1000.0, 750.0, false));
}

#[test]
fn move_control_px_in_range_places_without_warning() {
    let script = run("move_control_px($B, 1000, 750)");
    assert!(script.warnings().is_empty(), "{:?}", script.warnings());
    let w = widget(&script);
    assert_eq!((w.rect.x, w.rect.y, w.hidden), (1000.0, 750.0, false));
}

#[test]
fn move_control_px_negative_position_hides_and_keeps_its_place() {
    let script = run("move_control_px($B, 40, 50) move_control_px($B, -1, 5)");
    let w = widget(&script);
    assert_eq!((w.rect.x, w.rect.y, w.hidden), (40.0, 50.0, true));
}
