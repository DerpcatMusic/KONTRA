//! A Kontakt 8 `native_ui` page laid out into the tree. The page renders to
//! primitives ([`kontra_lua::nui`]); this places them as SwiftUI stacks do
//! and lowers each into a widget the renderer already draws: a rectangle or
//! ring as a [`ui::Kind::Shape`], an image as an image, a text as a label.
//! A node's modifiers wrap it from the inside out.
//!
//! Not copied: `Canvas` paths, `TextInput`, `rotation` (a knob's pointer
//! stays upright), gestures. They lay out but draw nothing.

use kontra_ksp::ControlKind;
use kontra_lua::nui::{self, Control, Node, Prop};
use kontra_ui_model as ui;

use super::{Builder, Decl};

/// A page's modules (path under `native_ui/`, source) and its entry.
#[derive(Clone, Debug, Default)]
pub struct Native {
    modules: Vec<(String, String)>,
    entry: String,
}

impl Native {
    /// The page among a script's library files, if it loads one.
    pub(super) fn of(data: &[(String, Vec<u8>)]) -> Option<Self> {
        let (_, entry) = data.iter().find(|(n, _)| n == "native_ui.entry")?;
        let modules = data
            .iter()
            .filter_map(|(n, bytes)| {
                let path = n.strip_prefix("native_ui/")?;
                Some((path.to_owned(), String::from_utf8_lossy(bytes).into_owned()))
            })
            .collect();
        Some(Self {
            modules,
            entry: String::from_utf8_lossy(entry).into_owned(),
        })
    }
}

/// The page's id of a control: its script name without the sigil.
fn id(name: &str) -> &str {
    name.trim_start_matches(['$', '%', '@', '~', '?', '!'])
}

fn control(d: &Decl<'_>) -> Control {
    let range = matches!(
        d.kind,
        ControlKind::Knob | ControlKind::Slider | ControlKind::ValueEdit
    );
    let par = |i: usize| range.then(|| d.params.get(i)).flatten().map(|&v| f64::from(v));
    let label = d.view.and_then(|v| {
        let t = |k: &str| v.texts.iter().find(|(p, _)| p == k).map(|(_, t)| t.clone());
        t("$CONTROL_PAR_LABEL").or_else(|| t("$CONTROL_PAR_TEXT"))
    });
    let entries = if d.look.menu.is_empty() {
        let menu = d.view.map(|v| v.menu.as_slice()).unwrap_or_default();
        menu.iter().map(|(t, v, _)| (t.clone(), f64::from(*v))).collect()
    } else {
        let menu = d.look.menu.iter();
        menu.map(|m| (m.text.as_str().to_owned(), f64::from(m.value))).collect()
    };
    let (min, max) = match d.kind {
        ControlKind::Button | ControlKind::Switch => (0., 1.),
        _ => (par(0).unwrap_or(0.), par(1).unwrap_or(1_000_000.)),
    };
    Control {
        name: id(d.name).to_owned(),
        label: label.unwrap_or_else(|| id(d.name).to_owned()),
        min,
        max,
        default: min,
        entries,
    }
}

/// What a page's parameter reads for a control at `v`: a knob, slider or
/// value edit 0 to 1 across its range (the page maps it back through its
/// own range configs), a menu or button its own value.
fn value(d: &Decl<'_>, c: &Control, v: f64) -> f64 {
    match d.kind {
        ControlKind::Knob | ControlKind::Slider | ControlKind::ValueEdit if c.max > c.min => {
            (v - c.min) / (c.max - c.min)
        }
        _ => v,
    }
}

/// Renders `native` over the script's controls and appends what it draws
/// on the page, in place of the controls' own widgets. Whatever the page
/// does not cover is reported, and then it shows nothing of the page.
pub(super) fn face(bld: &mut Builder<'_>, native: &Native, decls: &[Decl<'_>]) {
    let rendered = nui::load(&native.modules, &native.entry).and_then(|mut page| {
        let controls: Vec<_> = decls.iter().map(control).collect();
        page.set_controls(&controls)?;
        let values: Vec<_> = decls
            .iter()
            .zip(&controls)
            .filter_map(|(d, c)| Some((c.name.clone(), value(d, c, d.look.value? as f64))))
            .collect();
        page.render(&values)
    });
    let root = match rendered {
        Ok(Some(root)) => root,
        Ok(None) => return,
        Err(e) => {
            let at = e.line.map_or_else(String::new, |l| format!(":{l}"));
            bld.unsupported(None, "native_ui", format!("{}{at}: {}", e.module, e.message));
            return;
        }
    };
    let size = bld.ui.pages.first().map_or(PAGE, |p| p.size);
    for w in &mut bld.ui.widgets {
        w.hidden = true;
    }
    let first = bld.ui.widgets.len();
    lower(&root, size, bld);
    bld.ui.paint_order = Some((first..bld.ui.widgets.len()).map(ui::WidgetRef).collect());
}

/// Kontakt 8's instrument page when the script sets none.
const PAGE: ui::Size = ui::Size {
    width: 970.,
    height: 600.,
};

/// What a node draws with, inherited from the modifiers around it.
#[derive(Clone, Copy, Debug)]
struct Env {
    font: f64,
    color: [f64; 4],
    opacity: f64,
}

/// A size, or an offer of room along each axis.
type Size = (f64, f64);

