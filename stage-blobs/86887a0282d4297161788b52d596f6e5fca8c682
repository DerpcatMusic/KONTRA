use super::*;

/// The two halves of `.disabled` are one feature: the look it declared
/// for `State::Disabled` is painted, the look it declared for a hover is
/// not, and the pointer sitting on it produces no gesture at all.
#[test]
fn a_disabled_node_paints_its_off_look_and_hits_nothing() {
    let mut ui = Ui::default();
    let tree = |off: bool| {
        block(100., 100.)
            .fill(Role::Field)
            .on(State::Hover, |s| s.fill(Role::Primary))
            .on(State::Disabled, |s| s.fill(Role::Dim))
            .when(off, Styled::disabled)
            .focusable()
            .a11y(A11y::Button)
            .id("bypass")
    };
    let fill = |f: &Frame<'_>| {
        f.scene
            .paint
            .iter()
            .find(|p| &*p.key == "bypass")
            .expect("painted")
            .paint
            .solid()
    };
    let pal = Theme::DEFAULT.palette;
    let role = |r: Role| match Fill::from(r).paint(&pal, pal.background()) {
        Some(Paint::Solid(c)) => c,
        _ => panic!("a role paints solid"),
    };
    // Live: two frames, because a gesture reads the previous hit map.
    ui.frame(tree(false), None, at(50., 50., false), 0.016)
        .unwrap();
    for _ in 0..12 {
        ui.frame(tree(false), None, at(50., 50., false), 0.016)
            .unwrap();
    }
    let frame = ui
        .frame(tree(false), None, at(50., 50., false), 0.016)
        .unwrap();
    let lit = fill(&frame);
    assert!(ui.get("bypass").hovered, "live, and under the pointer");
    assert_eq!(lit, role(Role::Primary), "the explicit hover look, once");

    for _ in 0..2 {
        ui.frame(tree(true), None, at(50., 50., true), 0.016)
            .unwrap();
    }
    let off = fill(
        &ui.frame(tree(true), None, at(50., 50., true), 0.016)
            .unwrap(),
    );
    let r = ui.get("bypass");
    assert!(!r.hovered && !r.pressed && !r.held, "no gesture: {r:?}");
    assert_eq!(off, role(Role::Dim), "the disabled look, unlifted");

    // And it is no Tab stop -- nor does it keep a focus it already had.
    ui.frame(tree(true), None, key(Key::Tab), 0.016).unwrap();
    assert!(!ui.focused("bypass"));
    ui.focus("bypass");
    ui.frame(tree(true), None, PointerInput::default(), 0.016)
        .unwrap();
    assert!(!ui.focused("bypass"), "a focus on a dead node is dropped");
}

/// A shortcut is not focus-gated -- except by a field that is typing.
#[test]
fn a_shortcut_fires_unfocused_and_never_while_a_field_has_the_focus() {
    let mut ui = Ui::default();
    let mut value = String::new();
    let undo = |ui: &Ui| {
        ui.shortcuts()
            .iter()
            .any(|k| k.key == Key::Function(1) || k.key == Key::Space)
    };
    let tree = |ui: &mut Ui, v: &mut String| widgets::text_input(ui, "f", v).el;

    let root = tree(&mut ui, &mut value);
    ui.frame(root, None, key(Key::Function(1)), 0.016).unwrap();
    assert!(undo(&ui), "nothing is focused, so the shortcut is ours");
    assert!(!ui.focus_is_text());

    ui.focus("f");
    let root = tree(&mut ui, &mut value);
    ui.frame(root, None, key(Key::Space), 0.016).unwrap();
    assert!(!undo(&ui), "the field is typing: the key is its own");
    assert!(ui.focus_is_text());
    assert_eq!(ui.keys("f").len(), 1, "and it still gets it");
}

#[test]
fn tab_walks_the_focusable_surfaces_in_scene_order() {
    let mut ui = Ui::default();
    let tree = || {
        col([
            block(20., 20.).focusable().id("a"),
            block(20., 20.).id("plain"),
            block(20., 20.).focusable().id("b"),
        ])
    };
    ui.frame(tree(), None, PointerInput::default(), 0.016)
        .unwrap();
    for want in ["a", "b", "a"] {
        ui.frame(tree(), None, key(Key::Tab), 0.016).unwrap();
        assert!(ui.focused(want), "expected {want}");
    }
    ui.frame(tree(), None, key(Key::Escape), 0.016).unwrap();
    assert!(!ui.focused("a") && !ui.focused("b"));
}

