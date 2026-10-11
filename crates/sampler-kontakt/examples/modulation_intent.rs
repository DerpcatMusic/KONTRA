//! Installed-library modulation intent, aggregates only; all scripts stay in RAM.
use ni_file::kontakt::objects::{
    ExternalModArray32, GroupList, InternalModArray16, Modulator, Program,
};
use std::{collections::BTreeMap, path::Path};
pub fn main() {
    for path in [
        "/mnt/MAIN_STORAGE/Libraries/Kontakt/Conflux 1.1.0 [Native Instruments]/Instruments/Conflux.nki",
        "/mnt/MAIN_STORAGE/Libraries/Kontakt/ANALOG STRINGS/Instruments/ANALOG STRINGS.nki",
    ] {
        let path = Path::new(path);
        let chunks = sampler_kontakt::read_chunks(path).unwrap();
        let program = Program::try_from(chunks.find_first(0x28).unwrap()).unwrap();
        let groups = GroupList::try_from(program.0.find_first(0x33).unwrap()).unwrap();
        let mut counts = BTreeMap::<String, usize>::new();
        let mut count = |key: String| *counts.entry(key).or_default() += 1;
        for group in &groups.groups {
            let mut sources = BTreeMap::new();
            if let Some(c) = group.0.find_first(0x3b) {
                for (slot, m) in InternalModArray16::try_from(c).unwrap().slots().unwrap() {
                    let p = m.params().unwrap();
                    let kind = match &p.modulator {
                        Modulator::Lfo(_) => "LFO",
                        Modulator::Ahdsr(_) => "AHDSR",
                        Modulator::Flex(_) => "Flex",
                        _ => "other",
                    };
                    sources.insert(slot, kind);
                    if let Modulator::Lfo(l) = &p.modulator {
                        count(format!(
                            "LFO wave={} version={:x} phase_zero={} width_half={} fade_zero={} weights={:?}",
                            l.waveform,
                            l.version,
                            l.initial_values[3] == 0.,
                            l.initial_values[2] == 0.5,
                            l.initial_values[0] == 0.,
                            l.trailing_values.map(|w| w.map(|x| if x == 0. {
                                0
                            } else if x.abs() == 1. {
                                1
                            } else {
                                2
                            }))
                        ));
                    }
                }
            }
            for (external, id) in [(false, 0x3b), (true, 0x3c)] {
                let Some(c) = group.0.find_first(id) else {
                    continue;
                };
                let rows = if external {
                    ExternalModArray32::try_from(c)
                        .unwrap()
                        .slots()
                        .unwrap()
                        .into_iter()
                        .map(|(_, m)| {
                            let p = m.params().unwrap();
                            (format!("{:?}", p.source), p.targets)
                        })
                        .collect::<Vec<_>>()
                } else {
                    InternalModArray16::try_from(c)
                        .unwrap()
                        .slots()
                        .unwrap()
                        .into_iter()
                        .map(|(slot, m)| (sources[&slot].to_string(), m.params().unwrap().targets))
                        .collect()
                };
                for (source, targets) in rows {
                    for t in targets {
                        if t.param == "loopStart" || t.param == "loopLength" {
                            count(format!(
                                "loop target {} mode={}",
                                t.param,
                                group.source_identity().unwrap().mode
                            ));
                        }
                        count(format!(
                            "{} -> {} owner={} signed={} invert={} lag={} shape={}",
                            source,
                            t.param,
                            if matches!(t.param.as_str(), "startPhase" | "frequency" | "intensity")
                            {
                                t.slot
                                    .and_then(|s| sources.get(&usize::from(s)).copied())
                                    .unwrap_or("missing source")
                            } else if t.slot.is_some() {
                                "FX"
                            } else {
                                "group"
                            },
                            t.signed_intensity() < 0.,
                            t.invert,
                            t.lag_ms != 0,
                            t.shaper.is_some_and(|s| s.enabled)
                        ));
                    }
                }
            }
            count(format!(
                "source mode={} reverse={}",
                group.source_identity().unwrap().mode,
                group.params().unwrap().reverse
            ));
        }
        let library = sampler_kontakt::read(path).unwrap();
        for zone in &library.instrument.zones {
            if zone.routes.iter().any(|r| {
                matches!(
                    library.instrument.routes[r.0].target,
                    sampler_ir::Target::LoopStart | sampler_ir::Target::LoopLength
                )
            }) {
                if let sampler_ir::Looping::Slots(slots) = &zone.playback.looping {
                    for slot in slots.iter().flatten() {
                        count(format!(
                            "controlled zone start_zero={} loop_start_zero={} crossfade_zero={} loop_count={} tuning_unity={}",
                            zone.playback.start == 0,
                            slot.range.start == 0,
                            slot.range.crossfade == sampler_ir::Span::Frames(0),
                            slot.count,
                            slot.tuning == 1.
                        ));
                    }
                }
            }
        }
        for b in &library.instrument.behaviors {
            // Read every authored script in RAM; retain only documented engine identifiers.
            for token in b
                .source
                .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            {
                if token.starts_with("ENGINE_PAR_") {
                    count(format!("script {token}"));
                }
            }
        }
        println!(
            "{} groups={} scripts={}\n{counts:#?}",
            path.file_name().unwrap().to_string_lossy(),
            groups.groups.len(),
            library.instrument.behaviors.len()
        );
    }
}
