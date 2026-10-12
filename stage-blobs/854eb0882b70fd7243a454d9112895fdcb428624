use super::*;
use crate::widgets;
use mui_geometry::Point;
use mui_input::Mods;
use mui_scene::prelude::*;

fn at(x: f64, y: f64, down: bool) -> PointerInput {
    PointerInput {
        pos: Some(Point::new(x, y)),
        buttons: Buttons::default().set(Button::Primary, down),
        ..PointerInput::default()
    }
}
fn key(k: Key) -> Input {
    Input {
        keys: vec![KeyPress {
            key: k,
            mods: Mods::default(),
        }],
        ..Input::default()
    }
}
fn paint_color(frame: &Frame<'_>, id: &str) -> Color {
    frame
        .scene
        .paint
        .iter()
        .find(|p| &*p.key == id)
        .unwrap_or_else(|| panic!("missing paint for {id}"))
        .paint
        .solid()
}
fn role_color(role: Role) -> Color {
    let pal = Theme::DEFAULT.palette;
    match Fill::from(role).paint(&pal, pal.background()) {
        Some(Paint::Solid(c)) => c,
        _ => panic!("role has no solid paint"),
    }
}

/// A ring: the outer circle one way round, the inner the other, so the
/// hole is outside under the non-zero rule the renderer fills by.
fn ring(c: Point, outer: f64, inner: f64) -> mui_geometry::Path {
    let arc = move |r: f64, rev: bool| {
        (0..64).map(move |i| {
            let k = if rev { 64 - i } else { i };
            let a = std::f64::consts::TAU * f64::from(k) / 64.0;
            Point::new(c.x + r * a.cos(), c.y + r * a.sin())
        })
    };
    let mut p = mui_geometry::Path::polyline(arc(outer, false), true);
    for (i, q) in arc(inner, true).enumerate() {
        p = if i == 0 { p.move_to(q) } else { p.line_to(q) };
    }
    p.close()
}

fn solid(f: &Frame) -> mui_scene::Paint {
    f.scene.paint[0].paint.clone()
}

fn to_paint(r: Role) -> mui_scene::Paint {
    let mut ui = Ui::default();
    solid(
        &ui.frame(block(40., 40.).fill(r), None, Input::default(), 0.016)
            .unwrap(),
    )
}

mod drag_drop;
mod input;
mod memo;
mod motion;
mod pointer;
mod scroll;
mod snapshots;
