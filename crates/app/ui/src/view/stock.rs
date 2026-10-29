//! A script control with no picture of its own draws the host's stock
//! face, as Kontakt does: a dark field under its words, a dial with its
//! name over it and its value under it, a fader, the bars of a table.
//! Ported from v1 `0cb7a8a0:src/ui/perf_view.rs` (`face`).

use kontra_ui_model::{self as model, Kind};
use mui::prelude::*;

/// Kontakt's stock graphite: the field, its edge, a lit switch, the value.
const FIELD: f32 = 0.16;
const EDGE: f32 = 0.32;
const ON: f32 = 0.36;
const VALUE: f32 = 0.85;

fn grey(l: f32) -> Fill {
    Fill::from(Color::srgb(l, l, l))
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> Path {
    Path::polyline(
        [(x, y), (x + w, y), (x + w, y + h), (x, y + h)].map(|(x, y)| Point::new(x, y)),
        true,
    )
}

/// The arc of a circle at `c` of radius `r` from `from` to `to` of a
/// Kontakt dial's 270° sweep.
fn sweep(c: Point, r: f64, from: f64, to: f64) -> Path {
    let steps = 32;
    Path::polyline(
        (0..=steps).map(|n| {
            let a =
                (135. + 270. * (from + (to - from) * f64::from(n) / f64::from(steps))).to_radians();
            Point::new(c.x + r * a.cos(), c.y + r * a.sin())
        }),
        false,
    )
}

/// Whether `w` draws a stock face: a script control without a picture
/// whose background the script has not hidden.
pub(super) fn shows(w: &model::Widget, source: model::Source) -> bool {
    // ponytail: KSP's stock look only; UVI's own once a Lua library needs it.
    matches!(source, model::Source::Ksp { .. })
        && !w.hide.background
        && matches!(
            w.kind,
            Kind::Knob { .. }
                | Kind::Slider { .. }
                | Kind::Button { .. }
                | Kind::Switch
                | Kind::Menu { .. }
                | Kind::ValueEdit { .. }
                | Kind::TextEdit
                | Kind::Table { .. }
        )
}

/// `w`'s stock face at `t` (0..1 of its range), `width` by `height`
/// view points; a knob leaves `text` points above and below its dial.
pub(super) fn face(w: &model::Widget, t: f64, (width, height): (f64, f64), text: f64) -> El {
    let on = t >= 1.;
    let kind = w.kind.clone();
    let bars: Vec<f64> = match (&w.kind, &w.value) {
        (Kind::Table { range, .. }, Some(model::Value::Integers(v))) => {
            let span = (range.max - range.min).max(f64::EPSILON);
            v.iter()
                .map(|&n| ((f64::from(n) - range.min) / span).clamp(0., 1.))
                .collect()
        }
        _ => Vec::new(),
    };
    canvas(move |z| {
        let (w, h) = (z.width, z.height);
        let field = |d: &mut Vec<Draw>, l: f32| {
            d.push(Draw::fill(rect(0., 0., w, h), grey(l)));
            d.push(Draw::stroke(rect(0.5, 0.5, w - 1., h - 1.), grey(EDGE), 1.));
        };
        let mut d = Vec::new();
        match kind {
            Kind::Knob { .. } => {
                let c = Point::new(w / 2., h / 2.);
                let r = ((h - 2. * text).min(w) / 2. - 2.).max(2.);
                d.push(Draw::stroke(sweep(c, r, 0., 1.), grey(EDGE), 2.));
                d.push(Draw::stroke(sweep(c, r, 0., t), grey(VALUE), 2.));
                let a = (135. + 270. * t).to_radians();
                d.push(Draw::stroke(
                    Path::polyline(
                        [
                            Point::new(c.x + r * 0.2 * a.cos(), c.y + r * 0.2 * a.sin()),
                            Point::new(c.x + r * 0.75 * a.cos(), c.y + r * 0.75 * a.sin()),
                        ],
                        false,
                    ),
                    grey(VALUE),
                    1.5,
                ));
            }
            Kind::Slider { .. } => {
                let vertical = h > w;
                let (len, across) = if vertical { (h, w) } else { (w, h) };
                let thumb = (across * 0.6).clamp(4., 12.).min(len / 3.);
                let at = thumb / 2. + t * (len - thumb);
                let mid = (across / 2.).round();
                let place = |along: f64, off: f64, l: f64, th: f64| {
                    if vertical {
                        rect(off, len - along - l, th, l)
                    } else {
                        rect(along, off, l, th)
                    }
                };
                d.push(Draw::fill(
                    place(thumb / 2., mid - 1., len - thumb, 2.),
                    grey(EDGE),
                ));
                d.push(Draw::fill(
                    place(thumb / 2., mid - 1., at - thumb / 2., 2.),
                    grey(VALUE),
                ));
                let th = (across - 2.).min(thumb * 1.4).max(2.);
                d.push(Draw::fill(
                    place(at - thumb / 2., mid - th / 2., thumb, th),
                    grey(VALUE),
                ));
            }
            Kind::Button { .. } | Kind::Switch => field(&mut d, if on { ON } else { FIELD }),
            Kind::Table { .. } => {
                field(&mut d, FIELD);
                let bw = w / bars.len().max(1) as f64;
                for (n, v) in bars.iter().enumerate() {
                    let bh = v * (h - 2.);
                    d.push(Draw::fill(
                        rect(n as f64 * bw + 1., h - 1. - bh, (bw - 1.).max(1.), bh),
                        grey(VALUE),
                    ));
                }
            }
            _ => field(&mut d, FIELD),
        }
        d
    })
    .w(width)
    .h(height)
}
