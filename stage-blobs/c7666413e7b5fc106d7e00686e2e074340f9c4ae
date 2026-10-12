//! Authored slots joined to actual translation outcomes. No payloads are exported.
use ni_file::kontakt::objects::*;
use std::collections::{HashMap, HashSet};

pub(crate) type Targets = HashMap<(usize, usize, bool, usize), bool>;

struct Outcomes<'a> {
    failures: HashMap<&'a str, Vec<&'a str>>,
    modulators: HashSet<(usize, usize, bool)>,
    dynamic: bool,
    writes: &'a [sampler_ksp::EnginePar],
    buses: HashSet<i32>,
    targets: &'a Targets,
}
impl<'a> Outcomes<'a> {
    fn new(
        instrument: &'a ir::Instrument,
        dynamic: bool,
        writes: &'a [sampler_ksp::EnginePar],
        targets: &'a Targets,
    ) -> Self {
        let mut failures = HashMap::<_, Vec<_>>::new();
        for u in &instrument.unsupported {
            failures
                .entry(u.location.as_str())
                .or_default()
                .push(u.feature.as_str());
        }
        Self {
            failures,
            modulators: instrument
                .source_indices
                .modulators
                .iter()
                .filter(|m| m.runtime.is_some())
                .map(|m| (m.group, m.slot, m.external))
                .collect(),
            dynamic,
            writes,
            targets,
            buses: instrument.bus_addresses.iter().map(|(id, _)| *id).collect(),
        }
    }
}
use sampler_ir::{self as ir, DspDisposition as D, DspSlotKind as K, DspSlotReason as R};

fn effect(
    fx: &crate::effects::Slot,
    scope: &str,
    voice: bool,
    muted: bool,
    actual: &Outcomes,
) -> ir::DspSlot {
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
    let mut source = crate::effects::Impulses {
        store: &mut impulses,
        recipes: None,
        load: &mut load,
    };
    let chain = crate::effects::chain_with(
        std::slice::from_ref(fx),
        if voice {
            crate::effects::Scope::Voice
        } else {
            crate::effects::Scope::Bus
        },
        Some(&mut source),
        Some((-1, -1)),
        (-1, -1),
        &[],
    );
    let at = format!("{scope} slot {}", fx.slot);
    let failures = actual
        .failures
        .get(at.as_str())
        .map(Vec::as_slice)
        .unwrap_or_default();
    let disposition = if muted {
        D::Dropped(R::MutedScopeNotInstantiated)
    } else if chain.notes.iter().any(|(_, f, _, _)| f == "effect") {
        D::Dropped(R::NotModeled)
    } else if failures.iter().any(|f| f.ends_with("impulse response")) {
        D::Dropped(R::ResourceUnavailable)
    } else if failures.contains(&"effect") {
        D::Dropped(R::NotModeled)
    } else if scope.starts_with("bus ")
        && !actual.dynamic
        && !scope
            .split_whitespace()
            .nth(1)
            .and_then(|v| v.parse::<i32>().ok())
            .is_some_and(|id| actual.buses.contains(&(1000 + id)))
    {
        D::Dropped(R::ScopeNotInstantiated)
    } else if fx.bypass && !actual.dynamic {
        D::Dropped(R::SavedBypassNotInstantiated)
    } else {
        D::Approximated(R::NativeLawUnverified)
    };
    ir::DspSlot {
        kind,
        scope: scope.into(),
        slot: fx.slot,
        module,
        enabled: !muted && !fx.bypass,
        disposition,
        targets: Vec::new(),
    }
}

