//! Measure the frontend on extracted Kontakt scripts (kept outside the repo).
//! Usage: cargo run -p sampler-ksp --release --example corpus [DIR]
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
    let (mut ok, mut total) = (0, 0);
    let verbose = std::env::var_os("KSP_VERBOSE").is_some();
    let mut symbols = std::collections::BTreeMap::<String, usize>::new();
    for path in &paths {
        let bytes = std::fs::read(path).unwrap();
        let source = String::from_utf8_lossy(&bytes);
        total += 1;
        let started = std::time::Instant::now();
        match sampler_ksp::analyze(&source) {
            Ok(summary) => {
                ok += 1;
                for s in &summary.symbols {
                    *symbols.entry(s.clone()).or_insert(0) += 1;
                }
                if verbose {
                    println!(
                        "{}: vars {} widgets {} fns {} callbacks {} depth {} warnings {}",
                        path.file_name().unwrap().to_string_lossy(),
                        summary.variables,
                        summary.widgets,
                        summary.functions,
                        summary.callbacks,
                        summary.call_depth,
                        summary.warnings
                    );
                }
            }
            Err(e) => println!("{}: {e}", path.file_name().unwrap().to_string_lossy()),
        }
        let elapsed = started.elapsed();
        if elapsed.as_millis() > 500 {
            println!("  slow {}: {elapsed:?}", path.file_name().unwrap().to_string_lossy());
        }
    }
    if verbose {
        println!("opaque vendor symbols: {}", symbols.len());
        for (name, count) in &symbols {
            println!("  {name} x{count}");
        }
    }
    println!("analyzed {ok}/{total}");
}
