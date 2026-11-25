//! A library's own controls under the pointer and the keys, one model for
//! every format: a continuous control drags from where it was pressed along
//! the axis and sweep its translator set ([`model::Drag`]; Shift is fine),
//! the wheel and the arrow keys step it, Ctrl or Cmd with a press puts back
//! its default (a drag goes on from it), a double click on a value edit types a number; a button or
//! switch flips at the edge its source asks; a table sets the cells the
//! pointer crosses; a menu floats its items. What the user asked comes back
//! as [`Edit`]s for the host to write into the engine; the caller shows them
//! at once with [`apply`]. A text edit takes typed text the same way.

use kontra_ui_model::{self as model, Interface, Kind, Orientation, PageRef, Sweep, WidgetRef};
use mui::prelude::*;

use crate::kit;
use crate::theme::TEXT;
pub use kontra_ui_view::{hovers, surface, takes_input};

/// One control set by the user, and the modifier keys held at the click
/// (a script reads them as `$CONTROL_PAR_KEY_*`).
#[derive(Clone, Debug, PartialEq)]
pub struct Edit {
    pub widget: WidgetRef,
    pub to: Set,
    pub keys: model::UiModifiers,
}

/// What an [`Edit`] sets.
#[derive(Clone, Debug, PartialEq)]
pub enum Set {
    /// The control's value.
    Value(f64),
    /// One zero-based cell of a table.
    Cell(u32, f64),
    /// A text edit's text, or the file a file selector picked.
    Text(String),
    /// A file selector's browsing: the path it shows selected and each open
    /// column's first row. The page's own; the host never hears it.
    Browse(String, Vec<u32>),
    /// Files dragged onto a mouse area: let go (`drop`) or passing over,
    /// whether the drag is over it now, and the files it takes by kind
    /// (0 audio, 1 MIDI, 2 array).
    Drop {
        drop: bool,
        over: bool,
        items: Vec<(u8, String)>,
    },
}

/// Files dragged over the window: where (scene points), their paths, and
/// whether they were let go there.
#[derive(Clone, Debug, PartialEq)]
pub struct FileDrag {
    pub at: Point,
    pub paths: Vec<String>,
    pub dropped: bool,
}

/// A page's gesture in flight: the control pressed and its value then, the
/// menu floated open, and what is being typed into a value or text edit.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Gesture {
    press: Option<Press>,
    /// The momentary button held, whose release runs the source's own.
    tap: Option<(usize, Ident)>,
    pub menu: Option<usize>,
    entry: Option<Entry>,
    /// The value edit whose arrow is held, its step, and when it steps next.
    arrow: Option<(usize, f64, f64)>,
    /// The XY pad being dragged, its cursor, and where that cursor's
    /// corner was at the press (points from the pad's corner).
    xy: Option<(usize, u32, (f64, f64))>,
    /// Each file selector browsed so far: the path it shows selected and
    /// each open column's first row, kept over the face's rebuilds.
    files: Vec<(usize, String, Vec<u32>)>,
    /// The sub-menu levels a hierarchical menu has drilled into, by child.
    branch: Vec<usize>,
    /// The hover areas the pointer is over, by their page rectangle: a
    /// native page re-renders its views (new widgets, new callbacks) when
    /// a hover sets its state, and the area stays the same one.
    hovered: Vec<[i64; 4]>,
    /// The mouse area a file drag is over.
    dragged_over: Option<WidgetRef>,
    /// What the open menu is, found again each frame (see [`Ident`]).
    menu_of: Option<Ident>,
    /// The pointer of the input this frame answers, newer than
    /// [`Ui::pointer`]'s (the last frame's): a drag follows it in this
    /// frame. Taken by [`read`]; without it a drag is a frame behind.
    pub latest: Option<Point>,
}

/// A run of characters, from and to.
type Span = (usize, usize);

#[derive(Clone, Debug, PartialEq)]
struct Entry {
    n: usize,
    of: Ident,
    /// Keep MUI's focus, selection and text input tied to this popup as its
    /// anchor follows the control's current index.
    ui_id: String,
    text: String,
    pending: Option<Span>,
}

/// A range control dragged: which, its value at the press, where the
/// pointer went down, and whether Shift was held then (fine travel).
#[derive(Clone, Debug, PartialEq)]
struct Press {
    n: usize,
    of: Ident,
    start: f64,
    from: Point,
    fine: bool,
}

/// What a gesture in flight holds on to. A native page re-renders its
/// views as they change (a hover shows Conflux's assign toggles beside
/// the macro being dragged), and every widget after a changed one moves
/// in the tree: by number alone, the drag would stop or move another
/// control. A script's control is its id and name; an area of the page's
/// own (a tap) is where it sits.
#[derive(Clone, Debug, PartialEq)]
enum Ident {
    Control(i32, String),
    Area([i64; 4]),
}

fn ident(tree: &Interface, n: WidgetRef, w: &model::Widget) -> Ident {
    match w.source_id {
        Some(id) => Ident::Control(id, w.name.clone()),
        None => Ident::Area(area(tree.page_rect(n))),
    }
}

/// A rectangle in sixteenths of a point.
fn area(r: model::Rect) -> [i64; 4] {
    [r.x, r.y, r.width, r.height].map(|v| (v * 16.).round() as i64)
}

/// Where `of`, last seen as widget `at`, is now: still there, or the first
/// shown widget taking input that is the same. `None` once it is hidden,
/// disabled or gone.
fn find(tree: &Interface, at: usize, of: &Ident) -> Option<usize> {
    let live = |n: usize| {
        let w = tree.widgets.get(n)?;
        (tree.visible(WidgetRef(n)) && takes_input(w) && ident(tree, WidgetRef(n), w) == *of)
            .then_some(n)
    };
    live(at).or_else(|| (0..tree.widgets.len()).find_map(live))
}

impl Gesture {
    /// Whether a control is still pressed: dragged, or its arrow held.
    #[must_use]
    pub fn held(&self) -> bool {
        self.press.is_some() || self.xy.is_some() || self.arrow.is_some()
    }

    /// Release any pointer or drag gesture in flight (e.g. when the native
    /// host cancels input or loses window focus). File-selector browsing state
    /// is preserved.
    pub fn cancel(&mut self) {
        self.press = None;
        self.tap = None;
        self.arrow = None;
        self.xy = None;
        self.dragged_over = None;
    }

