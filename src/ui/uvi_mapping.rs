//! UVI-owned inspection adapter to the shared key × velocity canvas.
use super::{instrument, theme::*};
#[cfg(feature = "plugin")]
use super::Cx;
use crate::uvi::{mapping::Inspection, program::{NodeId, SampleZone}, worker::Stamp};
use moose::mui::mui::prelude::*;
use std::{collections::HashSet, sync::{Arc, Weak}};

const LAYERS_SHOWN: usize = 128;
const DETAILS_SHOWN: usize = 24;
const MAP_ID: &str = "uvi-mapping-grid";

#[derive(Clone)]
pub(super) struct State {
    pub layer: Option<NodeId>,
    pub layer_page: usize,
    pub zone_page: usize,
    pub selected: Option<usize>,
    pub note: u8,
    pub velocity: u8,
    pub filter: bool,
    owner: Option<(Stamp, Weak<Vec<SampleZone>>)>,
    query_for: Option<(NodeId, u8, u8)>,
    matches: Arc<Vec<usize>>,
    key_zones: usize,
    sample_paths: usize,
}

impl Default for State {
    fn default() -> Self {
        Self { layer: None, layer_page: 0, zone_page: 0, selected: None, note: 60, velocity: 100,
            filter: false, owner: None, query_for: None, matches: Arc::default(), key_zones: 0, sample_paths: 0 }
    }
}

impl State {
    fn bind(&mut self, mapping: &Inspection) {
        let same = self.owner.as_ref().is_some_and(|(stamp, zones)| *stamp == mapping.stamp
            && zones.upgrade().is_some_and(|zones| Arc::ptr_eq(&zones, &mapping.zones)));
        if !same {
            *self = Self { owner: Some((mapping.stamp, Arc::downgrade(&mapping.zones))), ..Default::default() };
        }
    }

    fn query(&mut self, mapping: &Inspection) {
        let Some(layer) = self.layer else {
            self.query_for = None; self.matches = Arc::default(); self.key_zones = 0; self.sample_paths = 0;
            return;
        };
        let key = (layer, self.note, self.velocity);
        if self.query_for == Some(key) { return; }
        let mut matches = Vec::new();
        let mut paths = HashSet::new();
        self.key_zones = 0;
        // ponytail: scan one retained layer only when the discrete probe changes;
        // add a loader-built range index if measured probe latency requires it.
        for &index in mapping.layers.get(&layer).into_iter().flatten() {
            let zone = &mapping.zones[index];
            if (zone.low_key..=zone.high_key).contains(&self.note) {
                self.key_zones += 1;
                if (zone.low_velocity..=zone.high_velocity).contains(&self.velocity) {
                    matches.push(index); paths.insert(zone.sample_path.as_str());
                }
            }
        }
        self.sample_paths = paths.len();
        self.matches = Arc::new(matches);
        self.query_for = Some(key);
    }
}

fn pager(ui: &mut Ui, id: &str, count: usize, page: &mut usize) -> El {
    let pages = count.max(1);
    *page = (*page).min(pages - 1);
    let (previous, previous_el) = action(ui, format!("{id}-previous"), "Previous", false);
    let (next, next_el) = action(ui, format!("{id}-next"), "Next", false);
    let can_previous = *page > 0;
    let can_next = *page + 1 < pages;
    if previous && can_previous { *page -= 1; }
    if next && can_next { *page += 1; }
    row![previous_el.when(!can_previous, |el| el.disabled()), spacer(),
        caption(format!("{} / {pages}", *page + 1)), spacer(), next_el.when(!can_next, |el| el.disabled())]
        .gap(TIGHT).align(Align::Center).w(Len::Pct(100.)).min_w(0).shrink(0)
}

fn node_name(mapping: &Inspection, node: NodeId) -> String {
    let name = mapping.report["nodes"][node]["name"].as_str().unwrap_or_default();
    if name.is_empty() { format!("Layer {}", node + 1) } else { name.to_owned() }
}

