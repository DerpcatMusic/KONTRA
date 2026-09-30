//! The rack: every part stacked Kontakt-style, each under its own header.
//!
//! A header names the part, steps its preset, routes it and mixes it; its
//! chevron folds the part down to one slim line of it, its ✕ removes it.
//! Below an open header sit the part's notices and performance controls,
//! as much of them as the part's height shows: its foot drags to size it.
//! With sticky headers on, parts scrolled out of view keep their headers
//! stacked at the rack's edges. Presets dropped on a header replace that
//! part; parts dragged by their name reorder; anything dropped on the
//! rack's foot is added.

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

/// A slim header's height: one line of controls. Parts shrink no further,
/// and stuck headers stack at this pitch.
pub const SLIM: f64 = CONTROL + 6.;

/// Every part in rack order, then the foot that adds more.
///
/// The rack scrolls itself rather than as a scroll node, so it can scroll
/// to a part and knows where each part is: with sticky headers on, a part
/// scrolled above the view keeps its header stacked slim at the top, one
/// below it at the bottom, and a click on a stuck header brings its part
/// back. Positions are last frame's layout; a change in what the rack holds
/// asks for one more frame so they catch up.
pub fn view(ui: &mut Ui, cx: &mut Cx) -> El {
    let order: Vec<usize> = cx.selection.order.iter().map(|&n| n as usize).collect();
    if order.is_empty() {
        cx.state.rack_y = 0.;
        return col![instrument::welcome(cx), foot(ui, cx)]
            .gap(0)
            .align(Align::Stretch)
            .flex(1)
            .min_h(0);
    }
    wheel_taken();
    let mut items: Vec<El> = order.iter().map(|&slot| part(ui, cx, slot)).collect();
    items.push(foot(ui, cx));
    // A knob under the pointer turns; the rack scrolls otherwise.
    let taken = wheel_taken();
    let wheel = ui.wheel("rack-view").filter(|_| !taken);

    let (view_h, content_h, tops) = {
        let frame = |id: &str| ui.scene().and_then(|s| s.surface(id)).map(|s| s.frame);
        let content = frame("rack-content");
        let tops: Vec<Option<f64>> = (order.iter())
            .map(|slot| Some(frame(&format!("part-{slot}"))?.y - content?.y))
            .collect();
        let view_h = frame("rack-view").map_or(0., |f| f.size.height);
        (view_h, content.map_or(0., |f| f.size.height), tops)
    };
    let sticky = !cx.selection.sticky_off;
    // Where part `i` of the order sits scrolled to, under the headers stuck above it.
    let pitch = |i: usize| if sticky { i as f64 * SLIM } else { 0. };
    let max = (content_h - view_h).max(0.);
    let state = &mut *cx.state;
    if let Some(w) = wheel {
        state.rack_y += w.y;
    }
    if let Some(at) = (state.reveal)
        .and_then(|slot| order.iter().position(|s| *s == slot))
        .and_then(|i| Some(tops[i]? - pitch(i)))
    {
        state.rack_y = at;
        state.reveal = None;
    }
    let bar = ui.get("rack-bar");
    if bar.dragged && view_h > 0. {
        state.rack_y += bar.drag_delta.y * content_h / view_h;
    }
    if view_h > 0. {
        state.rack_y = state.rack_y.clamp(0., max);
    }
    // The bar under the hand follows it; everything else glides.
    let y = if bar.held { state.rack_y } else { ui.tween_with("rack-y", state.rack_y, quick()) };
    // What moves the layout: one more frame once it has, to read it back.
    let shape = {
        use std::hash::{DefaultHasher, Hash, Hasher};
        let mut h = DefaultHasher::new();
        for &slot in &order {
            let p = &cx.selection.parts[slot];
            (slot, p.collapsed, p.height.to_bits(), instrument::stage_deps(cx, slot)).hash(&mut h);
            cx.view.parts[slot].status.hash(&mut h);
        }
        cx.state.resizing.map(|(s, h)| (s, h.to_bits())).hash(&mut h);
        (h.finish() % 1_000_000) as f64
    };
    ui.tween_with("rack-shape", shape, Spring::instant());

    let content = col(items).gap(0).align(Align::Stretch).w(Len::Pct(100.)).shrink(0).id("rack-content");
    // A scroll node for its clip and squeeze, slid by the rack itself: the
    // wheel it claims never reaches the runtime's scrolling, and it shows
    // the rack's own bar instead of the runtime's.
    let viewport = col![content]
        .gap(0)
        .align(Align::Stretch)
        .w(Len::Pct(100.))
        .h(Len::Pct(100.))
        .scroll()
        .no_scrollbar()
        .scrolled(0., y.round())
        .id("rack-view");
    let mut layers = vec![viewport];
    if sticky && view_h > 0. {
        let n = order.len();
        for (i, &slot) in order.iter().enumerate() {
            let Some(top) = tops[i] else { continue };
            let at = top - y;
            let (above, below) = (pitch(i), view_h - (n - i) as f64 * SLIM);
            let stuck_at = if at < above {
                above
            } else if at > below {
                below
            } else {
                continue;
            };
            layers.push(stuck(ui, cx, slot, at < above).at(0., stuck_at));
        }
    }
    let mut row_items = vec![stack(layers).flex(1).min_w(0).min_h(0).h(Len::Pct(100.)).clip()];
    if content_h > view_h + 0.5 && view_h > 0. {
        row_items.push(scrollbar(ui, y, view_h, content_h));
    }
    row(row_items).gap(0).align(Align::Stretch).flex(1).min_h(0)
}