    /// Finds what is in flight in this frame's tree: a drag, a held button,
    /// the open menu and the field typed into follow their widget when the
    /// tree is rebuilt, and end when it is hidden, disabled or gone (a
    /// menu left open would pick for whatever widget took its number).
    fn follow(&mut self, tree: &Interface) {
        let at = |n: usize| {
            let n = WidgetRef(n);
            tree.widgets.get(n.0).map(|w| ident(tree, n, w))
        };
        if let Some(p) = &mut self.press {
            match find(tree, p.n, &p.of) {
                Some(n) => p.n = n,
                None => self.press = None,
            }
        }
        self.tap = (self.tap.take()).and_then(|(n, of)| Some((find(tree, n, &of)?, of)));
        if let Some(m) = self.menu {
            let of = self.menu_of.take().or_else(|| at(m));
            self.menu = of.as_ref().and_then(|of| find(tree, m, of));
            self.menu_of = of.filter(|_| self.menu.is_some());
            if self.menu.is_none() {
                self.branch.clear();
            }
        } else {
            self.menu_of = None;
        }
        self.entry = self.entry.take().and_then(|mut e| {
            e.n = find(tree, e.n, &e.of)?;
            Some(e)
        });
    }
}

/// A held arrow steps again after this, then every [`REPEAT_STEP`]
/// (Kontakt 8.13, measured: 12 steps in about 2 s).
const REPEAT_DELAY: f64 = 0.4;
const REPEAT_STEP: f64 = 0.15;

/// Which way a continuous control drags, and how many view points sweep
/// its range. A source that sets no [`model::Drag`] gets the renderer's: a
/// slider sweeps in its own length along its orientation, anything else in
/// 200 points up.
fn travel(w: &model::Widget, r: model::Rect, scale: f64) -> (bool, f64) {
    let drag = w.drag.unwrap_or(match &w.kind {
        Kind::Slider { orientation, .. } => model::Drag {
            axis: if *orientation == Orientation::Vertical || r.height > r.width {
                Orientation::Vertical
            } else {
                Orientation::Horizontal
            },
            sweep: Sweep::Lengths(1.),
        },
        _ => model::Drag {
            axis: Orientation::Vertical,
            sweep: Sweep::Points(200.),
        },
    });
    let vertical = drag.axis == Orientation::Vertical;
    let length = if vertical { r.height } else { r.width }.max(1.);
    let points = match drag.sweep {
        Sweep::Points(p) => p,
        Sweep::Lengths(k) => length * k,
    };
    (vertical, points.max(1.) * scale)
}

fn range(kind: &Kind) -> Option<model::Range> {
    match kind {
        Kind::Knob { range, .. } | Kind::Slider { range, .. } | Kind::ValueEdit { range, .. } => {
            Some(*range)
        }
        _ => None,
    }
}

/// The widget's mapper law (UVI Mapper, reference formulas): the value at
/// normalised position `pos`, `min + (max - min)·f(pos)` with `f` a power or
/// root, or `min·(max/min)^pos` for Exponential over a strictly positive
/// range. `inverse` gives the position of a value instead.
fn mapped(mapper: Option<&str>, range: model::Range, x: f64, inverse: bool) -> f64 {
    let (lo, span) = (range.min, range.max - range.min);
    let power = match mapper {
        Some("Exponential") if lo > 0. && range.max > 0. => {
            let ratio = range.max / lo;
            return if inverse {
                (x.clamp(lo, range.max) / lo).ln() / ratio.ln()
            } else {
                lo * ratio.powf(x.clamp(0., 1.))
            };
        }
        Some("QuinticRoot") => 0.2,
        Some("QuarticRoot") => 0.25,
        Some("CubeRoot") => 1. / 3.,
        Some("SquareRoot") => 0.5,
        Some("Quadratic") => 2.,
        Some("Cubic") => 3.,
        Some("Quartic") => 4.,
        Some("Quintic") => 5.,
        _ => 1.,
    };
    match (inverse, span == 0.) {
        (_, true) => lo,
        (false, _) => lo + span * x.clamp(0., 1.).powf(power),
        (true, _) => ((x - lo) / span).clamp(0., 1.).powf(1. / power),
    }
}

/// `from` moved `by` of the range's length along the widget's mapper.
fn slide(w: &model::Widget, range: model::Range, from: f64, by: f64) -> f64 {
    let span = range.max - range.min;
    match w.mapper.as_deref() {
        None | Some("Linear") => from + by * span,
        m => {
            let pos = mapped(m, range, from, true);
            mapped(m, range, (pos + by).clamp(0., 1.), false)
        }
    }
}

/// `v` inside `range`, on its step grid.
fn snap(range: model::Range, v: f64) -> f64 {
    let (lo, hi) = (range.min.min(range.max), range.max.max(range.min));
    let v = v.clamp(lo, hi);
    range.step.filter(|s| *s > 0.).map_or(v, |s| {
        // The nearest grid point inside: a top off the grid snaps below it.
        let (first, last) = ((lo / s).ceil(), (hi / s).floor());
        if first > last {
            v
        } else {
            (v / s).round().clamp(first, last) * s
        }
    })
}

/// Reference scene-unit line height for mouse wheel normalization (independent
/// of host `mui::host::NOTCH_LINES` scroller speed and widget drag travel).
const WHEEL_LINE_PX: f64 = 20.0;
/// Full-range wheel sweep in unscaled scene units (10 reference lines = 100%).
const WHEEL_SWEEP_PX: f64 = 200.0;
/// Maximum normalized range fraction applied by a single coarse wheel event.
/// Caps high-`NOTCH_LINES` host wheel events so changing list scroll speed does
/// not blow up knob/slider sensitivity, while preserving fine trackpad motion.
const WHEEL_MAX_STEP: f64 = 0.10;
/// Minimum non-zero wheel magnitude (scene px) that advances a coarse stepped
/// control by one [`notch`] when proportional travel would round back to `now`.
const WHEEL_STEP_MIN_PX: f64 = 4.0;

/// Next value of a ranged control after a wheel delta `wheel_y` (positive is
/// scroll down / decrease), decoupled from host `NOTCH_LINES` and drag travel.
fn wheel_value(w: &model::Widget, range: model::Range, now: f64, wheel_y: f64, fine: bool) -> f64 {
    if !wheel_y.is_finite() || wheel_y == 0.0 {
        return now;
    }
    let scale = if fine { 0.1 } else { 1.0 };
    if matches!(w.kind, Kind::ValueEdit { .. }) {
        let unit = notch(&w.kind, range);
        let dir = (-wheel_y).signum();
        let steps = (-wheel_y / WHEEL_LINE_PX).clamp(-1.0, 1.0);
        let delta = steps * unit * scale;
        let moved = snap(range, now + delta);
        if moved == now && wheel_y.abs() >= WHEEL_STEP_MIN_PX {
            return snap(range, now + dir * unit);
        }
        return moved;
    }
    let max_by = WHEEL_MAX_STEP * scale;
    let by = (-wheel_y / WHEEL_SWEEP_PX * scale).clamp(-max_by, max_by);
    let moved = snap(range, slide(w, range, now, by));
    if moved == now && wheel_y.abs() >= WHEEL_STEP_MIN_PX && range.step.is_some() {
        let min_step = range
            .step
            .filter(|s| *s > 0.0)
            .unwrap_or_else(|| notch(&w.kind, range));
        // Same direction `slide` moves: a descending range flips it.
        let dir = (-wheel_y).signum() * (range.max - range.min).signum();
        snap(range, now + dir * min_step)
    } else {
        moved
    }
}

