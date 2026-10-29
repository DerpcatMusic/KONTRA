//! The components every view is built from, each written once. They take
//! the [`Ui`], an id and the value they show, and return the element and
//! whether the user acted on it this frame. Hover, press and value changes
//! ease through mui's springs, so motion costs nothing once it settles.

use std::ops::RangeInclusive;

use mui::prelude::*;

pub use crate::icon::{Icon, glyph};

use crate::theme::{
    CONTROL, INSET, SMALL, SPACE, TEXT, TIGHT, accent, hairline, lift, quick, secondary,
};

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
    // Warnings are the one amber on screen.
    let tint = if icon == Icon::Warning {
        Fill::from(Role::Warning)
    } else {
        ink(lit)
    };
    let el = stack![glyph(icon, TEXT + TIGHT, tint).centered()]
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

/// A slider `size` big, vertical when taller than wide: a drag anywhere on
/// it sweeps `range`, a double click puts back `default`. A range centred
/// on 0 (pan) fills from the middle; others fill from the start. `name` is
/// its tooltip.
pub fn slider(
    ui: &mut Ui,
    id: impl Into<Id>,
    name: &str,
    value: &mut f64,
    range: RangeInclusive<f64>,
    default: f64,
    (w, h): (f64, f64),
) -> Response {
    let id: Id = id.into();
    let before = *value;
    let vertical = h > w;
    ui.drag(id.clone(), value, range.clone(), w.max(h), vertical);
    if ui.double_click(id.clone()) {
        *value = default;
    }
    let (lo, hi) = (*range.start(), *range.end());
    let at = |v: f64| {
        if hi == lo {
            0.
        } else {
            ((v - lo) / (hi - lo)).clamp(0., 1.)
        }
    };
    let t = ui.tween(id.field("t"), at(*value));
    let from: f64 = if lo == -hi { 0.5 } else { 0. };
    let hover = ui.state(id.clone()).hover as f32;
    let el = canvas(move |s| {
        let rect = |x0: f64, y0: f64, x1: f64, y1: f64| {
            Path::polyline(
                [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].map(|(x, y)| Point::new(x, y)),
                true,
            )
        };
        // Along the travel: 0 at the start (left, or bottom), 1 at the end.
        let (len, mid) = if vertical {
            (s.height - 4., s.width / 2.)
        } else {
            (s.width - 4., s.height / 2.)
        };
        let pos = |t: f64| 2. + len * if vertical { 1. - t } else { t };
        let bar = |a: f64, b: f64, half: f64| {
            let (a, b) = (pos(a).min(pos(b)), pos(a).max(pos(b)));
            if vertical {
                rect(mid - half, a, mid + half, b)
            } else {
                rect(a, mid - half, b, mid + half)
            }
        };
        let lit = Fill::from(Color::oklch(0.78 + 0.1 * hover, 0., 0.));
        let mut draw = vec![
            Draw::fill(bar(0., 1., 1.), lift(3)),
            Draw::fill(bar(from.min(t), from.max(t), 1.), lit),
        ];
        if from > 0. {
            draw.push(Draw::fill(
                bar(0.5 - 0.5 / len, 0.5 + 0.5 / len, 4.),
                lift(4),
            ));
        }
        let x = pos(t);
        draw.push(Draw::fill(
            if vertical {
                rect(mid - 5., x - 2., mid + 5., x + 2.)
            } else {
                rect(x - 2., mid - 5., x + 2., mid + 5.)
            },
            Fill::from(Color::oklch(0.85, 0., 0.)),
        ));
        draw
    })
    .w(w)
    .h(h)
    .shrink(0)
    .cursor(Cursor::Hand)
    .focusable()
    .named(name.to_owned())
    .tip(name.to_owned())
    .id(id);
    Response {
        el,
        changed: *value != before,
    }
}

/// A number with its icon that a horizontal drag nudges and a double click
/// resets, for values set once and left: a part's tune.
pub fn nudge(
    ui: &mut Ui,
    id: impl Into<Id>,
    (icon, name): (Icon, &str),
    value: &mut f64,
    range: RangeInclusive<f64>,
    default: f64,
    readout: &str,
) -> Response {
    let id: Id = id.into();
    let before = *value;
    ui.drag(id.clone(), value, range, TEXT * 40., false);
    if ui.double_click(id.clone()) {
        *value = default;
    }
    let lit = ui.state(id.clone()).hover > 0.5;
    let el = row![
        glyph(icon, TEXT, ink(lit)),
        caption(readout.to_owned())
            .fill(Role::Ink)
            .reserve("-00.00"),
    ]
    .gap(TIGHT)
    .align(Align::Center)
    .h(CONTROL)
    .shrink(0)
    .cursor(Cursor::ResizeH)
    .focusable()
    .named(name.to_owned())
    .tip(format!(
        "{name} {readout} · drag to change, double click to reset"
    ))
    .id(id);
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
        rows.push(row(std::mem::replace(&mut cells, rest)).gap(1).shrink(0));
    }
    // Floats are sized by the space around their parent; a menu takes its
    // own height so a long one is never squeezed.
    let tall = rows.len() as f64 * (CONTROL + 1.);
    let list = col(rows)
        .gap(1)
        .min_h(tall)
        .when(columns == 1, |e| e.w(TEXT * 16.));
    Response {
        el: popup(id, list),
        changed: picked,
    }
}

/// Opens or closes the popup `id` names on `hit`, and closes it on a click
/// away or Escape; whether it is open. `open` holds the one open popup.
pub fn toggle(ui: &Ui, open: &mut Option<String>, id: &str, hit: bool) -> bool {
    let was = open.as_deref() == Some(id);
    if hit {
        *open = (!was).then(|| id.to_owned());
    } else if was && ui.dismissed(&[id, &format!("{id}-popup")]) {
        *open = None;
    }
    open.as_deref() == Some(id)
}

/// A dropdown: `icon` and the current `label` with a chevron; clicked, it
/// floats `items` under itself, a grid when they are many and short. The
/// index picked comes back, and the pick closes it. `name` is its tooltip.
pub fn dropdown(
    ui: &mut Ui,
    open: &mut Option<String>,
    id: &str,
    (icon, label, name): (Option<Icon>, &str, &str),
    items: &[&str],
    current: Option<usize>,
) -> Response<Option<usize>> {
    let hit = ui.get(id).activated();
    let shown = toggle(ui, open, id, hit);
    let mut face = Vec::new();
    face.extend(icon.map(|i| glyph(i, TEXT, secondary())));
    face.push(caption(label.to_owned()).fill(Role::Ink).lines(1));
    face.push(glyph(Icon::Down, TEXT * 0.75, secondary()));
    let face = row(face)
        .gap(TIGHT)
        .align(Align::Center)
        .pad((TIGHT * 1.5, 0))
        .h(CONTROL * 0.9)
        .shrink(0)
        .stroke(if shown {
            hairline()
        } else {
            Role::Ink.alpha(0.)
        })
        .stroke_width(1)
        .focusable()
        .a11y(A11y::Button)
        .named(name.to_owned())
        .tip(name.to_owned())
        .id(id.to_owned());
    let mut el = vec![interactive(face)];
    let mut picked = None;
    if shown {
        let grid = items.len() > 8 && items.iter().all(|i| i.chars().count() <= 4);
        let m = menu(ui, id, items, if grid { 4 } else { 1 }, current);
        picked = m.changed;
        if picked.is_some() {
            *open = None;
        }
        el.push(m.el);
    }
    Response {
        el: row(el).shrink(0),
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
