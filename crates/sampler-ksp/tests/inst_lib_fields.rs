//! `$INST_LIB_*_ID` are library-tab fields (instrument metadata, not UI controls). A script
//! that loads a performance view and sets them (Shreddage 3.5 E-Upright) must not get
//! "unknown UI id" or "not in the performance view" diagnostics.
use sampler_ksp::{Environment, Limits};

#[test]
fn library_tab_fields_are_known_instrument_metadata() {
    let source = r#"on init
        load_performance_view("s3")
        set_control_par_str($INST_LIB_COPYRIGHT_ID,$CONTROL_PAR_TEXT,"(c) test")
        set_control_par_str($INST_LIB_DESCRIPTION_ID,$CONTROL_PAR_TEXT,"concept")
        set_control_par_str($INST_LIB_PIC_ONE_ID,$CONTROL_PAR_PICTURE,"library_one")
        set_control_par_str($INST_LIB_PIC_TWO_ID,$CONTROL_PAR_PICTURE,"library_two")
        end on"#;
    let script = sampler_ksp::compile_with(
        source,
        48000,
        Limits::LIBRARY,
        &[],
        &Environment::default(),
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    let warnings: Vec<&str> = script.warnings().iter().map(|w| w.message.as_str()).collect();
    assert!(warnings.is_empty(), "{warnings:?}");
}