/// How far one arrow key moves a control: a value edit by its step, or
/// one unit without one (as its arrows do), anything else by a hundredth
/// of its range or its step when coarser.
fn notch(kind: &Kind, range: model::Range) -> f64 {
    let step = range.step.filter(|s| *s > 0.);
    match kind {
        Kind::ValueEdit { .. } => step.unwrap_or(1.),
        _ => step.unwrap_or(0.).max((range.max - range.min).abs() / 100.),
    }
}

/// How far a press on a value edit's arrows steps it: one shown unit, up in
/// the upper half of its right 14 px, down in the lower (Kontakt 8.13,
/// measured; one callback a step, none on release).
fn arrow(ui: &Ui, id: &str, w: &model::Widget, scale: f64) -> Option<f64> {
    let Kind::ValueEdit {
        arrows: true,
        display,
        ..
    } = &w.kind
    else {
        return None;
    };
    let size = ui.scene()?.surface(id)?.frame.size;
    let at = ui.local(id)?;
    let unit = if display.ratio == 0. {
        1.
    } else {
        display.ratio
    };
    (at.x >= size.width - 14. * scale).then_some(if at.y < size.height / 2. { unit } else { -unit })
}

/// This frame's edits to the controls of `page` drawn at `scale`, and the
/// open menu's or typed entry's popup, to float over the page.
pub fn read(
    ui: &mut Ui,
    face: usize,
    tree: &Interface,
    page: PageRef,
    scale: f64,
    g: &mut Gesture,
) -> (Vec<Edit>, Option<El>) {
    let mut edits = Vec::new();
    let order = tree.draw_order(page);
    let pointer = ui.pointer().pos;
    let latest = g.latest.take().or(pointer);
    // A file drag moves the pointer with it; one that leaves the window or
    // is cancelled takes it away: the area it was over hears it leave.
    if pointer.is_none()
        && let Some(n) = g.dragged_over.take()
        && hears_drag(tree, n)
    {
        edits.push(left(n));
    }
    let down = ui.pointer().buttons.contains(mui::input::Button::Primary);
    g.follow(tree);
    let mut over = Vec::new();
    for (i, &n) in order.iter().enumerate() {
        if let Some(w) = tree.widgets.get(n.0).filter(|w| hovers(w)) {
            let key = area(tree.page_rect(n));
            let edit = if tree.visible(n) {
                hover(
                    ui,
                    surface(face, n.0),
                    (n, w, key),
                    scale,
                    pointer,
                    g,
                    &mut over,
                )
            } else {
                // Hidden under the pointer: it hears the pointer leave, as
                // SwiftUI's hover ends when its view goes away.
                g.hovered.contains(&key).then(|| Edit {
                    widget: n,
                    to: Set::Value(0.),
                    keys: model::UiModifiers::default(),
                })
            };
            edits.extend(edit);
            continue;
        }
        let Some(w) = tree.widgets.get(n.0).filter(|w| takes_input(w)) else {
            continue;
        };
        if !tree.visible(n) {
            continue;
        }
        // A native page's view with both a drag and a tap gesture lowers to
        // its control and, right over it, a momentary tap area (face::nui):
        // the area takes the pointer, so the control reads its drag and
        // wheel there, and the area still runs the page's own press and
        // release callbacks (Conflux's Bend springs back to 0 on release).
        let grip = range(&w.kind)
            .and(order.get(i + 1))
            .filter(|m| {
                tree.visible(**m)
                    && tree.widgets.get(m.0).is_some_and(|t| {
                        takes_input(t)
                            && matches!(t.kind, Kind::Button { momentary: true })
                            && t.rect == w.rect
                    })
            })
            .map_or(n.0, |m| m.0);
        let id = surface(face, grip);
        let r = ui.get(id.clone());
        // Menus and value edits don't report keys (KSP manual, KEY_SHIFT).
        let keys = match w.kind {
            Kind::Menu { .. } | Kind::ValueEdit { .. } => model::UiModifiers::default(),
            _ => model::UiModifiers {
                shift: r.press_mods.shift,
                ctrl: r.press_mods.ctrl,
                alt: r.press_mods.alt,
                cmd: r.press_mods.cmd,
            },
        };
        let edit = |to| Edit {
            widget: n,
            to,
            keys,
        };
        if matches!(w.kind, Kind::Table { .. }) {
            if r.held {
                edits.extend(cells(ui, &id, r.drag_delta, w, &edit));
            }
            continue;
        }
        if matches!(w.kind, Kind::Xy { .. }) {
            edits.extend(pad(ui, &id, &r, (tree, n), g, &edit));
            continue;
        }
        if matches!(w.kind, Kind::FileSelector { .. }) {
            edits.extend(browse(ui, &id, &r, (w, n.0), scale, g, &edit));
            continue;
        }
        let now = w.number();
        let before = edits.len();
        let mut set = |v: f64| {
            if v != now {
                edits.push(edit(Set::Value(v)));
            }
        };
        if let Some(range) = range(&w.kind) {
            let (vertical, points) = travel(w, tree.page_rect(n), scale);
            let along = |d: Vec2| if vertical { -d.y } else { d.x };
            let reset = r.press_mods.ctrl || r.press_mods.cmd;
            let arrow = r.pressed.then(|| arrow(ui, &id, w, scale)).flatten();
            let t = ui.time();
            if let Some(unit) = arrow {
                set(snap(range, now + unit));
                g.arrow = Some((n.0, unit, t + REPEAT_DELAY));
            }
            match g.arrow {
                Some((a, unit, next)) if a == n.0 && r.held => {
                    let next = if t >= next {
                        set(snap(range, now + unit));
                        t + REPEAT_STEP
                    } else {
                        next
                    };
                    g.arrow = Some((a, unit, next));
                    ui.repaint_in(next - t);
                }
                Some((a, ..)) if a == n.0 => g.arrow = None,
                _ => {}
            }
            if r.pressed && arrow.is_none() {
                // Ctrl puts back the default, and a drag goes on from there
                // (Kontakt 8 oracle, `tools/ksp-ui` geometry page: a
                // ctrl-drag moves a slider as far as a plain one).
                let start = if reset {
                    snap(range, range.default)
                } else {
                    now
                };
                g.press = pointer.map(|from| Press {
                    n: n.0,
                    of: ident(tree, n, w),
                    start,
                    from,
                    fine: r.press_mods.shift,
                });
                if reset {
                    set(start);
                }
            }
            // From the pointer, not the widget's own held state: a page
            // that re-renders the control mid-drag gives it a new surface.
            if let Some(p) = g.press.as_ref().filter(|p| p.n == n.0 && down)
                && let Some(at) = latest
            {
                // Sub-pixel: the pointer's travel is a float, so a
                // high-resolution mouse or trackpad moves it finely.
                let fine = if p.fine { 0.1 } else { 1. };
                // The travel is in position space, so a mapper bends it.
                let by = along(at - p.from) / points * fine;
                set(snap(range, slide(w, range, p.start, by)));
            }
            // Claimed, so the rack under the knob does not scroll with it.
            // Normalized independently of drag `points` and host `NOTCH_LINES`.
            if let Some(wheel) = ui.wheel(id.as_str()).filter(|_| !r.held) {
                let fine = ui.pointer().mods.shift;
                set(wheel_value(w, range, now, wheel.y, fine));
            }
            // Keyboard focus steps it, the way the wheel does.
            let step = ui.keys(id.as_str()).iter().fold(0., |at, k| match k.key {
                Key::Up | Key::Right => at + 1.,
                Key::Down | Key::Left => at - 1.,
                _ => at,
            });
            if step != 0. {
                set(snap(range, now + step * notch(&w.kind, range)));
            }
            if matches!(w.kind, Kind::ValueEdit { .. })
                && arrow.is_none()
                && ui.double_click(id.as_str())
            {
                let shown = match &w.kind {
                    Kind::ValueEdit { display, .. } if display.ratio != 0. => now / display.ratio,
                    _ => now,
                };
                let shown = shown.to_string();
                let all = (0, shown.chars().count());
                type_into(ui, g, (face, n), ident(tree, n, w), shown, all);
            }
            if !down && g.press.as_ref().is_some_and(|p| p.n == n.0) {
                g.press = None;
                // The source runs its callback on release, moved or not.
                if w.reports_release && edits.len() == before {
                    edits.push(edit(Set::Value(now)));
                }
            }
            continue;
        }
        match &w.kind {
            Kind::Button { momentary: true } => {
                // Its release always reports: a press can rebuild it at 0
                // (a native page re-rendering), and the release still runs
                // the source's own callback (a tab switching its page).
                if r.pressed {
                    set(1.);
                    g.tap = Some((n.0, ident(tree, n, w)));
                } else if !down && g.tap.as_ref().is_some_and(|t| t.0 == n.0) {
                    g.tap = None;
                    edits.push(edit(Set::Value(0.)));
                }
            }
            Kind::Button { .. } | Kind::Switch => {
                let hit = if w.activate_on_press {
                    r.pressed
                } else {
                    r.clicked
                };
                if hit || r.key_activated {
                    set(if now != 0. { 0. } else { 1. });
                }
            }
            Kind::Menu { .. } if r.clicked || r.key_activated => {
                g.menu = (g.menu != Some(n.0)).then_some(n.0);
                g.menu_of = g.menu.map(|_| ident(tree, n, w));
                g.branch.clear();
            }
            // A double-click selects the word under the pointer (Kontakt 8
            // oracle, tools/ksp-ui text page); the first click opened the
            // field with its caret at the end.
            Kind::TextEdit if ui.double_click(id.as_str()) => {
                let size = ui
                    .scene()
                    .and_then(|s| s.surface(&id))
                    .map(|s| s.frame.size);
                let at = ui.local(id.as_str()).zip(size).map_or(0, |(p, s)| {
                    kontra_ui_view::char_at(ui, tree, w, (s.width, s.height, scale), p.x)
                });
                type_into(
                    ui,
                    g,
                    (face, n),
                    ident(tree, n, w),
                    w.text.clone(),
                    word(&w.text, at),
                );
            }
            Kind::TextEdit if (r.clicked || r.key_activated) && g.entry.is_none() => {
                let end = w.text.chars().count();
                type_into(
                    ui,
                    g,
                    (face, n),
                    ident(tree, n, w),
                    w.text.clone(),
                    (end, end),
                );
            }
            _ => {}
        }
    }
    g.hovered = over;
    let popup =
        menu(ui, face, tree, g, &mut edits).or_else(|| entry(ui, face, tree, g, &mut edits));
    (edits, popup)
}

