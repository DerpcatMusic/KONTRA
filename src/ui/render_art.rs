//! Shared authored text and sliced art. No source-specific rendering decisions.
use super::{
    ir_view::{Assets, Picture},
    theme::*,
};
use moose::mui::mui::{
    geometry::Path as DrawPath,
    prelude::*,
    scene::{Fit, Image},
};
use sampler_ui_ir as ir;
use std::sync::Arc;

/// Min/max envelope from the current source zone; no audio reads in painting.
pub(super) fn waveform(
    peaks: Arc<[(f32, f32)]>,
    descriptor: Option<ir::Waveform>,
    duration_us: Option<u64>,
    colors: ir::Colors,
    hide_background: bool,
) -> El {
    let fill = |c: ir::Rgba| {
        Fill::from(
            Color::srgb(c.r as f32 / 255., c.g as f32 / 255., c.b as f32 / 255.)
                .with_alpha(c.a as f32 / 255.),
        )
    };
    let ink = colors.wave.map_or(Fill::from(value_ink(0.)), fill);
    let cursor_ink = colors.wave_cursor.map_or(Fill::from(Role::Ink), fill);
    let background = if hide_background {
        Role::Field.alpha(0.)
    } else {
        colors.background.map_or(Role::Field.alpha(1.), fill)
    };
    canvas(move |s| {
        let mut draws = Vec::new();
        let across = (s.width.ceil() as usize).clamp(1, 4096);
        let n = peaks.len();
        let mid = s.height / 2.;
        if n > 0 && mid > 0. {
            let mut top = Vec::with_capacity(across * 2);
            let mut foot = Vec::with_capacity(across * 2);
            for x in 0..across {
                let a = x * n / across;
                let b = ((x + 1) * n / across).max(a + 1).min(n);
                let (lo, hi) = peaks[a..b].iter().fold((0f32, 0f32), |(lo, hi), &(a, b)| {
                    (
                        if a.is_finite() {
                            lo.min(a.clamp(-1., 1.))
                        } else {
                            lo
                        },
                        if b.is_finite() {
                            hi.max(b.clamp(-1., 1.))
                        } else {
                            hi
                        },
                    )
                });
                for px in [
                    x as f64 * s.width / across as f64,
                    (x + 1) as f64 * s.width / across as f64,
                ] {
                    top.push(Point::new(px, mid - f64::from(hi).max(0.5 / mid) * mid));
                    foot.push(Point::new(px, mid - f64::from(lo).min(-0.5 / mid) * mid));
                }
            }
            top.extend(foot.into_iter().rev());
            draws.push(Draw::fill(DrawPath::polyline(top, true), ink.clone()));
        }
        if let Some(wave) = &descriptor
            && let Some(duration) = duration_us.filter(|&n| n > 0)
            && wave.cursor_us >= 0
        {
            let x =
                (wave.cursor_us as f64 / duration as f64).clamp(0., 1.) * (s.width - 1.).max(0.);
            draws.push(Draw::fill(
                rect(x.round(), 0., 1., s.height),
                cursor_ink.clone(),
            ));
        }
        draws
    })
    .fill(background)
}

