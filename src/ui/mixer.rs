//! The mixer: Kontakt's output section as a console of channel strips.
//!
//! ```text
//! SIGNALS  3 instruments              ┆ BUSES  st.1–st.16 to the host
//! ┌name──┐┌name──┐┌name──┐            ┆ ┌st.1──┐┌st.2──┐ +  ┆ ┌Master┐
//! │⎍ A 1 ││⎍ A 2 ││⎍ A 3 │  MIDI in   ┆ │◁ 2 in││◁ 1 in│    ┆ │ Sum  │
//! │ ◢ C  ││◣ L10 ││ ◢ R5 │  pan       ┆ │ ◢ C  ││ ◢ C  │    ┆ │      │
//! │ ┃ ▌▌ ││ ┃ ▌▌ ││ ┃ ▌▌ │  fader+meter ┆ ...                ┆ │ ┃ ▌▌ │
//! │ -3.0 ││ +0.2 ││ -inf │  dB        ┆                    ┆ │-12.0 │
//! │ S  M ││ S  M ││ S  M │            ┆                    ┆ │      │
//! │→ aux ││→ aux ││→ aux │  send      ┆                    ┆ │      │
//! │▮st.1 ││▮st.2 ││▮st.1 │  output    ┆ │▮1/2  ││▮3/4  │    ┆ │▮All  │
//! ```
//!
//! Instrument strips follow the rack order; a bus shows once a part routes
//! or sends to it, once it differs from its defaults, or after "+". The
//! master strip is the host Volume parameter. Meters are canvases that read
//! the audio thread's atomics while the scene is walked, so the tree never
//! depends on a level: [`super::Watch`] asks for frames only while a meter
//! is moving.

use super::{Cx, RackDrag, instrument, menu, theme::*};
use crate::engine::BUSES;
use crate::plugin::{Bus, Meters, P, SamplerParams};
use moose::mui::{Bridge, mui::prelude::*};
use std::path::Path;

/// A strip's width: a fader, a meter and a readout, no more.
const WIDTH: f64 = TEXT * 6.5;
/// The fader's thumb; the meter keeps the same end margins, so levels and
/// fader positions share one scale.
const THUMB: (f64, f64) = (TEXT * 1.5, TEXT * 0.75);
/// Two lines of a strip's name.
const NAME: f64 = SMALL * 2.6;
const DB: std::ops::RangeInclusive<f64> = -60.0..=6.0;

/// Which strip a menu or a drag is about.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Strip {
    Part(usize),
    Bus(usize),
}

/// A bus's own color: evenly spread hues at one quiet lightness, so a
/// part's routing label matches the strip it plays through.
pub fn bus_color(n: usize) -> Color {
    Color::oklch(0.7, 0.1, (n as f32 * 137.508 + 190.) % 360.)
}

/// Host stereo port `n` as a DAW lists it: "1/2", "3/4".
pub fn port_text(n: usize) -> String {
    format!("{}/{}", 2 * n + 1, 2 * n + 2)
}

/// "C3"-style MIDI input: "A 1", "B Omni".
pub fn input_text(port: u8, channel: i16) -> String {
    let port = char::from(b'A' + port.min(3));
    if channel < 0 {
        format!("{port} Omni")
    } else {
        format!("{port} {}", channel + 1)
    }
}

/// A fader readout, signed and short: "+0.2", "-10.0", "-inf".
pub fn db_short(db: f64) -> String {
    if db <= -59.95 {
        "-inf".into()
    } else if db.abs() < 0.05 {
        "0.0".into()
    } else {
        format!("{db:+.1}")
    }
}

/// Whether bus `n` earns a strip without being asked for.
fn in_use(cx: &Cx, n: usize) -> bool {
    cx.selection.bus(n) != Bus::default()
        || cx.selection.parts.iter().any(|p| {
            !p.path.is_empty() && (usize::from(p.output) == n || p.aux == n as i16)
        })
}

