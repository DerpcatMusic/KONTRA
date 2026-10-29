//! Machine-readable modulation routes from the translated model.
use kontra_engine::ir::{ModKind, Source};
use serde_json::{Value, json};

pub fn source(s: Source) -> Value {
    match s {
        Source::Controller(cc) => json!({"type": "controller", "cc": cc}),
        Source::ControllerFrom { cc, start } => {
            json!({"type": "controller", "cc": cc, "start": start})
        }
        Source::Shaped(id) => json!({"type": "shaped", "id": id}),
        Source::Blend(id) => json!({"type": "blend", "id": id}),
        Source::Envelope(id) => json!({"type": "envelope", "id": id}),
        Source::Lfo(id) => json!({"type": "lfo", "id": id}),
        Source::Script(id) => json!({"type": "script", "id": id}),
        other => json!({"type": format!("{other:?}")}),
    }
}

pub fn route(kind: ModKind) -> Value {
    match kind {
        ModKind::Envelope => json!({"target": "envelope", "source": null}),
        ModKind::Volume {
            source: s,
            intensity,
            curve,
            tau_ms,
        } => {
            json!({"target":"volume", "source":source(s), "intensity":intensity, "curve":format!("{curve:?}"), "tau_ms":tau_ms})
        }
        ModKind::Pitch {
            source: s,
            semitones,
            depth,
            tau_ms,
        } => {
            json!({"target":"pitch", "source":source(s), "semitones":semitones, "depth":depth, "tau_ms":tau_ms})
        }
        ModKind::Pan { source: s, amount } => {
            json!({"target":"pan", "source":source(s), "amount":amount})
        }
        ModKind::Cutoff {
            source: s,
            insert,
            depth,
        } => json!({"target":"cutoff", "source":source(s), "insert":insert, "depth":depth}),
        ModKind::Resonance {
            source: s,
            insert,
            depth,
        } => json!({"target":"resonance", "source":source(s), "insert":insert, "depth":depth}),
        ModKind::EqGain {
            source: s,
            insert,
            band,
            depth_db,
            limit_db,
            span_db,
        } => {
            json!({"target":"eq_gain", "source":source(s), "insert":insert, "band":band, "depth_db":depth_db, "limit_db":limit_db, "span_db":span_db})
        }
        ModKind::SampleStart { source: s, depth } => {
            json!({"target":"sample_start", "source":source(s), "depth":depth})
        }
        ModKind::Attack {
            source: s,
            intensity,
            floor_ms,
        } => {
            json!({"target":"attack", "source":source(s), "intensity":intensity, "floor_ms":floor_ms})
        }
        ModKind::Release {
            source: s,
            intensity,
            floor_ms,
        } => {
            json!({"target":"release", "source":source(s), "intensity":intensity, "floor_ms":floor_ms})
        }
        ModKind::LfoPhase {
            source: s,
            lfo,
            depth,
        } => json!({"target":"lfo_phase", "source":source(s), "lfo":lfo, "depth":depth}),
        ModKind::LfoRate {
            source: s,
            lfo,
            depth,
            octaves,
            range_hz,
        } => {
            json!({"target":"lfo_rate", "source":source(s), "lfo":lfo, "depth":depth, "octaves":octaves, "range_hz":range_hz})
        }
        ModKind::LfoHz {
            source: s,
            lfo,
            depth,
            range_hz,
        } => {
            json!({"target":"lfo_hz", "source":source(s), "lfo":lfo, "depth":depth, "range_hz":range_hz})
        }
        ModKind::Intensity {
            source: s,
            of,
            depth,
        } => json!({"target":"intensity", "source":source(s), "of":source(of), "depth":depth}),
        ModKind::LoopShift {
            source: s,
            length,
            depth,
        } => json!({"target":"loop_shift", "source":source(s), "length":length, "depth":depth}),
        ModKind::Mix {
            source: s,
            zone,
            row,
            side,
        } => json!({"target":"mix", "source":source(s), "zone":zone, "row":row, "side":side}),
        ModKind::EffectParam {
            source: s,
            insert,
            param,
            depth,
            span,
            unipolar,
        } => {
            json!({"target":"effect_param", "source":source(s), "insert":insert, "param":format!("{param:?}"), "depth":depth, "span":span, "unipolar":unipolar})
        }
        ModKind::Send { source: s, send } => {
            json!({"target":"send", "source":source(s), "send":send})
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controller_to_eq_retains_its_target_and_units() {
        let out = route(ModKind::EqGain {
            source: Source::Controller(1),
            insert: 2,
            band: 1,
            depth_db: 3.0,
            limit_db: 12.0,
            span_db: 12.0,
        });
        assert_eq!(out["source"]["cc"], 1);
        assert_eq!(out["target"], "eq_gain");
        assert_eq!(out["insert"], 2);
        assert_eq!(out["band"], 1);
        assert_eq!(out["depth_db"], 3.0);
    }
}

/// Concise controller-driven contribution ranges from exact engine records.
pub fn summary(events: &[Value]) -> String {
    use std::collections::BTreeMap;
    let mut groups: BTreeMap<(u64, String, String), (usize, f64, f64)> = BTreeMap::new();
    for event in events {
        if event.get("controller_can_affect").and_then(Value::as_bool) != Some(true) {
            continue;
        }
        let (Some(cc), Some(amount), Some(unit)) = (
            event.get("controller").and_then(Value::as_u64),
            event.get("contribution").and_then(Value::as_f64),
            event.get("unit").and_then(Value::as_str),
        ) else {
            continue;
        };
        let name = event
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("slot {} target {}", event["mod_slot"], event["target"]));
        let entry = groups
            .entry((cc, name, unit.to_owned()))
            .or_insert((0, amount, amount));
        entry.0 += 1;
        entry.1 = entry.1.min(amount);
        entry.2 = entry.2.max(amount);
    }
    let mut items: Vec<_> = groups.into_iter().collect();
    items.sort_by(|a, b| b.1.0.cmp(&a.1.0).then(a.0.cmp(&b.0)));
    let mut out = String::new();
    for ((cc, name, unit), (count, min, max)) in items.iter().take(8) {
        out.push_str(&format!(
            "  CC{cc} {name}: {min:.4}..{max:.4} {unit} ({count} evaluations)\n"
        ));
    }
    if items.len() > 8 {
        out.push_str(&format!(
            "  {} more controller paths in JSON\n",
            items.len() - 8
        ));
    }
    out
}
