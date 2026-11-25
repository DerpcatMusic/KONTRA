//! The components every view is built from, each written once. They take
//! the [`Ui`], an id and the value they show, and return the element and
//! whether the user acted on it this frame. Hover and press ease through
//! mui's springs; a value never does: a control draws this frame's value.

use std::ops::RangeInclusive;

use mui::prelude::*;

pub use crate::icon::{Icon, glyph};
pub(crate) use kontra_ui_view::rect_path;

use crate::theme::{
    CONTROL, HAIR, INSET, NOTCH, SMALL, SPACE, TEXT, TIGHT, accent, hairline, lift, quick,
    secondary,
};

/// The states every flat control shares: the pointing hand, a faint lift on
/// hover, a deeper one pressed, a 1 px ring for keyboard focus. No easing:
/// a node's paint carries its value too (a latch set, a button on), and a
/// value shows the frame it changes.
pub fn interactive(el: El) -> El {
    el.cursor(Cursor::Hand)
        .on(State::Hover, |s| s.fill(lift(1)))
        .on(State::Press, |s| s.fill(lift(2)))
        .on(State::FocusVisible, |s| {
            s.stroke(Role::Primary.alpha(0.9)).stroke_width(HAIR)
        })
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
        .shrink(0)
        .when(on, |e| e.fill(Role::Raised))
        .focusable()
        .a11y(A11y::Button)
        .named(name.to_owned())
        // No name, no tip: a menu's button while its menu is open.
        .when(!name.is_empty(), |e| e.tip(name.to_owned()))
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
        block(Len::Pct(100.), HAIR).fill(line).animate_with(quick())
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

/// A dial: a vertical drag sweeps `range`, a double click puts back
/// `default`. It shows no words; `name` and `readout` are its tooltip, so a
/// strip of them stays quiet until asked. MUI's K knob (shared with KURV), quiet.
pub fn knob(
    ui: &mut Ui,
    id: impl Into<Id>,
    (name, readout): (&str, &str),
    value: &mut f64,
    range: RangeInclusive<f64>,
    default: f64,
) -> Response {
    let domain = Domain::new(range).default(default).instant();
    let r = mui::widgets::knob(ui, id, name, value, domain);
    Response {
        el: r
            .el
            .quiet()
            .look(mui::widgets::Knob::k())
            .value_text(readout)
            .px(CONTROL / 1.25)
            .el(),
        changed: r.changed,
    }
}

/// A slider `size` big, vertical when taller than wide: a drag anywhere on
/// it sweeps `range`, the arrow keys step it, a double click puts back
/// `default`. A hairline rail, lit from the start, or from the middle with a
/// tick there for a range centred on 0 (pan), and a block thumb. `name` is
/// its tooltip. KONTRA's own slider as of v0.6.20, drawn at this frame's
/// value with no glide.
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
    mui::widgets::stepped(ui, id.as_str(), value, &range);
    if ui.double_click(id.clone()) {
        *value = default;
    }
    let (lo, hi) = (*range.start(), *range.end());
    let t = if hi == lo {
        0.
    } else {
        ((*value - lo) / (hi - lo)).clamp(0., 1.)
    };
    let from: f64 = if lo == -hi { 0.5 } else { 0. };
    let hover = ui.state(id.clone()).hover as f32;
    let el = canvas(move |s| {
        let rect = rect_path;
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
    .a11y(A11y::Slider {
        value: *value,
        min: lo,
        max: hi,
    })
    .named(name.to_owned())
    .tip(name.to_owned())
    .id(id);
    Response {
        el,
        changed: *value != before,
    }
}

/// The loudest a [`wedge`] goes, in dB: its end is twice the amplitude of
/// 0 dB, VLC's 200 %.
pub const WEDGE_TOP_DB: f64 = 6.;
/// The quietest a [`wedge`] reads as a level; below it is silence.
pub const WEDGE_FLOOR_DB: f64 = -60.;

/// Where `db` sits along a [`wedge`], 0..1, by amplitude: 0 dB is the middle.
pub fn wedge_at(db: f64) -> f64 {
    if db <= WEDGE_FLOOR_DB {
        0.
    } else {
        (10f64.powf((db - WEDGE_TOP_DB) / 20.)).clamp(0., 1.)
    }
}

/// The level at `t` along a [`wedge`], in dB.
pub fn wedge_db(t: f64) -> f64 {
    if t <= 0. {
        WEDGE_FLOOR_DB
    } else {
        (20. * t.log10() + WEDGE_TOP_DB).clamp(WEDGE_FLOOR_DB, WEDGE_TOP_DB)
    }
}

/// A level for a [`wedge`]'s readout: its share of 0 dB, and dB.
pub fn wedge_text(db: f64) -> String {
    let percent = (wedge_at(db) * 200.).round();
    if db <= WEDGE_FLOOR_DB {
        format!("{percent:.0}% · -inf dB")
    } else {
        format!("{percent:.0}% · {db:.1} dB")
    }
}

/// A master level VLC's way: a wedge rising left to right, filled from the
/// left up to the level, lit past 100 %. A press or drag sets the level
/// under the pointer, the wheel and arrow keys step it 5 %, a double click
/// puts back 0 dB. `db` runs [`WEDGE_FLOOR_DB`]..[`WEDGE_TOP_DB`].
pub fn wedge(
    ui: &mut Ui,
    id: impl Into<Id>,
    name: &str,
    db: &mut f64,
    (w, h): (f64, f64),
) -> Response {
    let id: Id = id.into();
    let before = *db;
    let mut t = wedge_at(*db);
    let set = t;
    // The press and each move set the level under the pointer; a still
    // hold rewrites nothing, so a double click's 0 dB stays put.
    let r = ui.get(id.clone());
    if (r.pressed || (r.held && r.drag_delta != Vec2::ZERO))
        && let Some(p) = ui.local(id.clone())
    {
        t = (p.x / w).clamp(0., 1.);
    }
    if let Some(turn) = ui.wheel(id.clone()) {
        // Wheel up is a negative y: louder.
        t = (t - turn.y.signum() * WEDGE_STEP).clamp(0., 1.);
    }
    let keyed = t;
    if mui::widgets::stepped(ui, id.as_str(), &mut t, &(0.0..=1.0)) {
        let moved = t - keyed;
        // An arrow's step is finer than the grid: go to the next grid point
        // its way, even from off the grid. Home/End/Page land on the grid.
        let k = keyed / WEDGE_STEP;
        t = if moved.abs() < WEDGE_STEP {
            let next = if moved > 0. {
                k.floor() + 1.
            } else {
                k.ceil() - 1.
            };
            (next * WEDGE_STEP).clamp(0., 1.)
        } else {
            (t / WEDGE_STEP).round() * WEDGE_STEP
        };
    }
    if t != set {
        *db = wedge_db(t);
    }
    // A screen reader's set/increment/decrement arrives in dB.
    if let Some(v) = ui.requested_value(id.clone()) {
        *db = v.clamp(WEDGE_FLOOR_DB, WEDGE_TOP_DB);
        t = wedge_at(*db);
    }
    if ui.double_click(id.clone()) {
        *db = 0.;
        t = wedge_at(0.);
    }
    let hover = ui.state(id.clone()).hover as f32;
    let el = canvas(move |s| {
        let (w, h) = (s.width, s.height);
        // The wedge's height at `x`: nothing at the left, all of it at the right.
        let part = |a: f64, b: f64| {
            let (a, b) = (a * w, b * w);
            Path::polyline(
                [(a, h), (b, h), (b, h - h * b / w), (a, h - h * a / w)]
                    .map(|(x, y)| Point::new(x, y)),
                true,
            )
        };
        let lit = Fill::from(Color::oklch(0.78 + 0.1 * hover, 0., 0.));
        let mut draw = vec![
            Draw::fill(part(0., 1.), lift(3)),
            Draw::fill(part(0., t.min(0.5)), lit),
        ];
        if t > 0.5 {
            draw.push(Draw::fill(part(0.5, t), accent()));
        }
        draw
    })
    .w(w)
    .h(h)
    .shrink(0)
    .cursor(Cursor::Hand)
    .focusable()
    .captures_wheel()
    .a11y(A11y::Slider {
        value: *db,
        min: WEDGE_FLOOR_DB,
        max: WEDGE_TOP_DB,
    })
    .named(name.to_owned())
    .tip(format!("{name} {}", wedge_text(*db)))
    .id(id);
    Response {
        el,
        changed: *db != before,
    }
}

/// A [`wedge`]'s wheel and key step along it: 5 % of its 200 %.
const WEDGE_STEP: f64 = 0.025;

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

/// A one-line field with a dim `hint` while empty, a search glyph, and a
/// clear button while it holds text.
pub fn field(ui: &mut Ui, id: impl Into<Id>, hint: &str, value: &mut String) -> Response<TextEdit> {
    let id: Id = id.into();
    let clear_id = format!("{}-clear", id.as_str());
    let cleared = !value.is_empty() && ui.get(clear_id.as_str()).activated();
    if cleared {
        value.clear();
    }
    let empty = value.is_empty();
    let edit = text_edit(ui, id.clone(), value, TextOpts::default());
    let mut body_layers = vec![
        edit.el
            .radius(0.)
            .no_fill()
            .w(Len::Pct(100.))
            .named(hint.to_owned()),
    ];
    if empty {
        let hint = caption(hint.to_owned()).fill(secondary()).lines(1);
        body_layers.push(
            row![hint]
                .align(Align::Center)
                .pad((TIGHT * 2., 0.))
                .h(Len::Pct(100.)),
        );
    }
    let search_ink = if empty {
        secondary()
    } else {
        Fill::from(Role::Ink)
    };
    let mut parts = vec![
        glyph(Icon::Search, TEXT, search_ink),
        stack(body_layers).grow(1).min_w(0),
    ];
    if !empty {
        let clear = interactive(
            stack![glyph(Icon::Close, SMALL, secondary()).centered()]
                .square(CONTROL - TIGHT)
                .shrink(0)
                .focusable()
                .a11y(A11y::Button)
                .named("Clear search")
                .tip("Clear search")
                .id(clear_id),
        );
        parts.push(clear);
    }
    let el = row(parts)
        .align(Align::Center)
        .pad(edges(0., if empty { 0. } else { TIGHT * 0.5 }, 0., SPACE))
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
    let edge = block(NOTCH, Len::Pct(100.))
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

/// KONTRA's look on MUI's overlays: a lifted, hairlined float of flat rows,
/// a grid of cells when `columns` is above one. The width fits the longest
/// of `items`: a list within 16 to 32 em so a long preset name reads whole,
/// a grid's cells each as wide as its longest pick.
fn overlay(items: &[&str], columns: usize) -> Overlay<'static> {
    let longest = items.iter().map(|i| i.chars().count()).max().unwrap_or(0) as f64;
    let wide = match columns {
        1 => (longest * TEXT * 0.6 + SPACE * 2.).clamp(TEXT * 16., TEXT * 32.),
        n => (longest * TEXT * 0.75 + SPACE * 3.).max(TEXT * 3.) * n as f64,
    };
    Overlay::default()
        .area(Area::Bottom)
        .fallback(Area::Top)
        .gap(0.)
        .frame(&popup_frame)
        .row(if columns == 1 { &list_face } else { &cell_face })
        .row_h(CONTROL + 1.)
        .min_w(wide)
        .columns(columns)
}

/// A menu row: the label to the menu's edge, the current pick raised.
fn list_face(r: MenuRow<'_>) -> El {
    interactive(
        row![
            body(r.label.to_owned())
                .text_size(TEXT)
                .fill(ink(r.selected))
                .lines(1)
                .min_w(0)
        ]
        .center_y()
        .pad(edges(0., SPACE, 0., SPACE))
        .when(r.selected, |e| e.fill(Role::Raised)),
    )
}

/// A grid cell: the label centred, the current pick raised.
fn cell_face(r: MenuRow<'_>) -> El {
    interactive(
        row![
            body(r.label.to_owned())
                .text_size(TEXT)
                .fill(ink(r.selected))
                .lines(1)
                .min_w(0)
        ]
        .center()
        .when(r.selected, |e| e.fill(Role::Raised)),
    )
}

/// A popup menu of `items` under the element `id` names, `columns` to a
/// row (one for words, more for a grid of short picks); the index picked.
/// `current` is raised. The caller opens it with [`toggle`]; Down on `id`
/// enters it.
pub fn menu(
    ui: &mut Ui,
    id: &str,
    items: &[&str],
    columns: usize,
    current: Option<usize>,
) -> Response<Option<usize>> {
    let m = overlay(items, columns).current(current).menu(
        ui,
        format!("{id}-popup"),
        id,
        &mut true,
        items,
    );
    Response {
        el: m.el.unwrap_or_else(|| stack![]),
        changed: m.changed,
    }
}

/// Opens or closes the popup `id` names on `hit`, and closes it on a click
/// away or Escape (seen whatever holds the focus, as a sheet sees it);
/// whether it is open. `open` holds the one open popup.
pub fn toggle(ui: &Ui, open: &mut Option<String>, id: &str, hit: bool) -> bool {
    let was = open.as_deref() == Some(id);
    if hit {
        *open = (!was).then(|| id.to_owned());
    } else if was
        && (ui.dismissed(&[id, &format!("{id}-popup")])
            || ui.shortcuts().iter().any(|k| k.key == Key::Escape))
    {
        *open = None;
    }
    open.as_deref() == Some(id)
}

/// A dropdown: `icon` and the current `label` with a drop arrow; clicked,
/// it floats `items` under itself, a grid when they are many and short. The
/// index picked comes back, and the pick closes it. `name` is its tooltip.
/// `open` holds the one open popup.
pub fn dropdown(
    ui: &mut Ui,
    open: &mut Option<String>,
    id: &str,
    (icon, label, name): (Option<Icon>, &str, &str),
    items: &[&str],
    current: Option<usize>,
) -> Response<Option<usize>> {
    let mut shown = open.as_deref() == Some(id);
    let tint = ink(shown);
    let mut face = Vec::new();
    face.extend(icon.map(|i| glyph(i, TEXT, tint.clone())));
    if !label.is_empty() {
        face.push(caption(label.to_owned()).fill(Role::Ink).lines(1));
    }
    // Vector chevron: KONTRA bundles no icon font, so MUI's `chooser` symbol
    // would shape as `.notdef` tofu boxes.
    let arrow = glyph(if shown { Icon::Up } else { Icon::Down }, SMALL * 0.9, tint);
    face.push(arrow);
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
        .stroke_width(HAIR);
    // The pointer rests on the button it opened: its tip would float over
    // the menu.
    let face = interactive(face).when(!shown, |e| e.tip(name.to_owned()));
    let grid = items.len() > 8 && items.iter().all(|i| i.chars().count() <= 4);
    let d = overlay(items, if grid { 4 } else { 1 })
        .dropdown(ui, id, name, face, &mut shown, items, current);
    if shown {
        *open = Some(id.to_owned());
    } else if open.as_deref() == Some(id) {
        *open = None;
    }
    Response {
        el: d.el,
        changed: d.changed,
    }
}

/// An icon button opening a menu of titled sections, each `(title, items,
/// current)`; returns `(section, item)` picked. It stays open across picks so
/// several sections can be set at once; a click away or Escape closes it.
/// `active` raises the face while a non-default pick is set. The rows are
/// `"{id}/{section}/{item}"`.
pub fn sectioned_menu(
    ui: &mut Ui,
    open: &mut Option<String>,
    id: &str,
    (icon, name): (Icon, &str),
    sections: &[(&str, &[&str], usize)],
    active: bool,
) -> Response<Option<(usize, usize)>> {
    let mut shown = open.as_deref() == Some(id);
    if ui.get(id).activated() {
        shown = !shown;
    }
    let mut changed = None;
    let mut body = Vec::new();
    for (s, &(title, items, current)) in sections.iter().enumerate() {
        body.push(caption(title.to_owned()).fill(secondary()).pad(edges(
            if s == 0 { 0. } else { TIGHT },
            SPACE,
            TIGHT * 0.5,
            SPACE,
        )));
        for (i, &item) in items.iter().enumerate() {
            let row_id = format!("{id}/{s}/{i}");
            if ui.get(row_id.as_str()).activated() {
                changed = Some((s, i));
            }
            body.push(
                list_face(MenuRow {
                    label: item,
                    index: i,
                    selected: i == current,
                })
                .h(CONTROL)
                .w(Len::Pct(100.))
                .focusable()
                .a11y(A11y::Button)
                .named(item.to_owned())
                .id(row_id),
            );
        }
    }
    let longest = sections
        .iter()
        .flat_map(|s| s.1)
        .map(|i| i.chars().count())
        .max()
        .unwrap_or(0);
    let pop = Overlay::default()
        .area(Area::Bottom)
        .fallback(Area::Top)
        .gap(0.)
        .frame(&popup_frame)
        .popover(
            ui,
            format!("{id}/menu"),
            id,
            &mut shown,
            col(body).w(longest as f64 * TEXT * 0.6 + SPACE * 4.),
        );
    let tint = ink(active || shown);
    let face = stack![glyph(icon, TEXT + TIGHT, tint).centered()]
        .square(CONTROL)
        .shrink(0)
        .when(active, |e| e.fill(Role::Raised))
        .focusable()
        .a11y(A11y::Button)
        .named(name.to_owned())
        .when(!shown, |e| e.tip(name.to_owned()))
        .id(id.to_owned());
    if shown {
        *open = Some(id.to_owned());
    } else if open.as_deref() == Some(id) {
        *open = None;
    }
    let face = interactive(face);
    Response {
        el: match pop {
            Some(pop) => stack![face, pop],
            None => face,
        },
        changed,
    }
}

/// A float's frame: lifted and hairlined. It opens and closes on the next
/// frame, with no slide or fade.
fn popup_frame(content: El) -> El {
    content
        .fill(lift(2))
        .stroke(hairline())
        .stroke_width(HAIR)
        .pad(TIGHT)
}

/// `content` floated over everything, pinned under the element `anchor`
/// names. It keeps the wheel from what lies under it (a knob, the rack); a
/// node that keeps the wheel does not scroll itself, so scrolling `content`
/// nests its scroller inside.
pub fn popup(anchor: &str, content: El) -> El {
    popup_frame(content)
        .captures_wheel()
        .float()
        .pin(
            Pin::to(anchor.to_owned())
                .area(Area::Bottom)
                .fallback(Area::Top),
        )
        .id(format!("{anchor}-popup"))
}

/// A panel opened on demand over the view: a title, a close button, then
/// `content`. Like a popup it keeps the wheel from what lies under it.
/// Returns whether it was closed this frame.
pub fn sheet(ui: &mut Ui, id: &str, title_text: &str, content: El) -> Response {
    let close = icon_button(ui, format!("{id}-close"), Icon::Close, "Close", false);
    // MUI drops focus on Escape before it hands the keys out, so only the
    // whole stream sees it: the sheet's own `keys` never would.
    let closed = close.changed || ui.shortcuts().iter().any(|k| k.key == Key::Escape);
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
    .stroke_width(HAIR)
    .captures_wheel()
    .focusable()
    .named(title_text.to_owned())
    .appear(Appear::Slide(0., TEXT))
    .id(id.to_owned());
    Response {
        el,
        changed: closed,
    }
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
    block(Len::Pct(100.), HAIR).fill(hairline()).shrink(0)
}

/// A vertical hairline.
pub fn vrule() -> El {
    block(HAIR, Len::Pct(100.)).fill(hairline()).shrink(0)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// What `under` read of a wheel turned over it for a few frames, with
    /// a long menu open on top of it or none, and how far the menu scrolled.
    fn wheel_under(with_menu: bool) -> (Option<Vec2>, f64) {
        let names: Vec<String> = (0..100).map(|n| format!("p{n}")).collect();
        let items: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut ui = Ui::default();
        let mut read = None;
        for n in 0..8 {
            read = read.or(ui.wheel("under"));
            let mut top = vec![block(20., 20.).id("m")];
            if with_menu {
                top.push(menu(&mut ui, "m", &items, 1, None).el);
            }
            let el = stack![block(900., 900.).id("under"), col(top)];
            let input = Input {
                pointer: PointerInput {
                    pos: Some(Point::new(450., 200.)),
                    ..Default::default()
                },
                wheel: if n >= 2 {
                    Vec2::new(0., 30.)
                } else {
                    Vec2::ZERO
                },
                ..Default::default()
            };
            assert!(
                ui.frame(el, Some(Size::new(900., 900.)), input, 1. / 60.)
                    .is_ok()
            );
        }
        (read, ui.scroll("m-popup/rows")[1])
    }

    /// Down from the anchor walks a long menu past its fold, and the list
    /// scrolls so the focused row stays in view.
    #[test]
    fn down_keys_walk_a_long_menu_and_keep_the_row_in_view() {
        let names: Vec<String> = (0..100).map(|n| format!("p{n}")).collect();
        let items: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut ui = Ui::default();
        ui.focus("m");
        for n in 0..40 {
            let m = menu(&mut ui, "m", &items, 1, None);
            let el = col![block(20., 20.).id("m"), m.el];
            let keys = if n >= 2 {
                vec![KeyPress {
                    key: Key::Down,
                    mods: Mods::default(),
                }]
            } else {
                Vec::new()
            };
            let input = Input {
                keys,
                ..Default::default()
            };
            assert!(
                ui.frame(el, Some(Size::new(400., 900.)), input, 1. / 60.)
                    .is_ok()
            );
        }
        let focused = ui.focus_key().map(str::to_owned).unwrap_or_default();
        let row: usize = focused.trim_start_matches("m-popup/").parse().unwrap_or(0);
        let top = row as f64 * (CONTROL + 1.);
        let at = ui.scroll("m-popup/rows")[1];
        assert!(at > 0., "walked past the fold: {focused}");
        assert!(at <= top && top < at + 900., "{top} in view of {at}");
    }

    /// Escape closes a sheet whatever holds the focus (here a field in it).
    #[test]
    fn escape_closes_a_sheet() {
        let mut ui = Ui::default();
        ui.focus("field");
        let mut closed = false;
        for n in 0..4 {
            let r = sheet(
                &mut ui,
                "s",
                "Settings",
                block(20., 20.).focusable().id("field"),
            );
            closed |= r.changed;
            let keys = match n {
                1 => vec![KeyPress {
                    key: Key::Escape,
                    mods: Mods::default(),
                }],
                _ => Vec::new(),
            };
            let input = Input {
                keys,
                ..Default::default()
            };
            assert!(
                ui.frame(r.el, Some(Size::new(900., 900.)), input, 1. / 60.)
                    .is_ok()
            );
        }
        assert!(closed);
    }

    /// Escape still closes a sheet right after a pick from a menu in it,
    /// when the focus sat on the picked row that is now gone.
    #[test]
    fn escape_closes_a_sheet_after_a_menu_pick() {
        let mut ui = Ui::default();
        let mut open = Some("dd".to_owned());
        let mut closed = false;
        for n in 0..6 {
            let d = dropdown(
                &mut ui,
                &mut open,
                "dd",
                (None, "Fit", "Size"),
                &["Fit", "200%"],
                Some(0),
            );
            let r = sheet(&mut ui, "s", "Settings", d.el);
            closed |= r.changed;
            let key = |key| {
                vec![KeyPress {
                    key,
                    mods: Mods::default(),
                }]
            };
            let keys = match n {
                1 => {
                    ui.focus("dd/list/1");
                    key(Key::Enter)
                }
                3 => key(Key::Escape),
                _ => Vec::new(),
            };
            let input = Input {
                keys,
                ..Default::default()
            };
            assert!(
                ui.frame(r.el, Some(Size::new(900., 900.)), input, 1. / 60.)
                    .is_ok()
            );
        }
        assert_eq!(open, None, "the pick closed the menu");
        assert!(closed);
    }

    #[test]
    fn the_wheel_over_a_popup_scrolls_it_and_never_reaches_what_lies_under_it() {
        assert!(wheel_under(false).0.is_some(), "without a menu it does");
        let (read, scrolled) = wheel_under(true);
        assert_eq!(read, None);
        assert!(scrolled > 0., "the menu scrolls instead");
    }
}
