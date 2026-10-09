//! The bin display: an additive spectrum as bars you can paint, in the
//! spirit of Razor's bin view and Prism's partial list.
//!
//! One canvas, one hit shape. The 1024 bars are *not* 1024 tagged draws --
//! the pointer's x is mapped to a bin arithmetically, which is why a drag
//! across the whole display costs one hit test rather than a thousand.
use mui_input::{Button, FINE_DRAG, Key};
use mui_scene::Size;
use mui_scene::prelude::*;

use crate::Ui;
use crate::widgets::Response;

/// One arrow press, as a fraction of the full level range. Shift takes
/// [`FINE_DRAG`] of it, the same ratio every other parameter here uses.
const NUDGE: f64 = 0.01;

/// How a bin index maps across the plot's width.
///
/// ```
/// use mui::prelude::*;
/// let mut saw: Vec<f32> = (1..=8).map(|n| 1.0 / n as f32).collect();
/// let log = Bins { authored: &mut saw, x: BinAxis::Log, ..Bins::default() };
/// // Partial 1 sits at the left edge, partial 8 at the right.
/// assert_eq!((log.x_of(0), log.x_of(7)), (0.0, 1.0));
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BinAxis {
    /// Every bin the same width: the bin *list*, which is what an operator
    /// chain edits.
    #[default]
    Linear,
    /// Bin `i` at `log2(i + 1)`: octaves are evenly spaced, which is what a
    /// spectrum sounds like.
    Log,
}

/// What a [`bins`] display edits and shows: the spectrum the patch
/// authored and the selection (both edited in place), the levels the
/// operator chain actually produced, and where the bins sit.
///
/// Every field but `authored` has a sensible nothing, so a plain harmonic
/// display is one struct-update from [`Bins::default`].
///
/// ```
/// use mui::prelude::*;
/// let mut saw: Vec<f32> = (1..=64).map(|n| 1.0 / n as f32).collect();
/// let live: Vec<f32> = saw.iter().map(|v| v * 0.5).collect();
/// let b = Bins { authored: &mut saw, live: Some(&live), selected: Some(3), ..Bins::default() };
/// assert_eq!(b.authored.len(), 64);
/// ```
#[derive(Debug, Default)]
pub struct Bins<'a> {
    /// The level the patch stores per partial, 0..1, painted in place. Its
    /// length is the bin count; everything else here is measured against it.
    pub authored: &'a mut [f32],
    /// The level the engine is producing right now -- `Plan::magnitudes` in
    /// A synth's additive oscillator -- drawn as a cap line over the bars, so
    /// the operator chain's effect is visible over what was authored.
    /// Ignored unless it is the same length as `authored`.
    pub live: Option<&'a [f32]>,
    /// Whether bin index runs linearly or by octave across the width.
    pub x: BinAxis,
    /// A normalized x per bin, for partials that are not harmonics. Ignored
    /// unless it is the same length as `authored`; `None` spaces them by
    /// [`Bins::x`].
    pub positions: Option<&'a [f32]>,
    /// The bin the arrow keys move and nudge, drawn in the ink colour. The
    /// arrows and a paint move it.
    pub selected: Option<usize>,
}

impl Bins<'_> {
    /// Where bin `i`'s centre sits across the plot, 0 at the left edge and 1
    /// at the right. The caller's own axis labels are this function.
    ///
    /// ```
    /// use mui::prelude::*;
    /// let mut saw: Vec<f32> = (1..=9).map(|n| 1.0 / n as f32).collect();
    /// let log = Bins { authored: &mut saw, x: BinAxis::Log, ..Bins::default() };
    /// // One octave is one step, wherever it falls: 1 -> 2 -> 4 -> 8.
    /// let step = log.x_of(1) - log.x_of(0);
    /// assert!((log.x_of(3) - log.x_of(1) - step).abs() < 1e-12);
    /// assert!((log.x_of(7) - log.x_of(3) - step).abs() < 1e-12);
    /// ```
    #[must_use]
    pub fn x_of(&self, i: usize) -> f64 {
        x_of(i, self.authored.len(), self.x, self.positions)
    }
}

