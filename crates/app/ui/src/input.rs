//! A library's own controls under the pointer and the keys, one model for
//! every format: a continuous control drags from where it was pressed along
//! the axis and sweep its translator set ([`model::Drag`]; Shift is fine),
//! the wheel and the arrow keys step it, Ctrl or Cmd with a click puts back
//! its default, a double click on a value edit types a number; a button or
//! switch flips at the edge its source asks; a table sets the cells the
//! pointer crosses; a menu floats its items. What the user asked comes back
//! as [`Edit`]s for the host to write into the engine; the caller shows them
//! at once with [`apply`]. A text edit takes typed text the same way.

use kontra_ui_model::{self as model, Interface, Kind, Orientation, PageRef, Sweep, WidgetRef};
use mui::prelude::*;

use crate::kit;
use crate::theme::TEXT;

/// One control set by the user, and the modifier keys held at the click
/// (a script reads them as `$CONTROL_PAR_KEY_*`).
#[derive(Clone, Debug, PartialEq)]
pub struct Edit {
    pub widget: WidgetRef,
    pub to: Set,
    pub keys: model::UiModifiers,
}

/// What an [`Edit`] sets.
#[derive(Clone, Debug, PartialEq)]
pub enum Set {
    /// The control's value.
    Value(f64),
    /// One zero-based cell of a table.
    Cell(u32, f64),
    /// A text edit's text.
    Text(String),
}

/// A page's gesture in flight: the control pressed and its value then, the
/// menu floated open, and what is being typed into a value or text edit.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Gesture {
    press: Option<(usize, f64)>,
    pub menu: Option<usize>,
    /// The control typed into, the text so far, and whether it has been
    /// selected whole (once the field is on screen).
    entry: Option<(usize, String, bool)>,
}

/// The id of widget `n`'s surface on face `face` (a rack part): every
/// face's ids are its own, so two open parts never share one.
pub fn surface(face: usize, n: usize) -> String {
    format!("kontra-w-{face}-{n}")
}

/// Whether a widget of `kind` takes the pointer.
pub(crate) fn takes_input(w: &model::Widget) -> bool {
    w.enabled
        && matches!(
            w.kind,
            Kind::Knob { .. }
                | Kind::Slider { .. }
                | Kind::ValueEdit { .. }
                | Kind::Button { .. }
                | Kind::Switch
                | Kind::Menu { .. }
                | Kind::Table { .. }
                | Kind::TextEdit
        )
}

/// A widget's value as a number.
pub fn value(w: &model::Widget) -> f64 {
    match w.value {
        Some(model::Value::Integer(v)) => f64::from(v),
        Some(model::Value::Real(v)) => v,
        _ => w.initial_value,
    }
}

/// Cell `k` of a table's value.
fn cell(w: &model::Widget, k: u32) -> Option<f64> {
    let k = k as usize;
    match &w.value {
        Some(model::Value::Integers(v)) => v.get(k).map(|&v| f64::from(v)),
        Some(model::Value::Reals(v)) => v.get(k).copied(),
        _ => None,
    }
}

/// Which way a continuous control drags, and how many view points sweep
/// its range. A source that sets no [`model::Drag`] gets the renderer's: a
/// slider sweeps in its own length along its orientation, anything else in
/// 200 points up.
fn travel(w: &model::Widget, r: model::Rect, span: f64, scale: f64) -> (bool, f64) {
    let drag = w.drag.unwrap_or(match &w.kind {
        Kind::Slider { orientation, .. } => model::Drag {
            axis: if *orientation == Orientation::Vertical || r.height > r.width {
                Orientation::Vertical
            } else {
                Orientation::Horizontal
            },
            sweep: Sweep::Lengths(1.),
        },
        _ => model::Drag {
            axis: Orientation::Vertical,
            sweep: Sweep::Points(200.),
        },
    });
    let vertical = drag.axis == Orientation::Vertical;
    let length = if vertical { r.height } else { r.width }.max(1.);
    let points = match drag.sweep {
        Sweep::Points(p) => p,
        Sweep::Lengths(k) => length * k,
        Sweep::PerUnit(p) => p * span.abs(),
    };
    (vertical, points.max(1.) * scale)
}

