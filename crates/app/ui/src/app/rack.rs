//! The rack: one strip per part, its view below when open. A strip holds
//! what is touched while playing (preset, channel, level, pan, solo, mute);
//! everything else is one click away in its menu.

use kontra_ui_model::PageRef;
use mui::prelude::*;

use super::{Action, App, Part, PartTab, toggle_menu};
use crate::keyboard::note_name;
use crate::kit::{self, Icon, icon_button, knob, latch, list_row, rule, tab};
use crate::theme::{CONTROL, SPACE, TEXT, TIGHT, part_color, secondary};
use crate::view;

pub(super) fn view(ui: &mut Ui, app: &mut App, out: &mut Vec<Action>) -> El {
    if app.parts.is_empty() {
        return col![
            title("Pick an instrument"),
            caption("Choose one in the browser and it plays here.").fill(secondary()),
        ]
        .gap(SPACE)
        .align(Align::Center)
        .justify(Justify::Center)
        .w(Len::Pct(100.))
        .h(Len::Pct(100.));
    }
    let mut strips = Vec::new();
    for n in 0..app.parts.len() {
        strips.push(header(ui, app, n, out));
        if let Some(part) = app.parts.get_mut(n).filter(|p| p.open) {
            strips.push(part_view(ui, part, n, out));
        }
        strips.push(rule());
    }
    col(strips).scroll().min_h(0).w(Len::Pct(100.))
}

fn header(ui: &mut Ui, app: &mut App, n: usize, out: &mut Vec<Action>) -> El {
    let id = |what: &str| format!("part-{n}-{what}");
    let Some(p) = app.parts.get_mut(n) else {
        return spacer();
    };
    let fold = icon_button(
        ui,
        id("fold"),
        if p.open { Icon::Down } else { Icon::Right },
        "Show or hide",
        false,
    );
    if fold.changed {
        p.open = !p.open;
    }
    let name = body(p.name.clone())
        .text_weight(Weight::SEMIBOLD)
        .lines(1)
        .min_w(TEXT * 6.)
        .shrink(1)
        .tip(format!("{}\n{}", p.name, p.library));
    let prev = icon_button(ui, id("prev"), Icon::Left, "Previous preset", false);
    let next = icon_button(ui, id("next"), Icon::Right, "Next preset", false);
    for (r, by) in [(&prev, -1), (&next, 1)] {
        if r.changed {
            out.push(Action::Step { part: n, by });
        }
    }
    let channel = if p.channel == 0 {
        "Omni".to_owned()
    } else {
        format!("Ch {}", p.channel)
    };
    let ch = kit::button(ui, id("channel"), &channel, false);
    let mut volume = p.volume;
    let vol = knob(
        ui,
        id("volume"),
        ("Volume", &super::db(volume)),
        &mut volume,
        -60.0..=12.0,
        0.,
    );
    if vol.changed {
        p.volume = volume;
        out.push(Action::Volume(n, volume));
    }
    let mut pan = p.pan;
    let pan_k = knob(
        ui,
        id("pan"),
        ("Pan", &pan_text(pan)),
        &mut pan,
        -1.0..=1.0,
        0.,
    );
    if pan_k.changed {
        p.pan = pan;
        out.push(Action::Pan(n, pan));
    }
    let solo = latch(ui, id("solo"), "S", "Solo", p.solo);
    let mute = latch(ui, id("mute"), "M", "Mute", p.mute);
    if solo.changed {
        p.solo = !p.solo;
        out.push(Action::Solo(n, p.solo));
    }
    if mute.changed {
        p.mute = !p.mute;
        out.push(Action::Mute(n, p.mute));
    }
    let more = icon_button(ui, id("more"), Icon::More, "More", false);
    let edge = block(3, CONTROL + 2. * TIGHT)
        .fill(Fill::from(part_color(n)))
        .shrink(0);
    let mut items = vec![
        edge,
        fold.el,
        name,
        prev.el,
        next.el,
        spacer(),
        ch.el.id(id("channel")),
        vol.el,
        pan_k.el,
        row![solo.el, mute.el].gap(1),
        more.el,
    ];
    let channel_menu = id("channel");
    if toggle_menu(ui, &mut app.view, &channel_menu, ch.changed) {
        // Channels 1 to 16 as a grid, then every channel.
        let labels: Vec<String> = (1..=16)
            .map(|c: u8| c.to_string())
            .chain(["All".to_owned()])
            .collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let current = app.parts.get(n).map(|p| (usize::from(p.channel) + 16) % 17);
        let m = kit::menu(ui, &channel_menu, &refs, 4, current);
        if let Some(c) = m.changed.map(|c| ((c + 1) % 17) as u8) {
            out.push(Action::Channel(n, c));
            if let Some(p) = app.parts.get_mut(n) {
                p.channel = c;
            }
            app.view.menu = None;
        }
        items.push(m.el);
    }
    if toggle_menu(ui, &mut app.view, &id("more"), more.changed) {
        let m = kit::menu(ui, &id("more"), &["Duplicate", "Remove"], 1, None);
        match m.changed {
            Some(0) => out.push(Action::Duplicate(n)),
            Some(_) => out.push(Action::Remove(n)),
            None => {}
        }
        if m.changed.is_some() {
            app.view.menu = None;
        }
        items.push(m.el);
    }
    row(items)
        .gap(TIGHT)
        .align(Align::Center)
        .pad(kit::edges(0., SPACE, 0., 0.))
        .fill(Role::Raised)
        .shrink(0)
}

