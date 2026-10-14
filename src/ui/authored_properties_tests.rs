//! Source-owned style fixtures; paint and gestures stay on the production path.
use super::{ir_view, tests::pixels, theme};
use moose::mui::mui::prelude::*;
use sampler_ui_ir as ir;

const XML: &str = include_str!("../../tools/fixtures/authored-ui-properties.uvip");

fn face() -> ir::Interface {
    sampler_uvi::script::ScriptHost::new(XML, (), Default::default())
        .unwrap()
        .interface()
}
fn tick(
    ui: &mut Ui,
    face: &ir::Interface,
    values: &mut ir_view::Values,
    state: &mut ir_view::InputState,
    input: Input,
) {
    let root = ir_view::view_state(
        ui,
        "authored",
        face,
        ir::PageRef(0),
        &ir_view::Assets::default(),
        ir::Presentation::Bitmap,
        1.,
        values,
        state,
    );
    ui.frame(root, Some(Size::new(320., 160.)), input, 1. / 60.)
        .unwrap();
}
fn point(x: f64, y: f64, held: bool) -> Input {
    Input {
        pointer: PointerInput {
            pos: Some(Point::new(x, y)),
            buttons: if held {
                Buttons::PRIMARY
            } else {
                Buttons::default()
            },
            ..Default::default()
        },
        ..Default::default()
    }
}
fn coloured(pixels: &[u8], rect: [usize; 4], channel: usize) -> usize {
    let [x, y, w, h] = rect;
    (y..y + h)
        .flat_map(|y| (x..x + w).map(move |x| 4 * (y * 320 + x)))
        .filter(|&i| {
            pixels[i + channel] > 180
                && (0..3).filter(|&c| c != channel).all(|c| pixels[i + c] < 90)
        })
        .count()
}

#[test]
fn authored_button_states_menu_arrow_and_outline_reach_cpu_pixels() {
    let mut face = face();
    let mut ui = theme::ui();
    let mut values = ir_view::Values::default();
    let mut state = ir_view::InputState::default();
    for _ in 0..3 {
        tick(&mut ui, &face, &mut values, &mut state, Input::default());
    }
    let off = pixels(&ui, 320, 160);
    assert!(
        coloured(&off, [12, 12, 120, 30], 1) > 20,
        "off caption must use authored green"
    );
    assert!(
        coloured(&off, [275, 12, 21, 30], 1) > 3,
        "menu arrow must use authored green"
    );
    assert!(
        coloured(&off, [156, 12, 140, 30], 0) > 100,
        "menu outline must use authored red"
    );
    for _ in 0..3 {
        tick(
            &mut ui,
            &face,
            &mut values,
            &mut state,
            point(60., 26., true),
        );
    }
    let held = pixels(&ui, 320, 160);
    assert!(
        coloured(&held, [12, 12, 120, 30], 0) > 20,
        "held momentary caption must use authored on ink"
    );
    assert_eq!(coloured(&held, [12, 12, 120, 30], 1), 0);
    face.widgets[1].menu_arrow = false;
    for _ in 0..3 {
        tick(
            &mut ui,
            &face,
            &mut values,
            &mut state,
            point(60., 26., false),
        );
    }
    let hidden = pixels(&ui, 320, 160);
    assert_eq!(
        coloured(&hidden, [275, 12, 21, 30], 1),
        0,
        "displayArrow=false suppresses authored caret"
    );
}

#[test]
fn authored_value_popup_is_pinned_passive_and_suppressed_by_false() {
    let mut face = face();
    let mut ui = theme::ui();
    let mut values = ir_view::Values::default();
    let mut state = ir_view::InputState::default();
    for _ in 0..3 {
        tick(&mut ui, &face, &mut values, &mut state, Input::default());
    }
    let id = "/authored-ir-2-value-popup";
    assert!(ui.scene().unwrap().surface(id).is_none());
    for _ in 0..3 {
        tick(
            &mut ui,
            &face,
            &mut values,
            &mut state,
            point(60., 84., true),
        );
    }
    let scene = ui.scene().unwrap();
    let popup = scene
        .surface(id)
        .expect("showPopupDisplay=true while adjusting");
    assert!(
        popup.key.as_str().starts_with('/'),
        "value readout must not capture control input"
    );
    assert!(
        popup.frame.x >= 0.
            && popup.frame.y >= 0.
            && popup.frame.x + popup.frame.size.width <= 320.
            && popup.frame.y + popup.frame.size.height <= 160.
    );
    assert!(
        scene
            .surfaces()
            .any(|surface| surface.text_value.as_deref() == Some("Authored value"))
    );
    face.widgets[2].popup_display = Some(false);
    for _ in 0..2 {
        tick(
            &mut ui,
            &face,
            &mut values,
            &mut state,
            point(60., 84., true),
        );
    }
    assert!(ui.scene().unwrap().surface(id).is_none());
}
