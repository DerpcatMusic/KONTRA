//! A page painter's paths ([`kontra_ui_model::Kind::Canvas`]) as draws.

use kontra_ui_model::{Paint, Rgba, Seg};
use mui::prelude::{Color, Draw, Fill, Path, Point};

/// `paints`, in page points, as draws in view points `k` times larger:
/// each path filled, then stroked.
#[must_use]
pub fn canvas(paints: &[Paint], k: f64) -> Vec<Draw> {
    let mut out = Vec::new();
    for p in paints {
        let path = path(&p.path, k);
        if let Some(c) = p.fill {
            out.push(Draw::fill(path.clone(), Fill::from(color(c))));
        }
        if let Some((c, width)) = p.stroke {
            out.push(Draw::stroke(path, Fill::from(color(c)), width * k));
        }
    }
    out
}

fn path(segs: &[Seg], k: f64) -> Path {
    let at = |x: f64, y: f64| Point::new(x * k, y * k);
    segs.iter().fold(Path::default(), |p, s| match *s {
        Seg::Move([x, y]) => p.move_to(at(x, y)),
        Seg::Line([x, y]) => p.line_to(at(x, y)),
        Seg::Cubic([ax, ay, bx, by, x, y]) => p.cubic_to(at(ax, ay), at(bx, by), at(x, y)),
        Seg::Close => p.close(),
    })
}

fn color(c: Rgba) -> Color {
    let ch = |v: u8| f32::from(v) / 255.;
    Color::srgba(ch(c.r), ch(c.g), ch(c.b), ch(c.a))
}
