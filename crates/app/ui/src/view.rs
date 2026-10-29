//! The Original view: one page of a tree, drawn as its source drew it.
//!
//! The page colour, the wallpaper from the skin offset down, then every
//! shown widget in draw order at its page position, scaled. Each widget is
//! a memo keyed by what it draws, so a frame where nothing moved rebuilds
//! nothing and a script write rebuilds the widgets it touched only.

use std::sync::Arc;

use kontra_ui_model::{
    self as model, ImageUse, Interface, Kind, PageRef, Role, kontakt_picture_frame,
};
use mui::prelude::*;

use crate::Pictures;

/// Text size where the source sets none, in source pixels.
const TEXT: f64 = 11.;

/// `page` of `tree` at `scale` view points per source pixel, and how many
/// widgets this frame built rather than kept. `face` scopes its ids: each
/// face drawn in one frame (a rack part) needs its own. `level` (0..1, the
/// part's output peak) lights its level meters.
pub fn page(
    ui: &mut Ui,
    face: usize,
    tree: &Interface,
    page: PageRef,
    pictures: &mut Pictures,
    (scale, level): (f64, f64),
) -> (El, usize) {
    let Some(p) = tree.pages.get(page.0) else {
        return (block(0., 0.), 0);
    };
    let mut built = 0;
    let (w, h) = (p.size.width * scale, p.size.height * scale);
    // Pictures decode at the pixels they cover on the device.
    let device = scale * ui.scale().unwrap_or(1.);
    let mut layers = vec![
        block(w, h)
            .radius(0.)
            .fill(
                p.background
                    .color
                    .map_or(Fill::from(Color::srgb(0., 0., 0.)), |c| {
                        Fill::from(color(c))
                    }),
            )
            .at(0., 0.),
    ];
    if let Some((a, (path, meta))) = p
        .background
        .image
        .and_then(|a| Some((a, image_asset(tree, a)?)))
    {
        let b = &p.background;
        let skin = (b.frame, b.origin_y, b.offset_y.max(0));
        for (n, y) in wallpaper(skin, meta.size.map_or(0., |s| s.height), meta.frames) {
            let Some(img) = pictures.frame(a, path, meta, n, device) else {
                continue;
            };
            // Its own size, whatever pixels it decoded at.
            let size = meta.size.unwrap_or(model::Size {
                width: f64::from(img.width),
                height: f64::from(img.height),
            });
            layers.push(image(&img, size.width * scale, size.height * scale).at(0., y * scale));
        }
    }
    let on_page = |r: model::Rect| {
        r.width > 0.
            && r.height > 0.
            && r.x < p.size.width
            && r.y < p.size.height
            && r.x + r.width > 0.
            && r.y + r.height > 0.
    };
    for n in tree.draw_order(page) {
        let r = tree.page_rect(n);
        if !tree.visible(n) || !on_page(r) {
            continue;
        }
        let Some(wd) = tree.widgets.get(n.0) else {
            continue;
        };
        let picture = wd.images.first().and_then(|u| {
            let (path, meta) = image_asset(tree, u.asset)?;
            let f = frame(wd, u, meta.frames as usize);
            Some((pictures.frame(u.asset, path, meta, f, device)?, meta))
        });
        let art = picture.as_ref().map(|(_, m)| drawn(r, *m));
        let style = wd.style.and_then(|s| tree.styles.get(s.0));
        let stock = stock::Look::of(tree, wd, value(wd));
        let words = if stock.is_some() { "" } else { caption(wd) };
        let lit = match wd.kind {
            Kind::LevelMeter {
                orientation,
                channel: Some(_),
            } => Some((orientation, lit(level))),
            _ => None,
        };
        let deps = (
            lit.map(|(o, t)| (o as u8, t.to_bits())),
            [r.x, r.y, r.width, r.height, scale].map(f64::to_bits),
            art.map(|a| [a.width, a.height].map(f64::to_bits)),
            picture.as_ref().map(|(i, _)| Arc::as_ptr(i) as usize),
            words,
            wd.text_y,
            style.map(|s| (s.color, s.align as u8, s.size.map(f32::to_bits))),
            wd.opacity.to_bits(),
            &stock,
        );
        let el = ui.memo(format!("kontra-ui-{face}-{}", n.0), deps, |_| {
            built += 1;
            let (w, h) = (r.width * scale, r.height * scale);
            let mut parts = Vec::new();
            if let (Some((img, _)), Some(a)) = (&picture, art) {
                parts.push(image(img, a.width * scale, a.height * scale).at(0., 0.));
            }
            if !words.is_empty() {
                parts.push(words_el(words, style, wd, w, h, scale).at(0., 0.));
            }
            if let Some((o, t)) = lit {
                parts.push(meter(&wd.colors, o, t, w, h).at(0., 0.));
            }
            let (w, h) = stock.as_ref().map_or((w, h), |look| {
                parts.extend(look.draw(wd, style, (w, h, scale)));
                look.size(w, h, scale)
            });
            stack(parts).w(w).h(h).opacity(wd.opacity)
        });
        // A control takes the pointer under its own id (crate::input).
        let el = if crate::input::takes_input(wd) {
            el.id(crate::input::surface(face, n.0))
                .focusable()
                .cursor(Cursor::Hand)
                .named(wd.name.clone())
        } else {
            el
        };
        layers.push(el.at(r.x * scale, r.y * scale));
    }
    (stack(layers).w(w).h(h).radius(0.).clip(), built)
}

