//! Kontakt's own drawing of a script control with no picture: a dark
//! button that turns light when on, a dark field reading `Title:  value`,
//! a dark menu box with its arrow, the round knob with its title and value
//! boxes beside it (shades measured on Kontakt 8's Pacific page), and a
//! fader, a text field and the bars of a table (v1 `0cb7a8a0:src/ui/perf_view.rs`).

use kontra_ui_model::{self as model, Interface, Kind, StockSkin, TextStyle, Widget};
use mui::prelude::*;
use std::sync::Arc;

/// A stock knob's own size: the knob, its title and value boxes. Kontakt
/// draws it so whatever width and height the script sets.
const KNOB: (f64, f64) = (87., 40.);
/// Kontakt's default ink on a light (on) button.
const DARK: u32 = 0x18_1818;
/// The installed font-13 atlas peaks at 191/255 coverage; restore that scale
/// before applying the script's colour and alpha.
const FIELD_ATLAS_COVERAGE: u16 = 191;

/// What a stock control shows; the memo key of its drawing.
#[derive(Hash)]
pub(super) enum Look {
    Button {
        on: bool,
        caption: String,
        shift: bool,
    },
    Field {
        title: String,
        value: String,
    },
    Menu {
        caption: String,
    },
    Knob {
        title: String,
        value: String,
        turn: u16,
        frame: u8,
    },
}

impl Look {
    pub(super) fn pictures(
        &self,
        store: &mut kontra_ui_pictures::Pictures,
        scale: f64,
    ) -> Option<[Arc<Image>; 3]> {
        let Self::Knob { frame, .. } = self else {
            return None;
        };
        let meta = |frames| model::ImageMeta {
            frames,
            axis: model::Orientation::Horizontal,
            ..Default::default()
        };
        let frame = usize::from(*frame);
        Some([
            store.frame(
                model::AssetRef(usize::MAX),
                "@stock/base",
                meta(1),
                0,
                scale,
            )?,
            store.frame(
                model::AssetRef(usize::MAX - 1),
                "@stock/cap",
                meta(128),
                frame,
                scale,
            )?,
            store.frame(
                model::AssetRef(usize::MAX - 2),
                "@stock/light",
                meta(128),
                frame,
                scale,
            )?,
        ])
    }
    /// The stock look of `w` at `t` (0..1 of its range), when Kontakt
    /// draws one: a script button, switch, menu, value edit or knob whose
    /// picture is unset or absent from the library. `shown` is its value
    /// as the script shows it, `name` a knob's title; `bare`, it draws
    /// whatever pictures the control has.
    pub(super) fn of(
        tree: &Interface,
        w: &Widget,
        t: f64,
        shown: String,
        name: String,
        bare: bool,
        shift: bool,
    ) -> Option<Self> {
        if !bare && !unpictured(tree, w) {
            return None;
        }
        Some(match &w.kind {
            Kind::Button { .. } | Kind::Switch => Self::Button {
                on: t >= 1.,
                caption: super::caption(w),
                shift,
            },
            Kind::Menu { .. } => Self::Menu {
                caption: super::caption(w),
            },
            Kind::ValueEdit { .. } => Self::Field {
                title: if w.hide.title {
                    String::new()
                } else {
                    format!("{}:", w.text.trim())
                },
                value: shown,
            },
            Kind::Knob { display, .. } => Self::Knob {
                title: name,
                // A script's label leaves the unit to Kontakt ("25.0k ms").
                value: match &w.value_text {
                    Some(_) if !shown.is_empty() => format!("{} {}", shown.trim(), display.unit)
                        .trim_end()
                        .into(),
                    _ => shown,
                },
                turn: (t * 1000.) as u16,
                frame: (t * 127.).round() as u8,
            },
            _ => return None,
        })
    }

    /// The area it draws on: a knob takes its own size.
    pub(super) fn size(&self, w: f64, h: f64, scale: f64) -> (f64, f64) {
        match self {
            Self::Knob { .. } => (KNOB.0 * scale, KNOB.1 * scale),
            _ => (w, h),
        }
    }

