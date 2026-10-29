//! `play_note` offsets: the constant bound per script, or the first call with none.
use sampler_ksp::{Limits, StartOffsets, compile};

fn view(source: &str) -> sampler_ksp::ScriptView {
    compile(source, 48_000, Limits::LIBRARY, &[])
        .unwrap()
        .view()
}

#[test]
fn constant_offset_sets_the_bound_in_micros() {
    let script = view("on note\n play_note(60, 100, 100000, 250000)\nend on");
    assert_eq!(script.start_offsets.max_micros, 100_000);
    assert_eq!(script.start_offsets.unbounded, None);
    assert_eq!(script.unbounded_line, None);
}

#[test]
fn largest_constant_offset_wins() {
    let script = view(
        "on note\n play_note(60, 100, 20000, 250000)\n play_note(62, 100, 100000, 250000)\nend on",
    );
    assert_eq!(script.start_offsets.max_micros, 100_000);
    assert_eq!(script.start_offsets.unbounded, None);
}

#[test]
fn zero_offset_is_no_offset() {
    let script = view("on note\n play_note(60, 100, 0, 250000)\nend on");
    assert_eq!(script.start_offsets, StartOffsets::default());
}

#[test]
fn variable_offset_is_unbounded_at_its_call() {
    let source = "on init declare $x := 0 end on\non note\n play_note(60, 100, $x, 250000)\nend on";
    let script = view(source);
    let span = script
        .start_offsets
        .unbounded
        .expect("a variable offset has no bound");
    assert!(source[span.start as usize..].starts_with("play_note"));
    assert_eq!(script.unbounded_line, Some(3));
}

#[test]
fn first_unbounded_call_wins() {
    let source = "on init declare $x := 0 end on\non note\n play_note(60, 100, $x, 250000)\n play_note(62, 100, $x, 250000)\nend on";
    let script = view(source);
    let span = script.start_offsets.unbounded.unwrap();
    assert_eq!(script.unbounded_line, Some(3));
    assert_eq!(span.start as usize, source.find("play_note(60").unwrap());
}

#[test]
fn no_play_note_has_no_offsets() {
    let script = view("on note\n message(\"hi\")\nend on");
    assert_eq!(script.start_offsets, StartOffsets::default());
}
