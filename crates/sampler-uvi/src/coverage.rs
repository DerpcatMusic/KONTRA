//! Diagnostic inventory from authored XML and the production translation's node map.
use super::{Translation, modulation::source_node};
use roxmltree::Node;
use sampler_ir::{DspDisposition as D, DspSlot as Slot, DspSlotKind as K, DspSlotReason as R};
use std::collections::{HashMap, HashSet};

pub(super) fn slots(out: &Translation, program: Node) -> Vec<Slot> {
    let mut references = HashMap::<_, bool>::new();
    let mut native_targets = HashMap::<_, Vec<sampler_ir::DspTarget>>::new();
    let used: HashSet<_> = out.used.iter().copied().collect();
    for connection in program
        .descendants()
        .filter(|n| n.has_tag_name("SignalConnection"))
    {
        if let (Some(owner), Some(source)) = (connection.parent(), connection.attribute("Source")) {
            if let Some(node) = source_node(owner, source) {
                let failed = out.dropped_connections.contains(&connection.id());
                *references.entry(node.id()).or_default() |= failed;
                let targets = native_targets.entry(node.id()).or_default();
                targets.push(target(connection, targets.len(), failed));
            }
        }
    }
    let mut slots = Vec::new();
    for node in program.descendants().filter(|n| n.is_element()) {
        let Some(parent) = node.parent() else {
            continue;
        };
        let kind = node.tag_name().name();
        let is_mod = parent.has_tag_name("ControlSignalSources");
        if !is_mod && !parent.has_tag_name("Inserts") {
            continue;
        }
        let enabled = !node.ancestors().any(|n| {
            n.attribute("Bypass")
                .is_some_and(|v| v.parse::<f64>().is_ok_and(|b| b != 0.0) || v == "true")
        });
        let disposition = if !enabled {
            D::Dropped(R::SavedBypassNotInstantiated)
        } else if is_mod {
            if references.get(&node.id()) == Some(&true) {
                D::Dropped(R::TargetsDropped)
            } else if !references.contains_key(&node.id()) {
                D::Implemented
            } else {
                D::Approximated(R::NativeLawUnverified)
            }
        } else if !used.contains(&node.id()) {
            D::Dropped(R::NotModeled)
        } else {
            D::Approximated(R::NativeLawUnverified)
        };
        let scope = node
            .ancestors()
            .skip(2)
            .find(|n| n.is_element())
            .map(|n| format!("{}:{}", n.tag_name().name(), n.id().get_usize()))
            .unwrap_or_default();
        let slot = parent
            .children()
            .filter(|n| n.is_element())
            .position(|n| n == node)
            .unwrap();
        slots.push(Slot {
            kind: if is_mod {
                K::Mod
            } else if matches!(
                kind,
                "OnePole"
                    | "ThreeBandShelves"
                    | "CombFilter"
                    | "XpanderFilter"
                    | "MS20"
                    | "DigitalEq"
            ) {
                K::Filter
            } else {
                K::Fx
            },
            scope,
            slot,
            module: kind.into(),
            enabled,
            disposition,
            targets: if is_mod {
                native_targets
                    .remove(&node.id())
                    .unwrap_or_default()
                    .into_iter()
                    .map(|mut t| {
                        if !enabled {
                            t.enabled = false;
                            t.disposition = D::Dropped(R::SavedBypassNotInstantiated);
                        }
                        t
                    })
                    .collect()
            } else {
                Vec::new()
            },
        });
    }
    for node in program
        .descendants()
        .filter(|n| n.has_tag_name("SignalConnection"))
    {
        let Some(source) = node.attribute("Source") else {
            continue;
        };
        if node.parent().and_then(|p| source_node(p, source)).is_some() {
            continue;
        }
        let enabled = !node.ancestors().any(|n| {
            n.attribute("Bypass")
                .is_some_and(|v| v.parse::<f64>().is_ok_and(|b| b != 0.0) || v == "true")
        });
        slots.push(Slot {
            kind: K::Mod,
            scope: format!("connection:{}", node.id().get_usize()),
            slot: 0,
            module: if source.starts_with('@') {
                source.into()
            } else {
                "unresolved-source".into()
            },
            enabled,
            targets: vec![target(
                node,
                0,
                out.dropped_connections.contains(&node.id()),
            )],
            disposition: if !enabled {
                D::Dropped(R::SavedBypassNotInstantiated)
            } else if out.dropped_connections.contains(&node.id()) {
                D::Dropped(R::TargetsDropped)
            } else {
                D::Approximated(R::NativeLawUnverified)
            },
        });
    }
    slots
}

