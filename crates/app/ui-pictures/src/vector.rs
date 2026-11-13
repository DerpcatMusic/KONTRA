//! An SVG as retained vector paths ([`Vector`]), painted by the renderer at
//! whatever device scale it is drawn at: never a bitmap.
//!
//! Fills, strokes, solid and gradient paints, group opacity and text (as
//! its outlines) carry over. A file asking for what paths cannot say
//! (filters, masks, clip paths, patterns, embedded images, blend modes)
//! gives `None`, and the caller draws it as pixels instead.

use mui::geometry::kurbo::{self, Affine, BezPath, Cap, Join, Stroke};
use mui::scene::{Vector, VectorCommand};
use peniko::color::{AlphaColor, Srgb};
use peniko::{BlendMode, Brush, Extend, Fill, Gradient};
use resvg::usvg::{self, Node, Paint, Transform};

/// `bytes` as vector paths `None` when it is no SVG or asks for what
/// paths cannot say.
pub(crate) fn vector(bytes: &[u8]) -> Option<Vector> {
    let tree = usvg::Tree::from_data(bytes, &usvg::Options::default()).ok()?;
    let mut out = Vec::new();
    group(tree.root(), &mut out)?;
    let size = tree.size();
    Vector::new(f64::from(size.width()), f64::from(size.height()), out)
}

fn group(g: &usvg::Group, out: &mut Vec<VectorCommand>) -> Option<()> {
    if g.clip_path().is_some()
        || g.mask().is_some()
        || !g.filters().is_empty()
        || g.blend_mode() != usvg::BlendMode::Normal
    {
        return None;
    }
    let alpha = g.opacity().get();
    let layer = alpha < 1.;
    if layer {
        let bounds = g.abs_layer_bounding_box()?;
        out.push(VectorCommand::PushLayer {
            path: bez(&usvg::tiny_skia_path::PathBuilder::from_rect(
                bounds.to_rect(),
            )),
            transform: Affine::IDENTITY,
            blend: BlendMode::default(),
            alpha,
        });
    }
    for node in g.children() {
        match node {
            Node::Group(g) => group(g, out)?,
            Node::Text(t) => group(t.flattened(), out)?,
            Node::Path(p) => path(p, out)?,
            Node::Image(_) => return None,
        }
    }
    if layer {
        out.push(VectorCommand::PopLayer);
    }
    Some(())
}

fn path(p: &usvg::Path, out: &mut Vec<VectorCommand>) -> Option<()> {
    if !p.is_visible() {
        return Some(());
    }
    let shape = bez(p.data());
    let transform = affine(p.abs_transform());
    let fill = |out: &mut Vec<VectorCommand>| -> Option<()> {
        let Some(f) = p.fill() else { return Some(()) };
        let (brush, brush_transform) = brush(f.paint(), f.opacity().get())?;
        out.push(VectorCommand::Fill {
            path: shape.clone(),
            transform,
            brush,
            brush_transform,
            rule: match f.rule() {
                usvg::FillRule::NonZero => Fill::NonZero,
                usvg::FillRule::EvenOdd => Fill::EvenOdd,
            },
        });
        Some(())
    };
    let stroke = |out: &mut Vec<VectorCommand>| -> Option<()> {
        let Some(s) = p.stroke() else { return Some(()) };
        let (brush, brush_transform) = brush(s.paint(), s.opacity().get())?;
        let style = Stroke::new(f64::from(s.width().get()))
            .with_caps(match s.linecap() {
                usvg::LineCap::Butt => Cap::Butt,
                usvg::LineCap::Round => Cap::Round,
                usvg::LineCap::Square => Cap::Square,
            })
            .with_join(match s.linejoin() {
                usvg::LineJoin::Miter | usvg::LineJoin::MiterClip => Join::Miter,
                usvg::LineJoin::Round => Join::Round,
                usvg::LineJoin::Bevel => Join::Bevel,
            })
            .with_miter_limit(f64::from(s.miterlimit().get()))
            .with_dashes(
                f64::from(s.dashoffset()),
                s.dasharray()
                    .unwrap_or_default()
                    .iter()
                    .map(|&d| f64::from(d)),
            );
        out.push(VectorCommand::Stroke {
            path: shape.clone(),
            transform,
            brush,
            brush_transform,
            stroke: style,
        });
        Some(())
    };
    match p.paint_order() {
        usvg::PaintOrder::FillAndStroke => {
            fill(out)?;
            stroke(out)
        }
        usvg::PaintOrder::StrokeAndFill => {
            stroke(out)?;
            fill(out)
        }
    }
}