fn part_view(ui: &mut Ui, p: &mut Part, n: usize, out: &mut Vec<Action>) -> El {
    let mut tabs = Vec::new();
    let mut offer = |ui: &mut Ui, which: PartTab, label: &str, there: bool| {
        if there {
            let t = tab(ui, format!("part-{n}-tab-{label}"), label, p.tab == which);
            if t.changed {
                p.tab = which;
            }
            tabs.push(t.el);
        }
    };
    offer(ui, PartTab::Interface, "Interface", p.face.is_some());
    offer(
        ui,
        PartTab::Articulations,
        "Articulations",
        !p.articulations.is_empty(),
    );
    if tabs.is_empty() {
        return caption("This instrument has no interface or articulations to show.")
            .fill(secondary())
            .pad(SPACE * 2.);
    }
    let shown = match (p.tab, &mut p.face) {
        (PartTab::Interface, Some(face)) => {
            let (page, _) = view::page(ui, &face.tree, PageRef(0), &mut face.pictures, 1.);
            row![spacer(), page, spacer()].pad(SPACE)
        }
        _ => articulations(ui, p, n, out),
    };
    col![row(tabs).gap(0).pad((0., SPACE)), rule(), shown].w(Len::Pct(100.))
}

fn articulations(ui: &mut Ui, p: &mut Part, n: usize, out: &mut Vec<Action>) -> El {
    let mut picked = None;
    let rows: Vec<El> = p
        .articulations
        .iter()
        .enumerate()
        .map(|(a, (name, key))| {
            let key = key.map(note_name).unwrap_or_default();
            let r = list_row(
                ui,
                format!("part-{n}-art-{a}"),
                (name, &key),
                Some(part_color(n)),
                a == p.articulation,
            );
            if r.changed {
                picked = Some(a);
            }
            r.el
        })
        .collect();
    if let Some(a) = picked {
        p.articulation = a;
        out.push(Action::Articulation(n, a));
    }
    col(rows).pad((TIGHT, SPACE)).w(Len::Pct(100.))
}

fn pan_text(pan: f64) -> String {
    let p = (pan * 100.).round();
    match p {
        0. => "Center".into(),
        p if p < 0. => format!("{:.0} L", -p),
        p => format!("{p:.0} R"),
    }
}
