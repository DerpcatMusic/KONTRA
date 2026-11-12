//! What a UVI program's Lua costs on the audio thread: per-block engine and
//! Lua time, arena allocations, Luau GC steps and arena lock waits, with an
//! optional second thread compiling another program in a loop.
//! `cargo run --release -p kontra-host --example lua_cost -- <program> [<contender>]`
//! Plays 4-note chords, 4 blocks apart and held 8, for `KONTRA_BLOCKS` blocks
//! (default 2000), as fast as it can (no real-time pacing).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use kontra_engine::{Engine, Event, HostNote, HostPattern, Prepared, SampleTime, Timed};
use kontra_lua::probe::{self, Probe};
use kontra_stream::{Store, StoreConfig};

const RATE: u32 = 48_000;
const BLOCK: usize = 512;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let path: PathBuf = args
        .next()
        .ok_or("usage: lua_cost <program> [<contender>]")?
        .into();
    let contender: Option<PathBuf> = args.next().map(Into::into);
    let blocks: usize = std::env::var("KONTRA_BLOCKS")
        .ok()
        .and_then(|b| b.parse().ok())
        .unwrap_or(2000);

    let mut store = Store::new(StoreConfig::DEFAULT)?;
    let loaded = kontra_host::instrument::load(&path, RATE, &mut store)?;
    let plan = Arc::new(loaded.plan);
    let mut engine = Engine::new(Arc::clone(&plan), store.client(), RATE);
    let mut out = vec![[0.0f32; 2]; BLOCK];
    let settle = |engine: &mut Engine, out: &mut [[f32; 2]]| {
        engine.process(out, &[]);
        let mut asked = Vec::new();
        engine.drain_requests(|r| asked.push(r));
        for r in asked {
            engine.install(Prepared::new(r, Some(&plan), RATE));
        }
    };
    for _ in 0..200 {
        settle(&mut engine, &mut out);
    }
    store.client().settle(std::time::Duration::from_secs(10));

    // The contender compiles its Lua in a loop until the run ends.
    let stop = Arc::new(AtomicBool::new(false));
    let loads = Arc::new(AtomicU32::new(0));
    let contender = contender.map(|p| {
        let ir = kontra_host::instrument::read(&p).ok();
        let (stop, loads) = (Arc::clone(&stop), Arc::clone(&loads));
        std::thread::spawn(move || {
            let scripts: Vec<_> = ir.map(|ir| ir.scripts).unwrap_or_default();
            while !stop.load(Ordering::Relaxed) {
                for s in &scripts {
                    let modules: Vec<(String, String)> = (s.data.iter())
                        .map(|(n, b)| (n.clone(), String::from_utf8_lossy(b).into_owned()))
                        .collect();
                    drop(kontra_lua::compile(&s.source, &modules, &s.saved));
                }
                loads.fetch_add(1, Ordering::Relaxed);
            }
        })
    });

    let _ = probe::take();
    let keys = [48u8, 52, 55, 60, 50, 53, 57, 62, 47, 50, 55, 59];
    let mut rows: Vec<(u64, Probe)> = Vec::with_capacity(blocks);
    let mut events = Vec::new();
    for b in 0..blocks {
        events.clear();
        let chord = |n: usize| keys.iter().skip((n % 3) * 4).take(4).copied();
        if b % 4 == 0 {
            for key in chord(b / 4) {
                let note = HostNote {
                    port: 0,
                    channel: 0,
                    key,
                    id: -1,
                    clap: true,
                };
                let on = Event::NoteOn {
                    note,
                    velocity: 0.8.into(),
                    tune: 0.0,
                };
                events.push(Timed {
                    at: SampleTime(0),
                    body: on,
                });
            }
        }
        if b % 4 == 0 && b >= 8 {
            for key in chord(b / 4 - 2) {
                let off = HostPattern {
                    port: 0,
                    channel: 0,
                    key: i32::from(key),
                    id: -1,
                    clap: true,
                };
                events.push(Timed {
                    at: SampleTime(0),
                    body: Event::NoteOff(off),
                });
            }
        }
        let t = std::time::Instant::now();
        engine.process(&mut out, &events);
        let ns = u64::try_from(t.elapsed().as_nanos()).unwrap_or(u64::MAX);
        rows.push((ns, probe::take()));
    }
    stop.store(true, Ordering::Relaxed);
    if let Some(c) = contender {
        let _ = c.join();
    }

    let pick = |f: &dyn Fn(&(u64, Probe)) -> u64| {
        let mut v: Vec<u64> = rows.iter().map(f).collect();
        v.sort_unstable();
        let at = |q: f64| {
            v.get(((v.len() as f64 - 1.0) * q) as usize)
                .copied()
                .unwrap_or(0)
        };
        (at(0.5), at(0.99), v.last().copied().unwrap_or(0))
    };
    let us = |n: u64| n as f64 / 1e3;
    let row = |name: &str, (p50, p99, max): (u64, u64, u64), scale: &dyn Fn(u64) -> f64| {
        println!(
            "| {name} | {:.1} | {:.1} | {:.1} |",
            scale(p50),
            scale(p99),
            scale(max)
        );
    };
    let total = rows.iter().fold(Probe::default(), |mut t, (_, p)| {
        t.runs += p.runs;
        t.allocs += p.allocs;
        t.bytes += p.bytes;
        t.gc_steps += p.gc_steps;
        t.gc_ns += p.gc_ns;
        t.waits += p.waits;
        t.wait_ns += p.wait_ns;
        t
    });
    let one = |n: u64| n as f64;
    println!(
        "{}: {blocks} blocks of {BLOCK}, contender loads {}",
        path.display(),
        loads.load(Ordering::Relaxed)
    );
    println!("| per block | p50 | p99 | max |\n|---|---:|---:|---:|");
    row("engine µs", pick(&|r| r.0), &us);
    row("Lua run µs", pick(&|r| r.1.run_ns), &us);
    row("Lua callback max µs", pick(&|r| r.1.run_ns_max), &us);
    row("callbacks", pick(&|r| r.1.runs), &one);
    row("arena allocs", pick(&|r| r.1.allocs), &one);
    row("arena bytes", pick(&|r| r.1.bytes), &one);
    row("GC steps", pick(&|r| r.1.gc_steps), &one);
    row("GC step max µs", pick(&|r| r.1.gc_ns_max), &us);
    row("lock waits", pick(&|r| r.1.waits), &one);
    row("lock wait max µs", pick(&|r| r.1.wait_ns_max), &us);
    println!(
        "totals: {} callbacks, {} allocs ({} B), {} GC steps ({:.1} ms), {} lock waits ({:.1} µs), spills {}",
        total.runs,
        total.allocs,
        total.bytes,
        total.gc_steps,
        total.gc_ns as f64 / 1e6,
        total.waits,
        us(total.wait_ns),
        kontra_lua::alloc::spills()
    );
    Ok(())
}
