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

const TABLE_XML: &str =
    include_str!("../../crates/sampler-uvi/tests/fixtures/table_inner_edge.uvip");

#[test]
fn authored_table_edges_reach_cpu_pixels_and_live_same_widget_updates() {
    let host = sampler_uvi::script::ScriptHost::new(TABLE_XML, (), Default::default()).unwrap();
    for component_bound in [true, false] {
        let mut face = host.interface();
        if !component_bound {
            face.widgets[0].components.clear();
        }
        let base = face.clone();
        let mut ui = theme::ui();
        let mut values = ir_view::Values::default();
        let mut state = ir_view::InputState::default();
        for (edge, channel) in [
            (
                ir::TableInnerEdge {
                    visible: true,
                    color: Some(ir::Rgba::rgb(0xff0000)),
                },
                Some(0),
            ),
            (
                ir::TableInnerEdge {
                    visible: false,
                    color: Some(ir::Rgba::rgb(0x0000ff)),
                },
                None,
            ),
            (
                ir::TableInnerEdge {
                    visible: true,
                    color: Some(ir::Rgba::rgb(0x0000ff)),
                },
                Some(2),
            ),
        ] {
            let mut next = base.clone();
            next.widgets[0].table_inner_edge = Some(edge);
            let patch = ir::InterfacePatch::between(&face, &next);
            if face != next {
                assert_eq!(patch.widgets.len(), 1, "edge-only publication");
            }
            patch.apply(&face.clone(), &Default::default(), &mut face);
            assert_eq!(face, next);
            for _ in 0..3 {
                tick(&mut ui, &face, &mut values, &mut state, Input::default());
            }
            let paint = pixels(&ui, 320, 160);
            for c in [0, 2] {
                let count = coloured(&paint, [12, 12, 120, 80], c);
                if Some(c) == channel {
                    assert!(
                        count >= 180,
                        "three authored separators: {count}, components={component_bound}"
                    );
                } else {
                    assert_eq!(count, 0, "disabled or previous separator ink survived");
                }
            }
            if channel.is_none() {
                for x in [41, 71, 101] {
                    let pixel = &paint[4 * (20 * 320 + x)..][..4];
                    assert!(
                        pixel[..3].iter().all(|channel| *channel > 220),
                        "no gap at boundary {x}: {pixel:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn table_edge_presentation_preserves_component_edits_and_one_based_lua_callbacks() {
    for visible in [false, true] {
        let mut host =
            sampler_uvi::script::ScriptHost::new(TABLE_XML, (), Default::default()).unwrap();
        let mut face = host.interface();
        face.widgets[0].table_inner_edge.as_mut().unwrap().visible = visible;
        let components = face.widgets[0].components.clone();
        let mut ui = theme::ui();
        let mut values = ir_view::Values::default();
        let mut state = ir_view::InputState::default();
        for _ in 0..3 {
            tick(&mut ui, &face, &mut values, &mut state, Input::default());
        }
        let third = ui
            .scene()
            .unwrap()
            .surface("authored-ir-0-cell-2")
            .unwrap()
            .frame;
        assert_eq!((third.x, third.size.width), (72., 30.));
        assert!(ui.request_action(SemanticAction::set_value("authored-ir-0-cell-2", 0.375)));
        for _ in 0..3 {
            tick(&mut ui, &face, &mut values, &mut state, Input::default());
        }
        assert_eq!(values[&components[2]], 0.375);
        for (i, id) in components.iter().enumerate() {
            assert_eq!(values[id], if i == 2 { 0.375 } else { 1. });
        }
        host.set_control(components[2], values[&components[2]])
            .unwrap();
        assert_eq!(host.global_text("calls"), "1");
        assert_eq!(host.global_text("last_index"), "3");
        assert_eq!(host.global_text("last_value"), "0.375");
        assert!(
            state.edits.is_empty(),
            "component controls use their own binding, not array indices"
        );
    }
}

#[test]
fn table_array_pointer_edits_keep_zero_based_indices_with_either_edge_policy() {
    for visible in [false, true] {
        let mut host =
            sampler_uvi::script::ScriptHost::new(TABLE_XML, (), Default::default()).unwrap();
        let mut face = host.interface();
        let components = std::mem::take(&mut face.widgets[0].components);
        face.widgets[0].table_inner_edge.as_mut().unwrap().visible = visible;
        let mut ui = theme::ui();
        let mut values = ir_view::Values::default();
        let mut state = ir_view::InputState::default();
        for _ in 0..3 {
            tick(&mut ui, &face, &mut values, &mut state, Input::default());
        }
        for held in [false, true, false] {
            tick(
                &mut ui,
                &face,
                &mut values,
                &mut state,
                point(87., 52., held),
            );
        }
        assert_eq!(state.edits.len(), 1);
        let edit = &state.edits[0];
        assert_eq!(edit.widget, ir::WidgetRef(0));
        assert_eq!((edit.index, edit.cursor), (2, 2));
        assert_eq!(edit.value, ir::Value::Real(0.5));
        host.set_control(components[edit.index as usize], 0.5)
            .unwrap();
        assert_eq!(host.global_text("calls"), "1");
        assert_eq!(host.global_text("last_index"), "3");
        assert_eq!(host.global_text("last_value"), "0.5");
    }
}

#[test]
fn table_argb_separator_composites_over_the_authored_bar() {
    let xml = TABLE_XML.replace("innerEdgeColour='red'", "innerEdgeColour='#800000ff'");
    let face = sampler_uvi::script::ScriptHost::new(&xml, (), Default::default())
        .unwrap()
        .interface();
    let mut ui = theme::ui();
    let mut values = ir_view::Values::default();
    let mut state = ir_view::InputState::default();
    for _ in 0..3 {
        tick(&mut ui, &face, &mut values, &mut state, Input::default());
    }
    let paint = pixels(&ui, 320, 160);
    let pixel = &paint[4 * (20 * 320 + 41)..][..4];
    assert!(
        (120..=135).contains(&pixel[0]) && (120..=135).contains(&pixel[1]),
        "ARGB alpha must blend: {pixel:?}"
    );
    assert!(
        pixel[2] > 245 && pixel[3] == 255,
        "ARGB blue over opaque white: {pixel:?}"
    );
}

#[test]
fn table_default_fallback_and_non_uvi_tables_keep_the_existing_pixels() {
    let host = sampler_uvi::script::ScriptHost::new(TABLE_XML, (), Default::default()).unwrap();
    for component_bound in [false, true] {
        let mut face = host.interface();
        if !component_bound {
            face.widgets[0].components.clear();
        }
        face.widgets[0].table_inner_edge = None;
        let legacy = face.clone();
        let paint = |face: &ir::Interface| {
            let mut ui = theme::ui();
            let mut values = ir_view::Values::default();
            let mut state = ir_view::InputState::default();
            for _ in 0..3 {
                tick(&mut ui, face, &mut values, &mut state, Input::default());
            }
            pixels(&ui, 320, 160)
        };
        let expected = paint(&legacy);
        face.widgets[0].table_inner_edge = Some(ir::TableInnerEdge {
            visible: true,
            color: None,
        });
        assert_eq!(
            paint(&face),
            expected,
            "unknown native default ink must retain fallback"
        );
        face.source = ir::Source::Ksp { slot: 0 };
        face.widgets[0].table_inner_edge = None;
        // The source policy changes editing, not stock table painting.
        assert_eq!(paint(&face), expected);
        assert_eq!(
            ir::Widget::new(
                "Stock",
                ir::PageRef(0),
                ir::Rect::new(0, 0, 10, 10),
                legacy.widgets[0].kind.clone()
            )
            .table_inner_edge,
            None
        );
    }
}
