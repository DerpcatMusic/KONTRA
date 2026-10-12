//! Off-audio init readback and bounded accepted-request UI projection.
use crate::model::{Model, Request, Value};
use sampler_core::waveform::{Property, attachment_key, initial, source_key, symbol_key};

pub(crate) fn read(wave: &sampler_ui_ir::Waveform, p: Property, index: i32) -> i32 {
    match p {
        Property::Cursor => wave.cursor_us as i32,
        Property::Flags => wave.flags as i32,
        Property::MidiStart => i32::from(wave.midi_start_note),
        Property::Highlight => wave.highlighted.map_or(-1, |i| i as i32),
        Property::Table => usize::try_from(index).ok().and_then(|i| wave.table.get(i)).copied().unwrap_or(0),
    }
}

/// A model is a final-state mirror, not a runtime event log. Attachment resets
/// only this widget's waveform requests. Property replacements cannot move
/// before its last attachment. Other service requests are never removed.
pub(crate) fn project(model: &mut Model, name: &str, id: i32, request: Request) -> bool {
    let owns = |r: &Request| matches!(r.args.first(), Some(Value::Int(ui)) if *ui == id)
        || matches!(r.args.first(), Some(Value::Text(var)) if var == name);
    if request.command == "attach_zone" {
        model.requests.retain(|r| !(owns(r) && matches!(r.command, "attach_zone" | "set_ui_wf_property")));
    } else if let Some(existing) = model.requests.iter_mut().rev().find(|r| {
        owns(r) && r.command == request.command && r.args.get(1..3) == request.args.get(1..3)
    }) {
        if *existing == request { return false; }
        *existing = request;
        return true;
    }
    model.requests.push(request);
    true
}

/// Seed only declared waveform IDs and the existing physical-source environment.
/// Sparse Store annotations reuse the existing global runtime headroom; the
/// 65536 decoder index bound is not an admission capacity guarantee.
pub(crate) fn seed(
    hir: &crate::hir::Hir,
    init: &crate::eval::Initial,
    environment: &crate::Environment,
    store: &mut Vec<([i32; 4], i64)>,
) -> Result<(), sampler_core::Error> {
    if !hir.uis.iter().any(|ui| ui.kind == crate::model::WidgetKind::Waveform && !ui.unresolved) { return Ok(()); }
    for (index, ui) in hir.uis.iter().enumerate() {
        if ui.kind != crate::model::WidgetKind::Waveform || ui.unresolved { continue; }
        let id = crate::builtins::FIRST_UI_ID + index as i32;
        let name = &hir.vars[ui.var.0 as usize].name;
        let wave = crate::ui::waveform_requests(&init.model, id, name);
        // Cached requests must still match their captured physical-source domain.
        if wave.as_ref().is_some_and(|w| w.zone <= 0 || !environment.zones.contains_key(&(w.zone as u32))) {
            return Err(sampler_core::Error::InvalidInput);
        }
        for (key, default) in initial(id) {
            let value = wave.as_ref().map_or(default, |w| {
                if key == attachment_key(id) { i64::from(w.zone) }
                else {
                    let p = match key[1] {
                        1 => Property::Cursor,
                        2 => Property::Flags,
                        3 => Property::MidiStart,
                        _ => Property::Highlight,
                    };
                    i64::from(read(w, p, 0))
                }
            });
            store.push((key, value));
        }
        if let Some(w) = wave {
            for (index, value) in w.table.into_iter().enumerate().filter(|(_, value)| *value != 0) {
                store.push((Property::Table.key(id, index as i32), i64::from(value)));
            }
        }
    }
    for &zone in environment.zones.keys() {
        if let Ok(zone) = i32::try_from(zone) {
            if zone > 0 { store.push((source_key(zone), 1)); }
        }
    }
    for (index, name) in hir.symbols.iter().enumerate() {
        if let Some(p) = Property::from_name(name) {
            store.push((symbol_key(crate::hir::OPAQUE_BASE + index as i32), p as i64));
        }
    }
    Ok(())
}
