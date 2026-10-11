//! Focused, opt-in witness for the two modulation patches; no corpus traversal.
#[allow(dead_code)]
#[path = "../examples/survey_unsupported.rs"]
mod survey;

#[test]
#[ignore = "requires the installed ANALOG STRINGS and Conflux libraries"]
fn analog_and_conflux_modulation_survey() {
    assert_eq!(
        survey::survey(concat!(
            "/mnt/MAIN_STORAGE/Libraries/Kontakt/ANALOG STRINGS/Instruments/ANALOG STRINGS.nki\n",
            "/mnt/MAIN_STORAGE/Libraries/Kontakt/Conflux 1.1.0 [Native Instruments]/Instruments/Conflux.nki\n",
        )),
        0
    );
}

#[allow(dead_code)]
#[path = "../examples/modulation_intent.rs"]
mod intent;

#[test]
#[ignore = "requires the installed ANALOG STRINGS and Conflux libraries"]
fn analog_and_conflux_modulation_intent() {
    intent::main();
}
