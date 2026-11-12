//! A Kontakt 8 `native_ui` shape ([`model::Kind::Shape`]): a rounded
//! rectangle or a ring's arc, filled with the widget's background colour.

use kontra_ui_model as model;
use mui::prelude::*;

/// The fill of `shape`, laid out `page_width` page points wide and drawn
/// `w` by `h` view points; nothing without a colour.
pub(super) fn fill(
    shape: model::Shape,
    page_width: f64,
    (w, h): (f64, f64),
    color: Option<model::Rgba>,
) -> Option<Draw> {
    let k = w / page_width.max(f64::EPSILON);
    Some(Draw::fill(
        outline(shape, w, h, k),
        Fill::from(super::color(color?)),
    ))
}

/// A [`Kind::Shape`] `w` by `h` view points, its measures scaled by `k`.
fn outline(shape: model::Shape, w: f64, h: f64, k: f64) -> Path {
    match shape {
        model::Shape::Rect { radius } => {
            let r = (radius * k).min(w / 2.).min(h / 2.).max(0.);
            let p = |x, y| Point::new(x, y);
            Path::default()
                .move_to(p(r, 0.))
                .line_to(p(w - r, 0.))
                .quad_to(p(w, 0.), p(w, r))
                .line_to(p(w, h - r))
                .quad_to(p(w, h), p(w - r, h))
                .line_to(p(r, h))
                .quad_to(p(0., h), p(0., h - r))
                .line_to(p(0., r))
                .quad_to(p(0., 0.), p(r, 0.))
                .close()
        }
        model::Shape::Ring {
            thickness,
            start,
            sweep,
        } => {
            // Outer edge forward, inner edge back: one closed band.
            let center = Point::new(w / 2., h / 2.);
            let outer = w.min(h) / 2.;
            let inner = (outer - thickness * k).max(f64::EPSILON);
            let (a, sweep) = (start.to_radians(), sweep.clamp(-360., 360.).to_radians());
            let arc = |radius: f64, from: f64, sweep: f64| mui::geometry::Arc {
                center,
                radius,
                start_angle: from,
                sweep,
                to: center + Vec2::from_angle(from + sweep) * radius,
            };
            let at = |r: f64, a: f64| center + Vec2::from_angle(a) * r;
            let mut p = Path::default().move_to(at(outer, a));
            p.commands
                .push(mui::geometry::PathCommand::ArcTo(arc(outer, a, sweep)));
            p = p.line_to(at(inner, a + sweep));
            p.commands.push(mui::geometry::PathCommand::ArcTo(arc(
                inner,
                a + sweep,
                -sweep,
            )));
            p.close()
        }
    }
}