/// The rack's scrollbar: a thin thumb that warms under the pointer and drags.
fn scrollbar(ui: &mut Ui, y: f64, view_h: f64, content_h: f64) -> El {
    let r = ui.get("rack-bar");
    let lift = ui.state("rack-bar").hover.max(if r.held { 1. } else { 0. }) as f32;
    canvas(move |s| {
        let len = (s.height * view_h / content_h).max(CONTROL);
        let at = (s.height - len) * (y / (content_h - view_h)).clamp(0., 1.);
        let w = 3. + 2. * f64::from(lift);
        vec![Draw::fill(
            rect(s.width - w - 1., at + 2., w, len - 4.),
            Role::Ink.alpha(0.18 + 0.3 * lift),
        )]
    })
    .w(8)
    .h(Len::Pct(100.))
    .shrink(0)
    .a11y(A11y::Slider { value: y, min: 0., max: content_h - view_h })
    .named("Scroll the rack")
    .id("rack-bar")
}

/// One part: its header, what shows of its notices and controls at the
/// height it is given, and an edge along its foot that sizes it. Dragged
/// shorter, the controls clip, then only the header stays, then its slim
/// line; a double-click on the edge swaps the slim line and all of it.
fn part(ui: &mut Ui, cx: &mut Cx, slot: usize) -> El {
    let edge_id = format!("resize-{slot}");
    let (part_h, head_h, body_h) = {
        let height = |id: String| ui.scene().and_then(|s| s.surface(&id)).map(|s| s.frame.size.height);
        (height(format!("part-{slot}")), height(format!("header-{slot}")), height(format!("body-{slot}")))
    };
    if let Some(h) = body_h {
        cx.state.bodies.insert(slot, h);
    }
    let p = &cx.selection.parts[slot];
    let collapsed = p.collapsed;
    // The full header's height, as long as it was last shown full.
    let full_h = head_h.filter(|h| !collapsed && *h > SLIM).unwrap_or(SLIM + CONTROL);
    let natural = cx.state.bodies.get(&slot).map(|b| full_h + b);
    let set = (!collapsed && p.height > 0.).then_some(f64::from(p.height));
    let r = ui.get(edge_id.as_str());
    let state = &mut *cx.state;
    if r.dragged {
        let from = state
            .resizing
            .filter(|(s, _)| *s == slot)
            .map_or(if collapsed { SLIM } else { set.or(part_h).unwrap_or(full_h) }, |(_, h)| h);
        let to = (from + r.drag_delta.y).max(SLIM);
        state.resizing = Some((slot, natural.map_or(to, |n| to.min(n))));
    }
    let dragging = state.resizing.filter(|(s, _)| *s == slot).map(|(_, h)| h);
    if r.released
        && let Some(h) = dragging
    {
        state.resizing = None;
        let p = &mut cx.selection.parts[slot];
        p.collapsed = h < full_h - 0.5;
        let all = natural.is_some_and(|n| h >= n - 1.);
        p.height = if p.collapsed || all { 0. } else { h as f32 };
    }
    if r.double_clicked {
        let p = &mut cx.selection.parts[slot];
        if !p.collapsed && p.height == 0. {
            p.collapsed = true;
        } else {
            (p.collapsed, p.height) = (false, 0.);
        }
    }
    let p = &cx.selection.parts[slot];
    let height = cx.state.resizing.filter(|(s, _)| *s == slot).map(|(_, h)| h).or((!p.collapsed && p.height > 0.).then_some(f64::from(p.height)));
    let slim = p.collapsed || height.is_some_and(|h| h < full_h - 0.5);
    let head = header_at(ui, cx, slot, slim);
    let mut lines = vec![head];
    if slim {
        // Mid-drag below a full header: the slim line, then the room it will take.
        if let Some(h) = height.filter(|h| *h > SLIM) {
            lines.push(block(Len::Pct(100.), h - SLIM));
        }
    } else {
        let mut body: Vec<El> = instrument::notices(cx, slot).into_iter().collect();
        // Kept as drawn while nothing it shows moves: meters and keys
        // redraw around it, not through it.
        let deps = (instrument::stage_deps(cx, slot), cx.selection.appearance);
        let stage = ui.memo(format!("stage-memo-{slot}"), deps, |ui| instrument::stage(ui, cx, slot));
        body.push(behind(cx, slot, stage));
        let body = col(body).gap(0).align(Align::Stretch).shrink(0).id(format!("body-{slot}"));
        match height.map(|h| h - full_h) {
            // Too little room for any of it: the header alone.
            Some(room) if room < 1. => {}
            Some(room) if natural.is_none_or(|n| room < n - full_h - 0.5) => {
                // Clipped, it fades into the edge: there is more below.
                let fade = block(Len::Pct(100.), room.min(CONTROL))
                    .fill(Gradient::linear(180., [(0., Role::Background.alpha(0.)), (1., Role::Background.alpha(0.9))]))
                    .anchor(Align::Start, Align::End);
                let body = col![body].gap(0).align(Align::Stretch).w(Len::Pct(100.)).h(room);
                lines.push(stack![body, fade].w(Len::Pct(100.)).h(room).shrink(0).clip());
            }
            _ => lines.push(body),
        }
    }
    lines.push(rule());
    let lift = ui.state(edge_id.as_str()).hover.max(if r.held { 1. } else { 0. }) as f32;
    let edge = canvas(move |s| {
        if lift <= 0.01 {
            return Vec::new();
        }
        let t = 2.;
        vec![Draw::fill(rect(0., s.height - t, s.width, t), accent().with_alpha(0.35 + 0.5 * lift))]
    })
    .w(Len::Pct(100.))
    .h(6)
    .anchor(Align::Start, Align::End)
    .cursor(Cursor::ResizeV)
    .tip("Drag to size the part, double-click to fold or unfold it")
    .named("Resize part")
    .id(edge_id);
    stack![col(lines).gap(0).align(Align::Stretch).w(Len::Pct(100.)), edge]
        .w(Len::Pct(100.))
        .shrink(0)
        .id(format!("part-{slot}"))
}