/// Hover area `w` (number `n`) as the pointer at `pointer` finds it: kept
/// in `over` while the pointer is on it, and an edit (1 on entering, 0 on
/// leaving) when that changed since the last frame, which runs the page's
/// own `enter` or `exit`.
fn hover(
    ui: &Ui,
    id: String,
    (n, w, key): (WidgetRef, &model::Widget, [i64; 4]),
    scale: f64,
    pointer: Option<Point>,
    g: &Gesture,
    over: &mut Vec<[i64; 4]>,
) -> Option<Edit> {
    let r = w.rect;
    let at = ui.scene().and_then(|s| s.surface(&id)).map(|s| s.frame);
    let inside = pointer.zip(at).is_some_and(|(p, f)| {
        p.x >= f.x && p.y >= f.y && p.x < f.x + r.width * scale && p.y < f.y + r.height * scale
    });
    if inside {
        over.push(key);
    }
    (inside != g.hovered.contains(&key)).then(|| Edit {
        widget: n,
        to: Set::Value(if inside { 1. } else { 0. }),
        keys: model::UiModifiers::default(),
    })
}

/// A file drag over page `page`: the topmost mouse area under it that
/// takes any of the files hears it let go, and, when it asked for drags,
/// the drag enter and leave it (KSP manual, `ui_mouse_area`).
pub fn files(
    ui: &Ui,
    face: usize,
    (tree, page): (&Interface, PageRef),
    scale: f64,
    drag: &FileDrag,
    g: &mut Gesture,
) -> Vec<Edit> {
    let p = drag.at;
    let target = tree.draw_order(page).iter().rev().find_map(|&n| {
        let w = tree.widgets.get(n.0).filter(|_| tree.visible(n))?;
        let Kind::MouseArea { takes, drag: hears } = w.kind else {
            return None;
        };
        let f = ui.scene()?.surface(&surface(face, n.0))?.frame;
        let (width, height) = (w.rect.width * scale, w.rect.height * scale);
        let inside = p.x >= f.x && p.y >= f.y && p.x < f.x + width && p.y < f.y + height;
        let items = taken(&drag.paths, takes);
        (inside && !items.is_empty()).then_some((n, hears, items))
    });
    let mut edits = Vec::new();
    let now = target.as_ref().map(|t| t.0);
    if g.dragged_over != now || drag.dropped {
        if let Some(n) = g
            .dragged_over
            .filter(|&n| Some(n) != now && hears_drag(tree, n))
        {
            edits.push(left(n));
        }
        if let Some((n, hears, items)) = target
            && (drag.dropped || hears)
        {
            let to = Set::Drop {
                drop: drag.dropped,
                over: true,
                items,
            };
            let keys = model::UiModifiers::default();
            edits.push(Edit {
                widget: n,
                to,
                keys,
            });
        }
    }
    g.dragged_over = now.filter(|_| !drag.dropped);
    edits
}

