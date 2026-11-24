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
    weight: Option<f32>,
    color: [f64; 4],
    opacity: f64,
    disabled: bool,
    text_align: Option<ui::Align>,
    line_limit: Option<usize>,
    tint: Option<[f64; 4]>,
    image_fit: ImageFit,
    resizable: bool,
}

/// How an `Image` fits inside its allocated room.
#[derive(Clone, Copy, Debug, PartialEq)]
enum ImageFit {
    /// Fill room (default when resizable with no aspect modifier).
    Stretch,
    /// Preserve aspect ratio, fitting completely inside the room.
    Fit(Option<f64>),
    /// Preserve aspect ratio, covering the room.
    Fill(Option<f64>),
}

/// A size, or an offer of room along each axis.
type Size = (f64, f64);

pub(super) fn lower(root: &Node, size: ui::Size, bld: &mut Builder<'_>) {
    let env = Env {
        font: 12.,
        weight: None,
        color: [255., 255., 255., 1.],
        opacity: 1.,
        disabled: false,
        text_align: None,
        line_limit: None,
        tint: None,
        image_fit: ImageFit::Stretch,
        resizable: false,
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

/// Parses a text alignment keyword (`leading`/`left`, `trailing`/`right`, `center`).
fn text_align(words: Option<&str>) -> Option<ui::Align> {
    let w = words?.to_ascii_lowercase();
    if w.contains("leading") || w.contains("left") {
        Some(ui::Align::Left)
    } else if w.contains("trailing") || w.contains("right") {
        Some(ui::Align::Right)
    } else if w.contains("center") {
        Some(ui::Align::Center)
    } else {
        None
    }
}

/// Parses a font weight keyword or numeric CSS weight into a `wght` axis value.
fn font_weight(p: Option<&Prop>) -> Option<f32> {
    match p {
        Some(Prop::Number(n)) if n.is_finite() => Some((*n as f32).clamp(100.0, 900.0)),
        Some(Prop::Text(s)) => match s.to_ascii_lowercase().as_str() {
            "thin" => Some(100.0),
            "ultralight" | "extralight" => Some(200.0),
            "light" => Some(300.0),
            "regular" | "normal" => None,
            "medium" => Some(500.0),
            "semibold" | "demibold" => Some(600.0),
            "bold" => Some(700.0),
            "heavy" | "extrabold" => Some(800.0),
            "black" => Some(900.0),
            _ => None,
        },
        _ => None,
    }
}

/// Parses an `.aspect_ratio(...)` argument into an [`ImageFit`].
fn aspect_ratio_fit(args: &[Prop]) -> ImageFit {
    let ratio = num(args.first())
        .or_else(|| num(arg(args, "ratio")))
        .or_else(|| num(arg(args, "aspect_ratio")))
        .filter(|r| r.is_finite() && *r > 0.0);
    let mode = text(args.get(1))
        .or_else(|| text(arg(args, "content_mode")))
        .or_else(|| text(arg(args, "mode")))
        .or_else(|| text(args.first()));
    if mode.is_some_and(|m| m.eq_ignore_ascii_case("fill")) {
        ImageFit::Fill(ratio)
    } else {
        ImageFit::Fit(ratio)
    }
}

/// The environment inside modifier `m`.
fn env_in(env: Env, m: &str, args: &[Prop]) -> Env {
    match (m, args.first()) {
        ("font_size", Some(Prop::Number(n))) if n.is_finite() && *n > 0. => Env { font: *n, ..env },
        ("bold", Some(Prop::Bool(false))) => Env {
            weight: None,
            ..env
        },
        ("bold", Some(Prop::Bool(true)) | None) => Env {
            weight: Some(700.0),
            ..env
        },
        ("font_weight", p) => Env {
            weight: font_weight(p),
            ..env
        },
        ("foreground_color", Some(Prop::Color(c))) => Env { color: *c, ..env },
        ("opacity", Some(Prop::Number(n))) if n.is_finite() => Env {
            opacity: (env.opacity * *n).clamp(0.0, 1.0),
            ..env
        },
        ("disabled", p) => {
            let off = matches!(p, Some(Prop::Bool(true)) | None);
            Env {
                disabled: env.disabled || off,
                ..env
            }
        }
        ("multiline_text_alignment", p) => Env {
            text_align: text_align(text(p)).or(env.text_align),
            ..env
        },
        ("line_limit", Some(Prop::Number(n))) if n.is_finite() && *n >= 1.0 => Env {
            line_limit: Some(*n as usize),
            ..env
        },
        ("line_limit", _) => Env {
            line_limit: None,
            ..env
        },
        ("tint" | "color_multiply", Some(Prop::Color(c))) => Env {
            tint: Some(*c),
            ..env
        },
        ("resizable", p) => Env {
            resizable: !matches!(p, Some(Prop::Bool(false))),
            ..env
        },
        ("scaled_to_fit", _) => Env {
            image_fit: ImageFit::Fit(None),
            ..env
        },
        ("scaled_to_fill", _) => Env {
            image_fit: ImageFit::Fill(None),
            ..env
        },
        ("aspect_ratio", _) => Env {
            image_fit: aspect_ratio_fit(args),
            ..env
        },
        ("effect", _) => effect_env(env, args),
        _ => env,
    }
}

/// Applies environment-level properties from an `.effect(...)` modifier (tint/opacity).
fn effect_env(mut env: Env, args: &[Prop]) -> Env {
    if let Some(Prop::Number(n)) = arg(args, "opacity")
        && n.is_finite()
    {
        env.opacity = (env.opacity * *n).clamp(0.0, 1.0);
    }
    if let Some(Prop::Color(c)) = arg(args, "tint").or_else(|| arg(args, "color_multiply")) {
        env.tint = Some(*c);
    }
    let kind = text(arg(args, "kind")).or_else(|| text(arg(args, "type")));
    if kind
        .is_some_and(|k| k.eq_ignore_ascii_case("tint") || k.eq_ignore_ascii_case("color_multiply"))
        && let Some(Prop::Color(c)) = arg(args, "color")
    {
        env.tint = Some(*c);
    }
    if kind.is_some_and(|k| k.eq_ignore_ascii_case("opacity"))
        && let Some(Prop::Number(n)) = arg(args, "value").or_else(|| arg(args, "amount"))
        && n.is_finite()
    {
        env.opacity = (env.opacity * *n).clamp(0.0, 1.0);
    }
    env
}

/// A frame's size along each axis from its `min_`/`max_` bounds, SwiftUI's
/// flexible frame: what it is offered held between the bounds, a missing
/// one being the child's size. Offered nothing (zero), it takes its finite
/// most, or what it is given when that is unbounded (Conflux's steppers
/// are `max_width = 72` around text framed `max_width = math.huge`).
/// Without bounds on an axis it is `plain` there.
fn flexible(args: &[Prop], room: Size, child: Size, plain: Size) -> [f64; 2] {
    let axis = |offered: f64, child: f64, plain: f64, min: &str, max: &str| {
        let (lo, hi) = (num(arg(args, min)), num(arg(args, max)));
        if lo.is_none() && hi.is_none() {
            return plain;
        }
        let hi = hi.unwrap_or(child).max(lo.unwrap_or(0.));
        match offered {
            o if o > 0. => o.clamp(lo.unwrap_or(child).min(hi), hi),
            _ if hi.is_finite() => hi,
            _ => 0.,
        }
    };
    [
        axis(room.0, child.0, plain.0, "min_width", "max_width"),
        axis(room.1, child.1, plain.1, "min_height", "max_height"),
    ]
}

/// `n`'s children, those of a `ForEach` among them in its place, however
/// deep (Conflux's radio groups wrap a `ForEach` of buttons in another).
fn children(n: &Node) -> Vec<&Node> {
    n.children
        .iter()
        .flat_map(|c| match c.kind.as_str() {
            "ForEach" => children(c),
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
            let [fw, fh] = flexible(args, (room.0, room.1), (iw, ih), (iw, ih));
            (w.unwrap_or(fw), h.unwrap_or(fh))
        }
        "padding" => {
            let [l, t, r, b] = pads(args);
            let (iw, ih) = inner((room.0 - l - r, room.1 - t - b));
            (
                if iw > 0. { iw + l + r } else { 0. },
                if ih > 0. { ih + t + b } else { 0. },
            )
        }
        // It fills the room it is offered and sits where it says in it;
        // a positioned view too, as in SwiftUI (Conflux's Random meter is
        // a track under a cloud of positioned dots).
        "align" | "position" => (0., 0.),
        _ => inner(room),
    }
}

/// Proportional glyph advance in em units for Noto Sans / UI sans-serif,
/// replacing flat character-count heuristics so narrow/wide labels lay out
/// without clipping or misaligning.
fn char_advance_em(c: char) -> f64 {
    match c {
        ' ' => 0.26,
        'i' | 'l' | 'I' | '!' | '|' | '.' | ',' | ':' | ';' | '\'' | '`' => 0.28,
        'j' | 't' | 'f' | 'r' | '(' | ')' | '[' | ']' | '{' | '}' | '/' | '\\' => 0.36,
        '-' | '"' => 0.38,
        's' | 'z' | 'c' | 'k' | 'v' | 'x' | 'y' => 0.50,
        '0'..='9' => 0.57,
        'm' | 'w' => 0.82,
        'M' | 'W' => 0.88,
        'A'..='Z' => 0.66,
        'a'..='z' => 0.55,
        c if c.is_ascii() => 0.56,
        _ => 0.65,
    }
}

/// Measures a single line's advance width in points for `env`.
fn line_width(line: &str, env: Env) -> f64 {
    let bold_scale = if env.weight.is_some_and(|w| w >= 600.0) {
        1.06
    } else {
        1.0
    };
    line.chars()
        .map(|c| char_advance_em(c) * env.font * bold_scale)
        .sum()
}

/// Wraps `text` into display lines within `max_width` (when positive and not
/// constrained to a single line) and enforces `env.line_limit`, appending an
/// ellipsis if lines are truncated.
fn layout_lines(text: &str, max_width: f64, env: Env) -> Vec<String> {
    let wrap = max_width > 0.0 && env.line_limit != Some(1);
    let mut lines: Vec<String> = Vec::new();
    for para in text.split('\n') {
        if !wrap || line_width(para, env) <= max_width {
            lines.push(para.to_owned());
            continue;
        }
        let mut current = String::new();
        for word in para.split_whitespace() {
            if current.is_empty() {
                current.push_str(word);
                continue;
            }
            let candidate = format!("{current} {word}");
            if line_width(&candidate, env) <= max_width {
                current = candidate;
            } else {
                lines.push(current);
                current = word.to_owned();
            }
        }
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    if let Some(limit) = env.line_limit
        && lines.len() > limit
    {
        lines.truncate(limit);
        if let Some(last) = lines.last_mut()
            && !last.ends_with('…')
        {
            last.push('…');
        }
    }
    lines
}

/// Intrinsic size of `text` under `env` and available width `room_w`.
fn measure_text(text: &str, room_w: f64, env: Env) -> Size {
    let lines = layout_lines(text, room_w, env);
    let width = lines
        .iter()
        .map(|l| line_width(l, env))
        .fold(0.0_f64, f64::max);
    let height = (lines.len().max(1) as f64) * env.font * 1.25;
    (width, height)
}

/// Fits an image of intrinsic size `intrinsic` into `room` under `fit`.
/// When only one axis of `room` is constrained (> 0), derives the other from
/// the aspect ratio.
fn fit_image(intrinsic: Size, room: Size, fit: ImageFit) -> Size {
    let (iw, ih) = intrinsic;
    let ratio = match fit {
        ImageFit::Fit(Some(r)) | ImageFit::Fill(Some(r)) => r,
        _ if iw > 0.0 && ih > 0.0 => iw / ih,
        _ => return room,
    };
    if !ratio.is_finite() || ratio <= 0.0 {
        return intrinsic;
    }
    match (room.0 > 0.0, room.1 > 0.0) {
        (false, false) => intrinsic,
        (true, false) => (room.0, room.0 / ratio),
        (false, true) => (room.1 * ratio, room.1),
        (true, true) => match fit {
            ImageFit::Stretch => room,
            ImageFit::Fit(_) => {
                let by_w = (room.0, room.0 / ratio);
                let by_h = (room.1 * ratio, room.1);
                if by_w.1 <= room.1 { by_w } else { by_h }
            }
            ImageFit::Fill(_) => {
                let by_w = (room.0, room.0 / ratio);
                let by_h = (room.1 * ratio, room.1);
                if by_w.1 >= room.1 { by_w } else { by_h }
            }
        },
    }
}

fn image_is_resizable(n: &Node, env: Env) -> bool {
    env.resizable || matches!(n.prop("resizable"), Some(Prop::Bool(true)))
}

fn image_fit_mode(n: &Node, env: Env) -> ImageFit {
    if env.image_fit != ImageFit::Stretch {
        return env.image_fit;
    }
    let ratio = num(n.prop("aspect_ratio")).filter(|r| r.is_finite() && *r > 0.0);
    match text(n.prop("content_mode"))
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("fit") => ImageFit::Fit(ratio),
        Some("fill") => ImageFit::Fill(ratio),
        _ => match ratio {
            Some(r) => ImageFit::Fit(Some(r)),
            None => ImageFit::Stretch,
        },
    }
}

fn primitive_size(bld: &mut Builder<'_>, n: &Node, room: Size, env: Env) -> Size {
    match n.kind.as_str() {
        "Spacer" => (0., 0.),
        "Text" => measure_text(&words(n), room.0, env),
        "TextInput" => {
            let sample = words(n);
            let display = if sample.is_empty() {
                text(n.prop("placeholder")).unwrap_or("")
            } else {
                sample.as_str()
            };
            let (tw, th) = measure_text(display, room.0, env);
            (tw + 8.0, (th + 4.0).max(env.font * 1.5))
        }
        "Image" => {
            let intrinsic = image(bld, n).and_then(|(_, s)| s);
            if !image_is_resizable(n, env) {
                intrinsic.unwrap_or((0., 0.))
            } else {
                let fit = image_fit_mode(n, env);
                match (fit, intrinsic) {
                    (ImageFit::Stretch, _) => (0., 0.),
                    (fit, Some(size)) if room.0 > 0. || room.1 > 0. => fit_image(size, room, fit),
                    (ImageFit::Fit(Some(r)) | ImageFit::Fill(Some(r)), None)
                        if room.0 > 0. || room.1 > 0. =>
                    {
                        fit_image((r, 1.0), room, fit)
                    }
                    (_, Some(size)) => size,
                    (_, None) => (0., 0.),
                }
            }
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
/// modifier is an `align` or a `position` (offsets and looks keep their
/// child's size).
fn fills(n: &Node) -> bool {
    n.mods
        .iter()
        .rev()
        .map(|(m, _)| m.as_str())
        .find(|m| {
            !matches!(
                *m,
                "offset"
                    | "opacity"
                    | "hidden"
                    | "foreground_color"
                    | "font_size"
                    | "bold"
                    | "font_weight"
                    | "multiline_text_alignment"
                    | "line_limit"
                    | "tint"
                    | "color_multiply"
                    | "shadow"
                    | "blur"
                    | "border"
                    | "effect"
                    | "resizable"
                    | "scaled_to_fit"
                    | "scaled_to_fill"
                    | "aspect_ratio"
                    | "background"
                    | "overlay"
                    | "rotation"
                    | "on_hover_gesture"
                    | "on_tap_gesture"
                    | "on_drag_gesture"
                    | "disabled"
                    | "popover"
            )
        })
        .is_some_and(|m| matches!(m, "align" | "position"))
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
    // One that shares out what is left takes what it is given, as a
    // SwiftUI stack with a spacer does: Conflux's module header (toggle,
    // spacer, title, spacer) spans its module, its toggle at the left.
    let length = if shares > 0 { 0. } else { fixed };
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
            // Re-open enclosing ancestor slots while placing this subtree so
            // bindings (e.g. `TextInput`) still see ancestor `reads`.
            for (j, s) in bld.nui.iter_mut().enumerate().take(i) {
                if j + s.inner >= i {
                    s.open = true;
                }
            }
            place(n, n.mods.len(), old.rect, old.env, bld);
            for (j, s) in bld.nui.iter_mut().enumerate().take(i) {
                if j + s.inner >= i {
                    s.open = false;
                }
            }
            // Rebuilt on the layer it was on (a popover's content stays over
            // the page), or higher where the rebuild opened a popover.
            for w in bld.ui.widgets.iter_mut().skip(old.start) {
                w.z = w.z.max(old.z);
            }
            for s in bld.nui.iter_mut().skip(i) {
                s.z = s.z.max(old.z);
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
            let own = measure(bld, n, k - 1, (r.width, r.height), inner_env);
            let own = or_room(own, r);
            let [fw, fh] = flexible(args, (r.width, r.height), own, (r.width, r.height));
            let w = num(arg(args, "width")).unwrap_or(fw);
            let h = num(arg(args, "height")).unwrap_or(fh);
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
        // Disabled views suppress interactive gestures.
        "on_drag_gesture" | "on_tap_gesture" | "on_hover_gesture" => {
            if !inner_env.disabled {
                gesture(bld, m, args, r);
            }
            place_layer(n, k - 1, r, inner_env, bld);
            return;
        }
        "shadow" => {
            place_with_layers(
                n,
                k - 1,
                r,
                inner_env,
                bld,
                |b, body| emit_shadow(b, body, args, inner_env),
                |_, _| {},
            );
            return;
        }
        "blur" => {
            place_with_layers(
                n,
                k - 1,
                r,
                inner_env,
                bld,
                |b, body| emit_blur(b, body, args, inner_env),
                |_, _| {},
            );
            return;
        }
        "border" => {
            place_with_layers(
                n,
                k - 1,
                r,
                inner_env,
                bld,
                |_, _| {},
                |b, body| emit_border(b, body, args, inner_env),
            );
            return;
        }
        "effect" => {
            place_with_layers(
                n,
                k - 1,
                r,
                inner_env,
                bld,
                |b, body| emit_effect_under(b, body, args, inner_env),
                |b, body| emit_effect_over(b, body, args, inner_env),
            );
            return;
        }
        // The other layer is offered the view's size and keeps its own,
        // centred, as in SwiftUI (Conflux's fixed-size heading images sat
        // stretched over their whole selector).
        "background" | "overlay" => {
            let other = |bld: &mut Builder<'_>| {
                if let Some(Prop::Node(b)) = args.first() {
                    let own = measure(bld, b, b.mods.len(), (r.width, r.height), env);
                    let at = aligned(r, or_room(own, r), (0.5, 0.5));
                    place(b, b.mods.len(), at, env, bld);
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

/// The hidden KSP `TextEdit` widget backing a `TextInput`, found via an
/// explicit `parameter` prop or the innermost open instance's read set.
fn text_input_control(bld: &Builder<'_>, n: &Node) -> Option<ui::Widget> {
    let matches_id = |w: &ui::Widget, id: &str| {
        w.hidden && super::id(&w.name) == id && matches!(w.kind, ui::Kind::TextEdit)
    };
    if let Some(id) = text(n.prop("parameter"))
        && let Some(w) = bld.ui.widgets.iter().find(|w| matches_id(w, id))
    {
        return Some(w.clone());
    }
    let open = bld.nui.iter().rev().filter(|s| s.open);
    open.flat_map(|s| &s.reads)
        .find_map(|id| bld.ui.widgets.iter().find(|w| matches_id(w, id)))
        .cloned()
}

/// Places layer `k` of `n` and emits decorative `under` layers below it and
/// `over` layers above it, sized to the wrapped body's actual placed rectangle.
fn place_with_layers(
    n: &Node,
    k: usize,
    r: ui::Rect,
    env: Env,
    bld: &mut Builder<'_>,
    under: impl FnOnce(&mut Builder<'_>, ui::Rect),
    over: impl FnOnce(&mut Builder<'_>, ui::Rect),
) {
    let (w_before, n_before) = (bld.ui.widgets.len(), bld.nui.len());
    place_layer(n, k, r, env, bld);
    let w_after_body = bld.ui.widgets.len();
    let body = bld
        .ui
        .widgets
        .get(w_before..w_after_body)
        .and_then(union_rect)
        .unwrap_or(r);
    under(bld, body);
    let added_under = bld.ui.widgets.len() - w_after_body;
    if added_under > 0 {
        if let Some(slice) = bld.ui.widgets.get_mut(w_before..) {
            slice.rotate_right(added_under);
        }
        for s in bld.nui.iter_mut().skip(n_before) {
            s.start += added_under;
            s.end += added_under;
        }
    }
    over(bld, body);
}

/// Bounds of the placed body, skipping decorations nested modifiers emitted
/// (an inner `.shadow` must not grow an outer `.border`).
fn union_rect(widgets: &[ui::Widget]) -> Option<ui::Rect> {
    let decor = ["native_ui shadow", "native_ui blur", "native_ui border"];
    let mut iter = widgets
        .iter()
        .filter(|w| !decor.contains(&w.name.as_str()))
        .map(|w| w.rect);
    let first = iter.next()?;
    let (mut x0, mut y0, mut x1, mut y1) = (
        first.x,
        first.y,
        first.x + first.width,
        first.y + first.height,
    );
    for rect in iter {
        x0 = x0.min(rect.x);
        y0 = y0.min(rect.y);
        x1 = x1.max(rect.x + rect.width);
        y1 = y1.max(rect.y + rect.height);
    }
    Some(ui::Rect {
        x: x0,
        y: y0,
        width: (x1 - x0).max(0.0),
        height: (y1 - y0).max(0.0),
    })
}

/// Emits layered translucent shadow rectangles behind `r` for `.shadow(...)`.
fn emit_shadow(bld: &mut Builder<'_>, r: ui::Rect, args: &[Prop], env: Env) {
    if r.width <= 0. || r.height <= 0. || env.opacity <= 0. {
        return;
    }
    let color = match arg(args, "color").or_else(|| args.first()) {
        Some(Prop::Color(c)) => *c,
        _ => [0., 0., 0., 0.35],
    };
    let radius = num(arg(args, "radius"))
        .or_else(|| num(args.get(1)))
        .unwrap_or(4.0)
        .max(0.0);
    let ox = num(arg(args, "x"))
        .or_else(|| num(args.get(2)))
        .unwrap_or(0.0);
    let oy = num(arg(args, "y"))
        .or_else(|| num(args.get(3)))
        .unwrap_or(2.0);
    let layers: &[f64] = if radius > 1.0 { &[1.0, 0.5] } else { &[0.0] };
    let alpha_share = 1.0 / layers.len() as f64;
    for &frac in layers {
        let spread = radius * frac;
        let rect = ui::Rect {
            x: r.x + ox - spread,
            y: r.y + oy - spread,
            width: r.width + 2.0 * spread,
            height: r.height + 2.0 * spread,
        };
        let mut w = ui::Widget::new(
            "native_ui shadow",
            ui::PageRef(0),
            rect,
            ui::Kind::Shape(ui::Shape::Rect { radius: spread }),
        );
        w.opacity = effective_opacity(env) as f32;
        w.colors.background = Some(rgba(color, alpha_share));
        bld.ui.widgets.push(w);
    }
}

/// Emits a soft translucent halo behind `r` for `.blur(radius)`.
fn emit_blur(bld: &mut Builder<'_>, r: ui::Rect, args: &[Prop], env: Env) {
    let radius = num(args.first())
        .or_else(|| num(arg(args, "radius")))
        .unwrap_or(0.0);
    if radius <= 0.0 || r.width <= 0. || r.height <= 0. || env.opacity <= 0. {
        return;
    }
    let spread = (radius * 0.5).min(16.0);
    let rect = ui::Rect {
        x: r.x - spread,
        y: r.y - spread,
        width: r.width + 2.0 * spread,
        height: r.height + 2.0 * spread,
    };
    let mut w = ui::Widget::new(
        "native_ui blur",
        ui::PageRef(0),
        rect,
        ui::Kind::Shape(ui::Shape::Rect { radius: spread }),
    );
    w.opacity = (effective_opacity(env) * 0.18) as f32;
    w.colors.background = Some(rgba(apply_tint(env.color, env.tint), 1.0));
    bld.ui.widgets.push(w);
}

/// Emits a crisp vector stroke rectangle over `r` for `.border(...)`.
fn emit_border(bld: &mut Builder<'_>, r: ui::Rect, args: &[Prop], env: Env) {
    if r.width <= 0. || r.height <= 0. || env.opacity <= 0. {
        return;
    }
    let color = match arg(args, "color").or_else(|| args.first()) {
        Some(Prop::Color(c)) => *c,
        _ => env.color,
    };
    let width = num(arg(args, "width"))
        .or_else(|| num(args.get(1)))
        .unwrap_or(1.0);
    if width <= 0.0 {
        return;
    }
    let inset = width * 0.5;
    let x0 = inset;
    let y0 = inset;
    let x1 = (r.width - inset).max(x0);
    let y1 = (r.height - inset).max(y0);
    let paint = ui::Paint {
        path: vec![
            ui::Seg::Move([x0, y0]),
            ui::Seg::Line([x1, y0]),
            ui::Seg::Line([x1, y1]),
            ui::Seg::Line([x0, y1]),
            ui::Seg::Close,
        ],
        fill: None,
        stroke: Some((rgba(apply_tint(color, env.tint), 1.0), width)),
    };
    let mut w = ui::Widget::new(
        "native_ui border",
        ui::PageRef(0),
        r,
        ui::Kind::Canvas(vec![paint]),
    );
    w.opacity = effective_opacity(env) as f32;
    bld.ui.widgets.push(w);
}

/// Under-node visual layers from `.effect(...)` (shadow, blur).
fn emit_effect_under(bld: &mut Builder<'_>, r: ui::Rect, args: &[Prop], env: Env) {
    let kind = text(arg(args, "kind"))
        .or_else(|| text(arg(args, "type")))
        .map(str::to_ascii_lowercase);
    match kind.as_deref() {
        Some("shadow" | "drop_shadow") => emit_shadow(bld, r, args, env),
        Some("blur") => emit_blur(bld, r, args, env),
        _ => {
            if let Some(sub) = arg(args, "shadow").or_else(|| arg(args, "drop_shadow")) {
                emit_shadow(bld, r, std::slice::from_ref(sub), env);
            }
            if let Some(sub) = arg(args, "blur") {
                emit_blur(bld, r, std::slice::from_ref(sub), env);
            }
        }
    }
}

/// Over-node visual layers from `.effect(...)` (border).
fn emit_effect_over(bld: &mut Builder<'_>, r: ui::Rect, args: &[Prop], env: Env) {
    let kind = text(arg(args, "kind"))
        .or_else(|| text(arg(args, "type")))
        .map(str::to_ascii_lowercase);
    if kind.as_deref() == Some("border") {
        emit_border(bld, r, args, env);
    } else if let Some(sub) = arg(args, "border") {
        emit_border(bld, r, std::slice::from_ref(sub), env);
    }
}

fn effective_opacity(env: Env) -> f64 {
    if env.disabled {
        env.opacity * 0.45
    } else {
        env.opacity
    }
}

fn apply_tint(c: [f64; 4], tint: Option<[f64; 4]>) -> [f64; 4] {
    match tint {
        Some(t) => [
            c[0] * (t[0] / 255.0).clamp(0.0, 1.0),
            c[1] * (t[1] / 255.0).clamp(0.0, 1.0),
            c[2] * (t[2] / 255.0).clamp(0.0, 1.0),
            c[3] * t[3].clamp(0.0, 1.0),
        ],
        None => c,
    }
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
/// answers with the page's own callbacks ([`super::TAP`]); a hover is one
/// that takes no press, set while the pointer is over it, whose `enter` and
/// `exit` run in place of a tap's `start` and `complete` (Conflux shows its
/// macros' assign toggles while the row is hovered).
fn gesture(bld: &mut Builder<'_>, m: &str, args: &[Prop], r: ui::Rect) {
    let hover = m == "on_hover_gesture";
    let callback = |k: &str| match arg(args, k) {
        Some(Prop::Callback(n)) => *n,
        _ => 0,
    };
    let (start, complete) = if hover {
        (callback("enter"), callback("exit"))
    } else {
        (callback("start"), callback("complete"))
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
        _ if start + complete > 0 => Some(ui::Widget {
            intercepts_mouse: !hover,
            ..ui::Widget::new(
                format!("{} {start} {complete}", super::TAP),
                ui::PageRef(0),
                r,
                ui::Kind::Button { momentary: true },
            )
        }),
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
        "Text" => ui::Kind::Label,
        "TextInput" => ui::Kind::TextEdit,
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
    let align_of = |default: ui::Align| env.text_align.unwrap_or(default);
    let anchor_x = |a: ui::Align| match a {
        ui::Align::Left => 0.0,
        ui::Align::Center => 0.5,
        ui::Align::Right => 1.0,
    };
    let text_lines = (n.kind == "Text").then(|| layout_lines(&words(n), r.width, env));
    // A text label uses explicit zero inset (bypassing KSP's 14 px label_inset)
    // and modest overhang anchored to its text alignment so left/right labels
    // never drift off their edge and narrow glyphs are never clipped.
    let (r, img_asset) = match n.kind.as_str() {
        "Text" => {
            let lines = text_lines.as_deref().unwrap_or(&[]);
            let measured_w = lines
                .iter()
                .map(|l| line_width(l, env))
                .fold(0.0_f64, f64::max);
            let measured_h = (lines.len().max(1) as f64) * env.font * 1.25;
            let slack_w = (measured_w + env.font * 0.25).max(r.width);
            let slack_h = measured_h.max(r.height);
            let ax = anchor_x(align_of(ui::Align::Center));
            (aligned(r, (slack_w, slack_h), (ax, 0.5)), None)
        }
        "Image" => {
            let Some((asset, intrinsic)) = image(bld, n) else {
                return;
            };
            let fitted = if !image_is_resizable(n, env) {
                match intrinsic {
                    Some(sz) => aligned(r, sz, (0.5, 0.5)),
                    None => r,
                }
            } else {
                let fit = image_fit_mode(n, env);
                let sz = match (fit, intrinsic) {
                    (ImageFit::Stretch, _) => (r.width, r.height),
                    (fit, Some(int)) => fit_image(int, (r.width, r.height), fit),
                    (ImageFit::Fit(Some(ratio)) | ImageFit::Fill(Some(ratio)), None) => {
                        fit_image((ratio, 1.0), (r.width, r.height), fit)
                    }
                    (_, None) => (r.width, r.height),
                };
                aligned(r, sz, (0.5, 0.5))
            };
            (fitted, Some(asset))
        }
        _ => (r, None),
    };
    let mut w = if n.kind == "TextInput" {
        if let Some(bound) = text_input_control(bld, n) {
            ui::Widget {
                rect: r,
                auto_size: false,
                default_axes: [false; 2],
                hidden: false,
                parent: None,
                images: Vec::new(),
                z: 0,
                ..bound
            }
        } else {
            ui::Widget::new(n.kind.clone(), ui::PageRef(0), r, kind)
        }
    } else {
        ui::Widget::new(n.kind.clone(), ui::PageRef(0), r, kind)
    };
    w.opacity = effective_opacity(env) as f32;
    w.colors.background = match n.prop("color") {
        Some(Prop::Color(c)) => Some(rgba(apply_tint(*c, env.tint), 1.)),
        _ => None,
    };
    let push_style = |bld: &mut Builder<'_>, style: ui::TextStyle| {
        let at = bld.ui.styles.iter().position(|s| *s == style);
        ui::StyleRef(at.unwrap_or_else(|| {
            bld.ui.styles.push(style);
            bld.ui.styles.len() - 1
        }))
    };
    match n.kind.as_str() {
        "Text" => {
            w.text = text_lines.map_or_else(|| words(n), |lines| lines.join("\n"));
            w.colors.background = None;
            let style = ui::TextStyle {
                font: ui::Font::Default,
                size: Some(env.font as f32),
                weight: env.weight,
                inset: Some([0.0, 0.0]),
                color: rgba(apply_tint(env.color, env.tint), 1.),
                align: align_of(ui::Align::Center),
            };
            w.style = Some(push_style(bld, style));
        }
        "TextInput" => {
            let explicit = words(n);
            if !explicit.is_empty() {
                w.text = explicit;
            }
            w.enabled = !env.disabled;
            w.intercepts_mouse = !env.disabled;
            let style = ui::TextStyle {
                font: ui::Font::Default,
                size: Some(env.font as f32),
                weight: env.weight,
                inset: Some([2.0, 2.0]),
                color: rgba(apply_tint(env.color, env.tint), 1.),
                align: align_of(ui::Align::Left),
            };
            w.style = Some(push_style(bld, style));
            // Keep placeholder strictly out of `w.text` (the edit/commit buffer)
            // and render it as a non-interactive hint label when empty.
            if w.text.is_empty()
                && let Some(ph) = text(n.prop("placeholder")).filter(|s| !s.is_empty())
            {
                let mut hint =
                    ui::Widget::new("native_ui placeholder", ui::PageRef(0), r, ui::Kind::Label);
                hint.text = ph.to_owned();
                hint.intercepts_mouse = false;
                hint.opacity = (effective_opacity(env) * 0.5) as f32;
                let hint_style = ui::TextStyle {
                    font: ui::Font::Default,
                    size: Some(env.font as f32),
                    weight: env.weight,
                    inset: Some([2.0, 2.0]),
                    color: rgba(apply_tint(env.color, env.tint), 1.),
                    align: align_of(ui::Align::Left),
                };
                hint.style = Some(push_style(bld, hint_style));
                bld.ui.widgets.push(w);
                bld.ui.widgets.push(hint);
                return;
            }
        }
        "Image" => {
            let Some(asset) = img_asset else {
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
        // An instance on the page (layer 0) rebuilt into an open popover:
        // the popover keeps its layer over the page.
        let mut shut = popover(false);
        shut.instance = Some("p".into());
        let mut opened = popover(true);
        opened.instance = Some("p".into());
        let room = ui::Size {
            width: 100.,
            height: 100.,
        };
        let mut bld = Builder {
            ui: ui::Interface::default(),
            assets: std::collections::HashMap::new(),
            styles: std::collections::HashMap::new(),
            picture: &picture,
            nui: Vec::new(),
        };
        lower(&shut, room, &mut bld);
        splice(&mut bld, 0, room, "p", Some(&opened));
        let over = bld.ui.widgets.iter().filter(|w| w.z == 1).count();
        assert_eq!(over, shown.len(), "a rebuild keeps the popover's layer");
    }

    /// Conflux's Random meter: a framed ZStack of a track rectangle and a
    /// faded cloud of positioned dots; the track fills the frame.
    #[test]
    fn a_framed_track_under_a_faded_dot_cloud_fills_its_frame() {
        let num = |k: &str, v: f64| (k.to_owned(), Prop::Number(v));
        let frame = |w, h| {
            (
                "frame",
                vec![Prop::Table(vec![num("width", w), num("height", h)])],
            )
        };
        let dot = node(
            "Rectangle",
            vec![
                frame(2., 2.),
                (
                    "position",
                    vec![Prop::Table(vec![num("x", 1.), num("y", 30.)])],
                ),
            ],
            Vec::new(),
        );
        let mut cloud = node(
            "ZStack",
            vec![("opacity", vec![Prop::Number(0.)])],
            vec![node("ForEach", Vec::new(), vec![dot])],
        );
        cloud.props = vec![("alignment".into(), Prop::Text("center".into()))];
        let track = node("Rectangle", Vec::new(), Vec::new());
        let meter = node("ZStack", vec![frame(4., 40.)], vec![track, cloud]);
        let picture = |_: &str| None;
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
        lower(&meter, room, &mut bld);
        let shapes: Vec<_> = (bld.ui.widgets.iter())
            .map(|w| (w.rect.width, w.rect.height, w.colors.background))
            .collect();
        assert_eq!(
            shapes.first().map(|s| (s.0, s.1)),
            Some((4., 40.)),
            "{shapes:?}"
        );
    }

    #[test]
    fn text_modifiers_font_metrics_line_limit_and_alignment_work() {
        let mut narrow = node("Text", Vec::new(), Vec::new());
        narrow.props = vec![("text".into(), Prop::Text("iiii".into()))];
        let mut wide = node("Text", vec![("bold", Vec::new())], Vec::new());
        wide.props = vec![("text".into(), Prop::Text("MMMM".into()))];
        let mut multi = node(
            "Text",
            vec![
                ("font_size", vec![Prop::Number(14.)]),
                ("bold", vec![Prop::Bool(true)]),
                (
                    "multiline_text_alignment",
                    vec![Prop::Text("leading".into())],
                ),
                ("line_limit", vec![Prop::Number(2.)]),
                ("disabled", vec![Prop::Bool(true)]),
            ],
            Vec::new(),
        );
        multi.props = vec![("text".into(), Prop::Text("one\ntwo\nthree".into()))];

        let picture = |_: &str| None;
        let mut bld = Builder {
            ui: ui::Interface::default(),
            assets: std::collections::HashMap::new(),
            styles: std::collections::HashMap::new(),
            picture: &picture,
            nui: Vec::new(),
        };
        let env = Env {
            font: 12.,
            weight: None,
            color: [255., 255., 255., 1.],
            opacity: 1.,
            disabled: false,
            text_align: None,
            line_limit: None,
            tint: None,
            image_fit: ImageFit::Stretch,
            resizable: false,
        };
        let nw = measure(&mut bld, &narrow, narrow.mods.len(), (200., 100.), env).0;
        let ww = measure(&mut bld, &wide, wide.mods.len(), (200., 100.), env).0;
        assert!(
            ww > nw * 2.5,
            "proportional metrics expected: wide={ww} narrow={nw}"
        );

        lower(
            &multi,
            ui::Size {
                width: 120.,
                height: 60.,
            },
            &mut bld,
        );
        let w = bld.ui.widgets.first();
        assert!(w.is_some());
        if let Some(w) = w {
            assert_eq!(w.text, "one\ntwo…");
            assert!(w.opacity < 0.5, "disabled text dims opacity: {}", w.opacity);
            assert_eq!(w.rect.x, 0.0, "leading-aligned text stays flush left");
            let style = w.style.and_then(|s| bld.ui.styles.get(s.0));
            assert_eq!(style.and_then(|s| s.weight), Some(700.0));
            assert_eq!(style.and_then(|s| s.inset), Some([0.0, 0.0]));
            assert_eq!(style.map(|s| s.align), Some(ui::Align::Left));
        }
    }

    #[test]
    fn image_aspect_ratio_fit_fill_and_non_resizable_sizing() {
        let picture = |path: &str| {
            path.ends_with("banner.png").then(|| ui::ImageMeta {
                size: Some(ui::Size {
                    width: 200.,
                    height: 100.,
                }),
                ..ui::ImageMeta::default()
            })
        };
        let run = |mods: Vec<(&str, Vec<Prop>)>, resizable_prop: bool| {
            let mut img = node("Image", mods, Vec::new());
            img.props = vec![
                ("file".into(), Prop::Text("banner.png".into())),
                ("resizable".into(), Prop::Bool(resizable_prop)),
            ];
            let mut bld = Builder {
                ui: ui::Interface::default(),
                assets: std::collections::HashMap::new(),
                styles: std::collections::HashMap::new(),
                picture: &picture,
                nui: Vec::new(),
            };
            lower(
                &img,
                ui::Size {
                    width: 100.,
                    height: 100.,
                },
                &mut bld,
            );
            bld.ui.widgets.first().map(|w| w.rect)
        };
        // Non-resizable keeps intrinsic 200x100 centered inside 100x100.
        let non_res = run(Vec::new(), false);
        assert_eq!(
            non_res,
            Some(ui::Rect {
                x: -50.,
                y: 0.,
                width: 200.,
                height: 100.,
            })
        );
        // Resizable + scaled_to_fit preserves 2:1 aspect inside 100x100 -> 100x50 centered.
        let fitted = run(vec![("scaled_to_fit", Vec::new())], true);
        assert_eq!(
            fitted,
            Some(ui::Rect {
                x: 0.,
                y: 25.,
                width: 100.,
                height: 50.,
            })
        );
        // Resizable via modifier + scaled_to_fill covers 100x100 at 2:1 -> 200x100 centered.
        let filled = run(
            vec![("resizable", Vec::new()), ("scaled_to_fill", Vec::new())],
            false,
        );
        assert_eq!(
            filled,
            Some(ui::Rect {
                x: -50.,
                y: 0.,
                width: 200.,
                height: 100.,
            })
        );
        // Explicit aspect_ratio(0.5, "fit") inside 100x100 -> 50x100 centered.
        let explicit = run(
            vec![(
                "aspect_ratio",
                vec![Prop::Number(0.5), Prop::Text("fit".into())],
            )],
            true,
        );
        assert_eq!(
            explicit,
            Some(ui::Rect {
                x: 25.,
                y: 0.,
                width: 50.,
                height: 100.,
            })
        );
    }

    #[test]
    fn text_input_lowers_to_text_edit_binds_ksp_and_respects_disabled() {
        let picture = |_: &str| None;
        let mut bld = Builder {
            ui: ui::Interface::default(),
            assets: std::collections::HashMap::new(),
            styles: std::collections::HashMap::new(),
            picture: &picture,
            nui: Vec::new(),
        };
        let mut hidden_edit = ui::Widget::new(
            "@macro_name",
            ui::PageRef(0),
            ui::Rect::new(0, 0, 10, 10),
            ui::Kind::TextEdit,
        );
        hidden_edit.hidden = true;
        hidden_edit.source_id = Some(42);
        hidden_edit.text = "Macro 1".into();
        bld.ui.widgets.push(hidden_edit);

        let mut input = node("TextInput", Vec::new(), Vec::new());
        input.props = vec![("placeholder".into(), Prop::Text("Name".into()))];
        input.instance = Some("macro/0".into());
        input.reads = vec!["macro_name".into()];

        lower(
            &input,
            ui::Size {
                width: 120.,
                height: 24.,
            },
            &mut bld,
        );
        let emitted = bld.ui.widgets.get(1);
        assert!(emitted.is_some());
        if let Some(w) = emitted {
            assert_eq!(w.kind, ui::Kind::TextEdit);
            assert_eq!(w.source_id, Some(42));
            assert_eq!(w.text, "Macro 1");
            assert!(w.enabled && w.intercepts_mouse);
        }

        // Disabled TextInput disables editing and mouse interception.
        input.mods.push(("disabled".into(), vec![Prop::Bool(true)]));
        let before = bld.ui.widgets.len();
        lower(
            &input,
            ui::Size {
                width: 120.,
                height: 24.,
            },
            &mut bld,
        );
        if let Some(w) = bld.ui.widgets.get(before) {
            assert_eq!(w.kind, ui::Kind::TextEdit);
            assert!(!w.enabled && !w.intercepts_mouse);
        }
    }

    #[test]
    fn visual_effect_modifiers_emit_shadow_blur_border_and_tint() {
        let picture = |_: &str| None;
        let mut bld = Builder {
            ui: ui::Interface::default(),
            assets: std::collections::HashMap::new(),
            styles: std::collections::HashMap::new(),
            picture: &picture,
            nui: Vec::new(),
        };
        let effect_arg = Prop::Table(vec![
            (
                "shadow".into(),
                Prop::Table(vec![
                    ("color".into(), Prop::Color([0., 0., 0., 0.5])),
                    ("radius".into(), Prop::Number(4.)),
                    ("x".into(), Prop::Number(1.)),
                    ("y".into(), Prop::Number(2.)),
                ]),
            ),
            ("blur".into(), Prop::Number(6.)),
            (
                "border".into(),
                Prop::Table(vec![
                    ("color".into(), Prop::Color([255., 255., 255., 1.])),
                    ("width".into(), Prop::Number(2.)),
                ]),
            ),
            ("tint".into(), Prop::Color([128., 255., 255., 1.])),
        ]);
        let rect = node(
            "Rectangle",
            vec![
                (
                    "frame",
                    vec![Prop::Table(vec![
                        ("width".into(), Prop::Number(80.)),
                        ("height".into(), Prop::Number(40.)),
                    ])],
                ),
                ("effect", vec![effect_arg]),
            ],
            Vec::new(),
        );
        lower(
            &rect,
            ui::Size {
                width: 100.,
                height: 60.,
            },
            &mut bld,
        );
        assert!(bld.ui.widgets.iter().any(|w| w.name == "native_ui shadow"));
        assert!(bld.ui.widgets.iter().any(|w| w.name == "native_ui blur"));
        assert!(
            bld.ui
                .widgets
                .iter()
                .any(|w| w.name == "native_ui border" && matches!(w.kind, ui::Kind::Canvas(_)))
        );
        let body = bld.ui.widgets.iter().find(|w| w.name == "Rectangle");
        assert!(body.is_some());
        if let Some(body) = body {
            assert_eq!(
                body.rect,
                ui::Rect {
                    x: 10.,
                    y: 10.,
                    width: 80.,
                    height: 40.,
                }
            );
            // Red [255, 0, 0] tinted by [128, 255, 255] -> ~128 red.
            assert!(
                body.colors
                    .background
                    .is_some_and(|c| c.r >= 127 && c.r <= 129 && c.g == 0 && c.b == 0)
            );
        }
        // Border wraps the inner body rect (10, 10, 80, 40), not the 100x60 outer room,
        // and sits after the body while shadow/blur sit before it.
        let body_idx = bld
            .ui
            .widgets
            .iter()
            .position(|w| w.name == "Rectangle")
            .unwrap_or(0);
        let shadow_idx = bld
            .ui
            .widgets
            .iter()
            .position(|w| w.name == "native_ui shadow")
            .unwrap_or(usize::MAX);
        let blur_idx = bld
            .ui
            .widgets
            .iter()
            .position(|w| w.name == "native_ui blur")
            .unwrap_or(usize::MAX);
        let border_idx = bld
            .ui
            .widgets
            .iter()
            .position(|w| w.name == "native_ui border")
            .unwrap_or(0);
        assert!(shadow_idx < body_idx && blur_idx < body_idx && border_idx > body_idx);

        // Nested: `.shadow(...)` then `.border(...)` — the border wraps the
        // body, not the body grown by the inner shadow's widgets.
        let mut nested = Builder {
            ui: ui::Interface::default(),
            assets: std::collections::HashMap::new(),
            styles: std::collections::HashMap::new(),
            picture: &picture,
            nui: Vec::new(),
        };
        let shadowed = node(
            "Rectangle",
            vec![
                (
                    "frame",
                    vec![Prop::Table(vec![
                        ("width".into(), Prop::Number(80.)),
                        ("height".into(), Prop::Number(40.)),
                    ])],
                ),
                (
                    "shadow",
                    vec![Prop::Table(vec![
                        ("color".into(), Prop::Color([0., 0., 0., 0.5])),
                        ("radius".into(), Prop::Number(4.)),
                        ("x".into(), Prop::Number(0.)),
                        ("y".into(), Prop::Number(2.)),
                    ])],
                ),
                (
                    "border",
                    vec![Prop::Table(vec![
                        ("color".into(), Prop::Color([255., 255., 255., 1.])),
                        ("width".into(), Prop::Number(2.)),
                    ])],
                ),
            ],
            Vec::new(),
        );
        lower(
            &shadowed,
            ui::Size {
                width: 100.,
                height: 60.,
            },
            &mut nested,
        );
        assert!(
            nested
                .ui
                .widgets
                .iter()
                .any(|w| w.name == "native_ui shadow")
        );
        assert_eq!(
            nested
                .ui
                .widgets
                .iter()
                .find(|w| w.name == "native_ui border")
                .map(|w| w.rect),
            Some(ui::Rect {
                x: 10.,
                y: 10.,
                width: 80.,
                height: 40.,
            })
        );
        assert_eq!(
            bld.ui.widgets.get(border_idx).map(|w| w.rect),
            Some(ui::Rect {
                x: 10.,
                y: 10.,
                width: 80.,
                height: 40.,
            })
        );
    }

    #[test]
    fn vstack_fits_single_axis_constrained_image_and_empty_text_input_keeps_placeholder_separate() {
        let picture = |name: &str| {
            (name == "native_ui/wide.png").then_some(ui::ImageMeta {
                size: Some(ui::Size {
                    width: 200.,
                    height: 100.,
                }),
                ..ui::ImageMeta::default()
            })
        };
        let mut bld = Builder {
            ui: ui::Interface::default(),
            assets: std::collections::HashMap::new(),
            styles: std::collections::HashMap::new(),
            picture: &picture,
            nui: Vec::new(),
        };
        let mut img = node(
            "Image",
            vec![("resizable", vec![]), ("scaled_to_fit", vec![])],
            Vec::new(),
        );
        img.props = vec![("file".into(), Prop::Text("wide.png".into()))];
        let vstack = node("VStack", Vec::new(), vec![img]);
        lower(
            &vstack,
            ui::Size {
                width: 100.,
                height: 100.,
            },
            &mut bld,
        );
        let placed = bld.ui.widgets.iter().find(|w| w.kind == ui::Kind::Image);
        assert_eq!(
            placed.map(|w| (w.rect.width, w.rect.height)),
            Some((100., 50.))
        );

        // Empty bound TextInput keeps `w.text` empty and preserves ancestor binding across splice.
        let mut hidden_edit = ui::Widget::new(
            "@macro_name",
            ui::PageRef(0),
            ui::Rect::new(0, 0, 10, 10),
            ui::Kind::TextEdit,
        );
        hidden_edit.hidden = true;
        hidden_edit.source_id = Some(99);
        bld.ui.widgets.clear();
        bld.nui.clear();
        bld.ui.widgets.push(hidden_edit);

        let mut child = node("TextInput", Vec::new(), Vec::new());
        child.props = vec![("placeholder".into(), Prop::Text("Hint".into()))];
        child.instance = Some("parent/child".into());
        let mut parent = node("VStack", Vec::new(), vec![child.clone()]);
        parent.instance = Some("parent".into());
        parent.reads = vec!["macro_name".into()];

        let size = ui::Size {
            width: 120.,
            height: 24.,
        };
        lower(&parent, size, &mut bld);
        splice(&mut bld, 1, size, "parent/child", Some(&child));
        let edit = bld
            .ui
            .widgets
            .iter()
            .skip(1)
            .find(|w| w.kind == ui::Kind::TextEdit);
        assert_eq!(edit.and_then(|w| w.source_id), Some(99));
        assert_eq!(edit.map(|w| w.text.as_str()), Some(""));
        assert!(
            bld.ui
                .widgets
                .iter()
                .any(|w| w.name == "native_ui placeholder"
                    && w.text == "Hint"
                    && !w.intercepts_mouse)
        );
    }
}