/// How much of a level meter a peak lights: −60 dB to 0 dB, in steps of a
/// hundredth so a frame redraws only when the bar moves.
// ponytail: the scale is ours (Kontakt's meter law is unmeasured), and both
// channels show the part's peak; per-channel and per-bus peaks when a
// library needs them apart.
fn lit(peak: f64) -> f64 {
    let db = 20. * peak.max(1e-9).log10();
    ((db + 60.) / 60.).clamp(0., 1.).mul_add(100., 0.).round() / 100.
}

/// A level meter lit to `t`: its OFF colour behind, its ON colour lit,
/// from the bottom when vertical, from the left otherwise.
fn meter(c: &model::Colors, o: model::Orientation, t: f64, w: f64, h: f64) -> El {
    let off = c
        .off
        .or(c.background)
        .map_or(Color::srgb(0.1, 0.1, 0.1), color);
    let on = c.on.or(c.bar).map_or(Color::srgb(0.45, 0.8, 0.45), color);
    let bar = match o {
        model::Orientation::Vertical => block(w, h * t)
            .radius(0.)
            .fill(Fill::from(on))
            .at(0., h * (1. - t)),
        model::Orientation::Horizontal => {
            block(w * t, h).radius(0.).fill(Fill::from(on)).at(0., 0.)
        }
    };
    stack(vec![
        block(w, h).radius(0.).fill(Fill::from(off)).at(0., 0.),
        bar,
    ])
    .w(w)
    .h(h)
}

/// The wallpaper frames a page shows and where each one's top sits on it.
/// The page starts below the header (`origin`) and the skin offset, which
/// run through the file across its frames of height `tall`
/// (`set_skin_offset`), so the page may straddle two frames.
fn wallpaper(
    (frame, origin, offset): (u32, u32, i32),
    tall: f64,
    frames: u32,
) -> Vec<(usize, f64)> {
    let row = f64::from(frame) * tall + f64::from(origin) + f64::from(offset);
    let first = if tall > 0. {
        (row / tall).floor() as usize
    } else {
        0
    };
    (first..first + 2)
        .filter(|&n| n == first || n < frames as usize)
        .map(|n| (n, n as f64 * tall - row))
        .collect()
}

