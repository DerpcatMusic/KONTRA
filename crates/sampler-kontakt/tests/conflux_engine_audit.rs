//! Installed-source counts only; never print script/saved-entry/sample payloads.
use std::{collections::BTreeMap, path::Path};

const ROOT: &str = "/mnt/MAIN_STORAGE/Libraries/Kontakt/Conflux 1.1.0 [Native Instruments]";
fn snapshot_paths(path: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(path).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() { snapshot_paths(&path, out); }
        else if path.extension().is_some_and(|e| e == "nksn") { out.push(path); }
    }
}
#[test]
#[ignore = "installed Conflux snapshot audit"]
fn all_conflux_snapshots_decode_saved_engine_and_slot_state() {
    let mut paths = Vec::new();
    snapshot_paths(&Path::new(ROOT).join("Snapshots"), &mut paths);
    paths.sort();
    assert_eq!(paths.len(), 201);
    let mut census = BTreeMap::new();
    for path in &paths {
        let state = sampler_kontakt::read_snapshot(path).unwrap();
        *census.entry((state.groups.len(), state.effects.len(), state.persistent.iter().map(Vec::len).sum::<usize>())).or_insert(0) += 1;
    }
    println!("SNAPSHOT_SHAPES {census:?}");
}
