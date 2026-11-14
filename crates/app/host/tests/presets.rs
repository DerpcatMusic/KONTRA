//! Browsing and restoring a real library's presets, as the player does:
//! the catalog lists them, the header lists an instrument with its
//! snapshots, and each preset, applied through [`Rack::load`], leaves the
//! part holding the script values its file saved.
#![cfg(test)]

use std::path::{Path, PathBuf};

use kontra_host::rack::{Rack, RackAudio};
use kontra_library::{Cache, Catalog, Progress, Root};
use kontra_stream::{Store, StoreConfig};

const RATE: u32 = 48_000;
const BLOCK: usize = 256;

fn rack() -> (Rack, RackAudio) {
    Rack::new(RATE, Store::new(StoreConfig::DEFAULT).unwrap(), BLOCK)
}

/// The catalog the browser shows for the library at `dir`.
fn catalog(dir: &Path) -> Catalog {
    let roots = [Root {
        path: dir.to_owned(),
        single: true,
    }];
    let programs = |b: &Path| kontra_format_uvi::programs(b).map_err(|e| e.to_string());
    kontra_library::scan(
        &roots,
        &Progress::default(),
        &mut Cache::default(),
        &programs,
    )
    .expect("not canceled")
}

/// Ticks and blocks enough for a recall's round trip and the scripts'
/// callbacks after it.
fn settle(rack: &mut Rack, audio: &mut RackAudio) {
    for _ in 0..16 {
        rack.tick();
        audio.process(BLOCK, &[]);
    }
    rack.tick();
}

/// The script values `path` saves, as its reader gives them.
fn saved_in(path: &Path) -> Vec<String> {
    let file = kontra_host::instrument::read_saved(path, &[], &[]).unwrap();
    (file.scripts.iter())
        .flat_map(|s| s.saved.iter().cloned())
        .collect()
}

/// What `path`, applied to part 0, leaves the part holding that its file
/// saved differently: (missing or changed values, how many it saved).
fn restore(
    rack: &mut Rack,
    audio: &mut RackAudio,
    path: &Path,
) -> Result<(Vec<String>, usize), String> {
    rack.load(0, path).map_err(|e| e.to_string())?;
    settle(rack, audio);
    let held = rack.saved_scripts(0).concat();
    let want = saved_in(path);
    // An array the file saved shorter than the script now declares keeps
    // its saved cells and the declared rest: the saved ones count.
    // A UVI widget's `name=value` is a float: the same number to its f32
    // precision holds, however it is written (`6.3656972e-07`).
    let same_number = |a: &str, b: &str| {
        let (Some((an, av)), Some((bn, bv))) = (a.split_once('='), b.split_once('=')) else {
            return false;
        };
        match (av.parse::<f64>(), bv.parse::<f64>()) {
            (Ok(x), Ok(y)) => an == bn && (x - y).abs() <= 1e-5 * x.abs().max(1.0),
            _ => false,
        }
    };
    let holds = |v: &String| {
        (held.iter()).any(|h| {
            h == v
                || h.starts_with(v.as_str()) && h.as_bytes().get(v.len()) == Some(&b' ')
                || same_number(h, v)
        })
    };
    let off: Vec<String> = (want.iter()).filter(|v| !holds(v)).cloned().collect();
    if std::env::var_os("KONTRA_PRESET_DETAIL").is_some() {
        for v in &off {
            // Kontakt's `name value`, UVI's `name=value`.
            let name_of = |s: &str| s.split([' ', '=']).next().map(str::to_owned);
            let name = name_of(v);
            let ours = held.iter().find(|h| name_of(h) == name);
            println!(
                "    file {:.160}\n    held {:.160}",
                v,
                ours.map_or("(none)", String::as_str)
            );
        }
    }
    Ok((off, want.len()))
}

