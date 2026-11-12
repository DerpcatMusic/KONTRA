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

/// A snapshot kept in Kontakt's user folder (`User Content/<instrument>/`)
/// loads as the same snapshot in its library does, its base found among
/// the instruments the player has.
#[test]
fn a_user_snapshot_binds_to_its_instrument_by_name() {
    let factory = Path::new(ANALOG);
    let nki =
        Path::new("/data/main/Libraries/Kontakt/ANALOG STRINGS/Instruments/ANALOG STRINGS.nki");
    if !factory.exists() || !nki.exists() {
        return;
    }
    let root = std::env::temp_dir().join(format!("kontra-user-content-{}", std::process::id()));
    let user = root.join("User Content/ANALOG STRINGS/9001 Punch.nksn");
    std::fs::create_dir_all(user.parent().unwrap()).unwrap();
    std::fs::copy(factory, &user).unwrap();
    let known = [Path::new("/elsewhere/Piano.nki").to_owned(), nki.to_owned()];
    let alone = kontra_format_kontakt::load(&user);
    let found = kontra_format_kontakt::preset(&user, &known).map(|p| p.base);
    let loaded = kontra_format_kontakt::load_known(&user, &known);
    std::fs::remove_dir_all(&root).unwrap();
    assert!(alone.is_err(), "no base near it");
    assert_eq!(found.unwrap(), nki);
    // What a snapshot sets: its base, script values and effects.
    let set = |i: &kontra_ir::Instrument| {
        let groups: Vec<_> = i.groups.iter().map(|g| &g.inserts).collect();
        let saved: Vec<_> = i.scripts.iter().map(|s| &s.saved).collect();
        format!(
            "{:?}",
            (
                &i.name,
                &i.source_path,
                saved,
                groups,
                &i.inserts,
                &i.sends,
                &i.main,
                &i.buses
            )
        )
    };
    let original = kontra_format_kontakt::load(factory).unwrap();
    assert_eq!(set(&loaded.unwrap()), set(&original));
}
