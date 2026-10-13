use super::*;

fn face(kind: Kind) -> Interface {
    let mut widget = ir::Widget::new("$test", PageRef(0), ir::Rect::new(10, 10, 100, 80), kind);
    widget.text = "Ag\nSecond".into();
    widget.style = Some(ir::StyleRef(0));
    Interface {
        pages: vec![ir::Page {
            size: ir::Size {
                width: 160.0,
                height: 120.0,
            },
            background: ir::Background {
                color: Some(ir::Rgba::rgb(0xf0efe4)),
                ..Default::default()
            },
            ..Default::default()
        }],
        widgets: vec![widget],
        styles: vec![ir::TextStyle {
            font: ir::Font::Stock(0),
            size: Some(12.),
            color: ir::Rgba::rgb(0x111111),
            align: ir::Align::Left,
        }],
        ..Default::default()
    }
}
fn pixels(face: &Interface) -> Vec<u8> {
    pixels_state(face, &mut InputState::default())
}
fn pixels_state(face: &Interface, input: &mut InputState) -> Vec<u8> {
    let mut ui = super::super::theme::ui();
    let mut values = Values::default();
    let assets = Assets::default();
    for _ in 0..3 {
        let root = view_state(
            &mut ui,
            "",
            face,
            PageRef(0),
            &assets,
            Presentation::Bitmap,
            1.,
            &mut values,
            input,
        );
        ui.frame(
            root,
            Some(Size::new(160., 120.)),
            Input::default(),
            1. / 60.,
        )
        .unwrap();
    }
    super::super::tests::pixels(&ui, 160, 120)
}

#[test]
fn menu_keeps_authored_selected_caption_when_item_is_hidden() {
    let paint = |visible: i32, value: i32| {
        let source = format!(
            "on init\nmake_perfview\nset_ui_width_px(160)\nset_ui_height_px(120)\ndeclare ui_menu $m\nadd_menu_item($m,\"Selected\",20)\nadd_menu_item($m,\"Other\",10)\n$m := {value}\nmove_control_px($m,10,10)\nset_control_par(get_ui_id($m),$CONTROL_PAR_WIDTH,100)\nset_control_par(get_ui_id($m),$CONTROL_PAR_HEIGHT,30)\nset_menu_item_visibility(get_ui_id($m),0,{visible})\nend on"
        );
        let script =
            sampler_ksp::compile(&source, 48000, sampler_ksp::Limits::LIBRARY, &[]).unwrap();
        pixels(&resolved(&script.ui(&|_| None).unwrap()))
    };
    let selected = paint(1, 20);
    let hidden_selected = paint(0, 20);
    let other = paint(0, 10);
    println!(
        "MENU_CAPTION_PIXELS selected={} hidden_selected={} other={}",
        blake3::hash(&selected),
        blake3::hash(&hidden_selected),
        blake3::hash(&other)
    );
    assert!(
        selected == hidden_selected,
        "hiding an item retains its selected caption"
    );
    assert!(
        hidden_selected != other,
        "deselection changes the authored caption"
    );
}

#[test]
fn authored_square_slider_axis_and_knob_type_change_pixels() {
    let paint = |declaration: &str, behavior: i32| {
        let source = format!(
            "on init\nmake_perfview\nset_ui_width_px(160)\nset_ui_height_px(120)\n{declaration}\n$x := 50\nmove_control_px($x,10,10)\nset_control_par(get_ui_id($x),$CONTROL_PAR_WIDTH,64)\nset_control_par(get_ui_id($x),$CONTROL_PAR_HEIGHT,64)\nset_control_par(get_ui_id($x),$CONTROL_PAR_HIDE,6)\nset_control_par(get_ui_id($x),$CONTROL_PAR_MOUSE_BEHAVIOUR,{behavior})\nend on"
        );
        let script =
            sampler_ksp::compile(&source, 48000, sampler_ksp::Limits::LIBRARY, &[]).unwrap();
        pixels(&resolved(&script.ui(&|_| None).unwrap()))
    };
    let horizontal = paint("declare ui_slider $x(0,100)", 1000);
    let vertical = paint("declare ui_slider $x(0,100)", -1000);
    let knob = paint("declare ui_knob $x(0,100,1)", 1000);
    println!(
        "AUTHORED_CONTROL_PIXELS horizontal={} vertical={} knob={}",
        blake3::hash(&horizontal),
        blake3::hash(&vertical),
        blake3::hash(&knob)
    );
    assert!(
        horizontal != vertical,
        "scripted slider axis changes the track"
    );
    assert!(horizontal != knob, "square ui_slider remains a slider");
    assert!(vertical != knob, "ui_knob alone selects the dial");
}

