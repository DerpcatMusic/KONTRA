//! Compile every KSP script in the installed Kontakt libraries and report
//! sizes against `sampler_ksp::Limits::LIBRARY`.
//!
//! `cargo run -p sampler-kontakt --release --example library_scan [dir]`
//! (default `$KONTRA_KONTAKT_LIBRARIES`).
use std::collections::{BTreeMap, HashSet};
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
    let measure = sampler_ksp::Limits {
        source_bytes: usize::MAX,
        instructions: usize::MAX,
        variables: usize::MAX,
        array_cells: usize::MAX,
    };
    let library = sampler_ksp::Limits::LIBRARY;
    let (mut unreadable, mut slots) = (0, 0);
    let mut seen = HashSet::new();
    let (mut compiled, mut over) = (0, 0);
    let mut failures = BTreeMap::<String, Vec<String>>::new();
    let mut largest: [(usize, String); 4] = Default::default();
    for path in &files {
        let kontakt = match sampler_kontakt::read(path) {
            Ok(k) => k,
            Err(_) => {
                unreadable += 1;
                continue;
            }
        };
        let env = sampler_ksp::Environment {
            groups: kontakt
                .instrument
                .groups
                .iter()
                .map(|g| g.name.clone())
                .collect(),
            ..Default::default()
        };
        for behavior in &kontakt.instrument.behaviors {
            slots += 1;
            if !seen.insert(behavior.source.clone()) {
                continue;
            }
            let label = format!(
                "{} / {}",
                path.strip_prefix(&root).unwrap_or(path).display(),
                behavior.name
            );
            match sampler_ksp::compile_with(&behavior.source, 48000, measure, &[], &env) {
                Ok(script) => {
                    compiled += 1;
                    let u = script.usage();
                    let used = [u.source_bytes, u.instructions, u.variables, u.array_cells];
                    let limit = [
                        library.source_bytes,
                        library.instructions,
                        library.variables,
                        library.array_cells,
                    ];
                    over += usize::from(used.iter().zip(limit).any(|(u, l)| *u > l));
                    for (max, n) in largest.iter_mut().zip(used) {
                        if n > max.0 {
                            *max = (n, label.clone());
                        }
                    }
                }
                Err(e) => failures
                    .entry(format!("{}:{}: {}", e.line, e.column, e.message))
                    .or_default()
                    .push(label),
            }
        }
    }
    let unique = seen.len();
    println!(
        "{} .nki files ({unreadable} unreadable), {slots} script slots, {unique} distinct scripts",
        files.len()
    );
    println!("compiled {compiled}/{unique}; {over} exceed Limits::LIBRARY");
    for ((n, label), name) in
        largest
            .iter()
            .zip(["source bytes", "instructions", "variables", "array cells"])
    {
        println!("  largest {name}: {n} ({label})");
    }
    for (error, scripts) in &failures {
        println!("  {} x {error}  e.g. {}", scripts.len(), scripts[0]);
    }
}
