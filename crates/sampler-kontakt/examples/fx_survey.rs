//! Tally active Kontakt effect modules across a library tree: instruments
//! and slots per module, by rack kind.
//!
//! `cargo run -p sampler-kontakt --release --example fx_survey [dir]`
//! (default `$KONTRA_KONTAKT_LIBRARIES`).
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn nkis(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            nkis(&path, out);
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("nki"))
        {
            out.push(path);
        }
    }
}

fn main() {
    let root = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("KONTRA_KONTAKT_LIBRARIES").ok())
        .expect("library directory or KONTRA_KONTAKT_LIBRARIES");
    let mut files = Vec::new();
    nkis(Path::new(&root), &mut files);
    files.sort();
    let (mut read, mut with_fx, mut with_ir) = (0, 0, 0);
    let mut caveats = BTreeMap::<String, BTreeSet<usize>>::new();
    // ALL=1: every unsupported feature, by name, with instruments and entries.
    let mut features = BTreeMap::<String, (BTreeSet<usize>, usize)>::new();
    // module -> (instruments, slots by rack kind)
    let mut modules = BTreeMap::<String, (BTreeSet<usize>, BTreeMap<&str, usize>)>::new();
    for (i, path) in files.iter().enumerate() {
        let Ok(kontakt) = sampler_kontakt::read(path) else {
            continue;
        };
        read += 1;
        let mut any = false;
        with_ir += usize::from(!kontakt.instrument.impulses.is_empty());
        for u in &kontakt.instrument.unsupported {
            if let Some(want) = std::env::var_os("VALUES") && u.feature == want.to_string_lossy() {
                println!("VALUE\t{}\t{}", u.value, u.location.split(' ').next().unwrap_or(""));
            }
            let f = features.entry(u.feature.clone()).or_default();
            f.0.insert(i);
            f.1 += 1;
            if std::env::var_os("LOOPS").is_some() && (u.feature.contains("loop")) {
                println!("LOOP\t{i}\t{}\t{}\t{}", u.feature, u.value, path.display());
            }
            if std::env::var_os("FILTERS").is_some()
                && u.feature == "effect"
                && u.value.starts_with("Filter")
            {
                let size = std::fs::metadata(path).map_or(0, |m| m.len());
                println!(
                    "FILTER\t{}\t{size}\t{}\t{}",
                    u.value,
                    path.display(),
                    u.location
                );
            }
            if u.feature.starts_with("Convolution") || u.feature == "impulse response" {
                caveats.entry(u.feature.clone()).or_default().insert(i);
            }
        }
        for u in kontakt
            .instrument
            .unsupported
            .iter()
            .filter(|u| u.feature == "effect")
        {
            any = true;
            let module = u.value.split_once(" v0x").map_or(&*u.value, |(m, _)| m);
            if std::env::var_os("FX_VALUES").is_some() {
                let rest = u.value.split_once(" v0x").map_or("", |(_, v)| v);
                println!("VALUE {module}\t{rest}");
            }
            let rack = if u.location.starts_with("group") {
                "group insert"
            } else if u.location.starts_with("bus") {
                "bus"
            } else if u.location.contains("send") {
                "instrument send"
            } else if u.location.contains("main") {
                "instrument main"
            } else {
                "instrument insert"
            };
            let entry = modules.entry(module.to_string()).or_default();
            entry.0.insert(i);
            *entry.1.entry(rack).or_default() += 1;
        }
        with_fx += usize::from(any);
    }
    println!(
        "{} files, {read} read, {with_fx} with active effects",
        files.len()
    );
    println!("{with_ir} instruments with a translated impulse response");
    for (feature, set) in caveats {
        println!("{:5} instruments  caveat: {feature}", set.len());
    }
    if std::env::var_os("ALL").is_some() {
        let mut rows: Vec<_> = features.iter().collect();
        rows.sort_by_key(|(_, (n, _))| std::cmp::Reverse(n.len()));
        for (feature, (n, entries)) in rows {
            println!("FEATURE\t{}\t{entries}\t{feature}", n.len());
        }
    }
    let mut rows: Vec<_> = modules.into_iter().collect();
    rows.sort_by_key(|(_, (instruments, _))| std::cmp::Reverse(instruments.len()));
    for (module, (instruments, racks)) in rows {
        println!("{:5} instruments  {module:24} {racks:?}", instruments.len());
    }
}
