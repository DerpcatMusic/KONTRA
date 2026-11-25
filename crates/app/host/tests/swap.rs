//! What a swap costs: scripts compiled, pinned sample bytes read and the
//! time until the part is in. The measurements on real libraries run with
//! `--ignored --nocapture --test-threads=1`: the counters are the process's.
#![cfg(test)]

use std::path::Path;
use std::time::Instant;

use kontra_host::instrument::script_compiles;
use kontra_host::rack::Rack;
use kontra_stream::{Store, StoreConfig, pinned_bytes_read};

/// One instrument of a library whose instruments share `source`.
fn sharing(name: &str, source: &str) -> kontra_engine::ir::Instrument {
    use kontra_engine::ir;
    ir::Instrument {
        name: name.into(),
        scripts: vec![ir::Script {
            name: "shared".into(),
            slot: 0,
            dialect: ir::Dialect::Ksp,
            source: source.into(),
            saved: Vec::new(),
            data: Vec::new(),
            links: Vec::new(),
            container: None,
            player: false,
        }],
        ..ir::Instrument::DEFAULT
    }
}

#[test]
fn another_instrument_with_the_same_script_compiles_nothing() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let source = "on init\n  declare ui_knob $swap_cache (0, 100, 1)\n  make_persistent($swap_cache)\nend on";
    let build = |store: &mut Store, name, source| {
        let before = script_compiles();
        let loaded = kontra_host::instrument::build(sharing(name, source), 48_000, store);
        assert_eq!(loaded.plan.scripts.len(), 1);
        script_compiles() - before
    };
    assert_eq!(build(&mut store, "Legato", source), 1);
    assert_eq!(build(&mut store, "Sustained", source), 0);
    assert_eq!(build(&mut store, "Other", "on init\nend on"), 1);
}

const K: &str = "/data/main/Libraries/Kontakt";
const U: &str = "/data/main/Libraries/UVI";

fn step(rack: &mut Rack, what: &str, path: &str) {
    let path = Path::new(path);
    let (c, b, t) = (script_compiles(), pinned_bytes_read(), Instant::now());
    let generation = |rack: &Rack| rack.parts()[0].as_ref().map(|p| p.script.generation());
    let installed = generation(rack);
    let revision = |rack: &Rack| {
        rack.parts()[0]
            .as_ref()
            .and_then(|p| p.view.as_ref())
            .map(|v| v.revision)
    };
    let before = revision(rack);
    rack.load(0, path).unwrap();
    eprintln!(
        "{what:<28} {:>8.1} ms  compiles {:>2}  read {:>10} B  install {:?}->{:?}  view {:?}->{:?}",
        t.elapsed().as_secs_f64() * 1e3,
        script_compiles() - c,
        pinned_bytes_read() - b,
        installed,
        generation(rack),
        before,
        revision(rack),
    );
}

#[test]
#[ignore = "real libraries; a measurement"]
fn what_each_swap_costs() {
    if !Path::new(K).exists() {
        return;
    }
    let store = Store::new(StoreConfig::DEFAULT).unwrap();
    let (mut rack, _audio) = Rack::new(48_000, store, 512);
    let as_ = format!("{K}/ANALOG STRINGS");
    let dolce = format!("{K}/Audio Imperia Dolce/Instruments/01 7 1st Violins");
    let ao = format!("{U}/UVI - Augmented Orchestra v1.1.2-R2R/Augmented Orchestra.ufs");
    let ufs = Path::new(&ao);
    let programs = kontra_format_uvi::programs(ufs).unwrap_or_default();
    eprintln!(
        "AO programs: {:?}",
        programs.iter().take(4).collect::<Vec<_>>()
    );
    step(
        &mut rack,
        "kontakt: first load",
        &format!("{as_}/Instruments/ANALOG STRINGS.nki"),
    );
    step(
        &mut rack,
        "kontakt: snapshot",
        &format!("{as_}/Snapshots/ANALOG STRINGS/Factory/9001 Punch.nksn"),
    );
    step(
        &mut rack,
        "kontakt: snapshot 2",
        &format!("{as_}/Snapshots/ANALOG STRINGS/Factory/A Bit Indie.nksn"),
    );
    step(
        &mut rack,
        "kontakt: other library",
        &format!("{dolce}/Dolce - 01 7 1st Violins - Legato.nki"),
    );
    step(
        &mut rack,
        "kontakt: same library",
        &format!("{dolce}/Dolce - 02 7 1st Violins - Sustained.nki"),
    );
    step(
        &mut rack,
        "kontakt: same again",
        &format!("{dolce}/Dolce - 03 7 1st Violins - Sustained Con Sordino.nki"),
    );
    let mut uvi = programs.iter().map(|p| format!("{ao}/{p}"));
    if let (Some(a), Some(b)) = (uvi.next(), uvi.next()) {
        step(&mut rack, "uvi: other library", &a);
        step(&mut rack, "uvi: same bank", &b);
    }
    step(
        &mut rack,
        "uvi: other bank",
        &format!("{U}/VWinds - Clarinets/VWinds-BbClarinet.ufs/Presets/Bb Clarinet.uvip"),
    );
}

