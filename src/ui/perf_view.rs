//! A script's performance view as its library drew it: the wallpaper and
//! every control at its own pixel position, in its own pictures, scaled to
//! the part. A control without its picture, or of a kind that has none, is
//! drawn plainly in its place. Edits reach the script the way the rebuilt
//! view's do ([`crate::plugin::Shared::edit_control`]), so its
//! `on ui_control` runs.

use super::menu::{self, Target};
use super::{Cx, theme::*};
use crate::artwork::Picture;
use crate::ksp::{Control, Interface, Value};
use moose::mui::mui::prelude::*;
use moose::mui::mui::scene::{Fit, Image};
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

/// Rows of the wallpaper above the performance view: Kontakt's instrument
/// header covers them.
pub const HEADER: f64 = 68.;

const HIDE_BG: i32 = 1;
const HIDE_VALUE: i32 = 2;
const HIDE_TITLE: i32 = 4;
const HIDE_WHOLE: i32 = 16;

/// Kontakt's default text, near enough: its own fonts are not drawn.
const FONT: f64 = 11.;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Knob,
    Slider,
    Switch,
    Button,
    Menu,
    Value,
    Label,
    Table,
    TextEdit,
    /// Takes the pointer in Kontakt, shows nothing: `ui_mouse_area`.
    Area,
    /// Any other kind (`ui_xy`, `ui_waveform`, a meter): a plain box.
    Other,
}

impl Kind {
    fn of(kind: &str) -> Self {
        match kind {
            "ui_knob" => Self::Knob,
            "ui_slider" => Self::Slider,
            "ui_switch" => Self::Switch,
            "ui_button" => Self::Button,
            "ui_menu" => Self::Menu,
            "ui_value_edit" => Self::Value,
            "ui_label" => Self::Label,
            "ui_table" => Self::Table,
            "ui_text_edit" => Self::TextEdit,
            "ui_mouse_area" => Self::Area,
            _ => Self::Other,
        }
    }
}

/// One visible control where the script put it, in authored pixels.
#[derive(Clone, Debug)]
pub struct Shown {
    /// Index into `Interface::controls`.
    pub control: usize,
    pub kind: Kind,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub z: i32,
    pub picture: Option<Arc<Picture>>,
}

fn int(c: &Control, name: &str) -> Option<i32> {
    match c.properties.get(name)? {
        Value::Int(n) => Some(*n),
        Value::Real(r) => Some(*r as i32),
        _ => None,
    }
}

fn prop<'a>(c: &'a Control, name: &str) -> &'a str {
    match c.properties.get(name) {
        Some(Value::Text(s)) => s,
        _ => "",
    }
}

fn value(c: &Control) -> f64 {
    match c.properties.get("$CONTROL_PAR_VALUE") {
        Some(Value::Int(n)) => f64::from(*n),
        Some(Value::Real(r)) => *r,
        _ => 0.,
    }
}

/// The frame of an `frames`-long strip a knob or slider at `value` shows.
pub fn frame(value: f64, min: f64, max: f64, frames: usize) -> usize {
    if frames <= 1 {
        return 0;
    }
    let span = max - min;
    let t = if span == 0. { 0. } else { ((value - min) / span).clamp(0., 1.) };
    (t * (frames - 1) as f64).round() as usize
}

/// The frame a switch or button shows: Kontakt's strips run off, on, then
/// pressed and hovered states.
pub fn switch_frame(on: bool, frames: usize) -> usize {
    usize::from(on).min(frames.saturating_sub(1))
}

/// The scale the view is drawn at in `avail` points of width: `setting`
/// when the player chose one, else the largest whole scale that fits, or
/// smaller than 1 to fit a narrow part.
pub fn scale(avail: f64, width: f64, setting: f32) -> f64 {
    if setting > 0. {
        return f64::from(setting);
    }
    if width <= 0. || avail <= 0. {
        return 1.;
    }
    let fit = avail / width;
    if fit >= 1. { fit.floor() } else { fit }
}

