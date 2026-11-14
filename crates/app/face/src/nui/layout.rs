//! A rendered `native_ui` tree laid out as SwiftUI stacks do and lowered
//! into widgets the renderer already draws: a rectangle or ring as a
//! [`ui::Kind::Shape`], an image as an image, a text as a label. A node's
//! modifiers wrap it from the inside out. Each component instance's root
//! is recorded where it was placed ([`Slot`]), so a re-render goes back
//! into the same room.

use kontra_lua::nui::{Node, Prop};
use kontra_ui_model as ui;

use super::super::Builder;

/// What a node draws with, inherited from the modifiers around it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Env {
    font: f64,
    color: [f64; 4],
    opacity: f64,
}

/// A size, or an offer of room along each axis.
type Size = (f64, f64);

pub(super) fn lower(root: &Node, size: ui::Size, bld: &mut Builder<'_>) {
    let env = Env {
        font: 12.,
        color: [255., 255., 255., 1.],
        opacity: 1.,
    };
    let r = ui::Rect {
        x: 0.,
        y: 0.,
        width: size.width,
        height: size.height,
    };
    place(root, root.mods.len(), r, env, bld);
}

fn num(p: Option<&Prop>) -> Option<f64> {
    p.and_then(Prop::number)
}

fn text(p: Option<&Prop>) -> Option<&str> {
    match p {
        Some(Prop::Text(t)) => Some(t),
        _ => None,
    }
}

/// Field `f` of a modifier's first argument.
fn arg<'a>(args: &'a [Prop], f: &str) -> Option<&'a Prop> {
    args.first().and_then(|a| a.field(f))
}

/// Edge insets of a `padding`, left, top, right, bottom: one number for
/// all, or named sides.
fn pads(args: &[Prop]) -> [f64; 4] {
    if let Some(n) = num(args.first()) {
        return [n; 4];
    }
    let f = |k: &str| num(arg(args, k));
    let (h, v) = (f("horizontal").unwrap_or(0.), f("vertical").unwrap_or(0.));
    [
        f("left").unwrap_or(h),
        f("top").unwrap_or(v),
        f("right").unwrap_or(h),
        f("bottom").unwrap_or(v),
    ]
}

/// Where a smaller thing sits in its room, 0 to 1 along each axis, from
/// words like `"top center"` or `"trailing"`.
fn alignment(words: Option<&str>) -> Size {
    let w = words.unwrap_or("center");
    let has = |k: &[&str]| k.iter().any(|k| w.contains(k));
    let x = if has(&["leading", "left"]) {
        0.
    } else if has(&["trailing", "right"]) {
        1.
    } else {
        0.5
    };
    let y = if has(&["top"]) {
        0.
    } else if has(&["bottom"]) {
        1.
    } else {
        0.5
    };
    (x, y)
}

fn aligned(r: ui::Rect, (w, h): Size, (ax, ay): Size) -> ui::Rect {
    ui::Rect {
        x: r.x + (r.width - w) * ax,
        y: r.y + (r.height - h) * ay,
        width: w,
        height: h,
    }
}

/// The environment inside modifier `m`.
fn env_in(env: Env, m: &str, args: &[Prop]) -> Env {
    match (m, args.first()) {
        ("font_size", Some(Prop::Number(n))) => Env { font: *n, ..env },
        ("foreground_color", Some(Prop::Color(c))) => Env { color: *c, ..env },
        ("opacity", Some(Prop::Number(n))) => Env {
            opacity: env.opacity * n,
            ..env
        },
        _ => env,
    }
}

/// `n`'s children, those of a `ForEach` among them in its place.
fn children(n: &Node) -> Vec<&Node> {
    n.children
        .iter()
        .flat_map(|c| match c.kind.as_str() {
            "ForEach" => c.children.iter().collect(),
            _ => vec![c],
        })
        .collect()
}

/// The size layer `k` of `n` (its first `k` modifiers) takes in `room`.
/// Zero along an axis means it takes what it is given.
fn measure(bld: &mut Builder<'_>, n: &Node, k: usize, room: Size, env: Env) -> Size {
    let Some((m, args)) = k.checked_sub(1).and_then(|i| n.mods.get(i)) else {
        return primitive_size(bld, n, room, env);
    };
    let mut inner = |room| measure(bld, n, k - 1, room, env_in(env, m, args));
    match m.as_str() {
        "frame" => {
            let (w, h) = (num(arg(args, "width")), num(arg(args, "height")));
            let (iw, ih) = inner((w.unwrap_or(room.0), h.unwrap_or(room.1)));
            (w.unwrap_or(iw), h.unwrap_or(ih))
        }
        "padding" => {
            let [l, t, r, b] = pads(args);
            let (iw, ih) = inner((room.0 - l - r, room.1 - t - b));
            (
                if iw > 0. { iw + l + r } else { 0. },
                if ih > 0. { ih + t + b } else { 0. },
            )
        }
        // It fills the room it is offered and sits where it says in it.
        "align" => (0., 0.),
        _ => inner(room),
    }
}

