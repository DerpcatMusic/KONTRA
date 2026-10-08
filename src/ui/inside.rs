//! A part's views beside its interface, from the translated instrument
//! (`PartView.instrument`): its articulations, how its zones map keys and
//! velocities, its envelopes, filters and modulation, and what it is.

use super::{Cx, theme::*};
use moose::mui::mui::prelude::*;
use sampler_ir as ir;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::Ordering;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum View {
    #[default]
    Interface,
    Articulations,
    Mapping,
    Sound,
    Info,
}

impl View {
    const ALL: [Self; 5] = [Self::Interface, Self::Articulations, Self::Mapping, Self::Sound, Self::Info];

    fn label(self) -> &'static str {
        match self {
            Self::Interface => "Interface",
            Self::Articulations => "Articulations",
            Self::Mapping => "Mapping",
            Self::Sound => "Sound",
            Self::Info => "Info",
        }
    }
}

/// What selects an articulation, as `Switching::to_bits` stores it (bits 1..4, in
/// this order); the core remaps the playing part live.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Driver {
    #[default]
    Keys,
    Velocity,
    Channel,
    Controller,
    Program,
}

impl Driver {
    const ALL: [Self; 5] = [Self::Keys, Self::Velocity, Self::Channel, Self::Controller, Self::Program];

    fn label(self) -> &'static str {
        match self {
            Self::Keys => "Keys",
            Self::Velocity => "Velocity",
            Self::Channel => "Channel",
            Self::Controller => "CC",
            Self::Program => "Program",
        }
    }
}

/// A part's views across frames.
#[derive(Default)]
pub struct State {
    /// `None` until picked: the interface when there is one, else Info.
    pub view: Option<View>,
    /// The articulation playing, following switch keys as they are played.
    pub active: Option<usize>,
    edit: Option<Edit>,
    /// The group the mapping picks out.
    pub group: Option<usize>,
}

impl State {
    pub fn editing(&self) -> bool { self.edit.is_some() }
}

/// Which views `slot` has something to show in.
fn offered(cx: &Cx, slot: usize) -> Vec<View> {
    let v = &cx.view.parts[slot];
    let has_face = v.interfaces.iter().any(|i| !i.widgets.is_empty());
    let inst = v.instrument.as_deref();
    View::ALL
        .into_iter()
        .filter(|view| match view {
            View::Interface => has_face,
            View::Articulations => inst.is_some_and(|i| !i.articulations.is_empty()),
            View::Mapping | View::Sound => inst.is_some_and(|i| !i.zones.is_empty()),
            View::Info => true,
        })
        .collect()
}

/// The view switch above the stage, and the view it picks.
pub fn bar(ui: &mut Ui, cx: &mut Cx, slot: usize) -> (View, Option<El>) {
    let offered = offered(cx, slot);
    let st = cx.state.inside.entry(slot).or_default();
    let view = st.view.filter(|v| offered.contains(v)).unwrap_or(offered[0]);
    if offered.len() < 2 {
        return (view, None);
    }
    let mut picked = view;
    let tabs = offered
        .iter()
        .map(|&v| {
            let (hit, el) = latch(ui, format!("view-{slot}-{}", v.label()), v.label(), v.label(), v == view);
            if hit {
                picked = v;
            }
            el
        })
        .collect();
    cx.state.inside.entry(slot).or_default().view = Some(picked);
    (picked, Some(segmented(tabs)))
}

/// The picked view's body; the interface is [`super::part`]'s.
pub fn view(ui: &mut Ui, cx: &mut Cx, slot: usize, view: View) -> Option<El> {
    let inst = cx.view.parts[slot].instrument.clone();
    let el = match (view, inst) {
        (View::Articulations, Some(i)) => articulations(ui, cx, slot, &i),
        (View::Mapping, Some(i)) => mapping(ui, cx, slot, &i),
        (View::Sound, Some(_)) if cx.state.selected == slot && cx.p.shared.editor_watch.load(Ordering::Relaxed)==usize::MAX => super::editor::view(ui, cx, slot),
        (View::Sound, Some(_)) => {
            let (hit,button)=super::theme::action(ui,format!("edit-open-{slot}"),"Edit sound",false);
            if hit {cx.state.select(slot);}
            col![caption("Select this part to edit its sound.").fill(secondary()),button].gap(SPACE)
        },

        (View::Info, i) => info(cx, slot, i.as_deref()),
        _ => return None,
    };
    Some(el.pad((SPACE, INSET)).w(Len::Pct(100.)).shrink(0).id(format!("inside-{slot}")))
}

