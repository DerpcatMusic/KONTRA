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
    fonts: HashMap<usize, Option<Font>>,
    pub meter: Option<Arc<dyn Fn(Option<u32>,u8)->[f32;2] + Send + Sync>>,
    open_menu: std::cell::Cell<Option<usize>>,
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

    pub fn sync_fonts(&mut self, face: &Interface, mut load: impl FnMut(&ir::Asset) -> Option<Font>) {
        self.fonts.retain(|&i,_| face.assets.get(i).is_some_and(|a| matches!(a.kind,ir::AssetKind::TrueTypeFont)));
        for (i,a) in face.assets.iter().enumerate().filter(|(_,a)| matches!(a.kind,ir::AssetKind::TrueTypeFont)) {
            self.fonts.entry(i).or_insert_with(||load(a));
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
        if face.source == ir::Source::FalconLua { continue }
        let w = &face.widgets[n];
        let meta = w.images.iter().filter(|i| i.role != Use::Handle).find_map(|i| match &face.assets.get(i.asset.0)?.kind {
            ir::AssetKind::Image(m) => m.size.map(|s| (s, m.stretch)),
            ir::AssetKind::BitmapFont | ir::AssetKind::TrueTypeFont => None,
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
        let mut el = widget(ui, face, n, assets, presentation, scale, values, ww, hh);
        let (mut x,mut y)=(x,y);
        let mut parent=face.widgets[n.0].parent;
        while let Some(p)=parent {
            if face.widgets[p.0].viewport.is_some() {
                let r=face.page_rect(p);
                let (px,py)=(r.x as f64*scale,r.y as f64*scale);
                el=stack![el.at(x-px,y-py)].w(r.width as f64*scale).h(r.height as f64*scale).clip();
                (x,y)=(px,py);
            }
            parent=face.widgets[p.0].parent;
        }
        layers.push(el.at(x,y));
    }
    let base=stack(layers).w(w).h(h).shrink(0).clip().a11y(A11y::Group).named("Instrument interface").id("ir-view");
    let Some(index)=assets.open_menu.get() else {return base};
    let Some(wd)=face.widgets.get(index).filter(|w|w.enabled && !w.hidden) else {assets.open_menu.set(None);return base};
    let Kind::Menu{items}=&wd.kind else {assets.open_menu.set(None);return base};
    let source=format!("ir-{index}");
    if ui.dismissed(&["ir-menu",&source]) { assets.open_menu.set(None);return base }
    let r=face.page_rect(WidgetRef(index));
    let mut selected=None;
    let rows=items.iter().filter(|i|i.visible).map(|item| {
        let key=format!("ir-menu-{index}-{}",item.value);
        if ui.get(key.as_str()).activated() {selected=Some(item.value);}
        caption(item.text.clone()).text_size(SMALL*scale).lines(1).pad((8.,4.)).min_h(24.).w(Len::Pct(100.)).focusable().a11y(A11y::Button).named(item.text.clone()).id(key)
    }).collect::<Vec<_>>();
    if let Some(value)=selected {
        if let Binding::Control(id)=wd.binding {values.insert(id,value as f64);}
        assets.open_menu.set(None);
        ui.focus(&source);
        return base
    }
    let mw=(r.width as f64*scale).max(180.).min(w);
    let mh=(rows.len() as f64*24.).min(240.).min(h);
    let x=(r.x as f64*scale).clamp(0.,(w-mw).max(0.));
    let y=((r.y+r.height as i32) as f64*scale).min((h-mh).max(0.));
    stack![base,col(rows).gap(0).w(mw).h(mh).scroll().fill(Role::Field).stroke(Role::Ink.alpha(0.25)).stroke_width(1.).id("ir-menu").at(x,y)].w(w).h(h)

}

/// The page's height, reaching down to its lowest visible control: a control
/// the source placed past the page edge is drawn whole, not cut.
pub fn height(face: &Interface, page: PageRef) -> u32 {
    let bottom = face.draw_order(page).into_iter().filter(|&n| face.visible(n)).map(|n| face.page_rect(n)).map(|r| (r.y + r.height as i32).max(0) as u32).max();
    face.pages[page.0].size.height.max(bottom.unwrap_or(0))
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
    let strip = wd.image(Use::Strip).filter(|_| bitmap || wd.label_in_image()).and_then(|a| assets.get(a));
    let hovered=ui.state(id.as_str()).hover>0.;
    let state_picture=|on:bool| {
        let hover=hovered;
        let role=match (on,hover){(true,true)=>Use::HoverPressed,(true,false)=>Use::Pressed,(false,true)=>Use::Hover,_=>Use::Strip};
        wd.image(role).and_then(|a|assets.get(a)).filter(|_|bitmap||wd.label_in_image()).or(strip)
    };
    let fixed = wd.images.iter().find(|i| i.role == Use::Strip).and_then(|i| i.frame).map(|f| f as usize);
    let control = match wd.binding {
        Binding::Control(c) => Some(c),
        _ => None,
    };
    let default = match &wd.kind {
        Kind::Knob { range, .. } | Kind::Slider { range, .. } | Kind::ValueEdit { range, .. } => range.default,
        _ => wd.initial_value,
    };
    let mut enabled = wd.enabled;
    let mut opacity = wd.opacity;
    let mut parent = wd.parent;
    for _ in 0..face.widgets.len() {
        let Some(p) = parent.and_then(|p| face.widgets.get(p.0)) else { break };
        enabled &= p.enabled;
        opacity *= p.opacity;
        parent = p.parent;
    }
    let can_edit = enabled && wd.intercepts_mouse;
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
    let words = |t: String| {
        let el=caption(t).text_size(text_size).fill(ink.clone()).lines(1);
        let font=wd.style.and_then(|s|match face.styles[s.0].font { ir::Font::File(a)=>assets.fonts.get(&a.0)?.clone(),_=>None });
        match font { Some(font)=>el.font(font), None=>el }
    };
    let number = |x: f64, d: &ir::Display| {
        let x = x / if d.ratio == 0. { 1. } else { d.ratio };
        let x = if x.fract() == 0. { format!("{x}") } else { format!("{x:.2}") };
        format!("{x} {}", d.unit).trim().to_owned()
    };

    let face_el: El = match &wd.kind {
        Kind::Knob { range, .. } | Kind::Slider { range, .. } => {
            let vertical = !matches!(wd.kind, Kind::Slider { orientation: ir::Orientation::Horizontal, .. });
            let held = if can_edit {
                drive(ui, &id, &mut v, &(range.min..=range.max), TRAVEL, vertical, range.default)
            } else {false};
            if let Some(step)=range.step { v=(v/step).round()*step; }
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
            // UVI Button is stateless: one callback per activation, including keyboard.
            v = if can_edit && ui.get(id.as_str()).activated() { 1. } else { 0. };
            let pressed = can_edit && ui.get(id.as_str()).held;
            match state_picture(pressed) {
                Some(p) => block(w, h).radius(0).fill(picture(p, fixed.unwrap_or_else(|| switch_frame(v > 0.5, p.frames.len()))).unwrap_or(Fill::from(Role::Field))),
                None => row![words(wd.text.clone())].align(Align::Center).justify(Justify::Center).radius(1).fill(Role::Ink.alpha(0.08 + 0.2 * v as f32)),
            }
            .focusable()
            .a11y(A11y::Button)
        }
        Kind::Button { .. } | Kind::Switch => {
            if can_edit && ui.get(id.as_str()).activated() {
                v = if v > 0.5 { 0. } else { 1. };
            }
            let on = v > 0.5;
            match state_picture(on) {
                Some(p) => block(w, h).radius(0).fill(picture(p, fixed.unwrap_or_else(|| switch_frame(on, p.frames.len()))).unwrap_or(Fill::from(Role::Field))),
                None => row![words(wd.text.clone())]
                    .align(Align::Center)
                    .justify(Justify::Center)
                    .radius(1)
                    .fill(if on { wd.colors.on.map_or(Fill::from(value_ink(0.).with_alpha(0.35)),|c|Fill::from(colour(c))) } else { wd.colors.off.map_or(Role::Ink.alpha(0.08),|c|Fill::from(colour(c))) })
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
            if can_edit && ui.get(id.as_str()).activated() && !shown.is_empty() {
                if wd.menu_cycle { v=f64::from(shown[at.map_or(0,|a|(a+1)%shown.len())].value); }
                else { assets.open_menu.set(if assets.open_menu.get()==Some(n.0){None}else{Some(n.0)}); }
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
            if can_edit { drive(ui, &id, &mut v, &(range.min..=range.max), TRAVEL, true, range.default); }
            if let Some(step)=range.step { v=(v/step).round()*step; }
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
        Kind::Label => row![words(wd.text.clone())].align(Align::Center),
        Kind::LevelMeter { .. } => {
            let source=assets.meter.clone();
            let (bus,channel)=match wd.binding{Binding::Meter{bus,channel}=>(bus,channel),_=>(None,0)};
            meter_v(move ||source.as_ref().map(|s|s(bus,channel)).unwrap_or([0.;2]))
        },
        Kind::Table { columns, range, cells, .. } => {
            let bars=(0..*columns as usize).map(|c| {
                let cid=wd.components.get(c).copied();
                let mut value=cid.and_then(|id|values.get(&id).copied()).unwrap_or_else(||cells.get(c).copied().unwrap_or(0.));
                let key=format!("{id}-cell-{c}");
                if can_edit { drive(ui,&key,&mut value,&(range.min..=range.max),TRAVEL,true,range.default); }
                if let Some(step)=range.step { value=(value/step).round()*step; }
                if let Some(id)=cid {values.insert(id,value);}
                let t=if range.max==range.min{0.}else{((value-range.min)/(range.max-range.min)).clamp(0.,1.)};
                canvas(move |s| vec![Draw::fill(rect(0.,s.height*(1.-t),s.width.max(1.),s.height*t),Role::Ink.alpha(0.6))])
                    .fill(Role::Ink.alpha(0.06)).flex(1).h(h).id(key).focusable()
                    .named(format!("{} {}",wd.name,c+1)).a11y(A11y::Slider{value,min:range.min,max:range.max})
            }).collect::<Vec<_>>();
            row(bars).gap(1).w(w).h(h)
        }
        Kind::Xy { .. } if wd.components.len()==2 => {
            let mut xy=[0.5;2];
            let mut axes=Vec::new();
            for axis in 0..2 {
                let cid=wd.components[axis];
                let target=face.widgets.iter().find(|w|w.binding==Binding::Control(cid));
                let range=target.and_then(|w|match w.kind{Kind::Knob{range,..}|Kind::Slider{range,..}|Kind::ValueEdit{range,..}=>Some(range),_=>None}).unwrap_or(ir::Range{min:0.,max:1.,default:0.5,step:None});
                let mut value=values.get(&cid).copied().unwrap_or(range.default);
                let key=format!("{id}-axis-{axis}");
                if can_edit {
                    drive(ui,&key,&mut value,&(range.min..=range.max),TRAVEL,false,range.default);
                    if ui.get(id.as_str()).held && let Some(p)=ui.local(&id) {
                        let t=if axis==0 {(p.x/w.max(1.)).clamp(0.,1.)}else{(1.-p.y/(h-18.).max(1.)).clamp(0.,1.)};
                        value=range.min+t*(range.max-range.min);
                    }
                }
                if let Some(step)=range.step {value=(value/step).round()*step;}
                values.insert(cid,value);
                xy[axis]=if range.max==range.min{0.}else{((value-range.min)/(range.max-range.min)).clamp(0.,1.)};
                axes.push(fader_face(xy[axis],0.,None,false,0.,ui.focus_visible(&key)).h(16).flex(1).id(key).focusable().named(format!("{} {}",wd.name,if axis==0{"X"}else{"Y"})).a11y(A11y::Slider{value,min:range.min,max:range.max}));
            }
            let pad=canvas(move |s|vec![Draw::fill(rect(xy[0]*(s.width-6.),(1.-xy[1])*(s.height-6.),6.,6.),Role::Ink.alpha(0.8))]).fill(Role::Ink.alpha(0.06)).w(w).h((h-18.).max(1.)).id(id.clone());
            col![pad,row(axes).gap(2).h(16)].gap(2)
        }
        Kind::Xy { .. } | Kind::Waveform | Kind::Wavetable { .. } | Kind::FileSelector { .. } | Kind::TextEdit => {
            row![words(wd.text.clone())].align(Align::Center).pad((TIGHT * scale, 0.)).fill(Role::Ink.alpha(0.06)).stroke(Role::Ink.alpha(0.15)).stroke_width(1)
        }
        Kind::Panel | Kind::Image | Kind::MouseArea => block(w, h),
    };
    if let Some(c) = control
        && wd.components.is_empty() {
        values.insert(c, v);
    }
    // Our faces are light-on-dark: a control without its own picture sits on
    // a dark plate (Kontakt's stock controls are dark too), in either mode,
    // wherever the art under it is light.
    let plate = strip.is_none()
        && !matches!(wd.kind, Kind::Label | Kind::Panel | Kind::Image | Kind::MouseArea)
        && light_under(face, assets, n);
    let face_el = if let Some(c)=wd.colors.background {face_el.fill(colour(c))}else{face_el};
    let face_el = if plate { face_el.radius(2).fill(Color::oklch(0.2, 0., 0.).with_alpha(0.85)) } else { face_el };
    let face_el = if can_edit {face_el}else{face_el.disabled()};
    let mut el = face_el.w(w).h(h).shrink(0).opacity(opacity).id(id).named(wd.automation.name.clone().unwrap_or_else(|| wd.name.clone()));
    if !wd.tooltip.is_empty() {
        el = el.tip(wd.tooltip.clone());
    }
    let bg = wd.images.iter().find(|i| i.role == Use::Background);
    match bg.and_then(|i| assets.get(i.asset).and_then(|p| picture(p, i.frame.unwrap_or(0) as usize))) {
        Some(bg) if !wd.hide.background => stack![block(w, h).radius(0).fill(bg), el].w(w).h(h).shrink(0),
        _ => el,
    }
}

/// Exercise the production asset loader, layout and CPU painter without a window.
#[cfg(feature = "shots")]
pub fn uvi_ui_health(face: &Interface, path: &std::path::Path) -> serde_json::Value {
    let mut source = super::pictures::Source::of(path);
    let mut assets = Assets::default();
    let (mut image_errors, mut font_errors) = (0, 0);
    assets.sync(face, Presentation::Bitmap, |asset| {
        if !matches!(asset.kind, ir::AssetKind::Image(_)) { return None; }
        let picture = source.load(asset);
        image_errors += usize::from(picture.is_none());
        picture
    });
    assets.sync_fonts(face, |asset| {
        let font = source.font(asset);
        font_errors += usize::from(font.is_none());
        font
    });
    let render = || -> Result<(), String> {
        face.validate().map_err(|e| e.to_string())?;
        let Some(page) = face.pages.first() else { return Err("no UI page".into()); };
        let scale = (1100. / f64::from(page.size.width.max(1))).min(1.);
        let (width, height) = ((f64::from(page.size.width) * scale).ceil().clamp(1., 1100.) as u16,
            (f64::from(page.size.height) * scale).ceil().clamp(1., 4096.) as u16);
        let mut values = Values::default();
        let mut ui = super::theme::ui();
        let face = resolved(face);
        for _ in 0..2 {
            let root = view(&mut ui, &face, PageRef(0), &assets, Presentation::Bitmap, scale, &mut values);
            ui.frame(root, Some(Size::new(width.into(), height.into())), Input::default(), 1./60.)
                .map_err(|e| e.to_string())?;
        }
        use moose::mui::mui::vello::{self, vello_cpu::{Pixmap, RenderContext, Resources}};
        let mut ctx = RenderContext::new(width, height);
        let mut resources = Resources::default();
        vello::paint(&mut vello::Cpu { ctx: &mut ctx, resources: &mut resources, cache: &mut vello::Cache::default() },
            ui.scene().ok_or("no scene")?, vello::kurbo::Affine::IDENTITY).map_err(|e| e.to_string())?;
        ctx.flush();
        ctx.render(&mut Pixmap::new(width, height), &mut resources);
        Ok(())
    };
    // Never log a panic payload: library text/resources may be embedded in it.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(render));
    let render_error = match result { Ok(Ok(())) => None, Ok(Err(e)) => Some(e), Err(_) => Some("render panic".into()) };
    serde_json::json!({"image_errors":image_errors,"font_errors":font_errors,"render_error":render_error})
}