pub fn view(ui: &mut Ui, cx: &mut Cx, bridge: &mut Bridge<SamplerParams>) -> El {
    let mut signals = Vec::new();
    for slot in cx.selection.order.clone() {
        signals.push(part_strip(ui, cx, slot as usize));
    }
    let count = signals.len();
    if signals.is_empty() {
        signals.push(
            caption("Load an instrument to give it a strip.")
                .fill(Role::Dim)
                .pad(INSET)
                .shrink(0),
        );
    }
    let shown = |cx: &Cx, n: usize| n < cx.state.buses_shown || in_use(cx, n);
    let mut buses = Vec::new();
    for n in 0..BUSES {
        if shown(cx, n) {
            buses.push(bus_strip(ui, cx, n));
        }
    }
    let hidden = (0..BUSES).find(|&n| !shown(cx, n));
    if let Some(next) = hidden {
        let (more, more_el) = icon_button(ui, "mix-add-bus", Icon::Plus, "Show another bus", false);
        if more {
            cx.state.buses_shown = next + 1;
        }
        buses.push(col![more_el, spacer()].pad(TIGHT).fill(Role::Surface).shrink(0));
    }
    let master = master_strip(ui, cx, bridge);
    let signals = group(
        "Signals",
        match count {
            1 => "1 instrument · MIDI in to a bus".into(),
            n => format!("{n} instruments · MIDI in to a bus"),
        },
        signals,
    );
    buses.push(block(SPACE, Len::Pct(100.)).fill(Role::Background).shrink(0));
    buses.push(master);
    let buses = group("Buses", "st.1–st.16 to the host · Master".into(), buses);
    row![signals, buses]
        .gap(SPACE)
        .align(Align::Stretch)
        .pad(SPACE)
        .flex(1)
        .min_h(0)
        .min_w(0)
        .scroll()
        .fill(Role::Background)
        .id("mixer")
}

/// A titled run of strips, hairlines between them.
fn group(title: &str, subtitle: String, strips: Vec<El>) -> El {
    col![
        row![section(title), caption(subtitle).fill(Role::Dim).lines(1)]
            .gap(SPACE)
            .align(Align::Center)
            .pad(edges(0., 0., TIGHT, TIGHT))
            .h(CONTROL)
            .shrink(0),
        row(strips)
            .gap(1)
            .align(Align::Stretch)
            .fill(hairline())
            .flex(1)
            .min_h(0)
            .shrink(0)
    ]
    .gap(0)
    .align(Align::Start)
    .shrink(0)
}

/// The column every strip is built on: a colored top edge, then `rows`.
fn frame(rows: Vec<El>, edge: Fill, selected: bool) -> El {
    let mut items = vec![block(Len::Pct(100.), 2).fill(edge).shrink(0)];
    items.push(
        col(rows)
            .gap(TIGHT)
            .align(Align::Stretch)
            .pad((TIGHT, TIGHT))
            .flex(1)
            .min_h(0),
    );
    col(items)
        .gap(0)
        .align(Align::Stretch)
        .w(WIDTH)
        .fill(if selected { Role::Raised } else { Role::Surface })
        .shrink(0)
}

/// A right-click on the strip itself or on one of its controls that has no
/// menu of its own: the whole id, or a prefix numbered `n`.
fn right_clicked(ui: &Ui, ids: &[&str], n: usize) -> bool {
    ids.iter().enumerate().any(|(k, id)| {
        let id = if k == 0 { (*id).to_owned() } else { format!("{id}-{n}") };
        ui.get(id).clicked_with(Button::Secondary)
    })
}

/// Room a strip without that row keeps, so faders line up across groups.
fn blank(height: f64) -> El {
    block(Len::Pct(100.), height).shrink(0)
}