// Articulations ---------------------------------------------------------

/// The articulation whose switch key is down now, if any.
fn played(cx: &Cx, arts: &[ir::Articulation]) -> Option<usize> {
    let shared = &cx.p.shared;
    let down = |k: u8| shared.played[k as usize].load(Ordering::Relaxed) > 0 || shared.heard[k as usize].load(Ordering::Relaxed) > 0;
    arts.iter().position(|a| a.switch_keys.iter().any(|&k| down(k)))
}

/// The articulation `slot` plays: the last switched to, else the default.
pub fn active(cx: &mut Cx, slot: usize) -> Option<usize> {
    let inst = cx.view.parts.get(slot)?.instrument.clone()?;
    let arts = &inst.articulations;
    // The runtime's own, when it holds the articulation; else follow the keys.
    let held = cx.p.shared.part(slot).map(|p| p.articulation.load(std::sync::atomic::Ordering::Relaxed));
    if let Some(n) = held.map(|n| n as usize).filter(|&n| n < arts.len()) {
        return Some(n);
    }
    let now = played(cx, arts);
    let st = cx.state.inside.entry(slot).or_default();
    if now.is_some() {
        st.active = now;
    }
    st.active.or_else(|| arts.iter().position(|a| a.default)).or((!arts.is_empty()).then_some(0))
}

/// Stable UI identity shared by selection, trigger editor and reorder actions.
pub fn row_id(slot: usize, source: &str) -> String {
    format!("art-{slot}-{}", &blake3::hash(source.as_bytes()).to_hex()[..16])
}

#[derive(Clone, Debug, PartialEq)]
pub enum ArtAction { Reset, ResetRow(String), Clear(String), Keep, Learn(Option<String>), Move(String, i32), Driver(u8), Reassign, Include(String), Split }

#[derive(Clone, Debug)]
struct Edit {
    source: String,
    text: String,
    learned: u64,
    learn: bool,
    error: Option<String>,
    conflict: Option<(crate::sound::articulation::Input, String)>,
}

#[derive(Clone, Debug)]
struct ArtDrag { slot: usize, source: String }


pub(super) fn mode(cx: &Cx, slot: usize, inst: &ir::Instrument) -> ir::Driver {
    let p = &cx.selection.parts[slot];
    p.articulation_overlay.driver.map(crate::sound::articulation::driver).unwrap_or_else(|| {
        if p.switching & 0x80 != 0 { crate::sound::articulation::driver(p.switching >> 1 & 7) } else { inst.switching.driver }
    })
}

fn begin(ui: &mut Ui, cx: &mut Cx, slot: usize, source: &str, text: String, learn: bool) {
    let learned = cx.p.shared.learned_note.load(Ordering::Relaxed);
    let part = &cx.selection.parts[slot];
    cx.p.shared.learn_target.store(if learn { 1 << 31 | u32::from(part.port) << 8 | ((i32::from(part.channel) + 1).max(0) as u32) << 16 } else { 0 }, Ordering::Relaxed);
    cx.state.inside.entry(slot).or_default().edit = Some(Edit { source: source.into(), text, learned, learn, error: None, conflict: None });
    ui.focus(format!("{}-edit", row_id(slot, source)));
}

