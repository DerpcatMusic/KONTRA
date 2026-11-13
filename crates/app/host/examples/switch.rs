//! Instrument switches in one rack: load the files in turn into part 0, as the
//! plugin does (heads deferred), with the audio side rendering on its own
//! thread. Prints resident memory after each switch (a leak shows as growth
//! across repeats) and the audio thread's slowest blocks.
//! `cargo run --release -p kontra-host --example switch -- <file>...`
//! (`KONTRA_SWITCHES=n` loads n times, cycling through the files;
//! `KONTRA_PLAY_SECONDS=n` plays four-note chords for n s after each load and
//! prints resident memory again, as the memory example does.)

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use kontra_engine::{Event, HostNote, HostPattern};
use kontra_host::rack::{Rack, Store, StoreConfig};

const BLOCK: usize = 512;

/// Four-note chords every half second, wandering over the keyboard: the
/// events falling in the block that starts at frame `start`.
fn chords(start: u64) -> Vec<(u32, Event)> {
    let mut input = Vec::new();
    for n in 0..=(start + BLOCK as u64) / 24_000 {
        let base = 36 + (n * 7 % 49) as u8;
        for key in [0, 4, 7, 12].map(|k| base + k) {
            let note = HostNote {
                port: 0,
                channel: 0,
                key,
                id: -1,
                clap: true,
            };
            let (on, off) = (n * 24_000, n * 24_000 + 20_000);
            if (start..start + BLOCK as u64).contains(&on) {
                input.push(((on - start) as u32, Event::NoteOn { note, velocity: 0.8, tune: 0.0 }));
            }
            if (start..start + BLOCK as u64).contains(&off) {
                input.push(((off - start) as u32, Event::NoteOff(HostPattern {
                    port: 0,
                    channel: 0,
                    key: i32::from(key),
                    id: -1,
                    clap: true,
                })));
            }
        }
    }
    input.sort_by_key(|(at, _)| *at);
    input
}

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
    let play = Arc::new(AtomicBool::new(false));
    let audio_thread = {
        let (stop, play) = (Arc::clone(&stop), Arc::clone(&play));
        std::thread::spawn(move || {
            let mut frame = 0u64;
            let (mut slowest, mut cpu) = (Vec::new(), Vec::new());
            while !stop.load(Ordering::Relaxed) {
                let (start, on) = (Instant::now(), on_cpu_ns());
                let input = if play.load(Ordering::Relaxed) {
                    chords(frame)
                } else {
                    Vec::new()
                };
                frame += BLOCK as u64;
                audio.process(BLOCK, &input);
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
        let after_load = rss_mb();
        let played = std::env::var("KONTRA_PLAY_SECONDS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok());
        if let Some(seconds) = played {
            play.store(true, Ordering::Relaxed);
            for _ in 0..seconds {
                std::thread::sleep(Duration::from_secs(1));
                rack.tick();
            }
            play.store(false, Ordering::Relaxed);
            println!("  after {seconds} s of chords: rss {} MB (load {after_load} MB)", rss_mb());
        }
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