    /// Its parts on a `w`×`h` widget at `scale`, and the ink and side
    /// its caption takes.
    pub(super) fn draw(
        &self,
        wd: &Widget,
        style: Option<&TextStyle>,
        (w, h, scale, device): (f64, f64, f64, f64),
        bare: bool,
        (artwork, button): (Option<&[Arc<Image>; 3]>, Option<&Arc<Image>>),
        glyphs: Option<&kontra_ui_pictures::Glyphs>,
    ) -> Vec<El> {
        let bg = bare || !wd.hide.background;
        let ink = |default: u32| style.map_or(rgb(default), |s| super::color(s.color));
        match self {
            Self::Button { on, caption, shift } => {
                let mut parts = Vec::new();
                if bg {
                    let fill: Fill = if *on {
                        Gradient::linear(180., [(0., rgb(0xfe_fefe)), (1., rgb(0xe1_e1e1))]).into()
                    } else {
                        rgb(0x5f_5f5f).into()
                    };
                    if *on && let Some(button) = button {
                        parts.push(block(w, h).fill(rgb(0xff_ffff)));
                        let meta = model::ImageMeta {
                            size: Some(model::Size {
                                width: 85.,
                                height: 16.,
                            }),
                            stretch: [true, false],
                            margins: model::Margins {
                                left: 5,
                                right: 5,
                                ..Default::default()
                            },
                            ..Default::default()
                        };
                        parts.push(super::sliced(button, &meta, (w, h), scale));
                    } else if *on {
                        parts.push(boxed(w, h, scale, fill, 0x2d_2d2d));
                    } else {
                        parts.push(
                            boxed(
                                (w - 2. * scale).max(0.),
                                (h - 2. * scale).max(0.),
                                scale,
                                fill,
                                0x2d_2d2d,
                            )
                            .at(0., scale),
                        );
                    }
                }
                // The default font darkens on a lit button; a chosen one keeps its ink.
                let stock = style.is_none_or(|s| s.font == model::Font::Stock(0));
                let ink = if *on && stock {
                    rgb(DARK)
                } else {
                    ink(0xfe_fefe)
                };
                let offset = if *shift { scale } else { 0. };
                let label = if *on
                    && button.is_some()
                    && style.is_none_or(|s| matches!(s.font, model::Font::Stock(0 | 12)))
                {
                    // The measured stock font advances four pixels for a space.
                    let metrics = if style.is_some_and(|s| s.font == model::Font::Stock(12)) {
                        StockMetrics::Button12
                    } else {
                        StockMetrics::Field
                    };
                    field_words(
                        caption,
                        ink,
                        model::Align::Center,
                        (w, h, scale, device),
                        glyphs,
                        if stock {
                            model::Rgba::rgb(DARK)
                        } else {
                            style.map_or(model::Rgba::rgb(DARK), |s| s.color)
                        },
                        metrics,
                        wd.text_y,
                    )
                } else {
                    words_at(
                        caption,
                        ink,
                        model::Align::Center,
                        (w, h, scale),
                        wd.text_y,
                        style.and_then(|s| s.size),
                    )
                };
                parts.push(if offset > 0. {
                    stack([label.at(offset, offset)]).w(w).h(h).clip()
                } else {
                    label
                });
                parts
            }
            Self::Field { title, value } => {
                let mut parts = Vec::new();
                if bg {
                    let height = (h - 2. * scale).max(0.);
                    if let Some(field) =
                        button.and_then(|image| field_skin(image, w, height, scale))
                    {
                        parts.push(super::image(&field, w, height).at(0., scale));
                    } else {
                        parts.push(
                            boxed(w, height, scale, rgb(0x3f_3f3f).into(), 0x2c_2c2c).at(0., scale),
                        );
                    }
                }
                let ink = ink(0xfe_fefe);
                // The installed font-13 atlas is 699×13.
                let font =
                    glyphs.filter(|_| style.is_some_and(|s| s.font == model::Font::Stock(13)));
                let color = style.map_or(model::Rgba::rgb(0xfefefe), |s| s.color);
                // The plain-text value clears the arrows' 14 px (the stock
                // font's already sits 11 px in, as measured).
                let arrows = matches!(wd.kind, Kind::ValueEdit { arrows: true, .. });
                let room = if arrows && font.is_none() {
                    (w - 10. * scale).max(0.)
                } else {
                    w
                };
                parts.push(field_words(
                    title,
                    ink,
                    model::Align::Left,
                    (w, h, scale, device),
                    font,
                    color,
                    StockMetrics::Field,
                    wd.text_y,
                ));
                if !wd.hide.value {
                    parts.push(field_words(
                        value,
                        ink,
                        model::Align::Right,
                        (room, h, scale, device),
                        font,
                        color,
                        StockMetrics::Field,
                        wd.value_y.or(wd.text_y),
                    ));
                }
                parts
            }
            Self::Menu { caption } => {
                let mut parts = Vec::new();
                if bg {
                    parts.push(
                        boxed(
                            (w - 2. * scale).max(0.),
                            (h - 2. * scale).max(0.),
                            scale,
                            rgb(0x5f_5f5f).into(),
                            0x2d_2d2d,
                        )
                        .at(0., scale),
                    );
                }
                let ink = ink(0xfe_fefe);
                parts.push(words_at(
                    caption,
                    ink,
                    model::Align::Left,
                    (w, h, scale),
                    wd.text_y,
                    None,
                ));
                if wd.menu_arrow {
                    parts.extend(arrow(ink, w, h, scale));
                }
                parts
            }
            Self::Knob {
                title, value, turn, ..
            } => {
                if let Some(art) = artwork.filter(|_| !wd.hide.title && !wd.hide.value) {
                    bitmap_knob(wd, title, value, scale, art, glyphs)
                } else {
                    knob(wd, title, value, *turn, scale, bare)
                }
            }
        }
    }
}