pub fn action(ui: &mut Ui, cx: &mut Cx, slot: usize, action: ArtAction) {
    use crate::sound::articulation::{Input, identities};
    let Some(inst) = cx.view.parts.get(slot).and_then(|v| v.instrument.clone()) else { return };
    let ids = identities(&inst.articulations);
    let mode = mode(cx, slot, &inst);
    match action {
        ArtAction::Learn(source) => {
            let source = source.or_else(|| active(cx, slot).and_then(|n| ids.get(n).cloned()));
            if let Some(source) = source { begin(ui, cx, slot, &source, String::new(), true); }
        }
        ArtAction::Move(source, by) => {
            let overlay = &mut cx.selection.parts[slot].articulation_overlay;
            let order = overlay.display_order(&inst.articulations);
            if let Some(at) = order.iter().position(|&n| ids[n] == source) {
                let to = at.saturating_add_signed(by as isize).min(order.len().saturating_sub(1));
                overlay.move_to(&inst.articulations, &source, to);
                ui.focus(format!("{}-name", row_id(slot, &source)));
            }
        }
        ArtAction::Reset => { cx.selection.parts[slot].articulation_overlay = Default::default(); cx.selection.parts[slot].switching = 0; cx.state.inside.entry(slot).or_default().edit = None; }
        ArtAction::Keep => cx.selection.parts[slot].articulation_overlay.keep_originals ^= true,
        ArtAction::Driver(driver) => { cx.selection.parts[slot].articulation_overlay.driver = Some(driver); cx.state.inside.entry(slot).or_default().edit = None; }
        ArtAction::ResetRow(source) => { cx.selection.parts[slot].articulation_overlay.inputs.remove(&source); cx.state.inside.entry(slot).or_default().edit = None; }
        ArtAction::Clear(source) => {
            let input = match mode { ir::Driver::Keys => Input::Keys(Vec::new()), ir::Driver::Channel => Input::Channel(None), ir::Driver::Velocity => Input::Velocity(None), ir::Driver::Controller => Input::Controller(None), ir::Driver::Program => Input::Program(None) };
            cx.selection.parts[slot].articulation_overlay.set(&source, input);
        }
        ArtAction::Include(source) => {
            let input = cx.selection.parts[slot].articulation_overlay.inputs.entry(source).or_default();
            input.enabled = Some(input.enabled == Some(false));
        }
        ArtAction::Split => {
            if !cx.selection.parts[slot].articulation_overlay.split_velocities(&inst.articulations) {
                cx.state.notice = "Velocity splitting supports at most 127 participating rows.".into();
            }
        }
        ArtAction::Reassign => {
            let overlay = &mut cx.selection.parts[slot].articulation_overlay;
            let order = overlay.display_order(&inst.articulations);
            let original: Vec<_> = inst.articulations.iter().enumerate().map(|(n, a)| overlay.input(&ids[n], a, mode)).collect();
            // Explicitly rotate the existing assignments into display order. Source IR stays unchanged.
            for (rank, n) in order.into_iter().enumerate() { overlay.set(&ids[n], original[rank].clone()); }
        }
    }
    if !cx.state.inside.entry(slot).or_default().edit.as_ref().is_some_and(|e| e.learn) { cx.p.shared.learn_target.store(0, Ordering::Relaxed); }
}

pub fn input_label(input: &crate::sound::articulation::Input) -> String {
    use crate::sound::articulation::Input;
    match input {
        Input::Keys(keys) => match keys.as_slice() { [] => "—".into(), [key] => note_name(*key), [key, ..] => format!("{} +{}", note_name(*key), keys.len() - 1) },
        Input::Velocity(Some((low, high))) => format!("{low}–{high}"),
        Input::Channel(Some(channel)) => format!("ch {}", u16::from(*channel) + 1),
        Input::Controller(Some((cc, low, high))) if low == high => format!("CC{cc} {low}"),
        Input::Controller(Some((cc, low, high))) => format!("CC{cc} {low}–{high}"),
        Input::Program(Some(program)) => format!("prog {}", u16::from(*program) + 1),
        _ => "—".into(),
    }
}

fn input_text(input: &crate::sound::articulation::Input) -> String {
    match input { crate::sound::articulation::Input::Keys(keys) => keys.iter().map(|&k| note_name(k)).collect::<Vec<_>>().join(", "), _ => input_label(input) }
}