fn part_strip(ui: &mut Ui, cx: &mut Cx, slot: usize) -> El {
    let id = format!("strip-{slot}");
    let r = ui.get(id.as_str());
    if r.clicked {
        cx.state.selected = slot;
    }
    if right_clicked(ui, &[&id, "mix-name", "mix-pan", "mix-fader"], slot) {
        menu::open(ui, cx, menu::Target::Strip(Strip::Part(slot)));
    }
    let library = cx.library_of(Path::new(&cx.selection.parts[slot].path));
    let tint = cx.state.tint(cx.view, &library);
    let name = strip_name(ui, cx, Strip::Part(slot));

    let part = cx.selection.parts[slot].clone();
    let (input, input_el) = route(
        ui,
        &format!("mix-in-{slot}"),
        Icon::MidiIn,
        None,
        input_text(part.port, part.channel),
        "MIDI input",
    );
    if input {
        menu::open_under(ui, cx, menu::Target::Midi(slot), &format!("mix-in-{slot}"));
    }
    let mut pan = f64::from(part.pan);
    let pan_el = pan_wedge(ui, &format!("mix-pan-{slot}"), &mut pan);
    let mut gain = f64::from(part.gain);
    let meter = cx.p.clone();
    let fader_el = channel(
        ui,
        &format!("mix-fader-{slot}"),
        "Volume",
        &mut gain,
        0.,
        !part.mute,
        move || Meters::read(&meter.shared.meters.parts[slot]),
    );
    let (mut solo, mut mute) = (part.solo, part.mute);
    let switches = solo_mute(ui, &format!("mix-{slot}"), &mut solo, &mut mute);
    let aux_text = if part.aux < 0 {
        "No send".to_owned()
    } else {
        cx.selection.bus(part.aux as usize).label(part.aux as usize)
    };
    let aux_chip = (part.aux >= 0).then(|| bus_color(part.aux as usize));
    let (aux, aux_el) = route(ui, &format!("mix-aux-{slot}"), Icon::Right, aux_chip, aux_text, "Aux send");
    if aux {
        menu::open_under(ui, cx, menu::Target::Aux(slot), &format!("mix-aux-{slot}"));
    }
    let mut aux_gain = f64::from(part.aux_gain);
    let send_el = send_level(ui, &format!("mix-send-{slot}"), &mut aux_gain, part.aux >= 0);
    let out = usize::from(part.output);
    let (output, output_el) = route(
        ui,
        &format!("mix-out-{slot}"),
        Icon::AudioOut,
        Some(bus_color(out)),
        cx.selection.bus(out).label(out),
        "Output",
    );
    if output {
        menu::open_under(ui, cx, menu::Target::Output(slot), &format!("mix-out-{slot}"));
    }

    let part = &mut cx.selection.parts[slot];
    part.pan = pan as f32;
    part.gain = gain as f32;
    part.aux_gain = aux_gain as f32;
    part.solo = solo;
    part.mute = mute;

    let rows = vec![
        name,
        input_el,
        pan_el,
        fader_el,
        row![switches].justify(Justify::Center).shrink(0),
        col![aux_el, send_el].gap(2).shrink(0),
        output_el,
    ];
    let edge = tint.map_or(Role::Ink.alpha(0.12), Fill::from);
    let label = super::rack::name(cx, slot);
    frame(rows, edge, cx.state.selected == slot)
        .a11y(A11y::Group)
        .named(label)
        .id(id)
}

