//! The rack: every part stacked Kontakt-style, each under its own header.
//!
//! A header, one compact line, names the part, steps its preset, routes it
//! and mixes it; its chevron folds the part down to the header alone (the
//! height springs), its ✕ removes it.
//! Below an open header sit the part's notices and performance controls,
//! as much of them as the part's height shows: its foot drags to size it.
//! With sticky headers on, parts scrolled out of view keep their headers
//! stacked at the rack's edges. Presets dropped on a header replace that
//! part; parts dragged by their name reorder; anything dropped on the
//! rack's foot or the empty canvas beyond it is added.

use super::{Cx, RackDrag, menu, move_part, part as instrument, theme::*};
use crate::library as import;
use moose::mui::mui::prelude::*;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;

/// The display name of a part: the player's, else the loaded instrument's
/// or the file's without its library's name in front.
pub fn name(cx: &Cx, slot: usize) -> String {
    let Some(part) = cx.selection.parts.get(slot) else {
        return String::new();
    };
    if !part.name.is_empty() {
        return part.name.clone();
    }
    let full = cx.instrument_name(slot).unwrap_or_else(|| super::header::stem(&part.path));
    without_library(&full, &cx.library_of(Path::new(&part.path))).to_owned()
}

/// A header's height with the rule under it: one line of controls. Parts
/// shrink no further, and stuck headers stack at this pitch.
pub const SLIM: f64 = CONTROL + 6.;


/// Parts built in a frame before the rack knows where they are.
const UNPLACED: usize = 4;

