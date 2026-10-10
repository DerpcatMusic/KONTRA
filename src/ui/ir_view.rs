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
use sampler_ui_ir::{
    self as ir, Binding, ControlId, Interface, Kind, PageRef, Presentation, Role as Use, WidgetRef,
};
use std::collections::HashMap;
use std::sync::Arc;

/// An image cut into its animation frames.
#[derive(Debug)]
pub struct Picture {
    pub frames: Vec<Arc<Image>>,
    indices: Vec<usize>,
    count: usize,
    /// Source rectangle represented by prepared pixels (wallpaper windows).
    pub window: Option<[u32; 4]>,
}

impl Picture {
    pub fn new(frames: Vec<Arc<Image>>) -> Self {
        let count = frames.len();
        Self {
            frames,
            indices: (0..count).collect(),
            count,
            window: None,
        }
    }
    pub(super) fn prepared(
        image: Arc<Image>,
        frame: usize,
        count: usize,
        window: Option<[u32; 4]>,
    ) -> Self {
        Self {
            frames: vec![image],
            indices: vec![frame],
            count,
            window,
        }
    }
    fn len(&self) -> usize {
        self.count
    }
    fn at(&self, n: usize) -> Option<&Arc<Image>> {
        self.indices
            .iter()
            .position(|&i| i == n.min(self.count.saturating_sub(1)))
            .and_then(|i| self.frames.get(i))
            .or_else(|| self.frames.first())
    }
}

/// The frame of `frames` a control at `value` in `min..=max` shows.
fn frame(value: f64, min: f64, max: f64, frames: usize) -> usize {
    let span = max - min;
    let t = if span == 0. {
        0.
    } else {
        ((value - min) / span).clamp(0., 1.)
    };
    (t * frames.saturating_sub(1) as f64).round() as usize
}

/// UVI mappers operate on normalized positions, including drag and strip frames.
fn mapped(range: &ir::Range, mapper: Option<&str>, value: f64, inverse: bool) -> f64 {
    if range.max == range.min {
        return if inverse { 0. } else { range.min };
    }
    if mapper == Some("Exponential") && range.min > 0. && range.max > range.min {
        return if inverse {
            (value.clamp(range.min, range.max) / range.min).ln() / (range.max / range.min).ln()
        } else {
            range.min * (range.max / range.min).powf(value.clamp(0., 1.))
        };
    }
    let power = match mapper {
        Some("Quadratic") => 2.,
        Some("Cubic") => 3.,
        Some("Quartic") => 4.,
        Some("Quintic") => 5.,
        Some("SquareRoot") => 0.5,
        Some("CubeRoot") => 1. / 3.,
        Some("QuarticRoot") => 0.25,
        Some("QuinticRoot") => 0.2,
        _ => 1.,
    };
    if inverse {
        ((value - range.min) / (range.max - range.min))
            .clamp(0., 1.)
            .powf(1. / power)
    } else {
        range.min + (range.max - range.min) * value.clamp(0., 1.).powf(power)
    }
}
fn drive_mapped(
    ui: &mut Ui,
    id: &str,
    value: &mut f64,
    range: &ir::Range,
    mapper: Option<&str>,
    vertical: bool,
) -> bool {
    let mut position = mapped(range, mapper, *value, true);
    let before = position;
    let held = drive(
        ui,
        id,
        &mut position,
        &(0. ..=1.),
        TRAVEL,
        vertical,
        mapped(range, mapper, range.default, true),
    );
    if position != before {
        *value = mapped(range, mapper, position, false);
    }
    held
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
    pub meter: Option<Arc<dyn Fn(Option<u32>, u8) -> [f32; 2] + Send + Sync>>,
    identities: HashMap<usize, ir::Asset>,
    preparation: Option<super::picture_worker::Preparation>,
}

impl Assets {
    pub fn prepare(
        &mut self,
        path: &std::path::Path,
        face: &Interface,
        page: PageRef,
        presentation: Presentation,
        scale: f64,
        values: &Values,
    ) {
        if self.preparation.as_ref().is_none_or(|p| p.path() != path) {
            self.preparation = Some(super::picture_worker::Preparation::new(path));
        }
        let prepared =
            self.preparation
                .as_mut()
                .unwrap()
                .prepare(face, page, presentation, scale, values);
        self.loaded.clear();
        self.fonts.clear();
        for (n, picture, font) in prepared {
            if picture.is_some() {
                self.loaded.insert(n, picture);
            }
            if font.is_some() {
                self.fonts.insert(n, font);
            }
        }
    }
    pub fn pending(&self) -> usize {
        self.preparation.as_ref().map_or(0, |p| p.pending())
    }

    #[cfg(feature = "shots")]
    pub fn scan(&self) -> super::pictures::Scan {
        self.preparation
            .as_ref()
            .map_or(Default::default(), |p| p.scan)
    }
    #[cfg(feature = "shots")]
    pub fn failures(&self) -> Vec<String> {
        self.preparation
            .as_ref()
            .map_or(Vec::new(), |p| p.failures())
    }
    #[cfg(feature = "shots")]
    pub fn completed_key_bytes(&self) -> Vec<(String, usize)> {
        self.preparation
            .as_ref()
            .map_or_else(Vec::new, |p| p.completed_key_bytes())
    }
    /// Loads what `presentation` draws and releases everything else.
    pub fn sync(
        &mut self,
        ui: &Interface,
        presentation: Presentation,
        mut load: impl FnMut(&ir::Asset) -> Option<Arc<Picture>>,
    ) {
        let need = ui.needed_assets(presentation);
        self.loaded
            .retain(|&k, _| need.get(k).copied().unwrap_or(false));
        for (k, _) in need.iter().enumerate().filter(|(_, n)| **n) {
            if self.identities.get(&k) != Some(&ui.assets[k]) {
                self.loaded.remove(&k);
            }
            self.identities.insert(k, ui.assets[k].clone());
            self.loaded.entry(k).or_insert_with(|| load(&ui.assets[k]));
        }
    }

    pub fn sync_fonts(
        &mut self,
        face: &Interface,
        mut load: impl FnMut(&ir::Asset) -> Option<Font>,
    ) {
        self.fonts.retain(|&i, _| {
            face.assets
                .get(i)
                .is_some_and(|a| matches!(a.kind, ir::AssetKind::TrueTypeFont))
        });
        for (i, a) in face
            .assets
            .iter()
            .enumerate()
            .filter(|(_, a)| matches!(a.kind, ir::AssetKind::TrueTypeFont))
        {
            self.fonts.entry(i).or_insert_with(|| load(a));
        }
    }

    pub(super) fn font(&self, a: &ir::AssetRef) -> Option<Font> {
        self.fonts.get(&a.0)?.clone()
    }

    pub fn get(&self, a: ir::AssetRef) -> Option<&Arc<Picture>> {
        self.loaded.get(&a.0)?.as_ref()
    }