/// A click focuses without a ring; Tab and `Ui::focus` focus with one.
#[test]
fn focus_is_visible_from_the_keyboard_and_code_not_from_a_click() {
    let mut ui = Ui::default();
    let tree = || {
        col([block(20., 20.)
            .fill(Role::Field)
            .on(State::FocusVisible, |s| s.stroke(Role::Primary))
            .focusable()
            .id("a")])
    };
    let ring = |f: &Frame<'_>| {
        f.scene
            .paint
            .iter()
            .any(|p| &*p.key == "a" && p.layer == Layer::Stroke)
    };
    ui.frame(tree(), None, PointerInput::default(), 0.016)
        .unwrap();
    ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
    let lit = ring(&ui.frame(tree(), None, at(10., 10., true), 0.016).unwrap());
    assert!(!lit);
    ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
    let lit = ring(&ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap());
    assert!(
        ui.focused("a") && !ui.focus_visible("a"),
        "clicked: no ring"
    );
    assert!(!lit);
    ui.frame(tree(), None, key(Key::Tab), 0.016).unwrap();
    let lit = ring(
        &ui.frame(tree(), None, PointerInput::default(), 0.016)
            .unwrap(),
    );
    assert!(ui.focus_visible("a") && lit, "tabbed: a ring");
    ui.blur();
    ui.focus("a");
    let lit = ring(
        &ui.frame(tree(), None, PointerInput::default(), 0.016)
            .unwrap(),
    );
    assert!(ui.focus_visible("a") && lit, "focused from code: a ring");
}

/// A shipped button is a real Tab stop and Enter reaches the same action
/// path as a primary click.
#[test]
fn a_button_can_be_focused_and_activated_from_the_keyboard() {
    fn tree(ui: &mut Ui) -> El {
        widgets::button(ui, "button", "Save").el.into_el()
    }

    let mut ui = Ui::default();
    let root = tree(&mut ui);
    ui.frame(root, None, PointerInput::default(), 0.016)
        .unwrap();
    let root = tree(&mut ui);
    ui.frame(root, None, key(Key::Tab), 0.016).unwrap();
    assert_eq!(ui.focus_key(), Some("button"));

    // Keys are delivered to the tree on the following frame, exactly as
    // mouse edges are, so the widget sees the same activation boundary.
    let root = tree(&mut ui);
    ui.frame(root, None, key(Key::Enter), 0.016).unwrap();
    let activated = widgets::button(&mut ui, "button", "Save").changed;
    assert!(activated, "Enter activates the focused button");
}

#[test]
fn typing_reaches_the_focused_field() {
    let mut ui = Ui::default();
    let mut value = String::new();
    let tree = |ui: &mut Ui, v: &mut String| widgets::text_input(ui, "f", v).el;
    let root = tree(&mut ui, &mut value);
    ui.frame(root, None, PointerInput::default(), 0.016)
        .unwrap();
    ui.focus("f");
    let typed = Input {
        text: "hi".into(),
        ..Input::default()
    };
    let root = tree(&mut ui, &mut value);
    ui.frame(root, None, typed, 0.016).unwrap();
    let root = tree(&mut ui, &mut value);
    ui.frame(root, None, key(Key::Backspace), 0.016).unwrap();
    let root = tree(&mut ui, &mut value);
    ui.frame(root, None, PointerInput::default(), 0.016)
        .unwrap();
    assert_eq!(value, "h");
}

/// A field tells a screen reader where its selection is and where each
/// character sits, and a reader's `SetTextSelection` moves it.
#[test]
fn a_field_reports_its_selection_and_carets_and_a_reader_can_select() {
    let mut ui = Ui::default();
    let mut value = String::from("héllo");
    let tree = |ui: &mut Ui, v: &mut String| widgets::text_input(ui, "f", v).el;
    let text = |ui: &Ui| match &ui.scene().unwrap().surface("f").unwrap().semantics {
        Some(mui_scene::Semantics {
            role: A11y::TextInput {
                selection, carets, ..
            },
            ..
        }) => (*selection, carets.clone()),
        _ => panic!("not a text input"),
    };
    let root = tree(&mut ui, &mut value);
    ui.frame(root, None, Input::default(), 0.016).unwrap();
    ui.focus("f");
    let root = tree(&mut ui, &mut value);
    ui.frame(root, None, key(Key::End), 0.016).unwrap();
    let root = tree(&mut ui, &mut value);
    ui.frame(root, None, Input::default(), 0.016).unwrap();
    let (sel, carets) = text(&ui);
    assert_eq!(sel, (5, 5), "the caret went to the end");
    assert_eq!(carets.len(), 6, "one per boundary of five characters");
    assert_eq!(carets[0], 8.0, "the text starts inside the padding");
    assert!(carets.windows(2).all(|w| w[1] > w[0]), "{carets:?}");

    assert!(ui.request_action(SemanticAction::set_selection("f", 1, 4)));
    assert!(!ui.request_action(SemanticAction::set_selection("f", 0, 6)));
    let root = tree(&mut ui, &mut value);
    let edits = ui
        .frame(root, None, Input::default(), 0.016)
        .unwrap()
        .edits
        .len();
    assert_eq!(text(&ui).0, (1, 4), "the reader's selection landed");
    assert_eq!(edits, 0, "a selection is not a bracketed edit");
    assert_eq!(value, "héllo");
}