#[test]
fn waveform_uses_current_envelope_duration_cursor_and_source_colour() {
    let mut face = face(Kind::Waveform);
    face.widgets[0].waveform = Some(ir::Waveform {
        zone: 9,
        flags: 0,
        cursor_us: 0,
        table: vec![],
        highlighted: None,
        midi_start_note: 60,
    });
    let mut input = InputState::default();
    let empty = pixels_state(&face, &mut input);
    input.peaks.insert(
        WidgetRef(0),
        Arc::from([(-0.1, 0.2), (-0.8, 0.7), (-0.3, 0.4)]),
    );
    input.wave_duration_us.insert(WidgetRef(0), 100_000);
    let envelope = pixels_state(&face, &mut input);
    assert_ne!(empty, envelope);
    face.widgets[0].waveform.as_mut().unwrap().cursor_us = 50_000;
    assert_ne!(envelope, pixels_state(&face, &mut input));
    let cursor = pixels_state(&face, &mut input);
    face.widgets[0].colors.wave = Some(ir::Rgba::rgb(0xff0000));
    face.widgets[0].colors.wave_cursor = Some(ir::Rgba::rgb(0x00ff00));
    assert_ne!(cursor, pixels_state(&face, &mut input));
}
#[test]
fn solid_background_controls_fallback_contrast() {
    let mut face = face(Kind::Knob {
        range: ir::Range {
            min: 0.,
            max: 100.,
            ..Default::default()
        },
        display: Default::default(),
    });
    assert!(light_under(&face, &Assets::default(), WidgetRef(0)));
    face.pages[0].background.color = Some(ir::Rgba::rgb(0x111111));
    assert!(!light_under(&face, &Assets::default(), WidgetRef(0)));
}
#[test]
fn label_alignment_and_offsets_change_pixels() {
    let mut face = face(Kind::Label);
    let left = pixels(&face);
    face.styles[0].align = ir::Align::Right;
    assert_ne!(left, pixels(&face));
    let right = pixels(&face);
    face.widgets[0].text_y = Some(1);
    assert_ne!(right, pixels(&face));
}
#[test]
fn table_cells_and_source_colours_change_pixels() {
    let mut face = face(Kind::Table {
        columns: 4,
        range: ir::Range {
            min: -100.,
            max: 100.,
            ..Default::default()
        },
        bipolar: true,
        cells: vec![0.; 4],
        steps_shown: Some(4),
    });
    let empty = pixels(&face);
    if let Kind::Table { cells, .. } = &mut face.widgets[0].kind {
        *cells = vec![-75., 25., 80., -25.];
    }
    assert_ne!(empty, pixels(&face));
    let values = pixels(&face);
    face.widgets[0].colors.bar = Some(ir::Rgba::rgb(0xff0000));
    assert_ne!(values, pixels(&face));
}

