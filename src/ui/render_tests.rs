use super::*;

fn face(kind: Kind) -> Interface {
    let mut widget = ir::Widget::new("$test", PageRef(0), ir::Rect::new(10, 10, 100, 80), kind);
    widget.text = "Ag\nSecond".into();
    widget.style = Some(ir::StyleRef(0));
    Interface {
        pages: vec![ir::Page {
            size: ir::Size {
                width: 160,
                height: 120,
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
fn default_dimensions_and_wallpaper_origin_are_independent() {
    let mut ui = face(Kind::Label);
    ui.widgets[0].auto_size = true;
    ui.widgets[0].default_axes = [false, true];
    ui.widgets[0].rect.width = 137;
    ui.widgets[0].rect.height = 0;
    assert_eq!(resolved(&ui).widgets[0].rect.width, 137);
    assert_eq!(resolved(&ui).widgets[0].rect.height, 18);
    ui.widgets[0].default_axes = [true, false];
    ui.widgets[0].rect.height = 41;
    assert_eq!(resolved(&ui).widgets[0].rect.height, 41);
}