#[test]
fn a_composition_paints_without_editing_the_value_and_the_commit_inserts() {
    let mut ui = Ui::default();
    let mut value = "ab".to_owned();
    let tree = |ui: &mut Ui, v: &mut String| widgets::text_input(ui, "f", v).el;
    let ime = |e: mui_input::Ime| Input {
        ime: vec![e],
        ..Input::default()
    };
    let root = tree(&mut ui, &mut value);
    ui.frame(root, None, PointerInput::default(), 0.016)
        .unwrap();
    ui.focus("f");
    ui.set_sel("f", 1, 1);

    let root = tree(&mut ui, &mut value);
    let f = ui
        .frame(
            root,
            None,
            ime(mui_input::Ime::Preedit {
                text: "xy".into(),
                cursor: Some((1, 1)),
            }),
            0.016,
        )
        .unwrap();
    let (at, _) = f.ime.expect("the host is told where the caret is");
    assert!(
        at.x > 0.0,
        "and the caret is in the scene's space, not the field's"
    );
    // The preedit reaches the *next* tree, as every input does here.
    let root = tree(&mut ui, &mut value);
    let f = ui
        .frame(root, None, PointerInput::default(), 0.016)
        .unwrap();
    assert_eq!(value, "ab", "a preedit never touches the value");
    let geometry = f
        .scene
        .surface("f")
        .unwrap()
        .text_geometry
        .as_ref()
        .unwrap();
    assert_eq!(geometry.text.as_ref(), "axyb");
    assert_eq!(geometry.state.marked, 1..3);
    assert!(
        f.scene
            .paint
            .iter()
            .any(|p| p.key.as_str() == "f" && matches!(p.layer, mui_scene::Layer::Draw(_))),
        "preedit underline"
    );

    let root = tree(&mut ui, &mut value);
    ui.frame(root, None, ime(mui_input::Ime::Commit("xy".into())), 0.016)
        .unwrap();
    let root = tree(&mut ui, &mut value);
    let f = ui
        .frame(root, None, PointerInput::default(), 0.016)
        .unwrap();
    assert_eq!(value, "axyb", "the commit landed at the caret");
    assert!(
        f.scene
            .surface("f")
            .unwrap()
            .text_geometry
            .as_ref()
            .unwrap()
            .state
            .marked
            .is_empty(),
        "and the composition, with it the underline, is gone"
    );
}

#[test]
fn a_long_value_scrolls_under_the_clip_instead_of_wrapping() {
    let mut ui = Ui::default();
    let mut value = "x".repeat(60);
    let win = Some(Size::new(200., 60.));
    let tree = |ui: &mut Ui, v: &mut String| widgets::text_input(ui, "f", v).el;
    let root = tree(&mut ui, &mut value);
    ui.frame(root, win, PointerInput::default(), 0.016).unwrap();
    ui.set_sel("f", 60, 60);
    let root = tree(&mut ui, &mut value);
    let f = ui.frame(root, win, PointerInput::default(), 0.016).unwrap();
    let field = f.scene.surface("f").expect("field");
    let geometry = field.text_geometry.as_ref().unwrap();
    assert_eq!(geometry.lines.len(), 1, "one visual line");
    assert!(
        geometry.caret.x + 2. <= field.frame.size.width && geometry.caret.x >= 0.,
        "the caret stayed in the field: {geometry:?}"
    );
}

#[test]
fn a_selection_is_extended_by_shift_and_deleted_as_one() {
    let mut ui = Ui::default();
    let mut value = String::from("hello");
    let run = |ui: &mut Ui, v: &mut String, input: Input| {
        let root = widgets::text_input(ui, "f", v).el;
        ui.frame(root, None, input, 0.016).unwrap();
    };
    run(&mut ui, &mut value, Input::default());
    ui.focus("f");
    let shift = |k| Input {
        keys: vec![KeyPress {
            key: k,
            mods: Mods {
                shift: true,
                ..Mods::default()
            },
        }],
        ..Input::default()
    };
    run(&mut ui, &mut value, shift(Key::Right));
    run(&mut ui, &mut value, shift(Key::Right));
    run(&mut ui, &mut value, key(Key::Backspace));
    run(&mut ui, &mut value, Input::default());
    assert_eq!(value, "llo", "two characters selected, one Backspace");
}

