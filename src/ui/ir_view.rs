//! A library's instrument interface drawn from its UI IR
//! ([`sampler_ui_ir::Interface`]), in either presentation:
//!
//! - **Bitmap**: the library's wallpaper, background art and control strips,
//!   each control showing the frame for its value.
//! - **Vector**: the wallpaper and background art only; every control is
//!   drawn by our own faces at its authored place and size. Strips, handles
//!   and bitmap fonts are never decoded, and are released on a switch.
//!
//! Values live with the caller, keyed by control identity; the view edits
//! them in place and the caller forwards changes to the control service.

use super::theme::*;
use moose::mui::mui::prelude::*;
use moose::mui::mui::scene::{Fit, Image};
use sampler_ui_ir::{self as ir, Binding, ControlId, Interface, Kind, PageRef, Presentation, Role as Use, WidgetRef};
use std::collections::HashMap;
use std::sync::Arc;

/// An image cut into its animation frames.
#[derive(Debug)]
pub struct Picture {
    pub frames: Vec<Arc<Image>>,
}

/// The frame of `frames` a control at `value` in `min..=max` shows.
fn frame(value: f64, min: f64, max: f64, frames: usize) -> usize {
    let span = max - min;
    let t = if span == 0. { 0. } else { ((value - min) / span).clamp(0., 1.) };
    (t * frames.saturating_sub(1) as f64).round() as usize
}

/// The frame a switch or button shows: Kontakt's strips run off, on, then
/// pressed and hovered states.
fn switch_frame(on: bool, frames: usize) -> usize {
    usize::from(on).min(frames.saturating_sub(1))
}

/// Decoded images the interface draws, by asset index; `None` failed to load.
#[derive(Default)]
pub struct Assets {
    loaded: HashMap<usize, Option<Arc<Picture>>>,
}

impl Assets {
    /// Loads what `presentation` draws and releases everything else.
    pub fn sync(&mut self, ui: &Interface, presentation: Presentation, mut load: impl FnMut(&ir::Asset) -> Option<Arc<Picture>>) {
        let need = ui.needed_assets(presentation);
        self.loaded.retain(|&k, _| need.get(k).copied().unwrap_or(false));
        for (k, _) in need.iter().enumerate().filter(|(_, n)| **n) {
            self.loaded.entry(k).or_insert_with(|| load(&ui.assets[k]));
        }
    }

    pub fn get(&self, a: ir::AssetRef) -> Option<&Arc<Picture>> {
        self.loaded.get(&a.0)?.as_ref()
    }

    /// Bytes of decoded pixels held, each image counted once.
    pub fn bytes(&self) -> usize {
        let mut seen = std::collections::HashSet::new();
        self.loaded
            .values()
            .flatten()
            .flat_map(|p| p.frames.iter())
            .filter(|i| seen.insert(Arc::as_ptr(i)))
            .map(|i| i.width as usize * i.height as usize * 4)
            .sum()
    }
}

/// Values the interface shows, by control.
pub type Values = HashMap<ControlId, f64>;

/// Runtime snapshots and pending widget edits, retained by the owning Face.
#[derive(Default)]
pub struct InputState {
    pub values: HashMap<WidgetRef, ir::Value>,
    pub meters: HashMap<WidgetRef, f64>,
    pub peaks: HashMap<WidgetRef, Arc<[(f32, f32)]>>,
    pub edits: Vec<Edit>,
    menu: Option<WidgetRef>,
    typing: Option<(WidgetRef, String)>,
    drafts: HashMap<WidgetRef, String>,
    cursors: HashMap<WidgetRef, usize>,
    files: HashMap<WidgetRef, (std::path::PathBuf, Vec<std::path::PathBuf>)>,
}

pub struct Edit {
    pub widget: WidgetRef,
    pub index: u32,
    pub value: ir::Value,
    pub mods: Mods,
    /// Even XY coordinate index, or the active table column.
    pub cursor: u32,
    /// W5 WidgetEventType: down=0, up=1, drag=2, drop=3.
    pub event: i32,
}

fn target(namespace: &str, n: WidgetRef) -> String {
    if namespace.is_empty() { format!("ir-{}", n.0) } else { format!("{namespace}-ir-{}", n.0) }
}


fn colour(c: ir::Rgba) -> Color {
    Color::srgba(f32::from(c.r) / 255., f32::from(c.g) / 255., f32::from(c.b) / 255., f32::from(c.a) / 255.)
}