/// The same authored/fallback swatch is used for the row and its keyboard key.
pub fn articulation_color(cx: &Cx, slot: usize, source: &str, art: &ir::Articulation) -> Color {
    if let Some(rgb) = cx.selection.parts[slot].articulation_overlay.inputs.get(source).and_then(|i| i.color) {
        return Color::srgb(f32::from(rgb[0]) / 255., f32::from(rgb[1]) / 255., f32::from(rgb[2]) / 255.);
    }
    if let Some(color) = art.switch_keys.iter().find_map(|&key| cx.view.parts[slot].keys.get(usize::from(key)).and_then(|k| k.color).and_then(ksp_key_color)) { return color; }
    let hash = blake3::hash(source.as_bytes());
    Color::oklch(0.72, 0.12, golden_hue(200., u16::from_le_bytes([hash.as_bytes()[0], hash.as_bytes()[1]]) as usize))
}

fn trigger_cell(ui: &mut Ui, cx: &mut Cx, slot: usize, inst: &ir::Instrument, source: &str, n: usize, mode: ir::Driver) -> El {
    use crate::sound::articulation::{Input, identities, parse_input};
    let id = row_id(slot, source);
    let edit_id = format!("{id}-edit");
    let cell_id = format!("{id}-trigger");
    let input = cx.selection.parts[slot].articulation_overlay.input(source, &inst.articulations[n], mode);
    let name = &inst.articulations[n].name;
    let tip = format!("{name} trigger: {}. Click to edit. MIDI 60 = C3. Use commas for multiple keys.", input_text(&input));
    if ui.get(cell_id.as_str()).activated() { begin(ui, cx, slot, source, input_text(&input), false); }
    let owns = cx.state.inside.entry(slot).or_default().edit.as_ref().is_some_and(|e| e.source == source);
    let Some(mut edit) = owns.then(|| cx.state.inside.entry(slot).or_default().edit.take()).flatten() else {
        // A different row owns the editor; leave it attached to that identity.
        let el = caption(input_label(&input)).fill(Role::Ink).lines(1).pad((0, TIGHT)).w(88.).h(24.).focusable().a11y(A11y::Button).named(format!("{name} trigger")).tip(tip).id(cell_id);
        return interactive(el, false);
    };
    let existed = ui.scene().is_some_and(|s| s.surface(&edit_id).is_some());
    if !existed { ui.focus(edit_id.as_str()); }
    let mut learned = None;
    if edit.learn && mode == ir::Driver::Keys {
        let now = cx.p.shared.learned_note.load(Ordering::Relaxed);
        learned = (now != edit.learned).then(|| Input::Keys(vec![now as u8 & 127]));
        edit.learned = now;
    }
    let field = text_edit(ui, edit_id.as_str(), &mut edit.text, TextOpts::default());
    if field.changed.changed { edit.error = None; edit.conflict = None; }
    // MUI clears focus on Escape before this frame; read both streams.
    let escape = ui.keys(edit_id.as_str()).iter().chain(ui.shortcuts()).any(|k| k.key == Key::Escape);
    let done = existed && !edit.learn && (field.changed.submitted || (!ui.focused(edit_id.as_str()) && edit.conflict.is_none()));
    let proposal = learned.map(Ok).or_else(|| (done && !escape).then(|| parse_input(mode, &edit.text)));
    let mut committed = false;
    if let Some(proposal) = proposal {
        match proposal {
            Err(error) => { edit.error = Some(error.into()); ui.focus(edit_id.as_str()); }
            Ok(input) => {
                let conflicts = cx.selection.parts[slot].articulation_overlay.conflicts(&inst.articulations, source, mode, &input);
                match conflicts.as_slice() {
                    [] => { cx.selection.parts[slot].articulation_overlay.set(source, input); committed = true; }
                    [other] => { edit.error = None; edit.conflict = Some((input, identities(&inst.articulations)[*other].clone())); }
                    _ => { edit.error = Some("Conflicts with multiple rows; clear those inputs first".into()); edit.conflict = None; }
                }
            }
        }
    }
    let el = field.el.w(88.).h(24.).radius(0).shrink(0).named(format!("{name} trigger")).tip(edit.error.clone().unwrap_or_else(|| if edit.learn { format!("{tip}\nPlay a new MIDI key to learn; Escape cancels") } else { tip }));
    if !escape && !committed { cx.state.inside.entry(slot).or_default().edit = Some(edit); }
    else { cx.p.shared.learn_target.store(0, Ordering::Relaxed); }
    el
}