fn lower(root: &Node, size: ui::Size, bld: &mut Builder<'_>) {
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
            (if iw > 0. { iw + l + r } else { 0. }, if ih > 0. { ih + t + b } else { 0. })
        }
        _ => inner(room),
    }
}

fn primitive_size(bld: &mut Builder<'_>, n: &Node, room: Size, env: Env) -> Size {
    match n.kind.as_str() {
        "Spacer" => (0., 0.),
        // ponytail: an average glyph width; measure with the font when a
        // page's layout turns on it.
        "Text" => (words(n).chars().count() as f64 * env.font * 0.55, env.font * 1.25),
        "Image" if !matches!(n.prop("resizable"), Some(Prop::Bool(true))) => {
            image(bld, n).and_then(|(_, s)| s).unwrap_or((0., 0.))
        }
        "ZStack" | "ForEach" => children(n).iter().fold((0., 0.), |(w, h), c| {
            let (cw, ch) = measure(bld, c, c.mods.len(), room, env);
            (w.max(cw), h.max(ch))
        }),
        "HStack" => stack(bld, n, Axis::Across, room, env).1,
        "VStack" => stack(bld, n, Axis::Down, room, env).1,
        _ => (0., 0.),
    }
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
        if self == Self::Across { (along, across) } else { (across, along) }
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
    let flexible = |s: &Size| axis.along(*s) <= 0.;
    let shares = sizes.iter().filter(|s| flexible(s)).count();
    let fixed: f64 = sizes.iter().map(|s| axis.along(*s)).sum::<f64>() + gaps;
    let share = if shares == 0 {
        0.
    } else {
        (axis.along(room) - fixed).max(0.) / shares as f64
    };
    let lengths: Vec<f64> = sizes
        .iter()
        .map(|s| if flexible(s) { share } else { axis.along(*s) })
        .collect();
    let thick = sizes.iter().map(|s| axis.across(*s)).fold(0., f64::max);
    let length = if shares > 0 {
        axis.along(room).max(fixed)
    } else {
        fixed
    };
    (lengths, axis.size(length, thick))
}

/// Places layer `k` of `n` in `r`, appending what it draws.
fn place(n: &Node, k: usize, r: ui::Rect, env: Env, bld: &mut Builder<'_>) {
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
        // Not copied: a turned node (a knob's pointer) shows nothing rather
        // than point the wrong way.
        "rotation" if num(args.first()).is_some_and(|a| a != 0.) => {
            bld.unsupported(None, "native_ui rotation", n.kind.clone());
            return;
        }
        "frame" => {
            let w = num(arg(args, "width")).unwrap_or(r.width);
            let h = num(arg(args, "height")).unwrap_or(r.height);
            let room = aligned(r, (w, h), (0.5, 0.5));
            aligned(room, size(bld, room), alignment(text(arg(args, "alignment"))))
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

/// What takes what it is given (zero) takes `r`'s size along that axis.
fn or_room((w, h): Size, r: ui::Rect) -> Size {
    (if w > 0. { w } else { r.width }, if h > 0. { h } else { r.height })
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
        if axis == Axis::Across { (0., a.1) } else { (a.0, 0.) }
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
        "Text" => ui::Kind::Label,
        "Image" => ui::Kind::Image,
        "ZStack" | "ForEach" => {
            let a = alignment(text(n.prop("alignment")));
            for c in children(n) {
                let size = or_room(measure(bld, c, c.mods.len(), (r.width, r.height), env), r);
                place(c, c.mods.len(), aligned(r, size, a), env, bld);
            }
            return;
        }
        "HStack" | "VStack" => {
            let axis = if n.kind == "HStack" { Axis::Across } else { Axis::Down };
            let (lengths, _) = stack(bld, n, axis, (r.width, r.height), env);
            let spacing = num(n.prop("spacing")).unwrap_or(0.);
            let mut at = 0.;
            for (c, len) in children(n).into_iter().zip(lengths) {
                let slot = match axis {
                    Axis::Across => ui::Rect { x: r.x + at, width: len, ..r },
                    Axis::Down => ui::Rect { y: r.y + at, height: len, ..r },
                };
                let thick = axis.across(measure(bld, c, c.mods.len(), (slot.width, slot.height), env));
                let thick = if thick > 0. { thick } else { axis.across((r.width, r.height)) };
                place(c, c.mods.len(), aligned(slot, axis.size(len, thick), along(axis)), env, bld);
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
    // a character (the label draws it centred, so the slack is free).
    let r = match n.kind.as_str() {
        "Text" => {
            let em = words(n).chars().count() as f64 * env.font;
            aligned(r, (em.max(r.width), (env.font * 1.5).max(r.height)), (0.5, 0.5))
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
        "Text" => {
            w.text = words(n);
            w.colors.background = None;
            bld.ui.styles.push(ui::TextStyle {
                font: ui::Font::Default,
                size: Some(env.font as f32),
                color: rgba(env.color, 1.),
                align: ui::Align::Center,
            });
            w.style = Some(ui::StyleRef(bld.ui.styles.len() - 1));
        }
        "Image" => {
            let Some((asset, _)) = image(bld, n) else { return };
            w.images.push(ui::ImageUse::new(asset, ui::Role::Background));
        }
        _ if w.colors.background.is_none() => return,
        _ => {}
    }
    bld.ui.widgets.push(w);
}
