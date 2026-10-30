//! An open part's view under its tabs: the library's interface, the
//! articulations, what the instrument holds, its scripts.

use kontra_ui_model::PageRef;
use mui::prelude::*;

use super::{Action, FACE_SCALES, FaceLook, Part, PartTab, View};
use crate::keyboard::note_name;
use crate::kit::{self, Icon, glyph, icon_button, interactive, latch, section};
use crate::theme::{CONTROL, SPACE, TEXT, TIGHT, part_color, secondary};
use crate::{input, view};

/// `room`: the width the rack has for a library's view, 0 while unknown.
pub(super) fn view(
    ui: &mut Ui,
    p: &mut Part,
    n: usize,
    room: (f64, f64),
    v: &mut View,
    out: &mut Vec<Action>,
) -> El {
    let tab = match p.tab {
        PartTab::Interface if p.face.is_none() => PartTab::Articulations,
        PartTab::Articulations if p.articulations.is_empty() => PartTab::Info,
        t => t,
    };
    let el = match (tab, &mut p.face) {
        (PartTab::Interface, Some(face)) => {
            let size = face
                .tree
                .pages
                .first()
                .map_or((0., 0.), |p| (p.size.width, p.size.height));
            let scale = fit(
                FACE_SCALES.get(v.face_scale).copied().unwrap_or(0.),
                room,
                size,
            );
            let look = p.look.unwrap_or(v.face_look);
            let (edits, el) = if look == FaceLook::Kontra {
                super::controls::view(ui, n, &face.tree, PageRef(0), &mut v.menu)
            } else {
                let (edits, popup) =
                    input::read(ui, n, &face.tree, PageRef(0), scale, &mut face.gesture);
                let at = (scale, &p.level[..]);
                let draw = if look == FaceLook::Vector {
                    view::vector_page
                } else {
                    view::page
                };
                let (page, _) = draw(ui, n, &face.tree, PageRef(0), &mut face.pictures, at);
                let page = match popup {
                    Some(menu) => stack![page, menu],
                    None => page,
                };
                // Wider than the rack at a fixed scale, it scrolls sideways.
                let el = row![spacer(), page.shrink(0), spacer()]
                    .w(Len::Pct(100.))
                    .scroll();
                (edits, el)
            };
            input::apply(&mut face.tree, &edits);
            out.extend(edits.into_iter().map(|e| {
                let (widget, keys) = (e.widget.0, e.keys);
                let control = |cell, value| Action::Control {
                    part: n,
                    widget,
                    cell,
                    value,
                    keys,
                };
                match e.to {
                    input::Set::Value(v) => control(None, v),
                    input::Set::Cell(k, v) => control(Some(k), v),
                    input::Set::Text(text) => Action::ControlText {
                        part: n,
                        widget,
                        text,
                    },
                }
            }));
            el
        }
        (PartTab::Articulations, _) => articulations(ui, p, n, v, out),
        (PartTab::Info | PartTab::Interface, _) => info(p),
        (PartTab::Scripts, _) => listing("Scripts", &p.scripts, "This instrument runs no scripts."),
        (PartTab::Mapping, _) => super::mapping::view(ui, p, n, v, out),
        (PartTab::Sound, _) => super::sound::view(ui, p, n, v, out),
    };
    el.w(Len::Pct(100.))
        .pad(kit::edges(TIGHT, SPACE, SPACE, SPACE))
}

/// Fit the full face in the rack viewport, or preserve the selected scale.
fn fit(chosen: f64, room: (f64, f64), size: (f64, f64)) -> f64 {
    if chosen > 0. {
        return chosen;
    }
    let width = if room.0 > 0. && size.0 > 0. {
        room.0 / size.0
    } else {
        1.
    };
    let height = if room.1 > 0. && size.1 > 0. {
        room.1 / size.1
    } else {
        width
    };
    width.min(height).min(2.)
}

fn listing(title: &str, lines: &[String], empty: &str) -> El {
    let mut items = vec![section(title)];
    if lines.is_empty() {
        items.push(body(empty.to_owned()).fill(secondary()));
    }
    items.extend(
        lines
            .iter()
            .map(|l| body(l.clone()).lines(1).min_w(0).tip(l.clone())),
    );
    col(items).gap(SPACE).align(Align::Start).pad(SPACE)
}

fn info(p: &Part) -> El {
    let rows: Vec<El> = p
        .info
        .iter()
        .map(|(label, value)| {
            row![
                caption(label.clone())
                    .fill(secondary())
                    .w(TEXT * 8.)
                    .shrink(0),
                body(value.clone()).lines(1).min_w(0).tip(value.clone()),
            ]
            .gap(SPACE)
            .align(Align::Center)
        })
        .collect();
    if rows.is_empty() {
        return listing("Info", &[], "Nothing known about this instrument yet.");
    }
    col(rows).gap(TIGHT).align(Align::Start).pad(SPACE)
}

