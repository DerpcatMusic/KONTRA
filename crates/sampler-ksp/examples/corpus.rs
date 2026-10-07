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
    let limits = Limits::LIBRARY;
    let (mut compiled, mut bound, mut total) = (0, 0, 0);
    let mut coverage = BTreeMap::<(&str, Coverage), usize>::new();
    let mut symbols = BTreeMap::<String, usize>::new();
    let mut warnings = 0;
    // Warning text with digits blanked, so one gap counts once per script set.
    let mut kinds = BTreeMap::<String, BTreeMap<String, usize>>::new();
    let (mut ui_ok, mut widgets, mut unsupported) = (0, 0, BTreeMap::<String, usize>::new());
    for path in &paths {
        let bytes = std::fs::read(path).unwrap();
        let source = String::from_utf8_lossy(&bytes);
        let file = path.file_name().unwrap().to_string_lossy();
        total += 1;
        let started = std::time::Instant::now();
        let script = match sampler_ksp::compile_with(
            &source,
            48000,
            limits,
            &[],
            &environment(path.parent().unwrap(), &source),
        ) {
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
        for w in script.warnings() {
            let kind: String = format!("{}: {}", w.builtin.unwrap_or("-"), w.message)
                .chars()
                .map(|c| if c.is_ascii_digit() { '#' } else { c })
                .collect();
            *kinds
                .entry(kind)
                .or_default()
                .entry(file.to_string())
                .or_default() += 1;
        }
        match script.ui(&|_| None) {
            Ok(ui) => {
                ui_ok += 1;
                widgets += ui.widgets.len();
                for u in &ui.unsupported {
                    *unsupported.entry(u.feature.clone()).or_default() += 1;
                }
            }
            Err(e) => println!("{file}: ui: {e}"),
        }
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
        println!("warning kinds ({}):", kinds.len());
        for (k, files) in &kinds {
            let files: Vec<_> = files
                .iter()
                .map(|(f, n)| format!("{}:{n}", &f[..8]))
                .collect();
            println!("  KIND {k} @@ {}", files.join(","));
        }
        println!("opaque vendor symbols: {}", symbols.len());
        let mut top: Vec<_> = unsupported.iter().collect();
        top.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
        println!("unsupported UI features (top 25):");
        for (f, n) in top.iter().take(25) {
            println!("  {f} {n}");
        }
    }
    let mut by_kind = BTreeMap::<Coverage, usize>::new();
    for ((_, c), n) in &coverage {
        *by_kind.entry(*c).or_default() += n;
    }
    println!("call sites by lowering: {by_kind:?}; warnings {warnings}");
    println!(
        "ui ir {ui_ok}/{compiled} valid, {widgets} widgets, {} unsupported entries",
        unsupported.values().sum::<usize>()
    );
    println!("compiled {compiled}/{total}, bound {bound}/{total}");
}

/// The script's performance view from `<dir>/<name>.nckp`, if present.
fn environment(dir: &std::path::Path, source: &str) -> sampler_ksp::Environment {
    let mut env = sampler_ksp::Environment::default();
    if let Some(name) = sampler_ksp::nckp::view_name(source)
        && let Ok(bytes) = std::fs::read(dir.join(format!("{name}.nckp")))
    {
        match sampler_ksp::nckp::parse(&bytes) {
            Ok((view, _)) => env.performance_view = view,
            Err(e) => println!("{name}.nckp: {e}"),
        }
    }
    env
}
