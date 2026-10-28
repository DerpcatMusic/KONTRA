//! The components every view is built from, each written once. They take
//! the [`Ui`], an id and the value they show, and return the element and
//! whether the user acted on it this frame. Hover, press and value changes
//! ease through mui's springs, so motion costs nothing once it settles.

use std::f64::consts::PI;
use std::ops::RangeInclusive;

use mui::prelude::*;

use crate::theme::{
    CONTROL, INSET, SMALL, SPACE, TEXT, TIGHT, accent, hairline, lift, quick, secondary,
};

/// Line icons on a 16-unit square: crisp at any size, no icon font.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Left,
    Right,
    Down,
    More,
    Close,
    Sidebar,
    Search,
    Star,
    Menu,
    Warning,
    Stop,
}

/// `icon` in `ink`, `size` points square.
pub fn glyph(icon: Icon, size: f64, ink: Fill) -> El {
    canvas(move |s| {
        let u = s.width.min(s.height) / 16.;
        let (ox, oy) = ((s.width - 16. * u) / 2., (s.height - 16. * u) / 2.);
        let p = |&(x, y): &(f64, f64)| Point::new(ox + x * u, oy + y * u);
        let line = |pts: &[(f64, f64)], closed: bool| {
            Draw::stroke(
                Path::polyline(pts.iter().map(p), closed),
                ink.clone(),
                1.5 * u.max(0.75),
            )
        };
        let shape =
            |pts: &[(f64, f64)]| Draw::fill(Path::polyline(pts.iter().map(p), true), ink.clone());
        let dot = |x: f64, y: f64| {
            shape(&[
                (x - 1., y - 1.),
                (x + 1., y - 1.),
                (x + 1., y + 1.),
                (x - 1., y + 1.),
            ])
        };
        let round = |cx: f64, cy: f64, r: f64| -> Vec<(f64, f64)> {
            (0..24)
                .map(|n| f64::from(n) * PI / 12.)
                .map(|a| (cx + r * a.cos(), cy + r * a.sin()))
                .collect()
        };
        match icon {
            Icon::Left => vec![line(&[(10., 4.), (6., 8.), (10., 12.)], false)],
            Icon::Right => vec![line(&[(6., 4.), (10., 8.), (6., 12.)], false)],
            Icon::Down => vec![line(&[(4., 6.), (8., 10.), (12., 6.)], false)],
            Icon::More => vec![dot(3.5, 8.), dot(8., 8.), dot(12.5, 8.)],
            Icon::Close => vec![
                line(&[(4.5, 4.5), (11.5, 11.5)], false),
                line(&[(11.5, 4.5), (4.5, 11.5)], false),
            ],
            Icon::Sidebar => vec![
                line(&[(2.5, 3.5), (13.5, 3.5), (13.5, 12.5), (2.5, 12.5)], true),
                line(&[(6.5, 3.5), (6.5, 12.5)], false),
            ],
            Icon::Search => vec![
                line(&round(7., 7., 4.), true),
                line(&[(10., 10.), (13.5, 13.5)], false),
            ],
            Icon::Star => {
                let points: Vec<_> = (0..10)
                    .map(|n| {
                        let (r, a) = (
                            if n % 2 == 0 { 6. } else { 2.6 },
                            -PI / 2. + f64::from(n) * PI / 5.,
                        );
                        (8. + r * a.cos(), 8.6 + r * a.sin())
                    })
                    .collect();
                vec![shape(&points)]
            }
            Icon::Menu => [4.5, 8., 11.5]
                .map(|y| line(&[(3., y), (13., y)], false))
                .into(),
            Icon::Warning => vec![
                line(&[(8., 2.5), (14., 13.), (2., 13.)], true),
                line(&[(8., 6.5), (8., 9.5)], false),
                dot(8., 11.3),
            ],
            Icon::Stop => vec![
                line(&round(8., 8., 5.5), true),
                line(&[(4.2, 11.8), (11.8, 4.2)], false),
            ],
        }
    })
    .square(size)
    .shrink(0)
}

/// The states every flat control shares: the pointing hand, a faint lift on
/// hover, a deeper one pressed, a 1 px ring for keyboard focus.
pub fn interactive(el: El) -> El {
    el.cursor(Cursor::Hand)
        .on(State::Hover, |s| s.fill(lift(1)))
        .on(State::Press, |s| s.fill(lift(2)))
        .on(State::FocusVisible, |s| {
            s.stroke(Role::Primary.alpha(0.9)).stroke_width(1)
        })
        .animate_with(quick())
}

fn ink(on: bool) -> Fill {
    if on {
        Fill::from(Role::Ink)
    } else {
        secondary()
    }
}