fn primitive_size(bld: &mut Builder<'_>, n: &Node, room: Size, env: Env) -> Size {
    match n.kind.as_str() {
        "Spacer" => (0., 0.),
        // ponytail: an average glyph width; measure with the font when a
        // page's layout turns on it.
        "Text" | "TextInput" => (
            words(n).chars().count() as f64 * env.font * 0.55,
            env.font * 1.25,
        ),
        "Image" if !matches!(n.prop("resizable"), Some(Prop::Bool(true))) => {
            image(bld, n).and_then(|(_, s)| s).unwrap_or((0., 0.))
        }
        // As big as its biggest child, but flexible along an axis where an
        // aligned child is (it fills its room): Conflux's source row aligns
        // its two selectors left and right of the knob in one ZStack.
        "ZStack" | "ForEach" => {
            let mut flex = false;
            let size = children(n).iter().fold((0f64, 0f64), |(w, h), c| {
                let (cw, ch) = measure(bld, c, c.mods.len(), room, env);
                flex |= fills(c);
                (w.max(cw), h.max(ch))
            });
            if flex { (0., 0.) } else { size }
        }
        "HStack" => stack(bld, n, Axis::Across, room, env).1,
        "VStack" => stack(bld, n, Axis::Down, room, env).1,
        _ => (0., 0.),
    }
}

/// Whether `n` fills the room it is offered: its outermost size-giving
/// modifier is an `align` (offsets and looks keep their child's size).
fn fills(n: &Node) -> bool {
    n.mods.iter().rev().map(|(m, _)| m.as_str()).find(|m| {
        !matches!(
            *m,
            "offset"
                | "opacity"
                | "hidden"
                | "foreground_color"
                | "font_size"
                | "bold"
                | "background"
                | "overlay"
                | "rotation"
                | "on_hover_gesture"
                | "on_tap_gesture"
                | "on_drag_gesture"
                | "disabled"
                | "popover"
        )
    }) == Some("align")
}