/// A label in the 9 px pixel font, as Kontakt draws it: the line laid out
/// in whole font pixels, then scaled to the screen by area, so at 150 % a
/// one-pixel stem is one full and one half-lit column (probed in Kontakt 8).
pub(super) fn label(
    text: &str,
    style: &TextStyle,
    text_y: Option<i32>,
    (w, h, s, device): (f64, f64, f64, f64),
    (left, right): (f64, f64),
    glyphs: &kontra_ui_pictures::Glyphs,
) -> Option<El> {
    let cells: Option<Vec<&Arc<Image>>> = text
        .chars()
        .map(|c| {
            let at = kontra_ui_pictures::glyph(c);
            (c == '?' || at != usize::from(b'?'))
                .then(|| glyphs.get(at))
                .flatten()
        })
        .collect();
    let cells = cells?;
    let advance = |c: char, g: &Image| if c == ' ' { 3 } else { g.width };
    let width: u32 = text.chars().zip(&cells).map(|(c, g)| advance(c, g)).sum();
    let (lw, lh) = ((w / s).ceil() as u32, (h / s).ceil() as u32);
    let room = (w / s - left - right - f64::from(width)).max(0.);
    let mut x = (left
        + match style.align {
            model::Align::Left => 0.,
            model::Align::Center => room / 2.,
            model::Align::Right => room,
        }) as u32;
    // Without TEXTPOS_Y the cell sits 2 px down (probed at 100 % and 150 %).
    let top = text_y.unwrap_or(2).max(0) as u32;
    let mut line = vec![0u8; lw as usize * lh as usize];
    for (c, g) in text.chars().zip(&cells) {
        for (gy, row) in g.rgba.chunks_exact(g.width as usize * 4).enumerate() {
            for (gx, p) in row.as_chunks::<4>().0.iter().enumerate() {
                let (px, py) = (x as usize + gx, top as usize + gy);
                if px < lw as usize
                    && let Some(cov) = line.get_mut(py * lw as usize + px)
                {
                    *cov = p[3];
                }
            }
        }
        x += advance(c, g);
    }
    let color = style.color;
    let (dw, dh) = (
        (f64::from(lw) * device).round() as u32,
        (f64::from(lh) * device).round() as u32,
    );
    let (across, down) = (area(lw, dw, device), area(lh, dh, device));
    let line = &line;
    let pixels: Vec<u8> = down
        .iter()
        .flat_map(|rows| across.iter().map(move |cols| (rows, cols)))
        .flat_map(|(rows, cols)| {
            let cov: f64 = rows
                .iter()
                .flat_map(|&(y, wy)| {
                    cols.iter().map(move |&(x, wx)| {
                        wy * wx * f64::from(line.get(y * lw as usize + x).copied().unwrap_or(0))
                    })
                })
                .sum();
            [
                color.r,
                color.g,
                color.b,
                ((cov.round().clamp(0., 255.) as u16 * u16::from(color.a)) / 255) as u8,
            ]
        })
        .collect();
    Image::rgba(dw, dh, pixels).map(|image| {
        stack([super::image(
            &Arc::new(image),
            f64::from(lw) * s,
            f64::from(lh) * s,
        )])
        .w(w)
        .h(h)
        .clip()
    })
}

