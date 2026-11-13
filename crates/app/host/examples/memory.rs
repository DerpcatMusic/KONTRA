//! Load time and memory of one instrument: what the store pins (the preload
//! law's heads) against the whole process, for RAM work against Kontakt.
//! `cargo run --release -p kontra-host --example memory -- <file>...`
//! With `KONTRA_PLAY_SECONDS=60` it then plays chords for that many seconds
//! of audio and reports the process again.

use std::sync::Arc;

use kontra_engine::{Engine, Event, HostNote, HostPattern, SampleTime, Timed};
use kontra_stream::{Store, StoreConfig};

/// `RssAnon` and `RssFile` in MB: heap and anonymous memory against mapped file pages.
fn rss_split() -> (u64, u64) {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let kb = |key: &str| {
        status
            .lines()
            .find(|l| l.starts_with(key))
            .and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok())
            .unwrap_or(0)
    };
    (kb("RssAnon:") / 1024, kb("RssFile:") / 1024)
}

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

/// CPU seconds this process has used, user + system, all threads: stable
/// under machine load, unlike wall time.
fn cpu_s() -> f64 {
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
    let fields: Vec<&str> = stat
        .rsplit_once(')')
        .map_or("", |(_, rest)| rest)
        .split_whitespace()
        .collect();
    let ticks = |i: usize| {
        fields
            .get(i)
            .and_then(|f| f.parse::<f64>().ok())
            .unwrap_or(0.0)
    };
    (ticks(11) + ticks(12)) / 100.0
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    for path in std::env::args().skip(1) {
        let before = rss_mb();
        let io = io_mb();
        let (start, cpu) = (std::time::Instant::now(), cpu_s());
        let mut store = Store::new(StoreConfig::DEFAULT)?;
        eprintln!("  rss after store {} MB", rss_mb().saturating_sub(before));
        let ir = kontra_host::instrument::read(path.as_ref())?;
        eprintln!("  rss after read {} MB", rss_mb().saturating_sub(before));
        let (read, read_cpu) = (start.elapsed(), cpu_s() - cpu);
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
            "{}: read {:.2} s (cpu {:.2} s), load {:.2} s (cpu {:.2} s); {samples} samples, {zones} zones ({} planned); pinned {pinned:.1} MB; rss +{} MB (anon {}, file {})",
            loaded.name,
            read.as_secs_f64(),
            read_cpu,
            start.elapsed().as_secs_f64(),
            cpu_s() - cpu,
            loaded.plan.zones.len(),
            rss_mb().saturating_sub(before),
            rss_split().0,
            rss_split().1
        );
        if let Some(seconds) = std::env::var("KONTRA_PLAY_SECONDS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
        {
            let loaded_rss = rss_mb();
            let mut engine = Engine::new(Arc::new(loaded.plan), store.client(), 48_000);
            play(&mut engine, seconds);
            println!(
                "  after {seconds} s of chords: rss {} MB (+{} MB since load); {:?}",
                rss_mb(),
                rss_mb().saturating_sub(loaded_rss),
                store.client().stats()
            );
        }
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

/// Four-note chords every half second, wandering over the keyboard, for
/// `seconds` of audio, paced so the stream keeps up.
fn play(engine: &mut Engine, seconds: u64) {
    const BLOCK: u64 = 512;
    let note = |key: i32| HostPattern {
        port: 0,
        channel: 0,
        key,
        id: -1,
        clap: true,
    };
    let mut block = vec![[0.0f32; 2]; BLOCK as usize];
    for start in (0..seconds * 48_000).step_by(BLOCK as usize) {
        let mut input = Vec::new();
        for n in 0..=(start + BLOCK) / 24_000 {
            let (on, base) = (n * 24_000, 36 + (n * 7 % 49) as i32);
            for key in [0, 4, 7, 12] {
                let at = |t: u64| SampleTime(t);
                let pattern = note(base + key);
                let host = HostNote {
                    port: 0,
                    channel: 0,
                    key: (base + key) as u8,
                    id: -1,
                    clap: true,
                };
                input.push(Timed {
                    at: at(on),
                    body: Event::NoteOn {
                        note: host,
                        velocity: 0.8,
                        tune: 0.0,
                    },
                });
                input.push(Timed {
                    at: at(on + 20_000),
                    body: Event::NoteOff(pattern),
                });
            }
        }
        // Only this block's events, in time order.
        input.retain(|e| (start..start + BLOCK).contains(&e.at.0));
        input.sort_by_key(|e| e.at.0);
        engine.process(&mut block, &input);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}
