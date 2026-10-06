use sampler_ir as ir;

#[test]
fn a_bound_script_hands_back_its_interface() {
    let mut instrument = ir::Instrument::default();
    instrument.behaviors.push(ir::Behavior {
        name: "panel".into(),
        language: ir::Language::Ksp,
        source: "on init\nmake_perfview\ndeclare ui_knob $tone(0, 100, 1)\nend on\n".into(),
        slot: None,
        state: Vec::new(),
        requires: Vec::new(),
    });
    let loaded = sampler_kontakt::prepare(instrument, 48000, Vec::new(), true).unwrap();
    assert_eq!(loaded.interfaces.len(), 1);
    assert!(!loaded.interfaces[0].widgets.is_empty(), "the knob is a widget");
    assert_eq!(loaded.interfaces[0].source, sampler_ui_ir::Source::Ksp { slot: 0 });

}

#[test]
fn a_failing_script_leaves_the_others_bound_in_their_slots() {
    let mut instrument = ir::Instrument::default();
    instrument.behaviors.push(ir::Behavior {
        name: "bad".into(),
        language: ir::Language::Ksp,
        source: "on init\ndeclare $x := \nend on\n".into(),
        slot: Some(0),
        state: Vec::new(),
        requires: Vec::new(),
    });
    instrument.behaviors.push(ir::Behavior {
        name: "panel".into(),
        language: ir::Language::Ksp,
        source: "on init\nmake_perfview\ndeclare ui_knob $tone(0, 100, 1)\nmake_persistent($tone)\nread_persistent_var($tone)\nend on\n".into(),
        slot: Some(3),
        state: vec![("$tone".into(), ir::Saved::Int(42))],
        requires: Vec::new(),
    });
    let loaded = sampler_kontakt::prepare(instrument, 48000, Vec::new(), true).unwrap();
    assert!(loaded.instrument.unsupported.iter().any(|u| u.feature == "script" && u.location == "bad"));
    assert_eq!(loaded.interfaces.len(), 1, "the good script keeps its interface");
    assert_eq!(loaded.interfaces[0].source, sampler_ui_ir::Source::Ksp { slot: 3 });
}
