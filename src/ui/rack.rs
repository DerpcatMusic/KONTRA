//! The rack: every part stacked Kontakt-style, each under its own header.
//!
//! A header names the part, steps its preset, routes it and mixes it; its
//! chevron folds the part down to the header alone, its ✕ removes it. Below
//! an open header sit the part's notices and performance controls. Presets
//! dropped on a header replace that part; parts dragged by their name
//! reorder; anything dropped on the rack's foot is added.

use super::{Cx, RackDrag, instrument, menu, move_part, theme::*};
use crate::import;
use moose::mui::mui::prelude::*;
use std::path::Path;
use std::sync::atomic::Ordering;

/// The display name of a part: the player's, the loaded instrument's, else the file's.
pub fn name(cx: &Cx, slot: usize) -> String {
    let Some(part) = cx.selection.parts.get(slot) else {
        return String::new();
    };
    if !part.name.is_empty() {
        return part.name.clone();
    }
    instrument::instrument_of(cx, slot)
        .map(|i| i.name.clone())
        .unwrap_or_else(|| super::header::stem(&part.path))
}

/// Every part in rack order, then the foot that adds more.
pub fn view(ui: &mut Ui, cx: &mut Cx) -> El {
    let mut panels = Vec::new();
    for slot in cx.selection.order.clone() {
        let slot = slot as usize;
        panels.push(header(ui, cx, slot));
        if !cx.selection.parts[slot].collapsed {
            panels.extend(instrument::notices(cx, slot));
            // Kept as drawn while nothing it shows moves: meters and keys
            // redraw around it, not through it.
            let deps = (instrument::stage_deps(cx, slot), cx.selection.appearance);
            let stage = ui.memo(format!("stage-memo-{slot}"), deps, |ui| instrument::stage(ui, cx, slot));
            panels.push(behind(cx, slot, stage));
        }
        panels.push(rule());
    }
    if panels.is_empty() {
        panels.push(instrument::welcome(cx));
    }
    panels.push(foot(ui, cx));
    col(panels)
        .gap(0)
        .align(Align::Stretch)
        .flex(1)
        .min_h(0)
        .scroll()
        .id("rack-scroll")
}

/// A part's controls over what the appearance puts behind them.
fn behind(cx: &mut Cx, slot: usize, stage: El) -> El {
    let look = super::Appearance::of(cx.selection.appearance);
    if look == super::Appearance::Plain {
        return stage;
    }
    let library = cx.library_of(Path::new(&cx.selection.parts[slot].path));
    match look {
        super::Appearance::Color => match cx.tint(&library) {
            Some(tint) => stage.fill(Color::oklch(0.225, 0.026, tint.hue())),
            None => stage,
        },
        _ => match cx.looks(&library).and_then(|l| l.backdrop[usize::from(cx.blurred())].clone()) {
            Some(image) => stack![
                block(Len::Pct(100.), Len::Pct(100.))
                    .fill(Fill::Image(image, moose::mui::mui::scene::Fit::Cover)),
                stage
            ]
            .w(Len::Pct(100.))
            .clip(),
            None => stage,
        },
    }
}

/// Where presets are dropped to be added, and a button that finds one.
fn foot(ui: &mut Ui, cx: &mut Cx) -> El {
    let dragging = ui.dragging::<RackDrag>().is_some();
    let over = ui.get("rack-drop").drop_target;
    if let Some(RackDrag::Instrument(path)) = ui.dropped_on::<RackDrag>("rack-drop") {
        cx.state.notice.clear();
        if import::is_multi(Path::new(&path)) {
            cx.p.shared.queue_multi(path);
        } else {
            cx.add(path);
        }
    }
    if ui.get("rack-drop").activated() {
        cx.state.browser = true;
        ui.focus("search");
    }
    let loaded = cx.selection.order.len();
    let text = if dragging {
        "Drop to add to the rack".to_owned()
    } else {
        format!("Add an instrument · {loaded} of 16")
    };
    let el = row![
        glyph(Icon::Plus, TEXT, Role::Ink.alpha(if dragging { 0.9 } else { 0.5 })),
        caption(text).fill(Role::Dim).lines(1)
    ]
    .gap(SPACE)
    .align(Align::Center)
    .justify(Justify::Center)
    .pad(SPACE)
    .h(CONTROL + 2. * SPACE)
    .w(Len::Pct(100.))
    .when(over, |e| e.stroke(accent()).stroke_width(1))
    .focusable()
    .a11y(A11y::Button)
    .named("Add to rack")
    .tip("Drop a preset here to add it, or click to search the browser")
    .id("rack-drop")
    .shrink(0);
    interactive(el, false)
}

