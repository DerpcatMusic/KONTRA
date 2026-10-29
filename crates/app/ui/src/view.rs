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

mod stock;

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
    at: (f64, f64),
) -> (El, usize) {
    drawn_page(ui, face, tree, page, pictures, at, false)
}

/// [`page`] without its pictures: each control where it sits, in its
/// stock look, on the page's colour.
pub fn vector_page(
    ui: &mut Ui,
    face: usize,
    tree: &Interface,
    page: PageRef,
    pictures: &mut Pictures,
    at: (f64, f64),
) -> (El, usize) {
    drawn_page(ui, face, tree, page, pictures, at, true)
}

fn drawn_page(
    ui: &mut Ui,
    face: usize,
    tree: &Interface,
    page: PageRef,
    pictures: &mut Pictures,
    (scale, level): (f64, f64),
    bare: bool,
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
        .filter(|_| !bare)
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
        let picture = wd.images.first().filter(|_| !bare).and_then(|u| {
            let (path, meta) = image_asset(tree, u.asset)?;
            let f = frame(wd, u, meta.frames as usize);
            Some((pictures.frame(u.asset, path, meta, f, device)?, meta))
        });
        let art = picture.as_ref().map(|(_, m)| drawn(r, *m));
        let style = wd.style.and_then(|s| tree.styles.get(s.0));
        let t = position(wd);
        let look = stock::Look::of(tree, wd, t, readout(wd, tree.source), knob_name(wd), bare);
        let stock = look.is_none() && stock::shows(tree, wd, bare);
        let words = if look.is_some() {
            String::new()
        } else {
            caption(wd, tree.source)
        };
        let glyphs = match style.map(|s| &s.font) {
            Some(&model::Font::Bitmap(a)) if !words.is_empty() => {
                (tree.assets.get(a.0)).and_then(|asset| pictures.font(a, &asset.path))
            }
            _ => None,
        };
        let outline = match style.map(|s| &s.font) {
            Some(&model::Font::File(a)) if !words.is_empty() => tree
                .assets
                .get(a.0)
                .and_then(|asset| pictures.outline_font(a, &asset.path)),
            _ => None,
        };
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
            (
                &words,
                &look,
                stock.then_some(t.to_bits()),
                match &wd.value {
                    Some(model::Value::Reals(v)) => {
                        Some(v.iter().map(|v| v.to_bits()).collect::<Vec<_>>())
                    }
                    Some(model::Value::Integers(v)) => {
                        Some(v.iter().map(|&v| f64::from(v).to_bits()).collect())
                    }
                    _ => None,
                },
                wd.colors.background,
                wd.colors.bar,
                wd.table_inner_edge.map(|edge| edge.visible),
                wd.hide.background,
                bare,
            ),
            glyphs
                .as_ref()
                .map(|g| Arc::as_ptr(g).cast::<()>() as usize),
            outline.as_ref().map(Font::id),
            wd.text_y,
            style.map(|s| (s.color, s.align as u8, s.size.map(f32::to_bits))),
            wd.opacity.to_bits(),
        );
        let el = ui.memo(format!("kontra-ui-{face}-{}", n.0), deps, |_| {
            built += 1;
            let (w, h) = (r.width * scale, r.height * scale);
            let mut parts = Vec::new();
            if let (Some((img, meta)), Some(a)) = (&picture, art) {
                parts
                    .push(sliced(img, meta, (a.width * scale, a.height * scale), scale).at(0., 0.));
            } else if stock {
                parts.push(
                    stock::face(
                        wd,
                        t,
                        (w, h),
                        bare,
                        matches!(tree.source, model::Source::FalconLua),
                    )
                    .at(0., 0.),
                );
            }
            if let Some(glyphs) = &glyphs {
                parts.push(bitmap_words(&words, style, wd, (w, h), scale, glyphs).at(0., 0.));
            } else if !words.is_empty() {
                parts.push(
                    words_el(
                        &words,
                        style,
                        wd,
                        (w, h),
                        scale,
                        wd.text_y,
                        outline.as_ref(),
                    )
                    .at(0., 0.),
                );
            }
            if let Some((o, t)) = lit {
                parts.push(meter(&wd.colors, o, t, w, h).at(0., 0.));
            }
            let (w, h) = look.as_ref().map_or((w, h), |look| {
                parts.extend(look.draw(wd, style, (w, h, scale), bare));
                look.size(w, h, scale)
            });
            stack(parts).w(w).h(h).opacity(wd.opacity)
        });
        // A control takes the pointer under its own id (crate::input), and
        // hovering it shows its help text (`set_control_help`).
        let el = if crate::input::takes_input(wd) {
            let el = el
                .id(crate::input::surface(face, n.0))
                .focusable()
                .cursor(Cursor::Hand)
                .named(wd.name.clone());
            if wd.tooltip.is_empty() {
                el
            } else {
                el.tip(wd.tooltip.clone())
            }
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
    match (&w.kind, u.role) {
        (Kind::Knob { .. } | Kind::Slider { .. } | Kind::ValueEdit { .. }, Role::Strip) => {
            (position(w) * frames.saturating_sub(1) as f64).round() as usize
        }
        (Kind::Button { .. } | Kind::Switch, Role::Strip) => {
            kontakt_picture_frame(value(w) != 0., false, false, frames)
        }
        _ => 0,
    }
}