fn bus_strip(ui: &mut Ui, cx: &mut Cx, n: usize) -> El {
    let id = format!("bus-{n}");
    if right_clicked(ui, &[&id, "bus-name", "bus-pan", "bus-fader"], n) {
        menu::open(ui, cx, menu::Target::Strip(Strip::Bus(n)));
    }
    // An instrument strip dropped here plays through this bus.
    // The drop lands on whatever control is under the pointer: anything
    // inside the strip counts.
    let within = |ui: &Ui, at: Option<Point>| {
        let (scene, at) = (ui.scene(), at);
        scene.and_then(|s| s.surface(&id)).zip(at).is_some_and(|(s, at)| {
            let f = s.frame;
            at.x >= f.x && at.x < f.x + f.size.width && at.y >= f.y && at.y < f.y + f.size.height
        })
    };
    let over = matches!(ui.dragging::<RackDrag>(), Some(RackDrag::Part(_))) && within(ui, ui.local("editor-root"));
    let target = ui.dropped().map(|(_, target)| target.to_owned());
    if let Some(target) = target {
        let center = ui.scene().and_then(|s| s.surface(&target)).map(|s| {
            let f = s.frame;
            Point::new(f.x + f.size.width / 2., f.y + f.size.height / 2.)
        });
        if within(ui, center)
            && let Some(RackDrag::Part(slot)) = ui.dropped_on::<RackDrag>(target.as_str())
            && let Some(part) = cx.selection.parts.get_mut(slot)
        {
            part.output = n as u8;
        }
    }
    let name = strip_name(ui, cx, Strip::Bus(n));
    let bus = cx.selection.bus(n);
    let sources = cx
        .selection
        .parts
        .iter()
        .filter(|p| !p.path.is_empty() && (usize::from(p.output) == n || p.aux == n as i16))
        .count();
    let sources_el = row![
        glyph(Icon::AudioIn, TEXT, Role::Dim.alpha(1.)),
        caption(match sources {
            0 => "No input".to_owned(),
            n => format!("{n} in"),
        })
        .fill(Role::Dim)
        .lines(1)
        .min_w(0)
    ]
    .gap(TIGHT)
    .align(Align::Center)
    .pad((TIGHT, 0))
    .h(CONTROL - 2.)
    .shrink(0);
    let mut pan = f64::from(bus.pan);
    let pan_el = pan_wedge(ui, &format!("bus-pan-{n}"), &mut pan);
    let mut gain = f64::from(bus.gain);
    let meter = cx.p.clone();
    let fader_el = channel(
        ui,
        &format!("bus-fader-{n}"),
        "Bus volume",
        &mut gain,
        0.,
        !bus.mute,
        move || Meters::read(&meter.shared.meters.buses[n]),
    );
    let (mut solo, mut mute) = (bus.solo, bus.mute);
    let switches = solo_mute(ui, &format!("bus-{n}"), &mut solo, &mut mute);
    let port = if bus.port < 0 { n } else { bus.port as usize };
    let (to, port_el) = route(
        ui,
        &format!("bus-port-{n}"),
        Icon::AudioOut,
        Some(bus_color(n)),
        format!("Out {}", port_text(port)),
        "Host output",
    );
    if to {
        menu::open_under(ui, cx, menu::Target::BusPort(n), &format!("bus-port-{n}"));
    }
    if (pan, gain, solo, mute) != (f64::from(bus.pan), f64::from(bus.gain), bus.solo, bus.mute) {
        let bus = cx.selection.bus_mut(n);
        bus.pan = pan as f32;
        bus.gain = gain as f32;
        bus.solo = solo;
        bus.mute = mute;
    }
    let rows = vec![
        name,
        sources_el,
        pan_el,
        fader_el,
        row![switches].justify(Justify::Center).shrink(0),
        blank(CONTROL - 2. + 2. + 4.),
        port_el,
    ];
    let label = bus.label(n);
    frame(rows, bus_color(n).into(), false)
        .when(over, |e| e.stroke(accent()).stroke_width(1))
        .a11y(A11y::Group)
        .named(label)
        .id(id)
}