fn range(kind: &Kind) -> Option<model::Range> {
    match kind {
        Kind::Knob { range, .. } | Kind::Slider { range, .. } | Kind::ValueEdit { range, .. } => {
            Some(*range)
        }
        _ => None,
    }
}

/// `v` inside `range`, on its step grid.
fn snap(range: model::Range, v: f64) -> f64 {
    let (lo, hi) = (range.min.min(range.max), range.max.max(range.min));
    let v = v.clamp(lo, hi);
    range.step.filter(|s| *s > 0.).map_or(v, |s| {
        // The nearest grid point inside: a top off the grid snaps below it.
        let (first, last) = ((lo / s).ceil(), (hi / s).floor());
        if first > last {
            v
        } else {
            (v / s).round().clamp(first, last) * s
        }
    })
}

/// How far one arrow key moves a control: a value edit by its step, or
/// one unit without one (as its arrows do), anything else by a hundredth
/// of its range or its step when coarser.
fn notch(kind: &Kind, range: model::Range) -> f64 {
    let step = range.step.filter(|s| *s > 0.);
    match kind {
        Kind::ValueEdit { .. } => step.unwrap_or(1.),
        _ => step.unwrap_or(0.).max((range.max - range.min).abs() / 100.),
    }
}

/// How far a press on a value edit's arrows steps it: one shown unit, up in
/// the upper half of its right 14 px, down in the lower (Kontakt 8.13,
/// measured; one callback a step, none on release).
// ponytail: one step a press; Kontakt repeats a held arrow (0.4 s, then
// 0.15 s a step), which needs a frame clock the input pass doesn't have.
fn arrow(ui: &Ui, id: &str, w: &model::Widget, scale: f64) -> Option<f64> {
    let Kind::ValueEdit {
        arrows: true,
        display,
        ..
    } = &w.kind
    else {
        return None;
    };
    let size = ui.scene()?.surface(id)?.frame.size;
    let at = ui.local(id)?;
    let unit = if display.ratio == 0. {
        1.
    } else {
        display.ratio
    };
    (at.x >= size.width - 14. * scale).then_some(if at.y < size.height / 2. { unit } else { -unit })
}