/// Whether mouse area `n` asked to hear drags pass over, not only drops.
fn hears_drag(tree: &Interface, n: WidgetRef) -> bool {
    let w = tree.widgets.get(n.0);
    w.is_some_and(|w| matches!(w.kind, Kind::MouseArea { drag: true, .. }))
}

/// A file drag leaving mouse area `n`.
fn left(n: WidgetRef) -> Edit {
    Edit {
        widget: n,
        to: Set::Drop {
            drop: false,
            over: false,
            items: Vec::new(),
        },
        keys: model::UiModifiers::default(),
    }
}

/// The files of `paths` a mouse area taking `takes` (audio, MIDI, array:
/// 0 none, 1 one, more any number) accepts, by kind.
// ponytail: more files of a kind than it takes one of are all refused,
// unmeasured; measure in Kontakt if a library drops several on a one.
fn taken(paths: &[String], takes: [u16; 3]) -> Vec<(u8, String)> {
    let kind = |p: &str| {
        let ext = std::path::Path::new(p).extension()?.to_str()?;
        match ext.to_ascii_lowercase().as_str() {
            "wav" | "aif" | "aiff" | "ncw" => Some(0u8),
            "mid" | "midi" => Some(1),
            "nka" => Some(2),
            _ => None,
        }
    };
    let kinds: Vec<(u8, String)> = (paths.iter())
        .filter_map(|p| Some((kind(p)?, p.clone())))
        .collect();
    let count = |k: u8| kinds.iter().filter(|(j, _)| *j == k).count();
    (kinds.iter())
        .filter(|(k, _)| match takes.get(usize::from(*k)) {
            Some(0) | None => false,
            Some(1) => count(*k) == 1,
            Some(_) => true,
        })
        .cloned()
        .collect()
}

/// The cells of a table the pointer crossed this frame, set to its height
/// there: a fast drag fills every column between, not only where it landed.
fn cells(
    ui: &Ui,
    id: &str,
    delta: Vec2,
    w: &model::Widget,
    edit: &dyn Fn(Set) -> Edit,
) -> Vec<Edit> {
    let Kind::Table {
        columns,
        range,
        steps_shown,
        ..
    } = &w.kind
    else {
        return Vec::new();
    };
    let (columns, range) = (*columns, *range);
    let shown = steps_shown
        .filter(|&s| s > 0)
        .unwrap_or(columns)
        .min(columns);
    let size = ui.scene().and_then(|s| s.surface(id)).map(|s| s.frame.size);
    let (Some(pos), Some(size)) = (ui.local(id), size) else {
        return Vec::new();
    };
    if shown == 0 || size.width <= 0. || size.height <= 0. {
        return Vec::new();
    }
    let at = |x: f64| ((x / size.width * f64::from(shown)).floor().max(0.) as u32).min(shown - 1);
    let height = |y: f64| {
        snap(
            range,
            range.max - (y / size.height).clamp(0., 1.) * (range.max - range.min),
        )
    };
    let from = pos - delta;
    let (first, last) = (at(from.x), at(pos.x));
    let count = first.abs_diff(last);
    (0..=count)
        .filter_map(|step| {
            let k = if count == 0 {
                1.
            } else {
                f64::from(step) / f64::from(count)
            };
            let c = if last >= first {
                first + step
            } else {
                first - step
            };
            let v = height(from.y + (pos.y - from.y) * k);
            (w.cell(c) != Some(v)).then(|| edit(Set::Cell(c, v)))
        })
        .collect()
}

/// An XY pad's cursor moved by the pointer, as its `MOUSE_MODE` says
/// (KSP manual, Control Parameters; 0 when unset, Kontakt 8 oracle):
/// 0 drags the active cursor from a press on it, 1 from a press anywhere,
/// both by the pointer's travel; 2 moves the cursor pressed (else the
/// active one) to the pointer and keeps it there.
fn pad(
    ui: &Ui,
    id: &str,
    r: &mui::input::Response,
    (tree, n): (&Interface, model::WidgetRef),
    g: &mut Gesture,
    edit: &dyn Fn(Set) -> Edit,
) -> Vec<Edit> {
    use kontra_ui_view::xy;
    let Some(w) = tree.widgets.get(n.0) else {
        return Vec::new();
    };
    let Kind::Xy {
        cursors,
        sensitivity,
        mouse_mode,
    } = w.kind
    else {
        return Vec::new();
    };
    let size = ui.scene().and_then(|s| s.surface(id)).map(|s| s.frame.size);
    let (Some(pos), Some(size)) = (ui.local(id), size) else {
        return Vec::new();
    };
    let points = (w.rect.width, w.rect.height);
    if points.0 <= 0. || size.width <= 0. {
        return Vec::new();
    }
    // View units per point.
    let k = size.width / points.0;
    let at = (pos.x / k, pos.y / k);
    let art = |i| xy::art_size(tree, w, i);
    let rect = |i| xy::rect(w, i, points, art(i));
    let inside = |i| {
        rect(i).is_some_and(|[x, y, cw, ch]| {
            (x..x + cw).contains(&at.0) && (y..y + ch).contains(&at.1)
        })
    };
    let mode = mouse_mode.unwrap_or(0);
    let active = xy::active(w).min(cursors.saturating_sub(1));
    if r.pressed {
        let cursor = match mode {
            0 => inside(active).then_some(active),
            1 => Some(active),
            _ => Some((0..cursors).rev().find(|&i| inside(i)).unwrap_or(active)),
        };
        // From the unrounded corner, so a press that does not move edits nothing.
        g.xy = cursor.and_then(|c| Some((n.0, c, xy::corner(w, c, points, art(c))?)));
        // A press off every cursor moves none, but the source still runs its
        // callback once, values unchanged (Kontakt 8 oracle, tools/ksp-ui
        // data page, MOUSE_MODE 0).
        if cursor.is_none() && w.reports_release {
            return (w.cell(2 * active))
                .map(|v| edit(Set::Cell(2 * active, v)))
                .into_iter()
                .collect();
        }
    }
    let Some((_, c, start)) = g.xy.filter(|(p, ..)| *p == n.0) else {
        return Vec::new();
    };
    // The source runs its callback once on the press and once on the
    // release, moved or not (Kontakt 8 oracle, probe gate.ksp: a held
    // press read 1 call, its release 2): an edit of the cursor's x cell
    // as it stands, each.
    let stands = || w.cell(2 * c).map(|v| edit(Set::Cell(2 * c, v)));
    let mut out: Vec<Edit> = stands()
        .filter(|_| r.pressed && w.reports_release)
        .into_iter()
        .collect();
    if r.released || !r.held {
        g.xy = None;
        out.extend(stands().filter(|_| r.released && w.reports_release));
        return out;
    }
    let [_, _, cw, ch] = rect(c).unwrap_or([0., 0., xy::CURSOR, xy::CURSOR]);
    let corner = if mode >= 2 {
        // The cursor's centre on the pointer: a stock square's is 6 in.
        let half = |s: f64| {
            if art(c).is_some() {
                s / 2.
            } else {
                (s - 1.) / 2.
            }
        };
        (at.0 - half(cw), at.1 - half(ch))
    } else {
        // MOUSE_BEHAVIOUR_X/Y scale the travel by |value| / 1000, 1000 (the
        // default) 1:1; a negative value moves the same way (Kontakt 8
        // oracle, probe/wave.ksp: -1000 as 1000, 500 half, 2000 double).
        let scale = |s: Option<u32>| f64::from(s.unwrap_or(1000)) / 1000.;
        let [sx, sy] = sensitivity.map(scale);
        (
            start.0 + r.drag_total.x / k * sx,
            start.1 + r.drag_total.y / k * sy,
        )
    };
    let (x, y) = xy::value(corner, points, art(c));
    out.extend(
        [(2 * c, x), (2 * c + 1, y)]
            .into_iter()
            .filter(|&(cell, v)| w.cell(cell).is_some_and(|now| (now - v).abs() > 1e-9))
            .map(|(cell, v)| edit(Set::Cell(cell, v))),
    );
    out
}

