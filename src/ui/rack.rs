//! The rack: a strip of part chips to switch between, and the mixer table
//! with routing, level, pan and mute/solo per part.

use super::{Cx, RackDrag, Tab, menu, move_part, theme::*};
use moose::mui::mui::prelude::*;
use std::path::Path;

/// The display name of a part: the player's, the loaded instrument's, else the file's.
pub fn name(cx: &Cx, slot: usize) -> String {
    let Some(part) = cx.selection.parts.get(slot) else {
        return String::new();
    };
    if !part.name.is_empty() {
        return part.name.clone();
    }
    let v = &cx.view.parts[slot];
    v.instrument
        .as_ref()
        .filter(|i| i.path == Path::new(&part.path) && v.program == part.program)
        .map(|i| i.name.clone())
        .unwrap_or_else(|| super::header::stem(&part.path))
}

/// Accept presets and parts dropped on `id`, which stands for `slot`.
fn drop_target(ui: &mut Ui, cx: &mut Cx, id: &str, slot: usize) {
    match ui.dropped_on::<RackDrag>(id) {
        Some(RackDrag::Instrument(path)) => cx.replace(slot, path),
        Some(RackDrag::Part(from)) => move_part(&mut cx.selection, from, slot),
        None => {}
    }
}

/// How a drop over `id` would land: a part inserts before it, a preset replaces it.
fn drop_look(ui: &Ui, id: &str, slot: usize) -> Option<bool> {
    if !ui.get(id).drop_target {
        return None;
    }
    match ui.dragging::<RackDrag>()? {
        RackDrag::Part(from) if *from == slot => None,
        RackDrag::Part(_) => Some(true),
        RackDrag::Instrument(_) => Some(false),
    }
}

/// One chip per part, in rack order, then a drop target that adds.
pub fn strip(ui: &mut Ui, cx: &mut Cx) -> El {
    let mut chips = Vec::new();
    for (position, slot) in cx.selection.order.clone().into_iter().map(|n| n as usize).enumerate() {
        let id = format!("part-{slot}");
        let name = name(cx, slot);
        let current = cx.state.selected == slot;
        let r = ui.get(id.as_str());
        if r.activated() {
            cx.state.selected = slot;
            cx.state.renaming = None;
            cx.state.library = Some(cx.library_of(Path::new(&cx.selection.parts[slot].path)));
            if cx.state.tab == Tab::Rack {
                cx.state.tab = Tab::Perform;
            }
        }
        if r.double_clicked {
            cx.show(slot);
            cx.state.renaming = Some(name.clone());
        }
        if r.clicked_with(Button::Secondary) {
            menu::open(ui, cx, menu::Target::Part(slot));
        }
        if r.dragged && r.button == Some(Button::Primary) {
            ui.start_drag(id.as_str(), RackDrag::Part(slot));
        }
        let look = drop_look(ui, &id, slot);
        drop_target(ui, cx, &id, slot);
        let part = &cx.selection.parts[slot];
        let (mute, solo) = (part.mute, part.solo);
        let failed = cx.view.parts[slot].status.starts_with("Load failed");
        let mut label = vec![
            caption(format!("{:02}", position + 1))
                .text_size(10)
                .fill(Role::Dim),
            body(fit(&name, 28))
                .text_size(12)
                .fill(if current && !mute { Role::Ink } else { Role::Dim })
                .lines(1)
                .min_w(0),
        ];
        if solo {
            label.push(caption("S").text_size(9).text_weight(Weight::BOLD).fill(accent()));
        }
        if mute {
            label.push(caption("M").text_size(9).text_weight(Weight::BOLD).fill(Role::Dim));
        }
        if failed {
            label.push(block(5, 5).fill(Role::Danger.alpha(1.)));
        }
        let underline = if current {
            Fill::from(accent())
        } else {
            Role::Ink.alpha(0.)
        };
        let chip = col![
            row(label)
                .gap(HALF + 2.)
                .align(Align::Center)
                .pad((GAP + 2., 0))
                .flex(1),
            block(Len::Pct(100.), 2).fill(underline)
        ]
        .gap(0)
        .h(CONTROL + 2.)
        .max_size(Size::new(220., 40.))
        .when(current, |e| e.fill(Role::Raised))
        .when(look == Some(false), |e| e.stroke(accent()).stroke_width(1))
        .focusable()
        .a11y(A11y::Button)
        .named(format!("{name}{}", if mute { ", muted" } else { "" }))
        .tip(format!("{name}\nDrag to reorder · drop a preset to replace · right-click for more"))
        .id(id)
        .shrink(0);
        let chip = interactive(chip, current);
        // Where a dragged part would land: an accent edge before this chip.
        chips.push(if look == Some(true) {
            row![block(2, CONTROL + 2.).fill(accent()), chip].gap(0).shrink(0)
        } else {
            chip
        });
    }
    let dragging = ui.dragging::<RackDrag>().is_some();
    let over = ui.get("rack-drop").drop_target;
    if let Some(RackDrag::Instrument(path)) = ui.dropped_on::<RackDrag>("rack-drop") {
        cx.state.notice.clear();
        if crate::import::is_multi(Path::new(&path)) {
            cx.p.shared.queue_multi(path);
        } else {
            cx.add(path);
        }
    }
    if ui.get("rack-drop").activated() {
        cx.state.browser = true;
        ui.focus("search");
    }
    let add = row![
        glyph(Icon::Plus, 12., Role::Ink.alpha(if dragging { 0.9 } else { 0.5 })),
        caption(if dragging { "Drop to add" } else { "Add" }).fill(Role::Dim)
    ]
    .gap(HALF + 2.)
    .align(Align::Center)
    .pad((GAP + 2., 0))
    .h(CONTROL + 2.)
    .stroke(if over {
        Fill::from(accent())
    } else {
        Role::Ink.alpha(if dragging { 0.35 } else { 0.12 })
    })
    .stroke_width(1)
    .radius(2)
    .focusable()
    .a11y(A11y::Button)
    .named("Add to rack: drop a preset here")
    .tip("Drop a preset here to add it to the rack")
    .id("rack-drop")
    .shrink(0);
    chips.push(interactive(add, false));
    let loaded = cx.selection.order.len();
    row![
        row![section("Rack"), caption(format!("{loaded}/16")).text_size(10).fill(Role::Dim)]
            .gap(HALF + 2.)
            .align(Align::Center)
            .shrink(0),
        row(chips)
            .gap(HALF)
            .align(Align::Center)
            .pad((0, HALF))
            .flex(1)
            .min_w(0)
            .scroll()
            .id("rack-chips")
    ]
    .gap(WIDE)
    .align(Align::Center)
    .pad(edges(0., GAP, 0., GAP + HALF))
    .h(BAR + 8.)
    .shrink(0)
    .fill(Role::Surface)
}