fn picture(p: &Picture, n: usize) -> Option<Fill> {
    let f = p.frames.get(n.min(p.frames.len().saturating_sub(1)))?;
    Some(Fill::Image(f.clone(), Fit::Fill))
}

/// Whether the art under the middle of `n` is light: the nearest containing
/// panel's background picture there, else the wallpaper.
fn light_under(face: &Interface, assets: &Assets, n: WidgetRef) -> bool {
    let r = face.page_rect(n);
    let (x, y) = (f64::from(r.x) + f64::from(r.width) / 2., f64::from(r.y) + f64::from(r.height) / 2.);
    let luma = |img: &Image, u: f64, v: f64| {
        if u < 0. || v < 0. || u >= f64::from(img.width) || v >= f64::from(img.height) {
            return None;
        }
        let i = (v as usize * img.width as usize + u as usize) * 4;
        let c = img.rgba.get(i..i + 4)?;
        (c[3] >= 128).then(|| 0.2126 * f32::from(c[0]) + 0.7152 * f32::from(c[1]) + 0.0722 * f32::from(c[2]) > 140.)
    };
    let mut at = face.widgets[n.0].parent;
    while let Some(p) = at {
        let pr = face.page_rect(p);
        if let Some(img) = face.widgets[p.0].image(Use::Background).and_then(|a| assets.get(a)).and_then(|pic| pic.frames.first()) {
            let u = (x - f64::from(pr.x)) / f64::from(pr.width.max(1)) * f64::from(img.width);
            let v = (y - f64::from(pr.y)) / f64::from(pr.height.max(1)) * f64::from(img.height);
            if let Some(l) = luma(img, u, v) {
                return l;
            }
        }
        at = face.widgets[p.0].parent;
    }
    let page = &face.pages[face.widgets[n.0].page.0];
    let wall = page.background.image.and_then(|a| assets.get(a)).and_then(|pic| pic.frames.first());
    wall.and_then(|img| luma(img, x, y + f64::from(page.background.offset_y))).unwrap_or(false)
}

/// Kontakt's layout grid (`move_control`): column and row pitch, and the
/// first cell's corner; `set_ui_height` rows are [`GRID_ROW_HEIGHT`] tall.
const GRID: (i32, i32, i32, i32) = (92, 21, 66, 2);
const GRID_ROW_HEIGHT: u32 = 68;

/// A widget's size where its source left it to the host: Kontakt's stock sizes.
// ponytail: KSP stock sizes for every source; per-source tables once Falcon UIs arrive.
fn default_size(kind: &Kind) -> (u32, u32) {
    match kind {
        Kind::Knob { .. } => (85, 52),
        Kind::Table { .. } | Kind::Xy { .. } | Kind::MouseArea => (92, 92),
        Kind::Waveform | Kind::Wavetable { .. } => (184, 92),
        Kind::FileSelector { .. } => (184, 184),
        Kind::LevelMeter { orientation: ir::Orientation::Vertical } => (8, 92),
        Kind::Panel => (0, 0),
        _ => (85, 18),
    }
}

/// `face` with grid placement, grid page heights and host-sized widgets
/// turned into pixels.
pub fn resolved(face: &Interface) -> Interface {
    let mut face = face.clone();
    for p in &mut face.pages {
        if let Some(rows) = p.height_rows.take() {
            p.size.height = rows * GRID_ROW_HEIGHT;
        }
    }
    for w in &mut face.widgets {
        if let ir::Placement::Grid { column, row } = w.placement {
            w.rect.x = (column as i32 - 1) * GRID.0 + GRID.2;
            w.rect.y = (row as i32 - 1) * GRID.1 + GRID.3;
            w.placement = ir::Placement::Pixels;
        }
        if w.auto_size {
            (w.rect.width, w.rect.height) = default_size(&w.kind);
            w.auto_size = false;
        }
    }
    // Kontakt sizes a control to its picture along any axis the picture does not stretch.
    for n in 0..face.widgets.len() {
        let w = &face.widgets[n];
        let meta = w.images.iter().filter(|i| i.role != Use::Handle).find_map(|i| match &face.assets.get(i.asset.0)?.kind {
            ir::AssetKind::Image(m) => m.size.map(|s| (s, m.stretch)),
            ir::AssetKind::BitmapFont => None,
        });
        if let Some((size, stretch)) = meta {
            let r = &mut face.widgets[n].rect;
            if !stretch[0] {
                r.width = size.width;
            }
            if !stretch[1] {
                r.height = size.height;
            }
        }
    }
    face
}

