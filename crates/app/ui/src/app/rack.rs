//! The rack: one header per part, as v2 drew it (preset, channel, output,
//! pan, volume, tune, solo, mute), its tabs and view below when open.

use std::sync::Arc;

use mui::prelude::*;

use super::browser::Carried;
use super::{Action, App, FaceLook, Part, PartTab, db, part};
use crate::kit::{self, Icon, icon_button, knob, latch, nudge, rule, tab};
use crate::theme::{CONTROL, SIDEBAR, SMALL, SPACE, TEXT, TIGHT, lift, part_color, secondary};

pub(super) fn view(ui: &mut Ui, app: &mut App, out: &mut Vec<Action>) -> El {
    // A preset dropped on free rack space adds a part.
    if let Some(Carried { library, preset }) = ui.dropped_on::<Carried>("rack") {
        out.push(Action::Load { library, preset });
    }
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
        .h(Len::Pct(100.))
        .id("rack");
    }
    // What the rack has of the editor's width for a library's view.
    let browser = if app.view.browser_hidden {
        0.
    } else {
        SIDEBAR + 1.
    };
    let room = (app.width - browser - 2. * SPACE).max(0.);
    // Fit one open face inside the rack viewport with its two header rows.
    let height = ui.scene().and_then(|s| s.surface("rack")).map_or(0., |s| {
        (s.frame.size.height - 2. * ROW - TIGHT - SPACE).max(0.)
    });
    let mut strips = Vec::new();
    for n in 0..app.parts.len() {
        let art = art(app, n);
        // A preset dropped on a part's header replaces what it plays.
        let id = format!("part-{n}-band");
        if let Some(Carried { library, preset }) = ui.dropped_on::<Carried>(id.as_str()) {
            out.push(Action::Replace {
                part: n,
                library,
                preset,
            });
        }
        let over = ui.get(id.as_str()).drop_target && ui.dragging::<Carried>().is_some();
        let head = band(header(ui, app, n, out), art.clone(), 0);
        strips.push(
            head.when(over, |e| e.stroke(Role::Ink).stroke_width(1))
                .id(id),
        );
        if app.parts.get(n).is_some_and(|p| p.open) {
            strips.push(band(tabs(ui, app, n, out), art, 1));
            if let Some(p) = app.parts.get_mut(n) {
                strips.push(part::view(ui, p, n, (room, height), &mut app.view, out));
            }
        }
        strips.push(rule());
    }
    if app.loading == Some(super::Loading::New) {
        let row = row![block(3, Len::Pct(100.)).fill(lift(3)).shrink(0), chip()]
            .gap(SPACE)
            .align(Align::Center);
        strips.push(band(row, None, 0));
        strips.push(rule());
    }
    strips.push(foot(ui, app, out));
    col(strips).scroll().min_h(0).w(Len::Pct(100.)).id("rack")
}

/// v2's load chip: shown until the instrument plays.
fn chip() -> El {
    caption("Loading…")
        .text_size(SMALL - 1.)
        .fill(secondary())
        .lines(1)
        .shrink(0)
        .named("Loading")
}

/// A header row's height: both rows are one, so the artwork lines up.
const ROW: f64 = CONTROL + 2. * TIGHT;
/// The artwork behind a part's header, fading out over this width (v2's).
const BANNER: f64 = crate::artwork::BANNER.0;

/// Part `n`'s library cover, once it has decoded.
fn art(app: &mut App, n: usize) -> Option<Arc<Image>> {
    let name = &app.parts.get(n)?.library;
    let lib = app.libraries.iter().position(|l| &l.name == name)?;
    let art = &app.libraries.get(lib)?.art;
    super::browser::cover(app.banners.as_mut()?, lib, art)
}