fn articulations(ui: &mut Ui, cx: &mut Cx, slot: usize, inst: &ir::Instrument) -> El {
    use crate::sound::articulation::identities;
    use super::menu::{self, Target};
    let arts = &inst.articulations;
    let ids = identities(arts);
    let active = active(cx, slot);
    let mode = mode(cx, slot, inst);
    let chosen = Driver::ALL[mode as usize].label();
    let driver_id = format!("art-driver-{slot}");
    let (hit, driver_el) = latch(ui, driver_id.as_str(), chosen, "Articulation trigger mode", false);
    if hit { menu::open_under(ui, cx, Target::ArtDriver(slot), &driver_id); }
    let more_id = format!("arts-more-{slot}");
    let (more, more_el) = icon_button(ui, more_id.as_str(), Icon::More, "Articulations menu", false);
    if more { menu::open_under(ui, cx, Target::Articulations(slot), &more_id); }
    let capacity = if !cx.selection.parts[slot].articulation_overlay.valid() { Some("Invalid saved mappings; Reset mappings") } else { match mode { ir::Driver::Channel if arts.len() > 16 => Some("16 channels maximum"), ir::Driver::Velocity if arts.len() > 127 => Some("127 velocity partitions maximum"), ir::Driver::Controller | ir::Driver::Program if arts.len() > 128 => Some("128 values maximum"), _ => None } };
    let edit = cx.state.inside.entry(slot).or_default().edit.clone();
    let error = edit.as_ref().and_then(|e| e.error.clone()).or_else(|| capacity.map(String::from));
    let mut head = vec![section("Articulations"), caption(arts.len().to_string()).fill(secondary()), spacer(), driver_el.h(24.), more_el.h(24.).w(24.)];
    if let Some(error) = error { head.insert(2, caption(error).fill(Role::Danger).lines(1).min_w(0).tip("Correct the trigger and press Enter")); }
    if let Some(edit) = edit.as_ref().filter(|e| e.learn) { head.insert(2, caption(format!("Learn {}…", arts[ids.iter().position(|id| id == &edit.source).unwrap_or(0)].name)).lines(1).min_w(0)); }
    if let Some(edit) = edit.as_ref() && let Some((proposal, other)) = &edit.conflict {
        let other_name = ids.iter().position(|id| id == other).map(|n| arts[n].name.as_str()).unwrap_or("other row");
        let (swap, swap_el) = latch(ui, format!("art-swap-{slot}"), "Swap", &format!("Swap triggers with {other_name}"), false);
        let (cancel, cancel_el) = icon_button(ui, format!("art-cancel-{slot}"), Icon::Close, "Cancel trigger swap", false);
        head.insert(2, caption(format!("Used by {other_name}")).lines(1).min_w(0));
        head.insert(3, swap_el.h(24.)); head.insert(4, cancel_el.h(24.));
        if swap {
            let n = ids.iter().position(|id| id == &edit.source).unwrap();
            let overlay = &mut cx.selection.parts[slot].articulation_overlay;
            let previous = overlay.input(&edit.source, &arts[n], mode);
            // Offer a concrete swap only when it also leaves every other row unambiguous.
            let mut candidate = overlay.clone();
            candidate.set(&edit.source, proposal.clone()); candidate.set(other, previous);
            let other_n = ids.iter().position(|id| id == other).unwrap();
            if candidate.conflicts(arts, other, mode, &candidate.input(other, &arts[other_n], mode)).is_empty() {
                *overlay = candidate;
                cx.p.shared.learn_target.store(0, Ordering::Relaxed);
                cx.state.inside.entry(slot).or_default().edit = None;
                ui.focus(format!("{}-trigger", row_id(slot, &edit.source)));
            } else { cx.state.inside.entry(slot).or_default().edit.as_mut().unwrap().error = Some("Swap would overlap another row; clear it first".into()); }
        }
        if cancel { cx.state.inside.entry(slot).or_default().edit = None; cx.p.shared.learn_target.store(0, Ordering::Relaxed); }
    }
    let head = row(head).gap(SPACE).align(Align::Center).h(24.).shrink(0);
    let order = cx.selection.parts[slot].articulation_overlay.display_order(arts);
    let mut rows = Vec::new();
    for (place, n) in order.into_iter().enumerate() {
        let a = &arts[n]; let source = &ids[n]; let id = row_id(slot, source); let on = active == Some(n);
        let name_id = format!("{id}-name");
        if ui.get(name_id.as_str()).activated() {
            cx.p.shared.select_articulation(slot, n);
            cx.state.inside.entry(slot).or_default().active = Some(n);
        }
        let handle_id = format!("{id}-drag");
        let r = ui.get(handle_id.as_str());
        if r.dragged && r.button == Some(Button::Primary) { ui.start_drag(handle_id.as_str(), ArtDrag { slot, source: source.clone() }); }
        // MUI reports the leaf under the pointer; every cell belongs to this row.
        let target = ui.dropped().and_then(|(_, target)| (target == id || target.starts_with(&format!("{id}-"))).then(|| target.to_owned()));
        if let Some(drag) = target.and_then(|t| ui.dropped_on::<ArtDrag>(t.as_str())).filter(|d| d.slot == slot) {
            cx.selection.parts[slot].articulation_overlay.move_to(arts, &drag.source, place);
            ui.focus(format!("{}-name", row_id(slot, &drag.source)));
        }
        let handle = canvas(|_| (0..3).flat_map(|y| [3., 7.].map(move |x| Draw::fill(rect(x, 6. + y as f64 * 4., 2., 2.), secondary()))).collect()).w(12.).h(24.).cursor(Cursor::Grab).named(format!("Reorder {}", a.name)).tip("Drag to reorder display only; Move Up/Down is in the row menu").id(handle_id);
        let color = articulation_color(cx, slot, source, a);
        let stripe = block(3., 18.).fill(color.with_alpha(if on { 1. } else { 0.7 })).shrink(0);
        let dot = block(4., 4.).fill(if on { Fill::from(Role::Ink) } else { Role::Ink.alpha(0.) }).shrink(0);
        let name = row![dot, body(a.name.clone()).fill(Role::Ink).lines(1).flex(1).min_w(0), caption(if a.default { "default" } else { "" }).fill(secondary()).lines(1)]
            .gap(TIGHT).align(Align::Center).flex(1).min_w(0).h(24.).focusable().a11y(A11y::Toggle { on }).named(format!("{}{}", a.name, if a.default { ", default articulation" } else { "" })).tip(a.name.clone()).id(name_id);
        let cell = trigger_cell(ui, cx, slot, inst, source, n, mode);
        let more_id = format!("{id}-more");
        let (hit, more) = icon_button(ui, more_id.as_str(), Icon::More, &format!("{} actions", a.name), false);
        if hit { menu::open_under(ui, cx, Target::Articulation(slot, source.clone()), &more_id); }
        let row = row![handle, stripe, interactive(name, on), cell, more.w(24.).h(24.)].gap(TIGHT).align(Align::Center).pad((0, TIGHT)).h(24.).when(on, |e| e.fill(Role::Raised)).a11y(A11y::Group).named(a.name.clone()).id(id);
        rows.push(row);
    }
    let list = col(rows).gap(0).align(Align::Stretch).max_size(Size::new(1e6, 24. * 12.)).scroll().shrink(0).id(format!("arts-{slot}"));
    col![head, list].gap(TIGHT).align(Align::Stretch).w(Len::Pct(100.)).min_w(0)
}