/// This frame's edits to the controls of `page` drawn at `scale`, and the
/// open menu's or typed entry's popup, to float over the page.
pub fn read(
    ui: &mut Ui,
    face: usize,
    tree: &Interface,
    page: PageRef,
    scale: f64,
    g: &mut Gesture,
) -> (Vec<Edit>, Option<El>) {
    let mut edits = Vec::new();
    for n in tree.draw_order(page) {
        let Some(w) = tree.widgets.get(n.0).filter(|w| takes_input(w)) else {
            continue;
        };
        if !tree.visible(n) {
            continue;
        }
        let id = surface(face, n.0);
        let r = ui.get(id.clone());
        // Menus and value edits don't report keys (KSP manual, KEY_SHIFT).
        let keys = match w.kind {
            Kind::Menu { .. } | Kind::ValueEdit { .. } => model::UiModifiers::default(),
            _ => model::UiModifiers {
                shift: r.press_mods.shift,
                ctrl: r.press_mods.ctrl,
                alt: r.press_mods.alt,
                cmd: r.press_mods.cmd,
            },
        };
        let edit = |to| Edit {
            widget: n,
            to,
            keys,
        };
        if matches!(w.kind, Kind::Table { .. }) {
            if r.held {
                edits.extend(cells(ui, &id, r.drag_delta, w, &edit));
            }
            continue;
        }
        let now = value(w);
        let before = edits.len();
        let mut set = |v: f64| {
            if v != now {
                edits.push(edit(Set::Value(v)));
            }
        };
        if let Some(range) = range(&w.kind) {
            let span = range.max - range.min;
            let (vertical, points) = travel(w, tree.page_rect(n), span, scale);
            let along = |d: Vec2| if vertical { -d.y } else { d.x };
            let reset = r.press_mods.ctrl || r.press_mods.cmd;
            let arrow = r.pressed.then(|| arrow(ui, &id, w, scale)).flatten();
            if let Some(unit) = arrow {
                set(snap(range, now + unit));
            }
            if r.pressed && arrow.is_none() {
                g.press = Some((n.0, now));
                if reset {
                    set(snap(range, range.default));
                }
            }
            match g.press {
                Some((p, start)) if p == n.0 && r.held && !reset => {
                    // Sub-pixel: the pointer's travel is a float, so a
                    // high-resolution mouse or trackpad moves it finely.
                    let fine = if r.press_mods.shift { 0.1 } else { 1. };
                    set(snap(
                        range,
                        start + along(r.drag_total) / points * span * fine,
                    ));
                }
                _ => {}
            }
            // Claimed, so the rack under the knob does not scroll with it.
            if let Some(wheel) = ui.wheel(id.as_str()).filter(|_| !r.held) {
                set(snap(range, now - wheel.y / points * span));
            }
            // Keyboard focus steps it, the way the wheel does.
            let step = ui.keys(id.as_str()).iter().fold(0., |at, k| match k.key {
                Key::Up | Key::Right => at + 1.,
                Key::Down | Key::Left => at - 1.,
                _ => at,
            });
            if step != 0. {
                set(snap(range, now + step * notch(&w.kind, range)));
            }
            if matches!(w.kind, Kind::ValueEdit { .. })
                && arrow.is_none()
                && ui.double_click(id.as_str())
            {
                let shown = match &w.kind {
                    Kind::ValueEdit { display, .. } if display.ratio != 0. => now / display.ratio,
                    _ => now,
                };
                type_into(ui, g, n.0, &id, shown.to_string());
            }
            if r.released && g.press.is_some_and(|(p, _)| p == n.0) {
                g.press = None;
                // The source runs its callback on release, moved or not.
                if w.reports_release && edits.len() == before {
                    edits.push(edit(Set::Value(now)));
                }
            }
            continue;
        }
        match &w.kind {
            Kind::Button { momentary: true } => {
                if r.pressed {
                    set(1.);
                } else if r.released {
                    set(0.);
                }
            }
            Kind::Button { .. } | Kind::Switch => {
                let hit = if w.activate_on_press {
                    r.pressed
                } else {
                    r.clicked
                };
                if hit || r.key_activated {
                    set(if now != 0. { 0. } else { 1. });
                }
            }
            Kind::Menu { .. } if r.clicked || r.key_activated => {
                g.menu = (g.menu != Some(n.0)).then_some(n.0);
            }
            Kind::TextEdit if (r.clicked || r.key_activated) && g.entry.is_none() => {
                type_into(ui, g, n.0, &id, w.text.clone());
            }
            _ => {}
        }
    }
    let popup =
        menu(ui, face, tree, g, &mut edits).or_else(|| entry(ui, face, tree, g, &mut edits));
    (edits, popup)
}

/// The cells of a table the pointer crossed this frame, set to its height
/// there: a fast drag fills every column between, not only where it landed.
fn cells(
    ui: &Ui,
    id: &str,
    delta: Vec2,
    w: &model::Widget,
    edit: &dyn Fn(Set) -> Edit,
) -> Vec<Edit> {
    let Kind::Table {
        columns,
        range,
        steps_shown,
        ..
    } = &w.kind
    else {
        return Vec::new();
    };
    let (columns, range) = (*columns, *range);
    let shown = steps_shown
        .filter(|&s| s > 0)
        .unwrap_or(columns)
        .min(columns);
    let size = ui.scene().and_then(|s| s.surface(id)).map(|s| s.frame.size);
    let (Some(pos), Some(size)) = (ui.local(id), size) else {
        return Vec::new();
    };
    if shown == 0 || size.width <= 0. || size.height <= 0. {
        return Vec::new();
    }
    let at = |x: f64| ((x / size.width * f64::from(shown)).floor().max(0.) as u32).min(shown - 1);
    let height = |y: f64| {
        snap(
            range,
            range.max - (y / size.height).clamp(0., 1.) * (range.max - range.min),
        )
    };
    let from = pos - delta;
    let (first, last) = (at(from.x), at(pos.x));
    let count = first.abs_diff(last);
    (0..=count)
        .filter_map(|step| {
            let k = if count == 0 {
                1.
            } else {
                f64::from(step) / f64::from(count)
            };
            let c = if last >= first {
                first + step
            } else {
                first - step
            };
            let v = height(from.y + (pos.y - from.y) * k);
            (cell(w, c) != Some(v)).then(|| edit(Set::Cell(c, v)))
        })
        .collect()
}