/// A word that does something. `on` holds it raised with ink text.
pub fn button(ui: &mut Ui, id: impl Into<Id>, label: &str, on: bool) -> Response {
    let id: Id = id.into();
    let changed = ui.get(id.clone()).activated();
    let el = row![
        body(label.to_owned())
            .text_size(TEXT)
            .fill(ink(on))
            .lines(1)
            .min_w(0)
    ]
    .align(Align::Center)
    .justify(Justify::Center)
    .pad((SPACE, 0))
    .h(CONTROL)
    .shrink(0)
    .when(on, |e| e.fill(Role::Raised))
    .focusable()
    .a11y(A11y::Button)
    .named(label.to_owned())
    .id(id);
    Response {
        el: interactive(el),
        changed,
    }
}

/// A square icon that does something; `name` is its tooltip. `on` holds it
/// raised, like a set switch.
pub fn icon_button(ui: &mut Ui, id: impl Into<Id>, icon: Icon, name: &str, on: bool) -> Response {
    let id: Id = id.into();
    let changed = ui.get(id.clone()).activated();
    let lit = on || ui.state(id.clone()).hover > 0.5;
    let el = stack![glyph(icon, TEXT + TIGHT, ink(lit)).centered()]
        .square(CONTROL)
        .when(on, |e| e.fill(Role::Raised))
        .focusable()
        .a11y(A11y::Button)
        .named(name.to_owned())
        .tip(name.to_owned())
        .id(id);
    Response {
        el: interactive(el),
        changed,
    }
}

/// A view switch: ink with an accent underline when current. The underline
/// eases in, so a switch reads as one motion.
pub fn tab(ui: &mut Ui, id: impl Into<Id>, label: &str, current: bool) -> Response {
    let id: Id = id.into();
    let changed = ui.get(id.clone()).activated();
    let line = if current {
        Fill::from(accent())
    } else {
        Role::Ink.alpha(0.)
    };
    let el = col![
        spacer(),
        body(label.to_owned())
            .text_size(TEXT)
            .fill(ink(current))
            .lines(1),
        spacer(),
        block(Len::Pct(100.), 1).fill(line).animate_with(quick())
    ]
    .align(Align::Center)
    .pad((SPACE, 0))
    .h(CONTROL + TIGHT)
    .shrink(0)
    .focusable()
    .a11y(A11y::Button)
    .named(label.to_owned())
    .id(id);
    Response {
        el: interactive(el),
        changed,
    }
}

/// A small word that latches: Solo, Mute, a filter. Set, it is raised and
/// inked; clear, a dim word on a faint field. `name` is its tooltip.
pub fn latch(ui: &mut Ui, id: impl Into<Id>, label: &str, name: &str, on: bool) -> Response {
    let id: Id = id.into();
    let changed = ui.get(id.clone()).activated();
    let el = row![
        caption(label.to_owned())
            .text_weight(Weight::SEMIBOLD)
            .fill(ink(on))
            .lines(1)
    ]
    .align(Align::Center)
    .justify(Justify::Center)
    .pad((TIGHT * 1.5, 0))
    .min_w(CONTROL * 0.8)
    .h(CONTROL * 0.8)
    .shrink(0)
    .fill(if on { Role::Raised } else { Role::Field })
    .focusable()
    .a11y(A11y::Toggle { on })
    .named(name.to_owned())
    .tip(name.to_owned())
    .id(id);
    Response {
        el: interactive(el),
        changed,
    }
}

/// A dial: a vertical drag sweeps `range` (Shift is fine), a double click
/// puts back `default`. It shows no words; `name` and `readout` are its
/// tooltip, so a strip of them stays quiet until asked.
pub fn knob(
    ui: &mut Ui,
    id: impl Into<Id>,
    (name, readout): (&str, &str),
    value: &mut f64,
    range: RangeInclusive<f64>,
    default: f64,
) -> Response {
    let id: Id = id.into();
    let before = *value;
    ui.drag(id.clone(), value, range.clone(), 120., true);
    if ui.double_click(id.clone()) {
        *value = default;
    }
    let span = range.end() - range.start();
    let t = if span == 0. {
        0.
    } else {
        ((*value - range.start()) / span).clamp(0., 1.)
    };
    let t = ui.tween(id.field("t"), t);
    let hover = ui.state(id.clone()).hover;
    let el = canvas(move |s| {
        let (c, r) = (
            Point::new(s.width / 2., s.height / 2.),
            s.width.min(s.height) / 2. - 2.,
        );
        let sweep = |from: f64, to: f64| {
            let steps = 32;
            Path::polyline(
                (0..=steps).map(|n| {
                    let a = (135. + 270. * (from + (to - from) * f64::from(n) / f64::from(steps)))
                        .to_radians();
                    Point::new(c.x + r * a.cos(), c.y + r * a.sin())
                }),
                false,
            )
        };
        let a = (135. + 270. * t).to_radians();
        vec![
            Draw::stroke(sweep(0., 1.), lift(3), 2.),
            Draw::stroke(
                sweep(0., t),
                Fill::from(Color::oklch(0.78 + 0.1 * hover as f32, 0., 0.)),
                2.,
            ),
            Draw::stroke(
                Path::polyline(
                    [
                        c,
                        Point::new(c.x + r * 0.7 * a.cos(), c.y + r * 0.7 * a.sin()),
                    ],
                    false,
                ),
                Fill::from(Role::Ink),
                1.5,
            ),
        ]
    })
    .square(CONTROL)
    .shrink(0)
    .cursor(Cursor::Hand)
    .focusable()
    .a11y(A11y::Button)
    .named(name.to_owned())
    .tip(format!("{name} {readout}"))
    .id(id);
    Response {
        el,
        changed: *value != before,
    }
}

