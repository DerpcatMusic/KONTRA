use sampler_ir::{DspDisposition as D, DspSlotKind as K};
use serde_json::{Value, json};

pub(super) fn slots(instrument: &sampler_ir::Instrument) -> Value {
    let Some(slots) = &instrument.dsp_slots else { return json!({"complete":false}) };
    let rows: Vec<_> = slots.iter().map(|s| {
        let (status, reason) = match s.disposition {
            D::Implemented => ("implemented", None),
            D::Approximated(r) => ("approximated", Some(format!("{r:?}"))),
            D::Dropped(r) => ("dropped", Some(format!("{r:?}"))),
        };
        json!({"kind":match s.kind { K::Fx=>"fx", K::Filter=>"filter", K::Mod=>"mod" },
            "scope":s.scope,"slot":s.slot,"module":s.module,"enabled":s.enabled,
            "disposition":status,"reason":reason})
    }).collect();
    let mut counts = serde_json::Map::new();
    for (key, kind) in [("fx_slots_dropped", K::Fx), ("filter_slots_dropped", K::Filter), ("mod_slots_dropped", K::Mod)] {
        let dropped: Vec<_> = slots.iter().filter(|s| s.kind == kind && matches!(s.disposition, D::Dropped(_))).collect();
        counts.insert(key.into(), json!({"enabled":dropped.iter().filter(|s|s.enabled).count(),
            "bypassed":dropped.iter().filter(|s|!s.enabled).count()}));
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
        ir.dsp_slots = Some(vec![sampler_ir::DspSlot {kind:K::Fx,scope:"instrument send".into(),slot:7,
            module:"FX:63".into(),enabled:false,disposition:D::Dropped(sampler_ir::DspSlotReason::NotModeled)}]);
        let value = slots(&ir);
        assert_eq!(value["counts"]["fx_slots_dropped"], json!({"enabled":0,"bypassed":1}));
        assert_eq!(value["slots"][0]["reason"], "NotModeled");
    }
}
