//! Performance harness for the native core on real instruments.
//!
//! `sampler-perf run [--out FILE] [--only NAME,..] [--cell BLOCK,THREADS] [--seconds S]` plays fixed
//! note schedules through each scenario at 64, 128 and 512-frame blocks and at
//! 1, 2, 4 and N render threads, timing every block against its deadline and
//! counting hardware events (`perf stat`), memory, disk reads, audio-thread
//! allocations and idle wakeups. Results are JSON.
//! `sampler-perf compare BASE.json NEW.json [--threshold PERCENT]` flags
//! regressions. Run it on an otherwise idle machine; numbers under load are noise.

use sampler_core::{Frame, Limits, Runtime, Stealing, Threads};
use sampler_midi::{Ingress, Packets, TimedPacket, Version};
use serde_json::{Value, json};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const RATE: u32 = 48_000;
const BLOCKS: [usize; 3] = [64, 128, 512];
const EVENTS: &str = "cycles:u,instructions:u,cache-misses:u,LLC-load-misses:u,branch-misses:u,page-faults:u,context-switches";

struct Counting;
thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static CALLS: Cell<u64> = const { Cell::new(0) };
    static EVENT_ERRORS: Cell<u64> = const { Cell::new(0) };
}
// SAFETY: forwards every call unchanged to System; only counts on the thread that asked.
#[allow(unsafe_code, reason = "Counting allocator forwards unchanged to System to count audio-thread heap calls")]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.get() {
            CALLS.set(CALLS.get() + 1);
        }
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if COUNTING.get() {
            CALLS.set(CALLS.get() + 1);
        }
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static GLOBAL: Counting = Counting;

type Message = (f64, [u8; 3]);

