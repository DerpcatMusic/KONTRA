use sampler_ui_ir::{Binding, Kind, Rect, Role, Source};

#[test]
fn performance_intent_reaches_ir_without_being_inferred_from_native_ui() {
    for (request, performance, native) in [
        ("", false, false),
        ("make_perfview", true, false),
        ("load_performance_view(\"missing\")", true, false),
        ("load_native_ui(\"entry\")", false, true),
    ] {
        let script = sampler_ksp::compile(&format!("on init {request} end on"), 48000, sampler_ksp::Limits::LIBRARY, &[]).unwrap();
        let face = script.ui(&|_| None).unwrap();
        assert_eq!(face.performance, performance, "{request}");
        assert_eq!(face.performance, script.has_performance_view());
        assert_eq!(face.native_ui.is_some(), native, "{request}");
        assert!(face.widgets.is_empty());
    }
}

#[test]
fn ksp_interface_maps_to_validated_ui_ir() {
    let source = r#"on init
        make_perfview
        set_ui_height_px(300)
        set_control_par_str($INST_WALLPAPER_ID, $CONTROL_PAR_PICTURE, "wall")
        declare ui_panel $P
        set_control_par(get_ui_id($P), $CONTROL_PAR_POS_X, 10)
        declare ui_knob $Cut(0, 1000, 10)
        set_knob_unit($Cut, $KNOB_UNIT_HZ)
        set_control_par(get_ui_id($Cut), $CONTROL_PAR_PARENT_PANEL, get_ui_id($P))
        set_control_par_str(get_ui_id($Cut), $CONTROL_PAR_PICTURE, "knob")
        set_control_par(get_ui_id($Cut), $CONTROL_PAR_HIDE, $HIDE_PART_TITLE)
        set_control_par(get_ui_id($Cut), $CONTROL_PAR_MOUSE_BEHAVIOUR, -500)
        set_control_par(get_ui_id($Cut), $CONTROL_PAR_BAR_COLOR, 0FF0000H)
        set_control_par(get_ui_id($Cut), $CONTROL_PAR_ALLOW_AUTOMATION, 0)
        set_control_par(get_ui_id($Cut), $CONTROL_PAR_PICTURE_STATE, 3)
        set_control_par_str($INST_ICON_ID, $CONTROL_PAR_PICTURE, "icon")
        declare ui_label $L(1, 1)
        set_text($L, "hi")
        set_control_par_str(get_ui_id($L), $CONTROL_PAR_PICTURE, "knob")
        declare ui_table %T[8](2, 2, -64)
        set_control_par_arr(get_ui_id(%T), $CONTROL_PAR_VALUE, 7, 2)
        declare ui_level_meter $M
        attach_level_meter(get_ui_id($M), -1, -1, 1, 2)
        declare ui_button $B
        move_control($B, 2, 3)
        end on"#;
    let limits = sampler_ksp::Limits {
        source_bytes: 4096,
        instructions: 256,
        variables: 16,
        array_cells: 16,
    };
    let env = sampler_ksp::Environment {
        slot: 3,
        ..Default::default()
    };
    let script = sampler_ksp::compile_with(source, 48000, limits, &[], &env).unwrap();
    let ui = script
        .ui(&|path| {
            (path == "Resources/pictures/knob.png")
                .then(|| sampler_ksp::ui::picture_meta("Number of Animations: 31\n"))
        })
        .unwrap();
    assert_eq!(ui.source, Source::Ksp { slot: 3 });
    assert_eq!(ui.pages[0].size.height, 300.0);
    assert_eq!(
        ui.assets[ui.pages[0].background.image.unwrap().0].path,
        "Resources/pictures/wall.png"
    );
    let [panel, knob, label, table, meter, button] = &ui.widgets[..] else {
        panic!("{:?}", ui.widgets);
    };
    assert_eq!((panel.kind.clone(), panel.rect.x), (Kind::Panel, 10.0));
    assert_eq!(knob.parent, Some(sampler_ui_ir::WidgetRef(0)));
    assert!(knob.hide.title && !knob.hidden);
    let control = script.controls()[0].definition.id;
    assert_eq!(
        knob.binding,
        Binding::Control(sampler_ui_ir::ControlId(control.0))
    );
    let Kind::Knob { range, display } = &knob.kind else {
        panic!()
    };
    assert_eq!(
        (range.max, display.ratio, display.unit.as_str()),
        (1000.0, 10.0, "Hz")
    );
    // The knob's strip and the label's static picture share one asset.
    assert_eq!(knob.images[0].role, Role::Strip);
    assert_eq!(label.images[0].role, Role::Background);
    assert_eq!(knob.images[0].asset, label.images[0].asset);
    let sampler_ui_ir::AssetKind::Image(meta) = ui.assets[knob.images[0].asset.0].kind else {
        panic!()
    };
    assert_eq!(meta.frames, 31);
    assert_eq!(label.text, "hi");
    assert!(matches!(
        table.kind,
        Kind::Table {
            columns: 8,
            bipolar: true,
            ..
        }
    ));
    assert!(matches!(table.binding, Binding::Variable { script: 3, .. }));
    assert_eq!(
        meter.binding,
        Binding::Meter {
            bus: Some(2),
            channel: 1
        }
    );
    assert_eq!((knob.rect, knob.auto_size), (Rect::new(0, 0, 0, 0), true));
    assert_eq!(
        knob.drag,
        Some(sampler_ui_ir::Drag {
            axis: sampler_ui_ir::Orientation::Vertical,
            sensitivity: 500
        })
    );
    assert_eq!(knob.colors.bar, Some(sampler_ui_ir::Rgba::rgb(0xFF0000)));
    assert!(!knob.automation.allowed);
    assert_eq!(knob.images[0].frame, Some(3));
    assert!(ui.icon.is_some());
    let Kind::Table { cells, .. } = &table.kind else {
        panic!()
    };
    assert_eq!(cells[2], 7.);
    assert_eq!(
        button.placement,
        sampler_ui_ir::Placement::Grid { column: 2, row: 3 }
    );
    assert!(ui.unsupported.is_empty(), "{:?}", ui.unsupported);
}