/// For each of `d` screen pixels along one axis, the font pixels of `n` it
/// covers and the share of it each one fills, at `device` pixels per font pixel.
pub(super) fn area(n: u32, d: u32, device: f64) -> Vec<Vec<(usize, f64)>> {
    (0..d)
        .map(|i| {
            let (lo, hi) = (f64::from(i) / device, f64::from(i + 1) / device);
            (lo.floor() as u32..(hi.ceil() as u32).min(n))
                .map(|j| {
                    let cover = hi.min(f64::from(j + 1)) - lo.max(f64::from(j));
                    (j as usize, cover * device)
                })
                .collect()
        })
        .collect()
}

#[derive(Clone, Copy)]
enum StockMetrics {
    Field,
    Button12,
}

#[allow(clippy::too_many_arguments)]
fn field_words(
    text: &str,
    ink: Color,
    align: model::Align,
    (w, h, s, device): (f64, f64, f64, f64),
    glyphs: Option<&kontra_ui_pictures::Glyphs>,
    color: model::Rgba,
    metrics: StockMetrics,
    y_override: Option<i32>,
) -> El {
    let Some(glyphs) = glyphs else {
        return words_at(text, ink, align, (w, h, s), y_override, None);
    };
    if text
        .chars()
        .any(|c| c != '?' && kontra_ui_pictures::glyph(c) == usize::from(b'?'))
    {
        return words_at(text, ink, align, (w, h, s), y_override, None);
    }
    let advance = |c: char, glyph: &Image| {
        (if c == ' ' {
            match metrics {
                StockMetrics::Field => f64::from(glyph.width),
                StockMetrics::Button12 => 4.,
            }
        } else {
            f64::from(glyph.width)
        }) * s
    };
    let width: f64 = text
        .chars()
        .filter_map(|c| {
            glyphs
                .get(kontra_ui_pictures::glyph(c))
                .map(|glyph| advance(c, glyph))
        })
        .sum();
    let mut x = match align {
        model::Align::Left => 7. * s,
        model::Align::Center => (w - width) / 2.,
        model::Align::Right => w - 11. * s - width,
    };
    let mut layers = Vec::new();
    for c in text.chars() {
        if let Some(glyph) = glyphs.get(kontra_ui_pictures::glyph(c)) {
            let width = f64::from(glyph.width) * s;
            let height = f64::from(glyph.height) * s;
            let pixels: Vec<u8> = glyph
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| {
                    [
                        color.r,
                        color.g,
                        color.b,
                        ((u16::from(match metrics {
                            StockMetrics::Field => {
                                (u16::from(p[3]) * 255 / FIELD_ATLAS_COVERAGE).min(255) as u8
                            }
                            StockMetrics::Button12 => p[3],
                        }) * u16::from(color.a))
                            / 255) as u8,
                    ]
                })
                .collect();
            if let Some(image) = Image::rgba(glyph.width, glyph.height, pixels)
                .and_then(|image| kontra_ui_pictures::at(image, device as f32))
            {
                let y = y_override.map_or_else(
                    || match metrics {
                        StockMetrics::Field => (h - height) / 2. - s,
                        StockMetrics::Button12 => (h - height) / 2.,
                    },
                    |y| f64::from(y) * s,
                );
                layers.push(super::image(&Arc::new(image), width, height).at(x, y));
            }
            x += advance(c, glyph);
        }
    }
    stack(layers).w(w).h(h).clip()
}

/// Resize fixed borders and the centre into one texture, avoiding filtering seams.
fn field_skin(image: &Image, width: f64, height: f64, scale: f64) -> Option<Arc<Image>> {
    let factor = f64::from(image.width) / 85.;
    let w = (width / scale * factor).round() as u32;
    let h = (height / scale * factor).round() as u32;
    if w == 0 || h == 0 || w > 4096 || h > 4096 {
        return None;
    }
    let axis = |p: u32, src: u32, dst: u32, edge: u32| {
        let edge = edge.min(src / 2).min(dst / 2);
        if p < edge {
            p
        } else if p >= dst - edge {
            src - (dst - p)
        } else {
            edge + (u64::from(p - edge) * u64::from(src - edge * 2) / u64::from(dst - edge * 2))
                as u32
        }
    };
    let (xedge, yedge) = ((4. * factor).round() as u32, (3. * factor).round() as u32);
    let mut pixels = Vec::with_capacity(w as usize * h as usize * 4);
    for y in 0..h {
        let sy = axis(y, image.height, h, yedge);
        for x in 0..w {
            let sx = axis(x, image.width, w, xedge);
            let at = (sy as usize * image.width as usize + sx as usize) * 4;
            pixels.extend_from_slice(image.rgba.get(at..at + 4)?);
        }
    }
    Image::rgba(w, h, pixels).map(Arc::new)
}

