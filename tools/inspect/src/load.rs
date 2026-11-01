//! Inventory the v3 instrument model and the plan prepared from it.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use kontra_engine::{Callbacks, Engine, ir::Law};
use kontra_stream::{Store, StoreConfig};
use serde_json::{Value, json};

use super::RATE;

pub fn run(path: &Path) -> Result<(Value, String)> {
    let (instrument, law_applications) =
        kontra_law::collect(|| kontra_host::instrument::read(path));
    let instrument = instrument.map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
    // Prepare requests samples referenced by at least one non-purged group.
    let resident_samples: HashSet<usize> = instrument
        .groups
        .iter()
        .filter(|g| !g.purged)
        .flat_map(|g| g.zones.iter().map(|z| z.sample))
        .collect();
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
    let mut scripts: Vec<_> = instrument.scripts.iter().enumerate().map(|(slot, s)| json!({
        "original_slot": slot, "name": s.name, "dialect": format!("{:?}", s.dialect),
        "data": s.data.iter().map(|(name, bytes)| json!({"name": name, "bytes": bytes.len()})).collect::<Vec<_>>(),
        "runtime_slot": null, "compile_status": "unknown", "callbacks": null,
    })).collect();
    let parameter_laws: Vec<_> = instrument
        .slot_laws
        .iter()
        .map(|l| {
            json!({
                "rack":format!("{:?}", l.at), "slot":l.slot, "knob":format!("{:?}", l.knob),
                "field":format!("{:?}", l.field), "law":law(&l.law)
            })
        })
        .collect();
    let effects = json!({"inserts": format!("{:?}", instrument.inserts),
        "sends": format!("{:?}", instrument.sends), "main": format!("{:?}", instrument.main),
        "buses": format!("{:?}", instrument.buses), "slot_laws": format!("{:?}", instrument.slot_laws)});
    let mut store = Store::new(StoreConfig::DEFAULT).context("creating sample store")?;
    let loaded = kontra_host::instrument::build(instrument, RATE, &mut store);
    let original_slots: Vec<_> = loaded.persist.iter().map(|vars| vars.script).collect();
    let mapping = script_mapping(scripts.len(), &original_slots, loaded.plan.scripts.len());
    let mut callback_count = 0usize;
    for (original, row) in scripts.iter_mut().enumerate() {
        let runtime = mapping
            .as_ref()
            .and_then(|slots| slots.get(original))
            .copied()
            .flatten();
        let compiled = runtime.and_then(|slot| loaded.plan.scripts.get(slot));
        if let Some(fields) = row.as_object_mut() {
            fields.insert("runtime_slot".into(), json!(runtime));
            fields.insert(
                "compile_status".into(),
                json!(if compiled.is_some() {
                    "compiled"
                } else if mapping.is_some() {
                    "not_compiled"
                } else {
                    "mapping_unavailable"
                }),
            );
            if let Some(script) = compiled {
                let (entries, count) = callbacks(&script.callbacks);
                callback_count += count;
                fields.insert("callbacks".into(), entries);
                fields.insert(
                    "knob_laws".into(),
                    json!(script.knobs.iter().map(|(target, curve)|
                    json!({"target":format!("{target:?}"), "law":law(curve)})).collect::<Vec<_>>()),
                );
                fields.insert(
                    "reverb_laws".into(),
                    json!(
                        script
                            .reverb_laws
                            .iter()
                            .enumerate()
                            .map(|(index, curve)| json!({"index":index, "law":law(curve)}))
                            .collect::<Vec<_>>()
                    ),
                );
                fields.insert(
                    "callback_entry_kind".into(),
                    json!(if script.foreign.is_some() {
                        "foreign_dispatch"
                    } else {
                        "bytecode_offset"
                    }),
                );
            }
        }
    }
    let zones = loaded.plan.zones.len();
    let prepared: HashSet<_> = loaded
        .plan
        .zones
        .iter()
        .filter_map(|z| Some((usize::try_from(z.group).ok()?, usize::from(z.member))))
        .collect();
    let mut input_zones = 0usize;
    let mut unprepared_reasons: BTreeMap<&str, usize> = BTreeMap::new();
    for (group_id, group) in groups.iter_mut().enumerate() {
        if let Some(items) = group.get_mut("zones").and_then(Value::as_array_mut) {
            input_zones += items.len();
            for (member, zone) in items.iter_mut().enumerate() {
                if let Some(fields) = zone.as_object_mut() {
                    let present = (member < usize::from(u16::MAX))
                        .then(|| prepared.contains(&(group_id, member)));
                    let reason = match present {
                        Some(true) => None,
                        Some(false) => fields
                            .get("sample")
                            .and_then(Value::as_u64)
                            .and_then(|n| usize::try_from(n).ok())
                            .map(|sample| absent_reason(sample, samples.len(), &resident_samples))
                            .or(Some("unknown")),
                        None => Some("unknown"),
                    };
                    if let Some(reason) = reason {
                        *unprepared_reasons.entry(reason).or_default() += 1;
                    }
                    fields.insert("prepared".into(), json!(present));
                    fields.insert("unprepared_reason".into(), json!(reason));
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
    let library = RefCell::new(kontra_host::ui::Library::open(path));
    let asset_cache = RefCell::new(HashMap::<String, Option<usize>>::new());
    let metadata = |name: &str| library.borrow_mut().meta(name);
    let asset_size = |name: &str| {
        if let Some(cached) = asset_cache.borrow().get(name) {
            return *cached;
        }
        let size = library.borrow_mut().read(name).map(|bytes| bytes.len());
        asset_cache.borrow_mut().insert(name.to_owned(), size);
        size
    };
    let ui: Vec<_> = loaded.faces.iter().zip(&loaded.pages).enumerate().map(|(slot, (face, page))| {
        if let (Some(face), Ok(slot_id)) = (face, u8::try_from(slot)) {
            match kontra_host::ui::interface(face, slot_id, &engine, &metadata) {
                Ok(tree) => json!({"runtime_slot": slot, "original_slot": mapping.as_ref().and_then(|_| loaded.persist.get(slot)).map(|p| p.script), "widgets": tree.widgets.iter().map(|w| json!({"name": w.name, "source_id": w.source_id, "kind": format!("{:?}", w.kind), "hidden": w.hidden, "enabled": w.enabled, "text": w.text, "details": format!("{w:?}")})).collect::<Vec<_>>(), "assets": tree.assets.iter().map(|a| {
                    let size = asset_size(&a.path);
                    json!({"path": a.path, "kind": format!("{:?}", a.kind),
                        "found": size.map(|_| true), "bytes": size,
                        "read_status": if size.is_some() { "resolved" } else { "not_resolved" }})
                }).collect::<Vec<_>>(), "unsupported": tree.unsupported.iter().map(|u| format!("{u:?}")).collect::<Vec<_>>()}),
                Err(e) => json!({"runtime_slot": slot, "error": e.to_string()}),
            }
        } else if let Some(page) = page {
            let tree = kontra_host::ui::lua::tree(page, &metadata);
            json!({"runtime_slot": slot, "original_slot": mapping.as_ref().and_then(|_| loaded.persist.get(slot)).map(|p| p.script), "widgets": tree.widgets.iter().map(|w| json!({"name": w.name, "source_id": w.source_id, "kind": format!("{:?}", w.kind), "hidden": w.hidden, "enabled": w.enabled, "text": w.text, "details": format!("{w:?}")})).collect::<Vec<_>>(), "assets": tree.assets.iter().map(|a| {
                    let size = asset_size(&a.path);
                    json!({"path": a.path, "kind": format!("{:?}", a.kind),
                        "found": size.map(|_| true), "bytes": size,
                        "read_status": if size.is_some() { "resolved" } else { "not_resolved" }})
                }).collect::<Vec<_>>(), "unsupported": tree.unsupported.iter().map(|u| format!("{u:?}")).collect::<Vec<_>>()})
        } else { json!({"runtime_slot": slot, "error": "no UI model"}) }
    }).collect();
    let (resolved_assets, unresolved_assets) = ui
        .iter()
        .filter_map(|model| model.get("assets").and_then(Value::as_array))
        .flatten()
        .fold((0usize, 0usize), |(resolved, unresolved), asset| {
            if asset.get("read_status").and_then(Value::as_str) == Some("resolved") {
                (resolved + 1, unresolved)
            } else {
                (resolved, unresolved + 1)
            }
        });
    let callback_count = mapping.as_ref().map(|_| callback_count);
    let callback_summary =
        callback_count.map_or_else(|| "unavailable".to_owned(), |n| n.to_string());
    let mut unavailable = vec![
        "law application provenance outside instrumented Kontakt and UVI paths; UVI cross-node mapper inputs",
        "reason for unresolved UI assets",
    ];
    if mapping.is_none() {
        unavailable.extend([
            "original-to-runtime script slot mapping",
            "script callbacks",
        ]);
    }
    let unanswered = format!("{:?}", engine.unhandled_actions());
    let faults = format!("{:?}", engine.script_faults());
    let summary = format!(
        "{}\n  samples: {}; groups: {}; zones: {} translated / {} prepared; scripts: {}; UI models: {}\n  unprepared zones by reason: {unprepared_reasons:?}\n  diagnostics: {}; init unanswered: {}; script faults: {}\n  compiled callback/dispatch entries: {callback_summary}; referenced UI assets: {resolved_assets} resolved / {unresolved_assets} not resolved\n  recorded translator law applications: {}; other law provenance unavailable\n",
        loaded.name,
        samples.len(),
        groups.len(),
        input_zones,
        zones,
        scripts.len(),
        ui.len(),
        diagnostics.len(),
        unanswered,
        faults,
        law_applications.len()
    );
    Ok((
        json!({"schema_version": 1, "command": "load", "instrument": path,
        "engine": "v3", "result": {"name": loaded.name, "samples": samples, "groups": groups,
        "translated_zones": input_zones, "prepared_zones": zones, "unprepared_reasons": unprepared_reasons, "effects": effects, "slot_parameter_laws": parameter_laws, "scripts": scripts, "ui": ui,
        "diagnostics": diagnostics, "init_unanswered": unanswered, "script_faults": faults,
        "laws": null, "law_applications": law_applications.iter().map(|a| json!({"law_id":a.id.0, "location":a.location, "input":a.input, "output":a.output})).collect::<Vec<_>>(), "law_coverage":"partial: Kontakt group/zone gain-pan-tune, group envelopes/release counter, timed envelopes/LFO/flex gate/EQ, external volume/lag/EQ/LFO rate/shapers saved EQ/filters, compressor/limiter, time floor, stream and filter lookups; UVI keygroup/zone gain-pan-tune, amplitude/AHD envelopes, LFO shape, matrix cell, filter and key/velocity fades", "compiled_callback_count": callback_count, "script_mapping_available": mapping.is_some(), "unavailable": unavailable}}),
        summary,
    ))
}

/// The curve as retained in the IR; no translator provenance is inferred.
pub(crate) fn law(curve: &Law) -> Value {
    match curve {
        Law::Linear { lo, hi } => json!({"kind":"linear", "lo":lo, "hi":hi}),
        Law::Exp { lo, hi } => json!({"kind":"exp", "lo":lo, "hi":hi}),
        Law::ExpAffine {
            base,
            offset,
            factor,
            shift,
            scale,
        } => {
            json!({"kind":"exp_affine", "base":base, "offset":offset, "factor":factor, "shift":shift, "scale":scale})
        }
        Law::Exp2Index {
            origin,
            divisor,
            offset,
            scale,
        } => {
            json!({"kind":"exp2_index", "origin":origin, "divisor":divisor, "offset":offset, "scale":scale})
        }
        Law::PowIndex {
            base,
            origin,
            divisor,
            scale,
        } => {
            json!({"kind":"pow_index", "base":base, "origin":origin, "divisor":divisor, "scale":scale})
        }
        Law::Db { lo, hi } => json!({"kind":"db", "lo":lo, "hi":hi}),
        Law::Power { lo, hi, exponent } => {
            json!({"kind":"power", "lo":lo, "hi":hi, "exponent":exponent})
        }
        Law::IntegerPower { exponent, bipolar } => {
            json!({"kind":"integer_power", "exponent":exponent, "bipolar":bipolar})
        }
        Law::Sine { power } => json!({"kind":"sine", "power":power}),
        Law::Table(values) => json!({"kind":"table", "values":values.iter().collect::<Vec<_>>() }),
        Law::Breakpoints(points) => {
            json!({"kind":"breakpoints", "points":points.iter().map(|(x,y,curve)| json!({"x":x, "y":y, "curve":curve})).collect::<Vec<_>>() })
        }
        Law::Formula(id) => json!({"kind":"formula", "id":format!("{id:?}")}),
    }
}

/// Original-to-runtime mapping from the host's preserved original script IDs.
fn script_mapping(
    original_count: usize,
    original_slots: &[usize],
    compiled_count: usize,
) -> Option<Vec<Option<usize>>> {
    if original_slots.len() != compiled_count {
        return None;
    }
    let mut result = vec![None; original_count];
    for (runtime, &original) in original_slots.iter().enumerate() {
        let target = result.get_mut(original)?;
        if target.is_some() {
            return None;
        }
        *target = Some(runtime);
    }
    Some(result)
}

/// The compiled engine dispatch entries; null means no entry was compiled.
fn callbacks(c: &Callbacks) -> (Value, usize) {
    let entries = [
        c.init,
        c.note,
        c.release,
        c.controller,
        c.persistence_changed,
        c.listener,
        c.pgs_changed,
        c.ui_controls,
        c.async_complete,
        c.widget,
    ];
    let count = entries.iter().filter(|e| e.is_some()).count()
        + c.ui_control.iter().filter(|(_, e)| e.is_some()).count();
    (
        json!({"init":c.init,"note":c.note,"release":c.release,"controller":c.controller,
        "persistence_changed":c.persistence_changed,"listener":c.listener,"pgs_changed":c.pgs_changed,
        "ui_controls":c.ui_controls,"async_complete":c.async_complete,"widget_dispatch":c.widget,
        "ui_control":c.ui_control.iter().map(|(id,entry)| json!({"control_id":id,"entry":entry})).collect::<Vec<_>>()}),
        count,
    )
}

/// Mirrors the prepare path's sample admission, without attributing an I/O error
/// to one of its open/load stages when the plan does not retain that detail.
fn absent_reason(sample: usize, sample_count: usize, resident: &HashSet<usize>) -> &'static str {
    if sample >= sample_count {
        "invalid_sample_index"
    } else if !resident.contains(&sample) {
        "sample_only_referenced_by_purged_groups"
    } else {
        "resident_sample_unavailable"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kontra_engine::ir::FormulaId;

    #[test]
    fn law_shapes_preserve_parameters_and_points() {
        assert_eq!(
            law(&Law::IntegerPower {
                exponent: 3,
                bipolar: false
            }),
            json!({"kind":"integer_power", "exponent":3, "bipolar":false})
        );
        assert_eq!(
            law(&Law::ExpAffine {
                base: 10.0,
                offset: -0.5,
                factor: 3.0,
                shift: 0.0,
                scale: 1.0
            }),
            json!({"kind":"exp_affine", "base":10.0, "offset":-0.5, "factor":3.0, "shift":0.0, "scale":1.0})
        );
        assert_eq!(
            law(&Law::PowIndex {
                base: 2.0,
                origin: 69.0,
                divisor: 12.0,
                scale: 440.0
            }),
            json!({"kind":"pow_index", "base":2.0, "origin":69.0, "divisor":12.0, "scale":440.0})
        );
        assert_eq!(
            law(&Law::Formula(FormulaId::AttackShape)),
            json!({"kind":"formula", "id":"AttackShape"})
        );
        assert_eq!(
            law(&Law::Exp2Index {
                origin: 0.0,
                divisor: 60.0,
                offset: -20.0,
                scale: 1.0
            }),
            json!({"kind":"exp2_index", "origin":0.0, "divisor":60.0, "offset":-20.0, "scale":1.0})
        );
        assert_eq!(
            law(&Law::Sine { power: 2.0 }),
            json!({"kind":"sine", "power":2.0})
        );
        assert_eq!(
            law(&Law::Power {
                lo: 1.0,
                hi: 4.0,
                exponent: 0.5
            }),
            json!({"kind":"power", "lo":1.0, "hi":4.0, "exponent":0.5})
        );
        assert_eq!(
            law(&Law::Table(vec![0.0, 2.0].into())),
            json!({"kind":"table", "values":[0.0, 2.0]})
        );
        assert_eq!(
            law(&Law::Breakpoints(vec![(0.0, 1.0, 0.25)].into())),
            json!({"kind":"breakpoints", "points":[{"x":0.0,"y":1.0,"curve":0.25}]})
        );
    }

    #[test]
    fn script_mapping_preserves_gaps_and_rejects_inconsistent_host_data() {
        assert_eq!(
            script_mapping(4, &[0, 2, 3], 3),
            Some(vec![Some(0), None, Some(1), Some(2)])
        );
        assert_eq!(script_mapping(2, &[0, 0], 2), None);
        assert_eq!(script_mapping(2, &[3], 1), None);
        assert_eq!(script_mapping(2, &[0], 2), None);
    }

    #[test]
    fn callbacks_distinguish_absent_entries_from_zero_offset() {
        let c = Callbacks {
            init: Some(0),
            controller: Some(9),
            ui_control: vec![(17, None), (18, Some(20))],
            ..Callbacks::default()
        };
        let (value, count) = callbacks(&c);
        assert_eq!(count, 3);
        assert_eq!(value["init"], json!(0));
        assert!(value["note"].is_null());
        assert!(value["ui_control"][0]["entry"].is_null());
        assert_eq!(value["ui_control"][1]["entry"], json!(20));
    }

    #[test]
    fn absent_zone_reasons_follow_prepare_sample_admission() {
        let resident = HashSet::from([1]);
        assert_eq!(
            absent_reason(0, 2, &resident),
            "sample_only_referenced_by_purged_groups"
        );
        assert_eq!(
            absent_reason(1, 2, &resident),
            "resident_sample_unavailable"
        );
        assert_eq!(absent_reason(2, 2, &resident), "invalid_sample_index");
    }
}