/// What a gesture on a [`bins`] display did this frame. It has already been
/// applied to [`Bins::authored`] and [`Bins::selected`] -- this says which
/// part, for a caller keeping history or mirroring the levels elsewhere.
///
/// ```
/// use mui::prelude::*;
/// assert_eq!(BinEdit::Reset(7), BinEdit::Reset(7));
/// assert_ne!(BinEdit::Select(0), BinEdit::Reset(0));
/// ```
#[derive(Clone, Debug, PartialEq)]
pub enum BinEdit {
    /// These bins were set to these levels. A drag paints every bin the
    /// pointer crossed since the last frame, interpolated, so a fast sweep
    /// leaves no gaps. The last one painted becomes the selection.
    Paint(Vec<(usize, f32)>),
    /// A secondary click put this bin back to 0.
    Reset(usize),
    /// The arrow keys moved the selection here.
    Select(usize),
}

/// Bin `i`'s centre, normalized. Free rather than a method so the paint
/// closure -- which owns copies, not the borrowed [`Bins`] -- can call it.
fn x_of(i: usize, n: usize, axis: BinAxis, pos: Option<&[f32]>) -> f64 {
    if let Some(p) = pos.filter(|p| p.len() == n).and_then(|p| p.get(i)) {
        return f64::from(*p).clamp(0.0, 1.0);
    }
    if n < 2 {
        return 0.5;
    }
    match axis {
        BinAxis::Linear => (i as f64 + 0.5) / n as f64,
        BinAxis::Log => ((i + 1) as f64).log2() / (n as f64).log2(),
    }
}

/// The bin whose centre is nearest `t`, a normalized x.
///
/// ponytail: a linear scan, so one inverse covers all three axes including
/// arbitrary `positions`. 1024 comparisons twice a frame is noise; a binary
/// search over the two monotone axes is the upgrade if a bin count ever
/// reaches `MAX_CAPACITY`.
fn bin_at(t: f64, n: usize, axis: BinAxis, pos: Option<&[f32]>) -> usize {
    (0..n)
        .min_by(|&i, &j| {
            (x_of(i, n, axis, pos) - t)
                .abs()
                .total_cmp(&(x_of(j, n, axis, pos) - t).abs())
        })
        .unwrap_or(0)
}

/// A level from a y inside the plot: the top of the box is 1.
fn level_at(y: f64, h: f64) -> f64 {
    (1.0 - y / h).clamp(0.0, 1.0)
}

/// The plot's size, from the frame it was given last time. The one place
/// this widget reads a resolved frame back, because a bin index is a
/// fraction of a width and nothing else knows the width.
fn plot(ui: &Ui, id: &str) -> Option<Size> {
    let s = ui.scene()?.surface(id)?.frame.size;
    Some(Size::new(s.width.max(1.0), s.height.max(1.0)))
}

/// The pointer half of the gesture: paint while held, reset on a secondary
/// click.
fn pointed(ui: &Ui, id: &str, b: &Bins, n: usize) -> Option<BinEdit> {
    let r = ui.get(id);
    let p = ui.local(id)?;
    let size = plot(ui, id)?;
    let bin = |x: f64| bin_at(x / size.width, n, b.x, b.positions);
    if r.clicked_with(Button::Secondary) {
        return Some(BinEdit::Reset(bin(p.x)));
    }
    if !r.held || r.button != Some(Button::Primary) {
        return None;
    }
    // Shift scales the move away from the level the press landed on, not
    // from the top of the box: a fine edit refines where you started, and a
    // press that has not moved is still absolute.
    let base = level_at((p - r.drag_total).y, size.height);
    let level = |y: f64| {
        let v = level_at(y, size.height);
        if r.mods.shift {
            base + (v - base) * FINE_DRAG
        } else {
            v
        }
    };
    let prev = p - r.drag_delta;
    let (i0, i1) = (bin(prev.x), bin(p.x));
    let (v0, v1) = (level(prev.y), level(p.y));
    let span = i1.abs_diff(i0);
    Some(BinEdit::Paint(
        (0..=span)
            .map(|k| {
                let t = if span == 0 {
                    1.0
                } else {
                    k as f64 / span as f64
                };
                let i = if i1 >= i0 { i0 + k } else { i0 - k };
                (i, (v0 + (v1 - v0) * t) as f32)
            })
            .collect(),
    ))
}