fn rack(
    array: BParamArrayBParFX8,
    scope: &str,
    voice: bool,
    muted: bool,
    actual: &Outcomes,
    address: (i32, i32),
    out: &mut Vec<ir::DspSlot>,
) {
    let mut decoded = crate::effects::rack(&array, |slot, _| {
        let native = array.items[slot]
            .as_ref()
            .and_then(|c| BParFX::try_from(c).ok());
        let module = native.as_ref().and_then(|fx| fx.effect()).map(|e| e.id);
        let bypass = native
            .as_ref()
            .and_then(|fx| fx.params().ok())
            .is_some_and(|p| p.bypass);
        out.push(ir::DspSlot {
            kind: if module == Some(0x18) {
                K::Filter
            } else {
                K::Fx
            },
            scope: scope.into(),
            slot,
            module: module.map_or_else(|| "decode_failed".into(), |id| format!("FX:{id:02x}")),
            enabled: !muted && !bypass,
            disposition: D::Dropped(R::Malformed),
            targets: Vec::new(),
        });
    });
    crate::effects::apply_writes(&mut decoded, actual.writes, address.0, address.1);
    out.extend(
        decoded
            .iter()
            .map(|fx| effect(fx, scope, voice, muted, actual)),
    );
}

fn modulation(
    scope: &str,
    slot: usize,
    module: String,
    disabled: bool,
    muted: bool,
    folded: bool,
    actual: &Outcomes,
    group: usize,
    external: bool,
    targets: &[ModTarget],
    modules: &HashMap<usize, String>,
) -> ir::DspSlot {
    let executed = actual.modulators.contains(&(group, slot, external));
    let disposition = if muted {
        D::Dropped(R::MutedScopeNotInstantiated)
    } else if disabled {
        D::Dropped(R::SavedBypassNotInstantiated)
    } else if folded {
        D::Implemented
    } else if !executed {
        D::Dropped(R::SourceNotExecuted)
    } else if actual.failures.get(scope).is_some_and(|rows| {
        rows.iter().any(|f| {
            matches!(
                *f,
                "modulation of a module parameter"
                    | "modulation target"
                    | "pan modulation"
                    | "empty modulation shaper"
            )
        })
    }) {
        D::Dropped(R::TargetsDropped)
    } else {
        D::Approximated(R::NativeLawUnverified)
    };
    ir::DspSlot {
        kind: K::Mod,
        scope: scope.into(),
        slot,
        module,
        enabled: !muted && !disabled,
        disposition,
        targets: targets
            .iter()
            .enumerate()
            .map(|(ordinal, t)| ir::DspTarget {
                ordinal,
                parameter: t.param.clone(),
                module_slot: t.slot.map(usize::from),
                module: t.slot.and_then(|s| modules.get(&usize::from(s))).cloned(),
                enabled: !muted && !disabled,
                disposition: if matches!(disposition, D::Dropped(R::TargetsDropped)) {
                    if actual.targets.get(&(group, slot, external, ordinal)) == Some(&true) {
                        D::Approximated(R::NativeLawUnverified)
                    } else {
                        D::Dropped(R::TargetsDropped)
                    }
                } else {
                    disposition
                },
            })
            .collect(),
    }
}