fn lines(text: &str, room: f64, wrap: bool, advance: impl Fn(&str) -> f64) -> Vec<String> {
    let mut out = Vec::new();
    for paragraph in text.split('\n') {
        let mut line: Option<String> = None;
        for word in paragraph.split(' ') {
            line = Some(match line {
                None => word.to_owned(),
                Some(l)
                    if wrap && !l.trim().is_empty() && advance(&format!("{l} {word}")) > room =>
                {
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
#[allow(clippy::too_many_arguments)]
pub(super) fn words(
    source: &str,
    style: Option<&ir::TextStyle>,
    assets: &Assets,
    bitmap: bool,
    ink: Fill,
    w: f64,
    h: f64,
    scale: f64,
    top: Option<i32>,
    label: bool,
) -> El {
    let align = style.map_or(
        if label {
            ir::Align::Left
        } else {
            ir::Align::Center
        },
        |s| s.align,
    );
    if bitmap
        && let Some(ir::Font::Bitmap(asset)) = style.map(|s| &s.font)
        && let Some(font) = assets.get(*asset).filter(|p| p.frames.len() == 256)
    {
        return bitmap_words(source, align, top, w, h, scale, label, font);
    }
    let size = style.and_then(|s| s.size).map_or(SMALL, f64::from) * scale;
    let weight = if matches!(style.map(|s| &s.font), Some(ir::Font::Stock(16..=25))) {
        700.
    } else {
        400.
    };
    let fonts = match style.map(|s| &s.font) {
        Some(ir::Font::File(a)) => assets.font(a).into_iter().collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    static DEFAULT: std::sync::OnceLock<Vec<Font>> = std::sync::OnceLock::new();
    let fonts = if fonts.is_empty() {
        DEFAULT.get_or_init(|| Font::new(NOTO_SANS).into_iter().collect())
    } else {
        &fonts
    };
    let advance = |t: &str| {
        mui_text::shape_run(fonts, t, size, &[("wght", weight)]).map_or(0., |r| r.advance)
    };
    let lh = size * 1.25;
    let normalized = source
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace("\\n", "\n");
    let lines = lines(
        &normalized,
        (w - 4. * scale).max(0.),
        label && h >= lh * 2.,
        advance,
    );
    let tall = lh * lines.len() as f64;
    let one = |t: String| {
        let width = advance(&t);
        let fit = if width > w - 4. * scale && width > 0. {
            (size * (w - 4. * scale).max(0.) / width).max(size * 0.75)
        } else {
            size
        };
        let mut el = text(t)
            .text_size(fit)
            .text_axis("wght", weight)
            .fill(ink.clone())
            .lines(1)
            .min_w(0.);
        if let Some(font) = fonts.first() {
            el = el.font(font.clone());
        }
        row![el]
            .align(Align::Center)
            .justify(match align {
                ir::Align::Left => Justify::Start,
                ir::Align::Center => Justify::Center,
                ir::Align::Right => Justify::End,
            })
            .w(w)
            .h(lh)
            .pad((2. * scale, 0.))
    };
    let el = stack![col(lines.into_iter().map(one)).gap(0.).w(w).h(tall).at(
        0.,
        top.map_or(((h - tall) / 2.).max(0.), |y| f64::from(y) * scale)
    )]
    .w(w)
    .h(h)
    .clip();
    if label && tall > h { el.scroll() } else { el }
}
#[allow(clippy::too_many_arguments)]
fn bitmap_words(
    text: &str,
    align: ir::Align,
    top: Option<i32>,
    w: f64,
    h: f64,
    s: f64,
    label: bool,
    font: &Picture,
) -> El {
    let glyph = |c| &font.frames[super::pictures::font_glyph(c)];
    let advance = |text: &str| {
        text.chars()
            .map(|c| f64::from(glyph(c).width) * s)
            .sum::<f64>()
    };
    let lh = f64::from(font.frames[0].height) * s;
    let lines = lines(text, w - 4. * s, label && h >= 2. * lh, advance);
    let tall = lh * lines.len() as f64;
    let y = top.map_or(((h - tall) / 2.).max(0.), |y| f64::from(y) * s);
    let mut layers = Vec::new();
    for (n, line) in lines.iter().enumerate() {
        let width = advance(line);
        let mut x = match align {
            ir::Align::Left => 2. * s,
            ir::Align::Center => (w - width) / 2.,
            ir::Align::Right => w - 2. * s - width,
        };
        for c in line.chars() {
            let image = glyph(c);
            let width = f64::from(image.width) * s;
            layers.push(
                block(width, lh)
                    .radius(0.)
                    .fill(Fill::Image(image.clone(), Fit::Fill))
                    .at(x, y + n as f64 * lh),
            );
            x += width;
        }
    }
    let el = stack(layers).w(w).h(h).clip();
    if label && tall > h { el.scroll() } else { el }
}

/// Arbitrary margins take precedence; absent margins retain v1's symmetric cuts.
pub(super) fn sliced(image: &Arc<Image>, meta: ir::ImageMeta, w: f64, h: f64, s: f64) -> El {
    let axis = |on: bool,
                own: u32,
                source: u32,
                to: f64,
                mut first: u32,
                mut last: u32|
     -> Vec<(u32, u32, f64)> {
        if !on || own < 2 || source < 2 {
            return vec![(0, own, to)];
        }
        if first == 0 && last == 0 {
            first = (source - 1) / 2;
            last = first;
        }
        if first.saturating_add(last) >= source {
            return vec![(0, own, to)];
        }
        let (a, b) = (first as f64 * s, last as f64 * s);
        if to <= a + b {
            return vec![(0, own, to)];
        }
        let left = (first as f64 * own as f64 / source as f64).round() as u32;
        let right = (last as f64 * own as f64 / source as f64).round() as u32;
        if left + right >= own {
            return vec![(0, own, to)];
        }
        vec![
            (0, left, a),
            (left, own - left - right, to - a - b),
            (own - right, right, b),
        ]
    };
    let size = meta.size.unwrap_or(ir::Size {
        width: image.width,
        height: image.height,
    });
    let across = axis(
        meta.stretch[0],
        image.width,
        size.width,
        w,
        meta.margins.left,
        meta.margins.right,
    );
    let down = axis(
        meta.stretch[1],
        image.height,
        size.height,
        h,
        meta.margins.top,
        meta.margins.bottom,
    );
    if across.len() == 1 && down.len() == 1 {
        return block(w, h)
            .radius(0.)
            .fill(Fill::Image(image.clone(), Fit::Fill));
    }
    let mut parts = Vec::new();
    let mut y = 0.;
    for &(sy, sh, th) in &down {
        let mut x = 0.;
        for &(sx, sw, tw) in &across {
            if sw > 0 && sh > 0 && tw > 0. && th > 0. {
                // Shared image and clipped source view: no per-corner RGBA copies.
                let (zx, zy) = (tw / sw as f64, th / sh as f64);
                parts.push(
                    stack![
                        block(image.width as f64 * zx, image.height as f64 * zy)
                            .radius(0.)
                            .fill(Fill::Image(image.clone(), Fit::Fill))
                            .at(-(sx as f64) * zx, -(sy as f64) * zy)
                    ]
                    .w(tw)
                    .h(th)
                    .clip()
                    .at(x, y),
                );
            }
            x += tw;
        }
        y += th;
    }
    stack(parts).w(w).h(h)
}

/// Authored table bars; input and runtime transport remain in widget_state.
pub(super) fn table(
    samples: Vec<f64>,
    range: ir::Range,
    bipolar: bool,
    steps: Option<u32>,
    colors: ir::Colors,
) -> El {
    let bar = colors.bar.map_or(Role::Ink.alpha(0.6), |c| {
        Fill::from(
            Color::srgb(c.r as f32 / 255., c.g as f32 / 255., c.b as f32 / 255.)
                .with_alpha(c.a as f32 / 255.),
        )
    });
    let zero_ink = colors.zero_line.map_or(Role::Ink.alpha(0.25), |c| {
        Fill::from(
            Color::srgb(c.r as f32 / 255., c.g as f32 / 255., c.b as f32 / 255.)
                .with_alpha(c.a as f32 / 255.),
        )
    });
    canvas(move |s| {
        let unit = |v: f64| {
            if range.max == range.min {
                0.
            } else {
                ((v - range.min) / (range.max - range.min)).clamp(0., 1.)
            }
        };
        let zero = if bipolar { unit(0.) } else { 0. };
        let bw = s.width / samples.len().max(1) as f64;
        let mut draws = samples
            .iter()
            .enumerate()
            .map(|(n, v)| {
                let t = unit(*v);
                Draw::fill(
                    rect(
                        n as f64 * bw,
                        s.height * (1. - t.max(zero)),
                        (bw - 1.).max(1.),
                        s.height * (t - zero).abs(),
                    ),
                    bar.clone(),
                )
            })
            .collect::<Vec<_>>();
        draws.push(Draw::fill(
            rect(0., s.height * (1. - zero), s.width, 1.),
            zero_ink.clone(),
        ));
        for step in 1..steps.unwrap_or(0).min(128) {
            let count = steps.unwrap().min(128);
            draws.push(Draw::fill(
                rect(0., s.height * step as f64 / count as f64, s.width, 1.),
                Role::Ink.alpha(0.1),
            ));
        }
        draws
    })
    .fill(colors.background.map_or(Role::Ink.alpha(0.06), |c| {
        Fill::from(
            Color::srgb(c.r as f32 / 255., c.g as f32 / 255., c.b as f32 / 255.)
                .with_alpha(c.a as f32 / 255.),
        )
    }))
}