fn words(n: &Node) -> String {
    match n.prop("text") {
        Some(Prop::Text(t)) => t.clone(),
        Some(Prop::Number(v)) => v.to_string(),
        _ => String::new(),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Axis {
    Across,
    Down,
}

impl Axis {
    fn along(self, s: Size) -> f64 {
        if self == Self::Across { s.0 } else { s.1 }
    }
    fn across(self, s: Size) -> f64 {
        if self == Self::Across { s.1 } else { s.0 }
    }
    /// A size from its length along the axis and across it.
    fn size(self, along: f64, across: f64) -> Size {
        if self == Self::Across {
            (along, across)
        } else {
            (across, along)
        }
    }
}

/// A stack along `axis` in `room`: each child's length along it, and the
/// stack's size. Children with a length of their own keep it; spacers and
/// children that take what they are given share what is left.
fn stack(bld: &mut Builder<'_>, n: &Node, axis: Axis, room: Size, env: Env) -> (Vec<f64>, Size) {
    let kids = children(n);
    let spacing = num(n.prop("spacing")).unwrap_or(0.);
    let sizes: Vec<Size> = kids
        .iter()
        .map(|c| measure(bld, c, c.mods.len(), axis.size(0., axis.across(room)), env))
        .collect();
    let gaps = spacing * kids.len().saturating_sub(1) as f64;
    // What is left goes to the children that fill their room; a spacer
    // takes a share only when none does (Conflux's slot content fills its
    // module above the spacer under it).
    let fills = kids
        .iter()
        .zip(&sizes)
        .any(|(c, s)| axis.along(*s) <= 0. && c.kind != "Spacer");
    let grows: Vec<bool> = kids
        .iter()
        .zip(&sizes)
        .map(|(c, s)| axis.along(*s) <= 0. && (!fills || c.kind != "Spacer"))
        .collect();
    let flexible = |i: usize| grows.get(i).copied().unwrap_or(false);
    let shares = grows.iter().filter(|g| **g).count();
    let fixed: f64 = sizes.iter().map(|s| axis.along(*s)).sum::<f64>() + gaps;
    let share = if shares == 0 {
        0.
    } else {
        (axis.along(room) - fixed).max(0.) / shares as f64
    };
    let lengths: Vec<f64> = sizes
        .iter()
        .enumerate()
        .map(|(i, s)| {
            if flexible(i) {
                share
            } else {
                axis.along(*s).max(0.)
            }
        })
        .collect();
    // A child that fills across (a shape, a ZStack of aligned children)
    // takes the room's whole width: Conflux's wide FX slot is a framed
    // VStack as wide as its frame, not as its header.
    let thick = sizes.iter().map(|s| axis.across(*s)).fold(0., f64::max);
    let thick = if kids.iter().zip(&sizes).any(|(c, s)| {
        axis.across(*s) <= 0. && !matches!(c.kind.as_str(), "Spacer" | "Text" | "TextInput")
    }) {
        thick.max(axis.across(room))
    } else {
        thick
    };
    let length = if shares > 0 {
        axis.along(room).max(fixed)
    } else {
        fixed
    };
    (lengths, axis.size(length, thick))
}

/// Where a component instance's root was placed: its room and what it
/// inherited, its widgets (`start..end`), and how many slots inside it
/// follow it in [`Builder::nui`].
#[derive(Clone, Debug)]
pub(crate) struct Slot {
    pub(super) path: String,
    pub(super) rect: ui::Rect,
    pub(super) env: Env,
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) inner: usize,
    /// The parameters it read, and whether it is still being placed.
    reads: Vec<String>,
    open: bool,
    /// The layer its widgets draw on: 1 inside a popover's content.
    z: i32,
}

/// Places layer `k` of `n` in `r`, appending what it draws; a whole
/// instance root is recorded as a [`Slot`].
pub(super) fn place(n: &Node, k: usize, r: ui::Rect, env: Env, bld: &mut Builder<'_>) {
    let Some(path) = n.instance.as_ref().filter(|_| k == n.mods.len()) else {
        return place_layer(n, k, r, env, bld);
    };
    let (start, at) = (bld.ui.widgets.len(), bld.nui.len());
    bld.nui.push(Slot {
        path: path.clone(),
        rect: r,
        env,
        start,
        end: start,
        inner: 0,
        reads: n.reads.clone(),
        open: true,
        z: 0,
    });
    place_layer(n, k, r, env, bld);
    let (end, inner) = (bld.ui.widgets.len(), bld.nui.len() - at - 1);
    if let Some(s) = bld.nui.get_mut(at) {
        (s.end, s.inner, s.open) = (end, inner, false);
    }
}

/// Puts instance `path`'s new tree (`None`: it draws nothing now) in the
/// room its last one had, in place of that one's widgets and slots; the
/// root (`""`) is the whole page again from widget `first` on.
pub(super) fn splice(
    bld: &mut Builder<'_>,
    first: usize,
    size: ui::Size,
    path: &str,
    node: Option<&Node>,
) {
    if path.is_empty() {
        bld.ui.widgets.truncate(first);
        bld.nui.clear();
        if let Some(n) = node {
            lower(n, size, bld);
        }
        return;
    }
    let Some(i) = bld.nui.iter().position(|s| s.path == path) else {
        return;
    };
    let Some(old) = bld.nui.get(i).cloned() else {
        return;
    };
    let tail = bld.ui.widgets.split_off(old.end);
    bld.ui.widgets.truncate(old.start);
    let after = bld.nui.split_off(i + 1 + old.inner);
    bld.nui.truncate(i);
    match node {
        Some(n) => {
            place(n, n.mods.len(), old.rect, old.env, bld);
            // Rebuilt on the layer it was on (a popover's content stays over the page).
            for w in bld.ui.widgets.iter_mut().skip(old.start) {
                w.z = old.z;
            }
            for s in bld.nui.iter_mut().skip(i) {
                s.z = old.z;
            }
        }
        None => bld.nui.push(Slot {
            end: old.start,
            inner: 0,
            ..old.clone()
        }),
    }
    let shift = bld.ui.widgets.len().wrapping_sub(old.end);
    let grown = (bld.nui.len() - i - 1).wrapping_sub(old.inner);
    bld.ui.widgets.extend(tail);
    bld.nui.extend(after.into_iter().map(|s| Slot {
        start: s.start.wrapping_add(shift),
        end: s.end.wrapping_add(shift),
        ..s
    }));
    // The slots around it end where it now ends, and hold what it holds.
    for (j, s) in bld.nui.iter_mut().enumerate().take(i) {
        if j + s.inner >= i {
            s.end = s.end.wrapping_add(shift);
            s.inner = s.inner.wrapping_add(grown);
        }
    }
}

fn place_layer(n: &Node, k: usize, r: ui::Rect, env: Env, bld: &mut Builder<'_>) {
    let Some((m, args)) = k.checked_sub(1).and_then(|i| n.mods.get(i)) else {
        return primitive(n, r, env, bld);
    };
    let inner_env = env_in(env, m, args);
    let size = |bld: &mut Builder<'_>, r: ui::Rect| {
        let s = measure(bld, n, k - 1, (r.width, r.height), inner_env);
        or_room(s, r)
    };
    let r = match m.as_str() {
        "hidden" if matches!(args.first(), Some(Prop::Bool(true)) | None) => return,
        // Turned about the centre of its room, as SwiftUI's rotation
        // effect: each widget's centre swings about it, and the widget
        // turns as much about its own.
        "rotation" => {
            let start = bld.ui.widgets.len();
            place_layer(n, k - 1, r, inner_env, bld);
            let angle = num(args.first()).unwrap_or(0.);
            let (cx, cy) = (r.x + r.width / 2., r.y + r.height / 2.);
            let (sin, cos) = angle.to_radians().sin_cos();
            for w in bld.ui.widgets.iter_mut().skip(start) {
                let (dx, dy) = (
                    w.rect.x + w.rect.width / 2. - cx,
                    w.rect.y + w.rect.height / 2. - cy,
                );
                w.rect.x = cx + dx * cos - dy * sin - w.rect.width / 2.;
                w.rect.y = cy + dx * sin + dy * cos - w.rect.height / 2.;
                w.rotation += angle;
            }
            return;
        }
        "frame" => {
            let w = num(arg(args, "width")).unwrap_or(r.width);
            let h = num(arg(args, "height")).unwrap_or(r.height);
            let room = aligned(r, (w, h), (0.5, 0.5));
            aligned(
                room,
                size(bld, room),
                alignment(text(arg(args, "alignment"))),
            )
        }
        "padding" => {
            let [l, t, rr, b] = pads(args);
            ui::Rect {
                x: r.x + l,
                y: r.y + t,
                width: (r.width - l - rr).max(0.),
                height: (r.height - t - b).max(0.),
            }
        }
        "offset" => ui::Rect {
            x: r.x + num(arg(args, "x")).unwrap_or(0.),
            y: r.y + num(arg(args, "y")).unwrap_or(0.),
            ..r
        },
        "align" => aligned(r, size(bld, r), alignment(text(args.first()))),
        "position" => {
            let (w, h) = size(bld, r);
            ui::Rect {
                x: r.x + num(arg(args, "x")).unwrap_or(0.) - w / 2.,
                y: r.y + num(arg(args, "y")).unwrap_or(0.) - h / 2.,
                width: w,
                height: h,
            }
        }
        "popover" => {
            let first = bld.ui.widgets.len();
            place(n, k - 1, r, inner_env, bld);
            // Anchored to where what it wraps was placed (its own align and
            // offset included); measured and centred if it drew nothing.
            let anchor = bounds(bld.ui.widgets.iter().skip(first).map(|w| w.rect))
                .unwrap_or_else(|| aligned(r, size(bld, r), (0.5, 0.5)));
            popover(bld, args, anchor, env);
            return;
        }
        // Its area goes under what it wraps: a press goes to the innermost
        // gesture, as in SwiftUI (Conflux's selectors open their popup from
        // an inner tap inside an outer one that only reports a touch).
        "on_drag_gesture" | "on_tap_gesture" => {
            gesture(bld, m, args, r);
            place_layer(n, k - 1, r, inner_env, bld);
            return;
        }
        "background" | "overlay" => {
            let other = |bld: &mut Builder<'_>| {
                if let Some(Prop::Node(b)) = args.first() {
                    place(b, b.mods.len(), r, env, bld);
                }
            };
            if m == "background" {
                other(bld);
                place(n, k - 1, r, inner_env, bld);
            } else {
                place(n, k - 1, r, inner_env, bld);
                other(bld);
            }
            return;
        }
        _ => r,
    };
    place(n, k - 1, r, inner_env, bld);
}

