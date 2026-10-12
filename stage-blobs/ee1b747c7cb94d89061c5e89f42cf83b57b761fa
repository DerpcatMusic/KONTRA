use sampler_ir::{DspDisposition as D, DspSlotKind as K};
use serde_json::{Value, json};

pub(super) fn slots(instrument: &sampler_ir::Instrument) -> Value {
    let Some(slots) = &instrument.dsp_slots else {
        return json!({"complete":false});
    };
    let rows: Vec<_> = slots
        .iter()
        .map(|s| {
            let (status, reason) = match s.disposition {
                D::Implemented => ("implemented", None),
                D::Approximated(r) => ("approximated", Some(format!("{r:?}"))),
                D::Dropped(r) => ("dropped", Some(format!("{r:?}"))),
            };
            json!({"kind":match s.kind { K::Fx=>"fx", K::Filter=>"filter", K::Mod=>"mod" },
            "scope":s.scope,"slot":s.slot,"module":s.module,"enabled":s.enabled,
            "disposition":status,"reason":reason,"targets":s.targets.iter().map(|t| {
                let (status,reason)=match t.disposition {D::Implemented=>("implemented",None),D::Approximated(r)=>("approximated",Some(format!("{r:?}"))),D::Dropped(r)=>("dropped",Some(format!("{r:?}")))};
                json!({"ordinal":t.ordinal,"parameter":t.parameter,"module_slot":t.module_slot,"module":t.module,"module_basis":if instrument.kontakt_objects.is_some(){"physical-insert-slot-lookup"}else{"native-xml-owner"},"enabled":t.enabled,"disposition":status,"reason":reason})
            }).collect::<Vec<_>>()})
        })
        .collect();
    let mut counts = serde_json::Map::new();
    for (key, kind) in [
        ("fx_slots_dropped", K::Fx),
        ("filter_slots_dropped", K::Filter),
        ("mod_slots_dropped", K::Mod),
    ] {
        let dropped: Vec<_> = slots
            .iter()
            .filter(|s| s.kind == kind && matches!(s.disposition, D::Dropped(_)))
            .collect();
        counts.insert(
            key.into(),
            json!({"enabled":dropped.iter().filter(|s|s.enabled).count(),
            "bypassed":dropped.iter().filter(|s|!s.enabled).count()}),
        );
    }
    json!({"complete":true,"slots":rows,"counts":counts})
}

pub(super) fn selections(records: Vec<sampler_core::SelectionRecord>) -> Vec<Value> {
    records.into_iter().map(|r| json!({"at":r.at,"event":r.event,"parent_event":r.parent_event,
        "key":r.key,"velocity":r.velocity,"trigger":format!("{:?}",r.trigger),"suppressed":r.suppressed,
        "started":r.candidates.iter().filter_map(|c|c.started.map(|s|json!({
            "region":c.region,"group":c.group,"source_zone":s.zone,"sample":s.sample,
            "frame":s.frame,"direction":format!("{:?}",s.direction),
            "loops":s.loops.iter().enumerate().filter_map(|(slot,l)|l.map(|l|json!({"slot":slot,"mode":if l.until_release {2}else{1},"start":l.start,"length":l.end-l.start,"count":l.count,"alternating":l.alternating,"crossfade":l.crossfade,"tuning":f64::from_bits(l.tuning_bits)}))).collect::<Vec<_>>()}))).collect::<Vec<_>>()})).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn absent_inventory_is_unknown_and_bypassed_drops_are_separate() {
        let mut ir = sampler_ir::Instrument::default();
        assert_eq!(slots(&ir)["complete"], false);
        ir.dsp_slots = Some(vec![sampler_ir::DspSlot {
            kind: K::Fx,
            scope: "instrument send".into(),
            slot: 7,
            module: "FX:63".into(),
            enabled: false,
            disposition: D::Dropped(sampler_ir::DspSlotReason::NotModeled),
            targets: Vec::new(),
        }]);
        let value = slots(&ir);
        assert_eq!(
            value["counts"]["fx_slots_dropped"],
            json!({"enabled":0,"bypassed":1})
        );
        assert_eq!(value["slots"][0]["reason"], "NotModeled");
    }
}

// Lex authored source only; do not use compiled callbacks or runtime selection.
fn selection_calls(source: &str) -> Vec<String> {
    let mut code = String::new();
    let mut quoted = false;
    let mut comment = 0usize;
    for c in source.chars() {
        match c {
            '"' if comment == 0 => {
                quoted = !quoted;
                code.push(' ');
            }
            '{' if !quoted => {
                comment += 1;
                code.push(' ');
            }
            '}' if !quoted && comment > 0 => comment -= 1,
            _ if !quoted && comment == 0 => code.push(c),
            _ => {}
        }
    }
    let selecting = [
        "allow_group",
        "disallow_group",
        "ignore_event",
        "play_note",
        "change_note",
        "change_vel",
        "set_event_par",
        "set_event_par_arr",
        "ENGINE_PAR_SAMPLER_MODE",
        "ENGINE_PAR_SAMPLE_START",
        "ENGINE_PAR_REVERSE",
    ];
    let mut calls: Vec<_> = code
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .filter(|t| selecting.contains(t))
        .map(str::to_owned)
        .collect();
    calls.sort();
    calls.dedup();
    calls
}