/// The open menu's popup, and the item picked from it.
fn menu(
    ui: &mut Ui,
    face: usize,
    tree: &Interface,
    g: &mut Gesture,
    edits: &mut Vec<Edit>,
) -> Option<El> {
    let m = g.menu?;
    let w = tree.widgets.get(m)?;
    let Kind::Menu { items } = &w.kind else {
        return None;
    };
    let id = surface(face, m);
    if ui.dismissed(&[&id, &format!("{id}-popup")]) {
        g.menu = None;
        return None;
    }
    if let Some(tree) = &w.menu_hierarchy {
        return sub_menu(ui, &id, (m, tree, items), g, edits);
    }
    let shown: Vec<&model::MenuItem> = items.iter().filter(|i| i.visible).collect();
    let labels: Vec<&str> = shown.iter().map(|i| i.text.as_str()).collect();
    let now = w.number() as i32;
    let current = shown.iter().position(|i| i.value == now);
    let menu = kit::menu(ui, &id, &labels, 1, current);
    if let Some(item) = menu.changed.and_then(|k| shown.get(k)) {
        edits.push(Edit {
            widget: WidgetRef(m),
            to: Set::Value(f64::from(item.value)),
            keys: model::UiModifiers::default(),
        });
        g.menu = None;
    }
    Some(menu.el)
}

/// A file selector under the pointer and the keys (Kontakt 8 oracle,
/// probes fs3.ksp and fs4.ksp): a click selects a row, and a folder's opens
/// its column; a double-click or Return on a file picks it, which runs the
/// callback; Up and Down move the selection in the deepest column; the
/// wheel scrolls the column under the pointer (Kontakt's ignores it).
fn browse(
    ui: &mut Ui,
    id: &str,
    r: &mui::input::Response,
    (w, n): (&model::Widget, usize),
    scale: f64,
    g: &mut Gesture,
    edit: &dyn Fn(Set) -> Edit,
) -> Vec<Edit> {
    use kontra_ui_view::files;
    let Kind::FileSelector {
        base_path,
        files: listed,
        column_width,
        ..
    } = &w.kind
    else {
        return Vec::new();
    };
    let kept = g.files.iter().find(|(m, ..)| *m == n);
    let (mut path, mut tops) = kept.map_or_else(
        || (w.text.clone(), Vec::new()),
        |(_, p, t)| (p.clone(), t.clone()),
    );
    let base = base_path.as_deref().unwrap_or("");
    let frame = ui.scene().and_then(|s| s.surface(id)).map(|s| s.frame.size);
    let Some(frame) = frame.filter(|_| scale > 0.) else {
        return Vec::new();
    };
    let size = (frame.width / scale, frame.height / scale);
    let pos = ui.local(id).map(|p| (p.x / scale, p.y / scale));
    let mut cols = files::columns(base, &path, *listed);
    let mut lay = files::Layout::of(&cols, *column_width, size);
    let full = |c: &files::Column, i: usize| {
        (c.entries.get(i)).map(|e| (c.dir.join(&e.name).to_string_lossy().into_owned(), e.folder))
    };
    let mut picked = None;
    let mut moved = false;
    let hit = pos.and_then(|p| lay.hit(&cols, &tops, p));
    let at = hit.and_then(|(c, i)| full(cols.get(c)?, i));
    if let Some((p, folder)) = at.filter(|_| r.clicked || ui.double_click(id)) {
        if ui.double_click(id) && !folder {
            picked = Some(p.clone());
        }
        moved = p != path;
        path = p;
    }
    let deepest = cols.iter().rposition(|c| c.picked.is_some());
    for k in ui.keys(id) {
        let col = deepest.and_then(|c| cols.get(c));
        let now = col.and_then(|c| c.picked);
        let to = match k.key {
            Key::Up => now.map(|i| i.saturating_sub(1)),
            Key::Down => now.map(|i| i + 1),
            Key::Enter => {
                let at = col.zip(now).and_then(|(c, i)| full(c, i));
                picked = at.filter(|(_, folder)| !folder).map(|(p, _)| p).or(picked);
                None
            }
            _ => None,
        };
        if let Some((p, _)) = col.zip(to).and_then(|(c, i)| full(c, i)) {
            moved = p != path;
            path = p;
        }
    }
    if moved {
        cols = files::columns(base, &path, *listed);
        lay = files::Layout::of(&cols, *column_width, size);
    }
    tops.resize(cols.len(), 0);
    let wheel = ui.wheel(id);
    let under = pos.map(|(x, _)| ((x + lay.shift) / lay.pitch).floor().max(0.) as usize);
    for (c, (col, top)) in cols.iter().zip(tops.iter_mut()).enumerate() {
        let last = lay.last_top(col.entries.len());
        if let Some(wy) = wheel.map(|v| v.y).filter(|_| under == Some(c)) {
            let rows = (wy / files::ROW).round();
            let rows = if rows == 0. { wy.signum() } else { rows };
            *top = (f64::from(*top) + rows).clamp(0., f64::from(last)) as u32;
        }
        // A selection the keys or a click moved stays in sight.
        if let Some(p) = col
            .picked
            .filter(|_| moved)
            .and_then(|p| u32::try_from(p).ok())
        {
            let rows = u32::try_from(lay.rows.max(1)).unwrap_or(1);
            *top = (*top).min(p).max((p + 1).saturating_sub(rows));
        }
        *top = (*top).min(last);
    }
    match g.files.iter_mut().find(|(m, ..)| *m == n) {
        Some(kept) => *kept = (n, path.clone(), tops.clone()),
        None => g.files.push((n, path.clone(), tops.clone())),
    }
    let mut out = vec![edit(Set::Browse(path, tops))];
    out.extend(picked.map(|p| edit(Set::Text(p))));
    out
}