fn bitmap_knob(
    wd: &Widget,
    title: &str,
    value: &str,
    s: f64,
    art: &[Arc<Image>; 3],
    glyphs: Option<&kontra_ui_pictures::Glyphs>,
) -> Vec<El> {
    let [base, cap, light] = art;
    let title_y = wd.text_y.map_or(2., f64::from) * s;
    let value_y = wd.value_y.map_or(22., f64::from) * s;
    vec![
        super::image(base, 87. * s, 39. * s).at(0., 0.),
        super::image(cap, 25. * s, 29. * s).at(7. * s, 7. * s),
        super::image(light, 39. * s, 39. * s).at(0., 0.),
        stock_words(title, s, glyphs).at(42. * s, title_y),
        stock_words(value, s, glyphs).at(42. * s, value_y),
    ]
}

fn stock_words(text: &str, scale: f64, glyphs: Option<&kontra_ui_pictures::Glyphs>) -> El {
    let Some(glyphs) = glyphs else {
        return words(
            text,
            rgb(0xfe_fefe),
            model::Align::Left,
            45. * scale,
            14. * scale,
            scale,
        );
    };
    let mut x = 0.;
    let mut layers = Vec::new();
    for c in text.chars() {
        if let Some(glyph) = glyphs.get(kontra_ui_pictures::glyph(c)) {
            let width = f64::from(glyph.width) * scale;
            layers.push(super::image(glyph, width, f64::from(glyph.height) * scale).at(x, 0.));
            x += width;
        }
    }
    stack(layers).w(45. * scale).h(13. * scale).clip()
}

fn knob(wd: &Widget, title: &str, value: &str, turn: u16, s: f64, bare: bool) -> Vec<El> {
    let light = rgb(0xfe_fefe);
    // Pacific's unpictured knobs have a lit upper ring and a dark lower edge.
    // KONTRA keeps the flat stock skin; both use the existing measured palette.
    let ring: Fill = if bare {
        rgb(0x2e_2e2e).into()
    } else {
        Gradient::linear(
            180.,
            [
                (0., rgb(0x5f_5f5f)),
                (0.5, rgb(0x2e_2e2e)),
                (1., rgb(0x14_1414)),
            ],
        )
        .into()
    };
    let mut parts = vec![
        block(38. * s, 38. * s)
            .pill()
            .fill(ring)
            .stroke(rgb(0x14_1414))
            .stroke_width(s)
            .at(0., s),
        block(22. * s, 22. * s)
            .pill()
            .fill(Gradient::linear(
                160.,
                [(0., rgb(0xff_ffff)), (1., rgb(0xc8_c8c3))],
            ))
            .at(8. * s, 9. * s),
    ];
    // The pointer: a dot on the cap, from 7 o'clock (0) to 5 o'clock (1).
    let a = (f64::from(turn) / 1000.).mul_add(270., -135.).to_radians();
    let (x, y) = (19. + 7. * a.sin(), 20. - 7. * a.cos());
    parts.push(
        block(3. * s, 3. * s)
            .pill()
            .fill(rgb(0x2a_2a2a))
            .at((x - 1.5) * s, (y - 1.5) * s),
    );
    let side = (KNOB.0 - 36.) * s;
    let title_y = wd.text_y.map_or(1., f64::from) * s;
    let value_y = wd.value_y.map_or(21., f64::from) * s;
    if !wd.hide.title {
        parts.push(
            stack([words(title, light, model::Align::Left, side, 16. * s, s)])
                .w(side)
                .h(16. * s)
                .radius(3. * s)
                .fill(rgb(0x8f_8e88))
                .at(36. * s, title_y),
        );
    }
    if !wd.hide.value {
        parts.push(
            stack([words(
                value,
                rgb(0xed_f0f0),
                model::Align::Left,
                side,
                17. * s,
                s,
            )])
            .w(side)
            .h(17. * s)
            .radius(3. * s)
            .fill(rgb(0x5f_5f5a))
            .at(36. * s, value_y),
        );
    }
    parts
}

