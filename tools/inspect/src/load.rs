//! Inventory the v3 instrument model and the plan prepared from it.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use kontra_engine::Engine;
use kontra_stream::{Store, StoreConfig};
use serde_json::{Value, json};

use super::RATE;

pub fn run(path: &Path) -> Result<(Value, String)> {
    let instrument = kontra_host::instrument::read(path)
        .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
    let samples: Vec<_> = instrument
        .samples
        .iter()
        .enumerate()
        .map(|(id, s)| {
            json!({
                "id": id, "name": s.name, "file": s.file, "offset": s.offset, "length": s.len,
                "channels": s.channels, "frames": s.frames, "rate": s.rate,
            })
        })
        .collect();
    let mut groups: Vec<_> = instrument
        .groups
        .iter()
        .enumerate()
        .map(|(id, g)| {
            json!({
                "id": id, "name": g.name, "trigger": format!("{:?}", g.trigger),
                "start_rules": format!("{:?}", g.start), "purged": g.purged,
                "stream": format!("{:?}", g.stream), "envelope": format!("{:?}", g.envelope),
                "zones": g.zones.iter().map(|z| json!({
                    "sample": z.sample, "keys": [z.keys.start(), z.keys.end()],
                    "velocities": [z.velocities.start(), z.velocities.end()], "root": z.root,
                    "start": z.start, "end": z.end, "start_mod": z.start_mod,
                    "tune": z.tune, "volume_db": z.volume_db, "pan": z.pan,
                    "loops": format!("{:?}", z.loops), "reverse": z.reverse, "slot": z.slot,
                })).collect::<Vec<_>>(),
                "modulators": g.mods.iter().map(|m| json!({
                    "name": m.name, "slot": m.slot, "target": m.target,
                    "bypass": m.bypass, "route": super::routes::route(m.kind),
                })).collect::<Vec<_>>(),
                "route_names": g.routes.iter().map(|r| json!({"slot":r.slot, "name":r.name, "names":r.names})).collect::<Vec<_>>(),
                "shapers": g.shapers.iter().map(|s| json!({"source":super::routes::source(s.source), "table":s.table})).collect::<Vec<_>>(),
                "blends": format!("{:?}", g.blends), "effects": format!("{:?}", g.inserts),
            })
        })
        .collect();
    let scripts: Vec<_> = instrument.scripts.iter().enumerate().map(|(slot, s)| json!({
        "original_slot": slot, "name": s.name, "dialect": format!("{:?}", s.dialect),
        "data": s.data.iter().map(|(name, bytes)| json!({"name": name, "bytes": bytes.len()})).collect::<Vec<_>>(),
        "callbacks": null,
    })).collect();
    let effects = json!({"inserts": format!("{:?}", instrument.inserts),
        "sends": format!("{:?}", instrument.sends), "main": format!("{:?}", instrument.main),
        "buses": format!("{:?}", instrument.buses), "slot_laws": format!("{:?}", instrument.slot_laws)});
    let mut store = Store::new(StoreConfig::DEFAULT).context("creating sample store")?;
    let loaded = kontra_host::instrument::build(instrument, RATE, &mut store);
    let zones = loaded.plan.zones.len();
    let prepared: HashSet<_> = loaded
        .plan
        .zones
        .iter()
        .filter_map(|z| Some((usize::try_from(z.group).ok()?, usize::from(z.member))))
        .collect();
    let mut input_zones = 0usize;
    for (group_id, group) in groups.iter_mut().enumerate() {
        if let Some(items) = group.get_mut("zones").and_then(Value::as_array_mut) {
            input_zones += items.len();
            for (member, zone) in items.iter_mut().enumerate() {
                if let Some(fields) = zone.as_object_mut() {
                    let present = (member < usize::from(u16::MAX))
                        .then(|| prepared.contains(&(group_id, member)));
                    fields.insert("prepared".into(), json!(present));
                }
            }
        }
    }
    let diagnostics: Vec<_> = loaded
        .diagnostics
        .iter()
        .map(|d| {
            json!({
                "impact": format!("{:?}", d.impact), "feature": d.feature, "detail": d.detail,
            })
        })
        .collect();
    let plan = Arc::new(loaded.plan);
    let engine = Engine::new(Arc::clone(&plan), store.client(), RATE);
    let ui: Vec<_> = loaded.faces.iter().zip(&loaded.pages).enumerate().map(|(slot, (face, page))| {
        if let (Some(face), Ok(slot_id)) = (face, u8::try_from(slot)) {
            match kontra_host::ui::interface(face, slot_id, &engine, &|_| None) {
                Ok(tree) => json!({"runtime_slot": slot, "widgets": tree.widgets.iter().map(|w| json!({"name": w.name, "source_id": w.source_id, "kind": format!("{:?}", w.kind), "hidden": w.hidden, "enabled": w.enabled, "text": w.text, "details": format!("{w:?}")})).collect::<Vec<_>>(), "assets": tree.assets.iter().map(|a| json!({"path": a.path, "kind": format!("{:?}", a.kind), "found": null})).collect::<Vec<_>>(), "unsupported": tree.unsupported.iter().map(|u| format!("{u:?}")).collect::<Vec<_>>()}),
                Err(e) => json!({"runtime_slot": slot, "error": e.to_string()}),
            }
        } else if let Some(page) = page {
            let tree = kontra_host::ui::lua::tree(page, &|_| None);
            json!({"runtime_slot": slot, "widgets": tree.widgets.iter().map(|w| json!({"name": w.name, "source_id": w.source_id, "kind": format!("{:?}", w.kind), "hidden": w.hidden, "enabled": w.enabled, "text": w.text, "details": format!("{w:?}")})).collect::<Vec<_>>(), "assets": tree.assets.iter().map(|a| json!({"path": a.path, "kind": format!("{:?}", a.kind), "found": null})).collect::<Vec<_>>(), "unsupported": tree.unsupported.iter().map(|u| format!("{u:?}")).collect::<Vec<_>>()})
        } else { json!({"runtime_slot": slot, "error": "no UI model"}) }
    }).collect();
    let unanswered = format!("{:?}", engine.unhandled_actions());
    let faults = format!("{:?}", engine.script_faults());
    let summary = format!(
        "{}\n  samples: {}; groups: {}; zones: {} translated / {} prepared; scripts: {}; UI models: {}\n  diagnostics: {}; init unanswered: {}; script faults: {}\n  law IDs and callbacks: unavailable (not recorded by the v3 IR)\n",
        loaded.name,
        samples.len(),
        groups.len(),
        input_zones,
        zones,
        scripts.len(),
        ui.len(),
        diagnostics.len(),
        unanswered,
        faults
    );
    Ok((
        json!({"schema_version": 1, "command": "load", "instrument": path,
        "engine": "v3", "result": {"name": loaded.name, "samples": samples, "groups": groups,
        "translated_zones": input_zones, "prepared_zones": zones, "effects": effects, "scripts": scripts, "ui": ui,
        "diagnostics": diagnostics, "init_unanswered": unanswered, "script_faults": faults,
        "laws": null, "unavailable": ["translator law IDs and provenance", "script callbacks", "original-to-runtime script slot mapping", "asset existence"]}}),
        summary,
    ))
}
