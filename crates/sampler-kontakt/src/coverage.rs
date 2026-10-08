//! Authored slots joined to actual translation outcomes. No payloads are exported.
use ni_file::kontakt::objects::*;
use std::collections::{HashMap, HashSet};

struct Outcomes<'a> {
    failures: HashMap<&'a str, Vec<&'a str>>,
    modulators: HashSet<(usize, usize, bool)>,
    dynamic: bool,
}
impl<'a> Outcomes<'a> {
    fn new(instrument: &'a ir::Instrument, dynamic: bool) -> Self {
        let mut failures = HashMap::<_, Vec<_>>::new();
        for u in &instrument.unsupported { failures.entry(u.location.as_str()).or_default().push(u.feature.as_str()); }
        Self { failures, modulators: instrument.source_indices.modulators.iter().filter(|m| m.runtime.is_some()).map(|m| (m.group, m.slot, m.external)).collect(), dynamic }
    }
}
use sampler_ir::{self as ir, DspDisposition as D, DspSlotKind as K, DspSlotReason as R};

fn effect(fx: &crate::effects::Slot, scope: &str, voice: bool, muted: bool, actual: &Outcomes) -> ir::DspSlot {
    let kind = if fx.module == 0x18 { K::Filter } else { K::Fx };
    let module = match fx.params() {
        Some(crate::effects::Params::Filter { kind, .. }) => format!("Filter:{kind}"),
        Some(crate::effects::Params::Eq { .. }) => format!("EQ:v{:x}", fx.version),
        _ => format!("FX:{:02x}", fx.module),
    };
    // Use the production translator, including dynamic bypassed slots. A unit
    // impulse probes processor admission only; actual resource faults win below.
    let mut impulses = Vec::new();
    let mut load = |_| Ok((48000, vec![[1.0; 2]]));
    let mut source = crate::effects::Impulses { store: &mut impulses, load: &mut load };
    let chain = crate::effects::chain_with(std::slice::from_ref(fx),
        if voice { crate::effects::Scope::Voice } else { crate::effects::Scope::Bus },
        Some(&mut source), Some((-1, -1)), (-1, -1));
    let at = format!("{scope} slot {}", fx.slot);
    let failures = actual.failures.get(at.as_str()).map(Vec::as_slice).unwrap_or_default();
    let disposition = if muted {
        D::Dropped(R::MutedScopeNotInstantiated)
    } else if chain.notes.iter().any(|(_, f, _, _)| f == "effect") {
        D::Dropped(R::NotModeled)
    } else if failures.iter().any(|f| f.ends_with("impulse response")) {
        D::Dropped(R::ResourceUnavailable)
    } else if failures.contains(&"effect") {
        D::Dropped(R::NotModeled)
    } else if fx.bypass && !actual.dynamic {
        D::Dropped(R::SavedBypassNotInstantiated)
    } else {
        D::Approximated(R::NativeLawUnverified)
    };
    ir::DspSlot { kind, scope: scope.into(), slot: fx.slot, module,
        enabled: !muted && !fx.bypass, disposition }
}

fn rack(array: BParamArrayBParFX8, scope: &str, voice: bool, muted: bool, actual: &Outcomes, out: &mut Vec<ir::DspSlot>) {
    let decoded = crate::effects::rack(&array, |slot, _| out.push(ir::DspSlot {
        kind: K::Fx, scope: scope.into(), slot, module: "decode_failed".into(),
        // An undecodable enabled state is not counted as bypassed.
        enabled: !muted, disposition: D::Dropped(R::Malformed),
    }));
    out.extend(decoded.iter().map(|fx| effect(fx, scope, voice, muted, actual)));
}

fn modulation(scope: &str, slot: usize, module: String, disabled: bool, muted: bool, folded: bool, actual: &Outcomes, group: usize, external: bool) -> ir::DspSlot {
    let executed = actual.modulators.contains(&(group, slot, external));
    let disposition = if muted { D::Dropped(R::MutedScopeNotInstantiated) }
        else if disabled { D::Dropped(R::SavedBypassNotInstantiated) }
        else if folded { D::Implemented }
        else if !executed { D::Dropped(R::SourceNotExecuted) }
        else if actual.failures.get(scope).is_some_and(|rows| rows.iter().any(|f|
            matches!(*f, "modulation of a module parameter" | "modulation target" | "pan modulation" | "empty modulation shaper"))) {
            D::Dropped(R::TargetsDropped)
        } else { D::Approximated(R::NativeLawUnverified) };
    ir::DspSlot { kind: K::Mod, scope: scope.into(), slot, module, enabled: !muted && !disabled, disposition }
}