/// The keyboard half: only while this widget holds the focus, because
/// `Ui::keys` is focus-gated.
fn typed(ui: &Ui, id: &str, b: &Bins, n: usize) -> Option<BinEdit> {
    let sel = b.selected.unwrap_or(0).min(n - 1);
    ui.keys(id).iter().find_map(|k| match k.key {
        Key::Left => Some(BinEdit::Select(sel.saturating_sub(1))),
        Key::Right => Some(BinEdit::Select((sel + 1).min(n - 1))),
        Key::Home => Some(BinEdit::Select(0)),
        Key::End => Some(BinEdit::Select(n - 1)),
        Key::Up | Key::Down => {
            let step = if k.mods.shift {
                NUDGE * FINE_DRAG
            } else {
                NUDGE
            };
            let d = if matches!(k.key, Key::Up) {
                step
            } else {
                -step
            };
            let was = f64::from(b.authored.get(sel).copied().unwrap_or(0.0));
            Some(BinEdit::Paint(vec![(
                sel,
                (was + d).clamp(0.0, 1.0) as f32,
            )]))
        }
        _ => None,
    })
}

/// One pixel column of the display: the loudest authored and live bin that
/// landed in it, and whether the selected one did.
#[derive(Clone, Copy, Default)]
struct Column {
    used: bool,
    authored: f64,
    live: f64,
    selected: bool,
}

/// Fold the bins into at most one column per pixel, taking the maximum of
/// each.
///
/// ponytail: max, not RMS or peak-and-average -- a bin that is loud must
/// never disappear into a column, which is the only thing this ceiling has
/// to promise. 1024 bins at 200 px is 200 bars; the upgrade, if a display
/// ever wants the shape of a dense column, is a second faint bar at the mean.
fn columns(
    size: Size,
    authored: &[f32],
    live: Option<&[f32]>,
    axis: BinAxis,
    pos: Option<&[f32]>,
    selected: Option<usize>,
) -> Vec<Column> {
    let n = authored.len();
    let cols = n.min(size.width as usize).max(1);
    let mut out = vec![Column::default(); cols];
    for (i, a) in authored.iter().enumerate() {
        let c = ((x_of(i, n, axis, pos) * cols as f64) as usize).min(cols - 1);
        let slot = &mut out[c];
        slot.used = true;
        slot.authored = slot.authored.max(f64::from(*a).clamp(0.0, 1.0));
        if let Some(v) = live.filter(|l| l.len() == n).and_then(|l| l.get(i)) {
            slot.live = slot.live.max(f64::from(*v).clamp(0.0, 1.0));
        }
        slot.selected |= selected == Some(i);
    }
    out
}

/// A bar standing on the floor of a `h`-tall box.
fn bar(x: f64, w: f64, h: f64, v: f64) -> Path {
    let top = h * (1.0 - v);
    Path::polyline(
        [
            Point::new(x, top),
            Point::new(x + w, top),
            Point::new(x + w, h),
            Point::new(x, h),
        ],
        true,
    )
}

/// The bin under the pointer, for a caller printing the partial's frequency
/// and level in its own words. `None` when the pointer is elsewhere.
///
/// ```
/// use mui::prelude::*;
/// let ui = Ui::default();
/// let mut saw: Vec<f32> = (1..=16).map(|n| 1.0 / n as f32).collect();
/// let b = Bins { authored: &mut saw, ..Bins::default() };
/// assert_eq!(bins_hover(&ui, "spectrum", &b), None, "nothing resolved yet");
/// ```
#[must_use]
pub fn bins_hover(ui: &Ui, id: impl Into<Id>, b: &Bins) -> Option<usize> {
    let id: Id = id.into();
    let id = id.as_str();
    let n = b.authored.len();
    let r = ui.get(id);
    if n == 0 || !(r.hovered || r.held) {
        return None;
    }
    let p = ui.local(id)?;
    let size = plot(ui, id)?;
    Some(bin_at(p.x / size.width, n, b.x, b.positions))
}