/// The visible controls in drawing order: back layer first, then as
/// declared. Kontakt sizes a control to a picture that does not stretch.
pub fn layout(interface: &Interface, pictures: &HashMap<String, Arc<Picture>>) -> Vec<Shown> {
    let mut out: Vec<Shown> = (interface.controls.iter().enumerate())
        .filter(|(_, c)| int(c, "$CONTROL_PAR_HIDE").unwrap_or(0) & HIDE_WHOLE == 0)
        .map(|(n, c)| {
            let picture = pictures.get(prop(c, "$CONTROL_PAR_PICTURE")).filter(|p| !p.frames.is_empty()).cloned();
            let size = |k: &str, or: i32| f64::from(int(c, k).unwrap_or(or));
            let kind = Kind::of(&c.kind);
            let (w, h) = match &picture {
                Some(p) if !p.resizable => (f64::from(p.frames[0].width), f64::from(p.frames[0].height)),
                // Kontakt's own knob has room for its name and value.
                None if kind == Kind::Knob => (size("$CONTROL_PAR_WIDTH", 85), size("$CONTROL_PAR_HEIGHT", 52).max(52.)),
                _ => (size("$CONTROL_PAR_WIDTH", 85), size("$CONTROL_PAR_HEIGHT", 18)),
            };
            Shown {
                control: n,
                kind,
                x: size("$CONTROL_PAR_POS_X", 0),
                y: size("$CONTROL_PAR_POS_Y", 0),
                w,
                h,
                z: int(c, "$CONTROL_PAR_Z_LAYER").unwrap_or(0),
                picture,
            }
        })
        .filter(|s| s.w > 0. && s.h > 0. && s.x < f64::from(interface.width) && s.y < f64::from(interface.height))
        .filter(|s| s.x + s.w > 0. && s.y + s.h > 0.)
        .collect();
    out.sort_by_key(|s| s.z);
    out
}

/// Whether `part`'s library has a performance view of its own to show:
/// a script's view with a wallpaper or pictures.
pub fn available(v: &crate::plugin::PartView) -> bool {
    v.interface.as_ref().is_some_and(|i| i.performance && !i.controls.is_empty())
        && (v.wallpaper.is_some() || !v.pictures.is_empty())
}

/// Whether a part showing `view` ([`crate::plugin::Part::view`]) shows the
/// original view, given the app's default and whether there is one.
pub fn original(view: u8, vector_default: bool, available: bool) -> bool {
    available
        && match view {
            1 => true,
            2 => false,
            _ => !vector_default,
        }
}

/// Whether `slot` shows its library's own view now.
pub fn shows(cx: &Cx, slot: usize) -> bool {
    let view = cx.selection.parts.get(slot).map_or(0, |p| p.view);
    original(view, cx.settings.vector_view, available(&cx.view.parts[slot]))
}

/// The width `slot`'s view has, as last laid out.
fn room(ui: &Ui, slot: usize) -> f64 {
    ui.scene()
        .and_then(|s| s.surface(&format!("part-{slot}")))
        .map_or(0., |s| s.frame.size.width)
}

/// Everything [`view`] reads beyond the frame's input, hashed.
pub fn deps(ui: &Ui, cx: &Cx, slot: usize) -> u64 {
    let v = &cx.view.parts[slot];
    let mut h = DefaultHasher::new();
    (room(ui, slot).round() as i64, cx.settings.view_scale.to_bits()).hash(&mut h);
    (Arc::as_ptr(&v.pictures) as usize, v.wallpaper.as_ref().map(|w| Arc::as_ptr(w) as usize)).hash(&mut h);
    if let Some(i) = &v.interface {
        (Arc::as_ptr(i) as usize, i.width, i.height).hash(&mut h);
        // An edit may change the interface in place.
        for c in &i.controls {
            for (k, v) in &c.properties {
                if matches!(k.as_str(), "$CONTROL_PAR_VALUE" | "$CONTROL_PAR_LABEL" | "$CONTROL_PAR_HIDE") {
                    format!("{v:?}").hash(&mut h);
                }
            }
        }
    }
    h.finish()
}