const INDEX: f64 = 28.;
const PORT: f64 = 36.;
const CHANNEL: f64 = 52.;
const OUTPUT: f64 = 44.;
const SWITCHES: f64 = 44.;

/// Every part with its routing and mix, one row each, in rack order.
pub fn mixer(ui: &mut Ui, cx: &mut Cx) -> El {
    let mut rows = Vec::new();
    let mut remove = None;
    let order = cx.selection.order.clone();
    for (position, &slot) in order.iter().enumerate() {
        let slot = slot as usize;
        let name = name(cx, slot);
        let row_id = format!("rack-row-{slot}");
        let rr = ui.get(row_id.as_str());
        if rr.clicked_with(Button::Secondary) {
            menu::open(ui, cx, menu::Target::Part(slot));
        }
        let look = drop_look(ui, &row_id, slot);
        drop_target(ui, cx, &row_id, slot);
        let grip_id = format!("rack-grip-{slot}");
        if ui.get(grip_id.as_str()).dragged {
            ui.start_drag(grip_id.as_str(), RackDrag::Part(slot));
        }
        let name_id = format!("rack-name-{slot}");
        let rn = ui.get(name_id.as_str());
        if rn.activated() {
            cx.show(slot);
        }
        if rn.clicked_with(Button::Secondary) {
            menu::open(ui, cx, menu::Target::Part(slot));
        }
        let selected = cx.state.selected == slot;
        let failed = cx.view.parts[slot].status.starts_with("Load failed");
        let part = &mut cx.selection.parts[slot];
        let mut port = f64::from(part.port) + 1.;
        let port_text = char::from(b'A' + part.port.min(3)).to_string();
        let port_el = field(ui, format!("port-{slot}"), "MIDI port", &mut port, 1.0..=4.0, port_text, PORT);
        part.port = port.round().clamp(1., 4.) as u8 - 1;
        let mut channel = f64::from(part.channel + 1);
        let channel_text = if part.channel < 0 {
            "Omni".into()
        } else {
            format!("{}", part.channel + 1)
        };
        let channel_el = field(ui, format!("channel-{slot}"), "MIDI channel", &mut channel, 0.0..=16.0, channel_text, CHANNEL);
        part.channel = channel.round() as i16 - 1;
        let mut output = f64::from(part.output) + 1.;
        let output_el = field(ui, format!("output-{slot}"), "Output", &mut output, 1.0..=8.0, format!("st.{}", part.output + 1), OUTPUT);
        part.output = output.round() as u8 - 1;
        let (mute, mute_el) = letter_toggle(ui, format!("mute-{slot}"), "M", &format!("Mute {name}"), part.mute);
        if mute {
            part.mute = !part.mute;
        }
        let (solo, solo_el) = letter_toggle(ui, format!("solo-{slot}"), "S", &format!("Solo {name}"), part.solo);
        if solo {
            part.solo = !part.solo;
        }
        let mut gain = f64::from(part.gain);
        let (_, gain_el) = fader(ui, &format!("gain-{slot}"), &format!("{name} volume"), &mut gain, -60.0..=6.0, Fader::LEVEL, db_text, None);
        part.gain = gain as f32;
        let mut pan = f64::from(part.pan);
        let (_, pan_el) = fader(ui, &format!("pan-{slot}"), &format!("{name} pan"), &mut pan, -1.0..=1.0, Fader::PAN, pan_text, None);
        part.pan = pan as f32;
        let (close, close_el) = icon_button(ui, format!("remove-{slot}"), Icon::Close, &format!("Remove {name}"), false);
        if close {
            remove = Some(slot);
        }
        let grip = row![caption(format!("{:02}", position + 1)).text_size(11).fill(Role::Dim)]
            .align(Align::Center)
            .justify(Justify::Center)
            .w(INDEX)
            .h(CONTROL)
            .cursor(Cursor::Grab)
            .tip("Drag to reorder")
            .named(format!("Move {name}"))
            .id(grip_id)
            .shrink(0);
        let name_el = row![
            body(name.clone())
                .text_size(12)
                .fill(if selected { Role::Ink } else { Role::Dim })
                .lines(1)
                .min_w(0)
        ]
        .align(Align::Center)
        .pad((GAP, 0))
        .h(CONTROL)
        .radius(2)
        .flex(2)
        .min_w(80)
        .focusable()
        .a11y(A11y::Button)
        .named(name.clone())
        .tip(format!("{name}\nClick to show · right-click for more"))
        .id(name_id);
        let edge = if selected {
            Fill::from(accent())
        } else if look == Some(true) {
            Fill::from(accent())
        } else {
            Role::Ink.alpha(0.)
        };
        rows.push(
            row![
                block(2, Len::Pct(100.)).fill(edge),
                grip,
                interactive(name_el, false),
                port_el,
                channel_el,
                output_el,
                row![mute_el, solo_el].gap(HALF).w(SWITCHES).shrink(0),
                gain_el,
                pan_el,
                close_el
            ]
            .gap(GAP)
            .align(Align::Center)
            .pad(edges(0., HALF, 0., 0.))
            .h(BAR + 8.)
            .fill(if failed {
                Role::Danger.alpha(0.08)
            } else if selected {
                Role::Surface.alpha(1.)
            } else {
                Role::Background.alpha(1.)
            })
            .when(look == Some(false), |e| e.stroke(accent()).stroke_width(1))
            .id(row_id)
            .shrink(0),
        );
        rows.push(rule());
    }
    if let Some(slot) = remove {
        cx.remove(slot);
    }
    let heading = |text: &str| section(text);
    let head = row![
        block(2, 1),
        heading("#").w(INDEX).justify(Justify::Center),
        heading("Instrument").flex(2).min_w(80).pad((GAP, 0)),
        heading("Port").w(PORT),
        heading("Channel").w(CHANNEL),
        heading("Out").w(OUTPUT),
        block(SWITCHES, 1),
        heading("Volume").flex(1).min_w(0),
        heading("Pan").flex(1).min_w(0),
        block(CONTROL, 1)
    ]
    .gap(GAP)
    .align(Align::Center)
    .pad(edges(0., HALF, 0., 0.))
    .h(BAR)
    .shrink(0);
    if order.is_empty() {
        rows.push(
            body("The rack is empty. Double-click a preset in the browser, or drag one onto the rack.")
                .fill(Role::Dim)
                .lines(2)
                .pad(WIDE),
        );
    }
    let used: usize = cx.view.parts.iter().map(|v| v.bytes).sum();
    let multi = if cx.selection.multi.is_empty() {
        "Untitled multi".to_owned()
    } else {
        super::header::stem(&cx.selection.multi)
    };
    let footer = row![caption(format!(
        "{multi}  ·  {} of 16 slots  ·  {}  ·  4 MIDI ports  ·  8 stereo outputs",
        order.len(),
        megabytes(used)
    ))
    .fill(Role::Dim)
    .lines(1)
    .min_w(0)]
    .align(Align::Center)
    .pad((WIDE, 0))
    .h(BAR)
    .shrink(0);
    col![
        head,
        rule(),
        col(rows)
            .gap(0)
            .flex(1)
            .min_h(0)
            .scroll()
            .id("rack-scroll"),
        rule(),
        footer
    ]
    .gap(0)
    .flex(1)
    .min_h(0)
    .min_w(0)
}