/// `KONTRA_PRESET_LIBRARIES`: library folders (a UVI bank's folder too),
/// separated by `|`. `KONTRA_PRESET_LIMIT` caps the presets read per
/// instrument or bank; the report says how many it left out.
#[test]
#[ignore = "reads real libraries on this machine"]
fn every_preset_lists_under_its_instrument_and_restores_its_values() {
    let Ok(dirs) = std::env::var("KONTRA_PRESET_LIBRARIES") else {
        return;
    };
    let limit = std::env::var("KONTRA_PRESET_LIMIT")
        .ok()
        .and_then(|l| l.parse().ok())
        .unwrap_or(usize::MAX);
    let mut failures = Vec::new();
    for dir in dirs.split('|').map(PathBuf::from) {
        for library in catalog(&dir).libraries {
            let presets = &library.presets;
            let multis = (presets.iter()).filter(|p| has_ext(p, "nkm")).count();
            // Browsing: each instrument with its snapshots, and the
            // snapshots no instrument of the library claims.
            let instruments: Vec<usize> = (0..presets.len())
                .filter(|&n| !has_ext(&presets[n], "nksn") && !has_ext(&presets[n], "nkm"))
                .collect();
            let mut claimed = 0;
            let mut groups = Vec::new();
            for &n in &instruments {
                let family = kontra_ui_model::preset_family(presets, Some(n));
                let own: Vec<usize> = family
                    .iter()
                    .copied()
                    .filter(|&k| k != n && has_ext(&presets[k], "nksn"))
                    .collect();
                claimed += own.len();
                groups.push((n, own));
            }
            let snapshots = (presets.iter()).filter(|p| has_ext(p, "nksn")).count();
            println!(
                "{}: {} presets, {} instruments, {snapshots} snapshots ({claimed} under their instrument), {multis} multis",
                library.name,
                presets.len(),
                instruments.len()
            );
            if claimed != snapshots {
                failures.push(format!(
                    "{}: {} snapshots under no instrument",
                    library.name,
                    snapshots - claimed
                ));
            }
            // Restoring: snapshots over their loaded instrument, the
            // rest (UVI programs, instruments, multis) each on its own.
            let (mut read, mut left, mut values, mut off_total) = (0, 0, 0, 0);
            for (n, own) in &groups {
                let (mut rack, mut audio) = rack();
                let instrument = &presets[*n];
                let mut each = vec![instrument.clone()];
                each.extend(own.iter().map(|&k| presets[k].clone()));
                left += each.len().saturating_sub(limit);
                // `KONTRA_PRESET_ONLY`: the instrument and the presets whose
                // path holds one of these `;`-separated words.
                let only = std::env::var("KONTRA_PRESET_ONLY").ok();
                let wanted = |p: &&PathBuf| {
                    only.as_deref().is_none_or(|o| {
                        (*p == instrument && !own.is_empty())
                            || o.split(';').any(|w| p.to_string_lossy().contains(w))
                    })
                };
                for path in each.iter().filter(wanted).take(limit) {
                    read += 1;
                    match restore(&mut rack, &mut audio, path) {
                        Ok((off, saved)) => {
                            values += saved;
                            off_total += off.len();
                            if !off.is_empty() {
                                println!(
                                    "  {}: {} of {saved} values differ: {:?}",
                                    path.display(),
                                    off.len(),
                                    &off[..off.len().min(6)]
                                );
                            }
                        }
                        Err(e) => failures.push(format!("{}: {e}", path.display())),
                    }
                }
            }
            for path in (presets.iter()).filter(|p| has_ext(p, "nkm")).take(limit) {
                read += 1;
                let (mut rack, _audio) = rack();
                if let Err(e) = rack.load(0, path) {
                    failures.push(format!("{}: {e}", path.display()));
                }
                let notices = rack.take_notices();
                if !notices.is_empty() {
                    println!("  {}: {notices:?}", path.display());
                }
            }
            left += multis.saturating_sub(limit);
            println!(
                "  read {read}, left out {left}; {values} saved values, {off_total} not held after restore"
            );
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

fn has_ext(p: &Path, ext: &str) -> bool {
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext))
}
