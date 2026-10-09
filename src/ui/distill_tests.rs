//! Chrome copy and layout receipts. No instrument face is rendered here.
use super::*;
use tests::{Harness, pixels};

#[test]
fn empty_browser_keeps_help_on_its_action() {
    for (width, height) in [(900., 600.), (1180., 900.)] {
        let p = Arc::new(SamplerParams::new());
        let h = Harness::new(&p, width, height);
        let scene = h.ui.scene().unwrap();
        assert!(scene.surfaces().any(|s| s.text_value.as_deref() == Some("No libraries yet")));
        assert!(scene.surface("empty-add-many").is_none() && scene.surface("empty-add-one").is_none());
        assert!(scene.surface("libraries-add").unwrap().tip.as_deref().is_some_and(|tip| tip.contains("Add libraries")));
    }
}

#[test]
fn empty_rack_has_one_prompt_and_keeps_multi_help_on_hover() {
    let p = Arc::new(SamplerParams::new());
    let h = Harness::new(&p, 900., 600.);
    let scene = h.ui.scene().unwrap();
    assert!(
        !scene.surfaces().any(|s| s
            .text_value
            .as_deref()
            .is_some_and(|text| text.starts_with("Choose a library on the left"))),
        "one visible loading prompt is enough"
    );
    let prompt = scene
        .surfaces()
        .find(|s| s.text_value.as_deref() == Some("Pick an instrument"))
        .unwrap();
    assert!(
        scene
            .surface("rack-drop")
            .unwrap()
            .tip
            .as_deref()
            .is_some_and(|tip| tip.contains("Multis load the whole rack"))
    );
    let rack = scene.surface("rack-view").unwrap().frame;
    assert!(
        prompt.frame.x >= rack.x
            && prompt.frame.x + prompt.frame.size.width <= rack.x + rack.size.width
    );
}

#[test]
fn empty_settings_uses_the_add_actions_as_instructions() {
    let p = Arc::new(SamplerParams::new());
    let mut h = Harness::new(&p, 900., 600.);
    h.press("app-menu");
    h.press("menu-item-5");
    let scene = h.ui.scene().unwrap();
    assert!(
        !scene.surfaces().any(|s| s
            .text_value
            .as_deref()
            .is_some_and(|text| text.starts_with("No library folders yet."))),
        "the folder actions already explain the empty state"
    );
    assert!(
        scene
            .surface("root-pick-many")
            .unwrap()
            .tip
            .as_deref()
            .is_some_and(|tip| tip.contains("each library"))
    );
    assert!(
        scene.surface("root").is_none(),
        "the unused path input stays behind its action"
    );
    h.press("root-typed-toggle");
    assert!(
        h.ui.scene().unwrap().surface("root").is_some(),
        "a typed path remains available when requested"
    );
}

#[test]
#[cfg(feature = "shots")]
fn distill_startup_shots() {
    let Some(out) = std::env::var_os("KONTRA_DISTILL_SHOTS").map(PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&out).unwrap();
    for (width, height) in [(900, 600), (1180, 900)] {
        let p = Arc::new(SamplerParams::new());
        let mut h = Harness::new(&p, width as f64, height as f64);
        let save = |name: &str, h: &mut Harness| {
            h.idle(20);
            moose::core::screenshot::save_png(
                &out.join(format!("{name}-{width}.png")),
                &pixels(&h.ui, width, height),
                width.into(),
                height.into(),
            );
        };
        save("browser-rack-empty", &mut h);
        h.press("app-menu");
        h.press("menu-item-5");
        save("settings-empty", &mut h);
    }
}

#[test]
fn scale_settings_keep_window_help_on_hover() {
    for (width, height) in [(900., 600.), (1180., 900.)] {
        let p = Arc::new(SamplerParams::new());
        let mut h = Harness::new(&p, width, height);
        h.press("app-menu");
        h.press("menu-item-5");
        h.idle(20);
        let scene = h.ui.scene().unwrap();
        assert!(
            !scene
                .surfaces()
                .any(|s| s.text_value.as_deref() == Some("Window size is remembered"))
        );
        let label = scene
            .surfaces()
            .find(|s| s.text_value.as_deref() == Some("Interface scale"))
            .unwrap();
        assert_eq!(label.tip.as_deref(), Some("Window size is remembered"));
        let frame = label.frame;
        let viewport = scene.surface("settings-body").unwrap().frame;
        let reveal =
            (frame.y + frame.size.height + SPACE - viewport.y - viewport.size.height).max(0.);
        h.ui.set_scroll("settings-body", [0., reveal]);
        h.idle(30);
        let scene = h.ui.scene().unwrap();
        let label = scene
            .surfaces()
            .find(|s| s.text_value.as_deref() == Some("Interface scale"))
            .unwrap()
            .frame;
        let reset = scene.surface("ui-scale-reset").unwrap().frame;
        assert!(reset.x + reset.size.width <= width - INSET + 0.5);
        assert!(label.x >= INSET - 0.5);
        let at = Point::new(
            label.x + label.size.width / 2.,
            label.y + label.size.height / 2.,
        );
        for _ in 0..60 {
            h.tick(Input {
                pointer: PointerInput {
                    pos: Some(at),
                    ..Default::default()
                },
                ..Default::default()
            });
        }
        let tip =
            h.ui.scene()
                .unwrap()
                .surface("/tip")
                .expect("remembered-size help survives a real hover")
                .frame;
        assert!(tip.x >= -0.5 && tip.x + tip.size.width <= width + 0.5);
    }
}