enum Source {
    /// A Kontakt instrument, streamed as the host does.
    Kontakt(&'static str),
    /// A UVI program in a bank, decoded up front, played through its script host.
    Uvi(&'static str, &'static str),
}

struct Scenario {
    name: &'static str,
    source: Source,
    seconds: f64,
    notes: fn() -> Vec<Message>,
}

fn on_off(out: &mut Vec<Message>, key: u8, velocity: u8, start: f64, length: f64) {
    out.push((start, [0x90, key, velocity]));
    out.push((start + length, [0x80, key, 0]));
}

/// Dynamics and expression up: most string libraries scale level with CC1 and CC11.
fn expression() -> Vec<Message> {
    vec![(0.0, [0xb0, 1, 110]), (0.0, [0xb0, 11, 127])]
}

/// A 30-note chord every half second, held 1 s so two overlap (scripts size for 32 keys).
fn dense() -> Vec<Message> {
    let mut m = expression();
    for beat in 0..14 {
        for key in 40..70 {
            on_off(&mut m, key, 100, f64::from(beat) * 0.5, 1.0);
        }
    }
    m
}

/// A legato line over two held notes.
fn legato() -> Vec<Message> {
    let mut m = expression();
    on_off(&mut m, 36, 90, 0.0, 7.0);
    on_off(&mut m, 43, 90, 0.0, 7.0);
    for i in 0..28 {
        let key = 55 + [0, 2, 4, 5, 7, 5, 4, 2][i % 8];
        on_off(&mut m, key, 80 + (i % 5) as u8 * 8, i as f64 * 0.25, 0.3);
    }
    m
}

/// A new key every 0.2 s over five octaves: each starts a sample not yet played.
fn sweep() -> Vec<Message> {
    let mut m = expression();
    for i in 0..50 {
        on_off(&mut m, 36 + i as u8 + i as u8 / 2, 60 + (i % 7) as u8 * 9, f64::from(i) * 0.2, 1.2);
    }
    m
}

fn pad() -> Vec<Message> {
    let mut m = Vec::new();
    for key in [48, 55, 60, 64] {
        on_off(&mut m, key, 100, 0.0, 3.0);
    }
    m
}

const KONTAKT: &str = "KONTRA_KONTAKT_LIBRARIES";
const UVI: &str = "KONTRA_UVI_LIBRARIES";

const SCENARIOS: &[Scenario] = &[
    Scenario { name: "dense-strings", source: Source::Kontakt("Performance Samples Vista/Instruments/Vista - 5 Violins.nki"), seconds: 8.0, notes: dense },
    Scenario { name: "scripted-legato", source: Source::Kontakt("Pacific Ensemble Strings/Instruments/10 Cellos/Pacific - Ens Strings - 10 Cellos - Legato Sustains.nki"), seconds: 8.0, notes: legato },
    Scenario { name: "streaming-sweep", source: Source::Kontakt("Areia 1.2.0 [Audio Imperia]/Instruments/01 Core Technique Patches/01 Areia - 16 Violins - Core Techniques.nki"), seconds: 11.0, notes: sweep },
    Scenario { name: "convolution-pads", source: Source::Kontakt("ANALOG STRINGS/Instruments/ANALOG STRINGS.nki"), seconds: 8.0, notes: dense },
    Scenario { name: "uvi-scripted-pad", source: Source::Uvi("UVI - Augmented Orchestra v1.1.2-R2R/Augmented Orchestra.ufs", "PAD Angela.uvip"), seconds: 8.0, notes: pad },
];

fn root(var: &str, default: &str) -> PathBuf {
    std::env::var_os(var).map_or_else(|| default.into(), PathBuf::from)
}

fn proc_value(file: &str, key: &str) -> u64 {
    std::fs::read_to_string(file)
        .ok()
        .and_then(|s| s.lines().find_map(|l| l.strip_prefix(key)?.trim().split_whitespace().next()?.parse().ok()))
        .unwrap_or(0)
}

/// Voluntary context switches of every thread: each is a wakeup from a wait.
fn wakeups() -> u64 {
    std::fs::read_dir("/proc/self/task")
        .into_iter()
        .flatten()
        .flatten()
        .map(|t| proc_value(&format!("{}/status", t.path().display()), "voluntary_ctxt_switches:"))
        .sum()
}

enum Player {
    Midi { rt: Runtime, ingress: Ingress, horizon: Option<u32> },
    Uvi(Box<sampler_uvi::scripted::Player>),
}

struct Loaded {
    player: Player,
    stream_bytes: u64,
    impulses: usize,
    /// Reader threads and asset heads stay alive with the runtime.
    _keep: Option<Box<dyn std::any::Any>>,
}

fn fail(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn load(s: &Scenario, messages: &[Message]) -> Result<Loaded, String> {
    match &s.source {
        Source::Kontakt(rel) => {
            let path = root(KONTAKT, "/mnt/MAIN_STORAGE/Libraries/Kontakt").join(rel);
            let keys = messages.iter().map(|m| m.1[1]);
            let options = sampler_kontakt::Options {
                rate: RATE,
                keys: keys.clone().min().unwrap_or(0)..=keys.max().unwrap_or(127),
                library: Some(path.clone()),
                ..Default::default()
            };
            let streamed = sampler_kontakt::load_streamed(&path, &options, &Default::default(), |_| {}).map_err(fail)?;
            let sampler_kontakt::Streamed { loaded, cache, report, streamer, assets } = streamed;
            let impulses = loaded.instrument.impulses.len();
            let plan = loaded.plan;
            // Generous fixed capacities, so no event is refused for room.
            let limits = Limits { families: 4096, decisions: 4096, commands: 4096, behavior_fuel: std::env::var("PERF_FUEL").ok().and_then(|v| v.parse().ok()).unwrap_or(1 << 20), ..Limits::for_plan(&plan, 2048, 2048) };
            let horizon = (report.head_frames.max(sampler_core::PAGE_FRAMES) + 512) as u32;
            let mut rt = Runtime::new(plan, limits).map_err(fail)?.with_stream_cache(cache);
            rt.set_cold_starts(true);
            rt.set_voice_stealing(Some(Stealing::for_limits(RATE, limits.voices))).map_err(fail)?;
            let mut groups = [None; 16];
            groups[0] = Some(Version::Midi1);
            Ok(Loaded {
                player: Player::Midi { rt, ingress: Ingress::new(0, groups), horizon: Some(horizon) },
                stream_bytes: report.head_bytes as u64,
                impulses,
                _keep: Some(Box::new((streamer, assets))),
            })
        }
        Source::Uvi(bank, program) => {
            let bank = sampler_uvi::Bank::open(&root(UVI, "/mnt/MAIN_STORAGE/Libraries/UVI").join(bank)).map_err(fail)?;
            let options = sampler_kontakt::Options { rate: RATE, ..Default::default() };
            let program = sampler_uvi::load_program_scripted_with_options(&bank, program, &options).map_err(fail)?;
            let impulses = program.instrument.impulses.len();
            let limits = Limits { notes: 64, channels: 16, performances: 1, expressions: 64, families: 256, decisions: 256, voices: 512, commands: 256, behaviors: 16, behavior_fuel: 1 << 20, behavior_cells: 0, note_cells: 0 };
            let player = sampler_uvi::scripted::Player::new(program, limits, RATE).map_err(fail)?;
            Ok(Loaded { player: Player::Uvi(Box::new(player)), stream_bytes: 0, impulses, _keep: None })
        }
    }
}

/// Everything one cell reports besides the block times.
#[derive(Default)]
struct Cell_ {
    times: Vec<u64>,
    allocations: u64,
    peak: f32,
    /// Seconds of the first and last block with output above -80 dB.
    audible: Option<(f64, f64)>,
}

fn play(p: &mut Player, messages: &[Message], block: usize, frames: usize) -> Cell_ {
    let words: Vec<[u32; 1]> = messages
        .iter()
        .map(|&(_, [s, a, b])| [0x2000_0000 | u32::from(s) << 16 | u32::from(a) << 8 | u32::from(b)])
        .collect();
    let packets: Vec<(usize, TimedPacket<'_>)> = messages
        .iter()
        .zip(&words)
        .map(|(&(at, _), w)| {
            let packet = Packets::new(w).next().unwrap().unwrap();
            ((at * f64::from(RATE)).round() as usize, TimedPacket { offset: 0, packet })
        })
        .collect();
    let mut cell = Cell_ { times: Vec::with_capacity(frames / block + 1), allocations: 0, peak: 0.0, audible: None };
    let (mut buffer, mut next) = (vec![[0.0f32; 2]; block], 0);
    let mut batch: Vec<TimedPacket<'_>> = Vec::with_capacity(64);
    let start = Instant::now();
    for begin in (0..frames).step_by(block) {
        batch.clear();
        while next < packets.len() && packets[next].0 < begin + block {
            batch.push(TimedPacket { offset: packets[next].0 - begin, ..packets[next].1 });
            next += 1;
        }
        let before = CALLS.get();
        COUNTING.set(true);
        let t = Instant::now();
        render(p, &mut buffer, &batch, messages, begin, &mut next);
        let took = t.elapsed();
        COUNTING.set(false);
        cell.allocations += CALLS.get() - before;
        cell.times.push(took.as_nanos() as u64);
        let top = buffer.iter().flatten().fold(0.0f32, |m, x| m.max(x.abs()));
        cell.peak = cell.peak.max(top);
        if top > 1e-4 {
            let at = begin as f64 / f64::from(RATE);
            cell.audible = Some(cell.audible.map_or((at, at), |(first, _)| (first, at)));
        }
        // Real time: streamed pages arrive from reader threads, which an unpaced render starves.
        let due = start + Duration::from_secs_f64((begin + block) as f64 / f64::from(RATE));
        if let Some(wait) = due.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
    }
    cell
}

fn render(p: &mut Player, buffer: &mut [Frame], batch: &[TimedPacket<'_>], _: &[Message], _: usize, _: &mut usize) {
    match p {
        Player::Midi { rt, ingress, horizon } => {
            if let Some(h) = horizon {
                let _ = rt.service_streaming(*h);
            }
            let mut errors = 0;
            let _ = ingress.render(rt, buffer, batch, batch.len().max(64), |i, r| { if let Err(e) = r { errors += 1; if std::env::var_os("PERF_DEBUG").is_some() && errors < 4 && batch.len() > 0 { eprintln!("event {i} of block ({} events): {e:?}", batch.len()); } } });
            EVENT_ERRORS.set(EVENT_ERRORS.get() + errors);
            rt.flush_behaviors(|_, _, _| true);
            rt.flush_ended(|_| true);
        }
        Player::Uvi(player) => {
            // Script-host events are applied at block starts: coarse, but the same for every run.
            for packet in batch {
                let _ = packet;
            }
            let _ = player.render(buffer);
        }
    }
}

fn uvi_events(p: &mut Player, messages: &[Message], from: usize, to: usize) {
    if let Player::Uvi(player) = p {
        for &(at, [status, key, velocity]) in messages {
            let frame = (at * f64::from(RATE)).round() as usize;
            if (from..to).contains(&frame) {
                let _ = if status & 0xf0 == 0x90 { player.note_on(key, f64::from(velocity) / 127.0) } else { player.note_off(key) };
            }
        }
    }
}

fn stats(p: &Player) -> sampler_core::RuntimeStats {
    match p {
        Player::Midi { rt, .. } => rt.stats(),
        Player::Uvi(player) => player.runtime().stats(),
    }
}

fn percentile(sorted: &[u64], p: f64) -> u64 {
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

/// Counts hardware events of this process, while it runs `f`.
fn counted<T>(f: impl FnOnce() -> T) -> (T, Value) {
    let file = std::env::temp_dir().join(format!("sampler-perf-{}.csv", std::process::id()));
    let child = Command::new("perf")
        .args(["stat", "-x,", "-e", EVENTS, "-p", &std::process::id().to_string(), "-o"])
        .arg(&file)
        .stderr(Stdio::null())
        .spawn()
        .ok();
    std::thread::sleep(Duration::from_millis(300));
    let out = f();
    let mut counters = serde_json::Map::new();
    if let Some(mut child) = child {
        let _ = Command::new("kill").args(["-INT", &child.id().to_string()]).status();
        let _ = child.wait();
        for line in std::fs::read_to_string(&file).unwrap_or_default().lines() {
            let f: Vec<&str> = line.split(',').collect();
            if let (Some(value), Some(event)) = (f.first().and_then(|v| v.parse::<f64>().ok()), f.get(2)) {
                counters.insert(event.trim_end_matches(":u").to_string(), json!(value));
            }
        }
        let _ = std::fs::remove_file(&file);
    }
    if let (Some(c), Some(i)) = (counters.get("cycles").and_then(Value::as_f64), counters.get("instructions").and_then(Value::as_f64)) {
        counters.insert("ipc".into(), json!(i / c.max(1.0)));
    }
    (out, Value::Object(counters))
}

fn run_scenario(s: &Scenario, seconds: Option<f64>, cores: usize, only_cell: Option<(usize, usize)>) -> Result<(Vec<Value>, Value), String> {
    let messages = {
        let mut m = (s.notes)();
        m.sort_by(|a, b| a.0.total_cmp(&b.0).then((a.1[0] & 0xf0 == 0x90).cmp(&(b.1[0] & 0xf0 == 0x90))));
        m
    };
    let started = Instant::now();
    let rss0 = proc_value("/proc/self/status", "VmRSS:");
    let mut loaded = load(s, &messages)?;
    let load_info = json!({
        "scenario": s.name,
        "load_seconds": started.elapsed().as_secs_f64(),
        "rss_kb_after_load": proc_value("/proc/self/status", "VmRSS:"),
        "rss_kb_added": proc_value("/proc/self/status", "VmRSS:") as i64 - rss0 as i64,
        "stream_head_bytes": loaded.stream_bytes,
        "impulses": loaded.impulses,
    });
    // Idle: nothing playing, threads parked.
    let (w0, t0) = (wakeups(), Instant::now());
    std::thread::sleep(Duration::from_secs(3));
    let idle = (wakeups() - w0) as f64 / t0.elapsed().as_secs_f64();
    let frames = (seconds.unwrap_or(s.seconds) * f64::from(RATE)) as usize;
    let mut threads = vec![1, 2, 4, cores];
    threads.sort_unstable();
    threads.dedup();
    threads.retain(|&t| t <= cores);
    let mut cells = Vec::new();
    for &t in &threads {
        for &block in &BLOCKS {
            if only_cell.is_some_and(|(b, n)| (b, n) != (block, t)) {
                continue;
            }
            // UVI plays on one thread; scaling is measured on the Kontakt scenarios.
            if matches!(s.source, Source::Uvi(..)) && t > 1 {
                continue;
            }
            // A fresh instrument per cell: held notes and script state of one cell never leak into the next.
            drop(loaded);
            loaded = load(s, &messages)?;
            if let Player::Midi { rt, .. } = &mut loaded.player {
                rt.set_threads(Threads::Fixed(t));
            }
            // Let threads and caches settle on a second of silence.
            if matches!(loaded.player, Player::Uvi(_)) {
                play_uvi(&mut loaded.player, &[], block, RATE as usize);
            } else {
                play(&mut loaded.player, &[], block, RATE as usize);
            }
            EVENT_ERRORS.set(0);
            let (rss, io0, rchar0) = (proc_value("/proc/self/status", "VmRSS:"), proc_value("/proc/self/io", "read_bytes:"), proc_value("/proc/self/io", "rchar:"));
            let (cell, counters) = counted(|| {
                if matches!(loaded.player, Player::Uvi(_)) {
                    play_uvi(&mut loaded.player, &messages, block, frames)
                } else {
                    play(&mut loaded.player, &messages, block, frames)
                }
            });
            let mut times = cell.times.clone();
            times.sort_unstable();
            let deadline = (block as f64 / f64::from(RATE) * 1e9) as u64;
            let st = stats(&loaded.player);
            cells.push(json!({
                "scenario": s.name, "block": block, "threads": t,
                "blocks": times.len(),
                "deadline_us": deadline as f64 / 1e3,
                "p50_us": percentile(&times, 0.5) as f64 / 1e3,
                "p99_us": percentile(&times, 0.99) as f64 / 1e3,
                "max_us": *times.last().unwrap() as f64 / 1e3,
                "misses": times.iter().filter(|&&t| t > deadline).count(),
                "audio_thread_allocations": cell.allocations, "event_errors": EVENT_ERRORS.replace(0), "peak": cell.peak, "audible_seconds": cell.audible,
                "perf": counters,
                "rss_kb": proc_value("/proc/self/status", "VmRSS:"),
                "rss_kb_added": proc_value("/proc/self/status", "VmRSS:") as i64 - rss as i64,
                "disk_read_bytes": proc_value("/proc/self/io", "read_bytes:") - io0,
                "read_calls_bytes": proc_value("/proc/self/io", "rchar:") - rchar0,
                "stream_cache_bytes": st.stream_cache_bytes,
                "stream_underruns": st.stream_underruns,
                "voice_capacity": st.voice_capacity, "voice_drops": st.voice_drops,
            }));
            eprintln!("{} block {block} threads {t}: p50 {:.0} us p99 {:.0} us max {:.0} us", s.name, percentile(&times, 0.5) as f64 / 1e3, percentile(&times, 0.99) as f64 / 1e3, *times.last().unwrap() as f64 / 1e3);
        }
    }
    Ok((cells, json!({ "load": load_info, "idle_wakeups_per_second": idle })))
}

/// [`play`] for a script-hosted program: notes go to the player between blocks.
fn play_uvi(p: &mut Player, messages: &[Message], block: usize, frames: usize) -> Cell_ {
    let mut cell = Cell_ { times: Vec::with_capacity(frames / block + 1), allocations: 0, peak: 0.0, audible: None };
    let mut buffer = vec![[0.0f32; 2]; block];
    let start = Instant::now();
    for begin in (0..frames).step_by(block) {
        uvi_events(p, messages, begin, begin + block);
        let before = CALLS.get();
        COUNTING.set(true);
        let t = Instant::now();
        render(p, &mut buffer, &[], messages, begin, &mut 0);
        let took = t.elapsed();
        COUNTING.set(false);
        cell.allocations += CALLS.get() - before;
        cell.times.push(took.as_nanos() as u64);
        let top = buffer.iter().flatten().fold(0.0f32, |m, x| m.max(x.abs()));
        cell.peak = cell.peak.max(top);
        if top > 1e-4 {
            let at = begin as f64 / f64::from(RATE);
            cell.audible = Some(cell.audible.map_or((at, at), |(first, _)| (first, at)));
        }
        // Real time: streamed pages arrive from reader threads, which an unpaced render starves.
        let due = start + Duration::from_secs_f64((begin + block) as f64 / f64::from(RATE));
        if let Some(wait) = due.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
    }
    cell
}

fn run(args: &[String]) -> Result<(), String> {
    let (mut out, mut only, mut seconds, mut cell) = (None, None, None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let v = it.next().ok_or(format!("{a} needs a value"))?;
        match a.as_str() {
            "--out" => out = Some(PathBuf::from(v)),
            "--only" => only = Some(v.split(',').map(String::from).collect::<Vec<_>>()),
            "--cell" => {
                let (b, t) = v.split_once(',').ok_or("--cell BLOCK,THREADS")?;
                cell = Some((b.parse::<usize>().map_err(fail)?, t.parse::<usize>().map_err(fail)?));
            }
            "--seconds" => seconds = Some(v.parse::<f64>().map_err(fail)?),
            _ => return Err(format!("unknown option {a}")),
        }
    }
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    let (mut cells, mut extra, mut skipped) = (Vec::new(), Vec::new(), Vec::new());
    for s in SCENARIOS.iter().filter(|s| only.as_ref().is_none_or(|o| o.iter().any(|n| n == s.name))) {
        match run_scenario(s, seconds, cores, cell) {
            Ok((c, e)) => {
                cells.extend(c);
                extra.push(e);
            }
            Err(e) => {
                eprintln!("{}: skipped: {e}", s.name);
                skipped.push(json!({ "scenario": s.name, "reason": e }));
            }
        }
    }
    let model = std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|c| c.lines().find_map(|l| l.strip_prefix("model name")?.split(':').nth(1).map(|s| s.trim().to_string())))
        .unwrap_or_default();
    let result = json!({
        "format": 1, "rate": RATE, "cores": cores, "cpu": model,
        "commit": Command::new("git").args(["rev-parse", "--short", "HEAD"]).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()),
        "load_and_idle": extra, "skipped": skipped, "cells": cells,
    });
    let text = serde_json::to_string_pretty(&result).map_err(fail)?;
    match out {
        Some(path) => std::fs::write(path, text).map_err(fail),
        None => {
            println!("{text}");
            Ok(())
        }
    }
}

/// Metrics where a rise is a regression, and whether noise needs the threshold.
const WATCHED: &[(&str, &str)] = &[("p50_us", "p50"), ("p99_us", "p99"), ("rss_kb", "resident")];

fn key(c: &Value) -> String {
    format!("{} block {} threads {}", c["scenario"].as_str().unwrap_or("?"), c["block"], c["threads"])
}

fn compare(args: &[String]) -> Result<bool, String> {
    let [base, new, rest @ ..] = args else { return Err("usage: compare BASE.json NEW.json [--threshold PERCENT]".into()) };
    let threshold = match rest {
        [] => 10.0,
        [flag, v] if flag == "--threshold" => v.parse::<f64>().map_err(fail)?,
        _ => return Err("usage: compare BASE.json NEW.json [--threshold PERCENT]".into()),
    };
    let read = |p: &String| -> Result<Value, String> { serde_json::from_str(&std::fs::read_to_string(p).map_err(fail)?).map_err(fail) };
    let (base, new) = (read(base)?, read(new)?);
    let empty = Vec::new();
    let old_cells = base["cells"].as_array().unwrap_or(&empty);
    let mut regressions = 0;
    for c in new["cells"].as_array().unwrap_or(&empty) {
        let Some(o) = old_cells.iter().find(|o| key(o) == key(c)) else {
            println!("{}: new cell, no baseline", key(c));
            continue;
        };
        let mut notes = Vec::new();
        let mut check = |label: &str, old: f64, now: f64| {
            if old > 0.0 && (now - old) / old * 100.0 > threshold {
                notes.push(format!("{label} {old:.1} -> {now:.1} (+{:.0}%)", (now - old) / old * 100.0));
            }
        };
        for (field, label) in WATCHED {
            check(label, o[field].as_f64().unwrap_or(0.0), c[field].as_f64().unwrap_or(0.0));
        }
        for event in ["cycles", "instructions", "cache-misses", "branch-misses"] {
            check(event, o["perf"][event].as_f64().unwrap_or(0.0), c["perf"][event].as_f64().unwrap_or(0.0));
        }
        let more = |f: &str| c[f].as_f64().unwrap_or(0.0) > o[f].as_f64().unwrap_or(0.0);
        for (f, label) in [("misses", "deadline misses"), ("audio_thread_allocations", "audio-thread allocations"), ("stream_underruns", "underruns")] {
            if more(f) {
                notes.push(format!("{label} {} -> {}", o[f], c[f]));
            }
        }
        if !notes.is_empty() {
            regressions += 1;
            println!("REGRESSION {}: {}", key(c), notes.join("; "));
        }
    }
    println!("{regressions} regressed cells (threshold {threshold}%)");
    Ok(regressions == 0)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("run") => run(&args[1..]).map(|()| true),
        Some("compare") => compare(&args[1..]),
        _ => Err("usage: sampler-perf run [--out FILE] [--only NAME,..] [--cell BLOCK,THREADS] [--seconds S] | compare BASE.json NEW.json [--threshold PERCENT]".into()),
    };
    match result {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(e) => {
            eprintln!("sampler-perf: {e}");
            std::process::exit(2);
        }
    }
}
