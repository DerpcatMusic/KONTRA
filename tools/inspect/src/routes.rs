//! Machine-readable modulation routes from the translated model.
use kontra_engine::ir::{EnvStage, LfoParam, ModRoute, ParamAddr, RouteLaw, Source, TimeScale};
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
        Source::ZoneEnvelope(id) => json!({"type": "zone_envelope", "id": id}),
        Source::Lfo(id) => json!({"type": "lfo", "id": id}),
        Source::Script(id) => json!({"type": "script", "id": id}),
        Source::Macro(n) => json!({"type": "macro", "id": n}),
        other => json!({"type": format!("{other:?}")}),
    }
}

fn time_scale(scale: TimeScale) -> Value {
    match scale {
        TimeScale::Linear { power } => json!({"type":"linear", "power":power}),
        TimeScale::LogKnob { floor_ms, power } => {
            json!({"type":"log_knob", "floor_ms":floor_ms, "power":power})
        }
    }
}

fn law(law: RouteLaw) -> Value {
    match law {
        RouteLaw::Linear => json!({"type":"linear"}),
        RouteLaw::Unipolar => json!({"type":"unipolar"}),
        RouteLaw::Gain(curve) => json!({"type":"gain", "curve":format!("{curve:?}")}),
        RouteLaw::Time(scale) => json!({"type":"time", "scale":time_scale(scale)}),
        RouteLaw::Range([lo, hi]) => json!({"type":"range", "lo":lo, "hi":hi}),
        RouteLaw::Clamp(limit) => json!({"type":"clamp", "limit":limit}),
    }
}

fn target(addr: ParamAddr) -> Value {
    let stage = |s: EnvStage| match s {
        EnvStage::Attack => "attack",
        EnvStage::Hold => "hold",
        EnvStage::Decay => "decay",
        EnvStage::Sustain => "sustain",
        EnvStage::Release => "release",
    };
    let param = |p: LfoParam| match p {
        LfoParam::Phase => "phase",
        LfoParam::Rate => "rate",
        LfoParam::Hz => "hz",
        LfoParam::Division => "division",
    };
    match addr {
        ParamAddr::AmpEnvelope => json!({"type":"envelope"}),
        ParamAddr::Volume => json!({"type":"volume"}),
        ParamAddr::Pitch => json!({"type":"pitch"}),
        ParamAddr::Pan => json!({"type":"pan"}),
        ParamAddr::SampleStart => json!({"type":"sample_start"}),
        ParamAddr::LoopShift { length } => json!({"type":"loop_shift", "length":length}),
        ParamAddr::EnvelopeStage { envelope, stage: s } => {
            json!({"type":stage(s), "envelope":envelope})
        }
        ParamAddr::LfoParam { lfo, param: p } => {
            json!({"type":format!("lfo_{}", param(p)), "lfo":lfo})
        }
        ParamAddr::FilterCutoff { insert } => json!({"type":"cutoff", "insert":insert}),
        ParamAddr::FilterResonance { insert } => json!({"type":"resonance", "insert":insert}),
        ParamAddr::EqBandGain { insert, band } => {
            json!({"type":"eq_gain", "insert":insert, "band":band})
        }
        ParamAddr::EffectParam { insert, param } => {
            json!({"type":"effect_param", "insert":insert, "param":format!("{param:?}")})
        }
        ParamAddr::Wavetable(value) => json!({"type":"wavetable", "value":format!("{value:?}")}),
        ParamAddr::ZoneMix { zone, row, side } => {
            json!({"type":"mix", "zone":zone, "row":row, "side":side})
        }
        ParamAddr::RouteIntensity { of } => json!({"type":"intensity", "of":source(of)}),
        ParamAddr::BusSend { send } => json!({"type":"send", "send":send}),
    }
}

pub fn route(r: ModRoute) -> Value {
    json!({
        "target": target(r.target), "source": r.driver().map(source),
        "depth": r.depth, "span": r.span, "law": law(r.law), "smooth_ms": r.smooth_ms,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controller_to_eq_retains_its_target_and_units() {
        let out = route(
            ModRoute::new(
                Source::Controller(1),
                ParamAddr::EqBandGain { insert: 2, band: 1 },
                3.0,
            )
            .span(12.0)
            .law(RouteLaw::Clamp(12.0)),
        );
        assert_eq!(out["source"]["cc"], 1);
        assert_eq!(
            out["target"],
            json!({"type":"eq_gain", "insert":2, "band":1})
        );
        assert_eq!(out["depth"], 3.0);
        assert_eq!(out["law"], json!({"type":"clamp", "limit":12.0}));
    }

    #[test]
    fn timing_routes_report_scale_and_envelope() {
        let stage = |stage, envelope| ParamAddr::EnvelopeStage { envelope, stage };
        let attack = route(
            ModRoute::new(Source::Velocity, stage(EnvStage::Attack, None), 0.5)
                .law(RouteLaw::Time(TimeScale::Linear { power: 3 })),
        );
        let release = route(
            ModRoute::new(
                Source::ReleaseVelocity,
                stage(EnvStage::Release, Some(2)),
                1.0,
            )
            .law(RouteLaw::Time(TimeScale::LogKnob {
                floor_ms: 2.0,
                power: 1,
            })),
        );
        assert_eq!(attack["law"]["scale"], json!({"type":"linear", "power":3}));
        assert_eq!(
            release["law"]["scale"],
            json!({"type":"log_knob", "floor_ms":2.0, "power":1})
        );
        assert_eq!(release["target"]["envelope"], 2);
        assert_eq!(route(ModRoute::AMP_ENVELOPE)["source"], Value::Null);
    }
}