#[test]
fn saved_state_and_new_widget_fields_reach_the_ir() {
    // Afflatus-style wallpaper chosen by a persistent menu: 0 at init gives an
    // empty picture name; the saved value restores it before the snapshot.
    let source = r#"on init
        make_perfview
        declare !bg[2] := ("dark", "light")
        declare ui_menu $Bg
        add_menu_item($Bg, "dark", 1)
        add_menu_item($Bg, "light", 2)
        make_persistent($Bg)
        set_control_par(get_ui_id($Bg), $CONTROL_PAR_HIDE, $HIDE_WHOLE_CONTROL)
        set_control_par($INST_ICON_ID, $CONTROL_PAR_HIDE, $HIDE_WHOLE_CONTROL)
        declare ui_table %T[4](2, 2, 100)
        set_table_steps_shown(%T, 8)
        declare ui_file_selector $F
        set_control_par(get_ui_id($F), $CONTROL_PAR_FILE_TYPE, $NI_FILE_TYPE_ARRAY)
        set_control_par_str(get_ui_id($F), $CONTROL_PAR_BASEPATH, "/presets")
        set_control_par(get_ui_id($F), $CONTROL_PAR_COLUMN_WIDTH, 120)
        end on
        on persistence_changed
        set_control_par_str($INST_WALLPAPER_ID, $CONTROL_PAR_PICTURE, !bg[$Bg - 1])
        end on"#;
    let limits = sampler_ksp::Limits {
        source_bytes: 4096,
        instructions: 256,
        variables: 16,
        array_cells: 16,
    };
    let wallpaper = |env: &sampler_ksp::Environment| {
        let script = sampler_ksp::compile_with(source, 48000, limits, &[], env).unwrap();
        let ui = script.ui(&|_| None).unwrap();
        (
            ui.pages[0]
                .background
                .image
                .map(|a| ui.assets[a.0].path.clone()),
            ui,
        )
    };
    let (unsaved, ui) = wallpaper(&Default::default());
    assert_eq!(unsaved, None);
    assert!(ui.assets.iter().all(|a| !a.path.ends_with("/.png")));
    assert!(
        ui.unsupported
            .iter()
            .any(|u| u.feature == "$INST_WALLPAPER_ID (empty picture name)")
    );
    assert!(ui.icon_hidden);
    let [_, table, files] = &ui.widgets[..] else {
        panic!("{:?}", ui.widgets);
    };
    assert!(matches!(
        table.kind,
        Kind::Table {
            steps_shown: Some(8),
            ..
        }
    ));
    assert_eq!(
        files.kind,
        Kind::FileSelector {
            base_path: Some("/presets".into()),
            files: sampler_ui_ir::Files::Data,
            column_width: Some(120),
        }
    );
    let env = sampler_ksp::Environment {
        persisted: [("$Bg".into(), sampler_ksp::model::Value::Int(1))].into(),
        ..Default::default()
    };
    let (saved, _) = wallpaper(&env);
    assert_eq!(saved.as_deref(), Some("Resources/pictures/light.png"));
}

#[test]
fn picture_size_is_one_frame_of_the_png() {
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    png.extend(64u32.to_be_bytes());
    png.extend(640u32.to_be_bytes());
    let meta = sampler_ksp::ui::picture_meta_png(Some("Number of Animations: 10\n"), &png);
    assert_eq!(
        meta.size,
        Some(sampler_ui_ir::Size {
            width: 64.0,
            height: 64.0
        })
    );
}