/// Previous or next preset in the part's library folder, if there is one.
fn step_preset(cx: &Cx, slot: usize, by: isize) -> Option<String> {
    let path = &cx.selection.parts[slot].path;
    let library = cx.library_of(Path::new(path));
    let multi = import::is_multi(Path::new(path));
    let siblings: Vec<_> = cx
        .view
        .files
        .iter()
        .filter(|p| import::is_multi(p) == multi && cx.library_of(p) == library)
        .collect();
    let at = siblings.iter().position(|p| p.to_string_lossy() == *path)?;
    let to = at.checked_add_signed(by)?;
    siblings.get(to).map(|p| p.to_string_lossy().into_owned())
}

/// Below this width the routing and mix controls take a second line under
/// the name: about the controls' width plus a name's.
const ONE_LINE: f64 = 860.;

/// The header banner: the library's artwork fading out over this width, and
/// taller than any header so covering one only ever crops it top and bottom.
pub const BANNER: (f64, f64) = (TEXT * 36., TEXT * 7.);

/// A part's header, after Koda's part strip: a thin bar with the library's
/// color down its left edge. The fold, the name over what it is, preset
/// stepping; MIDI and output routing, pan, gain and tune; solo and mute,
/// the activity dot, menu and remove; the part's meter at the right edge.
/// Folded, it is one slim line.
pub fn header(ui: &mut Ui, cx: &mut Cx, slot: usize) -> El {
    let id = format!("header-{slot}");
    // Last frame's width: the header spans the rack, so its own layout never
    // changes it and the choice can't oscillate.
    let narrow = ui
        .scene()
        .and_then(|s| s.surface(&id))
        .is_some_and(|s| s.frame.size.width < ONE_LINE);
    let selected = cx.state.selected == slot;
    let r = ui.get(id.as_str());
    if r.clicked {
        cx.state.selected = slot;
    }
    if r.clicked_with(Button::Secondary) {
        menu::open(ui, cx, menu::Target::Part(slot));
    }
    // A preset dropped on the header replaces the part; a part lands before it.
    let over = r.drop_target
        && match ui.dragging::<RackDrag>() {
            Some(RackDrag::Part(from)) => *from != slot,
            Some(RackDrag::Instrument(_)) => true,
            None => false,
        };
    match ui.dropped_on::<RackDrag>(id.as_str()) {
        Some(RackDrag::Instrument(path)) => cx.replace(slot, path),
        Some(RackDrag::Part(from)) => move_part(&mut cx.selection, from, slot),
        None => {}
    }

    let collapsed = cx.selection.parts[slot].collapsed;
    let (fold, fold_el) = icon_button(
        ui,
        format!("collapse-{slot}"),
        if collapsed { Icon::Right } else { Icon::Down },
        if collapsed { "Show controls" } else { "Hide controls" },
        false,
    );
    if fold {
        cx.selection.parts[slot].collapsed = !collapsed;
    }
    let (previous, prev_el) = icon_button(ui, format!("preset-prev-{slot}"), Icon::Left, "Previous preset", false);
    let (next, next_el) = icon_button(ui, format!("preset-next-{slot}"), Icon::Right, "Next preset", false);
    let before = step_preset(cx, slot, -1);
    let after = step_preset(cx, slot, 1);
    if let Some(path) = before.clone().filter(|_| previous).or(after.clone().filter(|_| next)) {
        cx.replace(slot, path);
    }
    let prev_el = prev_el.when(before.is_none(), |e| e.disabled().opacity(0.3));
    let next_el = next_el.when(after.is_none(), |e| e.disabled().opacity(0.3));
    let more_id = format!("more-{slot}");
    let (more, more_el) = icon_button(ui, more_id.as_str(), Icon::More, "Part menu", false);
    if more {
        menu::open_under(ui, cx, menu::Target::Part(slot), &more_id);
    }
    let (remove, remove_el) = icon_button(ui, format!("remove-{slot}"), Icon::Close, "Remove from rack", false);

    let midi_id = format!("midi-{slot}");
    let output_id = format!("output-{slot}");
    let part = &cx.selection.parts[slot];
    let channel = if part.channel < 0 { "Omni".to_owned() } else { (part.channel + 1).to_string() };
    // Port A is the usual one and goes unsaid.
    let midi_text = if part.port == 0 {
        channel
    } else {
        format!("{}{channel}", char::from(b'A' + part.port.min(3)))
    };
    let output_text = cx.selection.bus(part.output.into()).label(part.output.into());
    let (midi, midi_el) = route(ui, midi_id.as_str(), Icon::MidiIn, &midi_text, "D16", "MIDI input");
    if midi {
        menu::open_under(ui, cx, menu::Target::Midi(slot), &midi_id);
    }
    let (output, output_el) = route(ui, output_id.as_str(), Icon::AudioOut, &output_text, "st.16", "Output");
    if output {
        menu::open_under(ui, cx, menu::Target::Output(slot), &output_id);
    }

    let title = title(ui, cx, slot);
    let full = facts(cx, slot, false);
    let facts = if narrow { facts(cx, slot, true) } else { full.clone() };
    let progress = cx.view.parts[slot].loading.then(|| {
        let done = cx.p.shared.load_progress[slot].load(Ordering::Relaxed);
        f64::from(done) / f64::from(crate::engine::LOAD_DONE)
    });
    let library = cx.library_of(Path::new(&cx.selection.parts[slot].path));
    let tint = cx.tint(&library);
    let banner = cx.looks(&library).and_then(|l| l.banner[usize::from(cx.blurred())].clone());

    let part = &mut cx.selection.parts[slot];
    let mut gain = f64::from(part.gain);
    let gain_el = gain_knob(ui, &format!("volume-{slot}"), &mut gain);
    part.gain = gain as f32;
    let mut pan = f64::from(part.pan);
    let pan_el = pan_wedge(ui, &format!("pan-{slot}"), &mut pan);
    part.pan = pan as f32;
    let mut tune = f64::from(part.tune);
    let range = f64::from(crate::engine::TUNE_RANGE);
    let tune_el = tune_field(ui, &format!("tune-{slot}"), &mut tune, -range..=range);
    part.tune = tune as f32;
    let (mut solo, mut mute) = (part.solo, part.mute);
    let switches = solo_mute(ui, &slot.to_string(), &mut solo, &mut mute);
    (part.solo, part.mute) = (solo, mute);
    let p = cx.p.clone();
    let level = move || crate::plugin::Meters::read(&p.shared.meters.parts[slot]);
    let p = cx.p.clone();
    let dot = activity_dot(move || crate::plugin::Meters::read(&p.shared.meters.parts[slot]) != [0.; 2]);
    if remove {
        cx.remove(slot);
    }

    let facts_el = |narrow: bool| {
        caption(facts.to_uppercase())
            .text_size(SMALL - 2.)
            .text_weight(Weight::SEMIBOLD)
            .fill(Role::Dim)
            .lines(1)
            .min_w(0)
            .tip(full.clone())
            .when(narrow, |e| e.flex(1))
    };
    let name_row = row![title, prev_el, next_el].gap(0).align(Align::Center).min_w(0);
    // Folded and narrow, only the level stays in the line.
    let mix = if collapsed && narrow {
        gain_el
    } else {
        cluster(vec![midi_el, output_el, pan_el, gain_el, tune_el]).gap(SPACE)
    };
    let tail = cluster(vec![switches, dot, more_el, remove_el]).gap(TIGHT + 1.);
    let rows = match (collapsed, narrow) {
        // Folded: one slim line, what it is beside its name.
        (true, _) => vec![
            row![name_row.shrink(1), facts_el(true)]
                .gap(SPACE)
                .align(Align::Center)
                .flex(1)
                .min_w(0),
            mix,
            tail,
        ],
        (false, false) => vec![
            col![facts_el(false), name_row].gap(0).align(Align::Start).flex(1).min_w(0),
            mix,
            tail,
        ],
        (false, true) => vec![
            col![
                row![name_row.flex(1), tail].gap(SPACE).align(Align::Center),
                row![facts_el(true), mix].gap(SPACE).align(Align::Center)
            ]
            .gap(2)
            .align(Align::Stretch)
            .flex(1)
            .min_w(0),
        ],
    };
    let mut line = vec![fold_el];
    line.extend(rows);
    let body = row(line)
        .gap(SPACE)
        .align(Align::Center)
        .pad(edges(2., SPACE, 2., TIGHT))
        .flex(1)
        .min_w(0);
    // The library's color runs down the left edge, the accent's when it has
    // none and the part is selected; the bottom edge doubles as load progress.
    let edge = block(3, Len::Pct(100.))
        .fill(match tint {
            Some(t) => Fill::from(t),
            None if selected => accent().into(),
            None => Role::Ink.alpha(0.12),
        })
        .shrink(0);
    let bar = canvas(move |s| match progress {
        Some(done) => vec![
            Draw::fill(rect(0., 0., s.width, 1.), Role::Ink.alpha(0.12)),
            Draw::fill(rect(0., 0., (s.width * done.clamp(0., 1.)).max(SPACE), 1.), accent()),
        ],
        None => Vec::new(),
    })
    .w(Len::Pct(100.))
    .h(1)
    .shrink(0);
    let meter = col![meter_v(level)].pad((3, 0)).h(Len::Pct(100.)).shrink(0);
    let name = name(cx, slot);
    let line = row![edge, body, meter].gap(0).align(Align::Stretch);
    let line = match banner {
        Some(image) => stack![
            block(BANNER.0, Len::Pct(100.))
                .fill(Fill::Image(image, moose::mui::mui::scene::Fit::Cover))
                .anchor(Align::Start, Align::Start),
            line
        ]
        .w(Len::Pct(100.))
        .clip(),
        None => line,
    };
    col![line, bar]
        .gap(0)
        .fill(if selected { Role::Raised } else { Role::Surface })
        .when(over, |e| e.stroke(accent()).stroke_width(1))
        .a11y(A11y::Group)
        .named(name)
        .id(id)
        .shrink(0)
}