    /// Bytes of decoded pixels held, each image counted once.
    pub fn bytes(&self) -> usize {
        if let Some(p) = &self.preparation {
            return p.bytes();
        }
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
    pub wave_duration_us: HashMap<WidgetRef, u64>,
    pub edits: Vec<Edit>,
    menu: Option<WidgetRef>,
    typing: Option<(WidgetRef, String)>,
    drafts: HashMap<WidgetRef, String>,
    cursors: HashMap<WidgetRef, usize>,
    xy_drags: HashMap<WidgetRef, (usize, [f64; 2], Point)>,
    table_drags: HashMap<WidgetRef, (usize, f64)>,
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
    pub mouse_over: bool,
}

fn target(namespace: &str, n: WidgetRef) -> String {
    if namespace.is_empty() {
        format!("ir-{}", n.0)
    } else {
        format!("{namespace}-ir-{}", n.0)
    }
}

fn colour(c: ir::Rgba) -> Color {
    Color::srgba(
        f32::from(c.r) / 255.,
        f32::from(c.g) / 255.,
        f32::from(c.b) / 255.,
        f32::from(c.a) / 255.,
    )
}

fn picture(p: &Picture, n: usize) -> Option<Fill> {
    let f = p.at(n)?;
    Some(Fill::Image(f.clone(), Fit::Fill))
}

/// Sample authored backgrounds in paint order, compositing alpha over page colour.
fn light_under(face: &Interface, assets: &Assets, n: WidgetRef) -> bool {
    let r = face.page_rect(n);
    let (x, y) = (
        r.x as f64 + r.width as f64 / 2.,
        r.y as f64 + r.height as f64 / 2.,
    );
    let page = &face.pages[face.widgets[n.0].page.0];
    let base = page.background.color.unwrap_or(ir::Rgba::rgb(0x202020));
    let mut rgb = [base.r as f32, base.g as f32, base.b as f32];
    let blend = |rgb: &mut [f32; 3], c: ir::Rgba| {
        let a = c.a as f32 / 255.;
        for (out, v) in rgb.iter_mut().zip([c.r, c.g, c.b]) {
            *out = *out * (1. - a) + v as f32 * a;
        }
    };
    let sample = |rgb: &mut [f32; 3], img: &Image, u: f64, v: f64| {
        if u < 0. || v < 0. || u >= img.width as f64 || v >= img.height as f64 {
            return;
        }
        let i = (v as usize * img.width as usize + u as usize) * 4;
        if let Some(c) = img.rgba.get(i..i + 4) {
            blend(
                rgb,
                ir::Rgba {
                    r: c[0],
                    g: c[1],
                    b: c[2],
                    a: c[3],
                },
            );
        }
    };
    if let Some(pic) = page.background.image.and_then(|a| assets.get(a))
        && let Some(img) = pic.at(page.background.frame as usize)
    {
        let [wx, wy, sw, sh] = pic.window.unwrap_or([0, 0, img.width, img.height]);
        sample(
            &mut rgb,
            img,
            (x - wx as f64) * img.width as f64 / sw.max(1) as f64,
            (y + page.background.origin_y as f64 + page.background.offset_y.max(0) as f64
                - wy as f64)
                * img.height as f64
                / sh.max(1) as f64,
        );
    }
    for at in face.draw_order(face.widgets[n.0].page) {
        if at == n {
            break;
        }
        if !face.visible(at) {
            continue;
        }
        let w = &face.widgets[at.0];
        let r = face.page_rect(at);
        if x < r.x as f64
            || y < r.y as f64
            || x >= (r.x as f64 + r.width as f64)
            || y >= (r.y as f64 + r.height as f64)
        {
            continue;
        }
        if let Some(c) = w.colors.background {
            blend(&mut rgb, c);
        }
        if w.hide.background {
            continue;
        }
        if let Some(img) = w
            .images
            .iter()
            .find(|i| i.role == Use::Background)
            .and_then(|i| {
                assets
                    .get(i.asset)
                    .and_then(|p| p.at(i.frame.unwrap_or(0) as usize))
            })
        {
            sample(
                &mut rgb,
                img,
                (x - r.x as f64) / r.width.max(1.) as f64 * img.width as f64,
                (y - r.y as f64) / r.height.max(1.) as f64 * img.height as f64,
            );
        }
    }
    0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2] > 140.
}

fn art(
    face: &Interface,
    asset: ir::AssetRef,
    p: &Picture,
    n: usize,
    w: f64,
    h: f64,
    scale: f64,
) -> El {
    let Some(image) = p.at(n) else {
        return block(w, h);
    };
    let meta = match face.assets[asset.0].kind {
        ir::AssetKind::Image(m) => m,
        _ => ir::ImageMeta::default(),
    };
    super::render_art::sliced(image, meta, w, h, scale)
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
        Kind::LevelMeter {
            orientation: ir::Orientation::Vertical,
        } => (8, 92),
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
        if let Some(rows) = p.height_rows.take() {
            p.size.height = f64::from(rows * GRID_ROW_HEIGHT);
        }
    }
    for n in indices {
        let Some(w) = face.widgets.get_mut(n) else {
            continue;
        };
        if let ir::Placement::Grid { column, row } = w.placement {
            w.rect.x = f64::from((column as i32 - 1) * GRID.0 + GRID.2);
            w.rect.y = f64::from((row as i32 - 1) * GRID.1 + GRID.3);
            w.placement = ir::Placement::Pixels;
        }
        if w.auto_size {
            let defaults = default_size(&w.kind);
            let axes = if w.default_axes == [false; 2] {
                [true; 2]
            } else {
                w.default_axes
            };
            if axes[0] {
                w.rect.width = f64::from(defaults.0);
            }
            if axes[1] {
                w.rect.height = f64::from(defaults.1);
            }
            w.auto_size = false;
            w.default_axes = [false; 2];
        }
        if face.source == ir::Source::FalconLua {
            continue;
        }
        let meta = w
            .images
            .iter()
            .filter(|i| i.role != Use::Handle)
            .find_map(|i| match &face.assets.get(i.asset.0)?.kind {
                ir::AssetKind::Image(m) => m.size.map(|s| (s, m.stretch)),
                ir::AssetKind::BitmapFont | ir::AssetKind::TrueTypeFont => None,
            });
        if let Some((size, stretch)) = meta {
            if !stretch[0] {
                w.rect.width = size.width;
            }
            if !stretch[1] {
                w.rect.height = size.height;
            }
        }
    }
}

/// `page` at `scale` points per source pixel; `face` already [`resolved`].
pub fn view(
    ui: &mut Ui,
    face: &Interface,
    page: PageRef,
    assets: &Assets,
    presentation: Presentation,
    scale: f64,
    values: &mut Values,
) -> El {
    view_state(
        ui,
        "",
        face,
        page,
        assets,
        presentation,
        scale,
        values,
        &mut InputState::default(),
    )
}