/// Everything the host hears, after the Volume parameter.
fn master_strip(ui: &mut Ui, cx: &mut Cx, bridge: &mut Bridge<SamplerParams>) -> El {
    let meter = cx.p.clone();
    let fader_el = bridge.bind_as(ui, P::Volume, "master-fader".into(), |ui, id, v| {
        let mut db = *v * 66. - 60.;
        let el = channel(ui, id.as_str(), "Master volume", &mut db, -12., true, move || {
            Meters::read(&meter.shared.meters.master)
        });
        *v = (db + 60.) / 66.;
        el
    });
    let rows = vec![
        row![body("Master").text_size(SMALL).text_weight(Weight::SEMIBOLD).lines(1)]
            .align(Align::Start)
            .h(NAME)
            .shrink(0),
        row![caption("All buses").fill(Role::Dim).lines(1).min_w(0)]
            .align(Align::Center)
            .pad((TIGHT, 0))
            .h(CONTROL - 2.)
            .shrink(0),
        blank(super::theme::STRIP),
        fader_el,
        blank(super::theme::STRIP),
        blank(CONTROL - 2. + 2. + 4.),
        row![
            glyph(Icon::AudioOut, TEXT, Role::Dim.alpha(1.)),
            caption("Host").fill(Role::Dim).lines(1).min_w(0)
        ]
        .gap(TIGHT)
        .align(Align::Center)
        .pad((TIGHT, 0))
        .h(CONTROL - 2.)
        .shrink(0),
    ];
    frame(rows, Role::Ink.alpha(0.5), false)
        .a11y(A11y::Group)
        .named("Master")
        .id("master-strip")
}

/// A strip's name: dragged onto a bus to route there (instruments),
/// double-clicked to rename, a text field while renaming.
fn strip_name(ui: &mut Ui, cx: &mut Cx, strip: Strip) -> El {
    let (name_id, edit_id, name) = match strip {
        Strip::Part(slot) => (format!("mix-name-{slot}"), format!("mix-rename-{slot}"), super::rack::name(cx, slot)),
        Strip::Bus(n) => (format!("bus-name-{n}"), format!("bus-rename-{n}"), cx.selection.bus(n).label(n)),
    };
    let editing = match strip {
        Strip::Part(slot) => cx.state.renaming.as_mut().filter(|(s, _)| *s == slot),
        Strip::Bus(n) => cx.state.renaming_bus.as_mut().filter(|(s, _)| *s == n),
    };
    if let Some((_, text)) = editing {
        let existed = ui.scene().and_then(|s| s.surface(&edit_id)).is_some();
        if !existed {
            ui.focus(edit_id.as_str());
        }
        let field = text_edit(ui, edit_id.as_str(), text, TextOpts::default());
        let cancel = ui.keys(edit_id.as_str()).iter().any(|k| k.key == Key::Escape);
        let done = field.changed.submitted || (existed && !ui.focused(edit_id.as_str()));
        let el = field.el.h(NAME).min_w(0).named("Strip name");
        if cancel || done {
            let text = match strip {
                Strip::Part(_) => cx.state.renaming.take(),
                Strip::Bus(_) => cx.state.renaming_bus.take(),
            }
            .map(|(_, t)| t.trim().to_owned())
            .unwrap_or_default();
            if done && !cancel {
                rename(cx, strip, text);
            }
        }
        return el;
    }
    let r = ui.get(name_id.as_str());
    if r.double_clicked {
        start_rename(cx, strip);
    } else if r.dragged
        && r.button == Some(Button::Primary)
        && let Strip::Part(slot) = strip
    {
        ui.start_drag(name_id.as_str(), RackDrag::Part(slot));
    }
    let muted = match strip {
        Strip::Part(slot) => cx.selection.parts[slot].mute,
        Strip::Bus(n) => cx.selection.bus(n).mute,
    };
    let tip = match strip {
        Strip::Part(_) => format!("{name}\nDrag onto a bus to route · double-click to rename"),
        Strip::Bus(_) => format!("{name}\nDouble-click to rename"),
    };
    row![
        body(name.clone())
            .text_size(SMALL)
            .text_weight(Weight::SEMIBOLD)
            .fill(if muted { Role::Dim } else { Role::Ink })
            .lines(2)
            .min_w(0)
    ]
    .align(Align::Start)
    .h(NAME)
    .min_w(0)
    .shrink(0)
    .cursor(if matches!(strip, Strip::Part(_)) { Cursor::Grab } else { Cursor::Arrow })
    .tip(tip)
    .named(name)
    .id(name_id)
}

