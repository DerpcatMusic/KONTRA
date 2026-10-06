//! Real installed libraries. Each test skips when its library is absent: the
//! roots come from `KONTRA_KONTAKT_LIBRARIES` (path-list separated) or the
//! player's settings (`~/.config/kontra/settings.json`), as v1 finds them.
//! Nothing read here is written anywhere.

use sampler_core::{Input, Limits, Protocol, Runtime};
use std::path::{Path, PathBuf};

fn roots() -> Vec<PathBuf> {
    if let Some(paths) = std::env::var_os("KONTRA_KONTAKT_LIBRARIES") {
        return std::env::split_paths(&paths).collect();
    }
    let Some(home) = std::env::var_os("HOME") else {
        return Vec::new();
    };
    let settings = std::fs::read_to_string(Path::new(&home).join(".config/kontra/settings.json"))
        .unwrap_or_default();
    // Each root is `{"path": "...", ...}`; only roots precede other keys holding paths.
    let roots = settings
        .split("\"roots\"")
        .nth(1)
        .and_then(|s| s.split(']').next())
        .unwrap_or("");
    roots
        .split("\"path\"")
        .skip(1)
        .filter_map(|s| s.split('"').nth(1))
        .map(PathBuf::from)
        .collect()
}

fn find(relative: &str) -> Option<PathBuf> {
    let found = roots()
        .into_iter()
        .map(|root| root.join(relative))
        .find(|p| p.is_file());
    if found.is_none() {
        eprintln!("skipped: {relative} is not installed");
    }
    found
}

fn input(key: u8) -> Input {
    Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key,
        external_id: None,
    }
}

fn limits() -> Limits {
    Limits {
        notes: 16,
        channels: 1,
        performances: 1,
        families: 16,
        expressions: 16,
        voices: 256,
        decisions: 256,
        commands: 64,
        behaviors: 0,
        behavior_fuel: 0,
        behavior_cells: 0,
        note_cells: 0,
    }
}

/// Load `keys` of an installed instrument without scripts and play its middle key.
fn render(
    relative: &str,
    keys: std::ops::RangeInclusive<u8>,
) -> Option<(sampler_ir::Instrument, f32)> {
    let path = find(relative)?;
    let options = sampler_kontakt::Options {
        keys: keys.clone(),
        scripts: false,
        ..Default::default()
    };
    let loaded = sampler_kontakt::load(&path, &options, |_| {}).unwrap();
    let mut rt = Runtime::new(loaded.plan, limits()).unwrap();
    let key = (keys.start() + keys.end()) / 2;
    rt.trigger(input(key), key, 0.8).unwrap();
    let mut out = vec![[0.0; 2]; 24000];
    rt.render(&mut out).unwrap();
    let peak = out.iter().flatten().fold(0f32, |p, x| p.max(x.abs()));
    Some((loaded.instrument, peak))
}

#[test]
fn una_corda_pure_renders_from_its_encrypted_monolith() {
    let Some((ir, peak)) = render("Una Corda Library/Instruments/Una Corda Pure.nki", 60..=60)
    else {
        return;
    };
    assert_eq!(ir.name, "Una Corda Pure");
    assert!(
        ir.zones.len() > 1 && !ir.modulators.is_empty(),
        "{} zones",
        ir.zones.len()
    );
    assert!(
        ir.behaviors
            .iter()
            .all(|b| b.language == sampler_ir::Language::Ksp && !b.source.is_empty())
    );
    assert!(
        ir.unsupported.iter().any(|u| u.feature == "script"),
        "uncompiled scripts are reported"
    );
    assert!(peak > 0.01, "audible: peak {peak}");
}

#[test]
fn conflux_renders_its_tracked_zones_within_the_runtime_pitch_range() {
    let Some((ir, peak)) = render(
        "Conflux 1.1.0 [Native Instruments]/Instruments/Conflux.nki",
        48..=72,
    ) else {
        return;
    };
    assert!(
        ir.unsupported
            .iter()
            .any(|u| u.feature == "wavetable source")
    );
    assert!(peak > 0.01, "audible: peak {peak}");
}

#[test]
fn vista_cellos_render_from_loose_ncw_samples() {
    let Some((ir, peak)) = render(
        "Performance Samples Vista/Instruments/Vista - 3 Cellos.nki",
        48..=48,
    ) else {
        return;
    };
    let ncw = ir
        .assets
        .iter()
        .filter(|a| a.encoding == sampler_ir::Encoding::Ncw)
        .count();
    assert_eq!(ncw, ir.assets.len());
    assert!(peak > 0.01, "audible: peak {peak}");
}

/// The first instrument of every installed library translates, or
/// fails only for want of local access data. Sample audio is not decoded.
#[test]
fn installed_instruments_translate_or_report_why_not() {
    let mut translated = 0;
    for root in roots() {
        let Ok(libraries) = std::fs::read_dir(&root) else {
            continue;
        };
        for library in libraries.flatten() {
            let mut instruments = Vec::new();
            collect(&library.path().join("Instruments"), &mut instruments);
            instruments.sort();
            for path in instruments.iter().take(1) {
                match sampler_kontakt::read(path) {
                    Ok(kontakt) => {
                        translated += 1;
                        eprintln!(
                            "{}: {} groups, {} zones, {} unsupported",
                            path.display(),
                            kontakt.instrument.groups.len(),
                            kontakt.instrument.zones.len(),
                            kontakt.instrument.unsupported.len()
                        );
                    }
                    Err(error @ sampler_kontakt::LoadError::Access { .. }) => eprintln!("{error}"),
                    Err(error) => panic!("{error}"),
                }
            }
        }
    }
    eprintln!("{translated} instruments translated");
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("nki"))
        {
            out.push(path);
        }
    }
}