/// A menu's arrow: a small triangle pointing down at the right.
fn arrow(ink: Color, w: f64, h: f64, s: f64) -> Vec<El> {
    let (x, y) = (w - 11. * s, h / 2. - 1.5 * s);
    (0..3)
        .map(|row| {
            let r = f64::from(row);
            block((6. - 2. * r) * s, s)
                .radius(0.)
                .fill(ink)
                .at(x + r * s, y + r * s)
        })
        .collect()
}

/// A value edit's step arrows, ▲ over ▼, centred 9.5 px in from its right
/// edge (Kontakt 8.13, measured).
pub(super) fn steps(w: f64, h: f64, s: f64) -> El {
    let ink = rgb(0xd4_d4d0);
    // Measured on a 22 px field (▲ rows 6..9, ▼ 14..17): placed about the
    // middle, so each arrow sits in the half its press steps (input::arrow).
    let mid = (h / s / 2.).floor();
    let rows = [1., 3., 5., 5.].into_iter().zip(-5..);
    let rows: Vec<_> = rows
        .map(|(width, dy)| (width, mid + f64::from(dy)))
        .collect();
    let down = rows.iter().map(|&(width, y)| (width, 2. * mid + 1. - y));
    stack(
        rows.iter()
            .copied()
            .chain(down)
            .map(|(width, y)| {
                block(width * s, s)
                    .radius(0.)
                    .fill(ink)
                    .at(w - (9.5 + width / 2.) * s, y * s)
            })
            .collect::<Vec<_>>(),
    )
    .at(0., 0.)
}

/// A stock box: `fill` inside a one-pixel `edge`.
fn boxed(w: f64, h: f64, s: f64, fill: Fill, edge: u32) -> El {
    block(w, h)
        .radius(2. * s)
        .fill(fill)
        .stroke(rgb(edge))
        .stroke_width(s)
        .at(0., 0.)
}

/// One line of `text` across the box, padded from its sides, vertically
/// positioned at `y_override` (in source points) when set or centered otherwise.
fn words_at(
    text: &str,
    ink: Color,
    align: model::Align,
    (w, h, s): (f64, f64, f64),
    y_override: Option<i32>,
    size: Option<f32>,
) -> El {
    // Kontakt's stock font runs a size under a label's.
    let size = size.map_or(super::TEXT - 1., f64::from) * s;
    let row = row([text_el(text, size, ink)])
        .align(Align::Center)
        .justify(match align {
            model::Align::Left => Justify::Start,
            model::Align::Center => Justify::Center,
            model::Align::Right => Justify::End,
        })
        .w(w)
        .pad((4. * s, 0.));
    match y_override {
        Some(y) => stack([row.h(size * 1.25).at(0., f64::from(y) * s)])
            .w(w)
            .h(h)
            .clip(),
        None => row.h(h).at(0., 0.),
    }
}

/// One line of `text` across the box, padded from its sides.
fn words(text: &str, ink: Color, align: model::Align, w: f64, h: f64, s: f64) -> El {
    words_at(text, ink, align, (w, h, s), None, None)
}

fn text_el(t: &str, size: f64, ink: Color) -> El {
    text(t.to_owned())
        .text_size(size)
        .fill(Fill::from(ink))
        .lines(1)
        .min_w(0.)
}

fn rgb(c: u32) -> Color {
    super::color(model::Rgba::rgb(c))
}

/// The stock graphite: the field, its edge, the value.
const FIELD: f32 = 0.16;
const EDGE: f32 = 0.32;
const VALUE: f32 = 0.85;