fn library_menu_specimen() -> Arc<SamplerParams> {
    let p = Arc::new(SamplerParams::new());
    let dir = "/virtual/copy-audit/Strings";
    let mut view = p.shared.view.lock().unwrap();
    view.files = Arc::new(vec![format!("{dir}/Instrument.nki").into()]);
    view.shelf = Arc::new(crate::library::Shelf::new(vec![crate::library::Library {
        dir: dir.into(),
        name: "Strings".into(),
        instruments: 1,
        ..Default::default()
    }]));
    view.scanned = p.shared.libraries.wanted();
    drop(view);
    p
}

fn open_library_menu(h: &mut Harness) {
    h.settle_art();
    let card = h.ui.scene().unwrap().surface("library-0").unwrap();
    let top = card
        .frame
        .y
        .max(card.clip.map_or(f64::NEG_INFINITY, |c| c.y0));
    let bottom =
        (card.frame.y + card.frame.size.height).min(card.clip.map_or(f64::INFINITY, |c| c.y1));
    assert!(
        bottom > top,
        "the library card is visible before right-click"
    );
    let at = Point::new(
        card.frame.x + card.frame.size.width / 2.,
        (top + bottom) / 2.,
    );
    for buttons in [
        Buttons::default().set(Button::Secondary, true),
        Buttons::default(),
    ] {
        h.tick(Input {
            pointer: PointerInput {
                pos: Some(at),
                buttons,
                ..Default::default()
            },
            ..Default::default()
        });
    }
    h.idle(20);
}

#[test]
fn menu_explanations_leave_the_action_column_and_stay_on_hover() {
    for (width, height) in [(900., 600.), (1180., 900.)] {
        let p = library_menu_specimen();
        let mut h = Harness::new(&p, width, height);
        open_library_menu(&mut h);
        let scene = h.ui.scene().unwrap();
        assert!(!scene.surfaces().any(
            |s| s.text_value.as_deref() == Some("Display name only; files stay where they are")
        ));
        let item = scene.surface("menu-item-0").unwrap();
        assert_eq!(
            item.tip.as_deref(),
            Some("Rename…\nDisplay name only; files stay where they are")
        );
        let label = scene
            .surfaces()
            .find(|s| s.text_value.as_deref() == Some("Rename…"))
            .unwrap()
            .frame;
        assert!(
            label.size.width >= item.frame.size.width / 2.,
            "explanations must not squeeze the action caption"
        );
        assert!(label.x + label.size.width <= item.frame.x + item.frame.size.width - SPACE + 0.5);
        let at = tests::center(&h.ui, "menu-item-0");
        for _ in 0..60 {
            h.tick(Input {
                pointer: PointerInput {
                    pos: Some(at),
                    ..Default::default()
                },
                ..Default::default()
            });
        }
        assert!(
            h.ui.scene().unwrap().surface("/tip").is_some(),
            "the explanation survives actual hover capture"
        );
    }
}

#[test]
#[cfg(feature = "shots")]
fn copy_audit_shots() {
    let Some(out) = std::env::var_os("KONTRA_COPY_SHOTS").map(PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&out).unwrap();
    for (width, height) in [(900, 600), (1180, 900)] {
        let p = library_menu_specimen();
        let mut h = Harness::new(&p, width as f64, height as f64);
        let save = |name: &str, h: &mut Harness| {
            h.idle(20);
            moose::core::screenshot::save_png(
                &out.join(format!("{name}-{width}.png")),
                &pixels(&h.ui, width, height),
                width.into(),
                height.into(),
            );
        };
        open_library_menu(&mut h);
        save("library-menu", &mut h);
        h.tick(Input {
            keys: vec![KeyPress {
                key: Key::Escape,
                mods: Mods::default(),
            }],
            ..Default::default()
        });
        h.press("app-menu");
        save("app-menu", &mut h);
        h.press("menu-item-5");
        h.ui.set_scroll("settings-body", [0., CONTROL]);
        save("settings", &mut h);
        h.press("settings-close");
        h.press("app-menu");
        h.press("menu-item-3");
        save("about", &mut h);
    }
}

#[test]
fn compact_menu_keeps_keyboard_shortcuts_visible() {
    for (width, height) in [(900., 600.), (1180., 900.)] {
        let p = library_menu_specimen();
        p.selection.write().unwrap().parts = vec![Part {
            path: "/virtual/copy-audit/Strings/Instrument.nki".into(),
            name: "Strings".into(),
            ..Default::default()
        }];
        let mut h = Harness::new(&p, width, height);
        h.press("more-0");
        h.idle(20);
        for shortcut in ["Ctrl+D", "Del"] {
            assert!(
                h.ui.scene()
                    .unwrap()
                    .surfaces()
                    .any(|s| s.text_value.as_deref() == Some(shortcut)),
                "the part menu retains {shortcut}"
            );
        }
        h.tick(Input {
            keys: vec![KeyPress {
                key: Key::Escape,
                mods: Mods::default(),
            }],
            ..Default::default()
        });
        h.press("library-0");
        let at = tests::center(&h.ui, "instrument-0");
        for buttons in [
            Buttons::default().set(Button::Secondary, true),
            Buttons::default(),
        ] {
            h.tick(Input {
                pointer: PointerInput {
                    pos: Some(at),
                    buttons,
                    ..Default::default()
                },
                ..Default::default()
            });
        }
        h.idle(20);
        assert!(
            h.ui.scene()
                .unwrap()
                .surfaces()
                .any(|s| s.text_value.as_deref() == Some("Enter")),
            "the preset menu retains Enter"
        );
    }
}