#[cfg(feature = "plugin")]
pub(super) fn view(ui: &mut Ui, cx: &mut Cx) -> El {
    let slot = cx.state.selected;
    let v = &cx.view.parts[slot];
    // The existing getter fences source, saved state, rate, part generation and
    // activation stamp; an audio endpoint is not needed for read-only inspection.
    let mapping = v.uvi_mapping_inspection(cx.p, slot, &cx.selection);
    let Some((mapping, ready)) = mapping else {
        cx.state.uvi_mapping.remove(&slot);
        return col![body("Initial sample mapping is not available yet."),
            caption(if v.loading { "Waiting for the UVI program to be parsed." } else {
                "The program was not parsed for this load. See Info or Logs for its status."
            }).fill(secondary()).lines(3)].gap(SPACE).pad(INSET).flex(1).min_h(0);
    };
    let mut state = cx.state.uvi_mapping.get(&slot).filter(|(stamp, _)| *stamp == mapping.stamp)
        .map(|(_, state)| state.clone()).unwrap_or_default();
    let el = inspection(ui, &mapping, &mut state, ready);
    cx.state.uvi_mapping.insert(slot, (mapping.stamp, state));
    el
}

fn layer_list(ui: &mut Ui, mapping: &Inspection, state: &mut State) -> El {
    let old_page = state.layer_page;
    let page = pager(ui, "uvi-mapping-layer-page", mapping.layer_order.len().div_ceil(LAYERS_SHOWN), &mut state.layer_page);
    let start = state.layer_page * LAYERS_SHOWN;
    let end = (start + LAYERS_SHOWN).min(mapping.layer_order.len());
    let shown = &mapping.layer_order[start..end];
    if old_page != state.layer_page || !state.layer.is_some_and(|layer| shown.contains(&layer)) {
        state.layer = shown.first().copied(); state.zone_page = 0; state.selected = None;
    }
    let mut layers = Vec::new();
    for &layer in shown {
        let label = format!("{} · {} zones", node_name(mapping, layer), mapping.layers[&layer].len());
        let (hit, el) = action(ui, format!("uvi-mapping-layer-{layer}"), &label, state.layer == Some(layer));
        if hit && state.layer != Some(layer) { state.layer = Some(layer); state.zone_page = 0; state.selected = None; }
        layers.push(el.lines(2).min_w(0).w(Len::Pct(100.)).shrink(0));
    }
    col![section("Layers"), col(layers).gap(1).flex(1).min_h(0).scroll().id("uvi-mapping-layers"), page]
        .gap(SPACE).w(SIDEBAR_MIN).shrink(0).min_h(0)
}

fn probe_controls(ui: &mut Ui, state: &mut State) -> El {
    let before = (state.note, state.velocity, state.filter);
    let mut note = f64::from(state.note);
    let mut velocity = f64::from(state.velocity);
    let note_el = number(ui, "uvi-mapping-note", "Note", &mut note, 0. ..=127.,
        format!("{} ({})", note_name(state.note), state.note));
    let velocity_el = number(ui, "uvi-mapping-velocity", "Velocity", &mut velocity, 0. ..=127., state.velocity.to_string());
    state.note = note.round().clamp(0., 127.) as u8;
    state.velocity = velocity.round().clamp(0., 127.) as u8;
    if before.0 != state.note || before.1 != state.velocity { state.filter = true; }
    let (all, all_el) = action(ui, "uvi-mapping-all-zones", "All zones", !state.filter);
    let (at, at_el) = action(ui, "uvi-mapping-at-note", "At note / velocity", state.filter);
    if all { state.filter = false; }
    if at { state.filter = true; }
    if before != (state.note, state.velocity, state.filter) { state.zone_page = 0; }
    row![note_el.flex(1).min_w(SIDEBAR_MIN), velocity_el.flex(1).min_w(SIDEBAR_MIN), all_el, at_el]
        .wrap().gap(SPACE).line_gap(TIGHT).align(Align::Center).shrink(0)
}

fn sample_rows(ui: &mut Ui, mapping: &Inspection, state: &mut State, indices: &[usize], start: usize, end: usize) -> El {
    let mut rows = Vec::new();
    for &index in &indices[start..end] {
        let zone = &mapping.zones[index];
        let label = format!("Zone {} · {}\n{}–{} · velocity {}–{}", index + 1, zone.sample_path,
            note_name(zone.low_key), note_name(zone.high_key), zone.low_velocity, zone.high_velocity);
        let (hit, el) = action(ui, format!("uvi-mapping-zone-{index}"), &label, state.selected == Some(index));
        if hit { state.selected = Some(index); }
        rows.push(el.lines(4).w(Len::Pct(100.)).min_w(0).shrink(0));
    }
    if indices.is_empty() { rows.push(caption("No authored sampled zones match this inspection. Try another note, velocity or layer.").lines(4)); }
    col(rows).gap(1).align(Align::Stretch).flex(1).min_h(0).scroll().id("uvi-mapping-details")
}

