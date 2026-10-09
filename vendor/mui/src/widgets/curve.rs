//! The envelope editor: one canvas whose knots and tension handles are its
//! own hit shapes, over the cubic model `mui-motion` already ships.
//!
//! The drawn shape *is* the grab shape -- a knot is a 5 px disc and it
//! responds in that disc -- so nothing here keeps a second radius in step
//! with the first.
use mui_input::{Axis, FINE_DRAG};
use mui_scene::Size;
use mui_scene::curve::{Curve, CurvePoint, Handle};
use mui_scene::prelude::*;

use crate::Ui;
use crate::widgets::Response;

/// The knot radius, and the inset the plot keeps on every side so an end
/// knot sits inside the frame instead of half outside it.
const KNOT: f64 = 5.0;
/// The tension handle radius: smaller than a knot, so the two read apart.
const TENSION: f64 = 3.5;

/// What a drag on a [`curve`] moved this frame. The curve itself has already
/// been edited -- this says which part, for a caller keeping history or
/// showing a readout.
///
/// ```
/// use mui::prelude::*;
/// use mui::scene::curve::Handle;
/// assert_eq!(CurveEdit::Point(2), CurveEdit::Point(2));
/// assert_ne!(CurveEdit::Point(0), CurveEdit::Tension(0, Handle::Outgoing));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurveEdit {
    /// Knot `i` moved, clamped between its neighbours; the ends keep their
    /// phase, so only their value travels.
    Point(usize),
    /// One tension handle of segment `j` bent, clamped inside the segment.
    Tension(usize, Handle),
}

/// The tag grammar the canvas names its shapes with: `n{i}` is knot `i`,
/// `out{j}` and `in{j}` are segment `j`'s two tension handles. `Ui::tag(id)`
/// hands one of these back, so a caller can say what is under the pointer.
fn target(tag: &str) -> Option<CurveEdit> {
    if let Some(i) = tag.strip_prefix('n') {
        return Some(CurveEdit::Point(i.parse().ok()?));
    }
    if let Some(j) = tag.strip_prefix("out") {
        return Some(CurveEdit::Tension(j.parse().ok()?, Handle::Outgoing));
    }
    let j = tag.strip_prefix("in")?;
    Some(CurveEdit::Tension(j.parse().ok()?, Handle::Incoming))
}

/// The pixels one full unit of phase and of value travel, inside the inset.
fn span(size: Size) -> (f64, f64) {
    // A frame too small for the inset would divide by zero and put every
    // knot on one point; one pixel of span keeps the plot degenerate but
    // finite, and the drag proportional.
    (
        (size.width - 2.0 * KNOT).max(1.0),
        (size.height - 2.0 * KNOT).max(1.0),
    )
}
/// A normalized point in the canvas's own pixels: phase runs right, value up.
fn at(size: Size, p: CurvePoint) -> Point {
    let (w, h) = span(size);
    Point::new(
        KNOT + f64::from(p.phase) * w,
        KNOT + (1.0 - f64::from(p.value)) * h,
    )
}
/// A disc, drawn and hit as the same polygon.
fn dot(c: Point, r: f64) -> Path {
    Path::polyline(
        (0..20).map(|i| {
            let a = std::f64::consts::TAU * f64::from(i) / 20.0;
            Point::new(c.x + r * a.cos(), c.y + r * a.sin())
        }),
        true,
    )
}

/// Apply this frame's drag to whatever the press grabbed. The tag is latched
/// by the runtime for the length of the gesture, so a knot dragged past its
/// neighbour is still the knot that was grabbed.
fn dragged(ui: &Ui, id: &str, c: &mut Curve) -> Option<CurveEdit> {
    let r = ui.get(id);
    let what = target(ui.tag(id)?)?;
    if !r.dragged {
        return None;
    }
    let (w, h) = span(ui.scene()?.surface(id)?.frame.size);
    let mut d = r.drag_fine(FINE_DRAG);
    // Alt *at the press* locks the drag to the axis it has travelled
    // furthest along: letting go of Alt halfway must not change the gesture.
    if r.press_mods.alt {
        match r.drag_axis() {
            Some(Axis::X) => d.y = 0.0,
            Some(Axis::Y) => d.x = 0.0,
            None => {}
        }
    }
    if d.x == 0.0 && d.y == 0.0 {
        return None;
    }
    let (dp, dv) = ((d.x / w) as f32, (-d.y / h) as f32);
    match what {
        CurveEdit::Point(i) => {
            let p = *c.points().get(i)?;
            c.move_point(i, p.phase + dp, p.value + dv);
        }
        CurveEdit::Tension(j, hand) => {
            let hs = *c.handles().get(j)?;
            let p = match hand {
                Handle::Outgoing => hs.outgoing,
                Handle::Incoming => hs.incoming,
            };
            c.move_handle(
                j,
                hand,
                CurvePoint {
                    phase: p.phase + dp,
                    value: p.value + dv,
                },
            );
        }
    }
    Some(what)
}

