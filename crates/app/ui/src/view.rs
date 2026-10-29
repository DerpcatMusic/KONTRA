//! The Original view: one page of a tree, drawn as its source drew it.
//!
//! The page colour, the wallpaper from the skin offset down, then every
//! shown widget in draw order at its page position, scaled. Each widget is
//! a memo keyed by what it draws, so a frame where nothing moved rebuilds
//! nothing and a script write rebuilds the widgets it touched only.

use std::sync::Arc;

use kontra_ui_model::{self as model, Interface, Kind, PageRef};
use mui::prelude::*;

use crate::Pictures;

mod plan;
mod stock;

use plan::caption;
#[cfg(test)]
use plan::cuts;

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
    // Pointer state is an input to the otherwise pure draw projection.
    // Only source buttons and switches use Kontakt's held picture frame.
    let pressed: Vec<_> = tree
        .widgets
        .iter()
        .enumerate()
        .filter_map(|(n, w)| {
            if !matches!(
                tree.source,
                model::Source::Ksp { .. } | model::Source::PerformanceView
            ) || !matches!(w.kind, Kind::Button { .. } | Kind::Switch)
                || !tree.visible(model::WidgetRef(n))
            {
                return None;
            }
            let response = ui.get(crate::input::surface(face, n));
            (response.held && response.button == Some(Button::Primary))
                .then_some(model::WidgetRef(n))
        })
        .collect();
    let Some(plan) = plan::page(tree, page, bare, scale, &pressed) else {
        return (block(0., 0.), 0);
    };
    let p = plan.source;
    let mut built = 0;
    let (w, h) = (plan.size.width, plan.size.height);
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
    for item in plan.widgets {
        let n = item.index;
        let r = item.rect;
        let wd = item.source;
        let picture = item.picture.as_ref().and_then(|p| {
            Some((
                pictures.frame(p.use_.asset, p.path, p.meta, p.frame, device)?,
                p.meta,
            ))
        });
        let art = picture
            .as_ref()
            .and_then(|_| item.picture.as_ref().map(|p| p.size));
        let style = item.style;
        let t = item.position;
        let look = item.look;
        let stock_art = look
            .as_ref()
            .and_then(|look| look.pictures(pictures, device));
        let stock_glyphs = stock_art
            .as_ref()
            .and_then(|_| pictures.font(model::AssetRef(usize::MAX - 3), "@stock/font"));
        let stock = item.stock;
        let words = item.words;
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
            stock_art
                .as_ref()
                .map(|art| art.each_ref().map(|image| Arc::as_ptr(image) as usize)),
            stock_glyphs
                .as_ref()
                .map(|g| Arc::as_ptr(g).cast::<()>() as usize),
            wd.text_y,
            style.map(|s| (s.color, s.align as u8, s.size.map(f32::to_bits))),
            wd.opacity.to_bits(),
        );
        let el = ui.memo(format!("kontra-ui-{face}-{}", n.0), deps, |_| {
            built += 1;
            let (w, h) = (r.width, r.height);
            let mut parts = Vec::new();
            if let (Some((img, meta)), Some(a)) = (&picture, art) {
                parts.push(sliced(img, meta, (a.width, a.height), scale).at(0., 0.));
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
                parts.push(words_el(&words, style, wd, (w, h), scale, outline.as_ref()).at(0., 0.));
            }
            if let Some((o, t)) = lit {
                parts.push(meter(&wd.colors, o, t, w, h).at(0., 0.));
            }
            let (w, h) = look.as_ref().map_or((w, h), |look| {
                parts.extend(look.draw(
                    wd,
                    style,
                    (w, h, scale),
                    bare,
                    stock_art.as_ref(),
                    stock_glyphs.as_ref(),
                ));
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
        layers.push(el.at(r.x, r.y));
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

/// `w`'s value, or its initial one before the script sets it.
fn value(w: &model::Widget) -> f64 {
    match w.value {
        Some(model::Value::Integer(v)) => f64::from(v),
        Some(model::Value::Real(v)) => v,
        _ => w.initial_value,
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

/// An image asset's path and layout.
fn image_asset(tree: &Interface, a: model::AssetRef) -> Option<(&str, model::ImageMeta)> {
    let asset = tree.assets.get(a.0)?;
    match asset.kind {
        model::AssetKind::Image(m) => Some((asset.path.as_str(), m)),
        _ => None,
    }
}

fn words_el(
    words: &str,
    style: Option<&model::TextStyle>,
    w: &model::Widget,
    (width, height): (f64, f64),
    scale: f64,
    font: Option<&Font>,
) -> El {
    let geometry = plan::text(words, style, w, height, scale);
    let size = geometry.size;
    let ink = geometry.color.map_or(Color::srgb(1., 1., 1.), color);
    let align = geometry.align;
    let line = geometry.line;
    let top = geometry.top;
    let inset = geometry.inset;
    if geometry.multiline {
        let lines = words
            .split('\n')
            .enumerate()
            .map(|(n, words)| {
                let el = text(words.trim_end_matches('\r').to_owned())
                    .text_size(size)
                    .fill(Fill::from(ink));
                let el = match font {
                    Some(font) => el.font(font.clone()),
                    None => el,
                };
                row([el])
                    .align(Align::Center)
                    .justify(match align {
                        model::Align::Left => Justify::Start,
                        model::Align::Center => Justify::Center,
                        model::Align::Right => Justify::End,
                    })
                    .w(width)
                    .h(line)
                    .pad((inset, 0.))
                    .at(0., top + n as f64 * line)
            })
            .collect::<Vec<_>>();
        return stack(lines).w(width).h(height).radius(0.).clip();
    }
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
    .pad((inset, 0.))
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
    match plan::slices(meta, (img.width, img.height), (w, h), scale) {
        plan::Slices::Whole => image(img, w, h),
        plan::Slices::Pieces(slices) => {
            let pieces: Vec<El> = slices
                .into_iter()
                .filter_map(|slice| {
                    let [sx, sy, sw, sh] = slice.source;
                    let [x, y, width, height] = slice.destination;
                    Some(image(&crop(img, (sx, sy), (sw, sh))?, width, height).at(x, y))
                })
                .collect();
            stack(pieces).w(w).h(h)
        }
    }
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
/// with the face, never refitted through an outline font. Explicit newlines
/// start another row, including empty rows.
fn bitmap_words(
    words: &str,
    style: Option<&model::TextStyle>,
    w: &model::Widget,
    (width, height): (f64, f64),
    scale: f64,
    glyphs: &crate::pictures::Glyphs,
) -> El {
    let line = glyphs.first().map_or(0., |g| f64::from(g.height) * scale);
    let placements = plan::glyphs(words, style, w, (width, height), (scale, line), |n| {
        glyphs.get(n).map(|g| g.width)
    });
    let layers = placements
        .into_iter()
        .filter_map(|p| Some(image(glyphs.get(p.index)?, p.width, line).at(p.x, p.y)))
        .collect::<Vec<_>>();
    stack(layers).w(width).h(height).radius(0.).clip()
}

fn image(img: &Arc<mui::prelude::Image>, w: f64, h: f64) -> El {
    block(w, h)
        .radius(0.)
        .fill(Fill::Image(img.clone(), Fit::Fill))
}

pub(crate) fn color(c: model::Rgba) -> Color {
    Color::srgba(
        f32::from(c.r) / 255.,
        f32::from(c.g) / 255.,
        f32::from(c.b) / 255.,
        f32::from(c.a) / 255.,
    )
}

#[cfg(test)]
mod tests;