// Mapping ---------------------------------------------------------------

/// Group `g`'s color in the map and its list.
fn group_color(g: usize) -> Color {
    Color::oklch(0.72, 0.09, golden_hue(200., g))
}

fn mapping(ui: &mut Ui, cx: &mut Cx, slot: usize, inst: &ir::Instrument) -> El {
    let groups = inst.groups.len();
    let mut counts = vec![0usize; groups + 1];
    // ponytail: walks every zone each frame the map shows; cache per load if
    // a 50k-zone instrument makes it slow.
    let mut rects = HashSet::new();
    let (mut low, mut high) = (127u8, 0u8);
    for z in &inst.zones {
        let g = z.group.map_or(groups, |g| g.0);
        counts[g] += 1;
        rects.insert((g, z.keys.low, z.keys.high, z.velocities.low, z.velocities.high));
        (low, high) = (low.min(z.keys.low), high.max(z.keys.high));
    }
    let picked = cx.state.inside.entry(slot).or_default().group;
    let mut pick = picked;
    let mut list = Vec::new();
    let names = inst.groups.iter().map(|g| g.name.as_str()).chain(std::iter::once("No group"));
    for (g, name) in names.enumerate().filter(|&(g, _)| counts[g] > 0) {
        let id = format!("map-group-{slot}-{g}");
        let on = picked == Some(g);
        if ui.get(id.as_str()).activated() {
            pick = if on { None } else { Some(g) };
        }
        let label = if name.is_empty() { format!("Group {}", g + 1) } else { name.to_owned() };
        let el = row![
            block(TIGHT, TIGHT * 3.).fill(group_color(g)).shrink(0),
            body(label.clone()).fill(if on { Fill::from(Role::Ink) } else { secondary() }).lines(1).flex(1).min_w(0),
            caption(counts[g].to_string()).fill(secondary()).shrink(0)
        ]
        .gap(SPACE)
        .align(Align::Center)
        .pad((0, SPACE))
        .h(CONTROL - TIGHT)
        .when(on, |e| e.fill(Role::Raised))
        .focusable()
        .a11y(A11y::Button)
        .named(label)
        .id(id);
        list.push(interactive(el, on));
    }
    cx.state.inside.entry(slot).or_default().group = pick;

    let (first, last) = (low.min(high), high.max(low));
    // Whole octaves, so each C sits at the start of an equal cell.
    let (low, high) = if low > high { (0, 119) } else { (low / 12 * 12, (high / 12 * 12 + 11).min(127)) };
    let rects: Vec<_> = rects.into_iter().collect();
    let keys = f64::from(high - low + 1);
    let map = canvas(move |s| {
        let x = |k: u8| f64::from(k - low) / keys * s.width;
        let y = |v: u8| (1. - f64::from(v) / 127.) * s.height;
        let mut out = vec![Draw::fill(rect(0., 0., s.width, s.height), Role::Field)];
        for c in (low..=high).filter(|k| k % 12 == 0) {
            out.push(Draw::fill(rect(x(c).round(), 0., 1., s.height), hairline()));
        }
        // The picked group last, so it lies on top.
        let mut sorted = rects.clone();
        sorted.sort_by_key(|r| (pick == Some(r.0), r.0));
        for (g, kl, kh, vl, vh) in sorted {
            let r = rect(x(kl), y(vh), x(kh) - x(kl) + s.width / keys, y(vl.saturating_sub(1)) - y(vh));
            let shown = pick.is_none_or(|p| p == g);
            let c = group_color(g);
            out.push(Draw::fill(r.clone(), c.with_alpha(if shown { 0.22 } else { 0.04 })));
            if shown {
                out.push(Draw::stroke(r.clone(), c.with_alpha(0.8), 1.));
            }
        }
        out
    })
    .flex(1)
    .min_w(0)
    .h(TEXT * 16.)
    .clip()
    .named("Zones by key and velocity")
    .id(format!("map-{slot}"));
    let scale = row((low..=high).filter(|k| k % 12 == 0).map(|c| row![caption(note_name(c)).text_size(SMALL).fill(secondary()).lines(1)].flex(1).min_w(0)).collect::<Vec<_>>())
        .gap(0)
        .w(Len::Pct(100.))
        .shrink(0);
    let head = row![section("Mapping"), caption(format!("{} zones · {} – {}", inst.zones.len(), note_name(first), note_name(last))).fill(secondary()).lines(1), spacer(), caption("Keys across, velocity up").fill(secondary())]
        .gap(SPACE)
        .align(Align::Center)
        .shrink(0);
    let groups = col(list).gap(1).align(Align::Stretch).pad(edges(0., SPACE, 0., 0.)).w(TEXT * 16.).h(TEXT * 16.).scroll().shrink(0);
    col![head, row![groups, col![map, scale].gap(TIGHT).flex(1).min_w(0)].gap(SPACE)].gap(SPACE).align(Align::Stretch)
}