/// An additive spectrum: a bar per partial, painted with the pointer.
///
/// The gesture is applied to `b` before the tree is built, as [`curve`]
/// applies its drag, and the returned [`BinEdit`] says what changed --
/// `None` when nothing did. A press-drag
/// paints every bin the pointer crossed between the last frame and this one,
/// Shift refines from the level the press landed on, a secondary click
/// resets the bin under the pointer, and with the focus here Left/Right move
/// the selection, Up/Down nudge it (Shift fine) and Home/End jump to the
/// ends.
///
/// Draw it with [`bins_hover`] beside it for a readout. Size it like any
/// canvas; it stretches if you do not.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let mut saw: Vec<f32> = (1..=64).map(|n| 1.0 / n as f32).collect();
/// let plot = bins(&mut ui, "spectrum", &mut Bins { authored: &mut saw, ..Bins::default() });
/// assert_eq!(plot.changed, None, "nothing is dragging");
/// let _ = plot.size(320, 140);
/// ```
pub fn bins(ui: &mut Ui, id: impl Into<Id>, b: &mut Bins) -> Response<Option<BinEdit>> {
    let id: Id = id.into();
    let id = id.as_str();
    let n = b.authored.len();
    let edit = (n > 0)
        .then(|| pointed(ui, id, b, n).or_else(|| typed(ui, id, b, n)))
        .flatten();
    match &edit {
        Some(BinEdit::Paint(painted)) => {
            for &(i, v) in painted {
                if let Some(slot) = b.authored.get_mut(i) {
                    *slot = v;
                    b.selected = Some(i);
                }
            }
        }
        Some(BinEdit::Reset(i)) => {
            if let Some(slot) = b.authored.get_mut(*i) {
                *slot = 0.0;
            }
        }
        Some(BinEdit::Select(i)) => b.selected = Some(*i),
        None => {}
    }
    // The closure outlives the borrow, so it takes copies. 1024 f32 is 4 KB
    // a frame, which is the same order as the draw list it produces.
    let (authored, live) = (b.authored.to_vec(), b.live.map(<[f32]>::to_vec));
    let (positions, axis, selected) = (b.positions.map(<[f32]>::to_vec), b.x, b.selected);
    let el = canvas(move |size| {
        let (w, h) = (size.width.max(1.0), size.height.max(1.0));
        // A faint level grid first, so every bar paints over it.
        let mut draws: Vec<Draw> = [0.25, 0.5, 0.75]
            .into_iter()
            .map(|v| {
                let y = h * (1.0 - v);
                Draw::stroke(
                    Path::polyline([Point::new(0.0, y), Point::new(w, y)], false),
                    Role::Dim.alpha(0.25),
                    1.0,
                )
            })
            .collect();
        let cols = columns(
            Size::new(w, h),
            &authored,
            live.as_deref(),
            axis,
            positions.as_deref(),
            selected,
        );
        let slot = w / cols.len() as f64;
        // A hairline gap only while there is room for one: at a bin a pixel
        // wide the bars must abut or the spectrum turns into a comb.
        let width = (slot - if slot > 3.0 { 1.0 } else { 0.0 }).max(0.5);
        let mut caps = Vec::with_capacity(cols.len() * 2);
        for (c, col) in cols.iter().enumerate() {
            if !col.used {
                continue;
            }
            let x = c as f64 * slot;
            let ink = if col.selected {
                Role::Ink
            } else {
                Role::Primary
            };
            draws.push(Draw::fill(bar(x, width, h, col.authored), ink));
            let y = h * (1.0 - col.live);
            caps.push(Point::new(x, y));
            caps.push(Point::new(x + slot, y));
        }
        if live.is_some() {
            draws.push(Draw::stroke(
                Path::polyline(caps, false),
                Role::Ink.alpha(0.7),
                1.5,
            ));
        }
        draws
    })
    .cursor(Cursor::Crosshair)
    .focusable()
    .named("Partial levels")
    .id(id);
    Response { el, changed: edit }
}
