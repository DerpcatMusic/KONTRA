//! Scans library roots as the browser does and prints each library with its
//! instrument count, then the scan's time, cold and with the cache.
//! `cargo run --release -p kontra-host --example library -- <root>...`

use std::time::Instant;

use kontra_library::{Cache, Progress, Root};

fn main() {
    let roots: Vec<Root> = std::env::args_os()
        .skip(1)
        .map(|p| Root {
            path: p.into(),
            single: false,
        })
        .collect();
    let programs =
        |bank: &std::path::Path| kontra_format_uvi::programs(bank).map_err(|e| e.to_string());
    let mut cache = Cache::default();
    for pass in ["cold", "cached"] {
        let start = Instant::now();
        let Some(catalog) =
            kontra_library::scan(&roots, &Progress::default(), &mut cache, &programs)
        else {
            return;
        };
        let ms = start.elapsed().as_secs_f64() * 1e3;
        if pass == "cold" {
            for l in &catalog.libraries {
                let art = l
                    .art
                    .first()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                println!(
                    "{:40} {:24} {:?} {:5} presets {:3} multis  {art}",
                    l.name,
                    l.vendor,
                    l.format,
                    l.presets.len(),
                    l.multis
                );
            }
            for issue in &catalog.issues {
                println!("issue {}: {}", issue.path.display(), issue.reason);
            }
        }
        println!(
            "{pass}: {} libraries in {ms:.1} ms ({} files read)",
            catalog.libraries.len(),
            cache.reads
        );
        cache.reads = 0;
    }
}
