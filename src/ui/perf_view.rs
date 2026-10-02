//! A script's performance view as its library drew it: the wallpaper and
//! every control at its own pixel position, in its own pictures, scaled to
//! the part. A control without its picture, or of a kind that has none, is
//! drawn plainly in its place. Edits reach the script the way the rebuilt
//! view's do ([`crate::plugin::Shared::edit_control`]), so its
//! `on ui_control` runs.

use super::menu::{self, Target};
use super::vector::{Face as VFace, Plan};
use super::wave::Peaks;
use super::{Cx, fitted, theme::*};
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

/// The bundled face's default approximation; factory glyphs are not bundled.
pub(super) const FONT: f64 = 11.;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
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
    FileSelector,
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
            "ui_file_selector" => Self::FileSelector,
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

/// How far a drag runs the whole range, in authored pixels. A real slider
/// follows its drawn track; mouse behaviour selects its axis. Knobs (also
/// sliders drawn as knobs) retain the scripted speed, and value edits run
/// a few pixels a step.
pub fn travel(kind: Kind, behaviour: i32, span: f64, size: (f64, f64), picture: &str) -> f64 {
    let (w, h) = size;
    if kind == Kind::Slider && !knob_like(picture, w, h) {
        return if drags_vertically(kind, w, h, picture, behaviour) { h } else { w };
    }
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

/// Automatic zoom fits the whole authored view; explicit zoom stays scrollable.
fn scale_to_fit(room: Size, authored: Size, setting: f32) -> f64 {
    let width = if room.height > 0. && authored.height > 0. {
        room.width.min(room.height / authored.height * authored.width)
    } else { room.width };
    scale(width, authored.width, setting)
}

/// Where control `n` sits on the view, and whether it is hidden: a control
/// in a `ui_panel` (`$CONTROL_PAR_PARENT_PANEL`) is placed from its panel's
/// corner, and hidden with it, all the way up.
// ponytail: finds each parent by a scan; index the IDs if views with
// thousands of nested controls show up.
pub(super) fn placed(interface: &Interface, n: usize) -> (f64, f64, bool) {
    let (mut x, mut y, mut at, mut hidden) = (0., 0., n, false);
    // A valid parent chain cannot be longer than the control list.
    for _ in 0..interface.controls.len() {
        let c = &interface.controls[at];
        hidden |= int(c, "$CONTROL_PAR_HIDE").unwrap_or(0) & HIDE_WHOLE != 0;
        x += f64::from(int(c, "$CONTROL_PAR_POS_X").unwrap_or(0));
        y += f64::from(int(c, "$CONTROL_PAR_POS_Y").unwrap_or(0));
        let parent = int(c, "$CONTROL_PAR_PARENT_PANEL")
            .and_then(|id| interface.controls.iter().position(|p| p.id == id && p.kind == "ui_panel"));
        match parent {
            Some(p) if p != at => at = p,
            _ => return (x, y, hidden),
        }
    }
    // Cyclic panels cannot be placed; keep their children out of the view.
    (x, y, true)
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
/// a script's view with authored pictures or a background color.
pub fn available(v: &crate::plugin::PartView) -> bool {
    v.interface.as_ref().is_some_and(|i| i.performance && !i.controls.is_empty()
        && (i.background_color.is_some() || v.wallpaper.is_some() || !v.pictures.is_empty()))
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

/// The rack's height minus the actual header/notices before this stage.
/// Relative frames remain stable while the rack scrolls or its part animates.
fn room_height(ui: &Ui, slot: usize) -> f64 {
    let Some(scene) = ui.scene() else { return 0. };
    let (Some(rack), Some(part), Some(stage)) = (scene.surface("rack-view"),
        scene.surface(&format!("part-{slot}")), scene.surface(&format!("stage-{slot}"))) else { return 0. };
    let tabs = scene.surface(&format!("script-pages-{slot}")).map_or(0., |s| s.frame.size.height);
    (rack.frame.size.height - (stage.frame.y - part.frame.y).max(0.) - tabs).max(0.)
}

/// Reference cost of the former scalar/table memo scan, measured by the opt-in UI probe.
#[cfg(test)]
pub(super) fn hash_properties(i: &Interface, h: &mut DefaultHasher) {
    for c in &i.controls {
        for (k, v) in &c.properties {
            // Scalar properties include layout, text, picture and colors.
            // Hash directly: formatting them allocates on every redraw.
            k.hash(h);
            match v {
                Value::Int(n) => n.hash(h),
                Value::Real(n) => n.to_bits().hash(h),
                Value::Text(t) => t.hash(h),
                Value::IntArray(a) if c.kind == "ui_table" => a.hash(h),
                Value::RealArray(a) if c.kind == "ui_table" => {
                    for n in a { n.to_bits().hash(h); }
                }
                Value::Array(a) if c.kind == "ui_table" => {
                    for v in a {
                        match v {
                            Value::Int(n) => n.hash(h),
                            Value::Real(n) => n.to_bits().hash(h),
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

/// Everything [`view`] reads beyond the frame's input, hashed.
pub fn deps(ui: &Ui, cx: &Cx, slot: usize) -> u64 {
    let v = &cx.view.parts[slot];
    let mut h = DefaultHasher::new();
    (room(ui, slot).round() as i64, room_height(ui, slot).round() as i64, cx.settings.view_scale.to_bits()).hash(&mut h);
    shows(cx, slot).hash(&mut h);
    v.live_revisions.hash(&mut h);
    for edit in v.edited_values() { edit.hash(&mut h); }
    (Arc::as_ptr(&v.pictures) as usize, v.wallpaper.as_ref().map(|w| Arc::as_ptr(w) as usize)).hash(&mut h);
    if let Some(i) = &v.interface {
        (Arc::as_ptr(i) as usize, i.width, i.height, i.wallpaper_state, i.skin_offset).hash(&mut h);
        // Published interfaces are immutable; source revisions and pending
        // scalar pairs above invalidate drawing without rescanning properties.
    }
    // A wave read since.
    if let (Some(i), Some(u)) = (&v.instrument, &v.interface) {
        for c in u.controls.iter().filter(|c| c.kind == "ui_waveform") {
            attached(Some(i), c).and_then(super::wave::ask).is_some().hash(&mut h);
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
    let names: Vec<String> = crate::artwork::picture_names(interface)
        .filter(|name| !v.pictures.contains_key(name.as_ref()))
        .filter(|name| cx.state.perf_asked.insert((path.clone(), name.to_string())))
        .map(std::borrow::Cow::into_owned)
        .collect();
    if names.is_empty() {
        return;
    }
    let p = cx.p.clone();
    let program = v.program;
    let Some(shared) = p.shared.part(slot) else { return };
    let generation = shared.generation.load(std::sync::atomic::Ordering::Acquire);
    let _ = std::thread::Builder::new().name("kontakto-pictures".into()).spawn(move || {
        let mut trace = crate::diagnostics::LoadTrace::new(&path, program, Some(slot));
        trace.detail("operation", "control_pictures");
        trace.stage("artwork");
        let (found, errors) = crate::artwork::pictures_report(&path, names.iter().map(String::as_str));
        for e in &errors { trace.issue("artwork", crate::diagnostics::code(e), e); }
        trace.detail("pictures_loaded", found.len());
        let report = trace.finish("loaded");
        let mut view = p.shared.view.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let v = &mut view.parts[slot];
        if v.instrument.as_ref().is_some_and(|i| i.path == path) && v.program == program
            && shared.generation.load(std::sync::atomic::Ordering::Acquire) == generation
        {
            if !found.is_empty() {
                let mut all = (*v.pictures).clone();
                all.extend(found);
                v.pictures = Arc::new(all);
            }
            if !errors.is_empty() && let Some(load) = &mut v.load_report {
                let load = Arc::make_mut(load);
                if load["status"] == "loaded" { load["status"] = "partial".into(); }
                for issue in report["issues"].as_array().into_iter().flatten() {
                    let issues = load["issues"].as_array_mut().unwrap();
                    if !issues.contains(issue) { issues.push(issue.clone()); }
                }
            }
        }
    });
}

/// A published row's stamps cover its properties, menu and dense value. The
/// interface identity remains the fallback for fixtures without publication stamps.
fn control_deps(cx: &Cx, slot: usize, interface: &Arc<Interface>, shown: &Shown, scale: (f64, f64), resources: u64, under: Option<f32>) -> u64 {
    let part = &cx.view.parts[slot];
    let mut h = DefaultHasher::new();
    (part.script_epoch, part.script_slot, shown.control, shown.kind).hash(&mut h);
    if part.live_control_versions.len() == interface.controls.len() {
        part.live_control_versions[shown.control].hash(&mut h);
    } else {
        (Arc::as_ptr(interface) as usize).hash(&mut h);
    }
    (shown.x.to_bits(), shown.y.to_bits(), shown.w.to_bits(), shown.h.to_bits(), shown.z,
        scale.0.to_bits(), scale.1.to_bits(), resources, under.map(f32::to_bits)).hash(&mut h);
    part.control_value(shown.control).map(f64::to_bits).hash(&mut h);
    cx.state.held.filter(|&(p, n, _)| (p, n) == (slot, shown.control))
        .map(|(_, _, value)| value.to_bits()).hash(&mut h);
    cx.state.menu.as_ref().is_some_and(|menu| matches!(&menu.target,
        Target::Script { part, control } if *part == slot && *control == shown.control)).hash(&mut h);
    if shown.kind == Kind::Waveform {
        attached(part.instrument.as_deref(), &interface.controls[shown.control])
            .and_then(super::wave::ask).map(|wave| Arc::as_ptr(&wave) as usize).hash(&mut h);
    }
    h.finish()
}

/// `slot`'s performance view as its library drew it, or vectorized: the
/// same controls in the same places in KONTRA's own look.
pub fn view(ui: &mut Ui, cx: &mut Cx, slot: usize) -> El {
    let vector = shows(cx, slot) == ViewMode::Vectorized;
    let v = &cx.view.parts[slot];
    let (Some(interface), pictures, wallpaper) = (v.interface.clone(), v.pictures.clone(), v.wallpaper.as_ref().and_then(|p| { let i = v.interface.as_ref()?; p.wallpaper(i.wallpaper_state, i.skin_offset) })) else {
        return block(0, 0);
    };
    let frame_width = v.wallpaper.as_ref().and_then(|p| p.atlas.map(|a| a[0]));
    fetch(cx, slot, &interface);
    let (w, h) = (f64::from(interface.width), f64::from(interface.height));
    let s = scale_to_fit(Size::new(room(ui, slot), room_height(ui, slot)), Size::new(w,h), cx.settings.view_scale);
    // Control placement lands on whole device pixels. Wallpaper windows retain
    // source pixels and let the renderer apply the view scale.
    let dev = ui.scale().unwrap_or(1.);
    let px = |v: f64| (v * dev).round() / dev;
    let mut layers = Vec::new();
    if let Some((image, origin)) = &wallpaper {
        let x = origin[0];
        let y = origin[1].saturating_add(HEADER as u32);
        let width = frame_width.unwrap_or(image.width);
        let (cw, ch) = ((interface.width.max(0) as u32).min(width).min(image.width.saturating_sub(x)),
            (interface.height.max(0) as u32).min(image.height.saturating_sub(y)));
        // Only the visible window reaches the GPU. Large valid PNG strips can exceed
        // its image-atlas limit; retaining the full strip there is not sufficient.
        if let Some(window) = fitted::window(image, x, y, cw, ch) {
            layers.push(block(px(f64::from(cw) * s), px(f64::from(ch) * s))
                .fill(Fill::Image(window, Fit::Fill)).at(0., 0.));
        }
    }
    // In the original's order either way: what covered a control there
    // covers it here.
    let drawn: Vec<(Shown, Option<Arc<Image>>)> = layout(&interface, &pictures)
        .into_iter()
        .map(|shown| {
            let c = &interface.controls[shown.control];
            let image = picture_frame(&shown, c, cx.view.parts[slot].control_value(shown.control).unwrap_or_else(|| value(c)));
            (shown, image)
        })
        .collect();
    let part = &cx.view.parts[slot];
    let plans = if vector { super::vector::plan(&interface, &pictures, &drawn, &mut cx.state.vector_assets, |n| part.control_value(n)) } else { Vec::new() };
    let resources = if vector { 0 } else {
        // Shared resource dependencies once, rather than scanning each control's
        // properties or invalidating every row on an unrelated publication.
        let mut h = DefaultHasher::new();
        (Arc::as_ptr(&pictures) as usize, part.instrument.as_ref().map(|i| Arc::as_ptr(i) as usize),
            fitted::generation(slot), cx.selection.appearance).hash(&mut h);
        interface.fonts.hash(&mut h);
        h.finish()
    };
    let background_luma = interface.background_color.map(|color|
        (0.2126 * ((color >> 16) & 255) as f32 + 0.7152 * ((color >> 8) & 255) as f32
            + 0.0722 * (color & 255) as f32) / 255.);
    for (n, (shown, _)) in drawn.iter().enumerate() {
        let c = &interface.controls[shown.control];
        let look = match plans.get(n) {
            Some(plan) if super::vector::native_control(shown, c)
                || shown.kind == Kind::Label && plan.face == VFace::Clear => Look::Vector(plan),
            _ => {
                // What its text sits on: its own picture, else what is under it.
                let (cx_, cy) = (shown.x + shown.w / 2., shown.y + shown.h / 2.);
                let under = [-0.25, 0., 0.25]
                    .iter()
                    .filter_map(|dx| luma_under(&drawn[..=n], wallpaper.as_ref().map(|(image, origin)| (image.as_ref(), *origin)), cx_ + dx * shown.w, cy).or(background_luma))
                    .fold(None, |m: Option<(f32, u32)>, l| Some(m.map_or((l, 1), |(t, k)| (t + l, k + 1))))
                    .map(|(t, k)| t / k as f32);
                Look::Original(under)
            }
        };
        let el = if !vector {
            let Look::Original(under) = look else { unreachable!() };
            let deps = control_deps(cx, slot, &interface, shown, (s, dev), resources, under);
            // MUI heats nested memos for hover, focus and pointer capture. A
            // table publication can therefore keep every other control's
            // styled/layout/paint subtree without delaying the active drag.
            ui.memo(format!("kpv-memo-{slot}-{}", shown.control), deps,
                |ui| control(ui, cx, slot, shown, c, s, look))
        } else {
            control(ui, cx, slot, shown, c, s, look)
        };
        layers.push(el.at(px(shown.x * s), px(shown.y * s)));
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
        .w(px(w * s))
        .h(px(h * s))
        .shrink(0)
        .fill({
            let color = interface.background_color.unwrap_or(0);
            Color::srgb(((color >> 16) & 255) as f32 / 255., ((color >> 8) & 255) as f32 / 255., (color & 255) as f32 / 255.)
        })
        .clip()
        .named(if vector { "Vectorized performance view" } else { "Original performance view" });
    // Centred by a whole-pixel inset in the part's width (which the memo
    // over this reads): centring by layout halves odd pixels.
    let inset = (((room(ui, slot) - px(w * s)) / 2. * dev).floor() / dev).max(0.);
    row![area].pad(edges(0., 0., 0., inset)).align(Align::Start).w(Len::Pct(100.))
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
pub(super) fn keep_spaces(text: &str) -> String {
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
        Kind::FileSelector => {
            let path = prop(c, "$CONTROL_PAR_FILEPATH");
            if path.is_empty() { "Select file…".into() } else { path.rsplit('/').next().unwrap_or(path).into() }
        }
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

/// Responsive controls inherit their base font when a state font is unset.
/// Menu "on" means its popup is open, rather than a nonzero selected value.
fn font_id(c: &Control, on: bool, pressed: bool, hovered: bool) -> Option<i32> {
    let base = int(c, "$CONTROL_PAR_FONT_TYPE");
    if !matches!(c.kind.as_str(), "ui_button" | "ui_switch" | "ui_menu") { return base; }
    let state = match (on, pressed, hovered) {
        (false, true, _) => "$CONTROL_PAR_FONT_TYPE_OFF_PRESSED",
        (true, true, _) => "$CONTROL_PAR_FONT_TYPE_ON_PRESSED",
        (false, false, true) => "$CONTROL_PAR_FONT_TYPE_OFF_HOVER",
        (true, false, true) => "$CONTROL_PAR_FONT_TYPE_ON_HOVER",
        (true, false, false) => "$CONTROL_PAR_FONT_TYPE_ON",
        _ => return base,
    };
    int(c, state).filter(|&id| id >= 0).or(base)
}

#[derive(Clone, Copy)]
struct TextFont { size: f64, weight: f32, color: Option<Color> }
impl TextFont {
    fn advance(self, text: &str, size: f64) -> f64 {
        static FONT: std::sync::OnceLock<Vec<Font>> = std::sync::OnceLock::new();
        let fonts = FONT.get_or_init(|| Font::new(NOTO_SANS).into_iter().collect());
        mui_text::shape_run(fonts, text, size, &[("wght", self.weight)]).map_or(0., |run| run.advance)
    }
    fn fit(self, text: &str, room: f64, size: f64) -> f64 {
        let width = self.advance(text, size);
        if width <= room || width <= 0. { size } else { (size * room / width).max(size * 0.75) }
    }
}

/// Approximate the factory chart using the existing variable Noto face. The
/// colors are sampled from NI's published factory font chart, not guessed from
/// the wallpaper. Exact bitmap glyph metrics still require original resources.
/// https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/control-parameters
fn text_font(c: &Control, on: bool, pressed: bool, hovered: bool) -> TextFont {
    const COLORS: [[u8; 3]; 26] = [
        [254,254,254], [254,254,254], [55,55,51], [204,204,204], [227,194,105],
        [228,209,130], [125,48,18], [133,53,25], [72,68,60], [0,0,0],
        [217,217,217], [137,140,141], [27,27,26], [210,220,225], [185,185,185],
        [95,95,95], [0,0,0], [254,254,254], [254,254,254], [0,0,0],
        [127,127,127], [127,127,127], [0,0,0], [127,127,127], [255,255,255], [12,36,49],
    ];
    let id = font_id(c, on, pressed, hovered).filter(|id| (0..26).contains(id));
    let rgb = |[r, g, b]: [u8; 3]| Color::srgb(f32::from(r) / 255., f32::from(g) / 255., f32::from(b) / 255.);
    let explicit = int(c, "$CONTROL_PAR_TEXT_COLOR").map(|color| {
        let color = color as u32;
        rgb([(color >> 16) as u8, (color >> 8) as u8, color as u8])
    });
    TextFont {
        size: if matches!(id, Some(1 | 5 | 7 | 16 | 17 | 20)) { 13. } else { FONT },
        weight: if matches!(id, Some(16..=25)) { 700. } else { 400. },
        color: explicit.or_else(|| id.map(|id| rgb(COLORS[id as usize]))),
    }
}

/// Loader/audit diagnostics, once per interface; never emitted while drawing.
/// Custom IDs whose bitmap names were not resolved retain a readable fallback.
pub(crate) fn font_fallbacks(interface: &Interface) -> Vec<String> {
    let ids: std::collections::BTreeSet<i32> = interface.controls.iter().flat_map(|c|
        ["$CONTROL_PAR_FONT_TYPE", "$CONTROL_PAR_FONT_TYPE_ON", "$CONTROL_PAR_FONT_TYPE_OFF_PRESSED",
         "$CONTROL_PAR_FONT_TYPE_ON_PRESSED", "$CONTROL_PAR_FONT_TYPE_OFF_HOVER", "$CONTROL_PAR_FONT_TYPE_ON_HOVER"]
        .into_iter().filter_map(|name| int(c, name)).filter(|&id| id >= 0)
    ).collect();
    let mut warnings = Vec::new();
    let factory: Vec<_> = ids.iter().filter(|&&id| id < 26).copied().collect();
    if !factory.is_empty() {
        warnings.push(format!("Factory font IDs {factory:?} use the bundled Noto Sans approximation; original factory bitmap glyphs and exact metrics are not loaded"));
    }
    for id in ids.into_iter().filter(|&id| id >= 26 && interface.fonts.get((id - 26) as usize).is_none()) {
        warnings.push(format!("Custom font ID {id}: bitmap font resource is unavailable; using bundled Noto Sans"));
    }
    warnings
}

/// `text` as the lines a label `room` points wide shows at `size`: one per
/// newline and, when `wrap`, a new one before a word that would run past
/// the edge. A word wider than the room keeps a line of its own.
pub fn break_lines(text: &str, room: f64, size: f64, wrap: bool) -> Vec<String> {
    break_font_lines(text, room, size, wrap, TextFont { size, weight: 400., color: None })
}

fn break_font_lines(text: &str, room: f64, size: f64, wrap: bool, font: TextFont) -> Vec<String> {
    break_measured_lines(text, room, wrap, |text| font.advance(text, size))
}

fn break_measured_lines(text: &str, room: f64, wrap: bool, advance: impl Fn(&str) -> f64) -> Vec<String> {
    let mut out = Vec::new();
    for paragraph in text.split('\n') {
        let mut line: Option<String> = None;
        // Split on single spaces so a script's padding stays as it spaced it.
        for word in paragraph.split(' ') {
            line = Some(match line {
                None => word.to_owned(),
                Some(l) if wrap && !l.trim().is_empty() && advance(&format!("{l} {word}")) > room => {
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
fn words(words: String, align: i32, top: Option<f64>, w: f64, h: f64, s: f64, ink: impl Into<Fill>, label: bool, font: TextFont) -> El {
    let ink: Fill = ink.into();
    let justify = match align {
        1 => Justify::Center,
        2 => Justify::End,
        _ => Justify::Start,
    };
    let lh = font.size * 1.25 * s;
    let lines = if label { break_font_lines(&words, w - 4. * s, font.size * s, h >= 2. * lh, font) } else { vec![words] };
    let one = |t: String| {
        let size = font.fit(&t, w - 4. * s, font.size * s);
        row![text(t).text_size(size).text_axis("wght", font.weight).fill(ink.clone()).lines(1).min_w(0)].justify(justify).align(Align::Center).w(w).pad((2. * s, 0.))
    };
    if lines.len() > 1 {
        let tall = lh * lines.len() as f64;
        let block = col(lines.into_iter().map(|t| one(t).h(lh))).gap(0).w(w).h(tall);
        return block.at(0., top.map_or((h - tall) / 2., |y| y * s));
    }
    let line = one(lines.into_iter().next().unwrap_or_default());
    match top {
        Some(y) => line.h(font.size * s * 1.4).at(0., y * s),
        None => line.h(h).at(0., 0.),
    }
}

/// Authored bitmap glyphs keep their own advance, pixels and alpha. They
/// are scaled with the whole UI, never refitted through an outline font.
#[allow(clippy::too_many_arguments)]
fn bitmap_words(words: &str, align: i32, top: Option<f64>, w: f64, h: f64, s: f64, label: bool, font: &Picture) -> El {
    let glyph = |c| &font.frames[crate::artwork::font_glyph(c)];
    let advance = |text: &str| text.chars().map(|c| f64::from(glyph(c).width) * s).sum::<f64>();
    let lh = f64::from(font.frames[0].height) * s;
    let lines = break_measured_lines(words, w - 4. * s, label && h >= 2. * lh, advance);
    let tall = lh * lines.len() as f64;
    let y = top.map_or((h - tall) / 2., |y| y * s);
    let mut layers = Vec::new();
    for (n, line) in lines.iter().enumerate() {
        let width = advance(line);
        let mut x = match align { 1 => (w - width) / 2., 2 => w - 2. * s - width, _ => 2. * s };
        for c in line.chars() {
            let image = glyph(c);
            let width = f64::from(image.width) * s;
            layers.push(block(width, lh).radius(0).fill(Fill::Image(image.clone(), Fit::Fill)).at(x, y + n as f64 * lh));
            x += width;
        }
    }
    stack(layers).w(w).h(h).clip()
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
pub(super) fn frame_of(shown: &Shown, c: &Control) -> Option<Arc<Image>> {
    picture_frame(shown, c, value(c))
}

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
fn sliced(image: &Arc<Image>, stretch: [bool; 2], w: f64, h: f64, s: f64, dev: f64, slot: usize) -> El {
    let fit = |image: &Arc<Image>, w: f64, h: f64| {
        Fill::Image(fitted::fitted(image, (w * dev).round() as u32, (h * dev).round() as u32, slot), Fit::Fill)
    };
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
        return block(w, h).fill(fit(image, w, h));
    }
    let mut parts = Vec::new();
    let mut y = 0.;
    for &(sy, sh, th) in &down {
        let mut x = 0.;
        for &(sx, sw, tw) in &across {
            if let Some(piece) = fitted::cut(image, sx, sy, sw, sh) {
                parts.push(block(tw, th).fill(fit(&piece, tw, th)).at(x, y));
            }
            x += tw;
        }
        y += th;
    }
    stack(parts).w(w).h(h)
}

/// Contrast fallback for text without a recognized explicit font/color.
fn ink(under: Option<f32>) -> Color {
    match under {
        Some(l) if l > 0.6 => Color::srgb(0.1, 0.1, 0.1),
        _ => Color::srgb(0.88, 0.88, 0.88),
    }
}

/// How light (0 to 1) what lies under authored point `(x, y)` is: the
/// topmost opaque picture of `below` there, else the wallpaper.
pub(super) fn luma_under(below: &[(Shown, Option<Arc<Image>>)], wallpaper: Option<(&Image, [u32; 2])>, x: f64, y: f64) -> Option<f32> {
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
    let (wallpaper, origin) = wallpaper?;
    at(wallpaper, x + f64::from(origin[0]), y + HEADER + f64::from(origin[1]))
}

/// How a control is drawn.
#[derive(Clone, Copy)]
enum Look<'a> {
    /// Its picture, or KONTRA's face where it has none; how light what
    /// its text sits on is.
    Original(Option<f32>),
    /// KONTRA's face and words as planned.
    Vector(&'a Plan),
}

/// One control: its picture's frame or KONTRA's face, its text, and its
/// pointer handling.
fn control(ui: &mut Ui, cx: &mut Cx, slot: usize, shown: &Shown, c: &Control, s: f64, look: Look) -> El {
    let vector = matches!(look, Look::Vector(_));
    let marker_on_wave = matches!(look, Look::Vector(p) if p.face == VFace::Marker);
    let id = format!("kpv-{slot}-{}", shown.control);
    let dev = ui.scale().unwrap_or(1.);
    let (w, h) = ((shown.w * s * dev).round() / dev, (shown.h * s * dev).round() / dev);
    let hide = int(c, "$CONTROL_PAR_HIDE").unwrap_or(0);
    let raw = cx.view.parts[slot].control_value(shown.control).unwrap_or_else(|| value(c));
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
            // A marker on a wave moves along it.
            let vertical = drags_vertically(shown.kind, shown.w, shown.h, prop(c, "$CONTROL_PAR_PICTURE"), behaviour) && !marker_on_wave;
            let before = now;
            let distance = if marker_on_wave { w } else if matches!(look, Look::Vector(p) if p.face == VFace::Normal)
                && shown.kind == Kind::Slider && !knob_like(prop(c, "$CONTROL_PAR_PICTURE"), shown.w, shown.h)
            {
                // The native thumb's center moves within the track's end
                // caps. Use that exact span, also at fractional view scales.
                (if vertical { h } else { w }) - FADER_THUMB
            } else {
                travel(shown.kind, behaviour, hi - lo, (shown.w, shown.h), prop(c, "$CONTROL_PAR_PICTURE")) * s
            };
            // drive applies each pointer delta to the grabbed value, so a
            // press keeps the grab offset instead of snapping the thumb.
            let held = drive(ui, &id, &mut now, &(lo..=hi), distance, vertical, reset);
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
        Kind::FileSelector => {
            if ui.get(id.as_str()).activated() {
                let base = prop(c, "$CONTROL_PAR_BASEPATH");
                let v = &cx.view.parts[slot];
                let ask = super::picker::Ask::ScriptFile {
                    part: slot, epoch: v.script_epoch, slot: v.script_slot, control: shown.control,
                    from: std::path::PathBuf::from(base.replace('\\', "/")),
                    file_type: int(c, "$CONTROL_PAR_FILE_TYPE").unwrap_or(0),
                };
                if base.is_empty() { cx.state.notice = "File selector has no base directory.".into(); }
                else if !cx.state.picker.ask(ask) { cx.state.notice = "The file picker is unavailable in this session.".into(); }
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
    let vertical = drags_vertically(shown.kind, shown.w, shown.h, prop(c, "$CONTROL_PAR_PICTURE"), behaviour) && !marker_on_wave;
    let round = shown.kind == Kind::Knob || shown.kind == Kind::Slider && knob_like(prop(c, "$CONTROL_PAR_PICTURE"), shown.w, shown.h);
    let lift = if cx.state.held.is_some_and(|(p, n, _)| (p, n) == (slot, shown.control)) { 1. } else { ui.state(id.as_str()).hover as f32 };
    let wave = (shown.kind == Kind::Waveform)
        .then(|| attached(cx.view.parts[slot].instrument.as_deref(), c))
        .flatten()
        .and_then(super::wave::ask)
        .map(|p| {
            let cursor = int(c, "$UI_WF_PROP_PLAY_CURSOR").filter(|&v| v > 0).map(|v| p.at(v.into()));
            (p, cursor)
        });
    let own = || face(shown.kind, c, now, lo, hi, vertical, round, s, lift, ui.focus_visible(id.as_str()), wave.clone(), vector).w(w).h(h);
    let drawn = match (picture, look) {
        (Some(image), _) => sliced(&image, shown.picture.as_ref().map_or([false; 2], |p| p.stretch), w, h, s, dev, slot),
        (None, Look::Vector(plan)) => match plan.face {
            VFace::Normal if plan.skin => own().fill(Role::Surface),
            VFace::Normal => own(),
            VFace::Clear => block(w, h),
            VFace::Cover => block(w, h).fill(Role::Background),
            VFace::Panel(on) => block(w, h).fill(if on { Role::Raised } else { Role::Surface }),
            VFace::Marker => marker(((now - lo) / (hi - lo)).clamp(0., 1.), s, lift).w(w).h(h),
        },
        (None, Look::Original(_)) => own(),
    };
    let mut layers = vec![drawn];
    let el = if let Look::Vector(plan) = look {
        // Planned to fit, so nothing is clipped: a knob's name sits under it.
        for words in &plan.words {
            let justify = match words.align {
                1 => Justify::Center,
                2 => Justify::End,
                _ => Justify::Start,
            };
            let line = row![text(words.text.clone()).text_size(words.size * s).fill(Role::Ink).lines(1)].justify(justify).align(Align::Center);
            layers.push(line.w(words.w * s).h(words.h * s).at(words.x * s, words.y * s));
        }
        stack(layers).w(w).h(h)
    } else {
        // Text on our face is our ink; on a picture it reads what lies under it.
        let interaction = ui.get(id.as_str());
        let on = if shown.kind == Kind::Menu {
            cx.state.menu.as_ref().is_some_and(|menu| matches!(&menu.target, Target::Script { part, control } if *part == slot && *control == shown.control))
        } else { now >= 1. };
        let font = text_font(c, on, interaction.held, interaction.hovered);
        let bitmap = font_id(c, on, interaction.held, interaction.hovered).filter(|&id| id >= 26)
            .and_then(|id| cx.view.parts[slot].interface.as_ref()?.fonts.get((id - 26) as usize))
            .and_then(|name| cx.view.parts[slot].pictures.get(&crate::artwork::font_key(name)))
            .filter(|font| font.frames.len() == 256);
        let own_ink = font.color.map(Fill::from).unwrap_or_else(|| match look {
            Look::Original(under) if pictured || matches!(shown.kind, Kind::Label | Kind::Area | Kind::Knob | Kind::Slider | Kind::Other) => Fill::from(ink(under)),
            _ => Fill::from(Role::Ink),
        });
        let caption = |said: String, align, top, label| match bitmap {
            Some(bitmap) => bitmap_words(&said, align, top, w, h, s, label, bitmap),
            None => words(said, align, top, w, h, s, own_ink.clone(), label, font),
        };
        let (said, align, top) = caption_of(c, shown.kind, now);
        if !said.is_empty() {
            layers.push(caption(said, align, top, shown.kind == Kind::Label));
        }
        if shown.kind == Kind::Knob && !pictured {
            // Kontakt's own knob: its name over it, its value under it.
            let name = prop(c, "$CONTROL_PAR_TEXT");
            let name = if name.is_empty() { c.variable.trim_start_matches(['$', '~']) } else { name };
            if hide & HIDE_TITLE == 0 {
                layers.push(caption(keep_spaces(name), 1, Some(0.), false));
            }
            if hide & HIDE_VALUE == 0 {
                let label = prop(c, "$CONTROL_PAR_LABEL");
                let shown_value = if label.is_empty() { format!("{}", now.round()) } else { keep_spaces(label) };
                layers.push(caption(shown_value, 1, Some(shown.h - bitmap.map_or(font.size * 1.4, |p| f64::from(p.frames[0].height))), false));
            }
        }
        stack(layers).w(w).h(h).clip()
    };
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
    let el = if shown.kind == Kind::FileSelector {
        el.focusable().a11y(A11y::Button)
    } else if matches!(shown.kind, Kind::Knob | Kind::Slider | Kind::Value) {
        el.focusable().a11y(A11y::Slider { value: now, min: lo, max: hi })
    } else {
        el
    };
    el.captures_wheel().id(id)
}

/// A control in KONTRA's own look, filling its authored rect: what the
/// vectorized view draws, and the original where a picture is missing.
/// Nothing in it is translucent over what lies under it but its hairlines,
/// so it never greys out a picture beneath.
#[allow(clippy::too_many_arguments)]
fn face(kind: Kind, c: &Control, value: f64, lo: f64, hi: f64, vertical: bool, round: bool, s: f64, lift: f32, focused: bool, wave: Option<(Arc<Peaks>, Option<f64>)>, vector: bool) -> El {
    let t = ((value - lo) / (hi - lo)).clamp(0., 1.);
    if vector && matches!(kind, Kind::Knob | Kind::Slider) {
        return if round {
            // The same KONTRA dial, inside the bitmap's authored margin.
            let text = if kind == Kind::Knob { FONT * 1.4 * s } else { 4. * s };
            dial_face(t, 0., lift, focused).pad(edges(text, 4. * s, text, 4. * s))
        } else {
            fader_face(t, 0., None, vertical, lift, focused)
        };
    }
    let on = value >= 1.;
    let bars: Vec<f64> = match (kind, c.properties.get("$CONTROL_PAR_VALUE")) {
        (Kind::Table, Some(Value::IntArray(a))) => a.iter().map(|n| f64::from(*n)).collect(),
        (Kind::Table, Some(Value::RealArray(a))) => a.clone(),
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
    // out means it opaque. The vectorized view keeps to KONTRA's.
    let colour = |k: &str| {
        int(c, k).filter(|_| !vector).map(|v| {
            let v = v as u32;
            let byte = |shift: u32| ((v >> shift) & 0xff) as f32 / 255.;
            let a = if v >> 24 == 0 { 1. } else { byte(24) };
            Color::srgba(byte(16), byte(8), byte(0), a)
        })
    };
    let meter = (colour("$CONTROL_PAR_BG_COLOR"), colour("$CONTROL_PAR_OFF_COLOR"));
    let (ink, cursor_ink) = (colour("$CONTROL_PAR_WAVE_COLOR"), colour("$CONTROL_PAR_WAVE_CURSOR_COLOR"));
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
                // A pictured knob kept a margin round its ring, where the library
                // set its words: ours keeps inside it (see vector::hides).
                let r = if kind == Kind::Knob { w.min(h - 2. * text) / 2. - weight } else { w.min(h) * 0.35 };
                let r = r.max(2.);
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
            Kind::Value | Kind::TextEdit | Kind::FileSelector => boxed(&mut d, Role::Field.alpha(1.)),
            Kind::Table => {
                boxed(&mut d, Role::Field.alpha(1.));
                let baseline = (hi / (hi - lo)).clamp(0., 1.) * (h - 2.) + 1.;
                let bw = w / bars.len().max(1) as f64;
                for (n, v) in bars.iter().enumerate() {
                    if !v.is_finite() { continue; }
                    let y = (hi - v.clamp(lo, hi)) / (hi - lo) * (h - 2.) + 1.;
                    d.push(Draw::fill(rect(n as f64 * bw + 1., y.min(baseline), (bw - 1.).max(1.), (y - baseline).abs()), value_ink(0.)));
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
                let ink = ink.map_or(Fill::from(value_ink(0.)), Fill::from);
                match &wave {
                    Some((peaks, cursor)) => {
                        d.push(Draw::fill(wave_path(&peaks.columns, w, h), ink));
                        if let Some(t) = cursor {
                            d.push(Draw::fill(rect((t * (w - 1.)).round(), 0., 1., h), cursor_ink.map_or(Fill::from(Role::Ink), Fill::from)));
                        }
                    }
                    None => d.push(Draw::fill(rect(0., (h / 2.).round(), w, 1.), ink)),
                }
            }
            Kind::Label | Kind::Area | Kind::Other => {}
        }
        d
    })
}

/// The outline of `columns`' lows and highs across `w` by `h`, about its
/// middle: one column of the drawing per point across.
fn wave_path(columns: &[[f32; 2]], w: f64, h: f64) -> DrawPath {
    let (n, across) = (columns.len().max(1), (w.ceil() as usize).max(1));
    let mid = h / 2.;
    let (mut top, mut foot) = (Vec::with_capacity(across + 1), Vec::with_capacity(across + 1));
    for x in 0..across {
        let (a, b) = (x * n / across, ((x + 1) * n / across).max(x * n / across + 1).min(n));
        let (lo, hi) = columns[a..b].iter().fold((0f32, 0f32), |(l, m), c| (l.min(c[0]), m.max(c[1])));
        // A silent stretch keeps a hairline.
        let (lo, hi) = (f64::from(lo).min(-0.5 / mid), f64::from(hi).max(0.5 / mid));
        for px in [x as f64, (x as f64 + 1.).min(w)] {
            top.push(Point::new(px, mid - hi * mid));
            foot.push(Point::new(px, mid - lo * mid));
        }
    }
    top.extend(foot.into_iter().rev());
    DrawPath::polyline(top, true)
}

/// Which zone of `instrument` waveform `c` shows (`attach_zone`).
fn attached<'a>(instrument: Option<&'a crate::import::Instrument>, c: &Control) -> Option<&'a crate::import::Zone> {
    instrument?.zones.get(usize::try_from(int(c, "attached zone")?).ok()?)
}

/// The wave waveform `c` of `instrument` shows, read now: the audit's.
pub(super) fn wave_now(instrument: &crate::import::Instrument, c: &Control) -> Option<Arc<Peaks>> {
    attached(Some(instrument), c).and_then(super::wave::now)
}

/// A slider over a waveform, vectorized: a line at `t` across, with a
/// handle at its head.
fn marker(t: f64, s: f64, lift: f32) -> El {
    canvas(move |z| {
        let (w, h) = (z.width, z.height);
        let x = (t * (w - 1.)).round();
        let k = 5. * s;
        vec![
            Draw::fill(rect(x, 0., s.max(1.), h), value_ink(lift)),
            Draw::fill(rect((x - k / 2.).clamp(0., w - k), 0., k, k), value_ink(lift)),
        ]
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
        Arc::new(Picture { frames: vec![frame; frames], stretch: [resizable; 2], atlas: None })
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
        assert_eq!(travel(Kind::Slider, 0, 127., (120., 20.), "fader"), 120.);
        assert_eq!(travel(Kind::Slider, -1000, 127., (20., 120.), "fader"), 120.);
        assert_eq!(travel(Kind::Slider, 2500, 1_000_000., (734., 73.), "macro_slider_transparent"), 734.);
        assert_eq!(travel(Kind::Slider, 2500, 1_000_000., (111., 11.), "mini_macro1_slider"), 111.);
        assert_eq!(travel(Kind::Knob, 0, 127., (48., 50.), ""), 200.);
        assert_eq!(travel(Kind::Slider, -1000, 127., (48., 50.), "knob"), 100.);
        assert_eq!(travel(Kind::Slider, 5000, 127., (48., 50.), "knob"), 60., "the fastest knob still has room");
        assert_eq!(travel(Kind::Value, 0, 10., (80., 18.), ""), 60.);
    }

    #[test]
    fn sliders_use_drawn_length_for_pointer_drag_at_any_scale() {
        // Real Analog Strings macro dimensions/behaviour, plus a vertical
        // fader. Exercise the same Ui drag path as control(), not just the
        // distance formula. Pointer coordinates are logical host points.
        for (w, h, behaviour, picture) in [
            (734., 73., 2500, "macro_slider_transparent"),
            (111., 11., 2500, "mini_macro1_slider"),
            (13., 102., -1755, "fader"),
        ] {
            for view_scale in [0.5, 1., 2.] {
                for device_scale in [1., 2.] {
                    let mut ui = crate::ui::theme::ui();
                    ui.set_scale(Some(device_scale));
                    let size = Size::new(w * view_scale, h * view_scale);
                    let root = || canvas(|_| Vec::new()).w(size.width).h(size.height).id("slider");
                    ui.frame(root(), Some(size), Input::default(), 0.).unwrap();
                    let vertical = drags_vertically(Kind::Slider, w, h, picture, behaviour);
                    let distance = travel(Kind::Slider, behaviour, 1_000_000., (w, h), picture) * view_scale;
                    let physical_delta = 10.;
                    let delta = physical_delta / device_scale;
                    let grab = Point::new(size.width * 0.55, size.height * 0.55);
                    let mut value = 400_000.;
                    for (tick, (step, fine)) in [(0., false), (1., false), (1., false), (2., true)].into_iter().enumerate() {
                        let at = if vertical { Point::new(grab.x, grab.y - step * delta) } else { Point::new(grab.x + step * delta, grab.y) };
                        let input = Input { pointer: PointerInput {
                            pos: Some(at), buttons: Buttons::PRIMARY,
                            mods: Mods { shift: fine, ..Default::default() },
                            ..Default::default()
                        }, ..Default::default() };
                        ui.frame(root(), Some(size), input, (tick + 1) as f64 / 60.).unwrap();
                        drive(&mut ui, "slider", &mut value, &(0. ..=1_000_000.), distance, vertical, 0.);
                        let travelled = match step { 0. => 0., 1. => delta, _ => delta * 1.1 };
                        let expected = 400_000. + travelled / distance * 1_000_000.;
                        assert!((value - expected).abs() < 1e-6, "{w}x{h}, view {view_scale}, device {device_scale}, step {step}: {value} != {expected}");
                    }
                }
            }
        }
    }

    #[test]
    fn closed_physical_drag_keeps_the_starting_value_across_device_scales() {
        // Native X11 samples are integer physical pixels. At scale 1.5 the
        // first six-pixel move equals MUI's click threshold and is not yet
        // a drag. Losing it moved Analog's 180-frame macro one frame left.
        for scale in [1., 1.5, 2.] {
            for vertical in [false, true] {
                for fine in [false, true] {
                    let mut ui = crate::ui::theme::ui();
                    let size = Size::new(734., 734.);
                    let root = || block(size.width, size.height).id("macro");
                    ui.frame(root(), Some(size), Input::default(), 0.).unwrap();
                    let mut value = 500_000.;
                    let mut peak: f64 = 0.;
                    // Repeat on the same ID so the second press must reset
                    // the remembered first-drag frame too.
                    for _ in 0..2 {
                        for step in 0..=500 {
                            let physical = (250. * (step as f64 / 500. * std::f64::consts::TAU * 2.).sin()).round();
                            let delta = physical / scale;
                            let at = if vertical { Point::new(400., 400. - delta) } else { Point::new(400. + delta, 400.) };
                            let input = Input { pointer: PointerInput {
                                pos: Some(at), buttons: Buttons::PRIMARY,
                                mods: Mods { shift: fine, ..Default::default() }, ..Default::default()
                            }, ..Default::default() };
                            ui.frame(root(), Some(size), input, 0.012).unwrap();
                            drive(&mut ui, "macro", &mut value, &(0. ..=1_000_000.), 734., vertical, 0.);
                            peak = peak.max((value - 500_000.).abs());
                        }
                        assert!((value - 500_000.).abs() < 1e-6, "scale {scale}, vertical {vertical}, fine {fine}: {value}");
                        assert_eq!(frame(value.round(), 0., 1_000_000., 180), frame(500_000., 0., 1_000_000., 180), "authored sprite returns to the same frame");
                        ui.frame(root(), Some(size), Input::default(), 0.6).unwrap();
                        drive(&mut ui, "macro", &mut value, &(0. ..=1_000_000.), 734., vertical, 0.);
                    }
                    let expected = 250. / scale / 734. * 1_000_000. * if fine { 0.1 } else { 1. };
                    assert!((peak - expected).abs() < 1e-6, "the control moves through the full pointer excursion");
                }
            }
        }
    }

    #[test]
    fn native_fader_visible_thumb_follows_pointer_delta() {
        // Read the painted thumb, independently of the drag equation.
        // This face is shared by Vectorized and the native rack controls.
        for width in [111., 734.] {
            for scale in [0.5, 1., 2.] {
                let mut ui = crate::ui::theme::ui();
                let size = Size::new(width * scale, 30.);
                let root = |value| fader_face(value, 0., None, false, 0., false).w(size.width).h(size.height).id("slider");
                let mut value = 0.4;
                ui.frame(root(value), Some(size), Input::default(), 0.).unwrap();
                let thumb = |ui: &Ui| {
                    let pixels = crate::ui::tests::pixels(ui, size.width.ceil() as u16, 30);
                    let stride = size.width.ceil() as usize;
                    // Four rows above the center: the track has no ink here.
                    let lit: Vec<_> = (0..stride).filter(|&x| pixels[(11 * stride + x) * 4 + 3] > 127).collect();
                    assert!(!lit.is_empty(), "the native thumb paints");
                    (lit[0] + lit[lit.len() - 1]) as f64 / 2.
                };
                let before = thumb(&ui);
                let grab = Point::new(size.width * 0.55, 15.);
                for (tick, delta) in [0., 10.].into_iter().enumerate() {
                    let input = Input { pointer: PointerInput {
                        pos: Some(Point::new(grab.x + delta, grab.y)), buttons: Buttons::PRIMARY,
                        ..Default::default()
                    }, ..Default::default() };
                    ui.frame(root(value), Some(size), input, (tick + 1) as f64 / 60.).unwrap();
                    drive(&mut ui, "slider", &mut value, &(0. ..=1.), size.width - FADER_THUMB, false, 0.);
                }
                ui.frame(root(value), Some(size), Input::default(), 3. / 60.).unwrap();
                assert!((thumb(&ui) - before - 10.).abs() <= 1., "width {width}, scale {scale}: the visible thumb follows ten pointer points");
            }
        }
    }

    #[test]
    fn stretched_pictures_keep_their_edges_and_center() {
        let row = [[255,0,0,255], [255,0,0,255], [0,255,0,255], [0,0,255,255], [0,0,255,255]].concat();
        let image = Arc::new(Image::rgba(5, 5, row.repeat(5)).unwrap());
        let mut ui = crate::ui::theme::ui();
        let root = sliced(&image, [true; 2], 21., 15., 1., 1., 0);
        ui.frame(root, Some(Size::new(21., 15.)), Input::default(), 0.).unwrap();
        let rgba = crate::ui::tests::pixels(&ui, 21, 15);
        let pixel = |x: usize| &rgba[(7 * 21 + x) * 4..(7 * 21 + x) * 4 + 4];
        assert_eq!(pixel(0), &[255, 0, 0, 255]);
        assert_eq!(pixel(10), &[0, 255, 0, 255]);
        assert_eq!(pixel(20), &[0, 0, 255, 255]);
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
        interface.controls[0].properties.insert("$CONTROL_PAR_HIDE".into(), Value::Int(0));
        interface.controls[0].properties.insert("$CONTROL_PAR_PARENT_PANEL".into(), Value::Int(32769));
        assert_eq!(spots(&interface), [(4, 7., 8.)], "cyclic panels do not hang or draw misplaced children");
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
    fn original_text_respects_factory_state_fonts_and_authored_label_geometry() {
        let label = control("ui_label", &[("TEXT", Value::Text("VOLUME 1".into())), ("FONT_TYPE", Value::Int(23)),
            ("POS_X", Value::Int(38)), ("POS_Y", Value::Int(493)), ("WIDTH", Value::Int(70)), ("HEIGHT", Value::Int(18)),
            ("TEXT_ALIGNMENT", Value::Int(1))]);
        let interface = Interface { performance: true, width: 732, height: 540, controls: vec![label.clone()], ..Default::default() };
        let shown = layout(&interface, &HashMap::new()).remove(0);
        assert_eq!((shown.x, shown.y, shown.w, shown.h), (38., 493., 70., 18.));
        let font = text_font(&label, false, false, false);
        assert_eq!(font.color, Some(Color::srgb(127. / 255., 127. / 255., 127. / 255.)));
        assert_eq!(caption_of(&label, Kind::Label, 0.), ("VOLUME 1".into(), 1, None));
        let root = words("VOLUME 1".into(), 1, None, 70., 18., 1., font.color.unwrap(), true, font);
        let spec = moose::mui::mui::scene::SceneSpec::new(root).offered(Size::new(70., 18.)).font(Font::new(NOTO_SANS).unwrap());
        let scene = moose::mui::mui::scene::resolve(&spec).unwrap();
        let run = scene.paint.iter().find_map(|paint| paint.text.as_ref()).expect("authored caption is drawn through MUI");
        assert_eq!(run.axes.get("wght"), Some(font.weight));
        assert!(f64::from(run.size) <= font.size);

        let mut switch = control("ui_switch", &[("FONT_TYPE", Value::Int(15)), ("FONT_TYPE_ON", Value::Int(24)),
            ("FONT_TYPE_OFF_HOVER", Value::Int(13)), ("FONT_TYPE_ON_PRESSED", Value::Int(23))]);
        assert_eq!(font_id(&switch, false, false, false), Some(15));
        assert_eq!(font_id(&switch, true, false, false), Some(24));
        assert_eq!(font_id(&switch, false, false, true), Some(13));
        assert_eq!(font_id(&switch, true, true, true), Some(23), "pressed overrides hover");
        assert_eq!(font_id(&switch, true, false, true), Some(15), "unset state inherits base, not the on-state font");
        switch.properties.insert("$CONTROL_PAR_TEXT_COLOR".into(), Value::Int(0x102030));
        assert_eq!(text_font(&switch, true, false, false).color, Some(Color::srgb(16. / 255., 32. / 255., 48. / 255.)));
        let custom = control("ui_label", &[("FONT_TYPE", Value::Int(1024))]);
        let custom_interface = Interface { controls: vec![custom.clone(), custom], ..Default::default() };
        assert_eq!(font_fallbacks(&custom_interface).len(), 1, "missing custom fonts are deduplicated and explicit");
        assert!(font_fallbacks(&interface)[0].contains("approximation"));
    }

    #[test]
    fn authored_bitmap_fonts_keep_variable_advances_alpha_and_newlines() {
        let interface = crate::ksp::initialize("on init\nmake_perfview\ndeclare ui_label $caption(1,1)\nset_text($caption,\"Ai\")\nset_control_par(get_ui_id($caption),$CONTROL_PAR_FONT_TYPE,get_font_id(\"custom\"))\ndeclare $same := get_font_id(\"custom\")\nend on", 0, 8).unwrap();
        assert_eq!(interface.fonts, ["custom"]);
        assert_eq!(font_id(&interface.controls[0], false, false, false), Some(26));
        assert!(!font_fallbacks(&interface).iter().any(|warning| warning.contains("unavailable")));
        assert_eq!(crate::artwork::picture_names(&interface).next().as_deref(), Some("@font/custom"));
        let widths: Vec<u32> = (0..256).map(|n| match n { 65 => 7, 105 => 3, _ => 1 }).collect();
        let width = widths.iter().sum::<u32>();
        let mut rgba = vec![0; width as usize * 3 * 4];
        let mut x = 0;
        for &w in &widths {
            rgba[x as usize * 4..x as usize * 4 + 4].copy_from_slice(&[255,0,0,255]);
            for y in 1..3 { for col in x..x+w {
                let at = (y * width + col) as usize * 4;
                rgba[at..at+4].copy_from_slice(&[246,176,92,128]);
            }}
            x += w;
        }
        let image = Image::rgba(width, 3, rgba).unwrap();
        let font = Picture { frames: crate::artwork::font_frames(&image).unwrap(), stretch: [false;2], atlas: None };
        assert_eq!(font.frames[65].rgba.as_ref(), &[246,176,92,128].repeat(14));
        assert_eq!(crate::artwork::font_glyph('€'), 128);
        assert_eq!(crate::artwork::font_glyph('é'), 233);
        assert_eq!(crate::artwork::font_glyph('漢'), b'?' as usize);
        for scale in [1.,1.5,2.] {
            let root = bitmap_words("Ai\nA", 1, Some(1.), 20.*scale, 8.*scale, scale, true, &font);
            let scene = moose::mui::mui::scene::resolve(&moose::mui::mui::scene::SceneSpec::new(root).offered(Size::new(20.*scale,8.*scale))).unwrap();
            assert_eq!(scene.paint.iter().filter(|paint| paint.text.is_some()).count(), 0, "authored glyphs bypass the outline font");
            let glyphs: Vec<_> = scene.paint.iter().filter(|paint| matches!(paint.paint, moose::mui::mui::scene::Paint::Image { .. })).collect();
            assert_eq!(glyphs.len(), 3);
            assert_eq!(glyphs[0].offset, Point::new(5.*scale, scale));
            assert_eq!(glyphs[1].offset, Point::new(12.*scale, scale), "second glyph follows the authored seven-pixel advance");
            assert_eq!(glyphs[2].offset, Point::new(6.5*scale, 3.*scale), "newlines follow authored glyph height and alignment");
            assert_eq!(glyphs[0].rect.as_ref().unwrap().radius(), 0., "the theme must not round away glyph pixels");
        }
        let invalid = Image::rgba(1, 2, vec![0;8]).unwrap();
        assert!(crate::artwork::font_frames(&invalid).unwrap_err().contains("256"));
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
        assert_eq!(luma_under(&below, Some((&wallpaper, [0; 2])), 2., 2.), Some(1.), "a clear picture shows the one under it");
        assert_eq!(luma_under(&below, Some((&wallpaper, [0; 2])), 0., 20.), Some(0.), "else the wallpaper, below its header rows");
        assert_eq!(luma_under(&below, None, 50., 50.), None);
        assert_eq!(ink(Some(1.)), Color::srgb(0.1, 0.1, 0.1));
        assert_eq!(ink(None), ink(Some(0.)));
    }
}
