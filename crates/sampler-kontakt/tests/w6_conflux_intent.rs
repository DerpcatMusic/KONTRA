//! Installed-library intent census; emits aggregates and documented identifiers only.
use ni_file::kontakt::objects::{
    BParFX, ExternalModArray32, GroupList, InternalModArray16, Program,
};
use std::{collections::BTreeMap, path::Path};
fn count(map: &mut BTreeMap<String, usize>, key: String) {
    *map.entry(key).or_default() += 1;
}
#[test]
#[ignore = "requires installed Conflux; run through kontakto-heavy"]
fn read_conflux_script_and_group_intent() {
    let path = Path::new(
        "/mnt/MAIN_STORAGE/Libraries/Kontakt/Conflux 1.1.0 [Native Instruments]/Instruments/Conflux.nki",
    );
    assert!(path.is_file(), "required installed gate absent");
    let chunks = sampler_kontakt::read_chunks(path).unwrap();
    let program = Program::try_from(chunks.find_first(0x28).unwrap()).unwrap();
    let groups = GroupList::try_from(program.0.find_first(0x33).unwrap()).unwrap();
    let mut sources = BTreeMap::new();
    let mut targets = BTreeMap::new();
    let mut fx = BTreeMap::new();
    let mut source_targets = BTreeMap::new();
    let mut internal_shapes = BTreeMap::new();
    for (gi, group) in groups.groups.iter().enumerate() {
        let p = group.params().unwrap();
        let mode = group.source_identity().unwrap().mode;
        count(
            &mut sources,
            format!(
                "mode{mode} muted{} solo{} tracking{} gain_nonzero{}",
                p.muted,
                p.soloed,
                p.key_tracking,
                p.volume != 0.
            ),
        );
        if let Some(w) = group.wavetable_source().unwrap() {
            let supported = p.key_tracking
                && matches!(w.form_type, 1 | 17)
                && matches!(w.form2_type, 1 | 17)
                && !w.inharmonic_enabled
                && w.mod_type == 0
                && w.phase_random == 0.;
            count(
                &mut sources,
                format!(
                    "wavetable v1_admitted{supported} forms{}/{} mod{} random{} inharmonic{}",
                    w.form_type,
                    w.form2_type,
                    w.mod_type,
                    w.phase_random != 0.,
                    w.inharmonic_enabled
                ),
            );
        }
        let mut modules = BTreeMap::new();
        for (slot, c) in group.insert_fx().unwrap().items.iter().enumerate() {
            if let Some(c) = c {
                let f = BParFX::try_from(c).unwrap();
                let p = f.params().unwrap();
                let module = f.effect().map_or(0, |c| c.id);
                modules.insert(slot, module);
                count(
                    &mut fx,
                    format!(
                        "module{module:x} enum{} version{:x} bypass{} wet_nonzero{}",
                        p.effect_type,
                        f.version(),
                        p.bypass,
                        p.output_gain != 0.
                    ),
                );
            }
        }
        let mut note_target =
            |internal: bool,
             slot: usize,
             bypass: bool,
             t: &ni_file::kontakt::objects::ModTarget| {
                let module = t.slot.and_then(|s| modules.get(&(s as usize))).copied();
                count(
                    &mut targets,
                    format!(
                        "{} target{} owner{:?} nonzero{} bypass{}",
                        if internal { "internal" } else { "external" },
                        t.param,
                        module,
                        t.intensity != 0.,
                        bypass
                    ),
                );
                if t.slot.is_some() && !bypass && t.intensity != 0. {
                    println!(
                        "ADDRESSED group{gi} source{slot} internal{internal} param{} module{:?}",
                        t.param, module
                    );
                }
            };
        if let Some(c) = group.0.find_first(0x3b) {
            for (slot, m) in InternalModArray16::try_from(c).unwrap().slots().unwrap() {
                let p = m.params().unwrap();
                if let ni_file::kontakt::objects::Modulator::Lfo(l) = &p.modulator {
                    count(
                        &mut internal_shapes,
                        format!(
                            "wave{} version{:x} nonzero_weights{}",
                            l.waveform,
                            l.version,
                            l.trailing_values
                                .unwrap_or([0.; 5])
                                .iter()
                                .filter(|x| **x != 0.)
                                .count()
                        ),
                    );
                }
                for t in p.targets {
                    note_target(true, slot, p.unknown_flags[1] != 0, &t);
                }
            }
        }
        if let Some(c) = group.0.find_first(0x3c) {
            for (slot, m) in ExternalModArray32::try_from(c).unwrap().slots().unwrap() {
                let p = m.params().unwrap();
                for t in p.targets {
                    count(
                        &mut source_targets,
                        format!(
                            "{:?} {} slot{:?} nonzero{}",
                            p.source,
                            t.param,
                            t.slot,
                            t.intensity != 0.
                        ),
                    );
                    note_target(false, slot, false, &t);
                }
            }
        }
    }
    for (rack, c) in program
        .0
        .children
        .iter()
        .filter(|c| c.id == 0x3a)
        .enumerate()
    {
        let slots = ni_file::kontakt::objects::BParamArrayBParFX8::try_from(c).unwrap();
        for (slot, c) in slots.items.iter().enumerate() {
            if let Some(c) = c {
                let f = BParFX::try_from(c).unwrap();
                let p = f.params().unwrap();
                println!(
                    "PROGRAM_FX rack{rack} slot{slot} module{:x} bypass{} wet_nonzero{} dry_nonzero{}",
                    f.effect().map_or(0, |c| c.id),
                    p.bypass,
                    p.output_gain != 0.,
                    p.dry_level != 0.
                );
            }
        }
    }
    println!("SOURCE_TARGETS {source_targets:#?}\nINTERNAL_SHAPES {internal_shapes:#?}");
    let library = sampler_kontakt::read(path).unwrap();
    let mut script = BTreeMap::new();
    let mut callbacks = BTreeMap::new();
    for behavior in &library.instrument.behaviors {
        for line in behavior.source.lines() {
            let l = line.trim();
            for token in l.split(|c: char| !c.is_ascii_alphanumeric() && c != '_') {
                if token.starts_with("ENGINE_PAR_") {
                    count(&mut script, token.to_string());
                }
            }
            if let Some(on) = l.strip_prefix("on ") {
                count(
                    &mut callbacks,
                    on.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                        .next()
                        .unwrap_or("")
                        .to_string(),
                );
            }
        }
    }
    let mut reasons = BTreeMap::new();
    for u in &library.instrument.unsupported {
        let detail = if u.feature == "effect" {
            u.value.clone()
        } else {
            String::new()
        };
        count(
            &mut reasons,
            format!("{:?} {} {}", u.reason, u.feature, detail),
        );
    }
    let mut automation = BTreeMap::new();
    for a in sampler_kontakt::program_automation(
        &program.0.private_data,
        program.version(),
        sampler_kontakt::Limits {
            bytes: 64 << 20,
            records: 65536,
        },
    )
    .unwrap()
    {
        let tag = std::str::from_utf8(a.tag.data()).unwrap_or("");
        let class = if tag.starts_with("pts_script_slider_") {
            "script_slider"
        } else if tag.is_empty() {
            "empty"
        } else if tag.to_ascii_lowercase().contains("volume") {
            "volume"
        } else if tag.to_ascii_lowercase().contains("pan") {
            "pan"
        } else if tag.to_ascii_lowercase().contains("tune") {
            "tune"
        } else {
            "other"
        };
        count(
            &mut automation,
            format!(
                "mode{} class{class} id{} secondary{:?} range_valid{}",
                a.mode,
                a.id,
                a.secondary_id,
                (0. ..=1.).contains(&a.low) && (0. ..=1.).contains(&a.high)
            ),
        );
    }
    println!(
        "GROUP_COUNT {}\nSOURCES {sources:#?}\nTARGETS {targets:#?}\nFX {fx:#?}\nSCRIPT_CONSTANTS {script:#?}\nCALLBACKS {callbacks:#?}\nAUTOMATION {automation:#?}\nREASONS {reasons:#?}",
        groups.groups.len()
    );
    assert!(!library.instrument.behaviors.is_empty());
    let waves: Vec<_> = library
        .instrument
        .groups
        .iter()
        .enumerate()
        .filter(|(_, g)| g.wavetable.is_some())
        .collect();
    println!(
        "MODELED_WAVES {} WAVE_ZONES {} WAVE_GAIN_NONZERO {}",
        waves.len(),
        library
            .instrument
            .zones
            .iter()
            .filter(|z| z
                .group
                .is_some_and(|r| library.instrument.groups[r.0].wavetable.is_some()))
            .count(),
        waves.iter().filter(|(_, g)| g.gain.linear() != 0.).count()
    );
    assert_eq!(waves.len(), 1);
    assert_eq!(
        library
            .instrument
            .unsupported
            .iter()
            .filter(|u| u.feature == "wavetable source")
            .count(),
        0,
        "v1-admitted saved wavetable must reach oscillator playback"
    );
}