#[test]
fn vendor_control_pars_keep_their_names() {
    let source = r#"on init
        declare ui_knob $K(0, 10, 1)
        set_control_par(get_ui_id($K), $CONTROL_PAR_NKS_TYPE, 1)
        set_control_par_str_arr(get_ui_id($K), $CONTROL_PAR_NKS_STR_VALUES, "a", 0)
        end on"#;
    let limits = sampler_ksp::Limits {
        source_bytes: 4096,
        instructions: 256,
        variables: 16,
        array_cells: 16,
    };
    let script =
        sampler_ksp::compile_with(source, 48000, limits, &[], &Default::default()).unwrap();
    let ui = script.ui(&|_| None).unwrap();
    let features: Vec<_> = ui.unsupported.iter().map(|u| u.feature.as_str()).collect();
    assert_eq!(
        features,
        ["$CONTROL_PAR_NKS_TYPE", "$CONTROL_PAR_NKS_STR_VALUES[]"]
    );
}

#[test]
fn a_saved_menu_is_the_item_position_not_its_value() {
    let source = r#"on init
        declare ui_menu $M
        add_menu_item($M, "kk", 4)
        add_menu_item($M, "soft", -3)
        add_menu_item($M, "linear", 0)
        make_instr_persistent($M)
        read_persistent_var($M)
        end on"#;
    let limits = sampler_ksp::Limits {
        source_bytes: 4096,
        instructions: 256,
        variables: 16,
        array_cells: 16,
    };
    let env = sampler_ksp::Environment {
        persisted: [("$M".into(), sampler_ksp::model::Value::Int(2))].into(),
        ..Default::default()
    };
    let script = sampler_ksp::compile_with(source, 48000, limits, &[], &env).unwrap();
    let widget = &script.model().interface.widgets[0];
    assert_eq!(
        format!("{:?}", widget.value),
        "Int(0)",
        "{:?}",
        widget.value
    );
}

#[test]
fn custom_fonts_do_not_collide_with_factory_fonts() {
    let source = r#"on init
        declare ui_label $label(1,1)
        declare $first
        declare $second
        $first := get_font_id("first")
        $second := get_font_id("second")
        set_control_par(get_ui_id($label),$CONTROL_PAR_FONT_TYPE,get_font_id("first"))
        end on"#;
    let script = sampler_ksp::compile(
        source,
        48000,
        sampler_ksp::Limits {
            source_bytes: 4096,
            instructions: 256,
            variables: 16,
            array_cells: 16,
        },
        &[],
    )
    .unwrap();
    let ui = script.ui(&|_| None).unwrap();
    assert_eq!(script.model().interface.fonts, vec!["first", "second"]);
    assert!(matches!(ui.styles[0].font, sampler_ui_ir::Font::Bitmap(_)));
    assert_eq!(ui.assets[0].path, "Resources/pictures/first.png");
}

#[test]
fn missing_performance_description_does_not_create_visible_unsized_knob() {
    let script = sampler_ksp::compile(
        "on init\nload_performance_view(\"missing\")\n$ghost := 0\nend on",
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
    )
    .unwrap();
    let ghost = &script.model().interface.widgets[0];
    assert!(ghost.unresolved);
    assert!(ghost.control.is_none());
    assert!(ghost.properties.is_empty());
    assert!(script.ui(&|_| None).unwrap().widgets.is_empty());
    assert!(
        script
            .warnings()
            .iter()
            .any(|d| d.message.contains("unbound"))
    );
}

#[test]
fn typed_seed_meter_and_waveform_addresses_reach_ir() {
    let script = sampler_ksp::compile(
        r#"on init
        declare ui_table %t[4](1,1,100)
        set_control_par_arr(get_ui_id(%t),$CONTROL_PAR_VALUE,42,2)
        declare ui_text_edit @text
        @text := "seed"
        declare ui_level_meter $m
        attach_level_meter(get_ui_id($m),3,4,1,2)
        declare ui_waveform $w(1,1)
        attach_zone($w,27,3)
        set_ui_wf_property($w,$UI_WF_PROP_PLAY_CURSOR,12000,0)
        set_ui_wf_property($w,$UI_WF_PROP_TABLE_VAL,42,2)
        declare ui_xy ?pad[4]
        set_control_par(get_ui_id(?pad),$CONTROL_PAR_ACTIVE_INDEX,2)
    end on"#,
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
    )
    .unwrap();
    let ui = script.ui(&|_| None).unwrap();
    assert_eq!(
        ui.widgets[0].value,
        Some(sampler_ui_ir::Value::Integers(vec![0, 0, 42, 0]))
    );
    assert_eq!(
        ui.widgets[1].value,
        Some(sampler_ui_ir::Value::Text("seed".into()))
    );
    assert_eq!(
        ui.widgets[2].meter,
        Some(sampler_ui_ir::MeterAddress {
            group: 3,
            slot: 4,
            channel: 1,
            bus: Some(2)
        })
    );
    let waveform = ui.widgets[3].waveform.as_ref().unwrap();
    assert_eq!(
        (waveform.zone, waveform.flags, waveform.cursor_us),
        (27, 3, 12000)
    );
    assert_eq!(waveform.table, vec![0, 0, 42]);
    assert_eq!(ui.widgets[4].active_index, Some(2));
}
