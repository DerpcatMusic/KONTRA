//! Measure the frontend on extracted Kontakt scripts (kept outside the repo):
//! compile, bind on an empty instrument, and tally how call sites were lowered.
//! Usage: cargo run -p sampler-ksp --release --example corpus [DIR]
//! KSP_VERBOSE=1 adds per-script lines, coverage and opaque symbol tallies.
use sampler_ksp::{Coverage, Limits};
use std::collections::BTreeMap;

fn main() {
    let dir = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("KSP_CORPUS").ok())
        .unwrap_or_else(|| format!("{}/.cache/ksp-corpus", std::env::var("HOME").unwrap()));
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .expect("corpus directory")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "ksp"))
        .collect();
    paths.sort();
    let verbose = std::env::var_os("KSP_VERBOSE").is_some();
    let limits = Limits {
        source_bytes: usize::MAX,
        instructions: usize::MAX,
        variables: usize::MAX,
        array_cells: usize::MAX,
    };
    let (mut compiled, mut bound, mut total) = (0, 0, 0);
    let mut coverage = BTreeMap::<(&str, Coverage), usize>::new();
    let mut symbols = BTreeMap::<String, usize>::new();
    let mut warnings = 0;
    for path in &paths {
        let bytes = std::fs::read(path).unwrap();
        let source = String::from_utf8_lossy(&bytes);
        let file = path.file_name().unwrap().to_string_lossy();
        total += 1;
        let started = std::time::Instant::now();
        let script = match sampler_ksp::compile(&source, 48000, limits, &[]) {
            Ok(script) => script,
            Err(e) => {
                println!("{file}: compile: {e}");
                continue;
            }
        };
        compiled += 1;
        for &(name, c, n) in script.coverage() {
            *coverage.entry((name, c)).or_default() += n;
        }
        for s in script.symbols() {
            *symbols.entry(s.clone()).or_default() += 1;
        }
        warnings += script.warnings().len();
        let summary = format!(
            "{file}: programs {} controls {} widgets {} warnings {} in {:?}",
            script.entries().len(),
            script.controls().len(),
            script.model().interface.widgets.len(),
            script.warnings().len(),
            started.elapsed()
        );
        let plan = sampler_core::Prepared::new(48000, vec![], vec![], 0).unwrap();
        match script.bind(plan) {
            Ok(_) => {
                bound += 1;
                if verbose {
                    println!("{summary}");
                }
            }
            Err(e) => println!("{summary}: bind: {e:?}"),
        }
    }
    if verbose {
        println!("coverage (builtin, how, call sites):");
        for ((name, c), n) in &coverage {
            println!("  {name} {c:?} {n}");
        }
        println!("opaque vendor symbols: {}", symbols.len());
    }
    let mut by_kind = BTreeMap::<Coverage, usize>::new();
    for ((_, c), n) in &coverage {
        *by_kind.entry(*c).or_default() += n;
    }
    println!("call sites by lowering: {by_kind:?}; warnings {warnings}");
    println!("compiled {compiled}/{total}, bound {bound}/{total}");
}