#[test]
fn copy_asks_the_host_for_the_clipboard_and_paste_takes_it_back() {
    let mut ui = Ui::default();
    let mut value = String::from("hi");
    let run = |ui: &mut Ui, v: &mut String, input: Input| {
        let root = widgets::text_input(ui, "f", v).el;
        ui.frame(root, None, input, 0.016)
            .unwrap()
            .clipboard
            .clone()
    };
    run(&mut ui, &mut value, Input::default());
    ui.focus("f");
    let ctrl = |c: char, clipboard: Option<String>| Input {
        keys: vec![KeyPress {
            key: Key::Char(c),
            mods: Mods {
                ctrl: true,
                ..Mods::default()
            },
        }],
        clipboard,
        ..Input::default()
    };
    run(&mut ui, &mut value, ctrl('a', None));
    run(&mut ui, &mut value, ctrl('c', None));
    let out = run(&mut ui, &mut value, Input::default());
    assert_eq!(out.as_deref(), Some("hi"), "the copy reached the frame");
    run(&mut ui, &mut value, ctrl('v', Some("yo".into())));
    run(&mut ui, &mut value, Input::default());
    assert_eq!(value, "yo", "and a paste replaced the selection");
}

#[test]
fn a_live_readout_swaps_its_glyphs_without_resolving_again() {
    use std::cell::Cell;
    use std::rc::Rc;

    let walks = Rc::new(Cell::new(0));
    let (w, seen) = (walks.clone(), walks.clone());
    let tree = move || {
        let w = w.clone();
        row![
            text("0.0").reserve("-88.8").id("gain"),
            canvas(move |_| {
                w.set(w.get() + 1);
                Vec::new()
            })
            .size(10., 10.)
        ]
    };
    let mut ui = Ui::default().font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
    ui.frame(tree(), Some(Size::new(300., 40.)), Input::default(), 0.016)
        .unwrap();
    assert_eq!(seen.get(), 1, "the one resolve");

    let glyphs = |ui: &Ui| {
        let t = ui
            .scene()
            .unwrap()
            .paint
            .iter()
            .find(|p| p.layer == mui_scene::Layer::Text);
        t.unwrap().text.clone().unwrap().glyphs
    };
    let (before, frame) = (
        glyphs(&ui),
        ui.scene().unwrap().surface("gain").unwrap().frame,
    );
    let hit_geometry = ui.scene().unwrap().clone();
    for i in 0..32 {
        ui.set_text("gain", format!("-{i}.5")).unwrap();
    }
    assert_eq!(seen.get(), 1, "32 readouts, still one layout resolve");
    assert_ne!(glyphs(&ui), before, "and the glyphs did change");
    assert_eq!(ui.scene().unwrap().surface("gain").unwrap().frame, frame);
    assert!(same_hit_geometry(&hit_geometry, ui.scene().unwrap()));
}

#[test]
fn set_text_says_so_when_there_is_nothing_to_set() {
    let mut ui = Ui::default().font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
    assert!(matches!(
        ui.set_text("gain", "1"),
        Err(SceneError::NoTextLayer)
    ));
    let tree = row![block(20., 20.).id("box")];
    ui.frame(tree, Some(Size::new(80., 40.)), Input::default(), 0.016)
        .unwrap();
    assert!(matches!(
        ui.set_text("box", "1"),
        Err(SceneError::NoTextLayer)
    ));
    assert!(matches!(
        ui.set_text("nope", "1"),
        Err(SceneError::NoTextLayer)
    ));
}

#[test]
fn fallback_text_input_caret_uses_the_fallback_advance() {
    let ui = Ui::default()
        .font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap())
        .fallback_font(Font::new(epaint_default_fonts::NOTO_EMOJI_REGULAR).unwrap());
    let value = "A😀";
    let carets = ui.carets(value, 16.);
    let [_, (_, before), (_, end)] = carets[..] else {
        panic!("{carets:?}");
    };
    assert!(end > before, "fallback glyph has no caret advance");
    assert_eq!(ui.hit(value, 16., end), value.chars().count());
}