/// The frame `w` shows of a picture with `frames`: the script's
/// `PICTURE_STATE`, else the one its value selects.
fn frame(w: &model::Widget, u: &ImageUse, frames: usize) -> usize {
    if let Some(f) = u.frame {
        return f as usize;
    }
    let value = value(w);
    match (&w.kind, u.role) {
        (
            Kind::Knob { range, .. } | Kind::Slider { range, .. } | Kind::ValueEdit { range, .. },
            Role::Strip,
        ) => {
            let span = range.max - range.min;
            let t = if span == 0. {
                0.
            } else {
                ((value - range.min) / span).clamp(0., 1.)
            };
            (t * frames.saturating_sub(1) as f64).round() as usize
        }
        (Kind::Button { .. } | Kind::Switch, Role::Strip) => {
            kontakt_picture_frame(value != 0., false, false, frames)
        }
        _ => 0,
    }
}

/// The value `w` shows: its own, else the one it starts at.
fn value(w: &model::Widget) -> f64 {
    match w.value {
        Some(model::Value::Integer(v)) => f64::from(v),
        Some(model::Value::Real(v)) => v,
        _ => w.initial_value,
    }
}

/// A picture takes its frame's own size along each axis it does not
/// stretch, whatever size the widget has (a label's text keeps its bounds).
fn drawn(r: model::Rect, m: model::ImageMeta) -> model::Size {
    let own = m.size.unwrap_or(model::Size {
        width: r.width,
        height: r.height,
    });
    model::Size {
        width: if m.stretch[0] { r.width } else { own.width },
        height: if m.stretch[1] { r.height } else { own.height },
    }
}

/// An image asset's path and layout.
fn image_asset(tree: &Interface, a: model::AssetRef) -> Option<(&str, model::ImageMeta)> {
    let asset = tree.assets.get(a.0)?;
    match asset.kind {
        model::AssetKind::Image(m) => Some((asset.path.as_str(), m)),
        _ => None,
    }
}

/// The words a widget shows: a label's or button's text, a menu's item.
fn caption(w: &model::Widget) -> &str {
    if w.hide.title || w.label_in_image() {
        return "";
    }
    match &w.kind {
        Kind::Label | Kind::Button { .. } | Kind::Switch | Kind::TextEdit => w.text.as_str(),
        Kind::Menu { items } => {
            let at = match w.value {
                Some(model::Value::Integer(v)) => v,
                _ => w.initial_value as i32,
            };
            items
                .iter()
                .find(|i| i.value == at)
                .map_or("", |i| i.text.as_str())
        }
        _ => "",
    }
}

fn words_el(
    words: &str,
    style: Option<&model::TextStyle>,
    w: &model::Widget,
    width: f64,
    height: f64,
    scale: f64,
) -> El {
    let size = style.and_then(|s| s.size).map_or(TEXT, f64::from) * scale;
    let ink = style.map_or(Color::srgb(1., 1., 1.), |s| color(s.color));
    let align = style.map_or(
        if matches!(w.kind, Kind::Label) {
            model::Align::Left
        } else {
            model::Align::Center
        },
        |s| s.align,
    );
    let line = size * 1.25;
    let top = w
        .text_y
        .map_or(((height - line) / 2.).max(0.), |y| f64::from(y) * scale);
    let el = row([text(words.to_owned())
        .text_size(size)
        .fill(Fill::from(ink))
        .lines(1)
        .min_w(0.)])
    .align(Align::Center)
    .justify(match align {
        model::Align::Left => Justify::Start,
        model::Align::Center => Justify::Center,
        model::Align::Right => Justify::End,
    })
    .w(width)
    .h(line)
    .pad((2. * scale, 0.))
    .at(0., top);
    stack([el]).w(width).h(height).radius(0.).clip()
}

fn image(img: &Arc<mui::prelude::Image>, w: f64, h: f64) -> El {
    block(w, h)
        .radius(0.)
        .fill(Fill::Image(img.clone(), Fit::Fill))
}

fn color(c: model::Rgba) -> Color {
    Color::srgba(
        f32::from(c.r) / 255.,
        f32::from(c.g) / 255.,
        f32::from(c.b) / 255.,
        f32::from(c.a) / 255.,
    )
}

mod stock;
#[cfg(test)]
mod tests;