#[allow(clippy::too_many_arguments)]
pub fn view_state(
    ui: &mut Ui,
    namespace: &str,
    face: &Interface,
    page: PageRef,
    assets: &Assets,
    presentation: Presentation,
    scale: f64,
    values: &mut Values,
    input: &mut InputState,
) -> El {
    let Some(p) = face.pages.get(page.0) else {
        return caption("No interface").fill(secondary());
    };
    let (w, h) = (
        f64::from(p.size.width) * scale,
        f64::from(height(face, page)) * scale,
    );
    let mut layers = Vec::new();
    let ground = block(w, h).radius(0).fill(
        p.background
            .color
            .map_or(Fill::from(Role::Field), |c| Fill::from(colour(c))),
    );
    layers.push(ground.at(0., 0.));
    // The wallpaper at its own size; the page shows it from `offset_y` down.
    if let Some(pic) = p.background.image.and_then(|a| assets.get(a))
        && let Some(img) = pic.at(p.background.frame as usize)
    {
        let [x, y, sw, sh] = pic.window.unwrap_or([0, 0, img.width, img.height]);
        layers.push(
            block(sw as f64 * scale, sh as f64 * scale)
                .radius(0)
                .fill(Fill::Image(img.clone(), Fit::Fill))
                .at(
                    x as f64 * scale,
                    (y as f64 - p.background.origin_y as f64 - p.background.offset_y.max(0) as f64)
                        * scale,
                ),
        );
    }
    for n in face.draw_order(page) {
        if !face.visible(n) {
            continue;
        }
        let r = face.page_rect(n);
        let intersects = |clip: ir::Rect| {
            r.x < clip.x + clip.width
                && r.y < clip.y + clip.height
                && r.x + r.width > clip.x
                && r.y + r.height > clip.y
        };
        if r.width == 0.
            || r.height == 0.
            || !intersects(ir::Rect { x: 0., y: 0., width: p.size.width, height: p.size.height })
        {
            continue;
        }
        if face.source == ir::Source::FalconLua {
            let mut parent = face.widgets[n.0].parent;
            let mut clipped = false;
            for _ in 0..face.widgets.len() {
                let Some(at) = parent else { break };
                let Some(panel) = face.widgets.get(at.0) else {
                    break;
                };
                if panel.viewport.is_some() && !intersects(face.page_rect(at)) {
                    clipped = true;
                    break;
                }
                parent = panel.parent;
            }
            if clipped {
                continue;
            }
        }
        let (x, y, ww, hh) = (
            f64::from(r.x) * scale,
            f64::from(r.y) * scale,
            f64::from(r.width) * scale,
            f64::from(r.height) * scale,
        );
        let mut el = widget_state(
            ui,
            namespace,
            face,
            n,
            assets,
            presentation,
            scale,
            values,
            input,
            ww,
            hh,
        );
        let (mut x, mut y) = (x, y);
        let mut parent = face.widgets[n.0].parent;
        while let Some(p) = parent {
            if face.widgets[p.0].viewport.is_some() {
                let r = face.page_rect(p);
                let (px, py) = (r.x as f64 * scale, r.y as f64 * scale);
                el = stack![el.at(x - px, y - py)]
                    .w(r.width as f64 * scale)
                    .h(r.height as f64 * scale)
                    .clip();
                (x, y) = (px, py);
            }
            parent = face.widgets[p.0].parent;
        }
        layers.push(el.at(x, y));
    }
    if let Some(popup) = menu_popup(ui, namespace, face, scale, values, input, w, h) {
        layers.push(popup);
    }
    stack(layers)
        .w(w)
        .h(h)
        .shrink(0)
        .clip()
        .a11y(A11y::Group)
        .named("Instrument interface")
        .id(if namespace.is_empty() {
            "ir-view".to_owned()
        } else {
            format!("{namespace}-ir-view")
        })
}

