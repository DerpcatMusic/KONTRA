//! UVI-owned adapter to the shared, read-only mapping canvas.
use super::{instrument, theme::*};
#[cfg(feature = "plugin")]
use super::Cx;
use crate::uvi::{mapping::Inspection, program::NodeId};
use moose::mui::mui::prelude::*;

const LAYERS_SHOWN: usize = 128;
const ZONES_DRAWN: usize = 4096;
const DETAILS_SHOWN: usize = 24;

#[derive(Clone, Default)]
pub(super) struct State {
    pub layer: Option<NodeId>,
    pub layer_page: usize,
    pub zone_page: usize,
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
    let mapping = v.uvi_mapping_inspection(cx.p, slot, &cx.selection);
    let Some((mapping, ready)) = mapping else {
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

pub(super) fn inspection(ui: &mut Ui, mapping: &Inspection, state: &mut State, ready: bool) -> El {
    let admitted = mapping.report["preflight_admitted"].as_bool().unwrap_or(false);
    let status = format!("Parsed · {} · {} · {}",
        if admitted { "static check admitted" } else { "static check rejected" },
        if mapping.channels.is_some() { "initial resources decoded" } else { "initial resources not decoded" },
        if ready { "playback ready" } else { "playback not ready" });
    let old_page = state.layer_page;
    let layer_pager = pager(ui, "uvi-mapping-layer-page", mapping.layer_order.len().div_ceil(LAYERS_SHOWN), &mut state.layer_page);
    let layer_start = state.layer_page * LAYERS_SHOWN;
    let layer_end = (layer_start + LAYERS_SHOWN).min(mapping.layer_order.len());
    let shown = &mapping.layer_order[layer_start..layer_end];
    if old_page != state.layer_page || !state.layer.is_some_and(|layer| shown.contains(&layer)) {
        state.layer = shown.first().copied();
        state.zone_page = 0;
    }
    let mut layers = Vec::new();
    for &layer in shown {
        let label = format!("{} · {}", node_name(&mapping, layer), mapping.layers[&layer].len());
        let (hit, el) = action(ui, format!("uvi-mapping-layer-{layer}"), &label, state.layer == Some(layer));
        if hit && state.layer != Some(layer) { state.layer = Some(layer); state.zone_page = 0; }
        layers.push(el.lines(2).min_w(0).w(Len::Pct(100.)).shrink(0));
    }
    let indices = state.layer.and_then(|layer| mapping.layers.get(&layer)).map(Vec::as_slice).unwrap_or_default();
    let zone_pager = pager(ui, "uvi-mapping-zone-page", indices.len().div_ceil(DETAILS_SHOWN), &mut state.zone_page);
    let detail_start = state.zone_page * DETAILS_SHOWN;
    let detail_end = (detail_start + DETAILS_SHOWN).min(indices.len());
    // ponytail: cap draw/list work; full mapping remains in the immutable snapshot.
    let zones = indices.iter().take(ZONES_DRAWN).map(|&index| {
        let zone = &mapping.zones[index];
        (zone.low_key, zone.high_key, zone.low_velocity, zone.high_velocity, true)
    }).collect();
    let mut details = vec![caption(status).lines(3).w(Len::Pct(100.)).min_w(0).shrink(0).id("uvi-mapping-readiness"),
        caption("Read-only initial mapping. Scripts may change samples or ranges during playback; this view does not track those changes.")
            .fill(secondary()).lines(4).w(Len::Pct(100.)).min_w(0).shrink(0)];
    if indices.is_empty() {
        details.push(body("No initial sampled zones. Generators and samples created by scripts are not shown.").lines(3)
            .w(Len::Pct(100.)).min_w(0).shrink(0));
    } else {
        details.push(caption(format!("Details {}–{} of {} zones · canvas first {} zones", detail_start + 1, detail_end,
            indices.len(), indices.len().min(ZONES_DRAWN))).lines(2).w(Len::Pct(100.)).min_w(0).shrink(0));
    }
    for &index in &indices[detail_start..detail_end] {
        let zone = &mapping.zones[index];
        let channels = mapping.channels.as_ref().and_then(|channels| channels.get(&zone.sample_path))
            .map_or_else(|| "channels unknown".to_owned(), |channels| format!("{channels} channels decoded"));
        details.push(col![
            caption(format!("Keys {}–{} · velocity {}–{} · root {} · {channels}", note_name(zone.low_key),
                note_name(zone.high_key), zone.low_velocity, zone.high_velocity, note_name(zone.root_note))).lines(3)
                .w(Len::Pct(100.)).min_w(0).shrink(0),
            caption(zone.sample_path.clone()).fill(secondary()).lines(4).w(Len::Pct(100.)).min_w(0).shrink(0)
        ].align(Align::Start).gap(TIGHT).pad((0, SPACE)).w(Len::Pct(100.)).min_w(0).shrink(0));
    }
    for &(node, reason) in mapping.rejections.iter().take(6) {
        let node = &mapping.report["nodes"][node];
        details.push(caption(format!("Unsupported {}: {}", node["kind"].as_str().unwrap_or("node"),
            node["preflight_rejections"][reason].as_str().unwrap_or("unknown reason"))).fill(Role::Danger).lines(5)
            .w(Len::Pct(100.)).min_w(0).shrink(0));
    }
    if mapping.rejections.len() > 6 { details.push(caption(format!("{} more static rejections. See Info or Logs.", mapping.rejections.len() - 6))
        .lines(2).w(Len::Pct(100.)).min_w(0).shrink(0)); }
    row![
        col![col(layers).gap(1).flex(1).min_h(0).scroll().id("uvi-mapping-layers"), layer_pager]
            .gap(SPACE).w(SIDEBAR_MIN).shrink(0).min_h(0),
        col![instrument::mapping_grid(zones, "Initial UVI key and velocity mapping").min_h(CONTROL * 5.),
            row![caption(note_name(0)), spacer(), caption("Key × velocity"), spacer(), caption(note_name(127))].shrink(0),
            col(details).align(Align::Start).gap(SPACE).flex(1).min_w(0).min_h(0).scroll().id("uvi-mapping-details"), zone_pager]
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
}