/// A part's header stuck to the rack's top or bottom edge while its part is
/// scrolled away: its color, name and state on one slim line, solo and
/// mute still at hand. A click scrolls back to the part.
fn stuck(ui: &mut Ui, cx: &mut Cx, slot: usize, top: bool) -> El {
    let id = format!("stuck-{slot}");
    let r = ui.get(id.as_str());
    if r.clicked {
        cx.state.selected = slot;
        cx.state.reveal = Some(slot);
    }
    let library = cx.library_of(Path::new(&cx.selection.parts[slot].path));
    let tint = cx.tint(&library);
    let selected = cx.state.selected == slot;
    let loading = loading(cx, slot);
    let name = name(cx, slot);
    let facts = facts(cx, slot, true).to_uppercase();
    let part = &mut cx.selection.parts[slot];
    let (mut solo, mut mute) = (part.solo, part.mute);
    let switches = solo_mute(ui, &id, &mut solo, &mut mute);
    (part.solo, part.mute) = (solo, mute);
    let p = cx.p.clone();
    let level = move || crate::plugin::Meters::read(&p.shared.meters.parts[slot]);
    let p = cx.p.clone();
    let dot = activity_dot(move || crate::plugin::Meters::read(&p.shared.meters.parts[slot]) != [0.; 2]);
    let mut line = vec![
        block(3, Len::Pct(100.))
            .fill(match tint {
                Some(t) => Fill::from(t),
                None if selected => accent().into(),
                None => Role::Ink.alpha(0.12),
            })
            .shrink(0),
        glyph(if top { Icon::Up } else { Icon::Down }, TEXT - 2., Role::Ink.alpha(0.45)),
        body(name.clone())
            .text_size(TEXT)
            .text_weight(Weight::SEMIBOLD)
            .fill(if part.mute { Role::Dim } else { Role::Ink })
            .lines(1)
            .min_w(0)
            .shrink(1),
    ];
    line.extend(loading.map(|done| load_chip(done, false)));
    line.extend([
        caption(facts)
            .text_size(SMALL - 2.)
            .text_weight(Weight::SEMIBOLD)
            .fill(Role::Dim)
            .lines(1)
            .min_w(0)
            .flex(1),
        switches,
        dot,
        col![meter_v(level)].pad((3, 0)).h(Len::Pct(100.)).shrink(0),
    ]);
    let el = row(line)
        .gap(SPACE)
        .align(Align::Center)
        .pad(edges(0., 0., 0., 0.))
        .h(SLIM)
        .w(Len::Pct(100.))
        .fill(if selected { Role::Raised } else { Role::Surface })
        .stroke(hairline())
        .stroke_width(1)
        .cursor(Cursor::Hand)
        .a11y(A11y::Button)
        .named(format!("{name}: scroll to it"))
        .tip(format!("{name}\nClick to scroll to it"))
        .id(id);
    interactive(el, selected)
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
/// Folded, it is one slim line with every control still on it.
pub fn header(ui: &mut Ui, cx: &mut Cx, slot: usize) -> El {
    let slim = cx.selection.parts[slot].collapsed;
    header_at(ui, cx, slot, slim)
}

/// How far `slot`'s samples have loaded, 0..1, while they load.
pub(super) fn loading(cx: &Cx, slot: usize) -> Option<f64> {
    cx.view.parts[slot].loading.then(|| {
        let done = cx.p.shared.load_progress[slot].load(Ordering::Relaxed);
        (f64::from(done) / f64::from(crate::engine::LOAD_DONE)).clamp(0., 1.)
    })
}

/// "Loading 42%" in the accent, or only "42%" where room is short; before
/// the first sample is read, "Loading…".
pub(super) fn load_chip(done: f64, words: bool) -> El {
    let text = match (done > 0., words) {
        (true, true) => format!("Loading {:.0}%", done * 100.),
        (true, false) => format!("{:.0}%", done * 100.),
        (false, _) => "Loading…".to_owned(),
    };
    caption(text)
        .text_size(SMALL - 1.)
        .text_weight(Weight::SEMIBOLD)
        .fill(accent())
        .lines(1)
        .shrink(0)
        .named("Loading")
}

/// [`header`], full or slim.
fn header_at(ui: &mut Ui, cx: &mut Cx, slot: usize, slim: bool) -> El {
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
    let progress = loading(cx, slot);
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

    let facts_el = |fill: bool| {
        caption(facts.to_uppercase())
            .text_size(SMALL - 2.)
            .text_weight(Weight::SEMIBOLD)
            .fill(Role::Dim)
            .lines(1)
            .min_w(0)
            .tip(full.clone())
            .when(fill, |e| e.flex(1))
    };
    let chip = progress.map(|done| load_chip(done, !(slim && narrow)));
    let mut name_row = vec![title, prev_el, next_el];
    name_row.extend(chip);
    let name_row = row(name_row).gap(0).align(Align::Center).min_w(0);
    let mix = cluster(vec![midi_el, output_el, pan_el, gain_el, tune_el]).gap(SPACE);
    let tail = cluster(vec![switches, dot, more_el, remove_el]).gap(TIGHT + 1.);
    let rows = match (slim, narrow) {
        // Slim: one line, every control still on it; what it is beside
        // its name where there is room for it.
        (true, false) => vec![
            row![name_row.shrink(1), facts_el(true)]
                .gap(SPACE)
                .align(Align::Center)
                .flex(1)
                .min_w(0),
            mix,
            tail,
        ],
        (true, true) => vec![name_row.flex(1).shrink(1), mix, tail],
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
        .gap(if slim && narrow { TIGHT } else { SPACE })
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
            Draw::fill(rect(0., 0., s.width, s.height), Role::Ink.alpha(0.1)),
            Draw::fill(rect(0., 0., (s.width * done).max(SPACE), s.height), accent()),
        ],
        None => Vec::new(),
    })
    .w(Len::Pct(100.))
    .h(if progress.is_some() { 2 } else { 0 })
    .shrink(0)
    .named("Load progress");
    let meter = col![meter_v(level)].pad((3, 0)).h(Len::Pct(100.)).shrink(0);
    let name = name(cx, slot);
    let line = row![edge, body, meter].gap(0).align(Align::Stretch).when(slim, |e| e.h(SLIM - 1.));
    let line = match banner {
        // The artwork fades in behind the controls; the title sits on a
        // darker stretch of it so a logo in the artwork never crowds it.
        Some(image) => stack![
            block(BANNER.0, Len::Pct(100.))
                .fill(Fill::Image(image, moose::mui::mui::scene::Fit::Cover))
                .anchor(Align::Start, Align::Start),
            block(BANNER.0 * 0.75, Len::Pct(100.))
                .fill(Gradient::linear(
                    90.,
                    [(0., Role::Surface.alpha(0.7)), (0.5, Role::Surface.alpha(0.45)), (1., Role::Surface.alpha(0.))],
                ))
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

/// What the part is: its library and size, or that it failed to load.
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
    if !v.loading && v.bytes > 0 {
        facts.push(megabytes(v.bytes));
        if v.purged_percent > 0 {
            facts.push(format!("{}% purged", v.purged_percent));
        }
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