/// Every part in rack order, then the foot and empty canvas that add more.
///
/// The rack scrolls itself rather than as a scroll node, so it can scroll
/// to a part and knows where each part is: with sticky headers on, a part
/// scrolled above the view keeps its header stacked at the top, one below
/// it at the bottom, and a click on a stuck header brings its part back.
/// Positions are last frame's layout; a change in what the rack holds asks
/// for one more frame so they catch up. Parts well out of view keep their
/// room but are not built.
pub fn view(ui: &mut Ui, cx: &mut Cx) -> El {
    let order: Vec<usize> = cx.selection.order.iter().map(|&n| n as usize).collect();
    // A click on the rack itself, off every part, lets go of the selection.
    if ui.get("rack-view").clicked {
        cx.state.selected_none();
    }
    let (view_h, content_h) = {
        let frame = |id: &str| ui.scene().and_then(|s| s.surface(id)).map(|s| s.frame);
        (frame("rack-view").map_or(0., |f| f.size.height), frame("rack-content").map_or(0., |f| f.size.height))
    };
    // Offscreen rows have no widget subtree or surface. Reconstruct their
    // positions from the same known body/fixed heights used by part(), rather
    // than asking the layout to keep every header alive just to locate it.
    let mut top = 0.;
    let mut measured = Vec::with_capacity(order.len());
    let frames: Vec<Option<(f64, f64)>> = order.iter().map(|&slot| {
        let frame = |id: String| ui.scene().and_then(|s| s.surface(&id)).map(|s| s.frame.size.height);
        if let Some(body) = frame(format!("body-{slot}")) { cx.state.bodies.insert(slot, body); }
        let part = &cx.selection.parts[slot];
        let natural = cx.state.bodies.get(&slot).map(|b| SLIM + b);
        let set = (!part.collapsed && part.height > 0.).then_some(f64::from(part.height));
        let target = target_height(part.collapsed, set, natural);
        let prior = frame(format!("part-{slot}"));
        measured.push(part.collapsed || natural.is_some() || prior.is_some());
        let height = cx.state.resizing.filter(|(s, _)| *s == slot).map(|(_, h)| h)
            .or_else(|| target.map(|h| ui.tween_with(format!("part-h-{slot}"), h, quick()).round()))
            .or(prior).unwrap_or(SLIM).max(SLIM);
        let result = Some((top, height));
        top += height;
        result
    }).collect();
    let sticky = !cx.selection.sticky_off;
    let n = order.len();
    // Where part `i`'s header shows with the rack scrolled to `y`: in place,
    // or stacked at the edge it went past.
    let place = |i: usize, top: f64, y: f64| {
        let at = top - y;
        if !sticky {
            return at;
        }
        // Keep the stacked rack headers while they fit. A larger rack uses
        // its current body's sticky header, pushed away by that body's end;
        // reserving n header pitches would otherwise cover the whole view.
        if n as f64 * SLIM > view_h {
            return at.max(0.).min(at + frames[i].map_or(SLIM, |f| f.1) - SLIM);
        }
        let (above, below) = (i as f64 * SLIM, view_h - (n - i) as f64 * SLIM);
        if at < above { above } else { at.min(below).max(above) }
    };
    // Which headers are stuck, and which parts are near enough the view to
    // build, from where the rack was last drawn.
    let drawn = cx.state.rack_drawn;
    let (mut stuck, mut near) = (vec![false; n], vec![true; n]);
    // Parts not laid out yet (a fresh window, parts just added) are built a
    // few a frame: sixteen big panels at once exceed the layout's node
    // budget, and a refused layout would never learn where anything is.
    let mut unplaced = 0;
    for (i, f) in frames.iter().enumerate() {
        match *f {
            Some((top, h)) if view_h > 0. => {
                stuck[i] = sticky && (place(i, top, drawn) - (top - drawn)).abs() > 0.5;
                near[i] = top - drawn + h > -SLIM && top - drawn < view_h + SLIM;
            }
            _ => {
                near[i] = unplaced < UNPLACED;
                unplaced += 1;
            }
        }
    }
    // Keep active pointer/text edits mounted, and measure a requested new
    // row before deciding where to reveal it. Other offscreen rows stay unbuilt.
    for (i, &slot) in order.iter().enumerate() {
        near[i] |= cx.state.resizing.is_some_and(|(s, _)| s == slot)
            || cx.state.renaming.as_ref().is_some_and(|(s, _)| *s == slot)
            || (cx.state.reveal == Some(slot) && !measured[i]);
    }
    wheel_taken();
    let mut items = Vec::with_capacity(n + 3);
    let mut shape = {
        use std::hash::{DefaultHasher, Hash};
        let mut h = DefaultHasher::new();
        (&stuck, &near, cx.state.resizing.map(|(s, h)| (s, h.to_bits()))).hash(&mut h);
        h
    };
    let mut omitted = 0.;
    for (i, &slot) in order.iter().enumerate() {
        if !near[i] {
            omitted += frames[i].map_or(SLIM, |f| f.1);
            continue;
        }
        if omitted > 0. {
            items.push(block(Len::Pct(100.), omitted).shrink(0));
            omitted = 0.;
        }
        items.push(part(ui, cx, slot, stuck[i], true, &mut shape));
    }
    if omitted > 0. { items.push(block(Len::Pct(100.), omitted).shrink(0)); }
    if order.is_empty() {
        add_drop(ui, cx, "rack-welcome");
        // The welcome itself uses flex-basis zero to fill its parent. Give
        // that parent a measured height so it remains a real drop surface.
        items.push(col![instrument::welcome(cx)]
            .h((view_h - CONTROL - 2. * SPACE).max(240.))
            .shrink(0)
            .id("rack-welcome"));
    }
    items.push(foot(ui, cx));
    // Keep a viewport of quiet canvas after Add. It belongs to this rack's
    // existing scroll content, so the footer can scroll completely above it.
    items.push(empty(ui, cx, view_h.max(240.)));
    let headers: Vec<(usize, El)> = (0..n)
        .filter(|&i| stuck[i] && frames[i].is_some_and(|(top, _)| {
            let at = place(i, top, drawn);
            at + SLIM > 0. && at < view_h
        }))
        .map(|i| (i, col![header_at(ui, cx, order[i], true), rule()].gap(0).w(Len::Pct(100.))))
        .collect();
    // A knob under the pointer turns; the rack scrolls otherwise.
    let mut taken = wheel_taken();
    let mut offsets = std::collections::HashMap::new();
    if let Some(scene) = ui.scene() {
        for surface in scene.surfaces() {
            if surface.content.height <= surface.frame.size.height && surface.content.width <= surface.frame.size.width { continue; }
            let mut parent = surface.parent.clone();
            let mut in_rack = false;
            while let Some(id) = parent {
                if id.as_str() == "rack-view" { in_rack = true; break; }
                parent = scene.surface(id.as_str()).and_then(|s| s.parent.clone());
            }
            if !in_rack { continue; }
            let key = surface.key.as_str();
            let offset = ui.scroll(key);
            if ui.get(key).wheel != Vec2::ZERO && cx.state.rack_scrolls.get(key).is_some_and(|previous| *previous != offset) {
                taken = true;
            }
            offsets.insert(key.to_owned(), offset);
        }
    }
    cx.state.rack_scrolls = offsets;
    let wheel = ui.wheel("rack-view").filter(|_| !taken);

    let max = (content_h - view_h).max(0.);
    let state = &mut *cx.state;
    if let Some(w) = wheel {
        state.rack_y += w.y;
    }
    let pitch = |i: usize| if sticky && n as f64 * SLIM <= view_h { i as f64 * SLIM } else { 0. };
    if let Some(at) = (state.reveal)
        .and_then(|slot| order.iter().position(|s| *s == slot))
        .and_then(|i| measured[i].then(|| frames[i].unwrap().0 - pitch(i)))
    {
        state.rack_y = at;
        state.reveal = None;
    }
    bar_drag(ui, "rack-bar", &mut state.rack_y, view_h, content_h);
    if view_h > 0. {
        state.rack_y = state.rack_y.clamp(0., max);
    }
    // The bar under the hand follows it; everything else glides.
    let y = glide(ui, "rack-y", state.rack_y, ui.get("rack-bar").held);
    cx.state.rack_drawn = y;
    // What moves the layout: one more frame once it has, to read it back.
    let shape = (std::hash::Hasher::finish(&shape) % 1_000_000) as f64;
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
        .scrolled(0., y)
        .id("rack-view");
    let mut layers = vec![viewport];
    for (i, header) in headers {
        let top = frames[i].map_or(0., |f| f.0);
        layers.push(header.at(0., place(i, top, y).round()));
    }
    let mut row_items = vec![stack(layers).flex(1).min_w(0).min_h(0).h(Len::Pct(100.)).clip()];
    if content_h > view_h + 0.5 && view_h > 0. {
        row_items.push(scrollbar(ui, "rack-bar", "Scroll the rack", y, view_h, content_h));
    } else {
        // Reserve the bar's width before the first measured layout. The
        // scroll tail always overflows; appearing later must not move controls.
        row_items.push(block(8, Len::Pct(100.)).shrink(0));
    }
    row(row_items).gap(0).align(Align::Stretch).flex(1).min_h(0)
}