/// Ask once for pictures the script names that were not read when the
/// part loaded (a page the script switched to), read off the frame.
fn fetch(cx: &mut Cx, slot: usize, interface: &Interface) {
    let v = &cx.view.parts[slot];
    let Some(path) = v.instrument.as_ref().map(|i| i.path.clone()) else { return };
    let names: Vec<String> = (interface.controls.iter())
        .map(|c| prop(c, "$CONTROL_PAR_PICTURE"))
        .filter(|n| !n.is_empty() && !v.pictures.contains_key(*n))
        .filter(|n| cx.state.perf_asked.insert((path.clone(), (*n).to_owned())))
        .map(str::to_owned)
        .collect();
    if names.is_empty() {
        return;
    }
    let p = cx.p.clone();
    let _ = std::thread::Builder::new().name("kontakto-pictures".into()).spawn(move || {
        let found = crate::artwork::pictures(&path, names.iter().map(String::as_str));
        if found.is_empty() {
            return;
        }
        let mut view = p.shared.view.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let v = &mut view.parts[slot];
        if v.instrument.as_ref().is_some_and(|i| i.path == path) {
            let mut all = (*v.pictures).clone();
            all.extend(found);
            v.pictures = Arc::new(all);
        }
    });
}

/// `slot`'s performance view as its library drew it.
pub fn view(ui: &mut Ui, cx: &mut Cx, slot: usize) -> El {
    let v = &cx.view.parts[slot];
    let (Some(interface), pictures, wallpaper) = (v.interface.clone(), v.pictures.clone(), v.wallpaper.clone()) else {
        return block(0, 0);
    };
    fetch(cx, slot, &interface);
    let (w, h) = (f64::from(interface.width), f64::from(interface.height));
    let s = scale(room(ui, slot), w, cx.settings.view_scale);
    let mut layers = Vec::new();
    if let Some(image) = wallpaper.clone() {
        let (iw, ih) = (f64::from(image.width), f64::from(image.height));
        layers.push(block(iw * s, ih * s).fill(Fill::Image(image, Fit::Fill)).at(0., -HEADER * s));
    }
    let drawn: Vec<(Shown, Option<Arc<Image>>)> = layout(&interface, &pictures)
        .into_iter()
        .map(|shown| {
            let image = picture_frame(&shown, &interface.controls[shown.control], value(&interface.controls[shown.control]));
            (shown, image)
        })
        .collect();
    for (n, (shown, _)) in drawn.iter().enumerate() {
        let c = &interface.controls[shown.control];
        // What its text sits on: its own picture, else what is under it.
        let (cx_, cy) = (shown.x + shown.w / 2., shown.y + shown.h / 2.);
        let under = [(-0.25, 0.), (0., 0.), (0.25, 0.)]
            .iter()
            .filter_map(|(dx, _)| luma_under(&drawn[..=n], wallpaper.as_deref(), cx_ + dx * shown.w, cy))
            .fold(None, |m: Option<(f32, u32)>, l| Some(m.map_or((l, 1), |(t, k)| (t + l, k + 1))))
            .map(|(t, k)| t / k as f32);
        layers.push(control(ui, cx, slot, shown, c, s, under).at(shown.x * s, shown.y * s));
    }
    let area = stack(layers)
        .w(w * s)
        .h(h * s)
        .shrink(0)
        .fill(Color::srgb(0., 0., 0.))
        .clip()
        .named("Original performance view");
    row![area].justify(Justify::Center).align(Align::Start).w(Len::Pct(100.))
}