pub(crate) fn slots(
    program: &Program,
    instrument: &ir::Instrument,
    dynamic: bool,
    writes: &[sampler_ksp::EnginePar],
    targets: &Targets,
) -> Result<Vec<ir::DspSlot>, ni_file::Error> {
    let actual = Outcomes::new(instrument, dynamic, writes, targets);
    let mut out = Vec::new();
    let mut racks = 0;
    let mut buses = 0;
    for child in &program.0.children {
        if child.id == 0x3a {
            let scope = ["instrument insert", "instrument send", "instrument main"]
                .get(racks)
                .copied()
                .unwrap_or("extra rack");
            rack(
                BParamArrayBParFX8::try_from(child)?,
                scope,
                false,
                false,
                &actual,
                (-1, [1, 0, 2].get(racks).copied().unwrap_or(-1)),
                &mut out,
            );
            racks += 1;
        } else if child.id == 0x45 {
            let bus = InsertBus::try_from(child)?;
            if let Some(c) = bus.0.find_first(0x3a) {
                rack(
                    BParamArrayBParFX8::try_from(c)?,
                    &format!("bus {buses}"),
                    false,
                    false,
                    &actual,
                    (-1, 1000 + buses),
                    &mut out,
                );
            }
            buses += 1;
        }
    }
    let groups = GroupList::try_from(
        program
            .0
            .find_first(0x33)
            .ok_or(ni_file::Error::Static("Missing groups"))?,
    )?;
    for (group, g) in groups.groups.iter().enumerate() {
        let p = g.params()?;
        let at = format!("group {group} {:?}", p.name);
        let first_insert = out.len();
        rack(
            g.insert_fx()?,
            &format!("{at} insert"),
            true,
            p.muted,
            &actual,
            (group as i32, -1),
            &mut out,
        );
        let modules: HashMap<_, _> = out[first_insert..]
            .iter()
            .map(|s| (s.slot, s.module.clone()))
            .collect();
        if let Some(c) = g.0.find_first(0x3b) {
            for (slot, m) in InternalModArray16::try_from(c)?.slots()? {
                let m = m.params()?;
                let module = match m.modulator {
                    Modulator::Ahdsr(_) => "AHDSR".into(),
                    Modulator::Flex(_) => "Flex".into(),
                    Modulator::Lfo(l) => format!("LFO:{}", l.waveform),
                    Modulator::Other { chunk_id } => format!("Mod:{chunk_id:x}"),
                };
                out.push(modulation(
                    &format!("{at} modulator slot {slot}"),
                    slot.into(),
                    module,
                    m.unknown_flags[1] != 0,
                    p.muted,
                    m.targets.is_empty(),
                    &actual,
                    group,
                    false,
                    &m.targets,
                    &modules,
                ));
            }
        }
        if let Some(c) = g.0.find_first(0x3c) {
            for (slot, m) in ExternalModArray32::try_from(c)?.slots()? {
                let m = m.params()?;
                let folded = m.source == ModSource::Unassigned
                    || m.targets.is_empty()
                    || matches!((&m.source, m.targets.as_slice()), (ModSource::Velocity, [t]) if t.param == "volume" && t.slot.is_none() && !t.invert && t.lag_ms == 0 && !t.shaper.as_ref().is_some_and(|s| s.enabled) && t.signed_intensity() == 1.0);
                out.push(modulation(
                    &format!("{at} external modulation slot {slot}"),
                    slot.into(),
                    format!("{:?}", m.source),
                    false,
                    p.muted,
                    folded,
                    &actual,
                    group,
                    true,
                    &m.targets,
                    &modules,
                ));
            }
        }
    }
    // Scope names contain authored group names in memory only; export physical IDs.
    for slot in &mut out {
        if slot.scope.starts_with("group ") {
            let id = slot.scope.split_whitespace().nth(1).unwrap();
            let external = slot.scope.contains("external modulation slot");
            let suffix = if slot.kind != K::Mod {
                "insert"
            } else if external {
                "external"
            } else {
                "internal"
            };
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
        let fx = crate::effects::Slot {
            slot: 5,
            module: 0x63,
            version: 0x10,
            bypass: true,
            output_gain: 1.,
            dry_level: 0.,
            output_set: false,
            public: Vec::new(),
        };
        let row = effect(
            &fx,
            "instrument send",
            false,
            false,
            &Outcomes::new(&ir::Instrument::default(), false, &[], &Targets::new()),
        );
        assert_eq!(row.disposition, D::Dropped(R::NotModeled));
        assert!(!row.enabled);
        assert_eq!(row.slot, 5);
    }
    #[test]
    fn bypassed_supported_effect_requires_an_executable_slot() {
        let fx = crate::effects::Slot {
            slot: 0,
            module: 0x13,
            version: 0x10,
            bypass: true,
            output_gain: 1.,
            dry_level: 0.,
            output_set: false,
            public: 1.0f32.to_le_bytes().to_vec(),
        };
        assert_eq!(
            effect(
                &fx,
                "instrument insert",
                false,
                false,
                &Outcomes::new(&ir::Instrument::default(), false, &[], &Targets::new())
            )
            .disposition,
            D::Dropped(R::SavedBypassNotInstantiated)
        );
        assert_eq!(
            effect(
                &fx,
                "instrument insert",
                false,
                false,
                &Outcomes::new(&ir::Instrument::default(), true, &[], &Targets::new())
            )
            .disposition,
            D::Approximated(R::NativeLawUnverified)
        );
    }
    #[test]
    fn targets_name_each_lost_route_without_counting_admitted_siblings() {
        let mut instrument = ir::Instrument::default();
        instrument
            .source_indices
            .modulators
            .push(ir::SourceModulator {
                group: 0,
                slot: 3,
                external: true,
                name: String::new(),
                runtime: Some(ir::ModulatorRef(0)),
            });
        instrument.unsupported.push(ir::Unsupported {
            location: "g".into(),
            feature: "modulation of a module parameter".into(),
            value: String::new(),
            reason: ir::Reason::NotModeled,
        });
        let targets = Targets::from([((0, 3, true, 0), true), ((0, 3, true, 1), false)]);
        let native = |param: &str| ModTarget {
            param: param.into(),
            intensity: 1.,
            lag_ms: 0,
            name: String::new(),
            slot: Some(7),
            invert: false,
            shaper: None,
            unknown_i16: -1,
            unknown_flags: 0,
        };
        let row = modulation(
            "g",
            3,
            "Constant".into(),
            false,
            false,
            false,
            &Outcomes::new(&instrument, false, &[], &targets),
            0,
            true,
            &[native("filterCutoff"), native("filterReso")],
            &HashMap::from([(7, "Filter:52".into())]),
        );
        assert_eq!(row.disposition, D::Dropped(R::TargetsDropped));
        assert_eq!(
            row.targets[0].disposition,
            D::Approximated(R::NativeLawUnverified)
        );
        assert_eq!(row.targets[1].disposition, D::Dropped(R::TargetsDropped));
        assert_eq!(row.targets[1].parameter, "filterReso");
        assert_eq!(row.targets[1].module.as_deref(), Some("Filter:52"));
    }

    #[test]
    fn folded_velocity_is_not_a_missing_modulator() {
        let row = modulation(
            "g",
            3,
            "Velocity".into(),
            false,
            false,
            true,
            &Outcomes::new(&ir::Instrument::default(), false, &[], &Targets::new()),
            0,
            true,
            &[],
            &HashMap::new(),
        );
        assert_eq!(row.disposition, D::Implemented);
        let row = modulation(
            "g",
            3,
            "Velocity".into(),
            false,
            false,
            false,
            &Outcomes::new(&ir::Instrument::default(), false, &[], &Targets::new()),
            0,
            true,
            &[],
            &HashMap::new(),
        );
        assert_eq!(row.disposition, D::Dropped(R::SourceNotExecuted));
    }
}

/// Native targets only; runtime admission and route lowering are not consulted.
pub(crate) fn start_mod_groups(program: &Program) -> Result<Vec<u32>, ni_file::Error> {
    let groups = GroupList::try_from(
        program
            .0
            .find_first(0x33)
            .ok_or(ni_file::Error::Static("Missing groups"))?,
    )?;
    let mut active = Vec::new();
    for (id, g) in groups.groups.iter().enumerate() {
        let relevant = |t: &ModTarget| t.param == "playPos" && t.signed_intensity() != 0.0;
        let mut start = false;
        if let Some(c) = g.0.find_first(0x3b) {
            for (_, m) in InternalModArray16::try_from(c)?.slots()? {
                let p = m.params()?;
                start |= p.unknown_flags[1] == 0 && p.targets.iter().any(relevant);
            }
        }
        if let Some(c) = g.0.find_first(0x3c) {
            for (_, m) in ExternalModArray32::try_from(c)?.slots()? {
                let p = m.params()?;
                start |= p.source != ModSource::Unassigned && p.targets.iter().any(relevant);
            }
        }
        if start {
            active.push(id as u32);
        }
    }
    Ok(active)
}
