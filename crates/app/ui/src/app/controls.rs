//! A library's controls one after another in KONTRA's own look: a knob,
//! a latch or a menu each, its name under it, whatever its pictures.

use kontra_ui_model::{Interface, Kind, PageRef};
use mui::prelude::*;

use crate::input::{Edit, takes_input, value};
use crate::kit::{self, knob, latch};
use crate::theme::{SPACE, TEXT, TIGHT, secondary};
use crate::view::readout;

mod name;

/// The controls of `page`, and this frame's edits to them. `menu` is the
/// editor's one open dropdown.
pub(super) fn view(
    ui: &mut Ui,
    face: usize,
    tree: &Interface,
    page: PageRef,
    menu: &mut Option<String>,
) -> (Vec<Edit>, El) {
    let mut edits = Vec::new();
    let mut cards = Vec::new();
    for n in tree.draw_order(page) {
        let Some(w) = tree.widgets.get(n.0).filter(|w| takes_input(w)) else {
            continue;
        };
        if !tree.visible(n) {
            continue;
        }
        let id = format!("kontra-c-{face}-{}", n.0);
        let name = name::label(tree, n);
        let now = value(w);
        let (picked, el) = match &w.kind {
            Kind::Knob { range, .. }
            | Kind::Slider { range, .. }
            | Kind::ValueEdit { range, .. } => {
                let (lo, hi) = (range.min.min(range.max), range.max.max(range.min));
                let mut v = now;
                let r = knob(
                    ui,
                    id,
                    (&name, &readout(w, tree.source)),
                    &mut v,
                    lo..=hi,
                    range.default,
                );
                let v = range
                    .step
                    .filter(|s| *s > 0.)
                    .map_or(v, |s| (v / s).round() * s);
                (r.changed.then_some(v), r.el)
            }
            Kind::Button { .. } | Kind::Switch => {
                let on = now >= 1.;
                let r = latch(ui, id, if on { "On" } else { "Off" }, &name, on);
                (r.changed.then_some(if on { 0. } else { 1. }), r.el)
            }
            Kind::Menu { items } => {
                let shown: Vec<_> = items.iter().filter(|i| i.visible).collect();
                let labels: Vec<&str> = shown.iter().map(|i| i.text.as_str()).collect();
                let at = shown.iter().position(|i| f64::from(i.value) == now);
                let label = at.and_then(|a| labels.get(a)).copied().unwrap_or("");
                let r = kit::dropdown(ui, menu, &id, (None, label, &name), &labels, at);
                let v = r
                    .changed
                    .and_then(|a| shown.get(a))
                    .map(|i| f64::from(i.value));
                (v, r.el)
            }
            _ => continue,
        };
        if let Some(v) = picked.filter(|v| *v != now) {
            edits.push(Edit {
                widget: n,
                value: v,
            });
        }
        cards.push(
            col![el, caption(name).fill(secondary()).lines(1).min_w(0)]
                .gap(TIGHT)
                .align(Align::Center)
                .w(TEXT * 8.),
        );
    }
    let el = if cards.is_empty() {
        body("This interface has no controls.".to_owned()).fill(secondary())
    } else {
        row(cards).wrap().gap(SPACE)
    };
    (edits, el)
}