/// A horizontal fader `width` wide: drag anywhere on it, double click for
/// `default`. Its readout sits beside it, reserved so digits do not shift.
pub fn fader(
    ui: &mut Ui,
    id: impl Into<Id>,
    (name, readout): (&str, &str),
    value: &mut f64,
    range: RangeInclusive<f64>,
    default: f64,
    width: f64,
) -> Response {
    let id: Id = id.into();
    let before = *value;
    ui.drag(id.clone(), value, range.clone(), width, false);
    if ui.double_click(id.clone()) {
        *value = default;
    }
    let span = range.end() - range.start();
    let t = if span == 0. {
        0.
    } else {
        ((*value - range.start()) / span).clamp(0., 1.)
    };
    let t = ui.tween(id.field("t"), t);
    let track = canvas(move |s| {
        let y = s.height / 2.;
        let x = (s.width - 4.) * t + 2.;
        let bar = |x0: f64, x1: f64, h: f64| {
            Path::polyline(
                [(x0, y - h), (x1, y - h), (x1, y + h), (x0, y + h)].map(|(x, y)| Point::new(x, y)),
                true,
            )
        };
        vec![
            Draw::fill(bar(0., s.width, 1.), lift(3)),
            Draw::fill(bar(0., x, 1.), Fill::from(Color::oklch(0.78, 0., 0.))),
            Draw::fill(bar(x - 2., x + 2., 6.), Fill::from(Role::Ink)),
        ]
    })
    .w(width)
    .h(CONTROL)
    .shrink(0)
    .cursor(Cursor::Hand)
    .focusable()
    .named(name.to_owned())
    .tip(name.to_owned())
    .id(id);
    let el = row![
        track,
        caption(readout.to_owned())
            .fill(secondary())
            .reserve("-00.0 dB")
    ]
    .gap(SPACE)
    .align(Align::Center)
    .shrink(0);
    Response {
        el,
        changed: *value != before,
    }
}

/// A one-line field with a dim `hint` while empty and a search glyph.
pub fn field(ui: &mut Ui, id: impl Into<Id>, hint: &str, value: &mut String) -> Response {
    let id: Id = id.into();
    let empty = value.is_empty();
    let edit = text_input(ui, id, value);
    let mut body_layers = vec![edit.el.radius(0.).no_fill().w(Len::Pct(100.))];
    if empty {
        let hint = caption(hint.to_owned()).fill(secondary()).lines(1);
        body_layers.push(
            row![hint]
                .align(Align::Center)
                .pad((TIGHT * 2., 0.))
                .h(Len::Pct(100.)),
        );
    }
    let el = row![
        glyph(Icon::Search, TEXT, secondary()),
        stack(body_layers).grow(1).min_w(0)
    ]
    .align(Align::Center)
    .pad(edges(0., 0., 0., SPACE))
    .h(CONTROL)
    .fill(Role::Field);
    Response {
        el,
        changed: edit.changed,
    }
}

/// A list row: `label` ellipsized, `detail` dim on the right, `mark` a
/// colour edge on the left. Selected rows are raised.
pub fn list_row(
    ui: &mut Ui,
    id: impl Into<Id>,
    (label, detail): (&str, &str),
    mark: Option<Color>,
    selected: bool,
) -> Response {
    let id: Id = id.into();
    let changed = ui.get(id.clone()).activated();
    let edge = block(2, Len::Pct(100.))
        .fill(mark.map_or(Role::Ink.alpha(0.), Fill::from))
        .shrink(0);
    let el = row![
        edge,
        body(label.to_owned())
            .text_size(TEXT)
            .fill(ink(selected))
            .lines(1)
            .min_w(0)
            .grow(1),
        caption(detail.to_owned())
            .fill(secondary())
            .lines(1)
            .shrink(0),
    ]
    .gap(SPACE)
    .align(Align::Center)
    .pad(edges(0., SPACE, 0., 0.))
    .h(CONTROL)
    .w(Len::Pct(100.))
    .when(selected, |e| e.fill(Role::Raised))
    .focusable()
    .a11y(A11y::Button)
    .named(label.to_owned())
    .tip(label.to_owned())
    .id(id);
    Response {
        el: interactive(el),
        changed,
    }
}

