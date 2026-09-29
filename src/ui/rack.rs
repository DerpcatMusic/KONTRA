//! The rack: a strip of part chips to switch between, and the mixer view
//! with routing, level, pan, mute/solo and ordering.

use super::{Cx, RackDrag, Tab, add_part, move_part, theme::*};
use crate::plugin::Part;
use moose::mui::mui::prelude::*;
use std::path::Path;

/// The display name of a part: the loaded instrument's, else the file's.
fn name(cx: &Cx, slot: usize) -> String {
    let part = &cx.selection.parts[slot];
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

/// One chip per part, in rack order, then a drop target that adds.
pub fn strip(ui: &mut Ui, cx: &mut Cx) -> El {
    let mut chips = Vec::new();
    for (position, slot) in cx
        .selection
        .order
        .clone()
        .into_iter()
        .map(|n| n as usize)
        .enumerate()
    {
        let id = format!("part-{slot}");
        let name = name(cx, slot);
        let current = cx.state.selected == slot;
        let (choose, el) = action(
            ui,
            id.as_str(),
            &format!("{:02}  {name}", position + 1),
            current,
        );
        if choose {
            cx.state.selected = slot;
            cx.state.library = Some(cx.library_of(Path::new(&cx.selection.parts[slot].path)));
            if cx.state.tab == Tab::Rack {
                cx.state.tab = Tab::Perform;
            }
        }
        if ui.get(id.as_str()).dragged {
            ui.start_drag(id.as_str(), RackDrag::Part(slot));
        }
        drop_target(ui, cx, &id, slot);
        let part = &cx.selection.parts[slot];
        let flags = match (part.mute, part.solo) {
            (true, _) => "  · muted",
            (_, true) => "  · solo",
            _ => "",
        };
        chips.push(
            el.lines(1)
                .max_size(Size::new(240., 40.))
                .when(part.mute, |e| e.opacity(0.55))
                .tip(format!(
                    "{name}{flags}\nDrag to reorder, drop a preset to replace"
                )),
        );
    }
    let dragging = ui.dragging::<RackDrag>().is_some();
    if let Some(RackDrag::Instrument(path)) = ui.dropped_on::<RackDrag>("rack-drop") {
        cx.state.notice.clear();
        if crate::import::is_multi(Path::new(&path)) {
            cx.p.shared.queue_multi(path);
        } else {
            cx.add(path);
        }
    }
    chips.push(
        caption(if dragging {
            "Drop here to add"
        } else {
            "+  Drag a preset here"
        })
        .fill(Role::Dim)
        .pad((GAP + HALF, HALF + 2.))
        .stroke(Role::Ink.alpha(if dragging { 0.4 } else { 0.12 }))
        .stroke_width(1)
        .radius(6)
        .focusable()
        .a11y(A11y::Button)
        .named("Add to rack drop target")
        .id("rack-drop")
        .on(State::Hover, |s| s.fill(Role::Raised))
        .shrink(0),
    );
    row![
        section("Rack").pad((0, HALF + 2.)),
        row(chips)
            .gap(HALF)
            .wrap()
            .align(Align::Center)
            .flex(1)
            .min_w(0)
    ]
    .gap(GAP + HALF)
    .align(Align::Start)
    .pad((WIDE, GAP))
    .shrink(0)
}

/// Every part with its routing and mix, one row each.
pub fn mixer(ui: &mut Ui, cx: &mut Cx) -> El {
    let mut rows = Vec::new();
    let (mut remove, mut duplicate, mut reorder) = (None, None, None);
    let order = cx.selection.order.clone();
    for (position, &slot) in order.iter().enumerate() {
        let slot = slot as usize;
        let name = name(cx, slot);
        let (choose, name_el) = action(
            ui,
            format!("rack-name-{slot}"),
            &name,
            cx.state.selected == slot,
        );
        if choose {
            cx.show(slot);
        }
        let part = &mut cx.selection.parts[slot];
        let mut port = f64::from(part.port) + 1.;
        let port_el = number(
            ui,
            format!("port-{slot}"),
            "MIDI",
            &mut port,
            1.0..=4.0,
            format!("{}", part.port + 1),
        );
        part.port = port.round() as u8 - 1;
        let mut channel = f64::from(part.channel + 1);
        let channel_text = if part.channel < 0 {
            "Omni".into()
        } else {
            format!("{}", part.channel + 1)
        };
        let channel_el = number(
            ui,
            format!("channel-{slot}"),
            "Ch",
            &mut channel,
            0.0..=16.0,
            channel_text,
        );
        part.channel = channel.round() as i16 - 1;
        let mut output = f64::from(part.output) + 1.;
        let output_el = number(
            ui,
            format!("output-{slot}"),
            "Out",
            &mut output,
            1.0..=8.0,
            format!("{}", part.output + 1),
        );
        part.output = output.round() as u8 - 1;
        let mut gain = f64::from(part.gain);
        let gain_el = number(
            ui,
            format!("gain-{slot}"),
            "Level",
            &mut gain,
            -60.0..=6.0,
            format!("{:.1} dB", part.gain),
        );
        part.gain = gain as f32;
        let mut pan = f64::from(part.pan);
        let pan_el = number(
            ui,
            format!("pan-{slot}"),
            "Pan",
            &mut pan,
            -1.0..=1.0,
            pan_text(part.pan),
        );
        part.pan = pan as f32;
        let (mute, mute_el) = action(ui, format!("mute-{slot}"), "M", part.mute);
        if mute {
            part.mute = !part.mute;
        }
        let (solo, solo_el) = action(ui, format!("solo-{slot}"), "S", part.solo);
        if solo {
            part.solo = !part.solo;
        }
        let (dup, dup_el) = action(ui, format!("duplicate-{slot}"), "Duplicate", false);
        if dup {
            duplicate = Some(part.clone());
        }
        let (up, up_el) = action(ui, format!("up-{slot}"), "Up", false);
        if up && position > 0 {
            reorder = Some((slot, order[position - 1] as usize));
        }
        let (down, down_el) = action(ui, format!("down-{slot}"), "Down", false);
        if down && position + 1 < order.len() {
            reorder = Some((order[position + 1] as usize, slot));
        }
        let (close, close_el) = action(ui, format!("remove-{slot}"), "Remove", false);
        if close {
            remove = Some(slot);
        }
        let failed = cx.view.parts[slot].status.starts_with("Load failed");
        let top = row![
            caption(format!("{:02}", position + 1))
                .fill(Role::Dim)
                .w(20),
            name_el.lines(1).min_w(0).named(name.clone()),
            spacer(),
            mute_el.named(format!("Mute {name}")),
            solo_el.named(format!("Solo {name}")),
            row![up_el, down_el, dup_el, close_el].gap(0)
        ]
        .gap(HALF)
        .align(Align::Center);
        let bottom = row![port_el, channel_el, output_el, spacer(), gain_el, pan_el]
            .gap(WIDE)
            .align(Align::Center)
            .pad(edges(0., 0., 0., 20. + HALF));
        rows.push(
            col![top, bottom]
                .gap(HALF)
                .pad((GAP + HALF, GAP))
                .fill(if failed {
                    Role::Danger.alpha(0.08)
                } else {
                    Role::Surface.alpha(1.)
                })
                .radius(8)
                .shrink(0),
        );
    }
    if let Some((from, to)) = reorder {
        move_part(&mut cx.selection, from, to);
    }
    if let Some(part) = duplicate {
        match add_part(&mut cx.selection, part) {
            Some(slot) => cx.state.selected = slot,
            None => cx.state.notice = "The rack is full (16 instruments).".into(),
        }
    }
    if let Some(slot) = remove {
        cx.selection.parts[slot] = Part::default();
        cx.state.selected = cx
            .selection
            .parts
            .iter()
            .position(|p| !p.path.is_empty())
            .unwrap_or(0);
    }
    if rows.is_empty() {
        rows.push(
            body("The rack is empty. Pick an instrument in the library to add it.")
                .fill(Role::Dim)
                .pad(GAP),
        );
    }
    let used: usize = cx.view.parts.iter().map(|v| v.bytes).sum();
    let multi = if cx.selection.multi.is_empty() {
        "Untitled multi".to_owned()
    } else {
        super::header::stem(&cx.selection.multi)
    };
    let loaded = cx
        .selection
        .parts
        .iter()
        .filter(|p| !p.path.is_empty())
        .count();
    rows.push(
        caption(format!(
            "{multi} · {loaded} of 16 slots · {} · 4 MIDI inputs · 8 stereo outputs",
            megabytes(used)
        ))
        .fill(Role::Dim)
        .lines(2)
        .pad(GAP),
    );
    col(rows)
        .gap(HALF)
        .pad(WIDE)
        .flex(1)
        .min_h(0)
        .scroll()
        .id("rack-scroll")
}

pub fn pan_text(pan: f32) -> String {
    if pan.abs() < 0.01 {
        "C".into()
    } else {
        format!(
            "{:.0}{}",
            pan.abs() * 100.,
            if pan < 0. { "L" } else { "R" }
        )
    }
}