fn target_height(collapsed: bool, set: Option<f64>, natural: Option<f64>) -> Option<f64> {
    match (collapsed, set, natural) {
        (true, ..) => Some(SLIM),
        (false, Some(h), n) => Some(n.map_or(h, |n| h.min(n))),
        (false, None, n) => n,
    }
}

/// One part: its header, as much of its notices and controls as its height
/// shows, and an edge along its foot that sizes it. The height springs to
/// where it is headed (folded to the header, dragged shorter, or all of
/// it) and follows the edge exactly while it is dragged; a double-click on
/// the edge folds or unfolds it. A header `stuck` at the rack's edge leaves
/// its room here; a part not `near` the view keeps its room unbuilt.
fn part(ui: &mut Ui, cx: &mut Cx, slot: usize, stuck: bool, near: bool, shape: &mut impl std::hash::Hasher) -> El {
    use std::hash::Hash;
    let edge_id = format!("resize-{slot}");
    let (part_h, body_h) = {
        let height = |id: String| ui.scene().and_then(|s| s.surface(&id)).map(|s| s.frame.size.height);
        (height(format!("part-{slot}")), height(format!("body-{slot}")))
    };
    if let Some(h) = body_h {
        cx.state.bodies.insert(slot, h);
    }
    // All of it: the header, then its notices and controls.
    let natural = cx.state.bodies.get(&slot).map(|b| SLIM + b);
    // A click anywhere on the part that no control takes selects it.
    if [format!("part-{slot}"), format!("body-{slot}"), format!("stage-{slot}"), format!("inside-{slot}")].into_iter().any(|id| ui.get(id).clicked) {
        cx.state.select(slot);
    }
    let r = ui.get(edge_id.as_str());
    let state = &mut *cx.state;
    if r.dragged {
        let from = state.resizing.filter(|(s, _)| *s == slot).map(|(_, h)| h).or(part_h).unwrap_or(SLIM);
        let to = (from + r.drag_delta.y).max(SLIM);
        state.resizing = Some((slot, natural.map_or(to, |n| to.min(n))));
    }
    let dragging = state.resizing.filter(|(s, _)| *s == slot).map(|(_, h)| h);
    let p = &mut cx.selection.parts[slot];
    if r.released
        && let Some(h) = dragging
    {
        state.resizing = None;
        // Dragged up to the header, it folds; down to all of it, it follows its content.
        p.collapsed = h < SLIM + TIGHT;
        let all = natural.is_some_and(|n| h >= n - 1.);
        p.height = if p.collapsed || all { 0. } else { h as f32 };
    }
    if r.double_clicked {
        if !p.collapsed && p.height == 0. {
            p.collapsed = true;
        } else {
            (p.collapsed, p.height) = (false, 0.);
        }
    }
    let dragging = state.resizing.filter(|(s, _)| *s == slot).map(|(_, h)| h);
    let set = (!p.collapsed && p.height > 0.).then_some(f64::from(p.height));
    let target = target_height(p.collapsed, set, natural);
    // Not yet laid out whole: all of it, unsprung, so the next frame knows how much that is.
    let height = dragging.or(target).map(|to| {
        // On whole points: the rack below stays sharp while it springs.
        let sprung = ui.tween_with(format!("part-h-{slot}"), to, quick()).round();
        if dragging.is_some() { to.round() } else { sprung }
    });
    (p.collapsed, set.map(f64::to_bits), natural.map(f64::to_bits)).hash(shape);

    let mut lines = vec![if stuck {
        block(Len::Pct(100.), SLIM - 1.).shrink(0)
    } else {
        header_at(ui, cx, slot, false)
    }];
    let room = height.map(|h| h - SLIM);
    if room.is_none_or(|r| r >= 0.5) {
        let body = if near {
            let mut body: Vec<El> = instrument::notices(cx, slot).into_iter().collect();
            let stage = instrument::stage(ui, cx, slot);
            body.push(behind(cx, slot, stage));
            col(body).gap(0).align(Align::Stretch).shrink(0).id(format!("body-{slot}"))
        } else {
            block(Len::Pct(100.), room.unwrap_or(0.)).shrink(0)
        };
        lines.push(match room {
            // What the height shows of it: the rest is clipped.
            Some(room) => col![body].gap(0).align(Align::Stretch).w(Len::Pct(100.)).h(room).shrink(0).clip(),
            None => body,
        });
    }
    lines.push(rule());
    let lift = edge_lift(ui, &edge_id);
    // Over the rule at the part's foot, the accent line.
    let edge = canvas(move |s| edge_mark(s, s.height - 1., false, lift))
    .w(Len::Pct(100.))
    .h(EDGE_GRAB + 2.)
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

/// Footer and empty canvas share the browser's append operation. Explicit
/// header targets still own replacement and part reordering.
fn add_drop(ui: &mut Ui, cx: &mut Cx, id: &str) {
    if let Some(RackDrag::Instrument(path)) = ui.dropped_on::<RackDrag>(id) {
        cx.state.notice.clear();
        if import::is_multi(Path::new(&path)) {
            cx.p.shared.queue_multi(path);
        } else {
            cx.add(path);
        }
    }
}

fn empty(ui: &mut Ui, cx: &mut Cx, height: f64) -> El {
    add_drop(ui, cx, "rack-empty");
    if ui.get("rack-empty").clicked {
        cx.state.selected_none();
    }
    let over = ui.get("rack-empty").drop_target
        && matches!(ui.dragging::<RackDrag>(), Some(RackDrag::Instrument(_)));
    block(Len::Pct(100.), height)
        .when(over, |el| el.stroke(accent()).stroke_width(1))
        .named("Drop an instrument to append to the rack")
        .shrink(0)
        .id("rack-empty")
}

/// Where presets are dropped to be added, and a button that finds one.
fn foot(ui: &mut Ui, cx: &mut Cx) -> El {
    let dragging = ui.dragging::<RackDrag>().is_some();
    let over = ui.get("rack-drop").drop_target;
    add_drop(ui, cx, "rack-drop");
    if ui.get("rack-drop").activated() {
        cx.state.browser = true;
        ui.focus("search");
    }
    let loaded = cx.selection.order.len();
    let text = if dragging {
        "Drop to add to the rack".to_owned()
    } else {
        format!("Add an instrument · {loaded} loaded")
    };
    let el = row![
        glyph(Icon::Plus, TEXT, if dragging { Fill::from(Role::Ink) } else { secondary() }),
        caption(text).fill(secondary()).lines(1)
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

/// `slot`'s previous and next presets in its library folder, looked up once
/// per preset and scan: stepping walks every file the browser knows.
fn neighbors(cx: &mut Cx, slot: usize) -> [Option<String>; 2] {
    let path = cx.selection.parts[slot].path.clone();
    let (files, root, known) = &mut cx.state.neighbors;
    let shelf = Arc::as_ptr(&cx.view.shelf) as usize;
    if !files.upgrade().is_some_and(|f| Arc::ptr_eq(&f, &cx.view.files)) || *root != shelf {
        (*files, *root) = (Arc::downgrade(&cx.view.files), shelf);
        known.clear();
    }
    if let Some(found) = known.get(&path) {
        return found.clone();
    }
    let found = [step_preset(cx, slot, -1), step_preset(cx, slot, 1)];
    cx.state.neighbors.2.insert(path, found.clone());
    found
}

/// Below this width routing uses the existing icon buttons; the part menu
/// keeps the duplicate Remove action so the name has meaningful room.
const ONE_LINE: f64 = 860.;
const NAME_MIN: f64 = 120.;

/// The header banner: the library's artwork fading out over this width, and
/// taller than any header so covering one only ever crops it top and bottom.
pub const BANNER: (f64, f64) = (TEXT * 36., TEXT * 7.);

/// How far `slot`'s samples have loaded, 0..1, while they load.
pub(super) fn loading(cx: &Cx, slot: usize) -> Option<f64> {
    cx.view.parts[slot].loading.then(|| {
        let done = cx.p.shared.part(slot).map_or(0, |part| part.load_progress.load(Ordering::Relaxed));
        (f64::from(done) / f64::from(crate::sound::Progress::DONE.0)).clamp(0., 1.)
    })
}

/// "Loading 42%", or only "42%" where room is short; before the first
/// sample is read, "Loading…".
pub(super) fn load_chip(done: f64, words: bool) -> El {
    let text = match (done > 0., words) {
        (true, true) => format!("Loading {:.0}%", done * 100.),
        (true, false) => format!("{:.0}%", done * 100.),
        (false, _) => "Loading…".to_owned(),
    };
    caption(text)
        .text_size(SMALL - 1.)
        .fill(secondary())
        .lines(1)
        .shrink(0)
        .named("Loading")
}

/// [`header`]; a `stuck` one, clicked, also scrolls back to its part.
fn header_at(ui: &mut Ui, cx: &mut Cx, slot: usize, stuck: bool) -> El {
    let id = format!("header-{slot}");
    // Last frame's width: the header spans the rack, so its own layout never
    // changes it and the choice can't oscillate.
    let narrow = ui
        .scene()
        .and_then(|s| s.surface(&id))
        .is_some_and(|s| s.frame.size.width < ONE_LINE);
    let r = ui.get(id.as_str());
    if r.clicked {
        cx.state.select(slot);
        if stuck {
            cx.state.reveal = Some(slot);
        }
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
    let [before, after] = neighbors(cx, slot);
    let arrow = |ui: &mut Ui, id: String, icon: Icon, name: &str, there: bool| {
        if there { icon_button(ui, id, icon, name, false) } else { (false, dead_icon(icon, name)) }
    };
    let (previous, prev_el) = arrow(ui, format!("preset-prev-{slot}"), Icon::Left, "Previous preset", before.is_some());
    let (next, next_el) = arrow(ui, format!("preset-next-{slot}"), Icon::Right, "Next preset", after.is_some());
    if let Some(path) = before.filter(|_| previous).or(after.filter(|_| next)) {
        cx.replace(slot, path);
    }
    let more_id = format!("more-{slot}");
    let (more, more_el) = icon_button(ui, more_id.as_str(), Icon::More, "Part menu", false);
    if more {
        menu::open_under(ui, cx, menu::Target::Part(slot), &more_id);
    }
    let (remove, remove_el) = if narrow { (false, None) } else {
        let (hit, el) = icon_button(ui, format!("remove-{slot}"), Icon::Close, "Remove from rack", false);
        (hit, Some(el))
    };

    let midi_id = format!("midi-{slot}");
    let output_id = format!("output-{slot}");
    let part = &cx.selection.parts[slot];
    let channel = match (part.mpe, part.channel < 0) {
        (true, _) => "MPE".to_owned(),
        (false, true) => "Omni".to_owned(),
        (false, false) => (part.channel + 1).to_string(),
    };
    // Port A is the usual one and goes unsaid.
    let midi_text = if part.port == 0 {
        channel
    } else {
        format!("{}{channel}", char::from(b'A' + part.port.min(3)))
    };
    let output_text = cx.selection.bus(part.output.into()).label(part.output.into());
    let (midi, midi_el) = if narrow {
        icon_button(ui, midi_id.as_str(), Icon::MidiIn, &format!("MIDI input: {midi_text}"), false)
    } else { route(ui, midi_id.as_str(), Icon::MidiIn, &midi_text, "D16", "MIDI input") };
    if midi {
        menu::open_under(ui, cx, menu::Target::Midi(slot), &midi_id);
    }
    let (output, output_el) = if narrow {
        icon_button(ui, output_id.as_str(), Icon::AudioOut, &format!("Output: {output_text}"), false)
    } else { route(ui, output_id.as_str(), Icon::AudioOut, &output_text, "st.16", "Output") };
    if output {
        menu::open_under(ui, cx, menu::Target::Output(slot), &output_id);
    }

    let title = title(ui, cx, slot);
    let progress = loading(cx, slot);
    let failed = cx.view.parts[slot].status.starts_with("Load failed");
    let selected = cx.state.chosen() == Some(slot);
    let library = cx.library_of(Path::new(&cx.selection.parts[slot].path));
    let banner = cx.looks(&library).and_then(|l| l.banner[usize::from(cx.blurred())].clone());

    let part = &mut cx.selection.parts[slot];
    let mut gain = f64::from(part.gain);
    let gain_el = gain_knob(ui, &format!("volume-{slot}"), &mut gain);
    part.gain = gain as f32;
    let mut pan = f64::from(part.pan);
    let pan_el = pan_wedge(ui, &format!("pan-{slot}"), &mut pan);
    part.pan = pan as f32;
    let mut tune = f64::from(part.tune);
    let range = f64::from(crate::sound::TUNE_RANGE);
    let tune_el = tune_field(ui, &format!("tune-{slot}"), &mut tune, -range..=range);
    part.tune = tune as f32;
    let (mut solo, mut mute) = (part.solo, part.mute);
    let switches = solo_mute(ui, &slot.to_string(), &mut solo, &mut mute);
    (part.solo, part.mute) = (solo, mute);
    let shared = cx.p.shared.part(slot);
    let meter = shared.clone();
    let level = move || meter.as_ref().map_or([0.; 2], |part| crate::plugin::Meters::read(&part.meter));
    let dot = activity_dot(move || shared.as_ref().is_some_and(|part| crate::plugin::Meters::read(&part.meter) != [0.; 2]));
    if remove {
        cx.remove(slot);
    }

    // Narrow, the name keeps the room: the bar along the foot shows the load.
    let chip = match progress {
        Some(_) if narrow => None,
        Some(done) => Some(load_chip(done, true)),
        None => failed.then(|| caption("Failed to load").text_size(SMALL - 1.).fill(secondary()).lines(1).shrink(0)),
    };
    let mut name_row = vec![title, prev_el, next_el];
    name_row.extend(chip);
    let name_row = row(name_row).gap(0).align(Align::Center).flex(1).min_w(0);
    let mix = cluster(vec![midi_el, output_el, pan_el, gain_el, tune_el]).gap(if narrow { TIGHT } else { SPACE });
    let mut tail = vec![switches];
    if !narrow { tail.push(dot); }
    tail.push(more_el);
    tail.extend(remove_el);
    let tail = cluster(tail).gap(TIGHT + 1.);
    let body = row![fold_el, name_row, mix, tail]
        .gap(if narrow { TIGHT } else { SPACE })
        .align(Align::Center)
        .pad(edges(0., SPACE, 0., TIGHT))
        .flex(1)
        .min_w(0);
    // The part's own color runs down the left edge; the bottom edge
    // doubles as load progress.
    let edge = block(3, Len::Pct(100.)).fill(part_color(slot)).shrink(0);
    let meter = col![meter_v(level)].pad((3, 0)).h(Len::Pct(100.)).shrink(0);
    let mut layers = Vec::new();
    if let Some(image) = banner {
        // The artwork fades in behind the controls; the name sits on a
        // darker stretch of it so a logo in the artwork never crowds it.
        layers.push(
            block(BANNER.0, Len::Pct(100.))
                .fill(Fill::Image(image, moose::mui::mui::scene::Fit::Cover))
                .anchor(Align::Start, Align::Start),
        );
        layers.push(
            block(BANNER.0 * 0.75, Len::Pct(100.))
                .fill(Gradient::linear(
                    90.,
                    [(0., Role::Surface.alpha(0.7)), (0.5, Role::Surface.alpha(0.45)), (1., Role::Surface.alpha(0.))],
                ))
                .anchor(Align::Start, Align::Start),
        );
    }
    layers.push(row![edge, body, meter].gap(0).align(Align::Stretch).w(Len::Pct(100.)).h(Len::Pct(100.)));
    layers.extend(progress.map(|done| {
        canvas(move |s| {
            vec![
                Draw::fill(rect(0., 0., s.width, s.height), Role::Ink.alpha(0.1)),
                Draw::fill(rect(0., 0., (s.width * done).max(SPACE), s.height), Role::Ink.alpha(0.6)),
            ]
        })
        .w(Len::Pct(100.))
        .h(2)
        .anchor(Align::Start, Align::End)
        .named("Load progress")
    }));
    let name = name(cx, slot);
    // Selected: a brighter fill inside a crisp line.
    stack(layers)
        .w(Len::Pct(100.))
        .h(SLIM - 1.)
        .fill(if selected { Role::Level(3) } else { Role::Surface })
        .when(selected, |e| e.stroke(Role::Ink.alpha(0.55)).stroke_width(1))
        .when(over, |e| e.stroke(Role::Ink).stroke_width(1))
        .clip()
        .a11y(A11y::Group)
        .named(name)
        .id(id)
        .shrink(0)
}

/// What the part is: its library, groups and zones, size, or that it failed to load.
fn facts(cx: &Cx, slot: usize) -> String {
    let v = &cx.view.parts[slot];
    let library = cx.library_of(Path::new(&cx.selection.parts[slot].path));
    // "Areia 1.2.0 [Audio Imperia]": the vendor in brackets goes.
    let library = library_label(&library);
    let mut facts = vec![library.split(" [").next().unwrap_or_default().to_owned()];
    if let Some(r) = &v.report {
        facts.push(format!("{} groups · {} zones", r.decoded.groups, r.decoded.zones));
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
        let el = field.el.h(STRIP).flex(1).min_w(NAME_MIN).named("Part name");
        if cancel {
            cx.state.renaming = None;
        } else if done {
            let text = cx.state.renaming.take().map(|(_, t)| t).unwrap_or_default();
            let text = text.trim();
            let part = &cx.selection.parts[slot];
            let default = cx.instrument_name(slot).unwrap_or_else(|| super::header::stem(&part.path));
            let shown = without_library(&default, &cx.library_of(Path::new(&part.path)));
            cx.selection.parts[slot].name = if text == default || text == shown || text.is_empty() {
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
        cx.state.select(slot);
    }
    if r.dragged && r.button == Some(Button::Primary) {
        ui.start_drag(name_id.as_str(), RackDrag::Part(slot));
    }
    let muted = cx.selection.parts[slot].mute;
    // The library and size stay in the tooltip: beside the name they only repeat it.
    let facts = facts(cx, slot);
    body(name.clone())
        .text_size(TEXT + 1.)
        .text_weight(Weight::SEMIBOLD)
        .fill(if muted { secondary() } else { Fill::from(Role::Ink) })
        .lines(1)
        .min_w(NAME_MIN)
        .shrink(1)
        .cursor(Cursor::Grab)
        .tip(format!("{name}\n{facts}\nDrag to reorder · double-click to rename"))
        .named(name)
        .id(name_id)
}