/// A shown `.popover(visible, content, direction, alignment, spacing,
/// auto_close_when, on_auto_close)` beside its anchor `a`: its content at
/// its own size, over the whole page (`z` 1), and a press anywhere but on
/// the anchor or the content runs `on_auto_close` (SwiftUI's popover; the
/// anchor keeps its own tap, so it can close what it opened).
fn popover(bld: &mut Builder<'_>, args: &[Prop], a: ui::Rect, env: Env) {
    let (Some(Prop::Bool(true)), Some(Prop::Node(content))) =
        (arg(args, "visible"), arg(args, "content"))
    else {
        return;
    };
    let page = bld
        .ui
        .pages
        .first()
        .map_or((970., 600.), |p| (p.size.width, p.size.height));
    let (start, slots) = (bld.ui.widgets.len(), bld.nui.len());
    let (w, h) = or_room(
        measure(bld, content, content.mods.len(), page, env),
        ui::Rect {
            x: 0.,
            y: 0.,
            width: a.width,
            height: a.height,
        },
    );
    // No larger than the page.
    // ponytail: content taller than the page is cut, not scrolled.
    let (w, h) = (w.min(page.0), h.min(page.1));
    let gap = num(arg(args, "spacing")).unwrap_or(0.);
    let (ax, ay) = alignment(text(arg(args, "alignment")));
    let along_x = a.x + (a.width - w) * ax;
    let along_y = a.y + (a.height - h) * ay;
    let (x, y) = match text(arg(args, "direction")).unwrap_or("up") {
        "down" => (along_x, a.y + a.height + gap),
        "left" => (a.x - gap - w, along_y),
        "right" => (a.x + a.width + gap, along_y),
        _ => (along_x, a.y - gap - h),
    };
    // Kept on the page, as a popover is.
    let c = ui::Rect {
        x: x.min(page.0 - w).max(0.),
        y: y.min(page.1 - h).max(0.),
        width: w,
        height: h,
    };
    if text(arg(args, "auto_close_when")) != Some("never")
        && let Some(Prop::Callback(close)) = arg(args, "on_auto_close")
    {
        // A press anywhere but on the anchor or the content closes it.
        for r in outside(page, [a, c]) {
            let mut w = ui::Widget::new(
                format!("{} {close} 0", super::TAP),
                ui::PageRef(0),
                r,
                ui::Kind::Button { momentary: true },
            );
            w.opacity = 0.;
            bld.ui.widgets.push(w);
        }
    }
    place(content, content.mods.len(), c, env, bld);
    for w in bld.ui.widgets.iter_mut().skip(start) {
        w.z = 1;
    }
    for s in bld.nui.iter_mut().skip(slots) {
        s.z = 1;
    }
}

