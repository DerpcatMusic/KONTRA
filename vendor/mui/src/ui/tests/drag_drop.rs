use super::*;

/// A source and a target side by side, 50 px each.
fn two() -> El {
    row([
        block(50., 50.).fill(Role::Field).id("src"),
        block(50., 50.).fill(Role::Field).id("dst"),
    ])
}

#[derive(Debug, PartialEq)]
struct Wave(&'static str);

/// Drag `src` out and back to `x`, then let go, attaching a payload once
/// the gesture is a drag. Returns the `Ui` on the frame the drop is reported in.
fn drag_to(x: f64) -> Ui {
    let mut ui = Ui::default();
    for _ in 0..2 {
        ui.frame(two(), None, at(25., 25., false), 0.016).unwrap();
    }
    ui.frame(two(), None, at(25., 25., true), 0.016).unwrap();
    // Out past the drag threshold first, then wherever this drag ends.
    ui.frame(two(), None, at(80., 25., true), 0.016).unwrap();
    ui.frame(two(), None, at(x, 25., true), 0.016).unwrap();
    assert!(ui.get("src").dragged, "the gesture is a drag by now");
    ui.start_drag("src", Wave("saw"));
    assert_eq!(ui.dragging::<Wave>(), Some(&Wave("saw")));
    ui.frame(two(), None, at(x, 25., false), 0.016).unwrap();
    ui
}

/// The payload reaches the target it was dropped on, once. A second ask
/// -- another widget, a later frame -- gets nothing, so a drop can never
/// be applied twice.
#[test]
fn a_dropped_payload_is_delivered_exactly_once() {
    let mut ui = drag_to(80.);
    assert_eq!(ui.dropped_on::<Wave>("dst"), Some(Wave("saw")));
    assert_eq!(ui.dropped_on::<Wave>("dst"), None, "already taken");
    assert!(ui.dragging::<Wave>().is_none(), "the gesture is over");
}

/// A payload nobody took does not survive its drag, and a release that
/// landed somewhere else was never that target's to take.
#[test]
fn a_release_elsewhere_delivers_nothing() {
    // Back onto the source: `dst` sees no drop.
    let mut ui = drag_to(25.);
    assert_eq!(ui.dropped_on::<Wave>("dst"), None);
    assert_eq!(ui.dropped_on::<Wave>("src"), Some(Wave("saw")));

    // And the frame after a drop nobody took, the payload is gone.
    let mut ui = drag_to(80.);
    ui.frame(two(), None, at(80., 25., false), 0.016).unwrap();
    assert_eq!(ui.dropped_on::<Wave>("dst"), None, "one frame only");

    // A target asking for the wrong type leaves it for the right one.
    let mut ui = drag_to(80.);
    assert!(ui.dropped_on::<f64>("dst").is_none(), "not an f64");
    assert_eq!(ui.dropped_on::<Wave>("dst"), Some(Wave("saw")));
}

/// The drawn shape is the hit shape: the ring responds, the hole it
/// leaves does not, and the tag says which draw was hit.
#[test]
fn a_tagged_canvas_responds_in_its_drawn_ring_only() {
    let mut ui = Ui::default();
    let tree = || {
        canvas(|s| {
            let c = Point::new(s.width / 2., s.height / 2.);
            vec![Draw::fill(ring(c, 50., 25.), Role::Primary).tag("band")]
        })
        .size(100., 100.)
        .id("dial")
    };
    // Two frames per move: the hit map a gesture reads is the last built.
    let hover = |ui: &mut Ui, x: f64, y: f64| {
        for _ in 0..2 {
            ui.frame(tree(), None, at(x, y, false), 0.016).unwrap();
        }
    };
    hover(&mut ui, 50., 50.);
    assert!(!ui.get("dial").hovered, "the hole is not the dial");
    assert_eq!(ui.tag("dial"), None);
    hover(&mut ui, 50., 12.);
    assert!(ui.get("dial").hovered, "the band is");
    assert_eq!(ui.tag("dial"), Some("band"));
    hover(&mut ui, 2., 2.);
    assert!(!ui.get("dial").hovered, "and the frame's corner is not");
}

/// The tag a press grabbed survives a drag that leaves the shape.
#[test]
fn the_tag_is_latched_for_the_length_of_the_gesture() {
    let mut ui = Ui::default();
    let tree = || {
        canvas(|s| {
            let c = Point::new(s.width / 2., s.height / 2.);
            vec![Draw::fill(ring(c, 50., 25.), Role::Primary).tag("band")]
        })
        .size(100., 100.)
        .id("dial")
    };
    for _ in 0..2 {
        ui.frame(tree(), None, at(50., 12., false), 0.016).unwrap();
    }
    ui.frame(tree(), None, at(50., 12., true), 0.016).unwrap();
    ui.frame(tree(), None, at(50., 50., true), 0.016).unwrap();
    assert_eq!(ui.tag("dial"), Some("band"), "still the shape it grabbed");
    ui.frame(tree(), None, at(50., 50., false), 0.016).unwrap();
    ui.frame(tree(), None, at(50., 50., false), 0.016).unwrap();
    assert_eq!(ui.tag("dial"), None, "released over the hole");
}
