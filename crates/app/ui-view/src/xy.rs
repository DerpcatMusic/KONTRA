//! An XY pad (`ui_xy`) as Kontakt 8 draws one (oracle probes, 2026-11-14,
//! scratch/lanes/ui/probe/xy.ksp and xy2.ksp at 100 %).
//!
//! The stock pad: a #3F3F3F field in a 1 px #2C2C2C rim and a 1 px #393939
//! bevel, and a 1 px #A6A6A6 frame 7 px in. A pad's own `PICTURE` replaces
//! it, drawn at its own size from the corner.
//!
//! Each x/y pair is a cursor, y up. A stock cursor is a 13 px square whose
//! centre travels between the frame's lines (with or without a picture):
//! the active one filled #787878 in a white edge, the others the edge
//! alone. A cursor picture (`CURSOR_PICTURE`) travels the whole pad less its
//! own size, as a slider's handle does, showing frame 1 when active and 0
//! when not.

use std::sync::Arc;

use kontra_ui_model as model;
use mui::prelude::*;

/// The frame's inset, in points: a stock cursor's centre travels from it
/// to its far line (one in from the opposite inset).
pub const FRAME: f64 = 7.;
/// The stock cursor's side, in points.
pub const CURSOR: f64 = 13.;

/// The active cursor: `ACTIVE_INDEX` names its x cell (even); the first
/// when unset.
#[must_use]
pub fn active(w: &model::Widget) -> u32 {
    w.active_index
        .and_then(|i| u32::try_from(i / 2).ok())
        .unwrap_or(0)
}

/// The size of cursor `i`'s picture in points, if it has one.
#[must_use]
pub fn art_size(tree: &model::Interface, w: &model::Widget, i: u32) -> Option<(f64, f64)> {
    let u = w
        .images
        .iter()
        .find(|u| u.role == model::Role::Handle && u.part == i)?;
    match tree.assets.get(u.asset.0)?.kind {
        model::AssetKind::Image(m) => m.size.map(|s| (s.width, s.height)),
        _ => None,
    }
}

/// How far a cursor of size `art` (stock when `None`) travels across and
/// down a pad of `size`, and where its rect's corner is at value 0 / top.
fn travel((width, height): (f64, f64), art: Option<(f64, f64)>) -> ((f64, f64), (f64, f64)) {
    match art {
        Some((aw, ah)) => (((width - aw).max(0.), (height - ah).max(0.)), (0., 0.)),
        None => (
            (
                (width - 2. * FRAME - 1.).max(0.),
                (height - 2. * FRAME - 1.).max(0.),
            ),
            (FRAME - 6., FRAME - 6.),
        ),
    }
}

/// Where cursor `i`'s rect's corner sits for its values, unrounded: the
/// inverse of [`value`].
#[must_use]
pub fn corner(
    w: &model::Widget,
    i: u32,
    size: (f64, f64),
    art: Option<(f64, f64)>,
) -> Option<(f64, f64)> {
    let (x, y) = (w.cell(2 * i)?, w.cell(2 * i + 1)?);
    let ((sx, sy), (ox, oy)) = travel(size, art);
    Some((ox + x.clamp(0., 1.) * sx, oy + (1. - y.clamp(0., 1.)) * sy))
}

/// Cursor `i`'s rect (x, y, width, height) in points from the pad's corner,
/// on whole points as it is drawn.
#[must_use]
pub fn rect(
    w: &model::Widget,
    i: u32,
    size: (f64, f64),
    art: Option<(f64, f64)>,
) -> Option<[f64; 4]> {
    let (x, y) = corner(w, i, size, art)?;
    let (cw, ch) = art.unwrap_or((CURSOR, CURSOR));
    Some([x.round(), y.round(), cw, ch])
}

/// The value (x, y) of a cursor whose rect's corner is at `(px, py)`.
#[must_use]
pub fn value((px, py): (f64, f64), size: (f64, f64), art: Option<(f64, f64)>) -> (f64, f64) {
    let ((sx, sy), (ox, oy)) = travel(size, art);
    let along = |p: f64, o: f64, s: f64| {
        if s > 0. {
            ((p - o) / s).clamp(0., 1.)
        } else {
            0.
        }
    };
    (along(px, ox, sx), 1. - along(py, oy, sy))
}

/// A cursor's picture: its inactive and active frames.
pub(super) type Art = (Arc<Image>, Arc<Image>);
/// A pictured cursor: its frames once decoded, and its size in points.
pub(super) type Cursor = (Option<Art>, (f64, f64));

/// The pad `w` drawn `width` by `height` points at scale `s`: the stock
/// field when `field`, then its cursors (a picture and its size from
/// `art`, by cursor, else the stock square).
pub(super) fn draw(
    w: &model::Widget,
    (width, height, s): (f64, f64, f64),
    field: bool,
    art: &dyn Fn(u32) -> Option<Cursor>,
) -> El {
    let model::Kind::Xy { cursors, .. } = w.kind else {
        return stack(Vec::<El>::new());
    };
    let px = |v: f64| (v * s).round();
    let fill = |x: f64, y: f64, w: f64, h: f64, c: u32| {
        block(px(x + w) - px(x), px(y + h) - px(y))
            .radius(0.)
            .fill(super::color(model::Rgba::rgb(c)))
            .at(px(x), px(y))
    };
    let mut parts = Vec::new();
    if field {
        let f = FRAME;
        parts.push(fill(0., 0., width, height, 0x2C_2C2C));
        parts.push(fill(1., 1., width - 2., height - 2., 0x39_3939));
        parts.push(fill(2., 2., width - 4., height - 4., 0x3F_3F3F));
        parts.push(fill(f, f, width - 2. * f, height - 2. * f, 0xA6_A6A6));
        parts.push(fill(
            f + 1.,
            f + 1.,
            width - 2. * f - 2.,
            height - 2. * f - 2.,
            0x3F_3F3F,
        ));
    }
    // `HIDE_PART_CURSOR` (32) set in `on init` leaves them drawn (probe/xy3.ksp).
    let on = active(w);
    for i in 0..cursors {
        let pictured = art(i);
        let Some([x, y, cw, ch]) = rect(
            w,
            i,
            (width, height),
            pictured.as_ref().map(|(_, size)| *size),
        ) else {
            continue;
        };
        match pictured {
            Some((Some((off, lit)), _)) => parts.push(
                block(px(x + cw) - px(x), px(y + ch) - px(y))
                    .radius(0.)
                    .fill(Fill::Image(if i == on { lit } else { off }, Fit::Fill))
                    .at(px(x), px(y)),
            ),
            // Its picture still decoding (or failed): nothing, where the
            // pointer still finds it.
            Some((None, _)) => {}
            None if i == on => {
                parts.push(fill(x, y, CURSOR, CURSOR, 0xFF_FFFF));
                parts.push(fill(x + 1., y + 1., CURSOR - 2., CURSOR - 2., 0x78_7878));
            }
            None => {
                // Hollow: the edge alone, the pad showing through.
                let far = CURSOR - 1.;
                for (ex, ey, ew, eh) in [
                    (x, y, CURSOR, 1.),
                    (x, y + far, CURSOR, 1.),
                    (x, y, 1., CURSOR),
                    (x + far, y, 1., CURSOR),
                ] {
                    parts.push(fill(ex, ey, ew, eh, 0xFF_FFFF));
                }
            }
        }
    }
    stack(parts).w(width * s).h(height * s)
}