/// The unnamed root is decoration like any unnamed node: it does not
/// hover as `""`. Clicking the empty background still drops the focus,
/// because a press that lands on no target does -- not because the root
/// happened to be one.
#[test]
fn empty_background_is_no_target_and_a_press_on_it_drops_the_focus() {
    let tree = || {
        // The field sits centred along the top: x 80..120, y 0..20.
        col![block(40., 20.).focusable().id("field")]
            .size(200., 200.)
            .fill(Role::Background)
    };
    let mut ui = Ui::default();
    ui.frame(tree(), None, Input::default(), 0.016).unwrap();
    ui.frame(tree(), None, at(100., 10., true), 0.016).unwrap();
    assert!(ui.focused("field"), "a press on the field focuses it");
    ui.frame(tree(), None, at(100., 10., false), 0.016).unwrap();
    ui.frame(tree(), None, at(150., 150., false), 0.016)
        .unwrap();
    assert_eq!(ui.interaction.hovered(), None, "the root is no target");
    assert!(ui.focused("field"), "hovering the background keeps it");
    ui.frame(tree(), None, at(150., 150., true), 0.016).unwrap();
    assert!(!ui.focused("field"), "a press on nothing drops it");
    assert_eq!(ui.interaction.held(), None, "and captures nothing");

    // A press that starts on the field and a second button going down
    // elsewhere mid-gesture is still the field's gesture.
    ui.frame(tree(), None, at(100., 10., false), 0.016).unwrap();
    ui.frame(tree(), None, at(100., 10., true), 0.016).unwrap();
    let both = PointerInput {
        pos: Some(Point::new(150., 150.)),
        buttons: Buttons::PRIMARY.set(Button::Secondary, true),
        ..PointerInput::default()
    };
    ui.frame(tree(), None, both, 0.016).unwrap();
    assert!(ui.focused("field"), "held, the field keeps the focus");
}

/// Enter on a button and an arrow on a slider are edits a plugin host
/// has to see bracketed, on the frame the tree applied them.
#[test]
fn keyboard_edits_are_bracketed_and_a_slider_steps() {
    let mut ui = Ui::default();
    let mut v = 0.5;
    let mut clicks = 0;
    let mut tree = |ui: &mut Ui| {
        let widgets::Response {
            el: b,
            changed: clicked,
        } = widgets::button(ui, "b", "Go");
        clicks += usize::from(clicked);
        let s = widgets::slider(ui, "s", "S", &mut v, 0.0..=1.0).el;
        col([b.el(), s.el()]).w(200.)
    };
    let root = tree(&mut ui);
    ui.frame(root, None, Input::default(), 0.016).unwrap();
    ui.focus("b");
    let root = tree(&mut ui);
    ui.frame(root, None, key(Key::Enter), 0.016).unwrap();
    let root = tree(&mut ui);
    let f = ui.frame(root, None, Input::default(), 0.016).unwrap();
    assert_eq!(
        f.edits,
        [("b".into(), Edit::Begin), ("b".into(), Edit::End)]
    );
    ui.focus("s");
    let shifted = |k| Input {
        keys: vec![KeyPress {
            key: k,
            mods: Mods {
                shift: true,
                ..Mods::default()
            },
        }],
        ..Input::default()
    };
    for input in [key(Key::Right), shifted(Key::Left), key(Key::PageDown)] {
        let root = tree(&mut ui);
        ui.frame(root, None, input, 0.016).unwrap();
    }
    let root = tree(&mut ui);
    let f = ui.frame(root, None, key(Key::End), 0.016).unwrap();
    assert_eq!(
        f.edits,
        [("s".into(), Edit::Begin), ("s".into(), Edit::End)]
    );
    assert_eq!(clicks, 1);
    assert!((v - 0.409).abs() < 1e-9, "+0.01, -0.001, -0.1: {v}");
    let mut tree = |ui: &mut Ui| {
        widgets::slider(ui, "s", "S", &mut v, 0.0..=1.0)
            .el
            .into_el()
    };
    let root = tree(&mut ui);
    ui.frame(root, None, Input::default(), 0.016).unwrap();
    assert_eq!(v, 1.0, "End goes to the end");
}