fn target(connection: Node, ordinal: usize, failed: bool) -> sampler_ir::DspTarget {
    let enabled = !connection.ancestors().any(|n| {
        n.attribute("Bypass")
            .is_some_and(|v| v.parse::<f64>().is_ok_and(|b| b != 0.0) || v == "true")
    });
    sampler_ir::DspTarget {
        ordinal,
        parameter: connection
            .attribute("Destination")
            .unwrap_or("unknown")
            .into(),
        module_slot: None,
        module: connection
            .parent()
            .and_then(|n| n.parent())
            .filter(|n| n.is_element())
            .map(|n| n.tag_name().name().into()),
        enabled,
        disposition: if !enabled {
            D::Dropped(R::SavedBypassNotInstantiated)
        } else if failed {
            D::Dropped(R::TargetsDropped)
        } else {
            D::Approximated(R::NativeLawUnverified)
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_oracle_preserves_reverse_and_fractional_bypass() {
        let text = r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer Reverse="1" Bypass="0.5"><PlaybackOptions Stop="480"/></SamplePlayer></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#;
        let document = roxmltree::Document::parse(text).unwrap();
        let native = native_family(document.root_element());
        assert!(native.zones[0].reverse);
        assert!(native.zones[0].muted);
        assert_eq!(native.zones[0].frames, 480);
    }
    #[test]
    fn native_oracle_keeps_original_oscillators_and_ranges() {
        let text = r#"<Program><Layers><Layer LowKey="50" HighKey="70"><Keygroups><Keygroup LowKey="40" HighKey="60" LowVelocity="80"><Oscillators><SamplePlayer Bypass="1" SamplePath="a.wav"/><SamplePlayer SamplePath="b.wav"><PlaybackOptions Start="48" Stop="480"><Loop Start="80" End="120"/></PlaybackOptions></SamplePlayer></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#;
        let (ir, ..) = super::super::translate_full(text, super::super::Source::Bank).unwrap();
        let native = ir.native_family.unwrap();
        assert_eq!(native.zones.len(), 2);
        assert_eq!(native.zones[1].id, 2);
        assert_eq!(
            native.zones[1].keys,
            sampler_ir::KeyRange { low: 50, high: 60 }
        );
        assert_eq!(native.zones[1].start, 48);
        assert_eq!(native.zones[1].loops[0].length, 40);
        assert!(native.zones[0].muted);
        assert_eq!(
            ir.source_indices.zones,
            vec![None, Some(sampler_ir::ZoneRef(0))]
        );
    }

    #[test]
    fn dropped_scope_connections_do_not_look_implemented() {
        let text = r#"<Program><ControlSignalSources><ConstantModulation Name="shared"/></ControlSignalSources><Connections><SignalConnection Source="$Program/shared" Destination="Gain" Ratio="1"/><SignalConnection Source="@MIDI CC 1" Destination="Pan" Ratio="1"/></Connections></Program>"#;
        let (ir, ..) = super::super::translate_full(text, super::super::Source::Bank).unwrap();
        let rows = ir.dsp_slots.unwrap();
        assert_eq!(rows.len(), 2);
        assert!(
            rows.iter()
                .all(|r| r.disposition == D::Dropped(R::TargetsDropped))
        );
    }

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

/// Read authored XML ranges and playback flags without consulting translated zones.
pub(super) fn native_family(program: Node) -> sampler_ir::NativeFamily {
    use sampler_ir::{KeyRange, NativeFamily, NativeFamilyLoop, NativeFamilyZone, VelocityRange};
    let mut data = NativeFamily {
        zones: Vec::new(),
        unknown: None,
    };
    let number = |n: Node, name: &str, default: i64| -> i64 {
        n.attribute(name)
            .and_then(|v| v.parse::<f64>().ok())
            .map_or(default, |v| v.round() as i64)
    };
    for (id, player) in program
        .descendants()
        .filter(|n| n.has_tag_name("SamplePlayer"))
        .enumerate()
    {
        let Some(keygroup) = player.ancestors().find(|n| n.has_tag_name("Keygroup")) else {
            data.unknown = Some("native-player-owner-absent".into());
            continue;
        };
        let mut low = number(keygroup, "LowKey", 0);
        let mut high = number(keygroup, "HighKey", 127);
        let layer = keygroup.ancestors().find(|n| n.has_tag_name("Layer"));
        if let Some(layer) = layer {
            low = low.max(number(layer, "LowKey", 0));
            high = high.min(number(layer, "HighKey", 127));
        }
        let velocity_low = number(keygroup, "LowVelocity", 1);
        let velocity_high = number(keygroup, "HighVelocity", 127);
        if [low, high, velocity_low, velocity_high]
            .iter()
            .any(|v| !(0..=127).contains(v))
        {
            data.unknown = Some("native-midi-range-invalid".into());
            continue;
        }
        let options = player
            .children()
            .find(|n| n.has_tag_name("PlaybackOptions"));
        for node in [Some(player), Some(keygroup), layer, options]
            .into_iter()
            .flatten()
        {
            for name in [
                "LowKey",
                "HighKey",
                "LowVelocity",
                "HighVelocity",
                "Start",
                "Stop",
                "SampleStart",
                "PlayDirection",
                "Reverse",
                "Bypass",
            ] {
                if node
                    .attribute(name)
                    .is_some_and(|v| v.parse::<f64>().map_or(true, |n| !n.is_finite()))
                {
                    data.unknown = Some("native-numeric-metadata-invalid".into());
                }
            }
        }
        let start = options.map_or(0, |n| number(n, "Start", 0));
        let frames = options.map_or(0, |n| number(n, "Stop", 0));
        if player
            .attribute("SampleStart")
            .is_some_and(|v| v.parse::<f64>().is_ok_and(|n| n != 0.0))
        {
            data.unknown = Some("native-uvi-sample-start-law-unverified".into());
        }
        let direction = options.map_or(0, |n| number(n, "PlayDirection", 0));
        if direction != 0 {
            data.unknown = Some("native-uvi-direction-law-unverified".into());
        }
        let mut loops = Vec::new();
        if let Some(options) = options {
            for (slot, l) in options
                .children()
                .filter(|n| n.has_tag_name("Loop"))
                .enumerate()
            {
                let kind = number(l, "Type", 0);
                if kind != 0 {
                    data.unknown = Some("native-uvi-loop-law-unverified".into());
                }
                loops.push(NativeFamilyLoop {
                    slot,
                    mode: if kind == 0 { 1 } else { kind as i32 + 1 },
                    start: number(l, "Start", 0),
                    length: number(l, "End", 0) - number(l, "Start", 0),
                    count: 0,
                    alternating: false,
                    crossfade: 0,
                    tuning: 1.0,
                });
            }
        }
        data.zones.push(NativeFamilyZone {
            id: id as u32 + 1,
            group: keygroup.id().get_usize() as u32,
            keys: KeyRange {
                low: low as u8,
                high: high as u8,
            },
            velocities: VelocityRange {
                low: velocity_low as u8,
                high: velocity_high as u8,
            },
            muted: player.ancestors().any(|n| {
                n.attribute("Bypass")
                    .is_some_and(|v| v.parse::<f64>().is_ok_and(|v| v != 0.0))
            }),
            start,
            end: 0,
            frames,
            reverse: player
                .attribute("Reverse")
                .is_some_and(|v| v.parse::<f64>().is_ok_and(|v| v != 0.0)),
            loops,
        });
    }
    data
}