pub(super) fn native_family(
    instrument: &sampler_ir::Instrument,
    pick: Option<(u8, u8)>,
    switch: Option<u8>,
) -> Value {
    let mut calls = Vec::new();
    for b in &instrument.behaviors {
        if b.language == sampler_ir::Language::Lua {
            calls.push("lua-selection-requires-native-capture".into());
        } else {
            calls.extend(selection_calls(&b.source));
        }
    }
    calls.sort();
    calls.dedup();
    let audition = pick.map(|(key,velocity)|json!({"key":key,"velocity":velocity,"switch":switch,"cc":{"1":100,"11":127},"channel":0}));
    if let Some(raw) = &instrument.kontakt_objects {
        let groups:Vec<_>=raw.groups.iter().enumerate().map(|(id,g)|json!({"id":id,"muted":g.muted,"soloed":g.soloed,
            "reverse":g.reverse,"release":g.release_trigger,"channel":g.midi_channel,"criteria_mask":g.criteria_mask,
            "criteria":g.criteria.iter().map(|c|json!({"mode":c.mode,"next":c.next_criteria,"key_min":c.key_min,"key_max":c.key_max,
                "controller":c.controller,"cc_min":c.cc_min,"cc_max":c.cc_max,"cycle":c.cycle_class,"sequencer_only":c.sequencer_only})).collect::<Vec<_>>()})).collect();
        let zones:Vec<_>=raw.zones.iter().enumerate().filter(|(_,z)|pick.is_none_or(|(k,v)|
            (z.low_key..=z.high_key).contains(&i16::from(k)) && (z.low_velocity..=z.high_velocity).contains(&i16::from(v))))
            .map(|(id,z)|json!({"id":id+1,"group":z.group,"low_key":z.low_key,"high_key":z.high_key,
                "low_velocity":z.low_velocity,"high_velocity":z.high_velocity,"start":z.sample_start,"end":z.sample_end,
                "frames":z.num_frames,"mod_range":z.sample_start_mod_range,"start_modulated":instrument.native_start_mod_groups.as_ref().map(|ids|ids.contains(&z.group)),"loops":z.loops.iter().map(|l|json!({"slot":l.slot,"mode":l.mode,
                    "start":l.loop_start,"length":l.loop_length,"count":l.loop_count,"alternating":l.alternating_loop,
                    "crossfade":l.x_fade_length,"tuning":l.loop_tuning})).collect::<Vec<_>>()})).collect();
        let p = &raw.program;
        json!({"basis":"native-reader","format":"kontakt","script_driven":calls,"audition":audition,"groups":groups,"zones":zones,
            "program":{"low_key":p.low_key,"high_key":p.high_key,"low_velocity":p.low_velocity,"high_velocity":p.high_velocity,
                "default_switch":if p.default_key_switch>=0 {Some(p.default_key_switch)}else{None},"group_solo":p.group_solo}})
    } else if let Some(raw) = &instrument.native_family {
        let groups: Vec<_> = raw
            .zones
            .iter()
            .map(|z| {
                json!({"id":z.id,"muted":z.muted,"reverse":z.reverse,
            "release":false,"channel":-1,"criteria":[]})
            })
            .collect();
        let zones:Vec<_>=raw.zones.iter().filter(|z|pick.is_none_or(|(k,v)|(z.keys.low..=z.keys.high).contains(&k) && (z.velocities.low..=z.velocities.high).contains(&v)))
            .map(|z|json!({"id":z.id,"group":z.id,"low_key":z.keys.low,"high_key":z.keys.high,"low_velocity":z.velocities.low,
                "high_velocity":z.velocities.high,"start":z.start,"end":z.end,"frames":z.frames,"mod_range":0,
                "loops":z.loops.iter().map(|l|json!({"slot":l.slot,"mode":l.mode,"start":l.start,"length":l.length,"count":l.count,
                    "alternating":l.alternating,"crossfade":l.crossfade,"tuning":l.tuning})).collect::<Vec<_>>()})).collect();
        json!({"basis":"native-reader","format":"uvi","script_driven":calls,"unknown":raw.unknown,
            "audition":audition,"program":{},"groups":groups,"zones":zones})
    } else {
        json!({"basis":"native-reader","script_driven":calls,"unknown":"native-family-data-absent"})
    }
}

#[cfg(test)]
mod oracle_tests {
    use super::*;
    #[test]
    fn selection_guard_ignores_comments_and_strings_but_keeps_native_calls() {
        assert!(selection_calls("{ allow_group(1) } message(\"play_note\")").is_empty());
        assert_eq!(
            selection_calls(
                "on note\n disallow_group($ALL_GROUPS)\n play_note(60,64,48,0)\nend on"
            ),
            ["disallow_group", "play_note"]
        );
    }
}