pub fn start_rename(cx: &mut Cx, strip: Strip) {
    match strip {
        Strip::Part(slot) => cx.state.renaming = Some((slot, super::rack::name(cx, slot))),
        Strip::Bus(n) => cx.state.renaming_bus = Some((n, cx.selection.bus(n).label(n))),
    }
}

/// Name a strip; the default name, or nothing, clears the player's own.
fn rename(cx: &mut Cx, strip: Strip, text: String) {
    match strip {
        Strip::Part(slot) => {
            let part = &cx.selection.parts[slot];
            let default = instrument::instrument_of(cx, slot)
                .map_or_else(|| super::header::stem(&part.path), |i| i.name.clone());
            cx.selection.parts[slot].name = if text == default { String::new() } else { text };
        }
        Strip::Bus(n) => {
            let default = Bus::default().label(n);
            cx.selection.bus_mut(n).name = if text == default { String::new() } else { text };
        }
    }
}

/// Level, pan, switches and send back to where a new strip starts; the
/// routing and the name stay.
pub fn reset(cx: &mut Cx, strip: Strip) {
    match strip {
        Strip::Part(slot) => {
            if let Some(part) = cx.selection.parts.get_mut(slot) {
                part.gain = 0.;
                part.pan = 0.;
                part.mute = false;
                part.solo = false;
                part.aux = -1;
                part.aux_gain = 0.;
            }
        }
        Strip::Bus(n) => {
            let bus = cx.selection.bus_mut(n);
            *bus = Bus {
                name: std::mem::take(&mut bus.name),
                port: bus.port,
                ..Bus::default()
            };
        }
    }
}

/// A routing field: an icon, the target's color on its left edge, and
/// what it points at. Opens a menu. [`route`](super::theme::route) with a
/// color, for the labels that name a bus.
fn route(ui: &mut Ui, id: &str, icon: Icon, chip: Option<Color>, text: String, name: &str) -> (bool, El) {
    let hit = ui.get(id).activated();
    let edge = chip.map_or(Role::Ink.alpha(0.), Fill::from);
    let el = row![
        block(2, Len::Pct(100.)).fill(edge).shrink(0),
        glyph(icon, TEXT, Role::Ink.alpha(0.72)),
        body(text.clone()).text_size(SMALL).lines(1).flex(1).min_w(0)
    ]
    .gap(TIGHT)
    .align(Align::Center)
    .pad(edges(0., TIGHT, 0., 0.))
    .h(CONTROL - 2.)
    .min_w(0)
    .fill(Role::Field)
    .focusable()
    .a11y(A11y::Button)
    .named(format!("{name}: {text}"))
    .tip(format!("{name}: {text}"))
    .id(id.to_owned())
    .shrink(0);
    (hit, interactive(el, false))
}

/// A send level as a thin bar; dim and inert when nothing is sent.
fn send_level(ui: &mut Ui, id: &str, db: &mut f64, on: bool) -> El {
    if on {
        drive(ui, id, db, &DB, TRAVEL, false, 0.);
    }
    let unit = (*db - DB.start()) / (DB.end() - DB.start());
    let text = db_text(*db);
    canvas(move |s| {
        let mut d = vec![Draw::fill(rect(0., 0., s.width, s.height), Role::Ink.alpha(0.1))];
        if on {
            d.push(Draw::fill(rect(0., 0., s.width * unit, s.height), value_ink(0.)));
        }
        d
    })
    .w(Len::Pct(100.))
    .h(4)
    .shrink(0)
    .cursor(if on { Cursor::ResizeH } else { Cursor::Arrow })
    .a11y(A11y::Slider { value: *db, min: -60., max: 6. })
    .named("Send level")
    .tip(if on { format!("Send level {text}: drag, double-click for 0 dB") } else { "Pick a send bus first".into() })
    .id(id.to_owned())
}