fn grey(l: f32) -> Fill {
    Fill::from(Color::srgb(l, l, l))
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> Path {
    Path::polyline(
        [(x, y), (x + w, y), (x + w, y + h), (x, y + h)].map(|(x, y)| Point::new(x, y)),
        true,
    )
}

/// Whether `w` draws a stock fader, shape or canvas: an unpictured
/// script slider whose background the script has not hidden, or a shape/canvas.
/// Tables render through [`table`] so bars still draw over custom pictures or
/// `HIDE_PART_BG`.
pub(super) fn shows(tree: &Interface, w: &Widget, bare: bool) -> bool {
    (bare || !w.hide.background)
        && (bare || unpictured(tree, w))
        // A text edit draws no field: its words sit on the page (probe/wave.ksp).
        && matches!(w.kind, Kind::Slider { .. })
        || matches!(w.kind, Kind::Shape(_) | Kind::Canvas(_))
}

/// Kontakt's face for a script label with no picture: a flat grey, one
/// pixel in on every side (Kontakt 8 oracle, a bare probe page).
pub(super) fn label_face(
    tree: &Interface,
    w: &Widget,
    (width, height, s): (f64, f64, f64),
) -> Option<El> {
    // Only a script's own `ui_label` (it has a `get_ui_id`): a native_ui
    // text on the same page draws no face (Conflux's tabs and captions).
    let scripted = w.source_id.is_some();
    (matches!(w.kind, Kind::Label) && scripted && !w.hide.background && unpictured(tree, w)).then(
        || {
            block((width - 2. * s).max(0.), (height - 2. * s).max(0.))
                .radius(0.)
                .fill(rgb(0x77_7671))
                .at(s, s)
        },
    )
}

/// A control on an interface using Kontakt's stock skin with no picture (or
/// one the library lacks); `false` for one still decoding.
fn unpictured(tree: &Interface, w: &Widget) -> bool {
    tree.stock_skin == StockSkin::Kontakt
        && !w.images.first().is_some_and(|u| {
            tree.assets.get(u.asset.0).is_some_and(|a| match a.kind {
                model::AssetKind::Image(m) => m.size.is_some(),
                _ => true,
            })
        })
}

/// `w`'s stock face at `t` (0..1 of its range), `width` by `height`
/// view points.
pub(super) fn face(w: &Widget, t: f64, (width, height): (f64, f64), bare: bool) -> El {
    let kind = w.kind.clone();
    let (background, page_width) = (w.colors.background, w.rect.width);
    let bar = w.colors.bar;
    canvas(move |z| {
        let (w, h) = (z.width, z.height);
        let field = |d: &mut Vec<Draw>, l: f32| {
            d.push(Draw::fill(rect(0., 0., w, h), grey(l)));
            d.push(Draw::stroke(rect(0.5, 0.5, w - 1., h - 1.), grey(EDGE), 1.));
        };
        let mut d = Vec::new();
        match kind {
            Kind::Slider { orientation, .. } => {
                let vertical = if bare {
                    matches!(orientation, model::Orientation::Vertical)
                } else {
                    h > w
                };
                let (len, across) = if vertical { (h, w) } else { (w, h) };
                let thumb = (across * 0.6).clamp(4., 12.).min(len / 3.);
                let at = thumb / 2. + t * (len - thumb);
                let mid = (across / 2.).round();
                let place = |along: f64, off: f64, l: f64, th: f64| {
                    if vertical {
                        rect(off, len - along - l, th, l)
                    } else {
                        rect(along, off, l, th)
                    }
                };
                let track = background.map_or(grey(EDGE), |c| Fill::from(super::color(c)));
                let active = bar.map_or(grey(VALUE), |c| Fill::from(super::color(c)));
                d.push(Draw::fill(
                    place(thumb / 2., mid - 1., len - thumb, 2.),
                    track,
                ));
                d.push(Draw::fill(
                    place(thumb / 2., mid - 1., at - thumb / 2., 2.),
                    active.clone(),
                ));
                let th = (across - 2.).min(thumb * 1.4).max(2.);
                d.push(Draw::fill(
                    place(at - thumb / 2., mid - th / 2., thumb, th),
                    active,
                ));
            }
            Kind::Shape(shape) => {
                d.extend(super::shape::fill(shape, page_width, (w, h), background))
            }
            Kind::Canvas(ref p) => d.extend(kontra_ui_pictures::canvas(p, w / page_width)),
            _ => field(&mut d, FIELD),
        }
        d
    })
    .w(width)
    .h(height)
}

/// Draws a `Kind::Table` widget's background (unless hidden or pictured),
/// zero line (for bipolar tables), and value bars, driven purely by
/// [`model::TableStyle`], `steps_shown`, `bipolar`, and `colors`.
pub(super) fn table(
    w: &Widget,
    (width, height): (f64, f64),
    bare: bool,
    has_picture: bool,
    style: model::TableStyle,
) -> El {
    let Kind::Table {
        columns,
        range,
        bipolar,
        steps_shown,
        ..
    } = w.kind
    else {
        return block(width, height);
    };
    let span = (range.max - range.min).max(f64::EPSILON);
    let mut bars: Vec<f64> = match &w.value {
        Some(model::Value::Integers(v)) => v
            .iter()
            .map(|&n| ((f64::from(n) - range.min) / span).clamp(0., 1.))
            .collect(),
        Some(model::Value::Reals(v)) => v
            .iter()
            .map(|&n| ((n - range.min) / span).clamp(0., 1.))
            .collect(),
        _ => Vec::new(),
    };
    let shown = if columns > 0 {
        steps_shown
            .filter(|&s| s > 0)
            .unwrap_or(columns)
            .min(columns) as usize
    } else {
        steps_shown
            .filter(|&s| s > 0)
            .map_or(bars.len(), |s| (s as usize).min(bars.len()))
    };
    bars.truncate(shown);
    let cols = shown.max(1) as f64;
    let background = w.colors.background;
    let bar = w.colors.bar;
    let zero_line = w.colors.zero_line;
    let draw_bg = !has_picture && (bare || !w.hide.background);
    let edges = w.table_inner_edge.is_none_or(|edge| edge.visible);
    let zero = ((0. - range.min) / span).clamp(0., 1.);
    canvas(move |z| {
        let (w, h) = (z.width, z.height);
        let mut d = Vec::new();
        let inset = if draw_bg && style.frame_rim {
            let bg_fill = background.map_or(grey(FIELD), |c| Fill::from(super::color(c)));
            d.push(Draw::fill(rect(0., 0., w, h), bg_fill));
            d.push(Draw::stroke(rect(0.5, 0.5, w - 1., h - 1.), grey(EDGE), 1.));
            1.
        } else {
            0.
        };
        let (ix, iy, iw, ih) = (
            inset,
            inset,
            (w - 2. * inset).max(0.),
            (h - 2. * inset).max(0.),
        );
        if iw <= 0. || ih <= 0. {
            return d;
        }
        let bw = iw / cols;
        let (cell_bg_w, bar_w, per_cell_bg) = match style.inner_edge {
            model::TableGap::Adaptive => {
                let gap = if bw >= 3. { 1. } else { 0. };
                let cw = (bw - gap).max(1.).min(bw);
                (cw, cw, false)
            }
            model::TableGap::CellEdges {
                gap_px,
                seamless_overlap_px,
            } => {
                let (gap, over) = if edges {
                    (f64::from(gap_px), 0.)
                } else {
                    (0., f64::from(seamless_overlap_px))
                };
                let cell = (bw - gap).max(0.);
                (cell, cell + over, edges)
            }
        };
        if draw_bg
            && !style.frame_rim
            && let Some(c) = background
        {
            let fill = Fill::from(super::color(c));
            if per_cell_bg {
                for n in 0..shown {
                    d.push(Draw::fill(
                        rect(ix + n as f64 * bw, iy, cell_bg_w, ih),
                        fill.clone(),
                    ));
                }
            } else {
                d.push(Draw::fill(rect(ix, iy, iw, ih), fill));
            }
        }
        let ink = bar.map_or(grey(VALUE), |c| Fill::from(super::color(c)));
        let is_bipolar = bipolar && zero > 0. && zero < 1.;
        let base = iy + ih * (1. - zero);
        if style.draw_zero_line && is_bipolar {
            let line_fill = zero_line.map_or(grey(EDGE), |c| Fill::from(super::color(c)));
            let ly = base.round().clamp(iy, (iy + ih - 1.).max(iy));
            d.push(Draw::fill(rect(ix, ly, iw, 1.), line_fill));
        }
        let min_zero = if is_bipolar {
            f64::from(style.min_zero_bar_px)
        } else {
            0.
        };
        for (n, &v) in bars.iter().enumerate() {
            let x = ix + n as f64 * bw;
            if is_bipolar {
                let y = iy + ih * (1. - v);
                let bh = (base - y).abs().max(min_zero);
                if bh > 0. {
                    let top = if y < base {
                        y
                    } else {
                        base.min((iy + ih - min_zero).max(iy))
                    };
                    d.push(Draw::fill(rect(x, top.min(base), bar_w, bh), ink.clone()));
                }
            } else {
                let bh = v * ih;
                if bh > 0. {
                    d.push(Draw::fill(rect(x, iy + ih - bh, bar_w, bh), ink.clone()));
                }
            }
        }
        d
    })
    .w(width)
    .h(height)
}