/// The text a control shows, its alignment (0 left, 1 centre, 2 right)
/// and its offset from the top, if the script set one.
fn caption_of(c: &Control, kind: Kind, value: f64) -> (String, i32, Option<f64>) {
    let hide = int(c, "$CONTROL_PAR_HIDE").unwrap_or(0);
    let own = prop(c, "$CONTROL_PAR_TEXT").to_owned();
    let words = match kind {
        Kind::Label | Kind::Switch | Kind::Button => own,
        Kind::Menu => c.menu.iter().find(|(_, v)| f64::from(*v) == value).map(|(t, _)| t.clone()).unwrap_or_default(),
        Kind::Value if hide & HIDE_VALUE != 0 => String::new(),
        Kind::Value if hide & HIDE_TITLE != 0 || own.is_empty() => format!("{value}"),
        Kind::Value => format!("{own} {value}"),
        Kind::TextEdit => match c.properties.get("$CONTROL_PAR_VALUE") {
            Some(Value::Text(t)) => t.clone(),
            _ => own,
        },
        _ => String::new(),
    };
    let default = if kind == Kind::Label { 0 } else { 1 };
    let align = int(c, "$CONTROL_PAR_TEXT_ALIGNMENT").unwrap_or(default);
    let top = int(c, "$CONTROL_PAR_TEXTPOS_Y").map(f64::from);
    (super::panel::clean(&words), align, top)
}

/// Text on a control `w` by `h` (scaled), aligned as the script asked.
fn words(words: String, align: i32, top: Option<f64>, w: f64, h: f64, s: f64, ink: Color) -> El {
    let t = text(words).text_size(FONT * s).fill(ink).lines(1).min_w(0);
    let justify = match align {
        1 => Justify::Center,
        2 => Justify::End,
        _ => Justify::Start,
    };
    let line = row![t].justify(justify).align(Align::Center).w(w).pad((2. * s, 0.));
    match top {
        Some(y) => line.h(FONT * s * 1.4).at(0., y * s),
        None => line.h(h).at(0., 0.),
    }
}

/// A control's range as declared: a switch's is 0 to 1.
fn range(kind: Kind, c: &Control) -> (f64, f64) {
    let toggle = matches!(kind, Kind::Switch | Kind::Button);
    (
        f64::from(int(c, "$CONTROL_PAR_MIN_VALUE").filter(|_| !toggle).unwrap_or(0)),
        f64::from(int(c, "$CONTROL_PAR_MAX_VALUE").filter(|_| !toggle).unwrap_or(if toggle { 1 } else { 127 })),
    )
}

/// The frame of its picture a control shows at `now`: by value for knobs
/// and sliders, by state for switches, as the script set it for the rest.
fn picture_frame(shown: &Shown, c: &Control, now: f64) -> Option<Arc<Image>> {
    let p = shown.picture.as_ref()?;
    let n = p.frames.len();
    let (min, max) = range(shown.kind, c);
    let f = match shown.kind {
        Kind::Knob | Kind::Slider => frame(now, min, max, n),
        Kind::Switch | Kind::Button => switch_frame(now >= 1., n),
        _ => usize::try_from(int(c, "$CONTROL_PAR_PICTURE_STATE").unwrap_or(0)).unwrap_or(0).min(n - 1),
    };
    p.frames.get(f).cloned()
}

/// Light text, or dark over something light: Kontakt's own fonts carry
/// their colors, which are not known here.
fn ink(under: Option<f32>) -> Color {
    match under {
        Some(l) if l > 0.6 => Color::srgb(0.1, 0.1, 0.1),
        _ => Color::srgb(0.88, 0.88, 0.88),
    }
}