/// The fader column: a vertical fader with a stereo meter beside it and
/// the level under both. `levels` is read each time the scene is walked,
/// not when the tree is built.
fn channel(
    ui: &mut Ui,
    id: &str,
    name: &str,
    db: &mut f64,
    reset: f64,
    live: bool,
    levels: impl Fn() -> [f32; 2] + Send + Sync + 'static,
) -> El {
    let travel = ui
        .scene()
        .and_then(|s| s.surface(id))
        .map_or(TRAVEL, |s| s.frame.size.height - THUMB.1)
        .max(CONTROL);
    let held = drive(ui, id, db, &DB, travel, true, reset);
    let lift = ui.state(id).hover.max(if held { 1. } else { 0. }) as f32;
    let focused = ui.focus_visible(id);
    let unit = |v: f64| ((v - DB.start()) / (DB.end() - DB.start())).clamp(0., 1.);
    let at = unit(*db);
    // Bottom-up, inside the thumb's half-height at each end.
    let y = move |h: f64, u: f64| THUMB.1 / 2. + (1. - u) * (h - THUMB.1);
    let fader = canvas(move |s| {
        let mid = (s.width / 2.).round();
        let mut d = vec![Draw::fill(rect(mid - 1., y(s.height, 1.), 2., s.height - THUMB.1), Role::Ink.alpha(0.14))];
        for mark in [0., -12., -24., -36., -48.] {
            let w = if mark == 0. { 6. } else { 3. };
            d.push(Draw::fill(
                rect(mid - 4. - w, y(s.height, unit(mark)).round(), w, 1.),
                Role::Ink.alpha(if mark == 0. { 0.4 } else { 0.2 }),
            ));
        }
        let top = y(s.height, at);
        if at > 0. {
            let fill: Fill = if live { accent().with_alpha(0.7).into() } else { Role::Ink.alpha(0.3) };
            d.push(Draw::fill(rect(mid - 1., top, 2., s.height - THUMB.1 / 2. - top), fill));
        }
        let (w, h) = THUMB;
        d.push(Draw::fill(rect((mid - w / 2.).round(), (top - h / 2.).round(), w, h), value_ink(lift)));
        d.push(Draw::fill(rect((mid - w / 2.).round() + 2., top.round(), w - 4., 1.), Role::Background.alpha(1.)));
        if focused {
            d.push(Draw::stroke(rect(0.5, 0.5, s.width - 1., s.height - 1.), Role::Primary.alpha(0.9), 1.));
        }
        d
    })
    .w(CONTROL)
    .h(Len::Pct(100.))
    .shrink(0)
    .cursor(Cursor::ResizeV)
    .focusable()
    .a11y(A11y::Slider { value: *db, min: -60., max: 6. })
    .named(name.to_owned())
    .tip(format!("{name}: drag, Shift for fine, wheel, double-click to reset"))
    .id(id.to_owned());
    // The meter keeps the fader's end margins, so both read on one scale.
    let meter = col![meter_v(levels).id(format!("{id}-meter"))]
        .pad((0., THUMB.1 / 2.))
        .align(Align::Stretch)
        .shrink(0);
    col![
        row![spacer(), fader, meter, spacer()]
            .gap(TIGHT)
            .align(Align::Stretch)
            .flex(1)
            .min_h(CONTROL * 3.),
        row![caption(db_short(*db)).text_size(SMALL).reserve("-00.0")]
            .justify(Justify::Center)
            .shrink(0)
    ]
    .gap(TIGHT)
    .flex(1)
    .min_h(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readouts() {
        assert_eq!(db_short(0.2), "+0.2");
        assert_eq!(db_short(-10.), "-10.0");
        assert_eq!(db_short(-0.01), "0.0");
        assert_eq!(db_short(-60.), "-inf");
        assert_eq!(port_text(0), "1/2");
        assert_eq!(port_text(15), "31/32");
        assert_eq!(input_text(1, -1), "B Omni");
        assert_eq!(input_text(0, 9), "A 10");
    }
}
