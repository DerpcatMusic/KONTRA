//! `$MARK_n` are event-mark bits 2^n. Kontakt 8 values were measured through the oracle
//! (MARK_1=2, MARK_2=4, MARK_3=8, MARK_5=32, MARK_10=1024, MARK_11=2048); a script using them
//! must compile without a warning about an unsupported constant.
use sampler_ksp::{Environment, Limits, compile_with};

#[test]
fn mark_constants_compile_as_event_mark_bits() {
    let source = "on init\n declare $m := $MARK_10 + $MARK_1\nend on\non note\n set_event_mark($EVENT_ID, $MARK_11)\nend on";
    let script = compile_with(
        source,
        48000,
        Limits {
            source_bytes: 65536,
            instructions: 4096,
            variables: 16,
            array_cells: 16,
        },
        &[],
        &Environment::default(),
    )
    .unwrap();
    assert!(script.warnings().is_empty(), "{:?}", script.warnings());
}