/// The height declared by the script, after grid rows are resolved.
pub fn height(face: &Interface, page: PageRef) -> f64 {
    face.pages[page.0].size.height
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
        Some(d) => {
            f64::from(if vertical {
                w.rect.height
            } else {
                w.rect.width
            })
            .max(1.)
                * 1000.
                / f64::from(d.sensitivity)
        }
        None if matches!(w.kind, Kind::Slider { .. }) => f64::from(if vertical {
            w.rect.height
        } else {
            w.rect.width
        })
        .max(1.),
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
    let strip = wd
        .image(Use::Strip)
        .filter(|_| bitmap || wd.label_in_image())
        .and_then(|a| assets.get(a));
    let hovered = ui.state(id.as_str()).hover > 0.;
    let state_picture = |on: bool| {
        let hover = hovered;
        let role = match (on, hover) {
            (true, true) => Use::HoverPressed,
            (true, false) => Use::Pressed,
            (false, true) => Use::Hover,
            _ => Use::Strip,
        };
        wd.image(role)
            .and_then(|a| assets.get(a))
            .filter(|_| bitmap || wd.label_in_image())
            .or(strip)
    };
    let fixed = wd
        .images
        .iter()
        .find(|i| i.role == Use::Strip)
        .and_then(|i| i.frame)
        .map(|f| f as usize);
    let control = match wd.binding {
        Binding::Control(c) => Some(c),
        _ => None,
    };
    let initial = match wd.value.as_ref() {
        Some(ir::Value::Integer(value)) => f64::from(*value),
        Some(ir::Value::Real(value)) => *value,
        _ => match &wd.kind {
            Kind::Knob { range, .. }
            | Kind::Slider { range, .. }
            | Kind::ValueEdit { range, .. } => range.default,
            _ => wd.initial_value,
        },
    };
    let mut enabled = wd.enabled;
    let mut opacity = wd.opacity;
    let mut parent = wd.parent;
    for _ in 0..face.widgets.len() {
        let Some(p) = parent.and_then(|p| face.widgets.get(p.0)) else {
            break;
        };
        enabled &= p.enabled;
        opacity *= p.opacity;
        parent = p.parent;
    }
    let can_edit = enabled && wd.intercepts_mouse;
    let mut v = control
        .and_then(|c| values.get(&c).copied())
        .unwrap_or(initial);
    let state = usize::from(v > 0.5)
        + if ui.get(id.as_str()).held {
            2
        } else if hovered {
            4
        } else {
            0
        };
    let style = if matches!(
        wd.kind,
        Kind::Button { .. } | Kind::Switch | Kind::Menu { .. }
    ) {
        wd.state_styles[state].or(wd.style)
    } else {
        wd.style
    }
    .and_then(|s| face.styles.get(s.0));
    let before = v;
    let own_face = strip.is_none() && !matches!(wd.kind, Kind::Label);
    let ink = match style {
        Some(s) if !own_face && s.color.a > 0 => Fill::from(colour(s.color)),
        _ if !own_face && light_under(face, assets, n) => Fill::from(Color::srgb(0.1, 0.1, 0.1)),
        _ => Fill::from(Role::Ink),
    };
    let words = |t: String| {
        super::render_art::words(
            &t,
            style,
            assets,
            bitmap,
            ink.clone(),
            w,
            style.and_then(|s| s.size).map_or(SMALL, f64::from) * scale * 1.4,
            scale,
            None,
            false,
        )
    };
    let number = |x: f64, d: &ir::Display| {
        let x = x / if d.ratio == 0. { 1. } else { d.ratio };
        let x = if x.fract() == 0. {
            format!("{x}")
        } else {
            format!("{x:.2}")
        };
        format!("{x} {}", d.unit).trim().to_owned()
    };

    let face_el: El = match &wd.kind {
        Kind::Knob { range, .. } | Kind::Slider { range, .. } => {
            let (vertical, travel) = gesture(wd, scale);
            let held = if can_edit {
                if wd.mapper.is_some() {
                    drive_mapped(ui, &id, &mut v, range, wd.mapper.as_deref(), vertical)
                } else {
                    drive_widget(
                        ui,
                        &id,
                        &mut v,
                        &(range.min..=range.max),
                        travel,
                        vertical,
                        range.default,
                        range.step,
                        true,
                    )
                }
            } else {
                false
            };
            if can_edit
                && (ui.get(id.as_str()).dragged
                    || ui.get(id.as_str()).wheel != Vec2::ZERO
                    || !ui.keys(id.as_str()).is_empty())
            {
                v = quantized(v, range);
            }
            let lift = ui.state(id.as_str()).hover.max(if held { 1. } else { 0. }) as f32;
            let unit = |x: f64| mapped(range, wd.mapper.as_deref(), x, true);
            match strip {
                Some(p) => art(
                    face,
                    wd.image(Use::Strip).unwrap(),
                    p,
                    fixed.unwrap_or_else(|| frame(unit(v), 0., 1., p.len())),
                    w,
                    h,
                    scale,
                ),
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
                    parts.push(
                        dial_face(
                            unit(v),
                            unit(range.min.max(0.).min(range.max)),
                            lift,
                            ui.focus_visible(&id),
                        )
                        .flex(1)
                        .min_h(0)
                        .w(Len::Pct(100.)),
                    );
                    if !wd.hide.value && !value.is_empty() {
                        parts.push(words(value));
                    }
                    col(parts).gap(0).align(Align::Center)
                }
                None if (0.75..=1.33).contains(&(w / h.max(1.))) => dial_face(
                    unit(v),
                    unit(range.min.max(0.).min(range.max)),
                    lift,
                    ui.focus_visible(&id),
                ),
                None => fader_face(unit(v), 0., None, vertical, lift, ui.focus_visible(&id)),
            }
            .cursor(if vertical {
                Cursor::ResizeV
            } else {
                Cursor::ResizeH
            })
            .focusable()
            .a11y(A11y::Slider {
                value: v,
                min: range.min,
                max: range.max,
            })
        }
        Kind::Button { momentary: true } => {
            // UVI Button is stateless: one callback per activation, including keyboard.
            if can_edit {
                v = if face.source == ir::Source::FalconLua {
                    if ui.get(id.as_str()).activated() {
                        1.
                    } else {
                        0.
                    }
                } else {
                    if ui.get(id.as_str()).held { 1. } else { 0. }
                };
            }
            let pressed = can_edit && ui.get(id.as_str()).held;
            match state_picture(pressed) {
                Some(p) => block(w, h).radius(0).fill(
                    picture(p, fixed.unwrap_or_else(|| switch_frame(v > 0.5, p.len())))
                        .unwrap_or(Fill::from(Role::Field)),
                ),
                None => row![words(wd.text.clone())]
                    .align(Align::Center)
                    .justify(Justify::Center)
                    .radius(1)
                    .fill(Role::Ink.alpha(0.08 + 0.2 * v as f32)),
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
                Some(p) => block(w, h).radius(0).fill(
                    picture(p, fixed.unwrap_or_else(|| switch_frame(on, p.len())))
                        .unwrap_or(Fill::from(Role::Field)),
                ),
                None => row![words(wd.text.clone())]
                    .align(Align::Center)
                    .justify(Justify::Center)
                    .radius(1)
                    .fill(if on {
                        wd.colors
                            .on
                            .map_or(Fill::from(value_ink(0.).with_alpha(0.35)), |c| {
                                Fill::from(colour(c))
                            })
                    } else {
                        wd.colors
                            .off
                            .map_or(Role::Ink.alpha(0.08), |c| Fill::from(colour(c)))
                    })
                    .stroke(Role::Ink.alpha(if on { 0.6 } else { 0.2 }))
                    .stroke_width(1),
            }
            .focusable()
            .a11y(A11y::Toggle { on })
        }
        Kind::Menu { items } => {
            let shown: Vec<&ir::MenuItem> = items.iter().filter(|i| i.visible).collect();
            // Drawing an unknown semantic value must not edit the script.
            let at = shown
                .iter()
                .position(|i| f64::from(i.value) == v)
                .or((!shown.is_empty()).then_some(0));
            if can_edit && ui.get(id.as_str()).activated() && !shown.is_empty() {
                if wd.menu_cycle {
                    v = f64::from(shown[at.map_or(0, |a| (a + 1) % shown.len())].value);
                } else {
                    input.menu = if input.menu == Some(n) { None } else { Some(n) };
                }
            }
            let label = at.map(|a| shown[a].text.clone()).unwrap_or_default();
            match strip {
                Some(p) => stack![
                    block(w, h)
                        .radius(0)
                        .fill(picture(p, 0).unwrap_or(Fill::from(Role::Field))),
                    row![words(label)]
                        .align(Align::Center)
                        .pad((TIGHT * scale, 0.))
                        .w(w)
                        .h(h)
                ],
                None => row![
                    words(label).flex(1).min_w(0),
                    glyph(Icon::Down, TIGHT * 2. * scale, secondary())
                ]
                .align(Align::Center)
                .pad((TIGHT * scale, 0.))
                .fill(Role::Ink.alpha(0.08)),
            }
            .focusable()
            .a11y(A11y::Button)
        }
        Kind::ValueEdit {
            range,
            display,
            arrows,
        } => {
            let response = ui.get(id.as_str());
            if can_edit
                && (response.double_clicked
                    || ui.keys(id.as_str()).iter().any(|k| k.key == Key::Enter))
            {
                input.typing = Some((
                    n,
                    format!(
                        "{}",
                        v / if display.ratio == 0. {
                            1.
                        } else {
                            display.ratio
                        }
                    ),
                ));
            }
            if input.typing.as_ref().is_some_and(|(at, _)| *at == n) {
                let edit_id = format!("{id}-type");
                let mounted = ui.scene().is_some_and(|s| s.surface(&edit_id).is_some());
                let (_, text) = input.typing.as_mut().unwrap();
                let field = text_edit(
                    ui,
                    edit_id.as_str(),
                    text,
                    TextOpts {
                        blur_on_submit: true,
                        ..Default::default()
                    },
                );
                if !mounted {
                    ui.focus(edit_id.as_str());
                }
                let cancel = !can_edit
                    || ui
                        .keys(edit_id.as_str())
                        .iter()
                        .any(|k| k.key == Key::Escape);
                if cancel {
                    input.typing = None;
                } else if field.changed.submitted || mounted && !ui.focused(edit_id.as_str()) {
                    if let Ok(number) = text.trim().parse::<f64>() {
                        if number.is_finite() {
                            v = quantized(
                                number
                                    * if display.ratio == 0. {
                                        1.
                                    } else {
                                        display.ratio
                                    },
                                range,
                            );
                        }
                    }
                    input.typing = None;
                }
                row![field.el].align(Align::Center)
            } else {
                let (vertical, travel) = gesture(wd, scale);
                if can_edit {
                    drive_widget(
                        ui,
                        &id,
                        &mut v,
                        &(range.min..=range.max),
                        travel,
                        vertical,
                        range.default,
                        range.step,
                        false,
                    );
                }
                let mut parts = Vec::new();
                if !wd.hide.title && !wd.text.is_empty() {
                    parts.push(words(wd.text.clone()).fill(secondary()).flex(1).min_w(0));
                }
                if !wd.hide.value {
                    parts.push(super::render_art::words(
                        wd.value_text
                            .as_deref()
                            .filter(|t| !t.is_empty())
                            .unwrap_or(&number(v, display)),
                        style,
                        assets,
                        bitmap,
                        ink.clone(),
                        w,
                        h,
                        scale,
                        wd.value_y,
                        false,
                    ));
                }
                if *arrows {
                    let up = format!("{id}-up");
                    let down = format!("{id}-down");
                    if can_edit && ui.get(up.as_str()).activated() {
                        v = quantized(v + range.step.unwrap_or(1.), range);
                    }
                    if can_edit && ui.get(down.as_str()).activated() {
                        v = quantized(v - range.step.unwrap_or(1.), range);
                    }
                    parts.push(
                        col![
                            glyph(Icon::Up, TIGHT * scale, secondary())
                                .focusable()
                                .id(up),
                            glyph(Icon::Down, TIGHT * scale, secondary())
                                .focusable()
                                .id(down)
                        ]
                        .gap(0),
                    );
                }
                row(parts)
                    .gap(TIGHT * scale)
                    .align(Align::Center)
                    .justify(Justify::Center)
                    .pad((TIGHT * scale, 0.))
                    .fill(Role::Ink.alpha(0.06))
                    .cursor(Cursor::ResizeV)
                    .focusable()
                    .a11y(A11y::Slider {
                        value: v,
                        min: range.min,
                        max: range.max,
                    })
            }
        }
        Kind::Label => super::render_art::words(
            &wd.text,
            style,
            assets,
            bitmap,
            ink.clone(),
            w,
            h,
            scale,
            wd.text_y,
            true,
        ),
        Kind::LevelMeter { orientation } => {
            let level = input.meters.get(&n).copied().unwrap_or_else(|| {
                let (bus, channel) = match wd.binding {
                    Binding::Meter { bus, channel } => (bus, channel),
                    _ => (None, 0),
                };
                assets
                    .meter
                    .as_ref()
                    .map_or(0., |read| f64::from(read(bus, channel)[0]))
            });
            let [lo, hi] = wd.meter_range.unwrap_or([0, 1_000_000]);
            let unit = if lo == hi {
                0.
            } else {
                ((level * 1_000_000. - f64::from(lo)) / f64::from(hi - lo)).clamp(0., 1.)
            };
            let vertical = *orientation == ir::Orientation::Vertical;
            canvas(move |s| {
                let area = if vertical {
                    rect(0., s.height * (1. - unit), s.width, s.height * unit)
                } else {
                    rect(0., 0., s.width * unit, s.height)
                };
                vec![Draw::fill(area, signal())]
            })
            .fill(Role::Ink.alpha(0.12))
        }
        Kind::Table {
            columns,
            range,
            cells,
            bipolar,
            steps_shown,
        } if !wd.components.is_empty() => {
            let bars = (0..*columns as usize)
                .map(|c| {
                    let cid = wd.components.get(c).copied();
                    let mut value = cid
                        .and_then(|id| values.get(&id).copied())
                        .unwrap_or_else(|| cells.get(c).copied().unwrap_or(0.));
                    let key = format!("{id}-cell-{c}");
                    if can_edit {
                        drive(
                            ui,
                            &key,
                            &mut value,
                            &(range.min..=range.max),
                            TRAVEL,
                            true,
                            range.default,
                        );
                    }
                    if let Some(step) = range.step {
                        value = (value / step).round() * step;
                    }
                    if let Some(id) = cid {
                        values.insert(id, value);
                    }
                    super::render_art::table(vec![value], *range, *bipolar, *steps_shown, wd.colors)
                        .flex(1)
                        .h(h)
                        .id(key)
                        .focusable()
                        .named(format!("{} {}", wd.name, c + 1))
                        .a11y(A11y::Slider {
                            value,
                            min: range.min,
                            max: range.max,
                        })
                })
                .collect::<Vec<_>>();
            row(bars).gap(1).w(w).h(h)
        }
        Kind::Table {
            columns,
            range,
            cells,
            bipolar,
            steps_shown,
        } => {
            let mut samples = match input.values.get(&n).or(wd.value.as_ref()) {
                Some(ir::Value::Integers(v)) => v.iter().map(|v| f64::from(*v)).collect::<Vec<_>>(),
                Some(ir::Value::Reals(v)) => v.clone(),
                _ => cells.iter().map(|v| *v as f64).collect(),
            };
            samples.resize(*columns as usize, range.default);
            let response = ui.get(id.as_str());
            if !response.held {
                input.table_drags.remove(&n);
            }
            if can_edit && (response.pressed || response.dragged) && !samples.is_empty() {
                if let Some(point) = ui.local(id.as_str()) {
                    let column = (((point.x / w.max(1.)).clamp(0., 1.) * samples.len() as f64)
                        .floor() as usize)
                        .min(samples.len() - 1);
                    let raw = range.min
                        + (1. - point.y / h.max(1.)).clamp(0., 1.) * (range.max - range.min);
                    let (start, previous) = if response.pressed {
                        (column, raw)
                    } else {
                        input.table_drags.get(&n).copied().unwrap_or((column, raw))
                    };
                    let distance = start.abs_diff(column);
                    if distance < sampler_core::WIDGET_EDIT_CAPACITY {
                        for step in 0..=distance {
                            let at = if start <= column {
                                start + step
                            } else {
                                start - step
                            };
                            let value = quantized(
                                if distance == 0 {
                                    raw
                                } else {
                                    previous + (raw - previous) * step as f64 / distance as f64
                                },
                                range,
                            );
                            samples[at] = value;
                            let value = if range.step == Some(1.) {
                                ir::Value::Integer(value as i32)
                            } else {
                                ir::Value::Real(value)
                            };
                            input.edits.push(Edit {
                                widget: n,
                                index: at as u32,
                                value,
                                mods: response.mods,
                                mouse_over: response.hovered,
                                cursor: column as u32,
                                event: if response.dragged { 2 } else { 0 },
                            });
                        }
                        input.table_drags.insert(n, (column, raw));
                        input.values.insert(n, ir::Value::Reals(samples.clone()));
                    }
                }
            }
            super::render_art::table(samples, *range, *bipolar, *steps_shown, wd.colors)
                .cursor(Cursor::Crosshair)
                .focusable()
        }
        Kind::Xy { .. } if wd.components.len() == 2 => {
            let mut xy = [0.5; 2];
            let mut axes = Vec::new();
            for axis in 0..2 {
                let cid = wd.components[axis];
                let target = face
                    .widgets
                    .iter()
                    .find(|w| w.binding == Binding::Control(cid));
                let range = target
                    .and_then(|w| match w.kind {
                        Kind::Knob { range, .. }
                        | Kind::Slider { range, .. }
                        | Kind::ValueEdit { range, .. } => Some(range),
                        _ => None,
                    })
                    .unwrap_or(ir::Range {
                        min: 0.,
                        max: 1.,
                        default: 0.5,
                        step: None,
                    });
                let mut value = values.get(&cid).copied().unwrap_or(range.default);
                let mapper = target.and_then(|w| w.mapper.as_deref());
                let key = format!("{id}-axis-{axis}");
                if can_edit {
                    drive_mapped(ui, &key, &mut value, &range, mapper, false);
                    if ui.get(id.as_str()).held
                        && let Some(p) = ui.local(&id)
                    {
                        let t = if axis == 0 {
                            (p.x / w.max(1.)).clamp(0., 1.)
                        } else {
                            (1. - p.y / (h - 18.).max(1.)).clamp(0., 1.)
                        };
                        value = mapped(&range, mapper, t, false);
                    }
                }
                if let Some(step) = range.step {
                    value = (value / step).round() * step;
                }
                values.insert(cid, value);
                xy[axis] = mapped(&range, mapper, value, true);
                axes.push(
                    fader_face(xy[axis], 0., None, false, 0., ui.focus_visible(&key))
                        .h(16)
                        .flex(1)
                        .id(key)
                        .focusable()
                        .named(format!("{} {}", wd.name, if axis == 0 { "X" } else { "Y" }))
                        .a11y(A11y::Slider {
                            value,
                            min: range.min,
                            max: range.max,
                        }),
                );
            }
            let pad = canvas(move |s| {
                vec![Draw::fill(
                    rect(
                        xy[0] * (s.width - 6.),
                        (1. - xy[1]) * (s.height - 6.),
                        6.,
                        6.,
                    ),
                    Role::Ink.alpha(0.8),
                )]
            })
            .fill(Role::Ink.alpha(0.06))
            .w(w)
            .h((h - 18.).max(1.))
            .id(id.clone());
            col![pad, row(axes).gap(2).h(16)].gap(2)
        }
        Kind::Xy {
            cursors,
            sensitivity,
            mouse_mode,
        } => {
            let mut points = match input.values.get(&n).or(wd.value.as_ref()) {
                Some(ir::Value::Reals(v)) => v.clone(),
                _ => vec![0.; *cursors as usize * 2],
            };
            points.resize(*cursors as usize * 2, 0.);
            let response = ui.get(id.as_str());
            let mode = mouse_mode.unwrap_or(0);
            if can_edit && !points.is_empty() {
                if response.pressed
                    && let Some(point) = ui.local(id.as_str())
                {
                    let active = wd.active_index.unwrap_or(0);
                    let active = if active >= 0 && active % 2 == 0 {
                        (active as usize / 2).min(points.len() / 2 - 1)
                    } else {
                        0
                    };
                    let on_cursor = |cursor: usize| {
                        (point.x - points[cursor * 2] * w).abs() <= 6. * scale
                            && (point.y - (1. - points[cursor * 2 + 1]) * h).abs() <= 6. * scale
                    };
                    let cursor = if mode == 2 {
                        (0..points.len() / 2)
                            .rev()
                            .find(|&cursor| on_cursor(cursor))
                            .unwrap_or(active)
                    } else {
                        active
                    };
                    if mode != 0 || on_cursor(cursor) {
                        input.cursors.insert(n, cursor);
                        input.xy_drags.insert(
                            n,
                            (cursor, [points[cursor * 2], points[cursor * 2 + 1]], point),
                        );
                    } else {
                        input.xy_drags.remove(&n);
                    }
                }
                if (response.pressed || response.dragged || response.released)
                    && let Some((cursor, raw, last)) = input.xy_drags.get_mut(&n)
                {
                    if let Some(point) = ui.local(id.as_str()) {
                        if mode == 2 {
                            *raw = [
                                (point.x / w.max(1.)).clamp(0., 1.),
                                (1. - point.y / h.max(1.)).clamp(0., 1.),
                            ];
                        } else if response.dragged {
                            let fine = if response.mods.shift { FINE_DRAG } else { 1. };
                            for (axis, delta, size) in
                                [(0, point.x - last.x, w), (1, last.y - point.y, h)]
                            {
                                raw[axis] = (raw[axis]
                                    + delta / size.max(1.)
                                        * f64::from(sensitivity[axis].unwrap_or(1000))
                                        / 1000.
                                        * fine)
                                    .clamp(0., 1.);
                            }
                        }
                        *last = point;
                    }
                    for (index, value) in [(*cursor * 2, raw[0]), (*cursor * 2 + 1, raw[1])] {
                        points[index] = value;
                        input.edits.push(Edit {
                            widget: n,
                            index: index as u32,
                            value: ir::Value::Real(value),
                            mods: response.mods,
                            mouse_over: response.hovered,
                            cursor: (*cursor * 2) as u32,
                            event: if response.released {
                                1
                            } else if response.dragged {
                                2
                            } else {
                                0
                            },
                        });
                    }
                    input.values.insert(n, ir::Value::Reals(points.clone()));
                }
            }
            if !response.held {
                input.xy_drags.remove(&n);
            }
            canvas(move |s| {
                points
                    .chunks_exact(2)
                    .map(|point| {
                        Draw::fill(
                            rect(
                                point[0] * s.width - 3.,
                                (1. - point[1]) * s.height - 3.,
                                6.,
                                6.,
                            ),
                            value_ink(0.),
                        )
                    })
                    .collect()
            })
            .fill(Role::Ink.alpha(0.06))
            .cursor(Cursor::Crosshair)
            .focusable()
        }
        Kind::TextEdit => {
            let draft = input.drafts.entry(n).or_insert_with(|| {
                match input.values.get(&n).or(wd.value.as_ref()) {
                    Some(ir::Value::Text(v)) => v.clone(),
                    _ => String::new(),
                }
            });
            if !ui.focused(id.as_str()) {
                if let Some(ir::Value::Text(value)) = input.values.get(&n).or(wd.value.as_ref()) {
                    draft.clone_from(value);
                }
            }
            let field = text_edit(
                ui,
                id.as_str(),
                draft,
                TextOpts {
                    blur_on_submit: true,
                    ..Default::default()
                },
            );
            if can_edit && field.changed.submitted {
                let text = ir::Value::Text(draft.clone());
                input.edits.push(Edit {
                    widget: n,
                    index: 0,
                    value: text.clone(),
                    mods: ui.get(id.as_str()).mods,
                    mouse_over: false,
                    cursor: 0,
                    event: 1,
                });
                input.values.insert(n, text);
            }
            field.el
        }
        Kind::FileSelector {
            base_path, files, ..
        } => {
            let directory = base_path.as_deref().unwrap_or(".");
            let (path, entries) = input.files.entry(n).or_insert_with(|| {
                let path = std::path::PathBuf::from(directory);
                // ponytail: one directory read per navigation; move to a worker if large folders stall.
                let mut entries = std::fs::read_dir(&path)
                    .into_iter()
                    .flatten()
                    .filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| p.is_dir() || file_matches(p, *files))
                    .collect::<Vec<_>>();
                entries.sort();
                (path, entries)
            });
            let mut rows = Vec::new();
            let up = format!("{id}-parent");
            let mut next = None;
            if path.parent().is_some() {
                if can_edit && ui.get(up.as_str()).activated() {
                    next = path.parent().map(std::path::Path::to_owned);
                }
                rows.push(caption("..").pad(TIGHT * scale).focusable().id(up));
            }
            for (at, path) in entries.iter().enumerate() {
                let item = format!("{id}-file-{at}");
                if can_edit && ui.get(item.as_str()).activated() {
                    if path.is_dir() {
                        next = Some(path.clone());
                    } else {
                        let value = ir::Value::Text(path.to_string_lossy().into_owned());
                        input.edits.push(Edit {
                            widget: n,
                            index: 0,
                            value: value.clone(),
                            mods: ui.get(item.as_str()).mods,
                            mouse_over: false,
                            cursor: 0,
                            event: 0,
                        });
                        input.values.insert(n, value);
                    }
                }
                rows.push(
                    caption(
                        path.file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned(),
                    )
                    .pad(TIGHT * scale)
                    .focusable()
                    .id(item),
                );
            }
            if let Some(next) = next {
                let mut entries = std::fs::read_dir(&next)
                    .into_iter()
                    .flatten()
                    .filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| p.is_dir() || file_matches(p, *files))
                    .collect::<Vec<_>>();
                entries.sort();
                input.files.insert(n, (next, entries));
            }
            col(rows).gap(0).scroll().fill(Role::Ink.alpha(0.06))
        }
        Kind::Waveform | Kind::Wavetable { .. } => super::render_art::waveform(
            input.peaks.get(&n).cloned().unwrap_or_default(),
            wd.waveform.clone(),
            input.wave_duration_us.get(&n).copied(),
            wd.colors,
            wd.hide.background,
        ),
        Kind::MouseArea => {
            let response = ui.get(id.as_str());
            if can_edit && (response.pressed || response.released) {
                let value = match input.values.get(&n).or(wd.value.as_ref()) {
                    Some(ir::Value::Integer(value)) => *value,
                    Some(ir::Value::Integers(values)) => {
                        values.first().copied().unwrap_or(v.round() as i32)
                    }
                    _ => v.round() as i32,
                };
                input.edits.push(Edit {
                    widget: n,
                    index: 0,
                    value: ir::Value::Integer(value),
                    mods: response.mods,
                    mouse_over: response.hovered,
                    cursor: 0,
                    event: if response.released { 1 } else { 0 },
                });
            }
            block(w, h)
        }
        Kind::Panel | Kind::Image => block(w, h),
    };
    if let Some(c) = control
        && wd.components.is_empty()
    {
        if can_edit && v.to_bits() != before.to_bits() {
            let response = ui.get(id.as_str());
            let mods = if matches!(wd.kind, Kind::Menu { .. } | Kind::ValueEdit { .. }) {
                Mods::default()
            } else {
                response.mods
            };
            let value = match &wd.kind {
                Kind::Knob { range, .. }
                | Kind::Slider { range, .. }
                | Kind::ValueEdit { range, .. }
                    if range.step != Some(1.) =>
                {
                    ir::Value::Real(v)
                }
                _ => ir::Value::Integer(v.round() as i32),
            };
            input.edits.push(Edit {
                widget: n,
                index: 0,
                value,
                mods,
                mouse_over: false,
                cursor: 0,
                event: if response.dragged { 2 } else { 0 },
            });
        }
        values.insert(c, v);
    }
    // Our faces are light-on-dark: a control without its own picture sits on
    // a dark plate (Kontakt's stock controls are dark too), in either mode,
    // wherever the art under it is light.
    let plate = strip.is_none()
        && !matches!(
            wd.kind,
            Kind::Label | Kind::Panel | Kind::Image | Kind::MouseArea
        )
        && light_under(face, assets, n);
    let face_el = if let Some(c) = wd.colors.background {
        face_el.fill(colour(c))
    } else {
        face_el
    };
    let face_el = if plate {
        face_el
            .radius(2)
            .fill(Color::oklch(0.2, 0., 0.).with_alpha(0.85))
    } else {
        face_el
    };
    let face_el = if can_edit {
        face_el
    } else {
        face_el.disabled()
    };
    let interactive = matches!(
        wd.kind,
        Kind::Knob { .. }
            | Kind::Slider { .. }
            | Kind::Button { .. }
            | Kind::Switch
            | Kind::Menu { .. }
            | Kind::ValueEdit { .. }
            | Kind::Table { .. }
            | Kind::Xy { .. }
            | Kind::FileSelector { .. }
            | Kind::TextEdit
            | Kind::MouseArea
    );
    // MUI reserves slash-prefixed IDs for non-target decoration.
    let target = if interactive { id } else { format!("/{id}") };
    let mut el = face_el
        .w(w)
        .h(h)
        .shrink(0)
        .opacity(opacity)
        .id(target)
        .named(
            wd.automation
                .name
                .clone()
                .unwrap_or_else(|| wd.name.clone()),
        );
    if !wd.tooltip.is_empty() {
        el = el.tip(wd.tooltip.clone());
    }
    let bg = wd.images.iter().find(|i| i.role == Use::Background);
    match bg.and_then(|i| {
        assets
            .get(i.asset)
            .and_then(|p| picture(p, i.frame.unwrap_or(0) as usize))
    }) {
        Some(_) if !wd.hide.background => {
            let i = bg.unwrap();
            let p = assets.get(i.asset).unwrap();
            stack![
                art(face, i.asset, p, i.frame.unwrap_or(0) as usize, w, h, scale),
                el
            ]
            .w(w)
            .h(h)
            .shrink(0)
        }
        _ => el,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn menu_popup(
    ui: &mut Ui,
    namespace: &str,
    face: &Interface,
    scale: f64,
    values: &mut Values,
    input: &mut InputState,
    width: f64,
    height: f64,
) -> Option<El> {
    let n = input.menu?;
    let wd = face.widgets.get(n.0)?;
    let Kind::Menu { items } = &wd.kind else {
        input.menu = None;
        return None;
    };
    let anchor = target(namespace, n);
    let popup = format!("{anchor}-popup");
    if !wd.enabled || !face.visible(n) || ui.dismissed(&[popup.as_str(), anchor.as_str()]) {
        input.menu = None;
        return None;
    }
    let mut rows = Vec::new();
    for (at, item) in items.iter().enumerate().filter(|(_, item)| item.visible) {
        let id = format!("{anchor}-item-{at}");
        if ui.get(id.as_str()).activated() {
            if let Binding::Control(control) = wd.binding {
                values.insert(control, f64::from(item.value));
                input.edits.push(Edit {
                    widget: n,
                    index: 0,
                    value: ir::Value::Integer(item.value),
                    mods: Mods::default(),
                    mouse_over: false,
                    cursor: 0,
                    event: 0,
                });
            }
            input.menu = None;
        }
        rows.push(
            caption(item.text.clone())
                .pad((TIGHT * scale, SPACE * scale))
                .fill(Role::Ink)
                .focusable()
                .a11y(A11y::Button)
                .id(id),
        );
    }
    let r = face.page_rect(n);
    let root = if namespace.is_empty() {
        "ir-view".to_owned()
    } else {
        format!("{namespace}-ir-view")
    };
    let positioned = ui.scene().and_then(|scene| {
        let anchor = scene.surface(&anchor)?.frame;
        let root = scene.surface(&root)?.frame;
        Some((
            anchor.x - root.x,
            anchor.y - root.y,
            anchor.size.width,
            anchor.size.height,
        ))
    });
    let (ax, ay, aw, ah) = positioned.unwrap_or((
        f64::from(r.x) * scale,
        f64::from(r.y) * scale,
        f64::from(r.width) * scale,
        f64::from(r.height) * scale,
    ));
    let w = aw.max(120.).min(width);
    let h = (rows.len() as f64 * CONTROL * scale).min(height);
    let x = ax.clamp(0., (width - w).max(0.));
    let y = (ay + ah).clamp(0., (height - h).max(0.));
    ui.capture_popup_wheel(popup.clone());
    Some(
        col(rows)
            .gap(0)
            .w(w)
            .h(h)
            .scroll()
            .fill(Role::Field)
            .stroke(Role::Ink.alpha(0.3))
            .stroke_width(1)
            .id(popup)
            .at(x, y),
    )
}

/// Detect the owning target before validating a payload, so an invalid batch
/// cannot fall through into a rack/instrument drop.
pub(super) fn file_drop_target(
    ui: &Ui,
    namespace: &str,
    face: &Interface,
    at: Point,
) -> Option<WidgetRef> {
    let mut hit = moose::mui::mui::input::Hit::default();
    for surface in ui
        .scene()?
        .surfaces()
        .filter(|s| (Id::is_named(&s.key) || s.pointer_states) && !s.disabled)
    {
        if surface.hits.is_empty() {
            hit.push_placed(
                surface.key.clone(),
                None,
                &surface.path,
                surface.offset,
                surface.clip,
                surface.clip_paths(),
            )
            .ok()?;
        }
        for (tag, path) in &surface.hits {
            hit.push_placed(
                surface.key.clone(),
                Some(tag.clone()),
                path,
                surface.offset,
                surface.clip,
                surface.clip_paths(),
            )
            .ok()?;
        }
    }
    let winner = hit.at(at)?;
    face.widgets.iter().enumerate().find_map(|(n, w)| {
        let n = WidgetRef(n);
        (matches!(w.kind, Kind::MouseArea)
            && face.visible(n)
            && w.enabled
            && w.intercepts_mouse
            && target(namespace, n) == winner)
            .then_some(n)
    })
}

/// One OS gesture; forward all returned edits in one admission transaction.
pub(super) fn file_drop(
    ui: &Ui,
    namespace: &str,
    face: &Interface,
    at: Point,
    paths: &[std::path::PathBuf],
    dropped: bool,
) -> Option<(WidgetRef, Vec<Edit>)> {
    if paths.is_empty() {
        return None;
    }
    let n = file_drop_target(ui, namespace, face, at)?;
    let mut counts = [0u32; 3];
    let mut edits = Vec::with_capacity(paths.len().min(96));
    for (index, path) in paths.iter().enumerate() {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        let (kind, slot) = match extension.as_str() {
            "wav" | "aif" | "aiff" | "ncw" => (ir::DropKind::Audio, 0),
            "mid" | "midi" => (ir::DropKind::Midi, 1),
            "nka" => (ir::DropKind::Array, 2),
            _ => return None,
        };
        counts[slot] += 1;
        if counts[slot] > sampler_core::WIDGET_DROP_CAPACITY {
            return None;
        }
        let path = path.to_str()?;
        if sampler_core::Text::try_new(path).is_err() {
            return None;
        }
        edits.push(Edit {
            widget: n,
            index: index.try_into().ok()?,
            value: ir::Value::DropPath {
                kind,
                path: path.into(),
            },
            mods: ui.pointer().mods,
            cursor: 0,
            event: if dropped { 5 } else { 4 },
            mouse_over: true,
        });
    }
    Some((n, edits))
}

fn file_matches(path: &std::path::Path, files: ir::Files) -> bool {
    let extension = path
        .extension()
        .and_then(|x| x.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match files {
        ir::Files::Any => true,
        ir::Files::Audio => matches!(
            extension.as_str(),
            "wav" | "aif" | "aiff" | "flac" | "ogg" | "mp3" | "ncw"
        ),
        ir::Files::Midi => matches!(extension.as_str(), "mid" | "midi"),
        ir::Files::Data => matches!(extension.as_str(), "nka" | "nkr" | "txt"),
    }
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod render_tests;
/// Exercise the production asset loader, layout and CPU painter without a window.
#[cfg(feature = "shots")]
pub fn uvi_ui_health(face: &Interface, path: &std::path::Path) -> serde_json::Value {
    let mut source = super::pictures::Source::of(path);
    let mut assets = Assets::default();
    let (mut image_errors, mut font_errors) = (0, 0);
    assets.sync(face, Presentation::Bitmap, |asset| {
        if !matches!(asset.kind, ir::AssetKind::Image(_)) {
            return None;
        }
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
        let Some(page) = face.pages.first() else {
            return Err("no UI page".into());
        };
        let scale = (1100. / page.size.width.max(1.)).min(1.);
        let (width, height) = (
            (f64::from(page.size.width) * scale).ceil().clamp(1., 1100.) as u16,
            (f64::from(page.size.height) * scale)
                .ceil()
                .clamp(1., 4096.) as u16,
        );
        let mut values = Values::default();
        let mut ui = super::theme::ui();
        let face = resolved(face);
        for _ in 0..2 {
            let root = view(
                &mut ui,
                &face,
                PageRef(0),
                &assets,
                Presentation::Bitmap,
                scale,
                &mut values,
            );
            ui.frame(
                root,
                Some(Size::new(width.into(), height.into())),
                Input::default(),
                1. / 60.,
            )
            .map_err(|e| e.to_string())?;
        }
        use moose::mui::mui::vello::{
            self,
            vello_cpu::{Pixmap, RenderContext, Resources},
        };
        let mut ctx = RenderContext::new(width, height);
        let mut resources = Resources::default();
        vello::paint(
            &mut vello::Cpu {
                ctx: &mut ctx,
                resources: &mut resources,
                cache: &mut vello::Cache::default(),
            },
            ui.scene().ok_or("no scene")?,
            vello::kurbo::Affine::IDENTITY,
        )
        .map_err(|e| e.to_string())?;
        ctx.flush();
        ctx.render(&mut Pixmap::new(width, height), &mut resources);
        Ok(())
    };
    // Never log a panic payload: library text/resources may be embedded in it.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(render));
    let render_error = match result {
        Ok(Ok(())) => None,
        Ok(Err(e)) => Some(e),
        Err(_) => Some("render panic".into()),
    };
    serde_json::json!({"image_errors":image_errors,"font_errors":font_errors,"render_error":render_error})
}

#[cfg(test)]
mod mapper_tests {
    use super::*;
    #[test]
    fn uvi_mapper_positions_round_trip_and_select_the_expected_strip_frame() {
        let range = ir::Range {
            min: 1.,
            max: 10000.,
            default: 100.,
            step: None,
        };
        assert!((mapped(&range, Some("Exponential"), 0.5, false) - 100.).abs() < 1e-9);
        assert_eq!(
            frame(mapped(&range, Some("Exponential"), 100., true), 0., 1., 101),
            50
        );
        for mapper in [
            "Linear",
            "Exponential",
            "Quadratic",
            "Cubic",
            "Quartic",
            "Quintic",
            "SquareRoot",
            "CubeRoot",
            "QuarticRoot",
            "QuinticRoot",
        ] {
            let value = mapped(&range, Some(mapper), 0.25, false);
            assert!(
                (mapped(&range, Some(mapper), value, true) - 0.25).abs() < 1e-9,
                "{mapper}"
            );
        }
    }
}