/// `w`'s value, or its initial one before the script sets it.
fn value(w: &model::Widget) -> f64 {
    match w.value {
        Some(model::Value::Integer(v)) => f64::from(v),
        Some(model::Value::Real(v)) => v,
        _ => w.initial_value,
    }
}

/// Where `w`'s value lies in its range, 0..1; a switch is 0 or 1.
fn position(w: &model::Widget) -> f64 {
    match &w.kind {
        Kind::Knob { range, .. } | Kind::Slider { range, .. } | Kind::ValueEdit { range, .. } => {
            let span = range.max - range.min;
            if span == 0. {
                0.
            } else {
                ((value(w) - range.min) / span).clamp(0., 1.)
            }
        }
        _ => f64::from(u8::from(value(w) != 0.)),
    }
}

/// `w`'s value as its source shows it: the script's label, else the value
/// over the display ratio with its unit.
pub(crate) fn readout(w: &model::Widget, source: model::Source) -> String {
    if w.hide.value {
        return String::new();
    }
    if let Some(t) = &w.value_text {
        return t.clone();
    }
    match &w.kind {
        // Ratio 0 ($VALUE_EDIT_MODE_NOTE_NAMES) names the note.
        Kind::ValueEdit { display, .. } if display.ratio == 0. => {
            crate::keyboard::note_name(value(w).clamp(0., 127.) as u8)
        }
        Kind::Knob { display, .. } | Kind::ValueEdit { display, .. } => {
            display.format_value(value(w), true, source)
        }
        _ => String::new(),
    }
}

