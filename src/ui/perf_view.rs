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
use crate::library::ViewMode;
use moose::mui::mui::geometry::Path as DrawPath;
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
pub(super) const FONT: f64 = 11.;

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
    /// `ui_level_meter`: Kontakt draws it from its colours, never a picture.
    Meter,
    /// `ui_waveform`: its frame and zero line; the attached zone's wave
    /// is not drawn yet.
    Waveform,
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
            "ui_level_meter" => Self::Meter,
            "ui_waveform" => Self::Waveform,
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

pub(super) fn prop<'a>(c: &'a Control, name: &str) -> &'a str {
    match c.properties.get(name) {
        Some(Value::Text(s)) => s,
        _ => "",
    }
}

pub(super) fn value(c: &Control) -> f64 {
    match c.properties.get("$CONTROL_PAR_VALUE") {
        Some(Value::Int(n)) => f64::from(*n),
        Some(Value::Real(r)) => *r,
        _ => 0.,
    }
}

/// Whether a slider `w` by `h` with `picture` is drawn as a knob: named so,
/// or near square and big enough to turn.
pub fn knob_like(picture: &str, w: f64, h: f64) -> bool {
    let named = ["knob", "dial", "rotary"].iter().any(|k| picture.to_lowercase().contains(k));
    named || (0.6..=1.6).contains(&(w / h)) && w.min(h) >= 24.
}

/// Whether a continuous control drags up and down, as in Kontakt: a knob or
/// a value edit always does, horizontal movement ignored; a slider follows
/// the sign of `$CONTROL_PAR_MOUSE_BEHAVIOUR` (negative is vertical), else
/// its own shape.
pub fn drags_vertically(kind: Kind, w: f64, h: f64, picture: &str, behaviour: i32) -> bool {
    match kind {
        Kind::Slider if !knob_like(picture, w, h) => match behaviour {
            0 => h > w,
            b => b < 0,
        },
        _ => true,
    }
}