/// The open menu's popup, and the item picked from it.
fn menu(
    ui: &mut Ui,
    face: usize,
    tree: &Interface,
    g: &mut Gesture,
    edits: &mut Vec<Edit>,
) -> Option<El> {
    let m = g.menu?;
    let w = tree.widgets.get(m)?;
    let Kind::Menu { items } = &w.kind else {
        return None;
    };
    let id = surface(face, m);
    if ui.dismissed(&[&id, &format!("{id}-popup")]) {
        g.menu = None;
        return None;
    }
    let shown: Vec<&model::MenuItem> = items.iter().filter(|i| i.visible).collect();
    let labels: Vec<&str> = shown.iter().map(|i| i.text.as_str()).collect();
    let now = value(w) as i32;
    let current = shown.iter().position(|i| i.value == now);
    let menu = kit::menu(ui, &id, &labels, 1, current);
    if let Some(item) = menu.changed.and_then(|k| shown.get(k)) {
        edits.push(Edit {
            widget: WidgetRef(m),
            to: Set::Value(f64::from(item.value)),
            keys: model::UiModifiers::default(),
        });
        g.menu = None;
    }
    Some(menu.el)
}

/// Opens typed entry on control `n` (surface `id`), starting from `text`
/// selected whole, so what the user types replaces it.
fn type_into(ui: &mut Ui, g: &mut Gesture, n: usize, id: &str, text: String) {
    ui.focus(format!("{id}-entry"));
    g.entry = Some((n, text, false));
}

/// What is being typed into a value edit (a number in its shown units) or
/// a text edit: Enter sets it, Escape or a click elsewhere drops it.
fn entry(
    ui: &mut Ui,
    face: usize,
    tree: &Interface,
    g: &mut Gesture,
    edits: &mut Vec<Edit>,
) -> Option<El> {
    let (m, mut text, mut selected) = g.entry.take()?;
    let w = tree.widgets.get(m)?;
    let id = surface(face, m);
    let field = format!("{id}-entry");
    if ui.dismissed(&[&id, &format!("{id}-popup")])
        || ui.keys(field.as_str()).iter().any(|k| k.key == Key::Escape)
    {
        return None;
    }
    if !selected {
        let all = SemanticAction::set_selection(field.as_str(), 0, text.chars().count());
        selected = ui.request_action(all);
    }
    let typed = text_edit(ui, field.as_str(), &mut text, TextOpts::default());
    if !typed.changed.submitted {
        g.entry = Some((m, text, selected));
        // In a row: the popup names its content, and the field keeps its own id.
        return Some(kit::popup(&id, row![typed.el.w(TEXT * 6.)]));
    }
    ui.blur();
    let set = |to| Edit {
        widget: WidgetRef(m),
        to,
        keys: model::UiModifiers::default(),
    };
    match &w.kind {
        Kind::ValueEdit { range, display, .. } => {
            let ratio = if display.ratio == 0. {
                1.
            } else {
                display.ratio
            };
            let typed = text.trim().parse::<f64>().ok().filter(|v| v.is_finite());
            edits.extend(typed.map(|v| set(Set::Value(snap(*range, v * ratio)))));
        }
        Kind::TextEdit => edits.push(set(Set::Text(text))),
        _ => {}
    }
    None
}

/// Shows `edits` in `tree` at once, before the host's next refresh.
pub fn apply(tree: &mut Interface, edits: &[Edit]) {
    for e in edits {
        let Some(w) = tree.widgets.get_mut(e.widget.0) else {
            continue;
        };
        match (&e.to, &mut w.value) {
            (Set::Text(text), _) => w.text.clone_from(text),
            (&Set::Cell(k, v), Some(model::Value::Integers(cells))) => {
                if let Some(c) = cells.get_mut(k as usize) {
                    *c = v.round() as i32;
                }
            }
            (&Set::Cell(k, v), Some(model::Value::Reals(cells))) => {
                if let Some(c) = cells.get_mut(k as usize) {
                    *c = v;
                }
            }
            (Set::Cell(..), _) => {}
            (&Set::Value(v), Some(model::Value::Real(_))) => w.value = Some(model::Value::Real(v)),
            (&Set::Value(v), _) => w.value = Some(model::Value::Integer(v.round() as i32)),
        }
    }
}