/// A platform's Increment takes the arrow keys' step, as a `SetValue`.
#[test]
fn increment_and_decrement_step_like_the_arrow_keys() {
    let mut ui = Ui::default();
    let mut v = 0.5;
    let root = widgets::slider(&mut ui, "s", "S", &mut v, 0.0..=1.0)
        .el
        .into_el();
    ui.frame(root, None, Input::default(), 0.016).unwrap();
    assert!(ui.request_action(SemanticAction::increment("s")));
    assert!(ui.request_action(SemanticAction::increment("s")));
    assert!(ui.request_action(SemanticAction::decrement("s")));
    let _ = widgets::slider(&mut ui, "s", "S", &mut v, 0.0..=1.0);
    assert!((v - 0.51).abs() < 1e-12, "+0.01 +0.01 -0.01: {v}");
    let f = ui
        .frame(block(1., 1.), None, Input::default(), 0.016)
        .unwrap();
    assert_eq!(
        f.edits,
        [("s".into(), Edit::Begin), ("s".into(), Edit::End)]
    );
    assert!(!ui.request_action(SemanticAction::decrement("absent")));
}

/// A click in a field scrolled to its tail lands where the text was
/// painted, not where it would be unscrolled.
#[test]
fn a_click_in_a_scrolled_field_lands_on_the_painted_character() {
    let mut ui = Ui::default();
    let mut value = "x".repeat(60);
    let win = Some(Size::new(200., 60.));
    let tree = |ui: &mut Ui, v: &mut String| widgets::text_input(ui, "f", v).el;
    let root = tree(&mut ui, &mut value);
    ui.frame(root, win, PointerInput::default(), 0.016).unwrap();
    ui.set_sel("f", 60, 60);
    let root = tree(&mut ui, &mut value);
    let f = ui.frame(root, win, PointerInput::default(), 0.016).unwrap();
    let field = f.scene.surface("f").unwrap().frame;
    let (x, y) = (field.right() - 10., field.y + field.size.height / 2.);
    let root = tree(&mut ui, &mut value);
    ui.frame(root, win, at(x, y, true), 0.016).unwrap();
    tree(&mut ui, &mut value);
    let (_, caret) = ui.sel("f");
    assert!(caret >= 58, "the tail was under the pointer, got {caret}");
}

#[test]
fn editable_wrap_and_candidate_caret_use_current_bounds_on_first_frame_and_resize() {
    let mut ui = Ui::default().font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
    let mut value = "one two three four five six seven eight nine".to_owned();
    ui.set_sel("f", value.chars().count(), value.chars().count());
    ui.focus("f");
    let tree = |ui: &mut Ui, value: &mut String| {
        widgets::text_edit(
            ui,
            "f",
            value,
            widgets::TextOpts {
                newline: widgets::Newline::Enter,
                rows: 2,
                ..Default::default()
            },
        )
        .el
    };
    let root = tree(&mut ui, &mut value);
    let frame = ui
        .frame(root, Some(Size::new(110., 80.)), Input::default(), 0.016)
        .unwrap();
    let field = frame.scene.surface("f").unwrap();
    let g = field.text_geometry.as_ref().unwrap();
    assert!(g.lines.len() > 2, "the first frame wraps: {g:?}");
    assert!(g.scroll > 0., "first-frame caret is brought into view");
    let first_count = g.lines.len();
    if let Some((at, _)) = frame.ime {
        assert_eq!(
            at,
            Point::new(field.frame.x + g.caret.x, field.frame.y + g.caret.y)
        );
    }
    let snapshot = ui.scene_snapshot().unwrap();
    let root = tree(&mut ui, &mut value);
    let frame = ui
        .frame(root, Some(Size::new(260., 80.)), Input::default(), 0.016)
        .unwrap();
    let field = frame.scene.surface("f").unwrap();
    let g = field.text_geometry.as_ref().unwrap();
    assert!(
        g.lines.len() < first_count,
        "the resize frame uses the new width"
    );
    assert_eq!(
        snapshot
            .surface("f")
            .unwrap()
            .text_geometry
            .as_ref()
            .unwrap()
            .lines
            .len(),
        first_count,
        "the snapshot keeps the previous width's editable geometry"
    );
    assert!(g.caret.x + 2. <= field.frame.size.width);
    let (at, _) = frame.ime.expect("candidate caret after focus intake");
    assert_eq!(
        at,
        Point::new(field.frame.x + g.caret.x, field.frame.y + g.caret.y)
    );
}

