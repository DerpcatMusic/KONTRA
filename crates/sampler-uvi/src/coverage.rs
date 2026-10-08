//! Diagnostic inventory from authored XML and the production translation's node map.
use super::{Translation, modulation::source_node};
use roxmltree::Node;
use sampler_ir::{DspDisposition as D, DspSlot as Slot, DspSlotKind as K, DspSlotReason as R};
use std::collections::{HashMap, HashSet};

pub(super) fn slots(out: &Translation, program: Node) -> Vec<Slot> {
    let mut references = HashMap::<_, bool>::new();
    let used: HashSet<_> = out.used.iter().copied().collect();
    for connection in program.descendants().filter(|n| n.has_tag_name("SignalConnection")) {
        if let (Some(owner), Some(source)) = (connection.parent(), connection.attribute("Source")) {
            if let Some(node) = source_node(owner, source) {
                let failed = out.dropped_connections.contains(&connection.id());
                *references.entry(node.id()).or_default() |= failed;
            }
        }
    }
    let mut slots = Vec::new();
    for node in program.descendants().filter(|n| n.is_element()) {
        let Some(parent) = node.parent() else { continue };
        let kind = node.tag_name().name();
        let is_mod = parent.has_tag_name("ControlSignalSources");
        if !is_mod && !parent.has_tag_name("Inserts") { continue }
        let enabled = !node.ancestors().any(|n| n.attribute("Bypass").is_some_and(|v| v.parse::<f64>().is_ok_and(|b| b != 0.0) || v == "true"));
        let disposition = if !enabled { D::Dropped(R::SavedBypassNotInstantiated) }
            else if is_mod {
                if references.get(&node.id()) == Some(&true) { D::Dropped(R::TargetsDropped) }
                else if !references.contains_key(&node.id()) { D::Implemented }
                else { D::Approximated(R::NativeLawUnverified) }
            } else if !used.contains(&node.id()) { D::Dropped(R::NotModeled) }
            else { D::Approximated(R::NativeLawUnverified) };
        let scope = node.ancestors().skip(2).find(|n| n.is_element()).map(|n| {
            format!("{}:{}", n.tag_name().name(), n.id().get_usize())
        }).unwrap_or_default();
        let slot = parent.children().filter(|n| n.is_element()).position(|n| n == node).unwrap();
        slots.push(Slot { kind: if is_mod { K::Mod } else if matches!(kind,
            "OnePole" | "ThreeBandShelves" | "CombFilter" | "XpanderFilter" | "MS20" | "DigitalEq") { K::Filter } else { K::Fx },
            scope, slot, module: kind.into(), enabled, disposition });
    }
    for node in program.descendants().filter(|n| n.has_tag_name("SignalConnection")) {
        let Some(source) = node.attribute("Source") else { continue };
        if node.parent().and_then(|p| source_node(p, source)).is_some() { continue }
        let enabled = !node.ancestors().any(|n| n.attribute("Bypass").is_some_and(|v| v.parse::<f64>().is_ok_and(|b| b != 0.0) || v == "true"));
        slots.push(Slot { kind: K::Mod, scope: format!("connection:{}", node.id().get_usize()), slot: 0,
            module: if source.starts_with('@') { source.into() } else { "unresolved-source".into() }, enabled,
            disposition: if !enabled { D::Dropped(R::SavedBypassNotInstantiated) }
                else if out.dropped_connections.contains(&node.id()) { D::Dropped(R::TargetsDropped) }
                else { D::Approximated(R::NativeLawUnverified) } });
    }
    slots
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authored_bypass_and_unsupported_slots_are_retained() {
        let text = r#"<Program><Inserts><Gain Volume="1"/><SparkVerb/><Gain Bypass="1"/></Inserts><ControlSignalSources><ConstantModulation Name="unused"/></ControlSignalSources></Program>"#;
        let (ir, ..) = super::super::translate_full(text, super::super::Source::Bank).unwrap();
        let rows = ir.dsp_slots.unwrap();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[1].disposition, D::Dropped(R::NotModeled));
        assert_eq!(rows[2].slot, 2);
        assert!(!rows[2].enabled);
        assert_eq!(rows[3].disposition, D::Implemented);
    }
}
