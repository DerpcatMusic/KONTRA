//! Load time and memory of one instrument: what the store pins (the preload
//! law's heads) against the whole process, for RAM work against Kontakt.
//! `cargo run --release -p kontra-host --example memory -- <file>...`

use kontra_stream::{Store, StoreConfig};

fn rss_mb() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let kb = |key: &str| {
        status
            .lines()
            .find(|l| l.starts_with(key))
            .and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok())
            .unwrap_or(0)
    };
    kb("VmRSS:") / 1024
}

/// Bytes this process has read: through read() calls, and from the disk.
fn io_mb() -> (u64, u64) {
    let io = std::fs::read_to_string("/proc/self/io").unwrap_or_default();
    let field = |key: &str| {
        io.lines()
            .find(|l| l.starts_with(key))
            .and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok())
            .unwrap_or(0)
    };
    (field("rchar:") >> 20, field("read_bytes:") >> 20)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    for path in std::env::args().skip(1) {
        let before = rss_mb();
        let io = io_mb();
        let start = std::time::Instant::now();
        let mut store = Store::new(StoreConfig::DEFAULT)?;
        let ir = kontra_host::instrument::read(path.as_ref())?;
        let read = start.elapsed();
        let read_io = io_mb();
        let (samples, zones) = (
            ir.samples.len(),
            ir.groups.iter().map(|g| g.zones.len()).sum::<usize>(),
        );
        if std::env::var_os("KONTRA_ZONES").is_some() {
            let zones = ir.groups.iter().flat_map(|g| &g.zones);
            let (mut modded, mut start_mod, mut frames) = (0, 0u64, 0u64);
            for z in zones {
                modded += usize::from(z.start_mod > 0);
                start_mod += z.start_mod;
                frames += ir
                    .samples
                    .get(z.sample)
                    .map_or(0, |s| s.frames * u64::from(s.channels));
            }
            let mut seen = std::collections::BTreeSet::new();
            let stored: u64 = (ir.samples.iter())
                .filter(|s| seen.insert((s.file.clone(), s.offset)))
                .filter_map(|s| s.len)
                .sum();
            println!(
                "  {} unique sample files, {:.1} MB stored",
                seen.len(),
                stored as f64 / 1e6
            );
            println!(
                "  {modded} zones with S.Mod, sum S.Mod {start_mod} frames, sum sample frames x channels {frames}"
            );
        }
        let loaded = kontra_host::instrument::build(ir, 48_000, &mut store);
        let pinned = store.pinned_bytes() as f64 / 1e6;
        println!(
            "{}: read {:.2} s, load {:.2} s; {samples} samples, {zones} zones ({} planned); pinned {pinned:.1} MB; rss +{} MB",
            loaded.name,
            read.as_secs_f64(),
            start.elapsed().as_secs_f64(),
            loaded.plan.zones.len(),
            rss_mb().saturating_sub(before)
        );
        let done = io_mb();
        println!(
            "  bytes read (calls / disk): format {} / {} MB, samples {} / {} MB",
            read_io.0 - io.0,
            read_io.1 - io.1,
            done.0 - read_io.0,
            done.1 - read_io.1
        );
    }
    Ok(())
}
