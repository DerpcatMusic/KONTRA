//! The rack: one header per part, as v2 drew it (preset, channel, output,
//! pan, volume, tune, solo, mute), its tabs and view below when open.

use mui::prelude::*;

use super::{Action, App, Part, PartTab, db, part};
use crate::kit::{self, Icon, icon_button, knob, latch, nudge, rule, tab};
use crate::theme::{CONTROL, SPACE, TEXT, TIGHT, part_color, secondary};

pub(super) fn view(ui: &mut Ui, app: &mut App, out: &mut Vec<Action>) -> El {
    if app.parts.is_empty() {
        return col![
            spacer(),
            title("Pick an instrument"),
            caption("Choose one in the browser and it plays here.").fill(secondary()),
            spacer(),
        ]
        .gap(SPACE)
        .align(Align::Center)
        .w(Len::Pct(100.))
        .h(Len::Pct(100.));
    }
    let mut strips = Vec::new();
    for n in 0..app.parts.len() {
        strips.push(header(ui, app, n, out));
        if app.parts.get(n).is_some_and(|p| p.open) {
            strips.push(tabs(ui, app, n, out));
            if let Some(p) = app.parts.get_mut(n) {
                strips.push(part::view(ui, p, n, &mut app.view, out));
            }
        }
        strips.push(rule());
    }
    col(strips).scroll().min_h(0).w(Len::Pct(100.))
}

/// The pan a part sits at, for its readout: C, or a side and how far.
pub(super) fn pan_text(pan: f64) -> String {
    let p = (pan * 100.).round();
    match p {
        0. => "C".into(),
        p if p < 0. => format!("{:.0} L", -p),
        p => format!("{p:.0} R"),
    }
}

/// A part's pan: its readout over a short slider.
pub(super) fn pan(ui: &mut Ui, id: String, value: &mut f64) -> Response {
    let s = kit::slider(
        ui,
        id,
        "Pan",
        value,
        -1.0..=1.0,
        0.,
        (TEXT * 3., TEXT * 0.9),
    );
    let el = col![
        caption(pan_text(*value))
            .text_size(TEXT * 0.75)
            .fill(secondary()),
        s.el
    ]
    .align(Align::Center)
    .shrink(0);
    Response {
        el,
        changed: s.changed,
    }
}

/// Solo and mute side by side; the actions they asked for.
pub(super) fn solo_mute(
    ui: &mut Ui,
    p: &mut Part,
    n: usize,
    id: &str,
    out: &mut Vec<Action>,
) -> El {
    let solo = latch(ui, format!("{id}-solo"), "S", "Solo", p.solo);
    let mute = latch(ui, format!("{id}-mute"), "M", "Mute", p.mute);
    if solo.changed {
        p.solo = !p.solo;
        out.push(Action::Solo(n, p.solo));
    }
    if mute.changed {
        p.mute = !p.mute;
        out.push(Action::Mute(n, p.mute));
    }
    row![solo.el, mute.el].gap(1).shrink(0)
}

