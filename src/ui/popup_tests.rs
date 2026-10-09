//! Wheel ownership is shared by chrome menus and authored dropdowns.
use super::*;

fn tree() -> El {
    stack![
        col![block(300., 900.)]
            .size(300., 200.)
            .scroll()
            .id("background"),
        stack![
            col((0..20)
                .map(|n| caption(format!("Choice {n}")).h(24.).shrink(0))
                .collect::<Vec<_>>())
            .gap(0)
            .size(140., 80.)
            .scroll()
            .id("popup-list")
        ]
        .size(140., 80.)
        .captures_wheel()
        .id("popup-boundary")
        .at(80., 30.)
    ]
    .size(300., 200.)
}

#[test]
fn popup_wheel_is_visible_only_inside_its_capture_boundary() {
    let mut ui = theme::ui();
    for _ in 0..3 {
        ui.frame(tree(), None, Input::default(), 1. / 60.).unwrap();
    }
    ui.frame(
        tree(),
        None,
        Input {
            pointer: PointerInput {
                pos: Some(Point::new(110., 50.)),
                ..Default::default()
            },
            wheel: Vec2::new(0., 60.),
            ..Default::default()
        },
        1. / 60.,
    )
    .unwrap();
    assert_eq!(ui.get("popup-list").wheel, Vec2::new(0., 60.));
    assert_eq!(
        ui.wheel("background"),
        None,
        "a custom scroller behind a popup must not see its wheel"
    );
    assert_eq!(ui.scroll("background"), [0., 0.]);
    assert!(ui.scroll("popup-list")[1] > 0.);
}

#[test]
fn an_exhausted_popup_never_hands_wheel_to_the_background() {
    let mut ui = theme::ui();
    for _ in 0..3 {
        ui.frame(tree(), None, Input::default(), 1. / 60.).unwrap();
    }
    ui.set_scroll("popup-list", [0., 10000.]);
    for _ in 0..20 {
        ui.frame(tree(), None, Input::default(), 1. / 60.).unwrap();
    }
    let end = ui.scroll("popup-list");
    ui.frame(
        tree(),
        None,
        Input {
            pointer: PointerInput {
                pos: Some(Point::new(110., 50.)),
                ..Default::default()
            },
            wheel: Vec2::new(0., 60.),
            ..Default::default()
        },
        1. / 60.,
    )
    .unwrap();
    assert_eq!(ui.scroll("popup-list"), end);
    assert_eq!(ui.get("background").wheel, Vec2::ZERO);
    assert_eq!(ui.scroll("background"), [0., 0.]);
}

#[test]
fn registered_popup_scope_survives_retention_and_ends_when_the_popup_closes() {
    let tree = |open| {
        let mut layers = vec![
            col![block(300., 900.)]
                .size(300., 200.)
                .scroll()
                .id("background"),
        ];
        if open {
            layers.push(
                col![block(140., 500.)]
                    .size(140., 80.)
                    .scroll()
                    .id("popup")
                    .at(80., 30.),
            );
        }
        stack(layers).size(300., 200.)
    };
    let wheel = || Input {
        pointer: PointerInput {
            pos: Some(Point::new(110., 50.)),
            ..Default::default()
        },
        wheel: Vec2::new(0., 60.),
        ..Default::default()
    };
    let mut ui = theme::ui();
    ui.capture_popup_wheel("popup");
    for _ in 0..3 {
        ui.frame(tree(true), None, Input::default(), 1. / 60.)
            .unwrap();
    }
    ui.frame(tree(true), None, wheel(), 1. / 60.).unwrap();
    assert_eq!(
        ui.wheel("background"),
        None,
        "retention keeps the popup scope"
    );
    assert!(ui.scroll("popup")[1] > 0., "the popup itself can scroll");
    ui.frame(tree(false), None, Input::default(), 1. / 60.)
        .unwrap();
    ui.frame(tree(false), None, wheel(), 1. / 60.).unwrap();
    assert_eq!(ui.wheel("background"), Some(Vec2::new(0., 60.)));
    assert!(
        ui.scroll("background")[1] > 0.,
        "closing restores normal scrolling"
    );
}

fn snapshot_rack() -> Arc<SamplerParams> {
    let p = Arc::new(SamplerParams::new());
    p.selection.write().unwrap().parts = (0..16)
        .map(|n| Part {
            path: format!("/virtual/Instrument {n}.nki"),
            name: format!("Instrument {n}"),
            collapsed: n > 0,
            ..Default::default()
        })
        .collect();
    let mut view = p.shared.view.lock().unwrap();
    view.scanned = p.shared.libraries.wanted();
    for (n, part) in view.parts.iter_mut().take(16).enumerate() {
        part.active = format!("Instrument {n}");
        part.instrument = Some(Arc::new(sampler_ir::Instrument::default()));
    }
    let mut shelf = crate::library::Shelf::default();
    shelf.snapshots.insert(
        PathBuf::from("/virtual/Instrument 0.nki"),
        crate::library::Snapshots {
            instrument: "Instrument 0".into(),
            paths: (0..60)
                .map(|n| PathBuf::from(format!("/virtual/Presets/Preset {n:02}.nksn")))
                .collect(),
        },
    );
    view.shelf = Arc::new(shelf);
    drop(view);
    p
}

