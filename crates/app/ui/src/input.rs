//! A library's own controls under the pointer, as Kontakt plays them: a
//! knob or slider drags from where it was pressed (Shift for fine), the
//! wheel turns it, Ctrl or Cmd with a click puts back its default
//! (`$CONTROL_PAR_DEFAULT_VALUE`); a button or switch flips; a menu floats
//! its items. What the user asked comes back as [`Edit`]s for the host to
//! write into the engine; the caller shows them at once with [`apply`].

use kontra_ui_model::{self as model, Interface, Kind, Orientation, PageRef, WidgetRef};
use mui::prelude::*;

use crate::kit;

/// One control set to a value by the user, and the modifier keys held at
/// the click (a script reads them as `$CONTROL_PAR_KEY_*`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edit {
    pub widget: WidgetRef,
    pub value: f64,
    pub keys: model::UiModifiers,
}

/// A page's gesture in flight: the control pressed and its value then, and
/// the menu floated open.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Gesture {
    press: Option<(usize, f64)>,
    pub menu: Option<usize>,
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

/// Which way a continuous control drags, and how many view points sweep
/// its range: KSP's `MOUSE_BEHAVIOUR` is relative to the picture, 1000
/// sweeping it once; a knob that sets none sweeps in 200 points, a slider
/// in its own length (v2's law).
// ponytail: v2's travel law, unmeasured against Kontakt 8; measure with the oracle's pointer.
fn travel(w: &model::Widget, r: model::Rect, scale: f64) -> (bool, f64) {
    let vertical = match (&w.kind, w.drag) {
        (_, Some(d)) => d.axis == Orientation::Vertical,
        (Kind::Slider { orientation, .. }, None) => {
            *orientation == Orientation::Vertical || r.height > r.width
        }
        _ => true,
    };
    let length = if vertical { r.height } else { r.width }.max(1.);
    let points = match (w.drag.filter(|d| d.sensitivity != 0), &w.kind) {
        (Some(d), _) => length * 1000. / f64::from(d.sensitivity),
        (None, Kind::Slider { .. }) => length,
        (None, _) => 200.,
    };
    (vertical, points * scale)
}

fn range(kind: &Kind) -> Option<model::Range> {
    match kind {
        Kind::Knob { range, .. } | Kind::Slider { range, .. } | Kind::ValueEdit { range, .. } => {
            Some(*range)
        }
        _ => None,
    }
}

/// This frame's edits to the controls of `page` drawn at `scale`, and the
/// open menu's popup, to float over the page.
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
        let now = value(w);
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
        let mut set = |v: f64| {
            if v != now {
                edits.push(Edit {
                    widget: n,
                    value: v,
                    keys,
                });
            }
        };
        if let Some(range) = range(&w.kind) {
            let snap = |v: f64| {
                let v = v.clamp(range.min.min(range.max), range.max.max(range.min));
                range
                    .step
                    .filter(|s| *s > 0.)
                    .map_or(v, |s| (v / s).round() * s)
            };
            let span = range.max - range.min;
            let (vertical, points) = travel(w, tree.page_rect(n), scale);
            let along = |d: Vec2| if vertical { -d.y } else { d.x };
            if r.pressed {
                g.press = Some((n.0, now));
                if r.press_mods.ctrl || r.press_mods.cmd {
                    set(snap(range.default));
                }
            }
            match g.press {
                Some((p, start))
                    if p == n.0 && r.held && !(r.press_mods.ctrl || r.press_mods.cmd) =>
                {
                    let fine = if r.press_mods.shift { 0.1 } else { 1. };
                    set(snap(start + along(r.drag_total) / points * span * fine));
                }
                _ => {}
            }
            // Claimed, so the rack under the knob does not scroll with it.
            if let Some(wheel) = ui.wheel(id.as_str()).filter(|_| !r.held) {
                set(snap(now - wheel.y / points * span));
            }
            if r.released && g.press.is_some_and(|(p, _)| p == n.0) {
                g.press = None;
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
            _ => {}
        }
    }
    let popup = g.menu.and_then(|m| {
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
                value: f64::from(item.value),
                keys: model::UiModifiers::default(),
            });
            g.menu = None;
        }
        Some(menu.el)
    });
    (edits, popup)
}

/// Shows `edits` in `tree` at once, before the host's next refresh.
pub fn apply(tree: &mut Interface, edits: &[Edit]) {
    for e in edits {
        if let Some(w) = tree.widgets.get_mut(e.widget.0) {
            w.value = Some(match w.value {
                Some(model::Value::Real(_)) => model::Value::Real(e.value),
                _ => model::Value::Integer(e.value.round() as i32),
            });
        }
    }
}