#[test]
fn ksp_table_steps_show_a_column_prefix_without_value_grid_lines() {
    let paint = |columns: usize, shown: Option<usize>, hidden: i32| {
        let shown = shown
            .map(|n| format!("set_table_steps_shown(%table,{n})\n"))
            .unwrap_or_default();
        let mut source = format!(
            "on init\nmake_perfview\nset_ui_width_px(160)\nset_ui_height_px(120)\ndeclare ui_table %table[{columns}] (1,1,100)\nmove_control_px(%table,10,10)\nset_control_par(get_ui_id(%table),$CONTROL_PAR_WIDTH,100)\nset_control_par(get_ui_id(%table),$CONTROL_PAR_HEIGHT,80)\n{shown}"
        );
        for n in 0..columns {
            let value = if n < 16 { (n * 5) as i32 } else { hidden };
            source.push_str(&format!("%table[{n}] := {value}\n"));
        }
        source.push_str("end on");
        let script =
            sampler_ksp::compile(&source, 48000, sampler_ksp::Limits::LIBRARY, &[]).unwrap();
        pixels(&resolved(&script.ui(&|_| None).unwrap()))
    };
    let prefix = paint(32, Some(16), 100);
    assert_eq!(prefix, paint(16, None, 0));
    assert_eq!(prefix, paint(32, Some(16), 0), "hidden cells do not paint");
    let full = paint(32, None, 100);
    assert_ne!(prefix, full);
    assert_eq!(full, paint(32, Some(32), 100));
}

#[test]
fn default_dimensions_and_wallpaper_origin_are_independent() {
    let mut ui = face(Kind::Label);
    ui.widgets[0].auto_size = true;
    ui.widgets[0].default_axes = [false, true];
    ui.widgets[0].rect.width = 137.0;
    ui.widgets[0].rect.height = 0.0;
    assert_eq!(resolved(&ui).widgets[0].rect.width, 137.0);
    assert_eq!(resolved(&ui).widgets[0].rect.height, 18.0);
    ui.widgets[0].default_axes = [true, false];
    ui.widgets[0].rect.height = 41.0;
    assert_eq!(resolved(&ui).widgets[0].rect.height, 41.0);
}

#[test]
fn slider_with_a_knob_picture_draws_as_the_vector_knob() {
    // Audio Imperia's "knob" is a ui_slider with a pyramid_knob strip (v1 knob_like).
    let paint = |declaration: &str| {
        let source = format!(
            "on init\nmake_perfview\nset_ui_width_px(160)\nset_ui_height_px(120)\n{declaration}\n$x := 50\nmove_control_px($x,10,10)\nset_control_par(get_ui_id($x),$CONTROL_PAR_WIDTH,64)\nset_control_par(get_ui_id($x),$CONTROL_PAR_HEIGHT,64)\nset_control_par(get_ui_id($x),$CONTROL_PAR_HIDE,6)\nset_control_par_str(get_ui_id($x),$CONTROL_PAR_PICTURE,\"pyramid_knob_small\")\nend on"
        );
        let script =
            sampler_ksp::compile(&source, 48000, sampler_ksp::Limits::LIBRARY, &[]).unwrap();
        pixels(&resolved(&script.ui(&|_| None).unwrap()))
    };
    assert_eq!(
        blake3::hash(&paint("declare ui_knob $x(0,100,1)")),
        blake3::hash(&paint("declare ui_slider $x(0,100)")),
    );
}

#[test]
fn skinned_button_caption_keeps_its_text_and_state_ink() {
    let mut face = face(Kind::Button {momentary:false});
    face.source = ir::Source::Ksp {slot:0};
    face.widgets[0].text = "Caption".into();
    face.assets.push(ir::Asset {path:"button.png".into(),kind:ir::AssetKind::Image(Default::default())});
    face.widgets[0].images.push(ir::ImageUse {role:ir::Role::Strip,asset:ir::AssetRef(0),frame:None});
    let mut assets = Assets::default();
    assets.sync(&face,Presentation::Bitmap,|_| Some(Arc::new(Picture::new(vec![
        Arc::new(moose::mui::mui::scene::Image::rgba(2,2,vec![20,20,20,255].repeat(4)).unwrap())
    ]))));
    let mut ui = super::super::theme::ui();
    let mut values = Values::default();
    let mut input = InputState::default();
    let paint = |ui:&mut Ui,face:&Interface,input:&mut InputState,values:&mut Values| {
        for _ in 0..3 {
            let root = view_state(ui,"",face,PageRef(0),&assets,Presentation::Bitmap,1.,values,input);
            ui.frame(root,Some(Size::new(160.,120.)),Input::default(),1./60.).unwrap();
        }
        super::super::tests::pixels(ui,160,120)
    };
    let caption = paint(&mut ui,&face,&mut input,&mut values);
    assert!(ui.scene().unwrap().paint.iter().any(|p|p.text.as_ref().is_some_and(|t|!t.glyphs.is_empty())),
        "a bitmap backdrop cannot erase the authored caption");
    face.widgets[0].text.clear();
    assert_ne!(caption,paint(&mut ui,&face,&mut input,&mut values));
    face.widgets[0].text = "Caption".into();
    face.styles[0].color = ir::Rgba::rgb(0xff0011);
    assert_ne!(caption,paint(&mut ui,&face,&mut input,&mut values),"authored font ink must reach the skin");
}