#[test]
fn composition_replaces_selection_and_maps_display_hits_back_to_source() {
    let mut ui = Ui::default().font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
    let mut value = "a😀bc".to_owned();
    let tree = |ui: &mut Ui, value: &mut String| widgets::text_input(ui, "f", value).el;
    let root = tree(&mut ui, &mut value);
    ui.frame(root, Some(Size::new(180., 50.)), Input::default(), 0.016)
        .unwrap();
    ui.focus("f");
    ui.set_sel("f", 1, 3);
    let root = tree(&mut ui, &mut value);
    ui.frame(
        root,
        None,
        Input {
            ime: vec![mui_input::Ime::Preedit {
                text: "e\u{301}字".into(),
                cursor: Some((1, 3)),
            }],
            ..Default::default()
        },
        0.016,
    )
    .unwrap();
    let root = tree(&mut ui, &mut value);
    let frame = ui
        .frame(root, Some(Size::new(100., 50.)), Input::default(), 0.016)
        .unwrap();
    let g = frame
        .scene
        .surface("f")
        .unwrap()
        .text_geometry
        .as_ref()
        .unwrap();
    assert_eq!(value, "a😀bc");
    assert_eq!(g.text.as_ref(), "ae\u{301}字c");
    assert_eq!(g.state.marked, 1..7);
    assert_eq!(g.state.caret, 4, "cursor snapped to grapheme boundary");
    assert_eq!(
        g.display_to_source(7),
        6,
        "after preedit maps past the replaced emoji and b"
    );
    assert_eq!(
        g.display_to_source(3),
        1,
        "inside composition maps to replacement start"
    );
    let last = &g.lines[0];
    let point = Point::new(
        last.origin.x + last.carets.x(g.text.len()),
        last.origin.y + g.line_height / 2.,
    );
    assert_eq!(g.hit(point), value.len());
    assert!(
        g.lines[0]
            .carets
            .selection_spans(1..4)
            .iter()
            .all(|s| s.end >= s.start)
    );
    let native = ui.text_input_state().unwrap();
    assert_eq!(native.text, "ae\u{301}字c");
    assert_eq!(native.marked, Some(1..7));
}

#[test]
fn multiline_visual_movement_preserves_goal_x_across_short_lines() {
    let mut ui = Ui::default().font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
    let mut value = "abcdef\nx\nabcdef".to_owned();
    let tree = |ui: &mut Ui, v: &mut String| {
        widgets::text_edit(
            ui,
            "f",
            v,
            widgets::TextOpts {
                newline: widgets::Newline::Enter,
                ..Default::default()
            },
        )
        .el
    };
    let root = tree(&mut ui, &mut value);
    ui.frame(root, Some(Size::new(220., 100.)), Input::default(), 0.016)
        .unwrap();
    ui.focus("f");
    ui.set_sel("f", 5, 5);
    let root = tree(&mut ui, &mut value);
    ui.frame(root, Some(Size::new(220., 100.)), key(Key::Down), 0.016)
        .unwrap();
    let root = tree(&mut ui, &mut value);
    ui.frame(root, Some(Size::new(220., 100.)), key(Key::Down), 0.016)
        .unwrap();
    assert_eq!(ui.sel("f"), (8, 8), "short middle line clamps caret");
    tree(&mut ui, &mut value);
    assert_eq!(ui.sel("f"), (14, 14), "second move restores goal column");
}

#[test]
fn native_replacement_selection_and_commits_keep_batch_order() {
    use mui_input::Ime;
    let mut ui = Ui::default();
    let mut value = "a😀bc".to_owned();
    let tree = |ui: &mut Ui, v: &mut String| widgets::text_input(ui, "f", v).el;
    let root = tree(&mut ui, &mut value);
    ui.frame(root, None, Input::default(), 0.016).unwrap();
    ui.focus("f");
    assert!(
        !ui.set_text_input_selection(2..3),
        "reject interior UTF8 byte ranges"
    );
    assert!(ui.set_text_input_selection(1..5));
    assert_eq!(ui.sel("f"), (1, 2));
    let root = tree(&mut ui, &mut value);
    ui.frame(
        root,
        None,
        Input {
            ime: vec![
                Ime::Selection(1..5),
                Ime::Commit("xy".into()),
                Ime::Selection(0..1),
                Ime::Commit("字".into()),
            ],
            ..Default::default()
        },
        0.016,
    )
    .unwrap();
    let response = widgets::text_input(&mut ui, "f", &mut value);
    assert!(response.changed);
    assert_eq!(
        value, "字xybc",
        "second replacement acts on first commit's buffer"
    );
    assert_eq!(ui.sel("f"), (1, 1));
}