/// An envelope or LFO shape: the curve's own cubics as one stroked path,
/// a draggable knot per point and two tension handles per segment.
///
/// The drag is applied to `c` before the tree is built, and the returned
/// [`CurveEdit`] says what moved -- `None` when the pointer is elsewhere or
/// resting. Shift is the fine drag every parameter here honours, and Alt
/// held at the press locks the gesture to one axis. Knots clamp between
/// their neighbours and handles inside their segment, which is
/// [`Curve::move_point`] and [`Curve::move_handle`] doing it, not this file.
///
/// The painted spine is the model's own control points, fed straight to
/// [`Path::cubic_to`], so what is drawn and what
/// [`Curve::evaluate`](mui_scene::curve::Curve::evaluate) samples for the DSP
/// side are one curve.
///
/// Size it like any canvas; it stretches if you do not.
///
/// ```
/// use mui::prelude::*;
/// use mui::scene::curve::Curve;
/// let mut ui = Ui::default();
/// let mut env = Curve::default();
/// let plot = curve(&mut ui, "env", &mut env);
/// assert_eq!(plot.changed, None, "nothing is dragging");
/// let _ = plot.size(240, 120);
/// ```
pub fn curve(ui: &mut Ui, id: impl Into<Id>, c: &mut Curve) -> Response<Option<CurveEdit>> {
    let id: Id = id.into();
    let id = id.as_str();
    let edit = dragged(ui, id, c);
    let (points, handles) = (c.points().to_vec(), c.handles().to_vec());
    let lit = ui.tag(id).map(str::to_owned);
    let el = canvas(move |size| {
        let hot = |t: &str| lit.as_deref() == Some(t);
        // `Curve` holds 2..=64 points and one handle pair per segment as an
        // invariant of every constructor and edit, so these are not fallible
        // lookups dressed as indexing.
        let mut spine = Path::default().move_to(at(size, points[0]));
        for (j, h) in handles.iter().enumerate() {
            spine = spine.cubic_to(
                at(size, h.outgoing),
                at(size, h.incoming),
                at(size, points[j + 1]),
            );
        }
        // The spine is a stroke and carries no tag: a press on the line is
        // not a grab, which is the whole reason the knots are discs.
        let mut draws = vec![Draw::stroke(spine, Role::Primary, 2.0)];
        for (j, h) in handles.iter().enumerate() {
            for (tag, from, hp, r) in [
                (format!("out{j}"), points[j], h.outgoing, TENSION),
                (format!("in{j}"), points[j + 1], h.incoming, TENSION),
            ] {
                let (a, b) = (at(size, from), at(size, hp));
                draws.push(Draw::stroke(Path::polyline([a, b], false), Role::Dim, 1.0));
                let ink = if hot(&tag) { Role::Primary } else { Role::Dim };
                draws.push(Draw::fill(dot(b, r), ink).tag(tag));
            }
        }
        // Knots last, so a knot sitting on a handle wins the hit: the hit
        // map answers with the shape pushed nearest the top.
        for (i, p) in points.iter().enumerate() {
            let tag = format!("n{i}");
            let ink = if hot(&tag) { Role::Primary } else { Role::Ink };
            draws.push(Draw::fill(dot(at(size, *p), KNOT), ink).tag(tag));
        }
        draws
    })
    .cursor(Cursor::Grab)
    .id(id);
    Response { el, changed: edit }
}