fn articulations(ui: &mut Ui, p: &mut Part, n: usize, v: &mut View, out: &mut Vec<Action>) -> El {
    let keys = latch(
        ui,
        format!("part-{n}-keys"),
        "Keys",
        "Show the keys",
        !v.keys_hidden,
    );
    if keys.changed {
        v.keys_hidden = !v.keys_hidden;
    }
    let mut rows = vec![
        row![
            caption(p.articulations.len().to_string()).fill(secondary()),
            spacer(),
            keys.el
        ]
        .align(Align::Center)
        .pad((TIGHT, 0.)),
    ];
    let mut picked = None;
    let mut moved = None;
    for a in 0..p.articulations.len() {
        let Some((name, key)) = p.articulations.get(a) else {
            continue;
        };
        let id = format!("part-{n}-art-{a}");
        let selected = a == p.articulation;
        if ui.get(id.clone()).activated() {
            picked = Some(a);
        }
        // The grip carries the name, so it stays with its row as it moves.
        let grip = format!("part-{n}-grip-{name}");
        let drag = ui.get(grip.clone());
        if drag.held {
            // Rows travelled count from where the drag began.
            let from = match &v.grip {
                Some((g, start)) if *g == grip => *start,
                _ => a,
            };
            v.grip = Some((grip.clone(), from));
            let step = (drag.drag_total.y / CONTROL).round() as i64;
            let to = (from as i64 + step).clamp(0, p.articulations.len() as i64 - 1) as usize;
            if to != a {
                moved = Some((a, to));
            }
        } else if v.grip.as_ref().is_some_and(|(g, _)| *g == grip) {
            v.grip = None;
        }
        let more = icon_button(ui, format!("{id}-more"), Icon::More, "More", false);
        let mut cells = vec![
            interactive(stack![glyph(Icon::Grip, TEXT, secondary()).centered()])
                .square(CONTROL)
                .cursor(Cursor::Grab)
                .tip("Drag to reorder")
                .id(grip),
            block(3, TEXT).fill(Fill::from(part_color(a))).shrink(0),
            body(name.clone())
                .text_size(TEXT)
                .fill(if selected {
                    Fill::from(Role::Ink)
                } else {
                    secondary()
                })
                .lines(1)
                .min_w(0)
                .grow(1)
                .tip(name.clone()),
        ];
        if a == p.default_articulation {
            cells.push(caption("default").fill(secondary()));
        }
        if !v.keys_hidden {
            cells.push(
                caption(key.map(note_name).unwrap_or_default())
                    .reserve("C#-2")
                    .shrink(0),
            );
        }
        cells.push(more.el);
        if kit::toggle(ui, &mut v.menu, &format!("{id}-more"), more.changed) {
            let m = kit::menu(ui, &format!("{id}-more"), &["Make default"], 1, None);
            if m.changed.is_some() {
                p.default_articulation = a;
                out.push(Action::DefaultArticulation(n, a));
                v.menu = None;
            }
            cells.push(m.el);
        }
        let r = row(cells)
            .gap(TIGHT)
            .align(Align::Center)
            .h(CONTROL)
            .w(Len::Pct(100.))
            .when(selected, |e| e.fill(Role::Raised))
            .focusable()
            .a11y(A11y::Button)
            .named(name.clone())
            .id(id);
        rows.push(interactive(r));
    }
    if let Some(a) = picked {
        p.articulation = a;
        out.push(Action::Articulation(n, a));
    }
    if let Some((from, to)) = moved {
        let item = p.articulations.remove(from);
        p.articulations.insert(to, item);
        for i in [&mut p.articulation, &mut p.default_articulation] {
            if *i == from {
                *i = to;
            } else if from < *i && *i <= to {
                *i -= 1;
            } else if to <= *i && *i < from {
                *i += 1;
            }
        }
        out.push(Action::MoveArticulation { part: n, from, to });
    }
    col(rows).w(Len::Pct(100.))
}

#[cfg(test)]
mod tests {
    use super::fit;

    #[test]
    fn a_library_view_fits_the_rack_or_keeps_its_chosen_scale() {
        assert!((fit(0., (900., 500.), (730., 700.)) - 500. / 700.).abs() < 1e-9);
        assert!((fit(0., (900., 1000.), (730., 700.)) - 900. / 730.).abs() < 1e-9);
        assert!((fit(0., (200., 500.), (730., 700.)) - 200. / 730.).abs() < 1e-9);
        assert_eq!(fit(0., (0., 0.), (730., 700.)), 1.);
        assert_eq!(fit(1.5, (900., 500.), (730., 700.)), 1.5);
    }
}