pub(crate) fn slots(program: &Program, instrument: &ir::Instrument, dynamic: bool) -> Result<Vec<ir::DspSlot>, ni_file::Error> {
    let actual = Outcomes::new(instrument, dynamic);
    let mut out = Vec::new();
    let mut racks = 0;
    let mut buses = 0;
    for child in &program.0.children {
        if child.id == 0x3a {
            let scope = ["instrument insert", "instrument send", "instrument main"].get(racks).copied().unwrap_or("extra rack");
            rack(BParamArrayBParFX8::try_from(child)?, scope, false, false, &actual, &mut out);
            racks += 1;
        } else if child.id == 0x45 {
            let bus = InsertBus::try_from(child)?;
            if let Some(c) = bus.0.find_first(0x3a) {
                rack(BParamArrayBParFX8::try_from(c)?, &format!("bus {buses}"), false, false, &actual, &mut out);
            }
            buses += 1;
        }
    }
    let groups = GroupList::try_from(program.0.find_first(0x33).ok_or(ni_file::Error::Static("Missing groups"))?)?;
    for (group, g) in groups.groups.iter().enumerate() {
        let p = g.params()?;
        let at = format!("group {group} {:?}", p.name);
        rack(g.insert_fx()?, &format!("{at} insert"), true, p.muted, &actual, &mut out);
        if let Some(c) = g.0.find_first(0x3b) {
            for (slot, m) in InternalModArray16::try_from(c)?.slots()? {
                let m = m.params()?;
                let module = match m.modulator { Modulator::Ahdsr(_) => "AHDSR".into(), Modulator::Flex(_) => "Flex".into(), Modulator::Lfo(l) => format!("LFO:{}", l.waveform), Modulator::Other { chunk_id } => format!("Mod:{chunk_id:x}") };
                out.push(modulation(&format!("{at} modulator slot {slot}"), slot.into(), module, m.unknown_flags[1] != 0, p.muted, m.targets.is_empty(), &actual, group, false));
            }
        }
        if let Some(c) = g.0.find_first(0x3c) {
            for (slot, m) in ExternalModArray32::try_from(c)?.slots()? {
                let m = m.params()?;
                let folded = m.source == ModSource::Unassigned || m.targets.is_empty() || matches!((&m.source, m.targets.as_slice()), (ModSource::Velocity, [t]) if t.param == "volume" && t.slot.is_none() && !t.invert && t.lag_ms == 0 && !t.shaper.as_ref().is_some_and(|s| s.enabled) && t.signed_intensity() == 1.0);
                out.push(modulation(&format!("{at} external modulation slot {slot}"), slot.into(), format!("{:?}", m.source), false, p.muted, folded, &actual, group, true));
            }
        }
    }
    // Scope names contain authored group names in memory only; export physical IDs.
    for slot in &mut out {
        if slot.scope.starts_with("group ") {
            let id = slot.scope.split_whitespace().nth(1).unwrap();
            let external = slot.scope.contains("external modulation slot");
            let suffix = if slot.kind != K::Mod { "insert" } else if external { "external" } else { "internal" };
            slot.scope = format!("group {id} {suffix}");
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_bypassed_effect_is_still_a_drop() {
        let fx = crate::effects::Slot { slot: 5, module: 0x63, version: 0x10, bypass: true, output_gain: 1., dry_level: 0., output_set: false, public: Vec::new() };
        let row = effect(&fx, "instrument send", false, false, &Outcomes::new(&ir::Instrument::default(), false));
        assert_eq!(row.disposition, D::Dropped(R::NotModeled));
        assert!(!row.enabled);
        assert_eq!(row.slot, 5);
    }
    #[test]
    fn folded_velocity_is_not_a_missing_modulator() {
        let row = modulation("g", 3, "Velocity".into(), false, false, true, &Outcomes::new(&ir::Instrument::default(), false), 0, true);
        assert_eq!(row.disposition, D::Implemented);
        let row = modulation("g", 3, "Velocity".into(), false, false, false, &Outcomes::new(&ir::Instrument::default(), false), 0, true);
        assert_eq!(row.disposition, D::Dropped(R::SourceNotExecuted));
    }
}
