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
        auto: false,
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

/// Values a restore leaves different by design, by library and saved
/// name, and why. Any other difference fails the survey.
const EXPECTED: &[(&str, &[&str], &str)] = &[
    (
        "Conflux",
        &[
            "$MAX_KEY",
            "$portamento",
            "$portamento_knob",
            "$mono",
            "$legato",
            "$mono_mode_knob",
            "$glide_abs",
            "$glide_rel",
            "$porta_combi_knob",
            "$auto",
        ],
        "its Mono & Glide script takes these from the PGS keys the main script sets after the snapshot's values land",
    ),
    (
        "Conflux",
        &["!gui__fx__param_labels", "!gui__fx__param_shortnames"],
        "label tables its `on init` refills for the effects it loads, over the saved strings",
    ),
];

/// A saved value's name: Kontakt's `name value`, UVI's `name=value`.
fn name_of(saved: &str) -> &str {
    saved.split([' ', '=']).next().unwrap_or(saved)
}

/// Whether `held` holds what `saved` saved.
fn holds(held: &str, saved: &str) -> bool {
    // An array the file saved shorter than the script now declares keeps
    // its saved cells and the declared rest: the saved ones count.
    let prefix = held.starts_with(saved) && held.as_bytes().get(saved.len()) == Some(&b' ');
    // A UVI widget saves its value as an f32, written in either notation
    // (`6.3656972e-07`, `6.365697e-7`): the same f32 once parsed, exactly.
    let same_f32 = match (held.split_once('='), saved.split_once('=')) {
        (Some((hn, hv)), Some((sn, sv))) => {
            hn == sn && matches!((hv.parse::<f32>(), sv.parse::<f32>()), (Ok(x), Ok(y)) if x == y)
        }
        _ => false,
    };
    held == saved || prefix || same_f32
}

/// One saved value the restored part holds differently.
struct Off {
    slot: usize,
    saved: String,
    held: Option<String>,
}

/// What `path`, applied to part 0, leaves its script slots holding that
/// its file saved differently, each slot against its own: (those values,
/// how many it saved).
fn restore(
    rack: &mut Rack,
    audio: &mut RackAudio,
    path: &Path,
) -> Result<(Vec<Off>, usize), String> {
    rack.load(0, path).map_err(|e| e.to_string())?;
    settle(rack, audio);
    let held = rack.saved_scripts(0);
    let file = kontra_host::instrument::read_saved(path, &[], &[]).map_err(|e| e.to_string())?;
    if held.len() != file.scripts.len() {
        return Err(format!(
            "{} script slots held, {} saved",
            held.len(),
            file.scripts.len()
        ));
    }
    resaved(rack, audio, path, &held)?;
    let mut off = Vec::new();
    let mut count = 0;
    for (script, held) in file.scripts.iter().zip(&held) {
        count += script.saved.len();
        for v in script
            .saved
            .iter()
            .filter(|v| !held.iter().any(|h| holds(h, v)))
        {
            off.push(Off {
                slot: script.slot,
                saved: v.clone(),
                held: held.iter().find(|h| name_of(h) == name_of(v)).cloned(),
            });
        }
    }
    Ok((off, count))
}

