use sampler_ksp::nckp::view_name;

fn authored_view(text: serde_json::Value, table: bool) -> sampler_ksp::Script {
    let controls = if table {
        serde_json::json!([{"index":9,"value":{"common":{"id":"Steps","size":{"width":160,"height":60}},
            "maxValue":100,"steps":{"total":32,"visible":8}}}])
    } else {
        serde_json::json!([{"index":1,"value":{"common":{"id":"Button","size":{"width":90,"height":24}},"text":text}}])
    };
    let (view, warnings) = sampler_ksp::nckp::parse(&serde_json::to_vec(&serde_json::json!({"value":{"performanceView":{"controls":controls}}})).unwrap()).unwrap();
    assert!(warnings.is_empty(), "authored presentation fields must be imported: {warnings:?}");
    sampler_ksp::compile_with(r#"on init
        declare $existing_font := get_font_id("Earlier")
        load_performance_view("fixture")
        end on"#,48000,sampler_ksp::Limits::LIBRARY,&[],&sampler_ksp::Environment {
        performance_view: view, ..Default::default()
    }).unwrap()
}

#[test]
fn nckp_custom_font_uses_the_existing_runtime_font_registry() {
    let script = authored_view(serde_json::json!({"string":"Caption","font":{"type":3,"custom":"CaptionFont"}}),false);
    let face = script.ui(&|_|None).unwrap();
    let style = &face.styles[face.widgets[0].style.unwrap().0];
    let sampler_ui_ir::Font::Bitmap(asset) = style.font else { panic!("custom font was lost") };
    assert_eq!(face.assets[asset.0].path,"Resources/pictures/CaptionFont.png");
    assert_eq!(script.model().interface.fonts, ["Earlier","CaptionFont"]);
}

#[test]
fn nckp_state_fonts_override_base_and_keep_absent_state_inheritance() {
    let script = authored_view(serde_json::json!({"font":{"type":3,"custom":""},"fonts":{
        "off":{"type":1,"custom":""},"on":{"type":2,"custom":""},
        "pressedOff":{"type":4,"custom":"PressedFont"},
        "pressedOn":{"type":5,"custom":""},"hoverOff":{"type":6,"custom":""},
        "hoverOn":{"type":7,"custom":"HoverFont"}}}),false);
    let face = script.ui(&|_|None).unwrap();
    let w = &face.widgets[0];
    for (at,font) in [(0,1),(1,2),(3,5),(4,6)] {
        assert_eq!(face.styles[w.state_styles[at].unwrap().0].font,sampler_ui_ir::Font::Stock(font));
    }
    for (at,name) in [(2,"PressedFont"),(5,"HoverFont")] {
        let sampler_ui_ir::Font::Bitmap(asset) = face.styles[w.state_styles[at].unwrap().0].font else { panic!() };
        assert_eq!(face.assets[asset.0].path,format!("Resources/pictures/{name}.png"));
    }
    let script = authored_view(serde_json::json!({"font":{"type":3,"custom":""},"fonts":{"off":{"type":1,"custom":""}}}),false);
    assert!(script.ui(&|_|None).unwrap().widgets[0].state_styles[1..].iter().all(Option::is_none));
}

#[test]
fn nckp_press_shift_flag_reaches_the_authored_control_property() {
    for shift in [false,true] {
        let script = authored_view(serde_json::json!({"shiftOnPress":shift}),false);
        assert_eq!(script.model().interface.widgets[0].int("$CONTROL_PAR_DISABLE_TEXT_SHIFTING"),Some(i32::from(!shift)));
    }
}

#[test]
fn nckp_visible_steps_keeps_storage_and_projects_the_authored_prefix() {
    let script = authored_view(serde_json::Value::Null,true);
    let face = script.ui(&|_|None).unwrap();
    let sampler_ui_ir::Kind::Table { columns,steps_shown,cells,.. } = &face.widgets[0].kind else { panic!() };
    assert_eq!((*columns,*steps_shown,cells.len()),(32,Some(8),32));
}

#[test]
fn resource_name_comes_from_a_complete_literal_init_command() {
    for (source, expected) in [
        (r#"on init load_performance_view("real") end on"#, "real"),
        (
            r#"{ load_performance_view("decoy") }
               on init load_performance_view("real") end on"#,
            "real",
        ),
        (
            r#"on init message("load_performance_view")
               load_performance_view("real") end on"#,
            "real",
        ),
        (
            r#"on init message("load_performance_view(decoy)")
               load_performance_view { comment } ...
               ("réal view") end on"#,
            "réal view",
        ),
        (r#"on init load_performance_view(("real")) end on"#, "real"),
        // The current KSP lexer preserves backslashes; do not unescape them here.
        (
            r#"on init load_performance_view("folder\real") end on"#,
            r"folder\real",
        ),
        (
            r#"USE_CODE_IF(OFF)
               on init load_performance_view("decoy") end on
               END_USE_CODE
               on init load_performance_view("real") end on"#,
            "real",
        ),
        (
            r#"SET_CONDITION(ON)
               USE_CODE_IF(ON)
               on init load_performance_view("real") end on
               END_USE_CODE"#,
            "real",
        ),
        (
            r#"on init if (1 = 1) load_performance_view("real") end if end on"#,
            "real",
        ),
        (
            r#"on init while (0) load_performance_view("real") end while end on"#,
            "real",
        ),
        (
            r#"on init select (1) case 1 load_performance_view("real") end select end on"#,
            "real",
        ),
    ] {
        assert_eq!(view_name(source), Some(expected), "{source}");
    }
}

#[test]
fn resource_name_never_guesses_from_nonliteral_or_ambiguous_source() {
    for source in [
        "on init end on",
        r#"{ load_performance_view("decoy") } on init end on"#,
        r#"on init message("load_performance_view(decoy)") end on"#,
        r#"on init load_performance_view_extra("decoy") end on"#,
        r#"on init declare @view := "real" load_performance_view(@view)
           message("unrelated") end on"#,
        r#"on init load_performance_view("base" & "suffix") end on"#,
        r#"on init declare @suffix := "suffix"
           load_performance_view("base" & @suffix) end on"#,
        r#"on init load_performance_view("real", "extra") end on"#,
        r#"on init load_performance_view() message("unrelated") end on"#,
        r#"on init load_performance_view("first") load_performance_view("second") end on"#,
        r#"on init if (1 = 1) load_performance_view("first")
           else load_performance_view("second") end if end on"#,
        r#"on init load_performance_view("first") end on
           on init load_performance_view("second") end on"#,
        r#"on note load_performance_view("runtime") end on"#,
        r#"function view load_performance_view("indirect") end function
           on init call view end on"#,
        r#"on init make_perfview load_performance_view("real") end on"#,
        r#"on init load_performance_view("real") make_perfview end on"#,
        r#"on init load_performance_view("real") end"#,
        r#"on init load_performance_view("real" end on"#,
        r#"on init load_performance_view("unterminated) end on"#,
        r#"{ unterminated load_performance_view("real")"#,
        // Embedded escaped quotes are not supported by the compiler's lexer.
        r#"on init load_performance_view("escaped\"quote") end on"#,
        r#"USE_CODE_IF(OFF) on init load_performance_view("real") end on"#,
    ] {
        assert_eq!(view_name(source), None, "{source}");
    }
}
