use super::*;

#[test]
fn nested_scrollers_yield_and_shorter_content_clamps_the_offset() {
    let mut ui = Ui::default();
    let tree = |tall| {
        col([
            row([block(100., 20.)])
                .size(30., 20.)
                .scroll()
                .id("horizontal"),
            block(30., if tall { 150. } else { 10. }),
        ])
        .size(30., 40.)
        .scroll()
        .id("outer")
    };
    ui.frame(tree(true), None, PointerInput::default(), 0.016)
        .unwrap();
    let input = Input {
        pointer: at(10., 10., false),
        wheel: Vec2::new(0., 80.),
        ..Input::default()
    };
    let frame = ui.frame(tree(true), None, input, 0.016).unwrap();
    assert!(frame.animating, "wheel changes need a catch-up frame");
    assert_eq!(ui.scroll("horizontal"), [0., 0.]);
    assert_eq!(ui.scroll("outer"), [0., 80.]);
    let frame = ui
        .frame(tree(false), None, PointerInput::default(), 0.016)
        .unwrap();
    assert!(frame.animating, "clamping needs a catch-up frame");
    assert_eq!(ui.scroll("outer"), [0., 0.]);
}

#[test]
fn nested_and_floating_content_does_not_extend_the_outer_scroll() {
    let mut ui = Ui::default();
    let tree = col([
        col([block(30., 1000.)]).size(30., 40.).scroll().id("inner"),
        block(30., 20.),
        block(30., 900.).float(),
    ])
    .gap(0.)
    .size(30., 50.)
    .scroll()
    .id("outer");
    ui.frame(tree, None, PointerInput::default(), 0.016)
        .unwrap();
    assert_eq!(
        ui.scene().unwrap().surface("outer").unwrap().content.height,
        60.
    );
}

#[test]
fn an_exhausted_inner_scroll_yields_to_its_parent() {
    let mut ui = Ui::default();
    let tree = || {
        col([
            col([block(30., 80.)]).size(30., 40.).scroll().id("inner"),
            block(30., 100.),
        ])
        .size(30., 60.)
        .scroll()
        .id("outer")
    };
    let input = || Input {
        pointer: at(10., 10., false),
        wheel: Vec2::new(0., 100.),
        ..Input::default()
    };
    ui.frame(tree(), None, input(), 0.016).unwrap();
    assert_eq!(ui.scroll("inner"), [0., 40.]);
    ui.frame(tree(), None, input(), 0.016).unwrap();
    assert!(ui.scroll("outer")[1] > 0.);
}

#[test]
fn the_wheel_scrolls_a_column_and_stops_at_its_end() {
    let mut ui = Ui::default();
    let tree = || {
        col([block(20., 100.), block(20., 100.)])
            .h(50.)
            .scroll()
            .id("list")
    };
    let wheel = |y: f64| Input {
        pointer: at(10., 10., false),
        wheel: Vec2::new(0., y),
        ..Input::default()
    };
    ui.frame(tree(), None, PointerInput::default(), 0.016)
        .unwrap();
    ui.frame(tree(), None, wheel(30.), 0.016).unwrap();
    assert_eq!(ui.scroll("list"), [0., 30.]);
    ui.frame(tree(), None, wheel(1000.), 0.016).unwrap();
    let [_, y] = ui.scroll("list");
    assert!(y > 30. && y <= 200., "clamped to the overflow, got {y}");
    ui.frame(tree(), None, wheel(-1e6), 0.016).unwrap();
    assert_eq!(ui.scroll("list"), [0., 0.], "and not past the top");
}

#[test]
fn a_non_finite_wheel_delta_is_ignored() {
    let mut ui = Ui::default();
    let tree = || {
        col([block(20., 100.), block(20., 100.)])
            .h(50.)
            .scroll()
            .id("list")
    };
    let wheel = |y: f64| Input {
        pointer: at(10., 10., false),
        wheel: Vec2::new(0., y),
        ..Input::default()
    };
    ui.frame(tree(), None, PointerInput::default(), 0.016)
        .unwrap();
    ui.frame(tree(), None, wheel(f64::NAN), 0.016).unwrap();
    assert_eq!(ui.scroll("list"), [0., 0.]);
    ui.frame(tree(), None, wheel(30.), 0.016)
        .expect("and the surface still scrolls afterwards");
    assert_eq!(ui.scroll("list"), [0., 30.]);
}

