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
use crate::meter::Shown;

mod plan;
mod shape;
mod stock;
mod wave;

use plan::caption;
#[cfg(test)]
use plan::cuts;

/// Text size where the source sets none, in source pixels.
const TEXT: f64 = 11.;

/// `page` of `tree` at `scale` view points per source pixel, and how many
/// widgets this frame built rather than kept. `face` scopes its ids: each
/// face drawn in one frame (a rack part) needs its own. `level` (what
/// each meter source shows, left and right: the part's output, then each
/// instrument bus; crate::meter) lights its level meters.
pub fn page(
    ui: &mut Ui,
    face: usize,
    tree: &Interface,
    page: PageRef,
    pictures: &mut Pictures,
    at: (f64, &[[Shown; 2]]),
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
    at: (f64, &[[Shown; 2]]),
) -> (El, usize) {
    drawn_page(ui, face, tree, page, pictures, at, true)
}

fn drawn_page(
    ui: &mut Ui,
    face: usize,
    tree: &Interface,
    page: PageRef,
    pictures: &mut Pictures,
    (scale, level): (f64, &[[Shown; 2]]),
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
    // A scrolling region's content, by region; the page is `None`.
    let mut groups = std::collections::HashMap::from([(None, layers)]);
    let mut regions = Vec::new();
    for item in plan.widgets {
        let n = item.index;
        let r = item.rect;
        let wd = item.source;
        let lit = match wd.kind {
            Kind::LevelMeter {
                orientation,
                channel: Some(c),
                bus,
            } => {
                let from = level.get(bus.map_or(0, |b| usize::from(b) + 1));
                let shown = from.and_then(|l| l.get(usize::from(c)));
                Some((orientation, lit(shown.copied().unwrap_or_default())))
            }
            _ => None,
        };
        // A meter with a picture strip shows the frame for its level (a UVI
        // AudioMeter's animated VU) instead of a bar.
        let stepped = |p: &plan::Picture| match lit {
            Some((_, (t, ..))) if p.meta.frames > 1 => {
                (t * f64::from(p.meta.frames - 1)).round() as usize
            }
            _ => p.frame,
        };
        let picture = item.picture.as_ref().and_then(|p| {
            Some((
                pictures.frame(p.use_.asset, p.path, p.meta, stepped(p), device)?,
                p.meta,
            ))
        });
        let vector = (item.picture.as_ref()).and_then(|p| pictures.vector(p.use_.asset, p.path));
        let lit = lit.filter(|_| !item.picture.as_ref().is_some_and(|p| p.meta.frames > 1));
        let art = picture
            .as_ref()
            .and_then(|_| item.picture.as_ref().map(|p| p.size));
        let style = item.style;
        let t = item.position;
        let handle = item.handle.as_ref().and_then(|p| {
            Some((
                pictures.frame(p.use_.asset, p.path, p.meta, 0, device)?,
                p.meta,
                p.size,
            ))
        });
        let look = item.look;
        let stock_art = look
            .as_ref()
            .and_then(|look| look.pictures(pictures, device));
        let button_art = look.as_ref().and_then(|look| match look {
            stock::Look::Button { on: true, .. } if bare || !wd.hide.background => pictures.frame(
                model::AssetRef(usize::MAX - 5),
                "@stock/button",
                model::ImageMeta {
                    frames: 3,
                    axis: model::Orientation::Vertical,
                    ..Default::default()
                },
                0,
                device,
            ),
            stock::Look::Field { .. } if bare || !wd.hide.background => pictures.frame(
                model::AssetRef(usize::MAX - 7),
                "@stock/field",
                model::ImageMeta::default(),
                0,
                device,
            ),
            _ => None,
        });
        let stock_glyphs = look.as_ref().and_then(|look| match look {
            stock::Look::Field { .. }
                if style.is_some_and(|s| s.font == model::Font::Stock(13)) =>
            {
                pictures.font(model::AssetRef(usize::MAX - 4), "@stock/font13")
            }
            stock::Look::Button { on: true, .. }
                if button_art.is_some()
                    && style.is_none_or(|s| matches!(s.font, model::Font::Stock(0 | 12))) =>
            {
                let (id, path) = if style.is_some_and(|s| s.font == model::Font::Stock(12)) {
                    (usize::MAX - 6, "@stock/font12")
                } else {
                    (usize::MAX - 4, "@stock/font13")
                };
                pictures.font(model::AssetRef(id), path)
            }
            stock::Look::Knob { .. } if stock_art.is_some() => {
                pictures.font(model::AssetRef(usize::MAX - 3), "@stock/font")
            }
            _ => None,
        });
        let stock_glyphs = stock_glyphs.or_else(|| {
            if matches!(tree.source, model::Source::Ksp { .. })
                && matches!(wd.kind, Kind::Label)
                && style.is_some_and(|s| s.font == model::Font::Stock(12))
            {
                pictures.font(model::AssetRef(usize::MAX - 6), "@stock/font12")
            } else {
                None
            }
        });
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
            Some(&model::Font::Stock(23))
                if matches!(tree.source, model::Source::Ksp { .. })
                    && matches!(wd.kind, Kind::Label)
                    && style.is_some_and(|s| s.align == model::Align::Center)
                    && words.contains("  ")
                    && !words.contains('\n') =>
            {
                pictures.outline_font(model::AssetRef(usize::MAX - 10), "@stock/raldo")
            }
            _ => None,
        };
        let words = if outline.is_some() && style.is_some_and(|s| s.font == model::Font::Stock(23))
        {
            words.replace("  ", "   ")
        } else {
            words
        };
        let deps = (
            lit.map(|(o, (t, peak, over))| (o as u8, t.to_bits(), peak.to_bits(), over)),
            [r.x, r.y, r.width, r.height, scale, device].map(f64::to_bits),
            art.map(|a| [a.width, a.height].map(f64::to_bits)),
            (
                picture.as_ref().map(|(i, _)| Arc::as_ptr(i) as usize),
                handle.as_ref().map(|(i, ..)| Arc::as_ptr(i) as usize),
                vector.as_ref().map(|v| Arc::as_ptr(v) as usize),
            ),
            (
                &words,
                &look,
                (stock || handle.is_some()).then_some(t.to_bits()),
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
                (wd.waveform.as_ref()).map(|v| (v.cursor_us, v.length_us, v.peaks.clone())),
            ),
            glyphs
                .as_ref()
                .map(|g| Arc::as_ptr(g).cast::<()>() as usize),
            outline.as_ref().map(Font::id),
            stock_art
                .as_ref()
                .map(|art| art.each_ref().map(|image| Arc::as_ptr(image) as usize)),
            (
                button_art.as_ref().map(|image| Arc::as_ptr(image) as usize),
                stock_glyphs
                    .as_ref()
                    .map(|g| Arc::as_ptr(g).cast::<()>() as usize),
            ),
            wd.text_y,
            style.map(|s| (s.color, s.align as u8, s.size.map(f32::to_bits))),
            (wd.opacity.to_bits(), wd.rotation.to_bits()),
        );
        let el = ui.memo(format!("kontra-ui-{face}-{}", n.0), deps, |_| {
            built += 1;
            let (w, h) = (r.width, r.height);
            let mut parts = Vec::new();
            if let (Some((img, meta)), Some(a)) = (&picture, art) {
                // A single-frame button or switch picture sits inside its one-pixel face border.
                let inset = if matches!(tree.source, model::Source::Ksp { .. })
                    && matches!(wd.kind, Kind::Button { .. } | Kind::Switch)
                    && meta.frames == 1
                {
                    scale
                } else {
                    0.
                };
                let image = match &vector {
                    Some(v) => block(a.width, a.height)
                        .radius(0.)
                        .fill(Fill::Vector(v.clone(), Fit::Fill)),
                    None => sliced(img, meta, (a.width, a.height), scale),
                }
                .at(inset, inset);
                parts.push(if inset > 0. {
                    stack([image]).w(a.width).h(a.height).radius(0.).clip()
                } else {
                    image
                });
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
            // A slider's handle rides its track over its picture: bottom to
            // top, left to right.
            if let (Some((img, meta, a)), Kind::Slider { orientation, .. }) = (&handle, &wd.kind) {
                let (x, y) = match orientation {
                    model::Orientation::Vertical => (0., (1. - t) * (h - a.height)),
                    model::Orientation::Horizontal => (t * (w - a.width), 0.),
                };
                parts.push(sliced(img, meta, (a.width, a.height), scale).at(x, y));
            }
            if let Some(glyphs) = &glyphs {
                parts.push(bitmap_words(&words, style, wd, (w, h), scale, glyphs).at(0., 0.));
            } else if !words.is_empty() {
                if let (Some(glyphs), Some(style)) = (&stock_glyphs, style)
                    && matches!(wd.kind, Kind::Label)
                    && !words.contains('\n')
                    && words
                        .chars()
                        .all(|c| c == '?' || crate::pictures::glyph(c) != usize::from(b'?'))
                    && style.font == model::Font::Stock(12)
                    && plan::label_fits(&words, glyphs, w, scale)
                {
                    parts.push(stock::label(
                        &words,
                        style,
                        wd.text_y,
                        (w, h, scale, device),
                        glyphs,
                    ));
                } else {
                    parts.push(
                        words_el(
                            &words,
                            style,
                            wd,
                            (w, h),
                            scale,
                            outline.as_ref(),
                            tree.source,
                        )
                        .at(0., 0.),
                    );
                }
            }
            if let Some(wave) =
                (wd.waveform.as_ref()).and_then(|v| wave::draw(v, &wd.colors, (w, h, scale)))
            {
                parts.push(wave.at(0., 0.));
            }
            if let Some((o, t)) = lit {
                parts.push(meter(&wd.colors, o, t, (w, h, scale)).at(0., 0.));
            }
            let (w, h) = look.as_ref().map_or((w, h), |look| {
                parts.extend(look.draw(
                    wd,
                    style,
                    (w, h, scale, device),
                    bare,
                    (stock_art.as_ref(), button_art.as_ref()),
                    stock_glyphs.as_ref(),
                ));
                look.size(w, h, scale)
            });
            stack(parts)
                .w(w)
                .h(h)
                .opacity(wd.opacity)
                .rotation(wd.rotation.to_radians())
        });
        // A control takes the pointer under its own id (crate::input), and
        // hovering it shows its help text (`set_control_help`).
        let id = crate::input::surface(face, n.0);
        // Its arrows show while the pointer is over it.
        let el = if matches!(wd.kind, Kind::ValueEdit { arrows: true, .. })
            && ui.get(id.clone()).hovered
        {
            stack(vec![el.at(0., 0.), stock::steps(r.width, r.height, scale)])
                .w(r.width)
                .h(r.height)
        } else {
            el
        };
        let el = if crate::input::takes_input(wd) {
            let el = el
                .id(id)
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
        let host = tree.scroller(n);
        let o = host.map_or(model::Rect::default(), |v| tree.page_rect(v));
        let (x, y) = (r.x - o.x * scale, r.y - o.y * scale);
        let group = groups.entry(host).or_insert_with(Vec::new);
        group.push(el.at(x, y));
        if wd.viewport.is_some() {
            regions.push((n, host, group.len(), [x, y, r.width, r.height]));
            group.push(block(0., 0.));
        }
    }
    // Innermost first, each into its slot: clipped, scrolled by wheel and bar.
    for (v, host, at, [x, y, w, h]) in regions.into_iter().rev() {
        let size = tree.content(v);
        let content = stack(groups.remove(&Some(v)).unwrap_or_default());
        let content = content.w(size.width * scale).h(size.height * scale);
        let id = format!("kontra-v-{face}-{}", v.0);
        let el = col![content].scroll().w(w).h(h).id(id.clone());
        // It keeps the wheel at its ends too: never the rack's.
        let el = col![el].w(w).h(h).captures_wheel().id(id + "-keep");
        if let Some(slot) = groups.get_mut(&host).and_then(|g| g.get_mut(at)) {
            *slot = el.at(x, y);
        }
    }
    let layers = groups.remove(&None).unwrap_or_default();
    (stack(layers).w(w).h(h).radius(0.).clip(), built)
}

/// What a meter lights (crate::meter), in steps of a hundredth so a frame
/// redraws only when the bar or its peak line moves.
// ponytail: a group, insert or main FX meter (gain reduction) reads the
// part's output; a group sum or a compressor's reduction when a library
// needs one (Morphology's group meters).
fn lit(s: Shown) -> (f64, f64, bool) {
    let step = |x: f64| (x * 100.).round() / 100.;
    (step(s.level), step(s.peak), s.over)
}

/// A level meter lit to `t`: its OFF colour behind, its ON colour lit (its
/// OVERLOAD colour while that is lit), from the bottom when vertical, from
/// the left otherwise; a one-pixel line in its PEAK colour at the held peak.
/// A script that sets no PEAK or OVERLOAD colour shows neither.
fn meter(
    c: &model::Colors,
    o: model::Orientation,
    (t, peak, over): (f64, f64, bool),
    (w, h, s): (f64, f64, f64),
) -> El {
    let off = c
        .off
        .or(c.background)
        .map_or(Color::srgb(0.1, 0.1, 0.1), color);
    let on = (c.overload.filter(|_| over))
        .or(c.on)
        .or(c.bar)
        .map_or(Color::srgb(0.45, 0.8, 0.45), color);
    let span = |t: f64| match o {
        model::Orientation::Vertical => (0., h * (1. - t), w, h * t),
        model::Orientation::Horizontal => (0., 0., w * t, h),
    };
    let fill = |(x, y, w, h): (f64, f64, f64, f64), c: Color| {
        block(w, h).radius(0.).fill(Fill::from(c)).at(x, y)
    };
    let mut parts = vec![fill((0., 0., w, h), off), fill(span(t), on)];
    if let Some(ink) = c.peak.filter(|_| peak > 0.) {
        let line = match o {
            model::Orientation::Vertical => (0., (h * (1. - peak)).min(h - s), w, s),
            model::Orientation::Horizontal => ((w * peak - s).max(0.), 0., s, h),
        };
        parts.push(fill(line, color(ink)));
    }
    stack(parts).w(w).h(h)
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
            crate::keyboard::note_name(w.number().clamp(0., 127.) as u8)
        }
        Kind::Knob { display, .. } | Kind::ValueEdit { display, .. } => {
            display.format_value(w.number(), true, source)
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
    source: model::Source,
) -> El {
    let native_style = style.cloned().map(|mut style| {
        if font.is_some() && style.font == model::Font::Stock(23) {
            style.size = Some(12.);
        }
        style
    });
    let geometry = plan::text(words, native_style.as_ref(), w, (height, scale), source);
    // A UVI size is the font's line height (JUCE's): its words draw at the em that fills it.
    let lua = matches!(source, model::Source::FalconLua);
    let size = geometry.size
        * font
            .filter(|_| lua)
            .map_or(1., kontra_ui_pictures::em_per_height);
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
                    Some(
                        image(
                            &crate::pictures::crop(img, (sx, sy), (sw, sh))?,
                            width,
                            height,
                        )
                        .at(x, y),
                    )
                })
                .collect();
            stack(pieces).w(w).h(h)
        }
    }
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