/// The smallest rect around `rects`, if any has area.
fn bounds(rects: impl Iterator<Item = ui::Rect>) -> Option<ui::Rect> {
    rects
        .filter(|r| r.width > 0. && r.height > 0.)
        .map(|r| (r.x, r.y, r.x + r.width, r.y + r.height))
        .reduce(|a, b| (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)))
        .map(|(x0, y0, x1, y1)| ui::Rect {
            x: x0,
            y: y0,
            width: x1 - x0,
            height: y1 - y0,
        })
}

/// The page of `size` less `holes`, as rows of rects: the grid their
/// edges cut, each row's free cells joined.
fn outside(size: (f64, f64), holes: [ui::Rect; 2]) -> Vec<ui::Rect> {
    let cuts = |edges: [f64; 4], end: f64| {
        let mut v: Vec<f64> = [0., end]
            .into_iter()
            .chain(edges.into_iter().map(|e| e.clamp(0., end)))
            .collect();
        v.sort_by(f64::total_cmp);
        v.dedup();
        v
    };
    let [p, q] = holes;
    let xs = cuts([p.x, p.x + p.width, q.x, q.x + q.width], size.0);
    let ys = cuts([p.y, p.y + p.height, q.y, q.y + q.height], size.1);
    let inside = |r: &ui::Rect, x: f64, y: f64| {
        (r.x..r.x + r.width).contains(&x) && (r.y..r.y + r.height).contains(&y)
    };
    let span = |x0: f64, y0: f64, x1: f64, y1: f64| ui::Rect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    };
    let mut out = Vec::new();
    for row in ys.windows(2) {
        let &[y0, y1] = row else { continue };
        let mut run: Option<f64> = None;
        for col in xs.windows(2) {
            let &[x0, x1] = col else { continue };
            let (mx, my) = ((x0 + x1) / 2., (y0 + y1) / 2.);
            let free = !holes.iter().any(|h| inside(h, mx, my));
            match (free, run) {
                (true, None) => run = Some(x0),
                (false, Some(from)) => {
                    out.push(span(from, y0, x0, y1));
                    run = None;
                }
                _ => {}
            }
        }
        if let (Some(from), Some(&end)) = (run, xs.last()) {
            out.push(span(from, y0, end, y1));
        }
    }
    out
}