#[test]
fn end_at_a_soft_wrap_keeps_the_caret_on_the_previous_visual_line() {
    let mut ui = Ui::default().font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
    let mut value = "abcdefghijk".to_owned();
    let tree = |ui: &mut Ui, v: &mut String| {
        widgets::text_edit(
            ui,
            "f",
            v,
            widgets::TextOpts {
                newline: widgets::Newline::Enter,
                ..Default::default()
            },
        )
        .el
    };
    let root = tree(&mut ui, &mut value);
    ui.frame(root, Some(Size::new(65., 100.)), Input::default(), 0.016)
        .unwrap();
    ui.focus("f");
    let root = tree(&mut ui, &mut value);
    ui.frame(root, Some(Size::new(65., 100.)), key(Key::End), 0.016)
        .unwrap();
    let root = tree(&mut ui, &mut value);
    let f = ui
        .frame(root, Some(Size::new(65., 100.)), Input::default(), 0.016)
        .unwrap();
    let g = f
        .scene
        .surface("f")
        .unwrap()
        .text_geometry
        .as_ref()
        .unwrap();
    assert_eq!(g.state.caret, g.lines[0].range.end);
    assert_eq!(g.row(g.state.caret), 0);
    assert_eq!(g.caret.y, g.lines[0].origin.y);
}

#[test]
fn selecting_hard_breaks_and_empty_lines_paints_each_newline() {
    let mut ui = Ui::default().font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
    let mut value = "ab\n\ncd".to_owned();
    ui.set_sel("f", 2, 4);
    let root = widgets::text_edit(
        &mut ui,
        "f",
        &mut value,
        widgets::TextOpts {
            newline: widgets::Newline::Enter,
            ..Default::default()
        },
    )
    .el;
    let f = ui
        .frame(root, Some(Size::new(100., 100.)), Input::default(), 0.016)
        .unwrap();
    assert_eq!(
        f.scene
            .surface("f")
            .unwrap()
            .text_geometry
            .as_ref()
            .unwrap()
            .lines
            .len(),
        3
    );
    assert_eq!(
        f.scene
            .paint
            .iter()
            .filter(|p| p.key.as_str() == "f" && matches!(p.layer, mui_scene::Layer::Draw(_)))
            .count(),
        2,
        "both the first and empty line's selected newline have a visible band"
    );
}

#[test]
fn a_rtl_selection_collapses_toward_the_visual_arrow_edge() {
    let mut ui = Ui::default().font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
    let mut value = "אבג".to_owned();
    let tree = |ui: &mut Ui, v: &mut String| widgets::text_input(ui, "f", v).el;
    let root = tree(&mut ui, &mut value);
    ui.frame(root, None, Input::default(), 0.016).unwrap();
    ui.focus("f");
    ui.set_sel("f", 0, 3);
    let root = tree(&mut ui, &mut value);
    ui.frame(root, None, key(Key::Right), 0.016).unwrap();
    tree(&mut ui, &mut value);
    assert_eq!(
        ui.sel("f"),
        (0, 0),
        "rightmost selection endpoint is logical start"
    );
}

#[test]
fn a_pointer_during_preedit_keeps_the_pending_replacement_selection() {
    let mut ui = Ui::default();
    let mut value = "abcdef".to_owned();
    let tree = |ui: &mut Ui, v: &mut String| widgets::text_input(ui, "f", v).el;
    let root = tree(&mut ui, &mut value);
    ui.frame(root, Some(Size::new(150., 60.)), Input::default(), 0.016)
        .unwrap();
    ui.focus("f");
    ui.set_sel("f", 1, 4);
    let root = tree(&mut ui, &mut value);
    ui.frame(
        root,
        Some(Size::new(150., 60.)),
        Input {
            ime: vec![mui_input::Ime::Preedit {
                text: "XY".into(),
                cursor: Some((1, 1)),
            }],
            ..Default::default()
        },
        0.016,
    )
    .unwrap();
    let root = tree(&mut ui, &mut value);
    let f = ui
        .frame(root, Some(Size::new(150., 60.)), Input::default(), 0.016)
        .unwrap();
    let field = f.scene.surface("f").unwrap();
    let g = field.text_geometry.as_ref().unwrap();
    let p = Point::new(
        field.frame.x + g.lines[0].origin.x + g.lines[0].carets.x(2),
        field.frame.y + g.lines[0].origin.y + g.line_height / 2.,
    );
    let root = tree(&mut ui, &mut value);
    ui.frame(root, Some(Size::new(150., 60.)), at(p.x, p.y, true), 0.016)
        .unwrap();
    tree(&mut ui, &mut value);
    assert_eq!(ui.sel("f"), (1, 4), "composition still replaces bcd");
    let root = tree(&mut ui, &mut value);
    ui.frame(
        root,
        Some(Size::new(150., 60.)),
        Input {
            ime: vec![mui_input::Ime::Commit("XY".into())],
            ..Default::default()
        },
        0.016,
    )
    .unwrap();
    tree(&mut ui, &mut value);
    assert_eq!(value, "aXYef");
}
