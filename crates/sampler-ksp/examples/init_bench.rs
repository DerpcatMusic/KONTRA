//! Load cost of KSP scripts, per script: `initialize` (lex, parse, sema, lower and
//! the `on init` evaluation) and `compile_initialized` (the rest of compile), best of
//! `KSP_INIT_REPS` runs each (default 5). The fingerprint hashes the compiled model, warnings and entry
//! shape, so an optimization that changes compile output shows up as a changed column.
//! Usage: cargo run -p sampler-ksp --release --example init_bench [DIR]
use sampler_ksp::Limits;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Instant;


fn main() {
    let dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| format!("{}/.cache/ksp-corpus", std::env::var("HOME").unwrap()));
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .expect("corpus directory")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "ksp"))
        .collect();
    paths.sort();
    let limits = Limits::LIBRARY;
    let reps: usize = std::env::var("KSP_INIT_REPS").ok().and_then(|r| r.parse().ok()).unwrap_or(5);
    println!(
        "{:<52} {:>9} {:>9} {:>9} {:>18}",
        "script", "init ms", "rest ms", "total ms", "fingerprint"
    );
    let (mut init_total, mut rest_total) = (0.0, 0.0);
    for path in &paths {
        let bytes = std::fs::read(path).unwrap();
        let source = String::from_utf8_lossy(&bytes);
        let file = path.file_name().unwrap().to_string_lossy();
        let env = environment(path.parent().unwrap(), &source);
        let (mut init_best, mut rest_best) = (f64::MAX, f64::MAX);
        let mut fingerprint = None;
        for _ in 0..reps {
            let started = Instant::now();
            let initialized = match sampler_ksp::initialize(&source, limits, &env) {
                Ok(initialized) => initialized,
                Err(e) => {
                    println!("{file}: init: {e}");
                    break;
                }
            };
            let init = started.elapsed().as_secs_f64() * 1e3;
            let started = Instant::now();
            let script = match sampler_ksp::compile_initialized(&source, 48000, limits, &[], initialized) {
                Ok(script) => script,
                Err(e) => {
                    println!("{file}: compile: {e}");
                    break;
                }
            };
            let rest = started.elapsed().as_secs_f64() * 1e3;
            init_best = init_best.min(init);
            rest_best = rest_best.min(rest);
            if fingerprint.is_none() {
                let mut h = DefaultHasher::new();
                format!("{:?}", script.model()).hash(&mut h);
                script.warnings().len().hash(&mut h);
                script.entries().len().hash(&mut h);
                format!("{:?}", script.symbols()).hash(&mut h);
                fingerprint = Some(h.finish());
            }
        }
        if init_best == f64::MAX {
            continue;
        }
        init_total += init_best;
        rest_total += rest_best;
        println!(
            "{:<52} {:>9.2} {:>9.2} {:>9.2} {:>18x}",
            file.chars().take(52).collect::<String>(),
            init_best,
            rest_best,
            init_best + rest_best,
            fingerprint.unwrap_or(0)
        );
    }
    println!(
        "{:<52} {:>9.2} {:>9.2} {:>9.2}",
        "TOTAL",
        init_total,
        rest_total,
        init_total + rest_total
    );
}

/// Same environment as the `corpus` example: the script's `.nckp` performance view, if present.
fn environment(dir: &std::path::Path, source: &str) -> sampler_ksp::Environment {
    let mut env = sampler_ksp::Environment::default();
    if let Some(name) = sampler_ksp::nckp::view_name(source)
        && let Ok(bytes) = std::fs::read(dir.join(format!("{name}.nckp")))
        && let Ok((view, _)) = sampler_ksp::nckp::parse(&bytes)
    {
        env.performance_view = view;
    }
    env
}
