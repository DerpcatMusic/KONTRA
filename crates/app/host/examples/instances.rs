//! Several plugin instances in one process, as a host opens a project: each
//! has a store of its own (sharing pinned pages process-wide) and a rack, and
//! loads one instrument with heads deferred. Prints each load's time, the
//! whole's wall time (all playable) and resident memory, current and peak.
//! `cargo run --release -p kontra-host --example instances -- <file>...`
//! (`KONTRA_SEQUENTIAL=1` loads one after another; `KONTRA_PRIVATE=1` leaves
//! every store's pages private, the behaviour before sharing.)

use std::path::Path;
use std::time::{Duration, Instant};

use kontra_host::rack::{Rack, Store, StoreConfig};

fn status_mb(key: &str) -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    status
        .lines()
        .find(|l| l.starts_with(key))
        .and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok())
        .unwrap_or(0)
        / 1024
}

fn instance(
    path: &Path,
    private: bool,
) -> Result<(Rack, f64), Box<dyn std::error::Error + Send + Sync>> {
    let mut store = Store::new(StoreConfig::DEFAULT)?;
    if !private {
        store.share_pages_in_process();
    }
    store.defer_heads(true);
    let (mut rack, audio) = Rack::new(48_000, store, 512);
    // Kept alive so the parts stay installed; never run here.
    std::mem::forget(audio);
    let start = Instant::now();
    rack.load(0, path).map_err(|e| e.to_string())?;
    Ok((rack, start.elapsed().as_secs_f64()))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    let sequential = std::env::var_os("KONTRA_SEQUENTIAL").is_some();
    let private = std::env::var_os("KONTRA_PRIVATE").is_some();
    println!("start: rss {} MB", status_mb("VmRSS:"));
    let wall = Instant::now();
    let mut racks = Vec::new();
    if sequential {
        for p in &paths {
            let (rack, secs) = instance(p.as_ref(), private).map_err(|e| e.to_string())?;
            println!(
                "  {:.2} s to load, rss {} MB   {}",
                secs,
                status_mb("VmRSS:"),
                p.rsplit('/').next().unwrap_or(p)
            );
            racks.push(rack);
        }
    } else {
        let results: Vec<_> = std::thread::scope(|s| {
            let jobs: Vec<_> = paths
                .iter()
                .map(|p| s.spawn(move || instance(p.as_ref(), private)))
                .collect();
            jobs.into_iter().map(|j| j.join()).collect()
        });
        for (p, r) in paths.iter().zip(results) {
            let (rack, secs) = r
                .map_err(|_| "instance thread panicked")?
                .map_err(|e| e.to_string())?;
            println!(
                "  {:.2} s to load   {}",
                secs,
                p.rsplit('/').next().unwrap_or(p)
            );
            racks.push(rack);
        }
    }
    println!(
        "all playable after {:.2} s; rss {} MB, peak {} MB",
        wall.elapsed().as_secs_f64(),
        status_mb("VmRSS:"),
        status_mb("VmHWM:")
    );
    // Let the deferred heads finish, then the settled figures.
    std::thread::sleep(Duration::from_secs(15));
    for rack in &mut racks {
        rack.tick();
    }
    println!(
        "settled: rss {} MB, peak {} MB",
        status_mb("VmRSS:"),
        status_mb("VmHWM:")
    );
    Ok(())
}
