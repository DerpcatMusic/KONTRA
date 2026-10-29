//! Kontakt's own drawing of a script control with no picture: a dark
//! button that turns light when on, a dark field reading `Title:  value`,
//! a dark menu box with its arrow, and the round knob with its title and
//! value boxes beside it. Shades measured on Kontakt 8's Pacific page.

use kontra_ui_model::{self as model, Interface, Kind, Source, TextStyle, Widget};
use mui::prelude::*;

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
    },
    Field {
        title: String,
        value: String,
    },
    Menu,
    Knob {
        title: String,
        value: String,
        turn: u16,
    },
}

impl Look {
    /// The stock look of `w`, when Kontakt draws it: a script control
    /// whose picture is unset or absent from the library.
    pub(super) fn of(tree: &Interface, w: &Widget, value: f64) -> Option<Self> {
        let pictured = w.images.first().is_some_and(|u| {
            tree.assets.get(u.asset.0).is_some_and(|a| match a.kind {
                model::AssetKind::Image(m) => m.size.is_some(),
                _ => true,
            })
        });
        if !matches!(tree.source, Source::Ksp { .. }) || pictured {
            return None;
        }
        Some(match &w.kind {
            Kind::Button { .. } | Kind::Switch => Self::Button { on: value != 0. },
            Kind::Menu { .. } => Self::Menu,
            Kind::ValueEdit { display, .. } => Self::Field {
                title: (!w.hide.title)
                    .then(|| format!("{}:", w.text))
                    .unwrap_or_default(),
                value: if display.ratio == 0. {
                    crate::keyboard::note_name(value.clamp(0., 127.) as u8)
                } else {
                    display.format_value(value, true, tree.source)
                },
            },
            Kind::Knob { range, display } => {
                let span = range.max - range.min;
                let t = if span == 0. {
                    0.
                } else {
                    ((value - range.min) / span).clamp(0., 1.)
                };
                let shown = w.value_text.as_deref().map(str::trim).map_or_else(
                    || display.format_value(value, false, tree.source),
                    Into::into,
                );
                Self::Knob {
                    title: w.text.clone(),
                    value: format!("{shown} {}", display.unit).trim_end().into(),
                    turn: (t * 1000.) as u16,
                }
            }
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
        (w, h, scale): (f64, f64, f64),
    ) -> Vec<El> {
        let bg = !wd.hide.background;
        let ink = |default: u32| style.map_or(rgb(default), |s| super::color(s.color));
        match self {
            Self::Button { on } => {
                let mut parts = Vec::new();
                if bg {
                    let fill: Fill = if *on {
                        Gradient::linear(180., [(0., rgb(0xfe_fefe)), (1., rgb(0xe1_e1e1))]).into()
                    } else {
                        rgb(0x5f_5f5f).into()
                    };
                    parts.push(boxed(w, h, scale, fill, 0x2d_2d2d));
                }
                // The default font darkens on a lit button; a chosen one keeps its ink.
                let stock = style.is_none_or(|s| s.font == model::Font::Stock(0));
                let ink = if *on && stock {
                    rgb(DARK)
                } else {
                    ink(0xfe_fefe)
                };
                parts.push(words(
                    &super::caption(wd),
                    ink,
                    model::Align::Center,
                    w,
                    h,
                    scale,
                ));
                parts
            }
            Self::Field { title, value } => {
                let mut parts = Vec::new();
                if bg {
                    parts.push(boxed(w, h, scale, rgb(0x3f_3f3f).into(), 0x2c_2c2c));
                }
                let ink = ink(0xfe_fefe);
                parts.push(words(title, ink, model::Align::Left, w, h, scale));
                if !wd.hide.value {
                    parts.push(words(value, ink, model::Align::Right, w, h, scale));
                }
                parts
            }
            Self::Menu => {
                let mut parts = Vec::new();
                if bg {
                    parts.push(boxed(w, h, scale, rgb(0x5f_5f5f).into(), 0x2d_2d2d));
                }
                let ink = ink(0xfe_fefe);
                parts.push(words(
                    &super::caption(wd),
                    ink,
                    model::Align::Left,
                    w,
                    h,
                    scale,
                ));
                parts.extend(arrow(ink, w, h, scale));
                parts
            }
            Self::Knob { title, value, turn } => knob(wd, title, value, *turn, scale),
        }
    }
}

fn knob(wd: &Widget, title: &str, value: &str, turn: u16, s: f64) -> Vec<El> {
    let light = rgb(0xfe_fefe);
    let mut parts = vec![
        block(38. * s, 38. * s)
            .pill()
            .fill(rgb(0x2e_2e2e))
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