/// `KONTRA_PRESET_SAVE`: a folder to save each restored snapshot into.
/// The saved file, loaded again, must hold `held` and the same instrument.
fn resaved(
    rack: &mut Rack,
    audio: &mut RackAudio,
    path: &Path,
    held: &[Vec<String>],
) -> Result<(), String> {
    let Some(dir) = std::env::var_os("KONTRA_PRESET_SAVE").map(PathBuf::from) else {
        return Ok(());
    };
    if !has_ext(path, "nksn") {
        return Ok(());
    }
    let to = dir.join("resaved.nksn");
    rack.save_snapshot(0, &to)
        .map_err(|e| format!("save: {e}"))?;
    let base = kontra_format_kontakt::preset(path, &[])
        .map_err(|e| e.to_string())?
        .base;
    // The instrument but its saved values, which hold what `on init` made.
    let shape = |p: &Path| -> Result<String, String> {
        let mut ir = kontra_host::instrument::read_saved(p, &[], std::slice::from_ref(&base))
            .map_err(|e| format!("{}: {e}", p.display()))?;
        (ir.name, ir.source_path, ir.library_path) = (String::new(), None, None);
        ir.scripts.iter_mut().for_each(|s| s.saved.clear());
        Ok(format!("{ir:?}"))
    };
    if shape(path)? != shape(&to)? {
        return Err("resaved: the instrument differs".into());
    }
    let ir = kontra_host::instrument::read_saved(&to, &[], std::slice::from_ref(&base))
        .map_err(|e| e.to_string())?;
    rack.load_ir(0, ir, Some(to.clone()))
        .map_err(|e| e.to_string())?;
    settle(rack, audio);
    let again = rack.saved_scripts(0);
    std::fs::remove_file(&to).map_err(|e| e.to_string())?;
    match (held.iter().flatten()).find(|h| !again.iter().flatten().any(|a| a == *h)) {
        Some(lost) => Err(format!("resaved: {lost:.120} is not held again")),
        None => Ok(()),
    }
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
    let detail = std::env::var_os("KONTRA_PRESET_DETAIL").is_some();
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
            let owner = |n: usize| kontra_ui_model::preset_owner(&library.dir, &presets[n]);
            // Each snapshot under the first instrument it names, once.
            let mut groups: Vec<(usize, Vec<usize>)> =
                instruments.iter().map(|&n| (n, Vec::new())).collect();
            let mut unclaimed = 0;
            for k in (0..presets.len()).filter(|&k| has_ext(&presets[k], "nksn")) {
                let mine = owner(k);
                match (groups.iter_mut()).find(|(n, _)| mine.is_some() && owner(*n) == mine) {
                    Some((_, own)) => own.push(k),
                    None => unclaimed += 1,
                }
            }
            let snapshots = (presets.iter()).filter(|p| has_ext(p, "nksn")).count();
            println!(
                "{}: {} presets, {} instruments, {snapshots} snapshots ({} under their instrument), {multis} multis",
                library.name,
                presets.len(),
                instruments.len(),
                snapshots - unclaimed,
            );
            if unclaimed > 0 {
                failures.push(format!(
                    "{}: {unclaimed} snapshots under no instrument",
                    library.name
                ));
            }
            // The header lists each instrument with exactly its own.
            for (n, own) in &groups {
                let family = kontra_ui_model::preset_family(&library.dir, presets, Some(*n));
                let listed = (family.iter()).filter(|&&k| has_ext(&presets[k], "nksn"));
                if !own.is_empty() && !listed.eq(own.iter()) {
                    failures.push(format!(
                        "{}: the header lists other snapshots",
                        presets[*n].display()
                    ));
                }
            }
            // Restoring: snapshots over their loaded instrument, the
            // rest (UVI programs, instruments, multis) each on its own.
            let (mut read, mut left, mut values, mut off_total, mut explained) = (0, 0, 0, 0, 0);
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
                            let expected = |o: &&Off| {
                                (EXPECTED.iter()).any(|(lib, names, _)| {
                                    library.name.contains(lib) && names.contains(&name_of(&o.saved))
                                })
                            };
                            let (known, unexplained): (Vec<&Off>, Vec<&Off>) =
                                off.iter().partition(expected);
                            explained += known.len();
                            off_total += unexplained.len();
                            for o in &unexplained {
                                failures.push(format!(
                                    "{} slot {}: saved {:.120}, held {:.120}",
                                    path.display(),
                                    o.slot,
                                    o.saved,
                                    o.held.as_deref().unwrap_or("(none)")
                                ));
                            }
                            if detail {
                                for o in &known {
                                    println!(
                                        "    expected, slot {}: saved {:.120}, held {:.120}",
                                        o.slot,
                                        o.saved,
                                        o.held.as_deref().unwrap_or("(none)")
                                    );
                                }
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
                "  read {read}, left out {left}; {values} saved values, {explained} differ as expected, {off_total} unexplained"
            );
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

fn has_ext(p: &Path, ext: &str) -> bool {
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext))
}

#[test]
fn a_restored_value_holds_exactly_its_saved_one() {
    // The same f32, however it is written; one f32 step off is a difference.
    assert!(holds("gain=6.3656972e-07", "gain=6.365697e-7"));
    assert!(!holds("gain=-1", "gain=-0.99999899"));
    assert!(!holds("pan=0.5", "gain=0.5"));
    // An array saved shorter than it is now declared.
    assert!(holds("%table 1 2 3 0", "%table 1 2 3"));
    assert!(!holds("%table 1 2 30", "%table 1 2 3"));
    assert_eq!(name_of("$mono 1"), "$mono");
    assert_eq!(name_of("gain=0.5"), "gain");
    // Every named exception says why.
    assert!(
        EXPECTED
            .iter()
            .all(|(_, names, why)| !names.is_empty() && !why.is_empty())
    );
}