#[test]
fn menu_responsive_font_uses_popup_state_instead_of_selected_numeric_value() {
    let mut face = face(Kind::Menu {items:vec![
        ir::MenuItem {text:"Choice".into(),value:0,visible:true},
        ir::MenuItem {text:"Choice".into(),value:42,visible:true},
    ]});
    face.widgets[0].text.clear();
    let mut on = face.styles[0].clone();
    on.color = ir::Rgba::rgb(0xff0011);
    face.styles.push(on);
    face.widgets[0].state_styles[1] = Some(ir::StyleRef(1));
    let closed = pixels(&face);
    face.widgets[0].initial_value = 42.;
    assert_eq!(closed,pixels(&face),"a different semantic menu value cannot select the ON font");
    let mut open = InputState {menu:Some(WidgetRef(0)),..Default::default()};
    let opened = pixels_state(&face,&mut open);
    assert_ne!(closed,opened,"the open popup uses the authored ON state");
}

#[test]
fn button_press_caption_shift_obeys_disable_text_shifting() {
    let held_origin = |disable:i32| {
        let source = format!("on init\nmake_perfview\nset_ui_width_px(160)\nset_ui_height_px(120)\ndeclare ui_button $b\nset_text($b,\"Caption\")\nmove_control_px($b,10,10)\nset_control_par(get_ui_id($b),$CONTROL_PAR_WIDTH,100)\nset_control_par(get_ui_id($b),$CONTROL_PAR_HEIGHT,80)\nset_control_par(get_ui_id($b),$CONTROL_PAR_DISABLE_TEXT_SHIFTING,{disable})\nend on");
        let script = sampler_ksp::compile(&source,48000,sampler_ksp::Limits::LIBRARY,&[]).unwrap();
        let face = resolved(&script.ui(&|_|None).unwrap());
        assert!(face.unsupported.is_empty(),"the press-shift property must project into UI IR");
        let mut ui = super::super::theme::ui();
        let mut values = Values::default(); let mut state = InputState::default();
        let mut origins = Vec::new();
        for down in [false,true] {
            for _ in 0..3 {
                let root = view_state(&mut ui,"",&face,PageRef(0),&Assets::default(),Presentation::Bitmap,1.,&mut values,&mut state);
                ui.frame(root,Some(Size::new(160.,120.)),Input {pointer:PointerInput {pos:Some(Point::new(60.,50.)),
                    buttons:if down {Buttons::PRIMARY} else {Buttons::default()},..Default::default()},..Default::default()},1./60.).unwrap();
            }
            assert_eq!(ui.get("ir-0").held,down);
            origins.push(ui.scene().unwrap().paint.iter().find_map(|p|p.text.as_ref().map(|t|t.origin)).unwrap());
        }
        origins
    };
    let fixed = held_origin(1);
    assert_eq!(fixed[0],fixed[1]);
    let shifted = held_origin(0);
    assert_eq!(shifted[1],Point::new(shifted[0].x+1.,shifted[0].y+1.));
}