/// Every installed instrument with authored modulation loads, lowers and
/// renders; prints per-feature report counts. Slow: run with --ignored.
#[test]
#[ignore]
fn survey_modulated_instruments_lower_and_render() {
    let mut instruments = Vec::new();
    for root in roots() {
        collect(&root, &mut instruments);
    }
    instruments.sort();
    let (mut loaded, mut routed, mut failed) = (0, 0, 0);
    let mut features = std::collections::BTreeMap::<String, usize>::new();
    for path in &instruments {
        let Ok(kontakt) = sampler_kontakt::read(path) else {
            continue;
        };
        for u in &kontakt.instrument.unsupported {
            if u.feature.contains("modulat")
                || u.feature.contains("LFO")
                || u.feature.contains("envelope")
            {
                *features
                    .entry(format!("{:?} {}", u.reason, u.feature))
                    .or_default() += 1;
            }
        }
        if kontakt.instrument.routes.is_empty() {
            continue;
        }
        routed += 1;
        let options = sampler_kontakt::Options {
            keys: 60..=60,
            scripts: false,
            ..Default::default()
        };
        match sampler_kontakt::load(path, &options, |_| {}) {
            Ok(loaded_plan) => {
                loaded += 1;
                let mut rt = match Runtime::new(loaded_plan.plan, limits()) {
                    Ok(rt) => rt,
                    Err(error) => {
                        println!("RUNTIME {}: {error:?}", path.display());
                        continue;
                    }
                };
                if let Err(error) = rt.trigger(input(60), 60, 0.8) {
                    println!("TRIGGER {}: {error:?}", path.display());
                    continue;
                }
                let mut out = vec![[0.0; 2]; 9600];
                rt.render(&mut out).unwrap();
                let peak = out.iter().flatten().fold(0f32, |p, x| p.max(x.abs()));
                assert!(peak.is_finite());
                println!(
                    "ok {peak:.3} {} routes {}",
                    path.display(),
                    loaded_plan.instrument.routes.len()
                );
            }
            Err(error) => {
                failed += 1;
                println!("FAIL {}: {error}", path.display());
            }
        }
    }
    for (feature, count) in &features {
        println!("{count:6} {feature}");
    }
    println!("{routed} instruments with routes: {loaded} loaded, {failed} failed");
}

/// Afflatus' multi-articulation patch switches in KSP: `on note` reads keys
/// `%KS_keys[$KS_Base]` onward into `$articulation`. The generated map taps
/// those keys, and its alternatives follow key order.
#[test]
fn afflatus_keyswitch_script_yields_an_articulation_map() {
    let Some(path) =
        find("Afflatus Chapter II Brass/Instruments/1. Ensembles/Multi Instruments/2 Horns KS.nki")
    else {
        return;
    };
    let ir = sampler_kontakt::read(&path).unwrap().instrument;
    for a in &ir.articulations {
        eprintln!(
            "{:>20} keys {:?} default {} -> {:?}",
            a.name, a.switch_keys, a.default, a.alternatives
        );
    }
    assert_eq!(ir.switching.owner, sampler_ir::SwitchOwner::Behavior);
    let names: Vec<_> = ir.articulations.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names.len(), 11);
    assert_eq!(&names[..3], ["Sustain + Legato", "Flutter", "Marcato"]);
    let keys: Vec<_> = ir.articulations.iter().map(|a| a.switch_keys[0]).collect();
    assert_eq!(keys, (24..=34).collect::<Vec<_>>());
    assert!(ir.articulations[0].default);
    let marcato = ir.articulations[2].alternatives;
    assert_eq!(
        marcato.controller.map(|c| (c.controller, c.low)),
        Some((32, 2))
    );
    assert_eq!((marcato.channel, marcato.program), (Some(2), Some(2)));
    let mut driven = ir.clone();
    driven.switching.driver = sampler_ir::Driver::Velocity;
    assert_eq!(driven.validate(), Ok(()));
}

/// Every report feature across the installed instruments, as (instruments,
/// entries). Translation only, nothing written: run with --ignored.
#[test]
#[ignore]
fn survey_reported_features() {
    let mut instruments = Vec::new();
    for root in roots() {
        collect(&root, &mut instruments);
    }
    instruments.sort();
    let mut features = std::collections::BTreeMap::<String, (usize, usize)>::new();
    for path in &instruments {
        let Ok(kontakt) = sampler_kontakt::read(path) else {
            continue;
        };
        let mut seen = std::collections::HashSet::new();
        for u in &kontakt.instrument.unsupported {
            let key = format!("{:?} {}", u.reason, u.feature);
            let entry = features.entry(key.clone()).or_default();
            entry.1 += 1;
            if seen.insert(key) {
                entry.0 += 1;
            }
        }
    }
    let mut sorted: Vec<_> = features.into_iter().collect();
    sorted.sort_by_key(|(_, (n, _))| std::cmp::Reverse(*n));
    for (feature, (n, entries)) in sorted {
        println!("{n:6} {entries:9} {feature}");
    }
    println!("{} instruments", instruments.len());
}