/// The control a drag in the instance being placed moves: the innermost
/// open instance that read a ranged control's parameter. Its own (hidden)
/// widget, which the editor drags as it would that control.
fn dragged(bld: &Builder<'_>) -> Option<ui::Widget> {
    let open = bld.nui.iter().rev().filter(|s| s.open);
    open.flat_map(|s| &s.reads)
        .find_map(|id| {
            bld.ui.widgets.iter().find(|w| {
                w.hidden
                    && super::id(&w.name) == id
                    && matches!(
                        w.kind,
                        ui::Kind::Knob { .. }
                            | ui::Kind::Slider { .. }
                            | ui::Kind::ValueEdit { .. }
                    )
            })
        })
        .cloned()
}

/// A gesture's area over what it modifies, drawn as nothing: a drag
/// drives its control's own widget; a tap is a momentary button the host
/// answers with the page's own callbacks ([`super::TAP`]).
fn gesture(bld: &mut Builder<'_>, m: &str, args: &[Prop], r: ui::Rect) {
    let callback = |k: &str| match arg(args, k) {
        Some(Prop::Callback(n)) => *n,
        _ => 0,
    };
    let w = match m {
        "on_drag_gesture" => dragged(bld).map(|w| ui::Widget {
            rect: r,
            auto_size: false,
            default_axes: [false; 2],
            hidden: false,
            parent: None,
            images: Vec::new(),
            z: 0,
            ..w
        }),
        _ if callback("start") + callback("complete") > 0 => Some(ui::Widget::new(
            format!(
                "{} {} {}",
                super::TAP,
                callback("start"),
                callback("complete")
            ),
            ui::PageRef(0),
            r,
            ui::Kind::Button { momentary: true },
        )),
        _ => None,
    };
    if let Some(mut w) = w.filter(|_| r.width > 0. && r.height > 0.) {
        w.opacity = 0.;
        bld.ui.widgets.push(w);
    }
}

/// What takes what it is given (zero) takes `r`'s size along that axis.
fn or_room((w, h): Size, r: ui::Rect) -> Size {
    (
        if w > 0. { w } else { r.width },
        if h > 0. { h } else { r.height },
    )
}

/// An image node's asset, which stretches to the widget it shows in, and
/// its own size.
fn image(bld: &mut Builder<'_>, n: &Node) -> Option<(ui::AssetRef, Option<Size>)> {
    let file = text(n.prop("file"))?;
    let asset = bld.file_asset(format!("native_ui/{file}"));
    let ui::AssetKind::Image(meta) = &mut bld.ui.assets.get_mut(asset.0)?.kind else {
        return None;
    };
    meta.stretch = [true; 2];
    Some((asset, meta.size.map(|s| (s.width, s.height))))
}

fn rgba(c: [f64; 4], opacity: f64) -> ui::Rgba {
    let ch = |v: f64| v.clamp(0., 255.) as u8;
    ui::Rgba {
        r: ch(c[0]),
        g: ch(c[1]),
        b: ch(c[2]),
        a: ch(c[3] * opacity * 255.),
    }
}