/// The wheel moves the target at once; the drawn offset glides after
/// it, never back, and lands on it exactly.
#[test]
fn a_wheel_scroll_glides_onto_its_target() {
    let mut ui = Ui::default();
    let tree = || {
        col([block(20., 100.).id("top"), block(20., 100.)])
            .h(50.)
            .scroll()
            .id("list")
    };
    let wheel = Input {
        pointer: at(10., 10., false),
        wheel: Vec2::new(0., 60.),
        ..Input::default()
    };
    ui.frame(tree(), None, PointerInput::default(), 0.016)
        .unwrap();
    assert!(ui.frame(tree(), None, wheel, 0.016).unwrap().animating);
    assert_eq!(ui.scroll("list"), [0., 60.], "the target moves at once");
    let mut prev = 0.0;
    for _ in 0..100 {
        let f = ui.frame(tree(), None, Input::default(), 0.016).unwrap();
        let y = -f.scene.surface("top").unwrap().frame.y;
        assert!(y >= prev && y <= 60., "{y} after {prev}");
        prev = y;
        if !f.animating {
            assert_eq!(y, 60., "lands exactly");
            return;
        }
    }
    panic!("never settled");
}

/// The README's shape: a column that scrolls, with no id. The wheel's
/// offset is keyed by the tree path, and the tree applies it by the same.
#[test]
fn an_unnamed_scroller_scrolls() {
    let mut ui = Ui::default();
    let tree = || {
        row![
            col((0..6).map(|_| block(20., 40.))).gap(S).scroll().w(200),
            block(40., 40.),
        ]
        .h(100.)
    };
    let wheel = Input {
        pointer: at(10., 10., false),
        wheel: Vec2::new(0., 30.),
        ..Input::default()
    };
    ui.frame(tree(), None, PointerInput::default(), 0.016)
        .unwrap();
    ui.frame(tree(), None, wheel, 0.016).unwrap();
    assert_eq!(ui.scroll(Id::runtime("/0")), [0., 30.]);
    // Long enough for the glide to land.
    let f = ui.frame(tree(), None, Input::default(), 1.0).unwrap();
    assert_eq!(f.scene.surface("/0/0").unwrap().frame.y, -30.);
}

/// A tip wraps the root and moves every tree path under `/0`; an unnamed
/// scroller keeps its offset through it, and after it.
#[test]
fn an_unnamed_scroller_keeps_its_offset_while_a_tip_is_up() {
    let mut ui = Ui::default();
    let tree = || {
        col![
            col([
                block(20., 40.).tip("why").id("item"),
                block(20., 40.),
                block(20., 40.),
            ])
            .gap(0.)
            .scroll()
            .h(50.),
            block(20., 200.),
        ]
        .gap(0.)
    };
    let win = || Some(Size::new(240., 300.));
    ui.frame(tree(), win(), PointerInput::default(), 0.016)
        .unwrap();
    let wheel = Input {
        pointer: at(120., 20., false),
        wheel: Vec2::new(0., 10.),
        ..Input::default()
    };
    ui.frame(tree(), win(), wheel, 0.016).unwrap();
    ui.frame(tree(), win(), PointerInput::default(), 1.0)
        .unwrap();
    let item_y = |f: &Frame| f.scene.surface("item").unwrap().frame.y;
    let f = ui
        .frame(tree(), win(), at(120., 20., false), 0.016)
        .unwrap();
    assert_eq!(item_y(&f), -10.);
    let f = ui.frame(tree(), win(), at(120., 20., false), 0.6).unwrap();
    assert!(f.tip.is_some(), "the tip is up");
    assert_eq!(item_y(&f), -10., "and the list did not jump");
    let f = ui
        .frame(tree(), win(), PointerInput::default(), 0.016)
        .unwrap();
    assert!(f.tip.is_none());
    assert_eq!(item_y(&f), -10., "nor when it went");
}

/// A widget that reads `Ui::wheel` keeps it: the scroll node around it
/// stays put, and the widget reads the delta the next build.
#[test]
fn a_widget_reading_the_wheel_keeps_it_from_the_scroll_around_it() {
    let mut ui = Ui::default();
    let tree = || {
        col([block(30., 20.).id("dial"), block(30., 100.)])
            .size(30., 40.)
            .scroll()
            .id("outer")
    };
    let input = |y: f64| Input {
        pointer: at(10., y, false),
        wheel: Vec2::new(0., 30.),
        ..Input::default()
    };
    ui.frame(tree(), None, input(10.), 0.016).unwrap();
    // Unclaimed, the column scrolls.
    assert_eq!(ui.scroll("outer"), [0., 30.]);
    let mut ui = Ui::default();
    ui.frame(tree(), None, PointerInput::default(), 0.016)
        .unwrap();
    assert_eq!(ui.wheel("dial"), None);
    ui.frame(tree(), None, input(10.), 0.016).unwrap();
    assert_eq!(ui.scroll("outer"), [0., 0.]);
    assert_eq!(ui.wheel("dial"), Some(Vec2::new(0., 30.)));
    // Off the dial, the column takes it again.
    ui.frame(tree(), None, input(30.), 0.016).unwrap();
    assert_eq!(ui.scroll("outer"), [0., 30.]);
}