#[test]
#[ignore = "real libraries; a measurement"]
fn where_a_swap_spends_its_time() {
    if !Path::new(K).exists() {
        return;
    }
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let dolce = format!("{K}/Audio Imperia Dolce/Instruments/01 7 1st Violins");
    let ao = format!(
        "{U}/UVI - Augmented Orchestra v1.1.2-R2R/Augmented Orchestra.ufs/Presets/00 Orchestra/01 Strings"
    );
    let mut held = Vec::new();
    for path in [
        format!("{dolce}/Dolce - 01 7 1st Violins - Legato.nki"),
        format!("{dolce}/Dolce - 02 7 1st Violins - Sustained.nki"),
        format!("{ao}/V Strings Bartok.uvip"),
        format!("{ao}/V Strings Col Legno.uvip"),
    ] {
        let t = Instant::now();
        let (instrument, timings) =
            kontra_format::timing::collect(|| kontra_host::instrument::read(Path::new(&path)));
        let total_read = t.elapsed();
        let loaded = kontra_host::instrument::build(instrument.unwrap(), 48_000, &mut store);
        eprintln!(
            "{:<50} read {:?} translate {:?} (all {total_read:?}) build {:?}",
            Path::new(&path).file_name().unwrap().to_string_lossy(),
            timings.read,
            timings.translate,
            loaded.timings,
        );
        let mut phases: Vec<(&str, std::time::Duration, usize)> = Vec::new();
        for &(phase, took) in &timings.phases {
            match phases.iter_mut().find(|p| p.0 == phase) {
                Some(p) => (p.1, p.2) = (p.1 + took, p.2 + 1),
                None => phases.push((phase, took, 1)),
            }
        }
        for (phase, took, n) in phases {
            eprintln!("    {phase:<40} {took:?} ({n}x)");
        }
        held.push(loaded);
    }
}

/// A snapshot the part cannot recall (here, into an empty part) decodes
/// its base and reads on the loader thread: `load` returns at once, and
/// a second snapshot superseding it is the one that lands.
#[test]
fn a_snapshot_load_reads_on_the_loader_thread() {
    use std::time::Duration;
    let factory = format!("{K}/ANALOG STRINGS/Snapshots/ANALOG STRINGS/Factory");
    let (first, latest) = (
        format!("{factory}/9001 Punch.nksn"),
        format!("{factory}/A Bit Indie.nksn"),
    );
    if !Path::new(&first).exists() {
        return;
    }
    let store = Store::new(StoreConfig::DEFAULT).unwrap();
    let (mut rack, _audio) = Rack::new(48_000, store, 512);
    rack.load_in_background();
    let asked = Instant::now();
    rack.load(0, Path::new(&first)).unwrap();
    rack.load(0, Path::new(&latest)).unwrap();
    assert!(
        asked.elapsed() < Duration::from_millis(200),
        "the control thread decodes nothing: {:?}",
        asked.elapsed()
    );
    assert!(rack.parts()[0].is_none() && rack.in_flight() == 1);
    let until = Instant::now() + Duration::from_secs(120);
    let mut failed = Vec::new();
    while rack.in_flight() > 0 && Instant::now() < until {
        failed.extend(rack.finish_loads().1);
        std::thread::yield_now();
    }
    failed.extend(rack.finish_loads().1);
    assert!(failed.is_empty(), "{failed:?}");
    let part = rack.parts()[0].as_ref().expect("the latest snapshot is in");
    assert_eq!(part.path.as_deref(), Some(Path::new(&latest)));
}