/// `page` at `scale` points per source pixel; `face` already [`resolved`].
pub fn view(ui: &mut Ui, face: &Interface, page: PageRef, assets: &Assets, presentation: Presentation, scale: f64, values: &mut Values) -> El {
    view_state(ui, "", face, page, assets, presentation, scale, values, &mut InputState::default())
}

#[allow(clippy::too_many_arguments)]
pub fn view_state(ui: &mut Ui, namespace: &str, face: &Interface, page: PageRef, assets: &Assets, presentation: Presentation, scale: f64, values: &mut Values, input: &mut InputState) -> El {
    let Some(p) = face.pages.get(page.0) else { return caption("No interface").fill(secondary()) };
    let (w, h) = (f64::from(p.size.width) * scale, f64::from(height(face, page)) * scale);
    let mut layers = Vec::new();
    let ground = block(w, h).radius(0).fill(p.background.color.map_or(Fill::from(Role::Field), |c| Fill::from(colour(c))));
    layers.push(ground.at(0., 0.));
    // The wallpaper at its own size; the page shows it from `offset_y` down.
    if let Some(img) = p.background.image.and_then(|a| assets.get(a)).and_then(|pic| pic.frames.first()) {
        let (iw, ih) = (f64::from(img.width) * scale, f64::from(img.height) * scale);
        layers.push(block(iw, ih).radius(0).fill(Fill::Image(img.clone(), Fit::Fill)).at(0., -f64::from(p.background.offset_y) * scale));
    }
    for n in face.draw_order(page) {
        if !face.visible(n) {
            continue;
        }
        let r = face.page_rect(n);
        let (x, y, ww, hh) = (f64::from(r.x) * scale, f64::from(r.y) * scale, f64::from(r.width) * scale, f64::from(r.height) * scale);
        layers.push(widget_state(ui, namespace, face, n, assets, presentation, scale, values, input, ww, hh).at(x, y));
    }
    if let Some(popup) = menu_popup(ui, namespace, face, scale, values, input, w, h) { layers.push(popup); }
    stack(layers).w(w).h(h).shrink(0).clip().a11y(A11y::Group).named("Instrument interface").id(if namespace.is_empty() {"ir-view".to_owned()} else {format!("{namespace}-ir-view")})
}

/// The page's height, reaching down to its lowest visible control: a control
/// the source placed past the page edge is drawn whole, not cut.
pub fn height(face: &Interface, page: PageRef) -> u32 {
    let bottom = face.draw_order(page).into_iter().filter(|&n| face.visible(n)).map(|n| face.page_rect(n)).map(|r| (r.y + r.height as i32).max(0) as u32).max();
    face.pages[page.0].size.height.max(bottom.unwrap_or(0))
}

/// Axis and full-range travel in drawn pixels, independent of bitmap fallback.
fn gesture(w: &ir::Widget, scale: f64) -> (bool, f64) {
    let vertical = match w.kind {
        Kind::Knob { .. } | Kind::ValueEdit { .. } => true,
        Kind::Slider { orientation, .. } => w.drag.map_or(
            orientation == ir::Orientation::Vertical || w.rect.height > w.rect.width,
            |d| d.axis == ir::Orientation::Vertical,
        ),
        _ => true,
    };
    let travel = match w.drag.filter(|d| d.sensitivity != 0) {
        Some(d) => f64::from(if vertical { w.rect.height } else { w.rect.width }).max(1.) * 1000. / f64::from(d.sensitivity),
        None if matches!(w.kind, Kind::Slider { .. }) => f64::from(if vertical { w.rect.height } else { w.rect.width }).max(1.),
        None => 200.,
    };
    (vertical, travel * scale)
}