/// A popup menu of `items`, `columns` to a row (one for words, more for a
/// grid of short picks); the index of the one picked. `current` is raised.
pub fn menu(
    ui: &mut Ui,
    id: &str,
    items: &[&str],
    columns: usize,
    current: Option<usize>,
) -> Response<Option<usize>> {
    let mut picked = None;
    let mut cells: Vec<El> = items
        .iter()
        .enumerate()
        .map(|(n, item)| {
            let r = button(ui, format!("{id}-{n}"), item, current == Some(n));
            if r.changed {
                picked = Some(n);
            }
            if columns == 1 {
                r.el.justify(Justify::Start).w(Len::Pct(100.))
            } else {
                r.el.w(TEXT * 3.)
            }
        })
        .collect();
    let mut rows = Vec::new();
    while !cells.is_empty() {
        let rest = cells.split_off(columns.clamp(1, cells.len()));
        rows.push(row(std::mem::replace(&mut cells, rest)).gap(1));
    }
    let list = col(rows).gap(1).when(columns == 1, |e| e.w(TEXT * 14.));
    Response {
        el: popup(id, list),
        changed: picked,
    }
}

/// `content` floated over everything, pinned under the element `anchor`
/// names, easing in from just above it.
pub fn popup(anchor: &str, content: El) -> El {
    content
        .fill(lift(2))
        .stroke(hairline())
        .stroke_width(1)
        .pad(TIGHT)
        .float()
        .pin(
            Pin::to(anchor.to_owned())
                .area(Area::Bottom)
                .fallback(Area::Top),
        )
        .appear(Appear::Slide(0., -TIGHT))
        .id(format!("{anchor}-popup"))
}

/// A panel opened on demand over the view: a title, a close button, then
/// `content`. Returns whether it was closed this frame.
pub fn sheet(ui: &mut Ui, id: &str, title_text: &str, content: El) -> Response {
    let close = icon_button(ui, format!("{id}-close"), Icon::Close, "Close", false);
    let closed = close.changed || ui.keys(id).iter().any(|k| k.key == Key::Escape);
    let el = col![
        row![section(title_text), spacer(), close.el].align(Align::Center),
        rule(),
        content.grow(1).min_h(0),
    ]
    .gap(SPACE)
    .pad(INSET)
    .w(TEXT * 36.)
    .max_size((TEXT * 36., TEXT * 40.))
    .fill(lift(1))
    .stroke(hairline())
    .stroke_width(1)
    .focusable()
    .appear(Appear::Slide(0., TEXT))
    .id(id.to_owned());
    Response {
        el,
        changed: closed,
    }
}

/// A level from 0 to 1 as a thin bar, `width` wide.
pub fn meter(level: f64, width: f64) -> El {
    let t = level.clamp(0., 1.);
    row![
        block(0., 3.)
            .grow(t)
            .fill(Fill::from(Color::oklch(0.78, 0., 0.))),
        block(0., 3.).grow(1. - t)
    ]
    .w(width)
    .fill(lift(3))
    .shrink(0)
}

/// A small uppercase title.
pub fn section(label: &str) -> El {
    caption(label.to_uppercase())
        .text_size(SMALL - 1.)
        .fill(secondary())
        .text_weight(Weight::SEMIBOLD)
        .lines(1)
        .min_w(0)
}

/// A strip of controls: a header row or a toolbar.
pub fn strip(items: Vec<El>) -> El {
    row(items)
        .gap(SPACE)
        .align(Align::Center)
        .pad((SPACE, TIGHT))
        .shrink(0)
}

/// A horizontal hairline.
pub fn rule() -> El {
    block(Len::Pct(100.), 1).fill(hairline()).shrink(0)
}

/// A vertical hairline.
pub fn vrule() -> El {
    block(1, Len::Pct(100.)).fill(hairline()).shrink(0)
}

/// Padding per side, in CSS order.
pub fn edges(top: f64, right: f64, bottom: f64, left: f64) -> mui::layout::Insets {
    mui::layout::Insets {
        left,
        right,
        top,
        bottom,
    }
}
