//! Real instruments, skipped where the library is not installed.
#![cfg(test)]

use std::path::Path;

const AREIA: &str = "/data/main/Libraries/Kontakt/Areia 1.2.0 [Audio Imperia]/Instruments/01 Core Technique Patches/01 Areia - 16 Violins - Core Techniques.nki";

#[test]
fn areia_violins_load_with_their_samples_and_script() {
    let path = Path::new(AREIA);
    if !path.exists() {
        return;
    }
    let i = kontra_format_kontakt::load(path).unwrap();
    let zones: usize = i.groups.iter().map(|g| g.zones.len()).sum();
    eprintln!(
        "{}: {} groups, {} zones, {} samples, {} scripts, stream {:?}",
        i.name,
        i.groups.len(),
        zones,
        i.samples.len(),
        i.scripts.len(),
        i.groups.first().and_then(|g| g.stream)
    );
    for d in &i.diagnostics {
        eprintln!("  {:?} {}: {}", d.impact, d.feature, d.detail);
    }
    if let Some(s) = i.samples.first() {
        eprintln!("  first sample {s:?}");
    }
    assert!(zones > 0 && !i.samples.is_empty() && !i.scripts.is_empty());
    assert!(!i.diagnostics.iter().any(|d| d.feature == "missing sample"));
}

const ANALOG: &str =
    "/data/main/Libraries/Kontakt/ANALOG STRINGS/Snapshots/ANALOG STRINGS/Factory/9001 Punch.nksn";

#[test]
fn an_analog_strings_snapshot_is_its_instrument_with_its_saved_values() {
    let path = Path::new(ANALOG);
    if !path.exists() {
        return;
    }
    let snap = kontra_format_kontakt::load(path).unwrap();
    let base = Path::new("/data/main/Libraries/Kontakt/ANALOG STRINGS/Instruments");
    let nki = std::fs::read_dir(base).unwrap().flatten().map(|e| e.path());
    let nki = nki
        .into_iter()
        .find(|p| p.extension().is_some_and(|e| e == "nki"))
        .unwrap();
    let base = kontra_format_kontakt::load(&nki).unwrap();
    assert_eq!(snap.name, "9001 Punch");
    assert_eq!(snap.groups.len(), base.groups.len());
    let saved = |i: &kontra_ir::Instrument| {
        i.scripts
            .iter()
            .map(|s| s.saved.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(snap.scripts.len(), base.scripts.len());
    assert_ne!(saved(&snap), saved(&base));
}

/// The members of one archive list in its own order, the same on every
/// read: a hash index's order changes per map, so a listing of
/// `ir_samples` (a response's sample index) changed between two loads.
#[test]
fn members_list_in_the_archives_order_on_every_read() {
    let offsets = [
        700u32, 100, 900, 300, 500, 200, 800, 400, 600, 1000, 1100, 1200,
    ];
    let mut bytes = 0x5e70ac54u32.to_le_bytes().to_vec();
    bytes.extend(0x110u16.to_le_bytes());
    bytes.extend([0; 8]);
    bytes.extend((offsets.len() as u32).to_le_bytes());
    bytes.extend([0; 4]);
    for (n, offset) in offsets.iter().enumerate() {
        bytes.extend(16u16.to_le_bytes());
        bytes.extend(offset.to_le_bytes());
        bytes.extend(0u16.to_le_bytes());
        for c in format!("m{n:02}").encode_utf16().chain([0]) {
            bytes.extend(c.to_le_bytes());
        }
    }
    let listed = || -> Vec<(u64, String)> {
        let archive =
            ni_file::nkr::Archive::read_index(std::io::Cursor::new(bytes.clone())).unwrap();
        archive
            .members()
            .map(|e| (e.header_offset, e.name.clone()))
            .collect()
    };
    let first = listed();
    let mut sorted = first.clone();
    sorted.sort();
    assert_eq!(first, sorted, "by header offset");
    for _ in 0..8 {
        assert_eq!(listed(), first);
    }
}

/// An instrument reads the same every time: an A/B or a saved project
/// relies on its IR, sample order included, being the file's own.
#[test]
fn an_instrument_loads_the_same_twice() {
    let root = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../tools/kontakt-reference/scenarios"
    );
    for name in ["calibration.nki", "cutoff_mod.nki"] {
        let path = Path::new(root).join(name);
        let read = || format!("{:?}", kontra_format_kontakt::load(&path).unwrap());
        assert_eq!(read(), read(), "{name}");
    }
}