fn sample_inspector(mapping: &Inspection, selected: Option<usize>) -> El {
    let Some((index, zone)) = selected.and_then(|index| mapping.zones.get(index).map(|zone| (index, zone))) else {
        return col![section("Selected sample"), caption("Select a zone to inspect its source and ranges.").lines(3)]
            .gap(SPACE).min_w(0).min_h(0).flex(1);
    };
    let range = format!("Keys {} ({}) – {} ({}) · velocity {}–{} · root {} ({})",
        note_name(zone.low_key), zone.low_key, note_name(zone.high_key), zone.high_key,
        zone.low_velocity, zone.high_velocity, note_name(zone.root_note), zone.root_note);
    let resource = mapping.samples.as_ref().and_then(|samples| samples.get(&zone.sample_path)).map_or_else(
        || "Sample rate, duration and channels are unknown until this path is decoded.".into(), |sample| {
            let duration = sample.duration().map_or_else(|| "duration unknown".into(), |seconds| format!("{seconds:.3} s"));
            format!("Decoded · {} Hz · {} channels · {} frames · {duration}", sample.rate, sample.channels, sample.frames)
        });
    col![section("Selected sample"),
        caption(format!("Zone {} · layer {} · keygroup {} · oscillator {}", index + 1, zone.layer + 1,
            zone.keygroup + 1, zone.player + 1)).lines(3),
        caption(zone.sample_path.clone()).named(zone.sample_path.clone()).lines(4).id("uvi-mapping-selected-path"),
        caption(range.clone()).named(range).lines(4).id("uvi-mapping-selected-range"),
        caption(resource.clone()).named(resource).lines(3).id("uvi-mapping-selected-resource"),
        caption(format!("Authored flags · {} · {} · {}", if zone.purged { "purged" } else { "not purged" },
            if zone.bypassed { "bypassed" } else { "not bypassed" }, if zone.reverse { "reverse" } else { "forward" })).lines(3),
        caption("Round-robin and microphone grouping are unknown in this initial zone view. Overlapping zones or duplicate paths do not establish either.")
            .fill(secondary()).lines(5)]
        .gap(SPACE).align(Align::Start).w(Len::Pct(100.)).min_w(0).min_h(0).scroll().flex(1).id("uvi-mapping-selected")
}

fn readiness(mapping: &Inspection, ready: bool) -> El {
    let status = format!("Parsed · {} · {} · {}",
        if mapping.report["preflight_admitted"].as_bool().unwrap_or(false) { "static check admitted" } else { "static check rejected" },
        if mapping.samples.is_some() { "initial resources decoded" } else { "initial resources not decoded" },
        if ready { "playback ready" } else { "playback not ready" });
    let mut details = vec![caption(status.clone()).named(status).lines(2).id("uvi-mapping-readiness")];
    for &(node, reason) in mapping.rejections.iter().take(6) {
        let node = &mapping.report["nodes"][node];
        details.push(caption(format!("Unsupported {}: {}", node["kind"].as_str().unwrap_or("node"),
            node["preflight_rejections"][reason].as_str().unwrap_or("unknown reason"))).fill(Role::Danger).lines(3));
    }
    if mapping.rejections.len() > 6 { details.push(caption(format!("{} more static rejections. See Info or Logs.", mapping.rejections.len() - 6)).lines(2)); }
    col(details).gap(TIGHT).min_w(0).shrink(0)
}

