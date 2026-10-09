//! Wheel ownership is shared by chrome menus and authored dropdowns.
use super::*;

fn tree() -> El {
    stack![
        col![block(300., 900.)].size(300., 200.).scroll().id("background"),
        stack![col((0..20).map(|n| caption(format!("Choice {n}")).h(24.).shrink(0)).collect::<Vec<_>>())
            .gap(0).size(140., 80.).scroll().id("popup-list")]
            .size(140., 80.).captures_wheel().id("popup-boundary").at(80., 30.)
    ].size(300., 200.)
}

#[test]
fn popup_wheel_is_visible_only_inside_its_capture_boundary() {
    let mut ui = theme::ui();
    for _ in 0..3 { ui.frame(tree(), None, Input::default(), 1./60.).unwrap(); }
    ui.frame(tree(), None, Input { pointer: PointerInput { pos: Some(Point::new(110., 50.)), ..Default::default() },
        wheel: Vec2::new(0., 60.), ..Default::default() }, 1./60.).unwrap();
    assert_eq!(ui.get("popup-list").wheel, Vec2::new(0., 60.));
    assert_eq!(ui.wheel("background"), None, "a custom scroller behind a popup must not see its wheel");
    assert_eq!(ui.scroll("background"), [0., 0.]);
    assert!(ui.scroll("popup-list")[1] > 0.);
}

#[test]
fn an_exhausted_popup_never_hands_wheel_to_the_background() {
    let mut ui = theme::ui();
    for _ in 0..3 { ui.frame(tree(), None, Input::default(), 1./60.).unwrap(); }
    ui.set_scroll("popup-list", [0., 10000.]);
    for _ in 0..20 { ui.frame(tree(), None, Input::default(), 1./60.).unwrap(); }
    let end = ui.scroll("popup-list");
    ui.frame(tree(), None, Input { pointer: PointerInput { pos: Some(Point::new(110., 50.)), ..Default::default() },
        wheel: Vec2::new(0., 60.), ..Default::default() }, 1./60.).unwrap();
    assert_eq!(ui.scroll("popup-list"), end);
    assert_eq!(ui.get("background").wheel, Vec2::ZERO);
    assert_eq!(ui.scroll("background"), [0., 0.]);
}