/// How far a drag runs the whole range, in authored pixels: the size of
/// `$CONTROL_PAR_MOUSE_BEHAVIOUR` is its speed, a value edit a few pixels
/// a step.
pub fn travel(kind: Kind, behaviour: i32, span: f64) -> f64 {
    match (kind, behaviour) {
        (Kind::Value, _) => (span * 3.).clamp(60., 600.),
        (_, 0) => 200.,
        (_, b) => (100_000. / f64::from(b.unsigned_abs())).clamp(60., 600.),
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

/// Where control `n` sits on the view, and whether it is hidden: a control
/// in a `ui_panel` (`$CONTROL_PAR_PARENT_PANEL`) is placed from its panel's
/// corner, and hidden with it, all the way up.
// ponytail: finds each parent by a scan; index the IDs if views with
// thousands of nested controls show up.
pub(super) fn placed(interface: &Interface, n: usize) -> (f64, f64, bool) {
    let (mut x, mut y, mut at, mut hidden) = (0., 0., n, false);
    // A panel in itself, or a deeper chain than any script builds, stops.
    // A hidden control still has its place: a list brings rows back.
    for _ in 0..16 {
        let c = &interface.controls[at];
        hidden |= int(c, "$CONTROL_PAR_HIDE").unwrap_or(0) & HIDE_WHOLE != 0;
        x += f64::from(int(c, "$CONTROL_PAR_POS_X").unwrap_or(0));
        y += f64::from(int(c, "$CONTROL_PAR_POS_Y").unwrap_or(0));
        let parent = int(c, "$CONTROL_PAR_PARENT_PANEL")
            .and_then(|id| interface.controls.iter().position(|p| p.id == id && p.kind == "ui_panel"));
        match parent {
            Some(p) if p != at => at = p,
            _ => break,
        }
    }
    (x, y, hidden)
}

/// The visible controls in drawing order: back layer first, then as
/// declared. Kontakt sizes a control to a picture that does not stretch.
/// A panel only places and hides what is in it.
pub fn layout(interface: &Interface, pictures: &HashMap<String, Arc<Picture>>) -> Vec<Shown> {
    let mut out: Vec<Shown> = (interface.controls.iter().enumerate())
        .filter(|(_, c)| c.kind != "ui_panel")
        .filter_map(|(n, c)| {
            let (x, y, hidden) = placed(interface, n);
            if hidden {
                return None;
            }
            let picture = pictures.get(prop(c, "$CONTROL_PAR_PICTURE")).filter(|p| !p.frames.is_empty()).cloned();
            let size = |k: &str, or: i32| f64::from(int(c, k).unwrap_or(or));
            let kind = Kind::of(&c.kind);
            let (w, h) = match &picture {
                Some(p) => p.size(size("$CONTROL_PAR_WIDTH", 85), size("$CONTROL_PAR_HEIGHT", 18)),
                // Kontakt's own knob has room for its name and value.
                None if kind == Kind::Knob => (size("$CONTROL_PAR_WIDTH", 85), size("$CONTROL_PAR_HEIGHT", 52).max(52.)),
                _ => (size("$CONTROL_PAR_WIDTH", 85), size("$CONTROL_PAR_HEIGHT", 18)),
            };
            Some(Shown {
                control: n,
                kind,
                x,
                y,
                w,
                h,
                z: int(c, "$CONTROL_PAR_Z_LAYER").unwrap_or(0),
                picture,
            })
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

/// The mode a part showing `view` ([`crate::plugin::Part::view`]) is in,
/// given the app's default and whether the library has a view of its own.
pub fn mode(view: u8, default: ViewMode, available: bool) -> ViewMode {
    if !available {
        return ViewMode::Kontra;
    }
    match view {
        1 => ViewMode::Original,
        2 => ViewMode::Kontra,
        3 => ViewMode::Vectorized,
        _ => default,
    }
}

/// What [`crate::plugin::Part::view`] keeps for `mode`; `None` follows the
/// app's default.
pub fn code(mode: Option<ViewMode>) -> u8 {
    match mode {
        None => 0,
        Some(ViewMode::Original) => 1,
        Some(ViewMode::Kontra) => 2,
        Some(ViewMode::Vectorized) => 3,
    }
}

/// The mode `slot` shows now.
pub fn shows(cx: &Cx, slot: usize) -> ViewMode {
    let view = cx.selection.parts.get(slot).map_or(0, |p| p.view);
    mode(view, cx.settings.view_mode, available(&cx.view.parts[slot]))
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
    (shows(cx, slot), cx.settings.vector_backdrop).hash(&mut h);
    (Arc::as_ptr(&v.pictures) as usize, v.wallpaper.as_ref().map(|w| Arc::as_ptr(w) as usize)).hash(&mut h);
    if let Some(i) = &v.interface {
        (Arc::as_ptr(i) as usize, i.width, i.height).hash(&mut h);
        // An edit may change the interface in place.
        for c in &i.controls {
            for (k, v) in &c.properties {
                if matches!(
                    k.as_str(),
                    "$CONTROL_PAR_VALUE" | "$CONTROL_PAR_LABEL" | "$CONTROL_PAR_HIDE" | "$CONTROL_PAR_TEXT" | "$CONTROL_PAR_POS_X" | "$CONTROL_PAR_POS_Y" | "$CONTROL_PAR_PARENT_PANEL"
                ) {
                    format!("{v:?}").hash(&mut h);
                }
            }
        }
    }
    cx.state.typing.as_ref().filter(|(p, ..)| *p == slot).hash(&mut h);
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

/// `slot`'s performance view as its library drew it, or vectorized: the
/// same controls in the same places in KONTRA's own look.
pub fn view(ui: &mut Ui, cx: &mut Cx, slot: usize) -> El {
    let vector = shows(cx, slot) == ViewMode::Vectorized;
    let v = &cx.view.parts[slot];
    let (Some(interface), pictures, wallpaper) = (v.interface.clone(), v.pictures.clone(), v.wallpaper.clone()) else {
        return block(0, 0);
    };
    fetch(cx, slot, &interface);
    let (w, h) = (f64::from(interface.width), f64::from(interface.height));
    let s = scale(room(ui, slot), w, cx.settings.view_scale);
    let mut layers = Vec::new();
    if let Some(image) = wallpaper.clone().filter(|_| !vector || cx.settings.vector_backdrop) {
        let (iw, ih) = (f64::from(image.width), f64::from(image.height));
        layers.push(block(iw * s, ih * s).fill(Fill::Image(image, Fit::Fill)).at(0., -HEADER * s));
        if vector {
            layers.push(block(w * s, h * s).fill(Role::Background.alpha(0.8)));
        }
    }
    let mut shown = layout(&interface, &pictures);
    if vector {
        // Our faces are solid where a picture let the words behind it show
        // through: the script's text goes on top.
        shown.sort_by_key(|s| s.kind == Kind::Label);
    }
    let drawn: Vec<(Shown, Option<Arc<Image>>)> = shown
        .into_iter()
        .map(|shown| {
            let c = &interface.controls[shown.control];
            let image = if vector { None } else { picture_frame(&shown, c, value(c)) };
            (shown, image)
        })
        .collect();
    // Names the wallpaper wrote beside the controls: ours say them.
    let names = if vector { vector_names(&interface, &pictures) } else { HashMap::new() };
    for (n, (shown, _)) in drawn.iter().enumerate() {
        let c = &interface.controls[shown.control];
        // What its text sits on: its own picture, else what is under it.
        let (cx_, cy) = (shown.x + shown.w / 2., shown.y + shown.h / 2.);
        let under = [(-0.25, 0.), (0., 0.), (0.25, 0.)]
            .iter()
            .filter_map(|(dx, _)| luma_under(&drawn[..=n], wallpaper.as_deref(), cx_ + dx * shown.w, cy))
            .fold(None, |m: Option<(f32, u32)>, l| Some(m.map_or((l, 1), |(t, k)| (t + l, k + 1))))
            .map(|(t, k)| t / k as f32);
        // A name the script's own text already gives beside it is not repeated.
        let near = |o: &Shown| {
            let t = &interface.controls[o.control];
            o.kind == Kind::Label && !prop(t, "$CONTROL_PAR_TEXT").trim().is_empty()
                && o.x < shown.x + shown.w + 8. && o.x + o.w > shown.x - 8.
                && o.y < shown.y + shown.h + FONT * 2. && o.y + o.h > shown.y - FONT * 2.
        };
        let name = names.get(&shown.control).filter(|_| !drawn.iter().any(|(o, _)| near(o)));
        let look = if vector { Look::Vector(name.map(String::as_str)) } else { Look::Original(under) };
        layers.push(control(ui, cx, slot, shown, c, s, look).at(shown.x * s, shown.y * s));
    }
    // Over everything: the value a drag is setting, or a value being typed.
    let find = |n: usize| drawn.iter().find(|(d, _)| d.control == n).map(|(d, _)| d.clone());
    if let Some((shown, v)) = cx.state.held.filter(|(p, ..)| *p == slot).and_then(|(_, n, v)| Some((find(n)?, v))) {
        let c = &interface.controls[shown.control];
        let label = prop(c, "$CONTROL_PAR_LABEL");
        let said = if label.is_empty() || shown.kind == Kind::Value { format!("{}", v.round()) } else { keep_spaces(label) };
        let tag = text(said).text_size(FONT * s).fill(Role::Ink).lines(1).pad((2. * s, 4. * s)).fill(Role::Raised);
        let (wide, tall) = ((shown.w * s).max(80. * s), FONT * s * 1.8);
        let y = if shown.y * s >= tall { shown.y * s - tall } else { (shown.y + shown.h) * s };
        layers.push(row![tag].justify(Justify::Center).w(wide).h(tall).at(shown.x * s + shown.w * s / 2. - wide / 2., y));
    }
    if let Some(shown) = cx.state.typing.as_ref().filter(|(p, ..)| *p == slot).and_then(|(_, n, _)| find(*n)) {
        let el = type_in(ui, cx, slot, &shown, &interface.controls[shown.control], s);
        layers.push(el.at(shown.x * s, shown.y * s));
    }
    let area = stack(layers)
        .w(w * s)
        .h(h * s)
        .shrink(0)
        .fill(if vector { Fill::from(Role::Background) } else { Fill::from(Color::srgb(0., 0., 0.)) })
        .clip()
        .named(if vector { "Vectorized performance view" } else { "Original performance view" });
    row![area].justify(Justify::Center).align(Align::Start).w(Len::Pct(100.))
}

/// A value edit's number being typed: Enter or a click away sets it, Esc
/// leaves it.
fn type_in(ui: &mut Ui, cx: &mut Cx, slot: usize, shown: &Shown, c: &Control, s: f64) -> El {
    let id = format!("kpv-type-{slot}");
    let existed = ui.scene().and_then(|sc| sc.surface(&id)).is_some();
    if !existed {
        ui.focus(id.as_str());
    }
    let Some((_, _, typed)) = cx.state.typing.as_mut() else { return block(0, 0) };
    let field = text_edit(ui, id.as_str(), typed, TextOpts::default());
    let cancel = ui.keys(id.as_str()).iter().any(|k| k.key == Key::Escape);
    if cancel {
        cx.state.typing = None;
    } else if field.changed.submitted || (existed && !ui.focused(id.as_str())) {
        let typed = cx.state.typing.take().map(|(.., t)| t).unwrap_or_default();
        let (min, max) = range(shown.kind, c);
        if let Ok(v) = typed.trim().parse::<f64>() {
            cx.p.shared.edit_control(slot, shown.control, v.clamp(min.min(max), min.max(max)).round() as i32);
        }
    }
    field.el.w(shown.w * s).h(shown.h * s).named("Type a value")
}

/// Text as the script spaced it (scripts pad with spaces to clear an icon),
/// without glyphs from a library's private icon font, which ours can't draw.
fn keep_spaces(text: &str) -> String {
    text.chars()
        .filter(|c| !matches!(*c as u32, 0xE000..=0xF8FF))
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim_end()
        .to_owned()
}

/// The text a control shows, its alignment (0 left, 1 centre, 2 right)
/// and its offset from the top, if the script set one.
pub(super) fn caption_of(c: &Control, kind: Kind, value: f64) -> (String, i32, Option<f64>) {
    let hide = int(c, "$CONTROL_PAR_HIDE").unwrap_or(0);
    let own = prop(c, "$CONTROL_PAR_TEXT").to_owned();
    let words = match kind {
        Kind::Label | Kind::Switch | Kind::Button => own,
        // A divider ("-----", "--- PASTE ---") that shares the value a
        // script parks its menu on (-1) is no choice to show.
        Kind::Menu => (c.menu.iter())
            .find(|(_, v)| f64::from(*v) == value)
            .map(|(t, _)| t.clone())
            .filter(|t| !t.trim().starts_with("--"))
            .unwrap_or_default(),
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
    // A label keeps its lines (add_text_line, or a script's "\n").
    let words = if kind == Kind::Label {
        let lines = words.replace("\r\n", "\n").replace('\r', "\n").replace("\\n", "\n");
        lines.split('\n').map(keep_spaces).collect::<Vec<_>>().join("\n").trim_end().to_owned()
    } else {
        keep_spaces(&words)
    };
    (words, align, top)
}

/// One line of a label's text, in authored points.
pub(super) const LINE: f64 = FONT * 1.25;

/// `text` as the lines a label `room` points wide shows at `size`: one per
/// newline and, when `wrap`, a new one before a word that would run past
/// the edge. A word wider than the room keeps a line of its own.
pub fn break_lines(text: &str, room: f64, size: f64, wrap: bool) -> Vec<String> {
    let mut out = Vec::new();
    for paragraph in text.split('\n') {
        let mut line: Option<String> = None;
        // Split on single spaces so a script's padding stays as it spaced it.
        for word in paragraph.split(' ') {
            line = Some(match line {
                None => word.to_owned(),
                Some(l) if wrap && !l.trim().is_empty() && super::cover::advance(&format!("{l} {word}"), size) > room => {
                    out.push(l);
                    word.to_owned()
                }
                Some(l) => format!("{l} {word}"),
            });
        }
        out.push(line.unwrap_or_default());
    }
    out
}

/// Text on a control `w` by `h` (scaled), aligned as the script asked; a
/// label's on as many lines as its newlines and its height make, from
/// `top` or centred.
#[allow(clippy::too_many_arguments)]
fn words(words: String, align: i32, top: Option<f64>, w: f64, h: f64, s: f64, ink: impl Into<Fill>, label: bool) -> El {
    let ink: Fill = ink.into();
    let justify = match align {
        1 => Justify::Center,
        2 => Justify::End,
        _ => Justify::Start,
    };
    let lh = LINE * s;
    let lines = if label { break_lines(&words, w - 4. * s, FONT * s, h >= 2. * lh) } else { vec![words] };
    let one = |t: String| {
        let size = fit(&t, w - 4. * s, FONT * s);
        row![text(t).text_size(size).fill(ink.clone()).lines(1).min_w(0)].justify(justify).align(Align::Center).w(w).pad((2. * s, 0.))
    };
    if lines.len() > 1 {
        let tall = lh * lines.len() as f64;
        let block = col(lines.into_iter().map(|t| one(t).h(lh))).gap(0).w(w).h(tall);
        return block.at(0., top.map_or((h - tall) / 2., |y| y * s));
    }
    let line = one(lines.into_iter().next().unwrap_or_default());
    match top {
        Some(y) => line.h(FONT * s * 1.4).at(0., y * s),
        None => line.h(h).at(0., 0.),
    }
}

/// The size `text` is set at to fit `room` points: `size`, or smaller down
/// to three quarters of it. Kontakt's own fonts are narrower than ours.
pub fn fit(text: &str, room: f64, size: f64) -> f64 {
    let wide = super::cover::advance(text, size);
    if wide <= room || wide <= 0. { size } else { (size * room / wide).max(size * 0.75) }
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

/// A picture's frame on a control `w` by `h` points: whole when it keeps its
/// size, else cut along each way it stretches into two ends kept as drawn
/// and the middle pixel (or two) stretched between them: a stretched menu
/// keeps its rounded ends and its arrow, a one-pixel divider its width.
// ponytail: cut again on every build; cache the cuts if big stretched
// pictures show up in profiles.
fn sliced(image: &Arc<Image>, stretch: [bool; 2], w: f64, h: f64, s: f64) -> El {
    let (iw, ih) = (image.width, image.height);
    let cuts = |on: bool, own: u32, to: f64| -> Vec<(u32, u32, f64)> {
        let end = own.saturating_sub(1) / 2;
        let e = f64::from(end) * s;
        if !on || end == 0 || (to - f64::from(own) * s).abs() < 0.5 || to <= 2. * e {
            return vec![(0, own, to)];
        }
        vec![(0, end, e), (end, own - 2 * end, to - 2. * e), (own - end, end, e)]
    };
    let (across, down) = (cuts(stretch[0], iw, w), cuts(stretch[1], ih, h));
    if across.len() == 1 && down.len() == 1 {
        return block(w, h).fill(Fill::Image(image.clone(), Fit::Fill));
    }
    let mut parts = Vec::new();
    let mut y = 0.;
    for &(sy, sh, th) in &down {
        let mut x = 0.;
        for &(sx, sw, tw) in &across {
            if let Some(piece) = crate::artwork::crop(image, sx, sy, sw, sh) {
                parts.push(block(tw, th).fill(Fill::Image(piece, Fit::Fill)).at(x, y));
            }
            x += tw;
        }
        y += th;
    }
    stack(parts).w(w).h(h)
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

/// How a control is drawn.
#[derive(Clone, Copy)]
enum Look<'a> {
    /// Its picture, or KONTRA's face where it has none; how light what
    /// its text sits on is.
    Original(Option<f32>),
    /// KONTRA's face, with the name the wallpaper wrote beside it.
    Vector(Option<&'a str>),
}

/// The names the vectorized view writes under knobs: the ones KONTRA's
/// own view finds, unless one of the script's labels says it already.
fn vector_names(interface: &Interface, pictures: &HashMap<String, Arc<Picture>>) -> HashMap<usize, String> {
    let said: std::collections::HashSet<String> = (interface.controls.iter())
        .filter(|c| c.kind == "ui_label")
        .map(|c| keep_spaces(prop(c, "$CONTROL_PAR_TEXT")).trim().to_lowercase())
        .collect();
    super::panel::names(interface, pictures)
        .into_iter()
        .filter(|(_, n)| !said.contains(&n.trim().to_lowercase()))
        .collect()
}

/// One control: its picture's frame or KONTRA's face, its text, and its
/// pointer handling.
fn control(ui: &mut Ui, cx: &mut Cx, slot: usize, shown: &Shown, c: &Control, s: f64, look: Look) -> El {
    let vector = matches!(look, Look::Vector(_));
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
            let behaviour = int(c, "$CONTROL_PAR_MOUSE_BEHAVIOUR").unwrap_or(0);
            let vertical = drags_vertically(shown.kind, shown.w, shown.h, prop(c, "$CONTROL_PAR_PICTURE"), behaviour);
            let before = now;
            let held = drive(ui, &id, &mut now, &(lo..=hi), travel(shown.kind, behaviour, hi - lo) * s, vertical, reset);
            if shown.kind == Kind::Value && ui.get(id.as_str()).double_clicked {
                // A value edit types on a double-click, as Kontakt's does.
                now = before;
                cx.state.typing = Some((slot, shown.control, format!("{}", raw.round())));
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
    let picture = if vector { None } else { picture_frame(shown, c, now) };
    let pictured = picture.is_some();
    let behaviour = int(c, "$CONTROL_PAR_MOUSE_BEHAVIOUR").unwrap_or(0);
    let vertical = drags_vertically(shown.kind, shown.w, shown.h, prop(c, "$CONTROL_PAR_PICTURE"), behaviour);
    let round = shown.kind == Kind::Knob || shown.kind == Kind::Slider && knob_like(prop(c, "$CONTROL_PAR_PICTURE"), shown.w, shown.h);
    let lift = if cx.state.held.is_some_and(|(p, n, _)| (p, n) == (slot, shown.control)) { 1. } else { ui.state(id.as_str()).hover as f32 };
    let mut layers = vec![match picture {
        Some(image) => sliced(&image, shown.picture.as_ref().map_or([false; 2], |p| p.stretch), w, h, s),
        None => face(shown.kind, c, now, lo, hi, vertical, round, s, lift).w(w).h(h),
    }];
    // Text on our face is our ink; on a picture it reads what lies under it.
    let own_ink = match look {
        Look::Original(under) if pictured || matches!(shown.kind, Kind::Label | Kind::Area) => Fill::from(ink(under)),
        _ => Fill::from(Role::Ink),
    };
    let (mut said, align, top) = caption_of(c, shown.kind, now);
    // A switch whose words were in its picture says its name instead.
    if let Look::Vector(Some(name)) = look
        && said.is_empty()
        && matches!(shown.kind, Kind::Switch | Kind::Button)
    {
        said = name.to_owned();
    }
    if !said.is_empty() {
        layers.push(words(said, align, top, w, h, s, own_ink.clone(), shown.kind == Kind::Label));
    }
    if shown.kind == Kind::Knob && !pictured {
        // Kontakt's own knob: its name over it, its value under it.
        let name = prop(c, "$CONTROL_PAR_TEXT");
        let name = if name.is_empty() { c.variable.trim_start_matches(['$', '~']) } else { name };
        if hide & HIDE_TITLE == 0 {
            layers.push(words(keep_spaces(name), 1, Some(0.), w, h, s, own_ink.clone(), false));
        }
        if hide & HIDE_VALUE == 0 {
            let label = prop(c, "$CONTROL_PAR_LABEL");
            let shown_value = if label.is_empty() { format!("{}", now.round()) } else { keep_spaces(label) };
            layers.push(words(shown_value, 1, Some(shown.h - FONT * 1.4), w, h, s, own_ink, false));
        }
    }
    let mut el = stack(layers).w(w).h(h).clip();
    // A knob the wallpaper named: the name under it, where the wallpaper had it.
    if let Look::Vector(Some(name)) = look
        && round
        && shown.w * s >= 16.
        && shown.kind != Kind::Knob
    {
        let tag = words(name.to_owned(), 1, None, (shown.w * s).max(72. * s), FONT * s * 1.4, s, secondary(), false);
        let off = (w - (shown.w * s).max(72. * s)) / 2.;
        el = stack![el, tag.at(off, h)].w(w).h(h);
    }
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
            Kind::Knob | Kind::Slider | Kind::Value if ui.get(id.as_str()).held => Cursor::Grabbing,
            Kind::Knob | Kind::Slider | Kind::Value if vertical => Cursor::ResizeV,
            Kind::Knob | Kind::Slider | Kind::Value => Cursor::ResizeH,
            _ => Cursor::Hand,
        })
        .named(name);
    let el = if help.is_empty() { el } else { el.tip(help.to_owned()) };
    el.captures_wheel().id(id)
}

/// A control in KONTRA's own look, filling its authored rect: what the
/// vectorized view draws, and the original where a picture is missing.
/// Nothing in it is translucent over what lies under it but its hairlines,
/// so it never greys out a picture beneath.
#[allow(clippy::too_many_arguments)]
fn face(kind: Kind, c: &Control, value: f64, lo: f64, hi: f64, vertical: bool, round: bool, s: f64, lift: f32) -> El {
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
    let hide = int(c, "$CONTROL_PAR_HIDE").unwrap_or(0);
    let bg = hide & HIDE_BG == 0;
    // A meter's own colours, 0AARRGGBBh; a script that leaves the alpha
    // out means it opaque.
    let colour = |k: &str| {
        int(c, k).map(|v| {
            let v = v as u32;
            let byte = |shift: u32| ((v >> shift) & 0xff) as f32 / 255.;
            let a = if v >> 24 == 0 { 1. } else { byte(24) };
            Color::srgba(byte(16), byte(8), byte(0), a)
        })
    };
    let meter = (colour("$CONTROL_PAR_BG_COLOR"), colour("$CONTROL_PAR_OFF_COLOR"));
    let wave = colour("$CONTROL_PAR_WAVE_COLOR");
    canvas(move |z| {
        let (w, h) = (z.width, z.height);
        let weight = (1.5 * s).max(1.);
        let edge = Role::Ink.alpha(0.12 + 0.08 * lift);
        let mut d = Vec::new();
        let boxed = |d: &mut Vec<Draw>, fill: Fill| {
            d.push(Draw::fill(rect(0., 0., w, h), fill));
            d.push(Draw::stroke(rect(0.5, 0.5, w - 1., h - 1.), edge.clone(), 1.));
        };
        match kind {
            Kind::Knob | Kind::Slider if round => {
                // Kontakt's own knob leaves rows for its name and value.
                let text = if kind == Kind::Knob { FONT * 1.4 * s } else { 0. };
                let (cx, cy) = (w / 2., h / 2.);
                let r = (w.min(h - 2. * text) / 2. - weight).max(2.);
                let (start, sweep) = (0.75 * std::f64::consts::PI, 1.5 * std::f64::consts::PI);
                d.push(Draw::fill(circle(cx, cy, r - weight * 1.5), Role::Raised.alpha(1.)));
                d.push(Draw::stroke(arc(cx, cy, r, start, sweep), Role::Ink.alpha(0.16 + 0.08 * lift), weight));
                if t > 0.002 {
                    d.push(Draw::stroke(arc(cx, cy, r, start, sweep * t), value_ink(0.), weight));
                }
                let angle = start + sweep * t;
                let (inner, outer) = (r * 0.2, r - weight * 2.);
                d.push(Draw::stroke(
                    DrawPath::polyline(
                        [
                            Point::new(cx + angle.cos() * inner, cy + angle.sin() * inner),
                            Point::new(cx + angle.cos() * outer, cy + angle.sin() * outer),
                        ],
                        false,
                    ),
                    value_ink(lift),
                    weight,
                ));
            }
            Kind::Knob | Kind::Slider => {
                // A thin track along its length, filled to a square thumb.
                let (len, across) = if vertical { (h, w) } else { (w, h) };
                let place = |along: f64, off: f64, l: f64, th: f64| {
                    if vertical { rect(off, len - along - l, th, l) } else { rect(along, off, l, th) }
                };
                let thumb = (across * 0.6).clamp(4., 12. * s).min(len / 3.);
                let inner = len - thumb;
                let mid = (across / 2.).round();
                let at = thumb / 2. + t * inner;
                d.push(Draw::fill(place(thumb / 2., mid - 1., inner, 2.), Role::Ink.alpha(0.16 + 0.08 * lift)));
                d.push(Draw::fill(place(thumb / 2., mid - 1., at - thumb / 2., 2.), value_ink(0.)));
                let th = (across - 2.).min(thumb * 1.4).max(2.);
                d.push(Draw::fill(place(at - thumb / 2., mid - th / 2., thumb, th), value_ink(lift)));
            }
            Kind::Switch | Kind::Button => {
                // On: raised, edged and marked in the accent; off: a field.
                if on {
                    d.push(Draw::fill(rect(0., 0., w, h), Role::Raised.alpha(1.)));
                    d.push(Draw::stroke(rect(0.5, 0.5, w - 1., h - 1.), accent(), 1.));
                    d.push(Draw::fill(rect(0., 0., (2. * s).max(2.), h), accent()));
                } else {
                    boxed(&mut d, Role::Field.alpha(1.));
                }
            }
            Kind::Menu => {
                boxed(&mut d, Role::Field.alpha(1.));
                let (x, y, k) = (w - 4. * s - 6. * s, h / 2., 3. * s);
                if w > 30. * s {
                    d.push(Draw::fill(
                        DrawPath::polyline([Point::new(x, y - k / 2.), Point::new(x + 2. * k, y - k / 2.), Point::new(x + k, y + k / 2.)], true),
                        secondary(),
                    ));
                }
            }
            Kind::Value | Kind::TextEdit => boxed(&mut d, Role::Field.alpha(1.)),
            Kind::Table => {
                boxed(&mut d, Role::Field.alpha(1.));
                let top = bars.iter().fold(1f64, |m, v| m.max(v.abs()));
                let bw = w / bars.len().max(1) as f64;
                for (n, v) in bars.iter().enumerate() {
                    let bh = (v.abs() / top).min(1.) * (h - 2.);
                    d.push(Draw::fill(rect(n as f64 * bw + 1., h - 1. - bh, (bw - 1.).max(1.), bh), value_ink(0.)));
                }
            }
            // A box Kontakt draws for a meter, a waveform, a pad: outlined
            // only, so it covers nothing.
            Kind::Other if bg => d.push(Draw::stroke(rect(0.5, 0.5, w - 1., h - 1.), edge, 1.)),
            // Unlit, as at silence: its background, its off colour inside.
            Kind::Meter => {
                d.push(Draw::fill(rect(0., 0., w, h), meter.0.map_or(Role::Field.alpha(1.), Fill::from)));
                if let Some(off) = meter.1 {
                    d.push(Draw::fill(rect(0., 0., w, h), off));
                }
            }
            Kind::Waveform => {
                if bg {
                    d.push(Draw::fill(rect(0., 0., w, h), meter.0.map_or(Role::Field.alpha(1.), Fill::from)));
                }
                d.push(Draw::fill(rect(0., (h / 2.).round(), w, 1.), wave.map_or(secondary(), Fill::from)));
            }
            Kind::Label | Kind::Area | Kind::Other => {}
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
            id: 0,
            variable: "$c".into(),
            kind: kind.into(),
            properties: props.iter().map(|(k, v)| (format!("$CONTROL_PAR_{k}"), v.clone())).collect::<BTreeMap<_, _>>(),
            menu: Vec::new(),
        }
    }

    #[test]
    fn a_menu_parked_on_a_divider_shows_nothing() {
        let menu = |items: &[(&str, i32)]| Control {
            menu: items.iter().map(|(t, v)| ((*t).to_owned(), *v)).collect(),
            ..control("ui_menu", &[])
        };
        let m = menu(&[("------------ PASTE ------------", -1), ("Layer 1", 0)]);
        assert_eq!(caption_of(&m, Kind::Menu, -1.).0, "");
        assert_eq!(caption_of(&m, Kind::Menu, 0.).0, "Layer 1");
        assert_eq!(Kind::of("ui_level_meter"), Kind::Meter);
        assert_eq!(Kind::of("ui_waveform"), Kind::Waveform);
    }

    fn picture(w: u32, h: u32, frames: usize, resizable: bool) -> Arc<Picture> {
        let frame = Arc::new(Image::rgba(w, h, vec![0u8; (w * h * 4) as usize]).unwrap());
        Arc::new(Picture { frames: vec![frame; frames], stretch: [resizable; 2] })
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
    fn drags_run_as_in_kontakt() {
        assert!(drags_vertically(Kind::Knob, 200., 20., "", 500), "a knob drags up and down whatever its behaviour");
        assert!(drags_vertically(Kind::Slider, 48., 50., "", 800), "a slider drawn as a knob too");
        assert!(drags_vertically(Kind::Slider, 120., 20., "big_knob", 0), "named a knob");
        assert!(!drags_vertically(Kind::Slider, 120., 20., "fader", 0), "a wide slider drags across");
        assert!(drags_vertically(Kind::Slider, 20., 120., "fader", 0), "a tall one up and down");
        assert!(drags_vertically(Kind::Slider, 120., 20., "fader", -500), "negative behaviour is vertical");
        assert!(!drags_vertically(Kind::Slider, 20., 120., "fader", 500), "positive is horizontal");
        assert!(drags_vertically(Kind::Value, 80., 18., "", 0), "a value edit drags up and down");
        assert_eq!(travel(Kind::Slider, 0, 127.), 200.);
        assert_eq!(travel(Kind::Slider, -1000, 127.), 100.);
        assert_eq!(travel(Kind::Slider, 5000, 127.), 60., "the fastest still has room");
        assert_eq!(travel(Kind::Value, 0, 10.), 60.);
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
    fn panels_place_and_hide_what_is_in_them() {
        let with_id = |id: i32, kind: &str, props: &[(&str, Value)]| Control { id, ..control(kind, props) };
        let at = |x: i32, y: i32, parent: i32| vec![("POS_X", Value::Int(x)), ("POS_Y", Value::Int(y)), ("PARENT_PANEL", Value::Int(parent))];
        let mut interface = Interface {
            performance: true,
            width: 600,
            height: 300,
            controls: vec![
                with_id(32768, "ui_panel", &[("POS_X", Value::Int(100)), ("POS_Y", Value::Int(50))]),
                with_id(32769, "ui_panel", &at(10, 10, 32768)),
                with_id(32770, "ui_knob", &at(5, 5, 32769)),
                with_id(32771, "ui_button", &at(0, 0, 32768)),
                // Not a panel: its parent ID is ignored.
                with_id(32772, "ui_switch", &at(7, 8, 32771)),
                // A panel in itself does not loop.
                with_id(32773, "ui_panel", &at(1, 1, 32773)),
            ],
            ..Interface::default()
        };
        let pictures = HashMap::new();
        let spots = |i: &Interface| layout(i, &pictures).iter().map(|s| (s.control, s.x, s.y)).collect::<Vec<_>>();
        assert_eq!(spots(&interface), [(2, 115., 65.), (3, 100., 50.), (4, 7., 8.)], "children from their panels' corners; panels draw nothing");
        // The inner panel hidden hides its knob; the outer hides both.
        interface.controls[1].properties.insert("$CONTROL_PAR_HIDE".into(), Value::Int(HIDE_WHOLE));
        assert_eq!(spots(&interface), [(3, 100., 50.), (4, 7., 8.)]);
        interface.controls[1].properties.insert("$CONTROL_PAR_HIDE".into(), Value::Int(0));
        interface.controls[0].properties.insert("$CONTROL_PAR_HIDE".into(), Value::Int(HIDE_WHOLE));
        assert_eq!(spots(&interface), [(4, 7., 8.)]);
    }

    #[test]
    fn a_part_keeps_its_mode_and_follows_the_default_otherwise() {
        use ViewMode::*;
        assert_eq!(mode(0, Original, true), Original);
        assert_eq!(mode(0, Vectorized, true), Vectorized, "the app's default");
        assert_eq!(mode(1, Kontra, true), Original, "a part's own choice wins");
        assert_eq!(mode(2, Original, true), Kontra);
        assert_eq!(mode(3, Original, true), Vectorized);
        assert_eq!(mode(3, Original, false), Kontra, "nothing to show: the rebuilt view");
        for m in [None, Some(Original), Some(Vectorized), Some(Kontra)] {
            assert_eq!(mode(code(m), Original, true), m.unwrap_or(Original), "{m:?} survives as a part's code");
        }
        assert_eq!((code(Some(Original)), code(Some(Kontra))), (1, 2), "codes saved before the vectorized view keep their meaning");
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
    fn labels_break_at_newlines_and_wrap_when_tall() {
        let label = control("ui_label", &[("TEXT", Value::Text("Mic\r\nPosition\\nClose".into()))]);
        assert_eq!(caption_of(&label, Kind::Label, 0.).0, "Mic\nPosition\nClose", "add_text_line, CRLF and a script's \\n");
        let switch = control("ui_switch", &[("TEXT", Value::Text("On\nOff".into()))]);
        assert_eq!(caption_of(&switch, Kind::Switch, 0.).0, "On Off", "only labels keep lines");
        let a = crate::ui::cover::advance("a", FONT);
        let room = crate::ui::cover::advance("aa aa", FONT) + a / 2.;
        assert_eq!(break_lines("aa aa aa", room, FONT, true), ["aa aa", "aa"], "wraps before the word that runs past the edge");
        assert_eq!(break_lines("aa aa aa", room, FONT, false), ["aa aa aa"], "too short to wrap: one line");
        assert_eq!(break_lines("aaaaaaaaaa b", room, FONT, true), ["aaaaaaaaaa", "b"], "a long word keeps its own line");
        assert_eq!(break_lines("x\n\n  y", 1000., FONT, true), ["x", "", "  y"], "blank lines and padding kept");
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