fn quantized(value: f64, range: &ir::Range) -> f64 {
    let value = match range.step.filter(|s| s.is_finite() && *s > 0.) {
        Some(step) => range.min + ((value - range.min) / step).round() * step,
        None => value,
    };
    value.clamp(range.min.min(range.max), range.min.max(range.max))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn widget_state(
    ui: &mut Ui,
    namespace: &str,
    face: &Interface,
    n: WidgetRef,
    assets: &Assets,
    presentation: Presentation,
    scale: f64,
    values: &mut Values,
    input: &mut InputState,
    w: f64,
    h: f64,
) -> El {
    let wd = &face.widgets[n.0];
    let id = target(namespace, n);
    let bitmap = presentation == Presentation::Bitmap;
    let strip = wd.image(Use::Strip).filter(|_| bitmap || wd.label_in_image()).and_then(|a| assets.get(a));
    let fixed = wd.images.iter().find(|i| i.role == Use::Strip).and_then(|i| i.frame).map(|f| f as usize);
    let control = match wd.binding {
        Binding::Control(c) => Some(c),
        _ => None,
    };
    let default = match &wd.kind {
        Kind::Knob { range, .. } | Kind::Slider { range, .. } | Kind::ValueEdit { range, .. } => range.default,
        _ => 0.,
    };
    let mut v = control.and_then(|c| values.get(&c).copied()).unwrap_or(default);
    let before = v;
    let text_size = wd.style.and_then(|s| face.styles[s.0].size).map_or(SMALL, f64::from) * scale;
    // Text on our own (dark) control faces is ours; the source's colour is for
    // text on its art: labels, and controls drawn by their pictures.
    let own_face = strip.is_none() && !matches!(wd.kind, Kind::Label);
    let ink = match wd.style {
        // A transparent colour is one the source left to its font (a custom bitmap font).
        Some(s) if !own_face && face.styles[s.0].color.a > 0 => Fill::from(colour(face.styles[s.0].color)),
        _ => Fill::from(Role::Ink),
    };
    let words = |t: String| caption(t).text_size(text_size).fill(ink.clone()).lines(1);
    let number = |x: f64, d: &ir::Display| {
        let x = x / if d.ratio == 0. { 1. } else { d.ratio };
        let x = if x.fract() == 0. { format!("{x}") } else { format!("{x:.2}") };
        format!("{x} {}", d.unit).trim().to_owned()
    };

    let face_el: El = match &wd.kind {
        Kind::Knob { range, .. } | Kind::Slider { range, .. } => {
            let (vertical, travel) = gesture(wd, scale);
            let held = wd.enabled && drive_widget(ui, &id, &mut v, &(range.min..=range.max), travel, vertical, range.default, range.step, true);
            if wd.enabled && (ui.get(id.as_str()).dragged || ui.get(id.as_str()).wheel != Vec2::ZERO || !ui.keys(id.as_str()).is_empty()) {
                v = quantized(v, range);
            }
            let lift = ui.state(id.as_str()).hover.max(if held { 1. } else { 0. }) as f32;
            let unit = |x: f64| if range.max == range.min { 0. } else { ((x - range.min) / (range.max - range.min)).clamp(0., 1.) };
            match strip {
                Some(p) => block(w, h).radius(0).fill(picture(p, fixed.unwrap_or_else(|| frame(v, range.min, range.max, p.frames.len()))).unwrap_or(Fill::from(Role::Field))),
                // A slider about as tall as wide was drawn as a knob by its strip.
                // Kontakt's stock knob: its name over the dial, the value under it.
                None if matches!(wd.kind, Kind::Knob { .. }) => {
                    let value = match (&wd.kind, &wd.value_text) {
                        // The script's own label, even an empty one, replaces the number.
                        (_, Some(t)) => t.clone(),
                        (Kind::Knob { display, .. }, _) => number(v, display),
                        _ => String::new(),
                    };
                    let mut parts = Vec::new();
                    if !wd.hide.title && !wd.text.is_empty() {
                        parts.push(words(wd.text.clone()));
                    }
                    parts.push(dial_face(unit(v), unit(range.min.max(0.).min(range.max)), lift, ui.focus_visible(&id)).flex(1).min_h(0).w(Len::Pct(100.)));
                    if !wd.hide.value && !value.is_empty() {
                        parts.push(words(value));
                    }
                    col(parts).gap(0).align(Align::Center)
                }
                None if (0.75..=1.33).contains(&(w / h.max(1.))) => dial_face(unit(v), unit(range.min.max(0.).min(range.max)), lift, ui.focus_visible(&id)),
                None => fader_face(unit(v), 0., None, vertical, lift, ui.focus_visible(&id)),
            }
            .cursor(if vertical { Cursor::ResizeV } else { Cursor::ResizeH })
            .focusable()
            .a11y(A11y::Slider { value: v, min: range.min, max: range.max })
        }
        Kind::Button { momentary: true } => {
            if wd.enabled { v = if ui.get(id.as_str()).held { 1. } else { 0. }; }
            match strip {
                Some(p) => block(w, h).radius(0).fill(picture(p, fixed.unwrap_or_else(|| switch_frame(v > 0.5, p.frames.len()))).unwrap_or(Fill::from(Role::Field))),
                None => row![words(wd.text.clone())].align(Align::Center).justify(Justify::Center).radius(1).fill(Role::Ink.alpha(0.08 + 0.2 * v as f32)),
            }
            .focusable()
            .a11y(A11y::Button)
        }
        Kind::Button { .. } | Kind::Switch => {
            if wd.enabled && ui.get(id.as_str()).activated() {
                v = if v > 0.5 { 0. } else { 1. };
            }
            let on = v > 0.5;
            match strip {
                Some(p) => block(w, h).radius(0).fill(picture(p, fixed.unwrap_or_else(|| switch_frame(on, p.frames.len()))).unwrap_or(Fill::from(Role::Field))),
                None => row![words(wd.text.clone())]
                    .align(Align::Center)
                    .justify(Justify::Center)
                    .radius(1)
                    .fill(if on { Fill::from(value_ink(0.).with_alpha(0.35)) } else { Role::Ink.alpha(0.08) })
                    .stroke(Role::Ink.alpha(if on { 0.6 } else { 0.2 }))
                    .stroke_width(1),
            }
            .focusable()
            .a11y(A11y::Toggle { on })
        }
        Kind::Menu { items } => {
            if wd.enabled && ui.get(id.as_str()).activated() {
                input.menu = if input.menu == Some(n) { None } else { Some(n) };
            }
            let shown: Vec<&ir::MenuItem> = items.iter().filter(|i| i.visible).collect();
            // Drawing an unknown semantic value must not edit the script.
            let at = shown.iter().position(|i| f64::from(i.value) == v).or((!shown.is_empty()).then_some(0));
            let label = at.map(|a| shown[a].text.clone()).unwrap_or_default();
            match strip {
                Some(p) => stack![block(w, h).radius(0).fill(picture(p, 0).unwrap_or(Fill::from(Role::Field))), row![words(label)].align(Align::Center).pad((TIGHT * scale, 0.)).w(w).h(h)],
                None => row![words(label).flex(1).min_w(0), glyph(Icon::Down, TIGHT * 2. * scale, secondary())]
                    .align(Align::Center)
                    .pad((TIGHT * scale, 0.))
                    .fill(Role::Ink.alpha(0.08)),
            }
            .focusable()
            .a11y(A11y::Button)
        }
        Kind::ValueEdit { range, display, arrows } => {
            let response = ui.get(id.as_str());
            if wd.enabled && (response.double_clicked || ui.keys(id.as_str()).iter().any(|k| k.key == Key::Enter)) {
                input.typing = Some((n, format!("{}", v / if display.ratio == 0. {1.} else {display.ratio})));
            }
            if input.typing.as_ref().is_some_and(|(at,_)| *at == n) {
                let edit_id = format!("{id}-type");
                let mounted = ui.scene().is_some_and(|s| s.surface(&edit_id).is_some());
                let (_, text) = input.typing.as_mut().unwrap();
                let field = text_edit(ui, edit_id.as_str(), text, TextOpts { blur_on_submit:true, ..Default::default() });
                if !mounted { ui.focus(edit_id.as_str()); }
                let cancel = !wd.enabled || ui.keys(edit_id.as_str()).iter().any(|k| k.key == Key::Escape);
                if cancel { input.typing = None; }
                else if field.changed.submitted || mounted && !ui.focused(edit_id.as_str()) {
                    if let Ok(number) = text.trim().parse::<f64>() { if number.is_finite() { v = quantized(number * if display.ratio == 0. {1.} else {display.ratio}, range); } }
                    input.typing = None;
                }
                row![field.el].align(Align::Center)
            } else {
                let (vertical, travel) = gesture(wd, scale);
                if wd.enabled { drive_widget(ui, &id, &mut v, &(range.min..=range.max), travel, vertical, range.default, range.step, false); }
                let mut parts = Vec::new();
                if !wd.hide.title && !wd.text.is_empty() { parts.push(words(wd.text.clone()).fill(secondary()).flex(1).min_w(0)); }
                parts.push(words(wd.value_text.clone().filter(|t| !t.is_empty()).unwrap_or_else(|| number(v, display))));
                if *arrows {
                    let up = format!("{id}-up"); let down = format!("{id}-down");
                    if wd.enabled && ui.get(up.as_str()).activated() { v = quantized(v + range.step.unwrap_or(1.),range); }
                    if wd.enabled && ui.get(down.as_str()).activated() { v = quantized(v - range.step.unwrap_or(1.),range); }
                    parts.push(col![glyph(Icon::Up,TIGHT*scale,secondary()).focusable().id(up),glyph(Icon::Down,TIGHT*scale,secondary()).focusable().id(down)].gap(0));
                }
                row(parts).gap(TIGHT * scale).align(Align::Center).justify(Justify::Center).pad((TIGHT * scale, 0.)).fill(Role::Ink.alpha(0.06)).cursor(Cursor::ResizeV).focusable().a11y(A11y::Slider {value:v,min:range.min,max:range.max})
            }
        }
        Kind::Label => row![caption(wd.text.clone()).text_size(text_size).fill(ink.clone())].align(Align::Center),
        Kind::LevelMeter { orientation } => {
            let level = input.meters.get(&n).copied().unwrap_or(0.);
            let [lo,hi] = wd.meter_range.unwrap_or([0,1_000_000]);
            let unit = if lo == hi {0.} else {((level*1_000_000.-f64::from(lo))/f64::from(hi-lo)).clamp(0.,1.)};
            let vertical = *orientation == ir::Orientation::Vertical;
            canvas(move |s| {
                let area = if vertical {rect(0.,s.height*(1.-unit),s.width,s.height*unit)} else {rect(0.,0.,s.width*unit,s.height)};
                vec![Draw::fill(area,signal())]
            }).fill(Role::Ink.alpha(0.12))
        },
        Kind::Table { columns, range, cells, .. } => {
            let mut samples = match input.values.get(&n).or(wd.value.as_ref()) {
                Some(ir::Value::Integers(v)) => v.iter().map(|v|f64::from(*v)).collect::<Vec<_>>(),
                Some(ir::Value::Reals(v)) => v.clone(),
                _ => cells.iter().map(|v|*v as f64).collect(),
            };
            samples.resize(*columns as usize,range.default);
            let response = ui.get(id.as_str());
            if wd.enabled && (response.pressed || response.dragged) && !samples.is_empty() {
                if let Some(point) = ui.local(id.as_str()) {
                    let column = ((point.x/w.max(1.)).clamp(0.,1.)*samples.len() as f64).floor() as usize;
                    let column = column.min(samples.len()-1);
                    let value = quantized(range.min+(1.-point.y/h.max(1.)).clamp(0.,1.)*(range.max-range.min),range);
                    samples[column]=value;
                    let value = if range.step == Some(1.) {ir::Value::Integer(value as i32)} else {ir::Value::Real(value)};
                    input.edits.push(Edit{widget:n,index:column as u32,value,mods:response.mods,cursor:column as u32,event:if response.dragged {2} else {0}});
                    input.values.insert(n,ir::Value::Reals(samples.clone()));
                }
            }
            let range = *range;
            canvas(move |s| {
                let bw = s.width/samples.len().max(1) as f64;
                let unit = |v:f64| if range.max == range.min {0.} else {((v-range.min)/(range.max-range.min)).clamp(0.,1.)};
                let zero=s.height*(1.-unit(0.));
                samples.iter().enumerate().map(|(c,v)| {
                    let y=s.height*(1.-unit(*v));
                    Draw::fill(rect(c as f64*bw,y.min(zero),(bw-1.).max(1.),(y-zero).abs().max(1.)),value_ink(0.))
                }).collect()
            }).fill(Role::Ink.alpha(0.06)).cursor(Cursor::Crosshair).focusable()
        }
        Kind::Xy { cursors, .. } => {
            let mut points = match input.values.get(&n).or(wd.value.as_ref()) {Some(ir::Value::Reals(v))=>v.clone(), _=>vec![0.; *cursors as usize*2]};
            points.resize(*cursors as usize*2,0.);
            let response=ui.get(id.as_str());
            if wd.enabled && (response.pressed || response.dragged) && !points.is_empty() {
                if let Some(point)=ui.local(id.as_str()) {
                    let (x,y)=((point.x/w.max(1.)).clamp(0.,1.),(1.-point.y/h.max(1.)).clamp(0.,1.));
                    let cursor=if response.pressed {
                        let nearest=points.chunks_exact(2).enumerate().min_by(|(_,a),(_,b)| ((a[0]-x).powi(2)+(a[1]-y).powi(2)).total_cmp(&((b[0]-x).powi(2)+(b[1]-y).powi(2)))).map_or(0,|(i,_)|i);
                        input.cursors.insert(n,nearest); nearest
                    } else {input.cursors.get(&n).copied().unwrap_or(0)};
                    for (index,value) in [(cursor*2,x),(cursor*2+1,y)] {
                        points[index]=value;
                        input.edits.push(Edit{widget:n,index:index as u32,value:ir::Value::Real(value),mods:response.mods,cursor:(cursor*2) as u32,event:if response.dragged {2} else {0}});
                    }
                    input.values.insert(n,ir::Value::Reals(points.clone()));
                }
            }
            canvas(move |s|points.chunks_exact(2).map(|point|Draw::fill(rect(point[0]*s.width-3.,(1.-point[1])*s.height-3.,6.,6.),value_ink(0.))).collect()).fill(Role::Ink.alpha(0.06)).cursor(Cursor::Crosshair).focusable()
        }
        Kind::TextEdit => {
            let draft=input.drafts.entry(n).or_insert_with(||match input.values.get(&n).or(wd.value.as_ref()) {Some(ir::Value::Text(v))=>v.clone(),_=>String::new()});
            let field=text_edit(ui,id.as_str(),draft,TextOpts {blur_on_submit:true,..Default::default()});
            if wd.enabled && field.changed.submitted {
                let text=ir::Value::Text(draft.clone());
                input.edits.push(Edit{widget:n,index:0,value:text.clone(),mods:ui.get(id.as_str()).mods,cursor:0,event:1});
                input.values.insert(n,text);
            }
            field.el
        }
        Kind::FileSelector {base_path,files,..} => {
            let directory=base_path.as_deref().unwrap_or(".");
            let (path,entries)=input.files.entry(n).or_insert_with(|| {
                let path=std::path::PathBuf::from(directory);
                // ponytail: one directory read per navigation; move to a worker if large folders stall.
                let mut entries=std::fs::read_dir(&path).into_iter().flatten().filter_map(Result::ok).map(|e|e.path()).filter(|p|p.is_dir() || file_matches(p,*files)).collect::<Vec<_>>();
                entries.sort(); (path,entries)
            });
            let mut rows=Vec::new();
            let up=format!("{id}-parent");
            let mut next=None;
            if path.parent().is_some() {
                if wd.enabled && ui.get(up.as_str()).activated() {next=path.parent().map(std::path::Path::to_owned);}
                rows.push(caption("..").pad(TIGHT*scale).focusable().id(up));
            }
            for (at,path) in entries.iter().enumerate() {
                let item=format!("{id}-file-{at}");
                if wd.enabled && ui.get(item.as_str()).activated() {
                    if path.is_dir() {next=Some(path.clone());}
                    else {
                        let value=ir::Value::Text(path.to_string_lossy().into_owned());
                        input.edits.push(Edit{widget:n,index:0,value:value.clone(),mods:ui.get(item.as_str()).mods,cursor:0,event:0});
                        input.values.insert(n,value);
                    }
                }
                rows.push(caption(path.file_name().unwrap_or_default().to_string_lossy().into_owned()).pad(TIGHT*scale).focusable().id(item));
            }
            if let Some(next)=next {
                let mut entries=std::fs::read_dir(&next).into_iter().flatten().filter_map(Result::ok).map(|e|e.path()).filter(|p|p.is_dir() || file_matches(p,*files)).collect::<Vec<_>>();
                entries.sort(); input.files.insert(n,(next,entries));
            }
            col(rows).gap(0).scroll().fill(Role::Ink.alpha(0.06))
        }
        Kind::Waveform | Kind::Wavetable {..} => {
            let peaks=input.peaks.get(&n).cloned().unwrap_or_default();
            canvas(move |s| {
                let width=s.width/peaks.len().max(1) as f64;
                peaks.iter().enumerate().map(|(i,(lo,hi))|Draw::fill(rect(i as f64*width,(1.-f64::from(*hi))*s.height/2.,width.max(1.),f64::from(hi-lo)*s.height/2.),Role::Ink)).collect()
            }).fill(Role::Ink.alpha(0.06))
        }
        Kind::Panel | Kind::Image | Kind::MouseArea => block(w, h),
    };
    if let Some(c) = control {
        if wd.enabled && v.to_bits() != before.to_bits() {
            let response = ui.get(id.as_str());
            let mods = if matches!(wd.kind,Kind::Menu{..}|Kind::ValueEdit{..}) {Mods::default()} else {response.mods};
            let value = match &wd.kind {
                Kind::Knob{range,..}|Kind::Slider{range,..}|Kind::ValueEdit{range,..} if range.step != Some(1.) => ir::Value::Real(v),
                _ => ir::Value::Integer(v.round() as i32),
            };
            input.edits.push(Edit{widget:n,index:0,value,mods,cursor:0,event:if response.dragged {2} else {0}});
        }
        values.insert(c, v);
    }
    // Our faces are light-on-dark: a control without its own picture sits on
    // a dark plate (Kontakt's stock controls are dark too), in either mode,
    // wherever the art under it is light.
    let plate = strip.is_none()
        && !matches!(wd.kind, Kind::Label | Kind::Panel | Kind::Image | Kind::MouseArea)
        && light_under(face, assets, n);
    let face_el = if plate { face_el.radius(2).fill(Color::oklch(0.2, 0., 0.).with_alpha(0.85)) } else { face_el };
    let interactive = matches!(wd.kind, Kind::Knob { .. } | Kind::Slider { .. } | Kind::Button { .. } | Kind::Switch | Kind::Menu { .. } | Kind::ValueEdit { .. } | Kind::Table {..} | Kind::Xy {..} | Kind::FileSelector {..} | Kind::TextEdit | Kind::MouseArea);
    // MUI reserves slash-prefixed IDs for non-target decoration.
    let target = if interactive { id } else { format!("/{id}") };
    let mut el = face_el.w(w).h(h).shrink(0).id(target).when(!wd.enabled, |e| e.disabled()).named(wd.automation.name.clone().unwrap_or_else(|| wd.name.clone()));
    if !wd.tooltip.is_empty() {
        el = el.tip(wd.tooltip.clone());
    }
    let bg = wd.images.iter().find(|i| i.role == Use::Background);
    match bg.and_then(|i| assets.get(i.asset).and_then(|p| picture(p, i.frame.unwrap_or(0) as usize))) {
        Some(bg) if !wd.hide.background => stack![block(w, h).radius(0).fill(bg), el].w(w).h(h).shrink(0),
        _ => el,
    }
}


#[allow(clippy::too_many_arguments)]
pub(super) fn menu_popup(ui:&mut Ui, namespace:&str, face:&Interface, scale:f64, values:&mut Values, input:&mut InputState, width:f64, height:f64) -> Option<El> {
    let n = input.menu?;
    let wd = face.widgets.get(n.0)?;
    let Kind::Menu {items} = &wd.kind else { input.menu=None; return None };
    let anchor = target(namespace,n);
    let popup = format!("{anchor}-popup");
    if !wd.enabled || !face.visible(n) || ui.dismissed(&[popup.as_str(),anchor.as_str()]) { input.menu=None; return None; }
    let mut rows = Vec::new();
    for (at,item) in items.iter().enumerate().filter(|(_,item)|item.visible) {
        let id = format!("{anchor}-item-{at}");
        if ui.get(id.as_str()).activated() {
            if let Binding::Control(control) = wd.binding {
                values.insert(control,f64::from(item.value));
                input.edits.push(Edit{widget:n,index:0,value:ir::Value::Integer(item.value),mods:Mods::default(),cursor:0,event:0});
            }
            input.menu=None;
        }
        rows.push(caption(item.text.clone()).pad((TIGHT*scale,SPACE*scale)).fill(Role::Ink).focusable().a11y(A11y::Button).id(id));
    }
    let r = face.page_rect(n);
    let root = if namespace.is_empty() {"ir-view".to_owned()} else {format!("{namespace}-ir-view")};
    let positioned = ui.scene().and_then(|scene| {
        let anchor=scene.surface(&anchor)?.frame;
        let root=scene.surface(&root)?.frame;
        Some((anchor.x-root.x,anchor.y-root.y,anchor.size.width,anchor.size.height))
    });
    let (ax,ay,aw,ah)=positioned.unwrap_or((f64::from(r.x)*scale,f64::from(r.y)*scale,f64::from(r.width)*scale,f64::from(r.height)*scale));
    let w = aw.max(120.).min(width);
    let h = (rows.len() as f64 * CONTROL*scale).min(height);
    let x = ax.clamp(0.,(width-w).max(0.));
    let y = (ay+ah).clamp(0.,(height-h).max(0.));
    Some(col(rows).gap(0).w(w).h(h).scroll().fill(Role::Field).stroke(Role::Ink.alpha(0.3)).stroke_width(1).id(popup).at(x,y))
}

fn file_matches(path:&std::path::Path,files:ir::Files)->bool {
    let extension=path.extension().and_then(|x|x.to_str()).unwrap_or("").to_ascii_lowercase();
    match files {
        ir::Files::Any=>true,
        ir::Files::Audio=>matches!(extension.as_str(),"wav"|"aif"|"aiff"|"flac"|"ogg"|"mp3"|"ncw"),
        ir::Files::Midi=>matches!(extension.as_str(),"mid"|"midi"),
        ir::Files::Data=>matches!(extension.as_str(),"nka"|"nkr"|"txt"),
    }
}
