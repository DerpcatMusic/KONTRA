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

#[test]
fn a_canceled_load_stops_before_decoding() {
    let Some(path) = find("Una Corda Library/Instruments/Una Corda Pure.nki") else {
        return;
    };
    let mut decoded = 0;
    let result = sampler_kontakt::load_cancelable(
        &path,
        &sampler_kontakt::Options::default(),
        |p| decoded += matches!(p, sampler_kontakt::Progress::Decoding { .. }) as usize,
        || true,
    );
    assert!(matches!(result, Err(sampler_kontakt::LoadError::Canceled)));
    assert_eq!(decoded, 0);
}
