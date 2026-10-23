//! Polyphonic variable scope and initializer diagnostics. Kontakt KSP manual
//! (variables, `polyphonic $`): values exist per note event, survive into the
//! matching `on release`, and are only meaningful in note and release callbacks.
use sampler_ksp::Limits;

fn limits() -> Limits {
    Limits {
        ..Limits::LIBRARY
    }
}

fn warnings(source: &str) -> Vec<String> {
    sampler_ksp::compile(source, 48000, limits(), &[])
        .expect("script compiles")
        .warnings()
        .iter()
        .map(|w| w.message.clone())
        .collect()
}

#[test]
fn note_and_release_reads_are_in_scope() {
    let source = "on init declare polyphonic $a end on
        on note $a := 3 end on
        on release play_note($EVENT_NOTE, $a, 0, 0) end on";
    assert!(warnings(source).is_empty(), "{:?}", warnings(source));
}

#[test]
fn read_in_ui_control_warns_and_compiles() {
    // Shape seen in Audio Imperia Areia (ui_control and on controller reads).
    let source = "on init declare ui_button $b declare polyphonic $a declare $x end on
        on ui_control($b) $x := $a end on";
    let w = warnings(source);
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].contains("read outside a note callback"), "{w:?}");
}

#[test]
fn read_in_controller_warns() {
    let source = "on init declare polyphonic $a declare $x end on
        on controller $x := $a end on";
    assert!(
        warnings(source)
            .iter()
            .any(|m| m.contains("read outside a note callback"))
    );
}

#[test]
fn function_reached_from_note_is_in_scope() {
    // Functions are lowered per callback context, so a helper called from
    // on note reads the note's own cell without warning.
    let source = "on init declare polyphonic $a declare $x end on
        function helper() $x := $a end function
        on note helper() end on
        on release helper() end on";
    assert!(warnings(source).is_empty(), "{:?}", warnings(source));
}

#[test]
fn initializer_is_not_applied_and_not_reported() {
    // Kontakt keeps no note-owned initializer and prints no diagnostic for it.
    let source = "on init declare polyphonic $a := 5 end on on note end on";
    assert!(warnings(source).is_empty(), "{:?}", warnings(source));
}