// Sound -----------------------------------------------------------------

fn hz(f: ir::Frequency) -> String {
    match f {
        ir::Frequency::Hertz(h) if h >= 1000. => format!("{:.1} kHz", h / 1000.),
        ir::Frequency::Hertz(h) => format!("{h:.0} Hz"),
        ir::Frequency::Beats(b) => format!("{b} beats"),
    }
}

pub(super) fn source_name(s: &ir::ModulationSource) -> String {
    match s {
        ir::ModulationSource::Envelope(_) => "Envelope".into(),
        ir::ModulationSource::Lfo(l) => format!("LFO {}", hz(l.rate)),
        ir::ModulationSource::Controller(c) => format!("CC {c}"),
        other => format!("{other:?}").split(['(', ' ', '{']).next().unwrap_or_default().to_owned(),
    }
}

// Info ------------------------------------------------------------------

fn info(cx: &Cx, slot: usize, inst: Option<&ir::Instrument>) -> El {
    let v = &cx.view.parts[slot];
    let pair = |k: &str, val: String| {
        row![caption(k.to_owned()).fill(secondary()).w(TEXT * 8.).shrink(0), body(val.clone()).lines(1).min_w(0).tip(val)].gap(SPACE).align(Align::Center).shrink(0)
    };
    let mut rows = Vec::new();
    rows.push(pair("Instrument", super::rack::name(cx, slot)));
    rows.push(pair("File", cx.selection.parts[slot].path.clone()));
    if let Some(r) = &v.report {
        let d = &r.decoded;
        rows.push(pair("Format", d.format.clone()));
        rows.push(pair("Contents", format!("{} zones · {} groups · {} samples · {} buses", d.zones, d.groups, d.samples, d.buses)));
        rows.push(pair("Scripts", format!("{} scripts · {} controls · {} articulations", d.scripts, d.controls, d.articulations)));
        let keys: Vec<u8> = (0..128).filter(|&k| d.maps(k)).collect();
        if let (Some(&lo), Some(&hi)) = (keys.first(), keys.last()) {
            rows.push(pair("Keys", format!("{} – {} · {} keys", note_name(lo), note_name(hi), keys.len())));
        }
        rows.push(pair("Not translated", r.missing.len().to_string()));
    }
    if let Some(i) = inst {
        let switches: usize = i.articulations.iter().map(|a| a.switch_keys.len()).sum();
        if switches > 0 {
            rows.push(pair("Keyswitches", switches.to_string()));
        }
    }
    if let Some(f) = cx.state.faces.get(&slot) {
        rows.push(pair("Interface", format!("{:.1} MB of pictures decoded", f.bytes() as f64 / (1024. * 1024.))));
    }
    row![col(rows).gap(TIGHT).align(Align::Stretch).w(TEXT * 60.).min_w(0), spacer()].w(Len::Pct(100.))
}

/// The keys `slot`'s articulations switch on, for the keyboard's marks.
pub fn switch_keys(cx: &Cx, slot: usize) -> Option<Arc<ir::Instrument>> {
    cx.view.parts.get(slot)?.instrument.clone().filter(|i| !i.articulations.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sound::articulation::Input;
    #[test]
    fn keyswitch_cells_show_actual_inputs_without_inventing_ranges() {
        assert_eq!(input_label(&Input::Keys(vec![49, 60, 73])), "C#2 +2");
        assert_eq!(input_label(&Input::Channel(Some(6))), "ch 7");
        assert_eq!(input_label(&Input::Velocity(Some((19, 36)))), "19–36");
        assert_eq!(input_label(&Input::Controller(Some((12, 3, 3)))), "CC12 3");
        assert_eq!(input_label(&Input::Program(Some(127))), "prog 128");
    }
}