/// How light (0 to 1) what lies under authored point `(x, y)` is: the
/// topmost opaque picture of `below` there, else the wallpaper.
fn luma_under(below: &[(Shown, Option<Arc<Image>>)], wallpaper: Option<&Image>, x: f64, y: f64) -> Option<f32> {
    let at = |image: &Image, u: f64, v: f64| {
        let (px, py) = (u.floor(), v.floor());
        if px < 0. || py < 0. || px >= f64::from(image.width) || py >= f64::from(image.height) {
            return None;
        }
        let i = (py as usize * image.width as usize + px as usize) * 4;
        let c = image.rgba.get(i..i + 4)?;
        (c[3] >= 128).then(|| (0.2126 * f32::from(c[0]) + 0.7152 * f32::from(c[1]) + 0.0722 * f32::from(c[2])) / 255.)
    };
    for (s, image) in below.iter().rev() {
        let Some(image) = image else { continue };
        if x >= s.x && x < s.x + s.w && y >= s.y && y < s.y + s.h {
            let u = (x - s.x) / s.w * f64::from(image.width);
            let v = (y - s.y) / s.h * f64::from(image.height);
            if let Some(l) = at(image, u, v) {
                return Some(l);
            }
        }
    }
    at(wallpaper?, x, y + HEADER)
}

/// One control: its picture's frame or a plain stand-in, its text, and its
/// pointer handling. `under` is how light what its text sits on is.
#[allow(clippy::too_many_arguments)]
fn control(ui: &mut Ui, cx: &mut Cx, slot: usize, shown: &Shown, c: &Control, s: f64, under: Option<f32>) -> El {
    let id = format!("kpv-{slot}-{}", shown.control);
    let (w, h) = (shown.w * s, shown.h * s);
    let hide = int(c, "$CONTROL_PAR_HIDE").unwrap_or(0);
    let raw = value(c);
    let (min, max) = range(shown.kind, c);
    let (lo, hi) = (min.min(max), min.max(max).max(min.min(max) + 1.));
    let reset = f64::from(int(c, "$CONTROL_PAR_DEFAULT_VALUE").unwrap_or(0)).clamp(lo, hi);
    let mut now = raw;
    let mut interactive = true;
    match shown.kind {
        Kind::Knob | Kind::Slider | Kind::Value => {
            let key = (slot, shown.control);
            now = match cx.state.held {
                Some((p, n, v)) if (p, n) == key => v,
                _ => raw,
            };
            // Kontakt's drag: negative behaviour is across, its size the speed.
            let behaviour = int(c, "$CONTROL_PAR_MOUSE_BEHAVIOUR").unwrap_or(0);
            let travel = match (shown.kind, behaviour) {
                (Kind::Value, _) => ((hi - lo) * 3.).clamp(60., 600.),
                (_, 0) => 200.,
                (_, b) => (100_000. / f64::from(b.unsigned_abs())).clamp(60., 600.),
            };
            let held = drive(ui, &id, &mut now, &(lo..=hi), travel * s, behaviour >= 0, reset);
            let r = ui.get(id.as_str());
            if r.pressed && (r.mods.ctrl || r.mods.cmd) {
                now = reset;
            }
            if held {
                cx.state.held = Some((slot, shown.control, now));
            } else if cx.state.held.is_some_and(|(p, n, _)| (p, n) == key) {
                cx.state.held = None;
            }
            if now.round() != raw {
                cx.p.shared.edit_control(slot, shown.control, now.round() as i32);
            }
        }
        Kind::Switch | Kind::Button => {
            let r = ui.get(id.as_str());
            if r.clicked || r.key_activated {
                now = if raw >= 1. { 0. } else { 1. };
                cx.p.shared.edit_control(slot, shown.control, now as i32);
            }
        }
        Kind::Menu => {
            if ui.get(id.as_str()).activated() {
                menu::open_under(ui, cx, Target::Script { part: slot, control: shown.control }, &id);
            }
        }
        _ => interactive = false,
    }
    let mut layers = vec![match picture_frame(shown, c, now) {
        Some(image) => block(w, h).fill(Fill::Image(image, Fit::Fill)),
        None => plain(shown.kind, c, now, lo, hi, hide, s).w(w).h(h),
    }];
    // Text on a plain control sits on its dark field.
    let field = shown.picture.is_none() && !matches!(shown.kind, Kind::Label | Kind::Knob | Kind::Area);
    let own_ink = ink(if field { None } else { under });
    let (said, align, top) = caption_of(c, shown.kind, now);
    if !said.is_empty() {
        layers.push(words(said, align, top, w, h, s, own_ink));
    }
    if shown.kind == Kind::Knob && shown.picture.is_none() {
        // Kontakt's own knob: its name over it, its value under it.
        let name = prop(c, "$CONTROL_PAR_TEXT");
        let name = if name.is_empty() { c.variable.trim_start_matches(['$', '~']) } else { name };
        if hide & HIDE_TITLE == 0 {
            layers.push(words(super::panel::clean(name), 1, Some(0.), w, h, s, ink(under)));
        }
        if hide & HIDE_VALUE == 0 {
            let label = prop(c, "$CONTROL_PAR_LABEL");
            let shown_value = if label.is_empty() { format!("{}", now.round()) } else { super::panel::clean(label) };
            layers.push(words(shown_value, 1, Some(shown.h - FONT * 1.4), w, h, s, ink(under)));
        }
    }
    let el = stack(layers).w(w).h(h).clip();
    if !interactive {
        return el;
    }
    let help = prop(c, "$CONTROL_PAR_HELP");
    let name = match prop(c, "$CONTROL_PAR_AUTOMATION_NAME") {
        "" => c.variable.clone(),
        n => n.to_owned(),
    };
    let el = el
        .cursor(match shown.kind {
            Kind::Knob | Kind::Slider | Kind::Value => Cursor::Grab,
            _ => Cursor::Hand,
        })
        .named(name);
    let el = if help.is_empty() { el } else { el.tip(help.to_owned()) };
    el.captures_wheel().id(id)
}