pub(super) fn inspection(ui: &mut Ui, mapping: &Inspection, state: &mut State, ready: bool) -> El {
    state.bind(mapping);
    let layers = layer_list(ui, mapping, state);
    if let Some((note, velocity)) = instrument::mapping_point(ui, MAP_ID) {
        state.note = note; state.velocity = velocity; state.filter = true; state.zone_page = 0;
    }
    if ui.get(MAP_ID).key_activated { state.filter = true; state.zone_page = 0; }
    let controls = probe_controls(ui, state);
    state.query(mapping);
    let matches = state.matches.clone();
    let all = state.layer.and_then(|layer| mapping.layers.get(&layer)).map(Vec::as_slice).unwrap_or_default();
    let indices = if state.filter { matches.as_slice() } else { all };
    let old_page = state.zone_page;
    let page = pager(ui, "uvi-mapping-zone-page", indices.len().div_ceil(DETAILS_SHOWN), &mut state.zone_page);
    let start = state.zone_page * DETAILS_SHOWN;
    let end = (start + DETAILS_SHOWN).min(indices.len());
    if old_page != state.zone_page || !state.selected.is_some_and(|selected|
        indices.binary_search(&selected).ok().is_some_and(|position| (start..end).contains(&position))) {
        state.selected = indices.get(start).copied();
    }
    let rows = sample_rows(ui, mapping, state, indices, start, end);
    let zones = indices[start..end].iter().map(|&index| {
        let zone = &mapping.zones[index];
        (zone.low_key, zone.high_key, zone.low_velocity, zone.high_velocity, true)
    }).collect();
    let selected = state.selected.and_then(|selected| indices[start..end].iter().position(|&index| index == selected));
    let grid = instrument::mapping_grid_selected(zones, "Initial UVI key and velocity mapping", selected,
        state.filter.then_some((state.note, state.velocity))).min_h(CONTROL * 5.).focusable().a11y(A11y::Button).cursor(Cursor::Hand).id(MAP_ID)
        .tip("Click a note and velocity to inspect every matching authored sampled zone in this layer. Enter inspects the current note and velocity. This does not play a note.");
    let count = format!("{} zones cover {} ({}) · {} zone references / {} sample paths match velocity {}",
        state.key_zones, note_name(state.note), state.note, matches.len(), state.sample_paths, state.velocity);
    let shown = if indices.is_empty() { "No zones on this page".into() } else {
        format!("Map and list show zones {}–{} of {}", start + 1, end, indices.len())
    };
    let sample_range = mapping.key_span().map_or_else(|| "No authored sample keys".into(), |(low, high)|
        format!("Authored sample keys {} – {}", note_name(low), note_name(high)));
    row![layers,
        col![controls, readiness(mapping, ready),
            caption(sample_range.clone()).named(sample_range).lines(2).id("uvi-mapping-sample-range"), grid,
            row![caption(note_name(0)), spacer(), caption("Key × velocity · authored initial zones"), spacer(), caption(note_name(127))].shrink(0),
            caption(count.clone()).named(count).lines(2).id("uvi-mapping-query-count"),
            caption(shown).lines(2),
            row![col![section(if state.filter { "Matching zones" } else { "Sample zones" }), rows, page]
                    .gap(SPACE).flex(1).min_w(SIDEBAR_MIN).min_h(0),
                sample_inspector(mapping, state.selected).min_w(SIDEBAR_MIN)]
                .wrap().gap(INSET).line_gap(SPACE).flex(1).min_h(0),
            caption("Read-only initial mapping. Scripts may change or route samples during playback; this is not the live sounding map.")
                .fill(secondary()).lines(2).shrink(0)]
            .gap(SPACE).flex(1).min_w(0).min_h(0)
    ].gap(INSET).pad(INSET).flex(1).min_h(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uvi::{program::parse_program, worker::Stamp};
    use std::sync::Arc;

    #[test]
    fn page_buttons_reach_last_layer_and_detail_then_reset_on_selection() {
        // Authored fixture: 129 layers, 25 zones in the first layer.
        let mut xml = String::from("<Program><Layers>");
        for layer in 0..129 {
            xml.push_str("<Layer><Keygroups>");
            for zone in 0..if layer == 0 { 25 } else { 1 } {
                xml.push_str(&format!("<Keygroup><Oscillators><SamplePlayer SamplePath=\"fixture-{layer}-{zone}.wav\"/></Oscillators></Keygroup>"));
            }
            xml.push_str("</Keygroups></Layer>");
        }
        xml.push_str("</Layers></Program>");
        let program = parse_program(&xml).unwrap();
        let mapping = Inspection::parsed(Stamp { epoch: 1, generation: 2, frame: 0 }, &program,
            Arc::new(serde_json::json!({"nodes":[],"preflight_admitted":true})));
        let mut ui = super::super::theme::ui();
        let mut state = State::default();
        let tick = |ui: &mut Ui, state: &mut State, input| {
            let el = inspection(ui, &mapping, state, false);
            ui.frame(el, Some(Size::new(900., 540.)), input, 1. / 60.).unwrap();
        };
        for _ in 0..3 { tick(&mut ui, &mut state, Input::default()); }
        let press = |ui: &mut Ui, state: &mut State, id: &str| {
            ui.focus(id);
            tick(ui, state, Input { keys: vec![KeyPress { key: Key::Enter, mods: Mods::default() }], ..Default::default() });
            for _ in 0..3 { tick(ui, state, Input::default()); }
        };
        press(&mut ui, &mut state, "uvi-mapping-zone-page-next");
        assert_eq!(state.zone_page, 1);
        assert_eq!(state.selected, Some(24), "page one selects global zone 24, not global zone zero");
        let label = ui.scene().unwrap().surface("uvi-mapping-selected-path").unwrap().semantics.as_ref()
            .and_then(|s| s.label.as_deref()).unwrap_or_default();
        assert_eq!(label, "fixture-0-24.wav");
        state.bind(&mapping.decoded(&std::collections::HashMap::new()));
        assert_eq!(state.selected, Some(24), "decoded metadata shares the initial zone owner");
        press(&mut ui, &mut state, "uvi-mapping-zone-page-next");
        assert_eq!(state.zone_page, 1, "last detail page stays bounded");
        press(&mut ui, &mut state, "uvi-mapping-zone-page-previous");
        assert_eq!(state.zone_page, 0);
        press(&mut ui, &mut state, "uvi-mapping-zone-page-next");
        press(&mut ui, &mut state, "uvi-mapping-layer-page-next");
        assert_eq!((state.layer_page, state.zone_page, state.layer), (1, 0, program.layers.last().copied()));
        press(&mut ui, &mut state, "uvi-mapping-layer-page-next");
        assert_eq!(state.layer_page, 1, "last layer page stays bounded");
        press(&mut ui, &mut state, "uvi-mapping-layer-page-previous");
        assert_eq!((state.layer_page, state.zone_page, state.layer), (0, 0, program.layers.first().copied()));
    }

    fn overlapping_mapping(stamp: Stamp) -> Inspection {
        let program = parse_program(r#"<Program><Layers><Layer><Keygroups>
          <Keygroup LowKey="60" HighKey="60"><Oscillators><SamplePlayer SamplePath="shared.wav" BaseNote="60"/></Oscillators></Keygroup>
          <Keygroup LowKey="60" HighKey="60" LowVelocity="65"><Oscillators><SamplePlayer SamplePath="shared.wav" BaseNote="61"/></Oscillators></Keygroup>
          <Keygroup LowKey="60" HighKey="60" LowVelocity="65"><Oscillators><SamplePlayer SamplePath="alternate.wav" BaseNote="60"/></Oscillators></Keygroup>
          <Keygroup LowKey="72" HighKey="72"><Oscillators><SamplePlayer SamplePath="other.wav"/></Oscillators></Keygroup>
        </Keygroups></Layer></Layers></Program>"#).unwrap();
        Inspection::parsed(stamp, &program, Arc::new(serde_json::json!({"nodes":[],"preflight_admitted":true})))
    }

    #[test]
    fn exact_probe_counts_references_and_paths_without_inventing_round_robin() {
        let mapping = overlapping_mapping(Stamp { epoch: 1, generation: 2, frame: 0 });
        let mut state = State::default();
        state.bind(&mapping);
        state.layer = mapping.layer_order.first().copied();
        state.query(&mapping);
        assert_eq!(state.matches.as_slice(), &[0, 1, 2]);
        assert_eq!((state.key_zones, state.sample_paths), (3, 2), "duplicate paths remain distinct authored zone references");
        let first = state.matches.clone();
        state.query(&mapping);
        assert!(Arc::ptr_eq(&first, &state.matches), "unchanged render queries share their cached index owner");
        for (velocity, expected) in [(64, vec![0]), (65, vec![0, 1, 2]), (127, vec![0, 1, 2]), (0, vec![])] {
            state.velocity = velocity; state.query(&mapping);
            assert_eq!(state.matches.as_slice(), expected.as_slice());
        }
        state.note = 61; state.query(&mapping);
        assert_eq!((state.key_zones, state.sample_paths, state.matches.len()), (0, 0, 0));
        let clone = state.clone();
        assert!(Arc::ptr_eq(&clone.matches, &state.matches));
    }

    #[test]
    fn selection_owner_and_stamp_reset_while_decoded_metadata_keeps_initial_zone_identity() {
        let stamp = Stamp { epoch: 1, generation: 2, frame: 0 };
        let mapping = overlapping_mapping(stamp);
        let mut state = State::default(); state.bind(&mapping);
        state.layer = mapping.layer_order.first().copied(); state.selected = Some(2); state.query(&mapping);
        let cached = state.matches.clone();
        let decoded = mapping.decoded(&std::collections::HashMap::new());
        state.bind(&decoded); state.query(&decoded);
        assert_eq!(state.selected, Some(2));
        assert!(Arc::ptr_eq(&cached, &state.matches));
        let other_owner = overlapping_mapping(stamp);
        state.bind(&other_owner);
        assert_eq!((state.selected, state.layer), (None, None), "even equal stamps cannot reinterpret a foreign index owner");
        state.layer = other_owner.layer_order.first().copied(); state.selected = Some(1);
        let mut changed_stamp = other_owner.decoded(&std::collections::HashMap::new());
        changed_stamp.stamp.generation += 1;
        state.bind(&changed_stamp);
        assert_eq!(state.selected, None, "an activation replacement retires editor selection");
    }

    #[test]
    fn map_probe_and_zone_rows_select_exact_global_source_with_paged_keyboard_access() {
        let mapping = overlapping_mapping(Stamp { epoch: 1, generation: 2, frame: 0 });
        let mut ui = super::super::theme::ui();
        let mut state = State::default();
        let tick = |ui: &mut Ui, state: &mut State, input| {
            let el = inspection(ui, &mapping, state, false);
            ui.frame(el, Some(Size::new(900., 700.)), input, 1. / 60.).unwrap();
        };
        let settle = |ui: &mut Ui, state: &mut State| { for _ in 0..3 { tick(ui, state, Input::default()); } };
        settle(&mut ui, &mut state);
        let label = |ui: &Ui, id: &str| ui.scene().unwrap().surface(id).unwrap().semantics.as_ref()
            .and_then(|s| s.label.as_deref()).unwrap_or_default().to_owned();
        let probe = |ui: &mut Ui, state: &mut State, note: u8, velocity: u8| {
            let frame = ui.scene().unwrap().surface(MAP_ID).unwrap().frame;
            let at = Point::new(frame.x + (f64::from(note) + 0.5) / 128. * frame.size.width,
                frame.y + (127.5 - f64::from(velocity)) / 128. * frame.size.height);
            tick(ui, state, Input { pointer: PointerInput { pos: Some(at), buttons: Buttons::PRIMARY, ..Default::default() }, ..Default::default() });
            settle(ui, state);
        };
        assert_eq!(state.selected, Some(0));
        probe(&mut ui, &mut state, 60, 100);
        assert!(state.filter);
        assert_eq!(state.matches.as_slice(), &[0, 1, 2]);
        assert!(label(&ui, "uvi-mapping-query-count").contains("3 zone references / 2 sample paths"));
        ui.focus("uvi-mapping-zone-2");
        tick(&mut ui, &mut state, Input { keys: vec![KeyPress { key: Key::Enter, mods: Mods::default() }], ..Default::default() });
        settle(&mut ui, &mut state);
        assert_eq!(state.selected, Some(2));
        assert_eq!(label(&ui, "uvi-mapping-selected-path"), "alternate.wav");
        assert!(label(&ui, "uvi-mapping-selected-range").contains("velocity 65–127"));
        assert!(label(&ui, "uvi-mapping-selected-resource").contains("unknown"));
        probe(&mut ui, &mut state, 60, 64);
        assert_eq!(state.matches.as_slice(), &[0]);
        assert_eq!(state.selected, Some(0));
        assert_eq!(label(&ui, "uvi-mapping-selected-path"), "shared.wav");
        probe(&mut ui, &mut state, 61, 64);
        assert!(state.matches.is_empty() && state.selected.is_none());
        assert!(ui.scene().unwrap().surface("uvi-mapping-selected-path").is_none());
    }
}