/// The output choices: the host's pick, then the stereo pairs with the
/// parts on each.
pub(super) fn outputs(parts: &[Part]) -> Vec<String> {
    let on = |k: u8| {
        parts
            .iter()
            .filter(|p| p.output == k)
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    std::iter::once("Automatic".to_owned())
        .chain((1..=16).map(|k| match on(k) {
            names if names.is_empty() => format!("st.{k}"),
            names => format!("st.{k} · {names}"),
        }))
        .collect()
}

/// A part's output dropdown.
pub(super) fn output(
    ui: &mut Ui,
    app: &mut App,
    n: usize,
    id: String,
    out: &mut Vec<Action>,
) -> El {
    let labels = outputs(&app.parts);
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let Some(k) = app.parts.get(n).map(|p| p.output) else {
        return spacer();
    };
    let label = if k == 0 {
        "Auto".into()
    } else {
        format!("st.{k}")
    };
    let d = kit::dropdown(
        ui,
        &mut app.view.menu,
        &id,
        (Some(Icon::Output), &label, "Output"),
        &refs,
        Some(usize::from(k)),
    );
    if let Some(k) = d.changed.and_then(|k| u8::try_from(k).ok()) {
        out.push(Action::Output(n, k));
        if let Some(p) = app.parts.get_mut(n) {
            p.output = k;
        }
    }
    d.el
}

fn header(ui: &mut Ui, app: &mut App, n: usize, out: &mut Vec<Action>) -> El {
    let id = |what: &str| format!("part-{n}-{what}");
    let presets: Vec<String> = app
        .parts
        .get(n)
        .and_then(|p| app.libraries.iter().find(|l| l.name == p.library))
        .map(|l| l.presets.clone())
        .unwrap_or_default();
    let output = output(ui, app, n, id("output"), out);
    let menu = &mut app.view.menu;
    let Some(p) = app.parts.get_mut(n) else {
        return spacer();
    };
    let fold = icon_button(
        ui,
        id("fold"),
        if p.open { Icon::Down } else { Icon::Right },
        if p.open { "Fold" } else { "Unfold" },
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
        .grow(1)
        .tip(format!("{}\n{}", p.name, p.library));
    let names: Vec<&str> = presets
        .iter()
        .map(|s| s.rsplit('/').next().unwrap_or(s))
        .collect();
    let current = names.iter().position(|&s| s == p.name);
    let preset = kit::dropdown(
        ui,
        menu,
        &id("preset"),
        (None, "", "Presets"),
        &names,
        current,
    );
    if let Some(preset) = preset.changed {
        out.push(Action::Switch { part: n, preset });
    }
    let prev = icon_button(ui, id("prev"), Icon::Left, "Previous preset", false);
    let next = icon_button(ui, id("next"), Icon::Right, "Next preset", false);
    for (r, by) in [(&prev, -1), (&next, 1)] {
        if r.changed {
            out.push(Action::Step { part: n, by });
        }
    }
    let channels: Vec<String> = std::iter::once("Omni".to_owned())
        .chain((1..=16).map(|c: u8| c.to_string()))
        .collect();
    let refs: Vec<&str> = channels.iter().map(String::as_str).collect();
    let ch = kit::dropdown(
        ui,
        menu,
        &id("channel"),
        (
            Some(Icon::Midi),
            refs.get(usize::from(p.channel)).copied().unwrap_or("Omni"),
            "MIDI channel",
        ),
        &refs,
        Some(usize::from(p.channel)),
    );
    if let Some(c) = ch.changed.and_then(|c| u8::try_from(c).ok()) {
        p.channel = c;
        out.push(Action::Channel(n, c));
    }
    let mut value = p.pan;
    let pan = pan(ui, id("pan"), &mut value);
    if pan.changed {
        p.pan = value;
        out.push(Action::Pan(n, value));
    }
    let mut value = p.volume;
    let vol = knob(
        ui,
        id("volume"),
        ("Volume", &db(value)),
        &mut value,
        -60.0..=12.0,
        0.,
    );
    if vol.changed {
        p.volume = value;
        out.push(Action::Volume(n, value));
    }
    let mut value = p.tune;
    let readout = format!("{value:.2}");
    let tune = nudge(
        ui,
        id("tune"),
        (Icon::Tune, "Tune"),
        &mut value,
        -36.0..=36.0,
        0.,
        &readout,
    );
    if tune.changed {
        p.tune = (value * 100.).round() / 100.;
        out.push(Action::Tune(n, p.tune));
    }
    let readout = caption(format!("{:.1}", p.volume)).reserve("-00.0");
    let solo_mute = solo_mute(ui, p, n, &id("sm"), out);
    let playing = p.level > 0.;
    let dot = block(6, 6)
        .radius(3.)
        .fill(if playing {
            Fill::from(part_color(n))
        } else {
            crate::theme::lift(3)
        })
        .shrink(0)
        .tip(if playing { "Playing" } else { "Silent" });
    let more = icon_button(ui, id("more"), Icon::More, "More", false);
    let close = icon_button(ui, id("remove"), Icon::Close, "Remove", false);
    if close.changed {
        out.push(Action::Remove(n));
    }
    let edge = block(3, CONTROL + 2. * TIGHT)
        .fill(Fill::from(part_color(n)))
        .shrink(0);
    let mut items = vec![
        edge,
        fold.el,
        name,
        preset.el,
        prev.el,
        next.el,
        ch.el,
        output,
        pan.el,
        row![vol.el, readout]
            .gap(TIGHT)
            .align(Align::Center)
            .shrink(0),
        tune.el,
        solo_mute,
        dot,
        more.el,
        close.el,
    ];
    if kit::toggle(ui, menu, &id("more"), more.changed) {
        let m = kit::menu(ui, &id("more"), &["Duplicate", "Remove"], 1, None);
        match m.changed {
            Some(0) => out.push(Action::Duplicate(n)),
            Some(_) => out.push(Action::Remove(n)),
            None => {}
        }
        if m.changed.is_some() {
            *menu = None;
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

/// An open part's tabs, its articulation, what it needs, its velocity.
fn tabs(ui: &mut Ui, app: &mut App, n: usize, out: &mut Vec<Action>) -> El {
    let menu = &mut app.view.menu;
    let Some(p) = app.parts.get_mut(n) else {
        return spacer();
    };
    let mut items = vec![block(3, CONTROL).fill(Fill::from(part_color(n))).shrink(0)];
    for (which, label, there) in [
        (PartTab::Interface, "Interface", p.face.is_some()),
        (
            PartTab::Articulations,
            "Articulations",
            !p.articulations.is_empty(),
        ),
        (PartTab::Mapping, "Mapping", true),
        (PartTab::Sound, "Sound", true),
        (PartTab::Info, "Info", true),
        (PartTab::Scripts, "Scripts", true),
    ] {
        if there {
            let t = tab(ui, format!("part-{n}-tab-{label}"), label, p.tab == which);
            if t.changed {
                p.tab = which;
            }
            items.push(t.el);
        }
    }
    let art = p
        .articulations
        .get(p.articulation)
        .map(|a| a.0.clone())
        .unwrap_or_default();
    items.push(
        caption(art.clone())
            .fill(secondary())
            .lines(1)
            .min_w(0)
            .shrink(1)
            .grow(1)
            .pad((SPACE, 0))
            .tip(art),
    );
    if !p.needs.is_empty() {
        let id = format!("part-{n}-needs");
        let r = icon_button(
            ui,
            id.clone(),
            Icon::Warning,
            "What it needs to play fully",
            false,
        );
        items.push(r.el);
        if kit::toggle(ui, menu, &id, r.changed) {
            let lines: Vec<El> = p
                .needs
                .iter()
                .map(|l| caption(l.clone()).w(TEXT * 22.))
                .collect();
            items.push(kit::popup(&id, col(lines).gap(TIGHT).pad(TIGHT)));
        }
    }
    const VELOCITY: [u8; 3] = [0, 64, 127];
    let at = VELOCITY.iter().position(|&v| v == p.velocity);
    let labels = ["Auto", "64", "127"];
    let v = kit::dropdown(
        ui,
        menu,
        &format!("part-{n}-velocity"),
        (
            None,
            at.and_then(|a| labels.get(a)).copied().unwrap_or("Auto"),
            "Velocity: as played, or fixed",
        ),
        &["Auto: as played", "64", "127"],
        at,
    );
    if let Some(&vel) = v.changed.and_then(|a| VELOCITY.get(a)) {
        p.velocity = vel;
        out.push(Action::Velocity(n, vel));
    }
    items.push(v.el);
    row(items)
        .gap(TIGHT)
        .align(Align::Center)
        .pad(kit::edges(0., SPACE, 0., 0.))
        .shrink(0)
}
