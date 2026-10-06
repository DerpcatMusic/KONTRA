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

use super::perf_view::{frame, switch_frame};
use super::theme::*;
use crate::artwork::Picture;
use moose::mui::mui::prelude::*;
use moose::mui::mui::scene::Fit;
use sampler_ui_ir::{self as ir, Binding, ControlId, Interface, Kind, PageRef, Presentation, Role as Use, WidgetRef};
use std::collections::HashMap;
use std::sync::Arc;

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

fn colour(c: ir::Rgba) -> Color {
    Color::srgba(f32::from(c.r) / 255., f32::from(c.g) / 255., f32::from(c.b) / 255., f32::from(c.a) / 255.)
}

fn picture(p: &Picture, n: usize) -> Option<Fill> {
    let f = p.frames.get(n.min(p.frames.len().saturating_sub(1)))?;
    Some(Fill::Image(f.clone(), Fit::Fill))
}

/// `page` at `scale` points per source pixel.
pub fn view(ui: &mut Ui, face: &Interface, page: PageRef, assets: &Assets, presentation: Presentation, scale: f64, values: &mut Values) -> El {
    let Some(p) = face.pages.get(page.0) else { return caption("No interface").fill(secondary()) };
    let (w, h) = (f64::from(p.size.width) * scale, f64::from(p.size.height) * scale);
    let mut layers = Vec::new();
    let mut ground = block(w, h).radius(0).fill(p.background.color.map_or(Fill::from(Role::Field), |c| Fill::from(colour(c))));
    if let Some(f) = p.background.image.and_then(|a| assets.get(a)).and_then(|pic| picture(pic, 0)) {
        ground = ground.fill(f);
    }
    layers.push(ground.at(0., 0.));
    for n in face.draw_order(page) {
        if !face.visible(n) {
            continue;
        }
        let r = face.page_rect(n);
        let (x, y, ww, hh) = (f64::from(r.x) * scale, f64::from(r.y) * scale, f64::from(r.width) * scale, f64::from(r.height) * scale);
        layers.push(widget(ui, face, n, assets, presentation, scale, values, ww, hh).at(x, y));
    }
    stack(layers).w(w).h(h).shrink(0).clip().a11y(A11y::Group).named("Instrument interface").id("ir-view")
}

#[allow(clippy::too_many_arguments)]
fn widget(
    ui: &mut Ui,
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
    let id = format!("ir-{}", n.0);
    let bitmap = presentation == Presentation::Bitmap;
    let strip = wd.image(Use::Strip).filter(|_| bitmap).and_then(|a| assets.get(a));
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
    let ink = wd.style.map_or(Fill::from(Role::Ink), |s| Fill::from(colour(face.styles[s.0].color)));
    let words = |t: String| caption(t).text_size(text_size).fill(ink.clone()).lines(1);

    let face_el: El = match &wd.kind {
        Kind::Knob { range, .. } | Kind::Slider { range, .. } => {
            let vertical = !matches!(wd.kind, Kind::Slider { orientation: ir::Orientation::Horizontal, .. });
            let held = drive(ui, &id, &mut v, &(range.min..=range.max), TRAVEL, vertical, range.default);
            let lift = ui.state(id.as_str()).hover.max(if held { 1. } else { 0. }) as f32;
            let unit = |x: f64| if range.max == range.min { 0. } else { ((x - range.min) / (range.max - range.min)).clamp(0., 1.) };
            match strip {
                Some(p) => block(w, h).radius(0).fill(picture(p, frame(v, range.min, range.max, p.frames.len())).unwrap_or(Fill::from(Role::Field))),
                None if matches!(wd.kind, Kind::Knob { .. }) => dial_face(unit(v), unit(range.min.max(0.).min(range.max)), lift, ui.focus_visible(&id)),
                None => fader_face(unit(v), 0., None, vertical, lift, ui.focus_visible(&id)),
            }
            .cursor(if vertical { Cursor::ResizeV } else { Cursor::ResizeH })
            .focusable()
            .a11y(A11y::Slider { value: v, min: range.min, max: range.max })
        }
        Kind::Button { momentary: true } => {
            v = if ui.get(id.as_str()).held { 1. } else { 0. };
            match strip {
                Some(p) => block(w, h).radius(0).fill(picture(p, switch_frame(v > 0.5, p.frames.len())).unwrap_or(Fill::from(Role::Field))),
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
                Some(p) => block(w, h).radius(0).fill(picture(p, switch_frame(on, p.frames.len())).unwrap_or(Fill::from(Role::Field))),
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
            let at = shown.iter().position(|i| f64::from(i.value) == v);
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
        Kind::ValueEdit { range, display } => {
            drive(ui, &id, &mut v, &(range.min..=range.max), TRAVEL, true, range.default);
            let shown = v / if display.ratio == 0. { 1. } else { display.ratio };
            row![words(format!("{shown} {}", display.unit).trim().to_owned())]
                .align(Align::Center)
                .justify(Justify::Center)
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
        Kind::Xy { .. } | Kind::Waveform | Kind::Wavetable | Kind::FileSelector | Kind::TextEdit => {
            row![words(wd.text.clone())].align(Align::Center).pad((TIGHT * scale, 0.)).fill(Role::Ink.alpha(0.06)).stroke(Role::Ink.alpha(0.15)).stroke_width(1)
        }
        Kind::Panel | Kind::Image | Kind::MouseArea => block(w, h),
    };
    if let Some(c) = control {
        values.insert(c, v);
    }
    let mut el = face_el.w(w).h(h).shrink(0).id(id).named(wd.automation_name.clone().unwrap_or_else(|| wd.name.clone()));
    if !wd.tooltip.is_empty() {
        el = el.tip(wd.tooltip.clone());
    }
    match wd.image(Use::Background).and_then(|a| assets.get(a)).and_then(|p| picture(p, 0)) {
        Some(bg) if !wd.hide.background => stack![block(w, h).radius(0).fill(bg), el].w(w).h(h).shrink(0),
        _ => el,
    }
}