/// What the part is: its library, size, and load progress while loading.
/// `brief` leaves out the group and zone counts, for a narrow header.
fn facts(cx: &Cx, slot: usize, brief: bool) -> String {
    let v = &cx.view.parts[slot];
    let library = cx.library_of(Path::new(&cx.selection.parts[slot].path));
    // "Areia 1.2.0 [Audio Imperia]": the vendor in brackets goes.
    let library = library_label(&library);
    let mut facts = vec![library.split(" [").next().unwrap_or_default().to_owned()];
    if let Some(i) = instrument::instrument_of(cx, slot).filter(|_| !brief) {
        facts.push(format!("{} groups · {} zones", i.groups.len(), i.zones.len()));
    }
    if v.loading {
        let done = cx.p.shared.load_progress[slot].load(Ordering::Relaxed);
        let percent = f64::from(done) / f64::from(crate::engine::LOAD_DONE) * 100.;
        facts.push(format!("loading samples {percent:.0}%"));
    } else if v.bytes > 0 {
        facts.push(megabytes(v.bytes));
    }
    if v.status.starts_with("Load failed") {
        facts.push("failed to load".into());
    }
    facts.retain(|f| !f.is_empty());
    facts.join(" · ")
}

/// The part's name, dragged to reorder and double-clicked to rename; a
/// text field while renaming.
fn title(ui: &mut Ui, cx: &mut Cx, slot: usize) -> El {
    let name_id = format!("name-{slot}");
    let edit_id = format!("rename-{slot}");
    let name = name(cx, slot);
    if let Some((_, text)) = cx.state.renaming.as_mut().filter(|(s, _)| *s == slot) {
        let existed = ui.scene().and_then(|s| s.surface(&edit_id)).is_some();
        if !existed {
            ui.focus(edit_id.as_str());
        }
        let field = text_edit(ui, edit_id.as_str(), text, TextOpts::default());
        let cancel = ui.keys(edit_id.as_str()).iter().any(|k| k.key == Key::Escape);
        let done = field.changed.submitted || (existed && !ui.focused(edit_id.as_str()));
        let el = field.el.h(STRIP).flex(1).min_w(0).named("Part name");
        if cancel {
            cx.state.renaming = None;
        } else if done {
            let text = cx.state.renaming.take().map(|(_, t)| t).unwrap_or_default();
            let text = text.trim();
            let part = &cx.selection.parts[slot];
            let default = instrument::instrument_of(cx, slot)
                .map_or_else(|| super::header::stem(&part.path), |i| i.name.clone());
            cx.selection.parts[slot].name = if text == default || text.is_empty() {
                String::new()
            } else {
                text.to_owned()
            };
        }
        return el;
    }
    let r = ui.get(name_id.as_str());
    if r.double_clicked {
        cx.state.renaming = Some((slot, name.clone()));
    } else if r.clicked {
        cx.state.selected = slot;
    }
    if r.dragged && r.button == Some(Button::Primary) {
        ui.start_drag(name_id.as_str(), RackDrag::Part(slot));
    }
    let muted = cx.selection.parts[slot].mute;
    body(name.clone())
        .text_size(TEXT + 1.)
        .text_weight(Weight::SEMIBOLD)
        .fill(if muted { Role::Dim } else { Role::Ink })
        .lines(1)
        .min_w(0)
        .shrink(1)
        .cursor(Cursor::Grab)
        .tip(format!("{name}\nDrag to reorder · double-click to rename"))
        .named(name)
        .id(name_id)
}