/// Places a primitive: a stack's children, or one widget.
fn primitive(n: &Node, r: ui::Rect, env: Env, bld: &mut Builder<'_>) {
    let along = |axis| {
        let a = alignment(text(n.prop("alignment")));
        if axis == Axis::Across {
            (0., a.1)
        } else {
            (a.0, 0.)
        }
    };
    let kind = match n.kind.as_str() {
        "Rectangle" => ui::Kind::Shape(ui::Shape::Rect {
            radius: num(n.prop("corner_radius")).unwrap_or(0.),
        }),
        "Ring" => {
            let sweep = num(n.prop("angle")).unwrap_or(360.);
            if sweep.abs() < 0.5 {
                return;
            }
            ui::Kind::Shape(ui::Shape::Ring {
                thickness: num(n.prop("thickness")).unwrap_or(1.),
                start: num(n.prop("start_angle")).unwrap_or(0.),
                sweep,
            })
        }
        "Text" | "TextInput" => ui::Kind::Label,
        "Image" => ui::Kind::Image,
        "Canvas" => ui::Kind::Canvas(Vec::new()),
        "ZStack" | "ForEach" => {
            let a = alignment(text(n.prop("alignment")));
            for c in children(n) {
                let size = or_room(measure(bld, c, c.mods.len(), (r.width, r.height), env), r);
                place(c, c.mods.len(), aligned(r, size, a), env, bld);
            }
            return;
        }
        "HStack" | "VStack" => {
            let axis = if n.kind == "HStack" {
                Axis::Across
            } else {
                Axis::Down
            };
            let (lengths, _) = stack(bld, n, axis, (r.width, r.height), env);
            let spacing = num(n.prop("spacing")).unwrap_or(0.);
            let mut at = 0.;
            for (c, len) in children(n).into_iter().zip(lengths) {
                let slot = match axis {
                    Axis::Across => ui::Rect {
                        x: r.x + at,
                        width: len,
                        ..r
                    },
                    Axis::Down => ui::Rect {
                        y: r.y + at,
                        height: len,
                        ..r
                    },
                };
                let thick = axis.across(measure(
                    bld,
                    c,
                    c.mods.len(),
                    (slot.width, slot.height),
                    env,
                ));
                let thick = if thick > 0. {
                    thick
                } else {
                    axis.across((r.width, r.height))
                };
                place(
                    c,
                    c.mods.len(),
                    aligned(slot, axis.size(len, thick), along(axis)),
                    env,
                    bld,
                );
                at += len + spacing;
            }
            return;
        }
        _ => return,
    };
    if r.width <= 0. || r.height <= 0. || env.opacity <= 0. {
        return;
    }
    // A text is never cut to its room: it overhangs it, centred, by an em
    // a character and one more for the label's inset (a lone "0" lost its
    // right half). The label draws it centred, so the slack is free.
    let r = match n.kind.as_str() {
        "Text" | "TextInput" => {
            let em = (words(n).chars().count() + 1) as f64 * env.font;
            aligned(
                r,
                (em.max(r.width), (env.font * 1.5).max(r.height)),
                (0.5, 0.5),
            )
        }
        _ => r,
    };
    let mut w = ui::Widget::new(n.kind.clone(), ui::PageRef(0), r, kind);
    w.opacity = env.opacity as f32;
    w.colors.background = match n.prop("color") {
        Some(Prop::Color(c)) => Some(rgba(*c, 1.)),
        _ => None,
    };
    match n.kind.as_str() {
        "Text" | "TextInput" => {
            w.text = words(n);
            w.colors.background = None;
            let style = ui::TextStyle {
                font: ui::Font::Default,
                size: Some(env.font as f32),
                color: rgba(env.color, 1.),
                align: ui::Align::Center,
            };
            let at = bld.ui.styles.iter().position(|s| *s == style);
            w.style = Some(ui::StyleRef(at.unwrap_or_else(|| {
                bld.ui.styles.push(style);
                bld.ui.styles.len() - 1
            })));
        }
        "Image" => {
            let Some((asset, _)) = image(bld, n) else {
                return;
            };
            w.images
                .push(ui::ImageUse::new(asset, ui::Role::Background));
        }
        "Canvas" => match n.prop("paint") {
            Some(Prop::Callback(c)) => w.name = format!("{} {c}", super::CANVAS),
            _ => return,
        },
        _ if w.colors.background.is_none() => return,
        _ => {}
    }
    bld.ui.widgets.push(w);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(kind: &str, mods: Vec<(&str, Vec<Prop>)>, children: Vec<Node>) -> Node {
        Node {
            kind: kind.into(),
            props: vec![("color".into(), Prop::Color([255., 0., 0., 1.]))],
            mods: mods.into_iter().map(|(m, a)| (m.into(), a)).collect(),
            children,
            instance: None,
            reads: Vec::new(),
        }
    }

    #[test]
    fn an_aligned_child_fills_its_room_and_sits_where_it_says() {
        let size = |w: f64, h: f64| {
            let named = vec![
                ("width".into(), Prop::Number(w)),
                ("height".into(), Prop::Number(h)),
            ];
            ("frame", vec![Prop::Table(named)])
        };
        let at = |words: &str| ("align", vec![Prop::Text(words.into())]);
        let panel = |words| node("Rectangle", vec![size(30., 20.), at(words)], Vec::new());
        // A page framed at its top centre, as a whole panel is.
        let framed = vec![
            ("width".into(), Prop::Number(300.)),
            ("height".into(), Prop::Number(200.)),
            ("alignment".into(), Prop::Text("top center".into())),
        ];
        let page = node(
            "ZStack",
            vec![("frame", vec![Prop::Table(framed)])],
            vec![panel("top left"), panel("bottom right")],
        );
        let picture = |_: &str| None;
        let mut bld = Builder {
            ui: ui::Interface::default(),
            assets: std::collections::HashMap::new(),
            styles: std::collections::HashMap::new(),
            picture: &picture,
            nui: Vec::new(),
        };
        lower(
            &page,
            ui::Size {
                width: 600.,
                height: 400.,
            },
            &mut bld,
        );
        let at: Vec<_> = bld
            .ui
            .widgets
            .iter()
            .map(|w| (w.rect.x, w.rect.y))
            .collect();
        assert_eq!(at, [(150., 100.), (420., 280.)]);
    }

    /// Conflux's module slot: a framed VStack of a header text, content
    /// that is a ZStack of aligned panels, and a spacer under it.
    #[test]
    fn a_framed_slot_gives_its_aligned_content_its_whole_width_and_the_leftover() {
        let num = |k: &str, v: f64| (k.to_owned(), Prop::Number(v));
        let at = |words: &str| ("align", vec![Prop::Text(words.into())]);
        let small = (
            "frame",
            vec![Prop::Table(vec![num("width", 30.), num("height", 20.)])],
        );
        let panel = |words| node("Rectangle", vec![small.clone(), at(words)], Vec::new());
        let mut header = node("Text", Vec::new(), Vec::new());
        header.props = vec![("text".into(), Prop::Text("FX".into()))];
        let content = node(
            "ZStack",
            Vec::new(),
            vec![panel("top left"), panel("bottom right")],
        );
        let slot = node(
            "VStack",
            vec![(
                "frame",
                vec![Prop::Table(vec![num("width", 228.), num("height", 208.)])],
            )],
            vec![header, content, node("Spacer", Vec::new(), Vec::new())],
        );
        let picture = |_: &str| None;
        let mut bld = Builder {
            ui: ui::Interface::default(),
            assets: std::collections::HashMap::new(),
            styles: std::collections::HashMap::new(),
            picture: &picture,
            nui: Vec::new(),
        };
        let room = ui::Size {
            width: 228.,
            height: 208.,
        };
        lower(&slot, room, &mut bld);
        let panels: Vec<_> = (bld.ui.widgets.iter())
            .filter(|w| matches!(w.kind, ui::Kind::Shape(_)))
            .map(|w| (w.rect.x, w.rect.y + w.rect.height))
            .collect();
        // Left and right edges of the frame, and the content takes all the
        // height under the header: the spacer gets none.
        assert_eq!(panels, [(0., panels[0].1), (198., 208.)]);
    }

    /// An open popover draws its content beside its anchor over the page
    /// (`z` 1), with a press outside both closing it; a shut one draws none.
    #[test]
    fn an_open_popover_sits_under_its_anchor_over_the_page() {
        let num = |k: &str, v: f64| (k.to_owned(), Prop::Number(v));
        let frame = |w, h| {
            (
                "frame",
                vec![Prop::Table(vec![num("width", w), num("height", h)])],
            )
        };
        let content = node("Rectangle", vec![frame(40., 30.)], Vec::new());
        let popover = |open| {
            let args = vec![
                ("visible".to_owned(), Prop::Bool(open)),
                ("content".to_owned(), Prop::Node(Box::new(content.clone()))),
                ("direction".to_owned(), Prop::Text("down".into())),
                num("spacing", 2.),
                ("on_auto_close".to_owned(), Prop::Callback(9)),
            ];
            node(
                "Rectangle",
                vec![frame(20., 10.), ("popover", vec![Prop::Table(args)])],
                Vec::new(),
            )
        };
        let picture = |_: &str| None;
        let lowered = |n: &Node| {
            let mut bld = Builder {
                ui: ui::Interface::default(),
                assets: std::collections::HashMap::new(),
                styles: std::collections::HashMap::new(),
                picture: &picture,
                nui: Vec::new(),
            };
            let room = ui::Size {
                width: 100.,
                height: 100.,
            };
            lower(n, room, &mut bld);
            bld.ui.widgets
        };
        assert_eq!(lowered(&popover(false)).len(), 1);
        let open = lowered(&popover(true));
        let shown: Vec<_> = open
            .iter()
            .filter(|w| w.z == 1)
            .map(|w| (w.name.as_str(), w.rect))
            .collect();
        // The anchor (20 by 10) is centred at 40,45: its popover 2 under it.
        assert!(
            shown
                .iter()
                .any(|(n, r)| *n == "Rectangle" && (r.x, r.y, r.width) == (30., 57., 40.))
        );
        // The close areas cover the page but the anchor and the content:
        // a press on either never closes it.
        let content = ui::Rect::new(30, 57, 40, 30);
        let anchor = ui::Rect::new(40, 45, 20, 10);
        let strips: Vec<_> = (shown.iter())
            .filter(|(n, _)| n.ends_with(" 9 0"))
            .map(|(_, r)| *r)
            .collect();
        let overlaps = |a: &ui::Rect, b: &ui::Rect| {
            a.x < b.x + b.width
                && b.x < a.x + a.width
                && a.y < b.y + b.height
                && b.y < a.y + a.height
        };
        assert!(
            !strips
                .iter()
                .any(|s| overlaps(s, &content) || overlaps(s, &anchor))
        );
        let area: f64 = strips.iter().map(|r| r.width * r.height).sum();
        assert!((area + 40. * 30. + 20. * 10. - 970. * 600.).abs() < 1e-6);
    }
}
