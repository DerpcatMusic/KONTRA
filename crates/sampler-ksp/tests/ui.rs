use sampler_ui_ir::{Binding, Kind, Rect, Role, Source};

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
    assert_eq!(ui.pages[0].size.height, 300);
    assert_eq!(
        ui.assets[ui.pages[0].background.image.unwrap().0].path,
        "Resources/pictures/wall.png"
    );
    let [panel, knob, label, table, meter, button] = &ui.widgets[..] else {
        panic!("{:?}", ui.widgets);
    };
    assert_eq!((panel.kind.clone(), panel.rect.x), (Kind::Panel, 10));
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
            axis: sampler_ui_ir::Orientation::Horizontal,
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
    assert_eq!(cells[2], 7);
    assert_eq!(
        button.placement,
        sampler_ui_ir::Placement::Grid { column: 2, row: 3 }
    );
    assert!(ui.unsupported.is_empty(), "{:?}", ui.unsupported);
}
