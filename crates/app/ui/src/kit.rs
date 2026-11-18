//! The components every view is built from, each written once. They take
//! the [`Ui`], an id and the value they show, and return the element and
//! whether the user acted on it this frame. Hover, press and value changes
//! ease through mui's springs, so motion costs nothing once it settles.

use std::ops::RangeInclusive;

use mui::prelude::*;

pub use crate::icon::{Icon, glyph};
pub(crate) use kontra_ui_view::rect_path;

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

/// A dial: a vertical drag sweeps `range`, a double click puts back
/// `default`. It shows no words; `name` and `readout` are its tooltip, so a
/// strip of them stays quiet until asked. MUI's knob, quiet.
pub fn knob(
    ui: &mut Ui,
    id: impl Into<Id>,
    (name, readout): (&str, &str),
    value: &mut f64,
    range: RangeInclusive<f64>,
    default: f64,
) -> Response {
    let r = mui::widgets::knob(ui, id, name, value, Domain::new(range).default(default));
    Response {
        el: r.el.quiet().value_text(readout).px(CONTROL / 1.25).el(),
        changed: r.changed,
    }
}

/// A slider `size` big, vertical when taller than wide: a drag anywhere on
/// it sweeps `range`, a double click puts back `default`. A range centred
/// on 0 (pan) fills from the middle. `name` is its tooltip. MUI's slider,
/// quiet.
pub fn slider(
    ui: &mut Ui,
    id: impl Into<Id>,
    name: &str,
    value: &mut f64,
    range: RangeInclusive<f64>,
    default: f64,
    (w, h): (f64, f64),
) -> Response {
    let r = mui::widgets::slider(ui, id, name, value, Domain::new(range).default(default));
    let control = if h > w { r.el.vertical() } else { r.el };
    Response {
        el: control
            .quiet()
            .px(w.min(h) / LANE_SHARE)
            .el()
            .w(w)
            .h(h)
            .shrink(0),
        changed: r.changed,
    }
}

/// MUI sizes a slider's lane as this share of the control's height.
const LANE_SHARE: f64 = 0.45;

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
pub fn field(ui: &mut Ui, id: impl Into<Id>, hint: &str, value: &mut String) -> Response<TextEdit> {
    let id: Id = id.into();
    let empty = value.is_empty();
    let edit = text_edit(ui, id, value, TextOpts::default());
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
    let mut face = Vec::new();
    face.extend(icon.map(|i| glyph(i, TEXT, secondary())));
    face.push(caption(label.to_owned()).fill(Role::Ink).lines(1));
    let face = chooser(row(face).gap(TIGHT).center_y(), shown)
        .pad((TIGHT * 1.5, 0))
        .h(CONTROL * 0.9)
        .shrink(0)
        .stroke(if shown {
            hairline()
        } else {
            Role::Ink.alpha(0.)
        })
        .stroke_width(1);
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

/// A float's frame: lifted, hairlined, easing in from just above.
fn popup_frame(content: El) -> El {
    content
        .fill(lift(2))
        .stroke(hairline())
        .stroke_width(1)
        .pad(TIGHT)
        .appear(Appear::Slide(0., -TIGHT))
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
    .stroke_width(1)
    .captures_wheel()
    .focusable()
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
