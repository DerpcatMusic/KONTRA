//! Kontakt's own drawing of a script control with no picture: a dark
//! button that turns light when on, a dark field reading `Title:  value`,
//! a dark menu box with its arrow, the round knob with its title and value
//! boxes beside it (shades measured on Kontakt 8's Pacific page), and a
//! fader, a text field and the bars of a table (v1 `0cb7a8a0:src/ui/perf_view.rs`).

use kontra_ui_model::{self as model, Interface, Kind, Source, TextStyle, Widget};
use mui::prelude::*;
use std::sync::Arc;

/// A stock knob's own size: the knob, its title and value boxes. Kontakt
/// draws it so whatever width and height the script sets.
const KNOB: (f64, f64) = (87., 40.);
/// Kontakt's default ink on a light (on) button.
const DARK: u32 = 0x18_1818;

/// What a stock control shows; the memo key of its drawing.
#[derive(Hash)]
pub(super) enum Look {
    Button {
        on: bool,
        caption: String,
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
        store: &mut crate::Pictures,
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
    ) -> Option<Self> {
        if !bare && !unpictured(tree, w) {
            return None;
        }
        Some(match &w.kind {
            Kind::Button { .. } | Kind::Switch => Self::Button {
                on: t >= 1.,
                caption: super::caption(w, tree.source),
            },
            Kind::Menu { .. } => Self::Menu {
                caption: super::caption(w, tree.source),
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
        glyphs: Option<&crate::pictures::Glyphs>,
    ) -> Vec<El> {
        let bg = bare || !wd.hide.background;
        let ink = |default: u32| style.map_or(rgb(default), |s| super::color(s.color));
        match self {
            Self::Button { on, caption } => {
                let mut parts = Vec::new();
                if bg {
                    let fill: Fill = if *on {
                        Gradient::linear(180., [(0., rgb(0xfe_fefe)), (1., rgb(0xe1_e1e1))]).into()
                    } else {
                        rgb(0x5f_5f5f).into()
                    };
                    if *on && let Some(button) = button {
                        parts.push(boxed(w, h, scale, rgb(0xffffff).into(), 0x2d_2d2d));
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
                        parts.push(
                            super::sliced(
                                button,
                                &meta,
                                ((w - 2. * scale).max(0.), (h - 2. * scale).max(0.)),
                                scale,
                            )
                            .at(scale, scale),
                        );
                    } else {
                        parts.push(boxed(w, h, scale, fill, 0x2d_2d2d));
                    }
                }
                // The default font darkens on a lit button; a chosen one keeps its ink.
                let stock = style.is_none_or(|s| s.font == model::Font::Stock(0));
                let ink = if *on && stock {
                    rgb(DARK)
                } else {
                    ink(0xfe_fefe)
                };
                if *on
                    && button.is_some()
                    && style.is_none_or(|s| matches!(s.font, model::Font::Stock(0 | 12)))
                {
                    // The measured stock font advances four pixels for a space.
                    let metrics = if style.is_some_and(|s| s.font == model::Font::Stock(12)) {
                        StockMetrics::Button12
                    } else {
                        StockMetrics::Field
                    };
                    parts.push(field_words(
                        caption,
                        ink,
                        model::Align::Center,
                        (w, h, scale, device),
                        glyphs,
                        style.map_or(model::Rgba::rgb(DARK), |s| s.color),
                        metrics,
                    ));
                } else {
                    parts.push(words(caption, ink, model::Align::Center, w, h, scale));
                }
                parts
            }
            Self::Field { title, value } => {
                let mut parts = Vec::new();
                if bg {
                    if let Some(field) = button.and_then(|image| field_skin(image, w, h, scale)) {
                        parts.push(super::image(&field, w, h));
                    } else {
                        parts.push(boxed(w, h, scale, rgb(0x3f_3f3f).into(), 0x2c_2c2c));
                    }
                }
                let ink = ink(0xfe_fefe);
                // The installed font-13 atlas is 699×13.
                let font =
                    glyphs.filter(|_| style.is_some_and(|s| s.font == model::Font::Stock(13)));
                let color = style.map_or(model::Rgba::rgb(0xfefefe), |s| s.color);
                parts.push(field_words(
                    title,
                    ink,
                    model::Align::Left,
                    (w, h, scale, device),
                    font,
                    color,
                    StockMetrics::Field,
                ));
                if !wd.hide.value {
                    parts.push(field_words(
                        value,
                        ink,
                        model::Align::Right,
                        (w, h, scale, device),
                        font,
                        color,
                        StockMetrics::Field,
                    ));
                }
                parts
            }
            Self::Menu { caption } => {
                let mut parts = Vec::new();
                if bg {
                    parts.push(boxed(w, h, scale, rgb(0x5f_5f5f).into(), 0x2d_2d2d));
                }
                let ink = ink(0xfe_fefe);
                parts.push(words(caption, ink, model::Align::Left, w, h, scale));
                parts.extend(arrow(ink, w, h, scale));
                parts
            }
            Self::Knob {
                title, value, turn, ..
            } => {
                if let Some(art) = artwork.filter(|_| !wd.hide.title && !wd.hide.value) {
                    bitmap_knob(title, value, scale, art, glyphs)
                } else {
                    knob(wd, title, value, *turn, scale, bare)
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
enum StockMetrics {
    Field,
    Button12,
}

fn field_words(
    text: &str,
    ink: Color,
    align: model::Align,
    (w, h, s, device): (f64, f64, f64, f64),
    glyphs: Option<&crate::pictures::Glyphs>,
    color: model::Rgba,
    metrics: StockMetrics,
) -> El {
    let Some(glyphs) = glyphs else {
        return words(text, ink, align, w, h, s);
    };
    if text
        .chars()
        .any(|c| c != '?' && crate::pictures::glyph(c) == usize::from(b'?'))
    {
        return words(text, ink, align, w, h, s);
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
                .get(crate::pictures::glyph(c))
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
        if let Some(glyph) = glyphs.get(crate::pictures::glyph(c)) {
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
                        ((u16::from(p[3]) * u16::from(color.a)) / 255) as u8,
                    ]
                })
                .collect();
            if let Some(image) = Image::rgba(glyph.width, glyph.height, pixels)
                .and_then(|image| crate::pictures::at_integer(image, device as f32))
            {
                layers.push(super::image(&Arc::new(image), width, height).at(
                    x,
                    (h - height) / 2.
                        - match metrics {
                            StockMetrics::Field => s,
                            StockMetrics::Button12 => 0.,
                        },
                ));
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
    title: &str,
    value: &str,
    s: f64,
    art: &[Arc<Image>; 3],
    glyphs: Option<&crate::pictures::Glyphs>,
) -> Vec<El> {
    let [base, cap, light] = art;
    vec![
        super::image(base, 87. * s, 39. * s).at(0., 0.),
        super::image(cap, 25. * s, 29. * s).at(7. * s, 7. * s),
        super::image(light, 39. * s, 39. * s).at(0., 0.),
        stock_words(title, s, glyphs).at(42. * s, 2. * s),
        stock_words(value, s, glyphs).at(42. * s, 22. * s),
    ]
}

fn stock_words(text: &str, scale: f64, glyphs: Option<&crate::pictures::Glyphs>) -> El {
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
        if let Some(glyph) = glyphs.get(crate::pictures::glyph(c)) {
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
    // Vector keeps the flat stock skin; both use the existing measured palette.
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
    if !wd.hide.title {
        parts.push(
            stack([words(title, light, model::Align::Left, side, 16. * s, s)])
                .w(side)
                .h(16. * s)
                .radius(3. * s)
                .fill(rgb(0x8f_8e88))
                .at(36. * s, s),
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
            .at(36. * s, 21. * s),
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
pub(super) fn steps(w: f64, s: f64) -> El {
    let ink = rgb(0xd4_d4d0);
    let rows = [(1., 6.), (3., 7.), (5., 8.), (5., 9.)];
    let down = rows.map(|(width, y)| (width, 23. - y));
    stack(
        rows.into_iter()
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

/// One line of `text` across the box, padded from its sides.
fn words(text: &str, ink: Color, align: model::Align, w: f64, h: f64, s: f64) -> El {
    // Kontakt's stock font runs a size under a label's.
    let size = (super::TEXT - 1.) * s;
    row([text_el(text, size, ink)])
        .align(Align::Center)
        .justify(match align {
            model::Align::Left => Justify::Start,
            model::Align::Center => Justify::Center,
            model::Align::Right => Justify::End,
        })
        .w(w)
        .h(h)
        .pad((4. * s, 0.))
        .at(0., 0.)
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

/// Whether `w` draws a stock fader, text field or table: an unpictured
/// script one whose background the script has not hidden.
pub(super) fn shows(tree: &Interface, w: &Widget, bare: bool) -> bool {
    (bare || !w.hide.background)
        && (bare
            || unpictured(tree, w)
            || (matches!(tree.source, Source::FalconLua) && matches!(w.kind, Kind::Table { .. })))
        && matches!(
            w.kind,
            Kind::Slider { .. } | Kind::TextEdit | Kind::Table { .. }
        )
}

/// A KSP control with no picture, or one the library lacks; `false` for
/// one still decoding.
// ponytail: KSP's stock look only; UVI's own once a Lua library needs it.
fn unpictured(tree: &Interface, w: &Widget) -> bool {
    matches!(tree.source, Source::Ksp { .. })
        && !w.images.first().is_some_and(|u| {
            tree.assets.get(u.asset.0).is_some_and(|a| match a.kind {
                model::AssetKind::Image(m) => m.size.is_some(),
                _ => true,
            })
        })
}

/// `w`'s stock face at `t` (0..1 of its range), `width` by `height`
/// view points.
pub(super) fn face(w: &Widget, t: f64, (width, height): (f64, f64), bare: bool, uvi: bool) -> El {
    let kind = w.kind.clone();
    let bars: Vec<f64> = match (&w.kind, &w.value) {
        (Kind::Table { range, .. }, Some(model::Value::Integers(v))) => {
            let span = (range.max - range.min).max(f64::EPSILON);
            v.iter()
                .map(|&n| ((f64::from(n) - range.min) / span).clamp(0., 1.))
                .collect()
        }
        (Kind::Table { range, .. }, Some(model::Value::Reals(v))) => {
            let span = (range.max - range.min).max(f64::EPSILON);
            v.iter()
                .map(|&n| ((n - range.min) / span).clamp(0., 1.))
                .collect()
        }
        _ => Vec::new(),
    };
    let background = w.colors.background;
    let bar = w.colors.bar;
    let edges = w.table_inner_edge.map(|edge| edge.visible).unwrap_or(false);
    let zero = match &w.kind {
        Kind::Table { range, .. } => {
            ((0. - range.min) / (range.max - range.min).max(f64::EPSILON)).clamp(0., 1.)
        }
        _ => 0.,
    };
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
                d.push(Draw::fill(
                    place(thumb / 2., mid - 1., len - thumb, 2.),
                    grey(EDGE),
                ));
                d.push(Draw::fill(
                    place(thumb / 2., mid - 1., at - thumb / 2., 2.),
                    grey(VALUE),
                ));
                let th = (across - 2.).min(thumb * 1.4).max(2.);
                d.push(Draw::fill(
                    place(at - thumb / 2., mid - th / 2., thumb, th),
                    grey(VALUE),
                ));
            }
            Kind::Table { .. } if uvi => {
                if let Some(c) = background {
                    d.push(Draw::fill(rect(0., 0., w, h), Fill::from(super::color(c))));
                }
                let ink = bar.map_or(grey(VALUE), |c| Fill::from(super::color(c)));
                let bw = w / bars.len().max(1) as f64;
                let base = h * (1. - zero);
                for (n, v) in bars.iter().enumerate() {
                    let y = h * (1. - v);
                    let gap = if edges { 1. } else { 0. };
                    d.push(Draw::fill(
                        rect(
                            n as f64 * bw,
                            y.min(base),
                            (bw - gap).max(0.),
                            (base - y).abs(),
                        ),
                        ink.clone(),
                    ));
                }
            }
            Kind::Table { .. } => {
                field(&mut d, FIELD);
                let bw = w / bars.len().max(1) as f64;
                for (n, v) in bars.iter().enumerate() {
                    let bh = v * (h - 2.);
                    d.push(Draw::fill(
                        rect(n as f64 * bw + 1., h - 1. - bh, (bw - 1.).max(1.), bh),
                        grey(VALUE),
                    ));
                }
            }
            _ => field(&mut d, FIELD),
        }
        d
    })
    .w(width)
    .h(height)
}
