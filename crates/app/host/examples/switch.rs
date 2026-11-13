//! Instrument switches in one rack: load the files in turn into part 0, as the
//! plugin does (heads deferred), with the audio side rendering on its own
//! thread. Prints resident memory after each switch (a leak shows as growth
//! across repeats) and the audio thread's slowest blocks.
//! `cargo run --release -p kontra-host --example switch -- <file>...`
//! (`KONTRA_SWITCHES=n` loads n times, cycling through the files.)

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use kontra_host::rack::{Rack, Store, StoreConfig};

const BLOCK: usize = 512;

fn rss_mb() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    status
        .lines()
        .find(|l| l.starts_with("VmRSS:"))
        .and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok())
        .unwrap_or(0)
        / 1024
}

/// Nanoseconds this thread has spent on a CPU (not waiting for one).
fn on_cpu_ns() -> u64 {
    std::fs::read_to_string("/proc/thread-self/schedstat")
        .ok()
        .and_then(|s| s.split_whitespace().next()?.parse().ok())
        .unwrap_or(0)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    let switches = std::env::var("KONTRA_SWITCHES")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(5);
    let mut store = Store::new(StoreConfig::DEFAULT)?;
    store.defer_heads(true);
    let (mut rack, mut audio) = Rack::new(48_000, store, BLOCK);
    let stop = Arc::new(AtomicBool::new(false));
    let audio_thread = {
        let stop = Arc::clone(&stop);
        std::thread::spawn(move || {
            let (mut slowest, mut cpu) = (Vec::new(), Vec::new());
            while !stop.load(Ordering::Relaxed) {
                let (start, on) = (Instant::now(), on_cpu_ns());
                audio.process(BLOCK, &[]);
                slowest.push(start.elapsed());
                cpu.push(Duration::from_nanos(on_cpu_ns() - on));
                // A block's worth of real time, as a host calls it.
                std::thread::sleep(Duration::from_micros(10_000));
            }
            slowest.sort();
            cpu.sort();
            (slowest, cpu)
        })
    };
    println!("start: rss {} MB", rss_mb());
    for n in 0..switches {
        let Some(path) = paths.get(n % paths.len().max(1)) else {
            break;
        };
        let start = Instant::now();
        rack.load(0, path.as_ref())?;
        let loaded = start.elapsed();
        // Retired engines are dropped here, off the audio thread.
        rack.tick();
        std::thread::sleep(Duration::from_secs(3));
        rack.tick();
        println!(
            "switch {}: {:.2} s to load, rss {} MB, live pinned {} MB   {}",
            n + 1,
            loaded.as_secs_f64(),
            rss_mb(),
            rack.store_mut().pinned_bytes() >> 20,
            path.rsplit('/').next().unwrap_or(path)
        );
    }
    if switches == 0 {
        // No loads: the audio thread's own jitter on this machine.
        std::thread::sleep(Duration::from_secs(30));
    }
    stop.store(true, Ordering::Relaxed);
    let (slowest, cpu) = audio_thread.join().map_err(|_| "audio thread panicked")?;
    let over = |ms: u64| {
        slowest
            .iter()
            .filter(|t| **t > Duration::from_millis(ms))
            .count()
    };
    let cpu_over = |ms: u64| {
        cpu.iter()
            .filter(|t| **t > Duration::from_millis(ms))
            .count()
    };
    println!(
        "audio thread on-CPU: slowest {:?}, {} over 1 ms, {} over 5 ms",
        cpu.last().copied().unwrap_or_default(),
        cpu_over(1),
        cpu_over(5)
    );
    println!(
        "audio thread wall: {} blocks, slowest {:?}, {} over 1 ms, {} over 5 ms",
        slowest.len(),
        slowest.last().copied().unwrap_or_default(),
        over(1),
        over(5)
    );
    Ok(())
}