#[test]
fn snapshot_picker_and_output_menu_keep_wheel_out_of_the_rack() {
    for (width, height) in [(900., 600.), (1180., 900.)] {
        let p = snapshot_rack();
        let mut h = tests::Harness::new(&p, width, height);
        h.idle(20);
        h.press("snapshot-0");
        let rack_y =
            h.ui.scene()
                .unwrap()
                .surface("rack-content")
                .unwrap()
                .frame
                .y;
        let at = tests::center(&h.ui, "menu-item-0");
        let wheel = Input {
            pointer: PointerInput {
                pos: Some(at),
                ..Default::default()
            },
            wheel: Vec2::new(0., 80.),
            ..Default::default()
        };
        h.tick(wheel.clone());
        assert_eq!(h.ui.get("rack-view").wheel, Vec2::ZERO);
        h.idle(20);
        assert!(
            h.ui.scroll("context-menu")[1] > 0.,
            "the snapshot list must scroll"
        );
        assert_eq!(
            h.ui.scene()
                .unwrap()
                .surface("rack-content")
                .unwrap()
                .frame
                .y,
            rack_y
        );
        h.ui.set_scroll("context-menu", [0., 10000.]);
        h.idle(20);
        h.tick(wheel);
        h.idle(20);
        assert_eq!(
            h.ui.scene()
                .unwrap()
                .surface("rack-content")
                .unwrap()
                .frame
                .y,
            rack_y,
            "an exhausted picker must not scroll the rack"
        );
        h.tick(Input {
            keys: vec![KeyPress {
                key: Key::Escape,
                mods: Mods::default(),
            }],
            ..Default::default()
        });
        h.idle(3);
        h.press("output-0");
        let at = tests::center(&h.ui, "menu-item-0");
        h.tick(Input {
            pointer: PointerInput {
                pos: Some(at),
                ..Default::default()
            },
            wheel: Vec2::new(0., 80.),
            ..Default::default()
        });
        assert_eq!(h.ui.get("rack-view").wheel, Vec2::ZERO);
        h.idle(20);
        assert_eq!(
            h.ui.scene()
                .unwrap()
                .surface("rack-content")
                .unwrap()
                .frame
                .y,
            rack_y
        );
    }
}

#[test]
fn authored_dropdown_scrolls_inside_its_popup_boundary() {
    let mut script = String::from("on init\n declare ui_menu $m\n");
    for n in 0..40 {
        script.push_str(&format!(" add_menu_item($m,\"Choice {n}\",{n})\n"));
    }
    script.push_str("end on");
    let script = sampler_ksp::compile(&script, 48000, sampler_ksp::Limits::LIBRARY, &[]).unwrap();
    let face = ir_view::resolved(&script.ui(&|_| None).unwrap());
    let mut values = ir_view::Values::default();
    let mut state = ir_view::InputState::default();
    let assets = ir_view::Assets::default();
    let mut ui = theme::ui();
    let tick = |ui: &mut Ui,
                values: &mut ir_view::Values,
                state: &mut ir_view::InputState,
                input: Input| {
        let widget = ir_view::widget_state(
            ui,
            "dropdown",
            &face,
            sampler_ui_ir::WidgetRef(0),
            &assets,
            sampler_ui_ir::Presentation::Vector,
            1.,
            values,
            state,
            130.,
            30.,
        )
        .at(80., 30.);
        let mut layers = vec![
            col![block(300., 900.)]
                .size(300., 200.)
                .scroll()
                .id("background"),
            widget,
        ];
        if let Some(popup) =
            ir_view::menu_popup(ui, "dropdown", &face, 1., values, state, 300., 200.)
        {
            layers.push(popup);
        }
        ui.frame(
            stack(layers).size(300., 200.).id("dropdown-ir-view"),
            None,
            input,
            1. / 60.,
        )
        .unwrap();
    };
    for _ in 0..3 {
        tick(&mut ui, &mut values, &mut state, Input::default());
    }
    ui.focus("dropdown-ir-0");
    tick(
        &mut ui,
        &mut values,
        &mut state,
        Input {
            keys: vec![KeyPress {
                key: Key::Enter,
                mods: Mods::default(),
            }],
            ..Default::default()
        },
    );
    for _ in 0..3 {
        tick(&mut ui, &mut values, &mut state, Input::default());
    }
    let inner = ui
        .scene()
        .unwrap()
        .surface("dropdown-ir-0-popup")
        .unwrap()
        .frame;
    assert_eq!(
        inner.size,
        Size::new(130., 200.),
        "popup geometry stays authored"
    );
    let at = Point::new(inner.x + 10., inner.y + 10.);
    tick(
        &mut ui,
        &mut values,
        &mut state,
        Input {
            pointer: PointerInput {
                pos: Some(at),
                ..Default::default()
            },
            wheel: Vec2::new(0., 60.),
            ..Default::default()
        },
    );
    assert_eq!(ui.wheel("background"), None);
    assert!(ui.scroll("dropdown-ir-0-popup")[1] > 0.);
    assert_eq!(ui.scroll("background"), [0., 0.]);
}