/// A control drawn without its picture: Kontakt's plain look, near enough.
fn plain(kind: Kind, c: &Control, value: f64, lo: f64, hi: f64, hide: i32, s: f64) -> El {
    let t = ((value - lo) / (hi - lo)).clamp(0., 1.);
    let on = value >= 1.;
    let bars: Vec<f64> = match (kind, c.properties.get("$CONTROL_PAR_VALUE")) {
        (Kind::Table, Some(Value::Array(a))) => a
            .iter()
            .map(|v| match v {
                Value::Int(n) => f64::from(*n),
                Value::Real(r) => *r,
                _ => 0.,
            })
            .collect(),
        _ => Vec::new(),
    };
    let bg = hide & HIDE_BG == 0;
    canvas(move |z| {
        let (w, h) = (z.width, z.height);
        let field = Color::srgba(0.12, 0.12, 0.13, 0.9);
        let edge = Color::srgba(1., 1., 1., 0.18);
        let lit = Color::srgb(0.62, 0.78, 0.95);
        let dim = Color::srgba(0.62, 0.78, 0.95, 0.6);
        let mut d = Vec::new();
        let boxed = |d: &mut Vec<Draw>, fill: Color| {
            d.push(Draw::fill(rect(0., 0., w, h), fill));
            d.push(Draw::stroke(rect(0.5, 0.5, w - 1., h - 1.), edge, 1.));
        };
        match kind {
            Kind::Knob => {
                let (cx, cy) = (w / 2., h / 2.);
                let r = (w.min(h) / 2. - 3. * s).max(2.);
                let (start, sweep) = (0.75 * std::f64::consts::PI, 1.5 * std::f64::consts::PI);
                d.push(Draw::fill(circle(cx, cy, r), field));
                d.push(Draw::stroke(arc(cx, cy, r, start, sweep), edge, 2. * s));
                if t > 0.002 {
                    d.push(Draw::stroke(arc(cx, cy, r, start, sweep * t), lit, 2. * s));
                }
            }
            Kind::Slider => {
                boxed(&mut d, field);
                if w >= h {
                    d.push(Draw::fill(rect(1., 1., (w - 2.) * t, h - 2.), dim));
                } else {
                    d.push(Draw::fill(rect(1., (h - 1.) - (h - 2.) * t, w - 2., (h - 2.) * t), dim));
                }
            }
            Kind::Switch | Kind::Button => boxed(&mut d, if on { Color::srgba(0.36, 0.42, 0.5, 0.95) } else { field }),
            Kind::Menu | Kind::Value | Kind::TextEdit | Kind::Other => boxed(&mut d, field),
            Kind::Label if bg => d.push(Draw::fill(rect(0., 0., w, h), Color::srgba(0.2, 0.2, 0.21, 0.85))),
            Kind::Table => {
                boxed(&mut d, field);
                let top = bars.iter().fold(1f64, |m, v| m.max(v.abs()));
                let bw = w / bars.len().max(1) as f64;
                for (n, v) in bars.iter().enumerate() {
                    let bh = (v.abs() / top).min(1.) * (h - 2.);
                    d.push(Draw::fill(rect(n as f64 * bw + 1., h - 1. - bh, (bw - 1.).max(1.), bh), dim));
                }
            }
            Kind::Label | Kind::Area => {}
        }
        d
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn control(kind: &str, props: &[(&str, Value)]) -> Control {
        Control {
            variable: "$c".into(),
            kind: kind.into(),
            properties: props.iter().map(|(k, v)| (format!("$CONTROL_PAR_{k}"), v.clone())).collect::<BTreeMap<_, _>>(),
            menu: Vec::new(),
        }
    }

    fn picture(w: u32, h: u32, frames: usize, resizable: bool) -> Arc<Picture> {
        let frame = Arc::new(Image::rgba(w, h, vec![0u8; (w * h * 4) as usize]).unwrap());
        Arc::new(Picture { frames: vec![frame; frames], resizable })
    }

    #[test]
    fn frames_follow_the_value() {
        assert_eq!(frame(0., 0., 100., 101), 0);
        assert_eq!(frame(100., 0., 100., 101), 100);
        assert_eq!(frame(50., 0., 100., 11), 5);
        assert_eq!(frame(-64., -64., 63., 128), 0, "a bipolar range starts at its first frame");
        assert_eq!(frame(63., -64., 63., 128), 127);
        assert_eq!(frame(500., 0., 100., 11), 10, "out of range clamps");
        assert_eq!(frame(30., 100., 0., 11), 7, "a reversed range runs backwards");
        assert_eq!(frame(5., 5., 5., 11), 0);
        assert_eq!(frame(3., 0., 10., 1), 0);
        assert_eq!((switch_frame(false, 6), switch_frame(true, 6)), (0, 1));
        assert_eq!(switch_frame(true, 1), 0, "a one-frame switch shows its only frame");
    }

    #[test]
    fn scale_is_whole_when_it_fits() {
        assert_eq!(scale(1180., 632., 0.), 1.);
        assert_eq!(scale(1300., 632., 0.), 2.);
        assert_eq!(scale(2000., 632., 0.), 3.);
        assert!((scale(500., 800., 0.) - 0.625).abs() < 1e-9, "a narrow part shrinks it to fit");
        assert_eq!(scale(500., 800., 1.5), 1.5, "the player's scale wins");
        assert_eq!(scale(0., 632., 0.), 1.);
    }

    #[test]
    fn layout_places_sizes_and_orders() {
        let interface = Interface {
            performance: true,
            width: 600,
            height: 300,
            controls: vec![
                control("ui_slider", &[("POS_X", Value::Int(10)), ("POS_Y", Value::Int(20)), ("WIDTH", Value::Int(5)), ("HEIGHT", Value::Int(5)), ("PICTURE", Value::Text("knob".into()))]),
                control("ui_label", &[("POS_X", Value::Int(0)), ("POS_Y", Value::Int(0)), ("WIDTH", Value::Int(600)), ("HEIGHT", Value::Int(300)), ("Z_LAYER", Value::Int(-1)), ("PICTURE", Value::Text("bg".into()))]),
                control("ui_switch", &[("POS_X", Value::Int(1)), ("HIDE", Value::Int(16))]),
                control("ui_menu", &[("POS_X", Value::Int(700))]),
                control("ui_button", &[("POS_X", Value::Int(40)), ("PICTURE", Value::Text("missing".into()))]),
            ],
            ..Interface::default()
        };
        let pictures: HashMap<String, Arc<Picture>> =
            [("knob".to_owned(), picture(48, 50, 101, false)), ("bg".to_owned(), picture(16, 16, 1, true))].into();
        let shown = layout(&interface, &pictures);
        let order: Vec<usize> = shown.iter().map(|s| s.control).collect();
        assert_eq!(order, [1, 0, 4], "the back layer first; hidden and off-view controls left out");
        assert_eq!((shown[1].x, shown[1].y, shown[1].w, shown[1].h), (10., 20., 48., 50.), "sized to its picture");
        assert_eq!((shown[0].w, shown[0].h), (600., 300.), "a resizable picture stretches to the control");
        assert!(shown[2].picture.is_none(), "a missing picture falls back to a plain control");
        assert_eq!((shown[2].w, shown[2].h), (85., 18.));
    }

    #[test]
    fn the_original_view_is_the_default_when_there_is_one() {
        assert!(original(0, false, true));
        assert!(!original(0, true, true), "the app's default");
        assert!(original(1, true, true) && !original(2, false, true), "a part's own choice wins");
        assert!(!original(1, false, false), "nothing to show: the rebuilt view");
    }

    #[test]
    fn captions_read_the_script() {
        let mut menu = control("ui_menu", &[("VALUE", Value::Int(2))]);
        menu.menu = vec![("Close".into(), 0), ("Far".into(), 2)];
        assert_eq!(caption_of(&menu, Kind::Menu, 2.), ("Far".into(), 1, None));
        let label = control("ui_label", &[("TEXT", Value::Text("Reverb".into())), ("TEXTPOS_Y", Value::Int(3))]);
        assert_eq!(caption_of(&label, Kind::Label, 0.), ("Reverb".into(), 0, Some(3.)));
        let edit = control("ui_value_edit", &[("TEXT", Value::Text("Voices".into())), ("TEXT_ALIGNMENT", Value::Int(2))]);
        assert_eq!(caption_of(&edit, Kind::Value, 4.), ("Voices 4".into(), 2, None));
    }

    #[test]
    fn text_ink_reads_what_lies_under_it() {
        let white = Arc::new(Image::rgba(2, 2, vec![255u8; 16]).unwrap());
        let clear = Arc::new(Image::rgba(2, 2, vec![0u8; 16]).unwrap());
        let at = |x, y, w, h| Shown { control: 0, kind: Kind::Label, x, y, w, h, z: 0, picture: None };
        let wallpaper = Image::rgba(1, 100, [0u8, 0, 0, 255].repeat(100)).unwrap();
        let below = vec![(at(0., 0., 10., 10.), Some(white.clone())), (at(0., 0., 5., 5.), Some(clear))];
        assert_eq!(luma_under(&below, Some(&wallpaper), 2., 2.), Some(1.), "a clear picture shows the one under it");
        assert_eq!(luma_under(&below, Some(&wallpaper), 0., 20.), Some(0.), "else the wallpaper, below its header rows");
        assert_eq!(luma_under(&below, None, 50., 50.), None);
        assert_eq!(ink(Some(1.)), Color::srgb(0.1, 0.1, 0.1));
        assert_eq!(ink(None), ink(Some(0.)));
    }
}
