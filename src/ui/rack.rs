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
            panels.push(instrument::stage(ui, cx, slot));
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

/// A bare number field: drag, type or step it.
fn field(
    ui: &mut Ui,
    id: String,
    name: &str,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    display: String,
    widest: &str,
) -> El {
    drag_value(ui, id, name, value, range)
        .size(S)
        .value_text(display)
        .el
        .el()
        .h(CONTROL)
        .reserve(widest.to_owned())
        .shrink(0)
}

/// A part's header: fold, name and preset stepping over what it is; MIDI and
/// output routing; solo and mute; level and pan; audition, menu and remove.
pub fn header(ui: &mut Ui, cx: &mut Cx, slot: usize) -> El {
    let id = format!("header-{slot}");
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
    let prev_el = prev_el.when(before.is_none(), |e| e.disabled().opacity(0.35));
    let next_el = next_el.when(after.is_none(), |e| e.disabled().opacity(0.35));
    let (audition, play_el) = icon_button(ui, format!("audition-{slot}"), Icon::Play, "Audition (Space)", false);
    if audition {
        cx.state.selected = slot;
        cx.p.shared.audition(None);
    }
    let more_id = format!("more-{slot}");
    let (more, more_el) = icon_button(ui, more_id.as_str(), Icon::More, "Part menu", false);
    if more {
        menu::open_under(ui, cx, menu::Target::Part(slot), &more_id);
    }
    let (remove, remove_el) = icon_button(ui, format!("remove-{slot}"), Icon::Close, "Remove from rack", false);

    let title = title(ui, cx, slot);
    let facts = facts(cx, slot);
    let progress = cx.view.parts[slot].loading.then(|| {
        let done = cx.p.shared.load_progress[slot].load(Ordering::Relaxed);
        f64::from(done) / f64::from(crate::engine::LOAD_DONE)
    });
    let library = cx.library_of(Path::new(&cx.selection.parts[slot].path));
    let tint = cx.state.tint(cx.view, &library);

    let part = &mut cx.selection.parts[slot];
    // Ports count from 1 when typed, and read A to D.
    let mut port = f64::from(part.port) + 1.;
    let port_el = field(
        ui,
        format!("port-{slot}"),
        "MIDI port",
        &mut port,
        1.0..=4.0,
        char::from(b'A' + part.port.min(3)).to_string(),
        "D",
    );
    part.port = port.round() as u8 - 1;
    let mut channel = f64::from(part.channel + 1);
    let channel_text = if part.channel < 0 {
        "Omni".to_owned()
    } else {
        (part.channel + 1).to_string()
    };
    let channel_el = field(ui, format!("channel-{slot}"), "MIDI channel", &mut channel, 0.0..=16.0, channel_text, "Omni");
    part.channel = channel.round() as i16 - 1;
    let mut output = f64::from(part.output) + 1.;
    let output_el = field(
        ui,
        format!("output-{slot}"),
        "Output",
        &mut output,
        1.0..=8.0,
        format!("st.{}", part.output + 1),
        "st.8",
    );
    part.output = output.round() as u8 - 1;
    let (solo, solo_el) = latch(ui, format!("solo-{slot}"), "S", "Solo", part.solo);
    if solo {
        part.solo = !part.solo;
    }
    let (mute, mute_el) = latch(ui, format!("mute-{slot}"), "M", "Mute", part.mute);
    if mute {
        part.mute = !part.mute;
    }
    let mut gain = f64::from(part.gain);
    let track = TEXT * 8.;
    let (_, gain_el) = fader(ui, &format!("volume-{slot}"), "Volume", &mut gain, -60.0..=6.0, Fader::LEVEL.length(track), db_text);
    part.gain = gain as f32;
    let mut pan = f64::from(part.pan);
    let (_, pan_el) = fader(ui, &format!("pan-{slot}"), "Pan", &mut pan, -1.0..=1.0, Fader::PAN.length(track), pan_text);
    part.pan = pan as f32;
    if remove {
        cx.remove(slot);
    }

    // Row labels share one width, so both rows' controls start in line.
    let label = |text: &str| section(text).reserve("MIDI").shrink(0);
    let identity = col![
        row![title, prev_el, next_el].gap(TIGHT).align(Align::Center).h(CONTROL),
        row![caption(facts).fill(Role::Dim).lines(1).min_w(0)]
            .align(Align::Center)
            .h(CONTROL)
    ]
    .gap(TIGHT)
    .flex(1)
    .min_w(0);
    let routing = col![
        cluster(vec![label("MIDI"), port_el, channel_el]).h(CONTROL),
        cluster(vec![label("Out"), output_el]).h(CONTROL)
    ]
    .gap(TIGHT)
    .shrink(0);
    let switches = col![segmented(vec![solo_el, mute_el]), play_el]
        .gap(TIGHT)
        .align(Align::Center)
        .shrink(0);
    let faders = col![
        cluster(vec![label("Vol"), gain_el]).h(CONTROL),
        cluster(vec![label("Pan"), pan_el]).h(CONTROL)
    ]
    .gap(TIGHT)
    .shrink(0);
    let actions = col![cluster(vec![more_el, remove_el]), spacer()].gap(TIGHT).shrink(0);
    let body = row![
        col![fold_el, spacer()].gap(TIGHT).shrink(0),
        identity,
        vrule(),
        routing,
        vrule(),
        switches,
        vrule(),
        faders,
        actions
    ]
    .gap(SPACE + TIGHT)
    .align(Align::Stretch)
    .pad((SPACE, SPACE))
    .flex(1)
    .min_w(0);
    // The library's color runs down the left edge; the bottom edge doubles
    // as load progress, so loading moves nothing.
    let edge = block(TIGHT - 1., Len::Pct(100.))
        .fill(tint.map_or(Role::Ink.alpha(0.12), Fill::from))
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
    let name = name(cx, slot);
    col![row![edge, body].gap(0).align(Align::Stretch), bar]
        .gap(0)
        .fill(if selected { Role::Raised } else { Role::Surface })
        .when(over, |e| e.stroke(accent()).stroke_width(1))
        .a11y(A11y::Group)
        .named(name)
        .id(id)
        .shrink(0)
}

/// What the part is: its library, size, and load progress while loading.
fn facts(cx: &Cx, slot: usize) -> String {
    let v = &cx.view.parts[slot];
    let library = cx.library_of(Path::new(&cx.selection.parts[slot].path));
    let mut facts = vec![library_label(&library)];
    if let Some(i) = instrument::instrument_of(cx, slot) {
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
    facts.join("  ·  ")
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
        let el = field.el.h(CONTROL).flex(1).min_w(0).named("Part name");
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
        .text_size(TEXT + TIGHT)
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
