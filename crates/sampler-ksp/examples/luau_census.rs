//! KSP -> Luau census and `on init` equivalence over a directory of `.ksp` files.
//! Usage: cargo run -p sampler-ksp --example luau_census -- <dir>
//! For each script: front end (parse, resolve, interpreter `on init`), Luau subset
//! coverage (unsupported constructs counted by name), and cell/text/engine equality.
use sampler_ksp::luau::{check_init, lower};
use std::collections::BTreeMap;

fn main() {
    let dir = std::env::args().nth(1).expect("usage: luau_census <dir>");
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .expect("corpus dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "ksp"))
        .collect();
    paths.sort();
    let (mut frontend_err, mut outside, mut compared, mut matched) = (0usize, 0usize, 0usize, 0usize);
    let mut front_reasons: BTreeMap<String, usize> = BTreeMap::new();
    let mut unsupported: BTreeMap<String, usize> = BTreeMap::new();
    let mut mismatches = Vec::new();
    let mut single_blockers: BTreeMap<String, usize> = BTreeMap::new();
    let mut blocker_counts: BTreeMap<usize, usize> = BTreeMap::new();
    // LUAU_CENSUS_BLOCKERS=<file>: one line per outside script, `name<TAB>construct,...`.
    let mut dump = std::env::var_os("LUAU_CENSUS_BLOCKERS").map(|p| std::fs::File::create(p).unwrap());
    for path in &paths {
        let source = String::from_utf8_lossy(&std::fs::read(path).unwrap()).into_owned();
        let lowered = match lower(&source) {
            Ok(l) => l,
            Err(e) => {
                frontend_err += 1;
                let reason: String = e.split(',').next().unwrap_or("").chars().take(70).collect();
                *front_reasons.entry(reason).or_default() += 1;
                continue;
            }
        };
        for (name, n) in &lowered.compiled.unsupported {
            *unsupported.entry(name.clone()).or_default() += n;
        }
        if !lowered.compiled.is_complete() {
            outside += 1;
            // Scripts blocked by exactly one construct name: what the next widening unlocks.
            *blocker_counts.entry(lowered.compiled.unsupported.len()).or_default() += 1;
            if let Some(f) = dump.as_mut() {
                use std::io::Write;
                let names: Vec<&str> = lowered.compiled.unsupported.keys().map(String::as_str).collect();
                writeln!(f, "{}\t{}", path.file_name().unwrap().to_string_lossy(), names.join(",")).unwrap();
            }
            if lowered.compiled.unsupported.len() == 1 {
                let name = lowered.compiled.unsupported.keys().next().cloned().unwrap_or_default();
                *single_blockers.entry(name).or_default() += 1;
            }
            continue;
        }
        compared += 1;
        match check_init(&lowered) {
            Ok(_) => matched += 1,
            Err(e) => mismatches.push(format!("{}: {e}", path.file_name().unwrap().to_string_lossy())),
        }
    }
    println!("scripts: {}", paths.len());
    println!("front end errors (interpreter on-init fault or parse/resolve): {frontend_err}");
    for (reason, n) in front_reasons.iter().rev().take(8) {
        println!("  {n:5}  {reason}");
    }
    println!("compiled, outside the Luau subset: {outside}");
    println!("in subset, init compared: {compared}; matched {matched}; mismatched {}", mismatches.len());
    for m in mismatches.iter().take(12) {
        println!("  MISMATCH {m}");
    }
    let mut single: Vec<_> = single_blockers.into_iter().collect();
    single.sort_by(|a, b| b.1.cmp(&a.1));
    println!("outside scripts by number of distinct blocking constructs: {blocker_counts:?}");
    println!("scripts blocked by exactly one construct:");
    for (name, n) in single.iter().take(20) {
        println!("  {n:5}  {name}");
    }
    let mut top: Vec<_> = unsupported.into_iter().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1));
    println!("unsupported constructs (sites, all scripts):");
    for (name, n) in top.iter().take(40) {
        println!("  {n:6}  {name}");
    }
}