/// A paint at `opacity`, with the transform of its gradient space.
fn brush(paint: &Paint, opacity: f32) -> Option<(Brush, Affine)> {
    let stops = |base: &usvg::BaseGradient| -> Vec<(f32, AlphaColor<Srgb>)> {
        (base.stops().iter())
            .map(|s| {
                (
                    s.offset().get(),
                    rgba(s.color(), s.opacity().get() * opacity),
                )
            })
            .collect()
    };
    let ramp = |mut g: Gradient, base: &usvg::BaseGradient| {
        g.extend = match base.spread_method() {
            usvg::SpreadMethod::Pad => Extend::Pad,
            usvg::SpreadMethod::Reflect => Extend::Reflect,
            usvg::SpreadMethod::Repeat => Extend::Repeat,
        };
        Brush::Gradient(g.with_stops(stops(base).as_slice()))
    };
    Some(match paint {
        Paint::Color(c) => (Brush::Solid(rgba(*c, opacity)), Affine::IDENTITY),
        Paint::LinearGradient(g) => (
            ramp(Gradient::new_linear((g.x1(), g.y1()), (g.x2(), g.y2())), g),
            affine(g.transform()),
        ),
        Paint::RadialGradient(g) => (
            ramp(
                Gradient::new_two_point_radial(
                    (g.fx(), g.fy()),
                    g.fr().get(),
                    (g.cx(), g.cy()),
                    g.r().get(),
                ),
                g,
            ),
            affine(g.transform()),
        ),
        Paint::Pattern(_) => return None,
    })
}

fn rgba(c: usvg::Color, alpha: f32) -> AlphaColor<Srgb> {
    AlphaColor::from_rgba8(
        c.red,
        c.green,
        c.blue,
        (alpha.clamp(0., 1.) * 255.).round() as u8,
    )
}

fn affine(t: Transform) -> Affine {
    Affine::new([t.sx, t.ky, t.kx, t.sy, t.tx, t.ty].map(f64::from))
}

fn bez(data: &usvg::tiny_skia_path::Path) -> BezPath {
    use usvg::tiny_skia_path::PathSegment as S;
    let p = |p: usvg::tiny_skia_path::Point| kurbo::Point::new(f64::from(p.x), f64::from(p.y));
    let mut out = BezPath::new();
    for segment in data.segments() {
        match segment {
            S::MoveTo(a) => out.move_to(p(a)),
            S::LineTo(a) => out.line_to(p(a)),
            S::QuadTo(a, b) => out.quad_to(p(a), p(b)),
            S::CubicTo(a, b, c) => out.curve_to(p(a), p(b), p(c)),
            S::Close => out.close_path(),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_svg_becomes_paths_and_what_paths_cannot_say_does_not() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="2">
            <defs><linearGradient id="g"><stop offset="0" stop-color="#f00"/><stop offset="1" stop-color="#00f"/></linearGradient></defs>
            <g opacity="0.5"><rect width="4" height="2" fill="url(#g)" stroke="#fff"/></g></svg>"##;
        let v = vector(svg).map(|v| (v.width, v.height));
        assert_eq!(v, Some((4., 2.)));
        let blurred = br##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="2">
            <filter id="b"><feGaussianBlur stdDeviation="1"/></filter>
            <rect width="4" height="2" filter="url(#b)"/></svg>"##;
        assert!(vector(blurred).is_none());
        assert!(vector(b"\x89PNG").is_none());
    }
}