/// Header row `row` (0 or 1) of a part on its library's artwork, as v2
/// drew it: both rows show one two-row image, offset, so the header reads
/// as one surface; the names sit on a wash that fades out quickly.
fn band(content: El, art: Option<Arc<Image>>, row: u8) -> El {
    let y = -f64::from(row) * ROW;
    let mut layers = Vec::new();
    if let Some(img) = art {
        // v2's light wash over the dimmed banner, fading out quickly.
        let wash = Gradient::linear(
            90.,
            [
                (0., Role::Surface.alpha(0.55)),
                (0.5, Role::Surface.alpha(0.2)),
                (1., Role::Surface.alpha(0.)),
            ],
        );
        layers.push(
            block(BANNER, ROW * 2.)
                .fill(Fill::Image(img, Fit::Cover))
                .at(0., y),
        );
        layers.push(block(BANNER * 0.75, ROW * 2.).fill(wash).at(0., y));
    }
    layers.push(content.w(Len::Pct(100.)).h(Len::Pct(100.)));
    stack(layers)
        .w(Len::Pct(100.))
        .h(ROW)
        .fill(Role::Surface)
        .clip()
        .shrink(0)
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
    let loading = app.loading == Some(super::Loading::Part(n));
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
    let name = match loading {
        true => row![name, chip()]
            .gap(SPACE)
            .align(Align::Center)
            .grow(1)
            .min_w(0),
        false => name,
    };
    let names: Vec<&str> = presets
        .iter()
        .map(|s| s.rsplit('/').next().unwrap_or(s))
        .collect();
    let current = names.iter().position(|&s| s == p.name);
    // v2's preset field beside the name: what plays, or what to pick.
    let shown = current
        .and_then(|c| names.get(c).copied())
        .unwrap_or("Select preset…");
    let preset = kit::dropdown(
        ui,
        menu,
        &id("preset"),
        (None, shown, "Preset"),
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
    let playing = p.level.iter().any(|&l| l > 0.);
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
        preset.el.w(TEXT * 16.).min_w(CONTROL * 4.).shrink(1),
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
    if p.face.is_some() && p.tab == PartTab::Interface {
        let now = p.look.unwrap_or(app.view.face_look);
        let at = FaceLook::ALL.iter().position(|l| *l == now);
        let label = at
            .and_then(|a| FaceLook::LABELS.get(a))
            .copied()
            .unwrap_or("");
        let pick = kit::dropdown(
            ui,
            menu,
            &format!("part-{n}-look"),
            (None, label, "How this interface is drawn"),
            &FaceLook::LABELS,
            at,
        );
        if let Some(&l) = pick.changed.and_then(|a| FaceLook::ALL.get(a)) {
            p.look = Some(l);
        }
        items.push(pick.el);
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

/// Where presets are dropped to be added, and a button that shows the
/// browser to find one (v2's `src/ui/rack.rs` `foot`).
fn foot(ui: &mut Ui, app: &mut App, out: &mut Vec<Action>) -> El {
    let dragging = ui.dragging::<Carried>().is_some();
    let over = ui.get("rack-drop").drop_target && dragging;
    if let Some(Carried { library, preset }) = ui.dropped_on::<Carried>("rack-drop") {
        out.push(Action::Load { library, preset });
    }
    if ui.get("rack-drop").activated() {
        app.view.browser_hidden = false;
        ui.focus("search");
    }
    let text = if dragging {
        "Drop to add to the rack".to_owned()
    } else {
        format!("Add an instrument · {} loaded", app.parts.len())
    };
    let ink = if dragging {
        Fill::from(Role::Ink)
    } else {
        secondary()
    };
    let el = row![
        kit::glyph(Icon::Plus, TEXT, ink),
        caption(text).fill(secondary()).lines(1)
    ]
    .gap(SPACE)
    .align(Align::Center)
    .justify(Justify::Center)
    .pad(SPACE)
    .h(CONTROL + 2. * SPACE)
    .w(Len::Pct(100.))
    .when(over, |e| e.stroke(Role::Ink).stroke_width(1))
    .focusable()
    .a11y(A11y::Button)
    .named("Add to rack")
    .tip("Drop a preset here to add it, or click to search the browser.")
    .id("rack-drop")
    .shrink(0);
    kit::interactive(el)
}