/// A hierarchical menu (UVI `hierarchical`, "a/b" items): the level `g.branch`
/// names, a branch drilling in (`›`) and a "‹ Back" line out, a leaf picked.
/// ponytail: one popup that drills in, not a cascade of floating popups.
fn sub_menu(
    ui: &mut Ui,
    id: &str,
    (m, nodes, items): (usize, &[model::MenuNode], &[model::MenuItem]),
    g: &mut Gesture,
    edits: &mut Vec<Edit>,
) -> Option<El> {
    let mut level = nodes;
    for &k in &g.branch {
        level = level.get(k).map_or(level, |n| &n.children);
    }
    let back = usize::from(!g.branch.is_empty());
    let mut labels: Vec<String> = Vec::new();
    if back == 1 {
        labels.push("‹ Back".into());
    }
    labels.extend(level.iter().map(|n| {
        if n.children.is_empty() {
            n.label.clone()
        } else {
            format!("{} ›", n.label)
        }
    }));
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let menu = kit::menu(ui, id, &refs, 1, None);
    if let Some(k) = menu.changed {
        match k.checked_sub(back).and_then(|i| level.get(i)) {
            None => {
                g.branch.pop();
            }
            Some(n) if !n.children.is_empty() => g.branch.push(k - back),
            Some(n) => {
                if let Some(item) = n.item_index.and_then(|i| items.get(i)) {
                    edits.push(Edit {
                        widget: WidgetRef(m),
                        to: Set::Value(f64::from(item.value)),
                        keys: model::UiModifiers::default(),
                    });
                }
                g.menu = None;
            }
        }
    }
    Some(menu.el)
}

/// Opens typed entry on the control, starting from `text`:
/// a value edit's selected whole, so what the user types replaces it; a
/// text edit's with the caret at its end, as Kontakt 8 opens one (oracle
/// probe fs3.ksp: a click then "x" turned "abc" into "abcx").
fn type_into(
    ui: &mut Ui,
    g: &mut Gesture,
    (face, n): (usize, WidgetRef),
    of: Ident,
    text: String,
    select: Span,
) {
    let ui_id = surface(face, n.0);
    ui.focus(format!("{ui_id}-entry"));
    g.entry = Some(Entry {
        n: n.0,
        of,
        ui_id,
        text,
        pending: Some(select),
    });
}

/// The word of `text` around character `i`, as a character range: the run
/// of non-spaces it is in, or the spaces it is between.
fn word(text: &str, i: usize) -> (usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    let i = i.min(chars.len().saturating_sub(1));
    let space = |c: Option<&char>| c.is_some_and(|c| c.is_whitespace());
    let kind = space(chars.get(i));
    let from = (0..i)
        .rev()
        .find(|&j| space(chars.get(j)) != kind)
        .map_or(0, |j| j + 1);
    let to = (i..chars.len())
        .find(|&j| space(chars.get(j)) != kind)
        .unwrap_or(chars.len());
    (from, to)
}

/// What is being typed into a value edit (a number in its shown units) or
/// a text edit: Enter sets it and Escape drops it. A click elsewhere drops
/// a value edit's but sets a text edit's (Kontakt 8 oracle, probe fs3.ksp:
/// "abc" then a click away ran the callback with "abc").
fn entry(
    ui: &mut Ui,
    face: usize,
    tree: &Interface,
    g: &mut Gesture,
    edits: &mut Vec<Edit>,
) -> Option<El> {
    let mut entry = g.entry.take()?;
    let w = tree.widgets.get(entry.n)?;
    let id = surface(face, entry.n);
    let field = format!("{0}-entry", entry.ui_id);
    let popup = format!("{0}-popup", entry.ui_id);
    let words = matches!(w.kind, Kind::TextEdit);
    let away = ui.dismissed(&[&id, &popup]);
    if (away && !words) || ui.keys(field.as_str()).iter().any(|k| k.key == Key::Escape) {
        return None;
    }
    if !away {
        if let Some((from, to)) = entry.pending
            && ui.request_action(SemanticAction::set_selection(field.as_str(), from, to))
        {
            entry.pending = None;
        }
        let typed = text_edit(ui, field.as_str(), &mut entry.text, TextOpts::default());
        if !typed.changed.submitted {
            g.entry = Some(entry);
            // In a row: the popup names its content, and the field keeps its own id.
            return Some(kit::popup(&id, row![typed.el.w(TEXT * 6.)]).id(popup));
        }
    }
    ui.blur();
    let set = |to| Edit {
        widget: WidgetRef(entry.n),
        to,
        keys: model::UiModifiers::default(),
    };
    match &w.kind {
        Kind::ValueEdit { range, display, .. } => {
            let ratio = if display.ratio == 0. {
                1.
            } else {
                display.ratio
            };
            let typed = entry
                .text
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite());
            edits.extend(typed.map(|v| set(Set::Value(snap(*range, v * ratio)))));
        }
        Kind::TextEdit => edits.push(set(Set::Text(entry.text))),
        _ => {}
    }
    None
}

