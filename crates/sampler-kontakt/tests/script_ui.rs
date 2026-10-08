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
    let loaded = sampler_kontakt::prepare(instrument, Vec::new(), &Default::default()).unwrap();
    assert_eq!(loaded.interfaces.len(), 1);
    assert!(
        !loaded.interfaces[0].widgets.is_empty(),
        "the knob is a widget"
    );
    assert_eq!(
        loaded.interfaces[0].source,
        sampler_ui_ir::Source::Ksp { slot: 0 }
    );
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
    let loaded = sampler_kontakt::prepare(instrument, Vec::new(), &Default::default()).unwrap();
    assert!(
        loaded
            .instrument
            .unsupported
            .iter()
            .any(|u| u.feature == "script" && u.location == "bad")
    );
    assert_eq!(
        loaded.interfaces.len(),
        1,
        "the good script keeps its interface"
    );
    assert_eq!(
        loaded.interfaces[0].source,
        sampler_ui_ir::Source::Ksp { slot: 3 }
    );
}

#[test]
fn host_recall_restores_menu_values_before_persistence_changed() {
    let source = r#"on init
        make_perfview
        declare ui_menu $mode
        add_menu_item($mode, "Soft", 4)
        add_menu_item($mode, "Linear", -3)
        add_menu_item($mode, "Hard", 0)
        make_persistent($mode)
        read_persistent_var($mode)
        declare ui_knob $echo(-100,100,1)
    end on
    on persistence_changed
        $echo := $mode
        set_control_par_str($INST_WALLPAPER_ID, $CONTROL_PAR_PICTURE, "mode" & $mode)
    end on
    on ui_control($mode)
        $echo := $mode + 1
    end on"#;
    let instrument = || ir::Instrument {
        behaviors: vec![ir::Behavior { name: "menu".into(), language: ir::Language::Ksp, source: source.into(), slot: Some(3), state: vec![("$mode".into(), ir::Saved::Int(1))], requires: vec![] }],
        ..Default::default()
    };
    let native = sampler_kontakt::prepare(instrument(), vec![], &Default::default()).unwrap();
    let mode = sampler_ksp::derived_control_id(3, "$mode");
    let echo = sampler_ksp::derived_control_id(3, "$echo");
    let value = |loaded: &sampler_kontakt::Loaded, id| loaded.plan.controls().iter().find(|c| c.id == id).unwrap().default;
    assert_eq!(value(&native, mode), sampler_core::ControlValue::Integer(-3), "native Kontakt saves a menu position");
    for saved in [4, -3, 0] {
        let loaded = sampler_kontakt::prepare(instrument(), vec![], &sampler_kontakt::Options { control_values: vec![(mode, saved)], ..Default::default() }).unwrap();
        assert_eq!(value(&loaded, mode), sampler_core::ControlValue::Integer(i64::from(saved)), "host recall carries values, including values that are valid item positions");
        assert_eq!(value(&loaded, echo), sampler_core::ControlValue::Integer(i64::from(saved)));
        assert!(loaded.interfaces[0].assets.iter().any(|a| a.path == format!("Resources/pictures/mode{saved}.png")));
    }
}
