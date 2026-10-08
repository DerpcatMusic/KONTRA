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
    identities: HashMap<usize, ir::Asset>,
}

impl Assets {
    /// Loads what `presentation` draws and releases everything else.
    pub fn sync(&mut self, ui: &Interface, presentation: Presentation, mut load: impl FnMut(&ir::Asset) -> Option<Arc<Picture>>) {
        let need = ui.needed_assets(presentation);
        self.loaded.retain(|&k, _| need.get(k).copied().unwrap_or(false));
        for (k, _) in need.iter().enumerate().filter(|(_, n)| **n) {
            if self.identities.get(&k) != Some(&ui.assets[k]) { self.loaded.remove(&k); }
            self.identities.insert(k, ui.assets[k].clone());
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
    let count = face.widgets.len();
    resolve_changed(&mut face, 0..count);
    face
}

/// Normalize only changed source widgets; publications retain all other widgets.
pub fn resolve_changed(face: &mut Interface, indices: impl IntoIterator<Item = usize>) {
    for p in &mut face.pages {
        if let Some(rows) = p.height_rows.take() { p.size.height = rows * GRID_ROW_HEIGHT; }
    }
    for n in indices {
        let Some(w) = face.widgets.get_mut(n) else { continue };
        if let ir::Placement::Grid { column, row } = w.placement {
            w.rect.x = (column as i32 - 1) * GRID.0 + GRID.2;
            w.rect.y = (row as i32 - 1) * GRID.1 + GRID.3;
            w.placement = ir::Placement::Pixels;
        }
        if w.auto_size { (w.rect.width, w.rect.height) = default_size(&w.kind); w.auto_size = false; }
        let meta = w.images.iter().filter(|i| i.role != Use::Handle).find_map(|i| match &face.assets.get(i.asset.0)?.kind {
            ir::AssetKind::Image(m) => m.size.map(|s| (s, m.stretch)),
            ir::AssetKind::BitmapFont => None,
        });
        if let Some((size, stretch)) = meta {
            if !stretch[0] { w.rect.width = size.width; }
            if !stretch[1] { w.rect.height = size.height; }
        }
    }
}

/// `page` at `scale` points per source pixel; `face` already [`resolved`].
pub fn view(ui: &mut Ui, face: &Interface, page: PageRef, assets: &Assets, presentation: Presentation, scale: f64, values: &mut Values) -> El {
    view_scoped(ui, "", face, page, assets, presentation, scale, values)
}

pub fn view_scoped(ui: &mut Ui, namespace: &str, face: &Interface, page: PageRef, assets: &Assets, presentation: Presentation, scale: f64, values: &mut Values) -> El {
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
        layers.push(widget(ui, namespace, face, n, assets, presentation, scale, values, ww, hh).at(x, y));
    }
    stack(layers).w(w).h(h).shrink(0).clip().a11y(A11y::Group).named("Instrument interface").id(format!("{namespace}ir-view"))
}

/// The page's height, reaching down to its lowest visible control: a control
/// the source placed past the page edge is drawn whole, not cut.
pub fn height(face: &Interface, page: PageRef) -> u32 {
    let bottom = face.draw_order(page).into_iter().filter(|&n| face.visible(n)).map(|n| face.page_rect(n)).map(|r| (r.y + r.height as i32).max(0) as u32).max();
    face.pages[page.0].size.height.max(bottom.unwrap_or(0))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn widget(
    ui: &mut Ui,
    namespace: &str,
    face: &Interface,
    n: WidgetRef,
    assets: &Assets,
    presentation: Presentation,
    scale: f64,
    values: &mut Values,
    w: f64,
    h: f64,
) -> El {
    let wd = &face.widgets[n.0];
    let id = format!("{namespace}ir-{}", n.0);
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
            let vertical = !matches!(wd.kind, Kind::Slider { orientation: ir::Orientation::Horizontal, .. });
            let held = drive(ui, &id, &mut v, &(range.min..=range.max), TRAVEL, vertical, range.default);
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
            v = if ui.get(id.as_str()).held { 1. } else { 0. };
            match strip {
                Some(p) => block(w, h).radius(0).fill(picture(p, fixed.unwrap_or_else(|| switch_frame(v > 0.5, p.frames.len()))).unwrap_or(Fill::from(Role::Field))),
                None => row![words(wd.text.clone())].align(Align::Center).justify(Justify::Center).radius(1).fill(Role::Ink.alpha(0.08 + 0.2 * v as f32)),
            }
            .focusable()
            .a11y(A11y::Button)
        }
        Kind::Button { .. } | Kind::Switch => {
            if ui.get(id.as_str()).activated() {
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
            let shown: Vec<&ir::MenuItem> = items.iter().filter(|i| i.visible).collect();
            // A value no entry has shows the first, as Kontakt does.
            let at = shown.iter().position(|i| f64::from(i.value) == v).or((!shown.is_empty()).then_some(0));
            if let Some(a) = at {
                v = f64::from(shown[a].value);
            }
            // ponytail: click steps to the next entry; a floating list belongs with the shared menu once it leaves v1 targets.
            if ui.get(id.as_str()).activated() && !shown.is_empty() {
                v = f64::from(shown[at.map_or(0, |a| (a + 1) % shown.len())].value);
            }
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
        Kind::ValueEdit { range, display, .. } => {
            drive(ui, &id, &mut v, &(range.min..=range.max), TRAVEL, true, range.default);
            // Kontakt's value edit: its name, then the value.
            let mut parts = Vec::new();
            if !wd.hide.title && !wd.text.is_empty() {
                parts.push(words(wd.text.clone()).fill(secondary()).flex(1).min_w(0));
            }
            parts.push(words(wd.value_text.clone().filter(|t| !t.is_empty()).unwrap_or_else(|| number(v, display))));
            row(parts)
                .gap(TIGHT * scale)
                .align(Align::Center)
                .justify(Justify::Center)
                .pad((TIGHT * scale, 0.))
                .fill(Role::Ink.alpha(0.06))
                .cursor(Cursor::ResizeV)
                .focusable()
                .a11y(A11y::Slider { value: v, min: range.min, max: range.max })
        }
        Kind::Label => row![caption(wd.text.clone()).text_size(text_size).fill(ink.clone())].align(Align::Center),
        Kind::LevelMeter { .. } => meter_v(|| [0.; 2]),
        Kind::Table { columns, .. } => {
            let columns = (*columns).max(1) as usize;
            canvas(move |s| {
                let bw = s.width / columns as f64;
                (0..columns).map(|c| Draw::fill(rect(c as f64 * bw, s.height - 1., (bw - 1.).max(1.), 1.), Role::Ink.alpha(0.5))).collect()
            })
            .fill(Role::Ink.alpha(0.06))
        }
        Kind::Xy { .. } | Kind::Waveform | Kind::Wavetable { .. } | Kind::FileSelector { .. } | Kind::TextEdit => {
            row![words(wd.text.clone())].align(Align::Center).pad((TIGHT * scale, 0.)).fill(Role::Ink.alpha(0.06)).stroke(Role::Ink.alpha(0.15)).stroke_width(1)
        }
        Kind::Panel | Kind::Image | Kind::MouseArea => block(w, h),
    };
    if let Some(c) = control {
        values.insert(c, v);
    }
    // Our faces are light-on-dark: a control without its own picture sits on
    // a dark plate (Kontakt's stock controls are dark too), in either mode,
    // wherever the art under it is light.
    let plate = strip.is_none()
        && !matches!(wd.kind, Kind::Label | Kind::Panel | Kind::Image | Kind::MouseArea)
        && light_under(face, assets, n);
    let face_el = if plate { face_el.radius(2).fill(Color::oklch(0.2, 0., 0.).with_alpha(0.85)) } else { face_el };
    let mut el = face_el.w(w).h(h).shrink(0).id(id).named(wd.automation.name.clone().unwrap_or_else(|| wd.name.clone()));
    if !wd.tooltip.is_empty() {
        el = el.tip(wd.tooltip.clone());
    }
    let bg = wd.images.iter().find(|i| i.role == Use::Background);
    match bg.and_then(|i| assets.get(i.asset).and_then(|p| picture(p, i.frame.unwrap_or(0) as usize))) {
        Some(bg) if !wd.hide.background => stack![block(w, h).radius(0).fill(bg), el].w(w).h(h).shrink(0),
        _ => el,
    }
}