/// Shows `edits` in `tree` at once, before the host's next refresh.
pub fn apply(tree: &mut Interface, edits: &[Edit]) {
    for e in edits {
        let Some(w) = tree.widgets.get_mut(e.widget.0) else {
            continue;
        };
        match (&e.to, &mut w.value) {
            (Set::Text(text), _) => w.text.clone_from(text),
            (Set::Drop { .. }, _) => {}
            (Set::Browse(path, shown), _) => {
                w.text.clone_from(path);
                if let Kind::FileSelector { tops, .. } = &mut w.kind {
                    tops.clone_from(shown);
                }
            }
            (&Set::Cell(k, v), Some(model::Value::Integers(cells))) => {
                if let Some(c) = cells.get_mut(k as usize) {
                    *c = v.round() as i32;
                }
            }
            (&Set::Cell(k, v), Some(model::Value::Reals(cells))) => {
                if let Some(c) = cells.get_mut(k as usize) {
                    *c = v;
                }
            }
            (Set::Cell(..), _) => {}
            (&Set::Value(v), Some(model::Value::Real(_))) => w.value = Some(model::Value::Real(v)),
            (&Set::Value(v), None) if matches!(w.binding, model::Binding::Param(_)) => {
                w.value = Some(model::Value::Real(v));
            }
            (&Set::Value(v), _) => w.value = Some(model::Value::Integer(v.round() as i32)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A knob dragged up half its 200-point sweep moves in the frame that
    /// answers the move, not the one after it: the edit that frame sends,
    /// and the value it draws, are already the new one.
    #[test]
    fn a_drag_follows_the_pointer_of_the_frame_it_answers() {
        let mut tree = Interface::default();
        tree.pages.push(kontra_ui_model::Page {
            size: model::Size {
                width: 200.,
                height: 200.,
            },
            ..Default::default()
        });
        let range = model::Range {
            min: 0.,
            max: 100.,
            default: 0.,
            step: Some(1.),
        };
        let display = model::Display::default();
        let rect = model::Rect::new(10, 10, 50, 50);
        let mut knob = model::Widget::new("$k", PageRef(0), rect, Kind::Knob { range, display });
        knob.value = Some(model::Value::Integer(0));
        tree.widgets.push(knob);
        let (mut ui, mut g) = (crate::ui(), Gesture::default());
        let mut pictures = crate::Pictures::new(Box::new(|_| None));
        let mut frame = |at: Point, down: bool, tree: &mut Interface| {
            g.latest = Some(at);
            let (edits, _) = read(&mut ui, 0, tree, PageRef(0), 1., &mut g);
            apply(tree, &edits);
            let (page, _) =
                crate::view::page(&mut ui, 0, tree, PageRef(0), &mut pictures, (1., &[]));
            let buttons = if down {
                Buttons::PRIMARY
            } else {
                Buttons::default()
            };
            let pointer = PointerInput {
                pos: Some(at),
                buttons,
                ..Default::default()
            };
            let input = Input {
                pointer,
                ..Default::default()
            };
            ui.frame(page, Some(Size::new(200., 200.)), input, 1. / 60.)
                .expect("laid out");
            edits
        };
        let on = Point::new(35., 35.);
        frame(on, false, &mut tree);
        frame(on, true, &mut tree);
        frame(on, true, &mut tree);
        let moved = frame(Point::new(35., -65.), true, &mut tree);
        assert_eq!(
            moved.iter().map(|e| e.to.clone()).collect::<Vec<_>>(),
            [Set::Value(50.)],
            "the move's own frame sends it"
        );
        assert_eq!(tree.widgets[0].number(), 50.);
    }

    /// The reference's own example: an exponential [1; 10000] puts 0, 25,
    /// 50, 75 and 100 % at 1, 10, 100, 1000 and 10000.
    #[test]
    fn mappers_follow_the_reference_formulas() {
        let r = model::Range {
            min: 1.,
            max: 10_000.,
            default: 1.,
            step: None,
        };
        for (pos, v) in [
            (0., 1.),
            (0.25, 10.),
            (0.5, 100.),
            (0.75, 1000.),
            (1., 10_000.),
        ] {
            assert!((mapped(Some("Exponential"), r, pos, false) - v).abs() < 1e-6 * v);
            assert!((mapped(Some("Exponential"), r, v, true) - pos).abs() < 1e-9);
        }
        let r = model::Range {
            min: 0.,
            max: 8.,
            default: 0.,
            step: None,
        };
        assert_eq!(mapped(Some("Cubic"), r, 0.5, false), 1.);
        assert_eq!(mapped(Some("CubeRoot"), r, 0.125, false), 4.);
        assert!((mapped(Some("Quadratic"), r, 2., true) - 0.5).abs() < 1e-12);
    }

    #[test]
    fn wheel_step_is_decoupled_from_notch_lines_and_drag_travel() {
        let range = model::Range {
            min: 0.,
            max: 100.,
            default: 25.,
            step: Some(1.),
        };
        let knob = model::Widget::new(
            "$knob",
            PageRef(0),
            model::Rect::new(0, 0, 40, 40),
            Kind::Knob {
                range,
                display: model::Display::default(),
            },
        );
        // 1 reference line (20 px) moves 10% (25 -> 15).
        assert_eq!(wheel_value(&knob, range, 25., 20., false), 15.);
        // A high-NOTCH_LINES host event (e.g. 10 * 20 = 200 px or 500 px) is
        // capped to the same 10% per notch rather than pegging the control.
        assert_eq!(wheel_value(&knob, range, 50., 200., false), 40.);
        assert_eq!(wheel_value(&knob, range, 50., -500., false), 60.);
        // Shift gives 10x finer wheel resolution (1% per notch).
        assert_eq!(wheel_value(&knob, range, 50., 20., true), 49.);
        assert_eq!(wheel_value(&knob, range, 50., 200., true), 49.);
        // Small trackpad deltas stay smooth and proportional.
        assert_eq!(wheel_value(&knob, range, 50., 4., false), 48.);
        // Zero and non-finite deltas are no-ops.
        assert_eq!(wheel_value(&knob, range, 50., 0., false), 50.);
        assert_eq!(wheel_value(&knob, range, 50., f64::NAN, false), 50.);

        // Coarse stepped knob (0..=4, step 1) never stalls on a wheel notch.
        let stepped = model::Range {
            min: 0.,
            max: 4.,
            default: 0.,
            step: Some(1.),
        };
        assert_eq!(wheel_value(&knob, stepped, 2., -20., false), 3.);
        // A descending range moves one way for small and large deltas alike.
        let desc = model::Range {
            min: 10.,
            max: 0.,
            default: 5.,
            step: Some(1.),
        };
        let small = wheel_value(&knob, desc, 5., -4., false);
        let large = wheel_value(&knob, desc, 5., -20., false);
        assert!(small < 5. && large < 5., "small={small} large={large}");
        assert_eq!(wheel_value(&knob, stepped, 2., 20., false), 1.);

        // ValueEdit steps by its discrete unit notch regardless of NOTCH_LINES.
        let edit_range = model::Range {
            min: 0.,
            max: 127.,
            default: 64.,
            step: Some(1.),
        };
        let edit = model::Widget::new(
            "$edit",
            PageRef(0),
            model::Rect::new(0, 0, 60, 18),
            Kind::ValueEdit {
                range: edit_range,
                display: model::Display::default(),
                arrows: true,
            },
        );
        assert_eq!(wheel_value(&edit, edit_range, 64., -20., false), 65.);
        assert_eq!(wheel_value(&edit, edit_range, 64., -200., false), 65.);
        assert_eq!(wheel_value(&edit, edit_range, 64., 200., false), 63.);
    }
}
