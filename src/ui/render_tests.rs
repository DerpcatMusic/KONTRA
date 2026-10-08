use super::*;

fn face(kind: Kind) -> Interface {
    let mut widget = ir::Widget::new("$test", PageRef(0), ir::Rect::new(10, 10, 100, 80), kind);
    widget.text = "Ag\nSecond".into();
    widget.style = Some(ir::StyleRef(0));
    Interface {
        pages: vec![ir::Page { size: ir::Size { width: 160, height: 120 }, background: ir::Background { color: Some(ir::Rgba::rgb(0xf0efe4)), ..Default::default() }, ..Default::default() }],
        widgets: vec![widget],
        styles: vec![ir::TextStyle { font: ir::Font::Stock(0), size: Some(12.), color: ir::Rgba::rgb(0x111111), align: ir::Align::Left }],
        ..Default::default()
    }
}
fn pixels(face: &Interface) -> Vec<u8> {
    let mut ui = super::super::theme::ui();
    let mut values = Values::default();
    let assets = Assets::default();
    for _ in 0..3 {
        let root = view(&mut ui, "", face, PageRef(0), &assets, Presentation::Bitmap, 1., &mut values);
        ui.frame(root, Some(Size::new(160., 120.)), Input::default(), 1./60.).unwrap();
    }
    super::super::tests::pixels(&ui, 160, 120)
}
#[test]
fn solid_background_controls_fallback_contrast() {
    let mut face = face(Kind::Knob { range: ir::Range { min: 0., max: 100., ..Default::default() }, display: Default::default() });
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
    let mut face = face(Kind::Table { columns: 4, range: ir::Range { min: -100., max: 100., ..Default::default() }, bipolar: true, cells: vec![0.;4], steps_shown: Some(4) });
    let empty = pixels(&face);
    if let Kind::Table { cells, .. } = &mut face.widgets[0].kind { *cells = vec![-75., 25., 80., -25.]; }
    assert_ne!(empty, pixels(&face));
    let values = pixels(&face);
    face.widgets[0].colors.bar = Some(ir::Rgba::rgb(0xff0000));
    assert_ne!(values, pixels(&face));
}