/// A stock knob's name: its text, else its variable's.
pub(crate) fn knob_name(w: &model::Widget) -> String {
    if w.hide.title {
        return String::new();
    }
    if w.text.is_empty() {
        w.name.trim_start_matches(['$', '~']).to_owned()
    } else {
        w.text.clone()
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

/// The words a widget shows: a label's or button's text, a menu's item,
/// a value edit's title and value.
fn caption(w: &model::Widget, source: model::Source) -> String {
    if w.label_in_image() {
        return String::new();
    }
    if let Kind::ValueEdit { .. } = w.kind {
        let title = if w.hide.title { "" } else { w.text.trim() };
        return format!("{title} {}", readout(w, source)).trim().to_owned();
    }
    if w.hide.title {
        return String::new();
    }
    match &w.kind {
        Kind::Label | Kind::Button { .. } | Kind::Switch | Kind::TextEdit => w.text.clone(),
        Kind::Menu { items } => {
            let at = match w.value {
                Some(model::Value::Integer(v)) => v,
                _ => w.initial_value as i32,
            };
            items
                .iter()
                .find(|i| i.value == at)
                .map_or(String::new(), |i| i.text.clone())
        }
        _ => String::new(),
    }
}

fn words_el(
    words: &str,
    style: Option<&model::TextStyle>,
    w: &model::Widget,
    (width, height): (f64, f64),
    scale: f64,
    text_y: Option<i32>,
    font: Option<&Font>,
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
    // Kontakt sets a label's words 7 px in from its side (oracle: Areia
    // 6.7, Pacific 7.7 px at 150%).
    let inset = if matches!(w.kind, Kind::Label) {
        7.
    } else {
        2.
    };
    let line = size * 1.25;
    let top = text_y.map_or(((height - line) / 2.).max(0.), |y| f64::from(y) * scale);
    // Kontakt's factory faces run narrower than ours: a label sized for
    // them steps down to fit rather than cut its words (v1 `TextFont::fit`).
    let said = |k: f64| {
        let el = text(words.to_owned())
            .text_size(size * k)
            .fill(Fill::from(ink));
        match font {
            Some(font) => el.font(font.clone()),
            None => el,
        }
    };
    // A candidate that wraps is taller than the line, so it loses.
    let el = row([fits([
        said(1.),
        said(0.9),
        said(0.8),
        said(0.75).lines(1).min_w(0.),
    ])
    .h(line)])
    .align(Align::Center)
    .justify(match align {
        model::Align::Left => Justify::Start,
        model::Align::Center => Justify::Center,
        model::Align::Right => Justify::End,
    })
    .w(width)
    .h(line)
    .pad((inset * scale, 0.))
    .at(0., top);
    stack([el]).w(width).h(height).radius(0.).clip()
}

/// A picture's frame on a control `w` by `h` points, as Kontakt draws a
/// resizable one (KSP manual, `$CONTROL_PAR_PICTURE`): its Fixed Top,
/// Bottom, Left and Right areas stay intact and only the rest stretches,
/// so a rounded button or a glow keeps its ends. `scale` is points per
/// picture pixel. A picture without fixed areas along an axis stretches
/// whole along it.
fn sliced(
    img: &Arc<mui::prelude::Image>,
    meta: &model::ImageMeta,
    (w, h): (f64, f64),
    scale: f64,
) -> El {
    let m = meta.margins;
    let own = meta
        .size
        .map_or((f64::from(img.width), f64::from(img.height)), |s| {
            (s.width, s.height)
        });
    let across = cuts(
        meta.stretch[0],
        (m.left, m.right),
        (own.0, img.width),
        w,
        scale,
    );
    let down = cuts(
        meta.stretch[1],
        (m.top, m.bottom),
        (own.1, img.height),
        h,
        scale,
    );
    if across.len() == 1 && down.len() == 1 {
        return image(img, w, h);
    }
    let mut pieces = Vec::new();
    let mut y = 0.;
    for &(sy, sh, dh) in &down {
        let mut x = 0.;
        for &(sx, sw, dw) in &across {
            if let Some(piece) = crop(img, (sx, sy), (sw, sh)) {
                pieces.push(image(&piece, dw, dh).at(x, y));
            }
            x += dw;
        }
        y += dh;
    }
    stack(pieces).w(w).h(h)
}

/// One axis of a nine-slice: (decoded start, decoded length, drawn points)
/// per piece. The fixed ends `lo`/`hi` (picture pixels) keep their size;
/// a control shorter than both shrinks them alike and drops the middle.
fn cuts(
    on: bool,
    (lo, hi): (u32, u32),
    (own, decoded): (f64, u32),
    to: f64,
    scale: f64,
) -> Vec<(u32, u32, f64)> {
    let ends = f64::from(lo + hi);
    if !on || lo + hi == 0 || ends >= own {
        return vec![(0, decoded, to)];
    }
    let k = f64::from(decoded) / own.max(1.);
    let (a, b) = (
        (f64::from(lo) * k).round() as u32,
        (f64::from(hi) * k).round() as u32,
    );
    let mid = decoded.saturating_sub(a + b);
    let fit = (to / (ends * scale)).min(1.);
    let (dl, dh) = (f64::from(lo) * scale * fit, f64::from(hi) * scale * fit);
    vec![
        (0, a, dl),
        (a, mid, (to - dl - dh).max(0.)),
        (a + mid, b, dh),
    ]
    .into_iter()
    .filter(|&(_, n, d)| n > 0 && d > 0.)
    .collect()
}

/// The `size` pixels of `img` from `at`.
fn crop(
    img: &mui::prelude::Image,
    (x, y): (u32, u32),
    (w, h): (u32, u32),
) -> Option<Arc<mui::prelude::Image>> {
    let stride = img.width as usize * 4;
    let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
    for row in y as usize..(y + h) as usize {
        let at = row * stride + x as usize * 4;
        rgba.extend_from_slice(img.rgba.get(at..at + w as usize * 4)?);
    }
    mui::prelude::Image::rgba(w, h, rgba).map(Arc::new)
}

/// Words in a library's picture font, as v1 drew them (`src/ui/perf_view.rs`
/// `bitmap_words`): each glyph its own advance, pixels and alpha, scaled
/// with the face, never refitted through an outline font; one line.
// ponytail: one line; break long label text the way v1 did if a library wraps it.
fn bitmap_words(
    words: &str,
    style: Option<&model::TextStyle>,
    w: &model::Widget,
    (width, height): (f64, f64),
    scale: f64,
    glyphs: &crate::pictures::Glyphs,
) -> El {
    let glyph = |c: char| glyphs.get(crate::pictures::glyph(c));
    let advance = |c: char| glyph(c).map_or(0., |g| f64::from(g.width) * scale);
    let line = glyphs.first().map_or(0., |g| f64::from(g.height) * scale);
    let wide: f64 = words.chars().map(advance).sum();
    let align = style.map_or(
        if matches!(w.kind, Kind::Label) {
            model::Align::Left
        } else {
            model::Align::Center
        },
        |s| s.align,
    );
    let mut x = match align {
        model::Align::Left => 2. * scale,
        model::Align::Center => (width - wide) / 2.,
        model::Align::Right => width - 2. * scale - wide,
    };
    let y = w
        .text_y
        .map_or((height - line) / 2., |y| f64::from(y) * scale);
    let mut layers = Vec::new();
    for c in words.chars() {
        let Some(g) = glyph(c) else { continue };
        let gw = f64::from(g.width) * scale;
        layers.push(image(g, gw, line).at(x, y));
        x += gw;
    }
    stack(layers).w(width).h(height).radius(0.).clip()
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

#[cfg(test)]
mod tests;
