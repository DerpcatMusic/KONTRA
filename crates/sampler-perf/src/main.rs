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
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

mod capacity;
mod stress;
mod sustain;
use sustain::cpu_ns;

const RATE: u32 = 48_000;
const BLOCKS: [usize; 3] = [64, 128, 512];
const EVENTS: &str = "cycles:u,instructions:u,cache-misses:u,LLC-load-misses:u,branch-misses:u,page-faults:u,context-switches";

struct Counting;
thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static CALLS: Cell<u64> = const { Cell::new(0) };
    static EVENT_ERRORS: Cell<u64> = const { Cell::new(0) };
    static SERVICE_ERRORS: Cell<u64> = const { Cell::new(0) };
    static SERVICE_MASK: Cell<u8> = const { Cell::new(0) };
    static STREAM_SERVICE_ERRORS: Cell<u64> = const { Cell::new(0) };
    static BEHAVIOR_ERRORS: Cell<u64> = const { Cell::new(0) };
    /// Set while the allocator itself captures a trace, so those calls are not sampled.
    static TRACING: Cell<bool> = const { Cell::new(false) };
}
/// Voice-pool capacity for the next [`load`]; `capacity` sweeps it.
static VOICE_CAP: AtomicU64 = AtomicU64::new(2048);
/// `PERF_ALLOC_TRACE=N`: capture a backtrace of every Nth counted heap call (0 = off).
static TRACE_EVERY: AtomicU64 = AtomicU64::new(0);
static TRACE_SEEN: AtomicU64 = AtomicU64::new(0);
static TRACES: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Records one sampled backtrace. Runs inside the allocator, so it allocates only under `TRACING`.
fn sample_trace() {
    let every = TRACE_EVERY.load(Ordering::Relaxed);
    if every == 0 || TRACING.get() || TRACE_SEEN.fetch_add(1, Ordering::Relaxed) % every != 0 {
        return;
    }
    TRACING.set(true);
    let text = std::backtrace::Backtrace::force_capture().to_string();
    if let Ok(mut traces) = TRACES.lock() {
        traces.push(text);
    }
    TRACING.set(false);
}

// SAFETY: forwards every call unchanged to System; only counts on the thread that asked.
#[allow(
    unsafe_code,
    reason = "Counting allocator forwards unchanged to System to count audio-thread heap calls"
)]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.get() {
            CALLS.set(CALLS.get() + 1);
            sample_trace();
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
    KontaktProgram(&'static str, usize),
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

/// One 16-note chord held for 4 s: the "16 voices" cell of the Areia perf lane.
fn chord16() -> Vec<Message> {
    let mut m = expression();
    for i in 0..16 {
        on_off(&mut m, 48 + i, 100, 0.0, 4.0);
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
    let n = std::env::var("PERF_SWEEP_N")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(50);
    for i in 0..n {
        on_off(
            &mut m,
            36 + i as u8 + i as u8 / 2,
            60 + (i % 7) as u8 * 9,
            f64::from(i) * 0.2,
            1.2,
        );
    }
    m
}

/// Shared with the real-Kontakt benchmark (tools/oracle/cpu_bench.py): I-vi-IV-V, 16 voices per chord,
/// 2 s per chord, so 8 s total with 16 sounding voices at a time.
fn progression16() -> Vec<Message> {
    const ROOTS: [u8; 4] = [36, 33, 41, 43];
    const VOICING: [u8; 16] = [0, 4, 7, 12, 16, 19, 24, 28, 31, 34, 36, 40, 43, 46, 48, 52];
    let mut m = expression();
    for (chord, root) in ROOTS.iter().enumerate() {
        for v in VOICING {
            on_off(&mut m, root + v, 100, chord as f64 * 2.0, 2.0);
        }
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
    Scenario {
        name: "dense-strings",
        source: Source::Kontakt("Performance Samples Vista/Instruments/Vista - 5 Violins.nki"),
        seconds: 8.0,
        notes: dense,
    },
    Scenario {
        name: "scripted-legato",
        source: Source::Kontakt(
            "Pacific Ensemble Strings/Instruments/10 Cellos/Pacific - Ens Strings - 10 Cellos - Legato Sustains.nki",
        ),
        seconds: 8.0,
        notes: legato,
    },
    Scenario {
        name: "streaming-sweep",
        source: Source::Kontakt(
            "Areia 1.2.0 [Audio Imperia]/Instruments/01 Core Technique Patches/01 Areia - 16 Violins - Core Techniques.nki",
        ),
        seconds: 11.0,
        notes: sweep,
    },
    Scenario {
        name: "areia-full-ens-chord",
        source: Source::Kontakt(
            "Areia 1.2.0 [Audio Imperia]/Instruments/03 Single Patches/01 Core Techniques/07 Full Ens/Areia - 04 Full Ens - Spiccato Slow.nki",
        ),
        seconds: 6.0,
        notes: chord16,
    },
    Scenario {
        name: "conflux-load",
        source: Source::Kontakt("Conflux 1.1.0 [Native Instruments]/Instruments/Conflux.nki"),
        seconds: 4.0,
        notes: dense,
    },
    Scenario {
        name: "convolution-pads",
        source: Source::Kontakt("ANALOG STRINGS/Instruments/ANALOG STRINGS.nki"),
        seconds: 8.0,
        notes: dense,
    },
    Scenario {
        name: "cpu-conflux",
        source: Source::Kontakt("Conflux 1.1.0 [Native Instruments]/Instruments/Conflux.nki"),
        seconds: 8.0,
        notes: progression16,
    },
    Scenario {
        name: "cpu-areia-full-ens",
        source: Source::Kontakt(
            "Areia 1.2.0 [Audio Imperia]/Instruments/03 Single Patches/01 Core Techniques/07 Full Ens/Areia - 04 Full Ens - Spiccato Slow.nki",
        ),
        seconds: 8.0,
        notes: progression16,
    },
    Scenario {
        name: "cpu-analog-strings",
        source: Source::Kontakt("ANALOG STRINGS/Instruments/ANALOG STRINGS.nki"),
        seconds: 8.0,
        notes: progression16,
    },
    Scenario {
        name: "uvi-scripted-pad",
        source: Source::Uvi(
            "UVI - Augmented Orchestra v1.1.2-R2R/Augmented Orchestra.ufs",
            "Presets/04 Hybrid/Asak Dancer.uvip",
        ),
        seconds: 8.0,
        notes: pad,
    },
];

fn root(var: &str, default: &str) -> PathBuf {
    std::env::var_os(var).map_or_else(|| default.into(), PathBuf::from)
}

fn proc_value(file: &str, key: &str) -> u64 {
    std::fs::read_to_string(file)
        .ok()
        .and_then(|s| {
            s.lines().find_map(|l| {
                l.strip_prefix(key)?
                    .trim()
                    .split_whitespace()
                    .next()?
                    .parse()
                    .ok()
            })
        })
        .unwrap_or(0)
}

/// Voluntary context switches of every thread: each is a wakeup from a wait.
fn wakeups() -> u64 {
    std::fs::read_dir("/proc/self/task")
        .into_iter()
        .flatten()
        .flatten()
        .map(|t| {
            proc_value(
                &format!("{}/status", t.path().display()),
                "voluntary_ctxt_switches:",
            )
        })
        .sum()
}

enum Player {
    Midi {
        rt: Runtime,
        ingress: Ingress,
        horizon: Option<u32>,
    },
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

/// Below this peak (-60 dBFS) a scenario counts as silent.
const SILENT: f32 = 0.001;

fn load(s: &Scenario, messages: &[Message], block: usize) -> Result<Loaded, String> {
    match &s.source {
        Source::Kontakt(rel) | Source::KontaktProgram(rel, _) => {
            let path = root(KONTAKT, "/data/main/Libraries/Kontakt").join(rel);
            let keys = messages
                .iter()
                .filter(|m| m.1[0] & 0xf0 == 0x90)
                .map(|m| m.1[1]);
            let options = sampler_kontakt::Options {
                rate: RATE,
                keys: keys.clone().min().unwrap_or(0)..=keys.max().unwrap_or(127),
                library: Some(path.clone()),
                ..Default::default()
            };
            let kontakt = match s.source {
                Source::KontaktProgram(_, index) => sampler_kontakt::read_program(&path, index),
                _ => sampler_kontakt::read(&path),
            }
            .map_err(fail)?;
            if std::env::var_os("PERF_METADATA").is_some() {
                sustain::metadata(s.name, &kontakt.instrument);
            }
            let streamed = sampler_kontakt::load_read_streamed(
                kontakt,
                &options,
                &sampler_kontakt::StreamPolicy {
                    voices: std::env::var("PERF_STREAM_VOICES")
                        .ok()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(2048),
                    block_frames: block,
                    ..Default::default()
                },
                |_| {},
            )
            .map_err(fail)?;
            let sampler_kontakt::Streamed {
                loaded,
                cache,
                report,
                streamer,
                assets,
            } = streamed;
            if std::env::var_os("PERF_METADATA").is_some() {
                for view in &loaded.scripts {
                    let callbacks: Vec<_> = (0..view.program_count())
                        .filter_map(|p| view.callback(p))
                        .collect();
                    eprintln!(
                        "script slot {}: {} programs, callbacks {:?}, listeners {:?}, init controller writes {:?}",
                        view.slot(),
                        view.program_count(),
                        callbacks,
                        view.model().listeners,
                        view.model().controllers
                    );
                }
            }
            let impulses = loaded.instrument.impulses.len();
            let plan = loaded.plan;
            // Generous fixed capacities, so no event is refused for room.
            let limits = Limits {
                families: 4096,
                decisions: 4096,
                commands: 4096,
                behavior_fuel: std::env::var("PERF_FUEL")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(1 << 20),
                behaviors: std::env::var("PERF_BEHAVIORS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(2048),
                ..Limits::for_plan(&plan, 2048, VOICE_CAP.load(Ordering::Relaxed) as usize)
            };
            // Dense chords re-trigger scripts faster than their release tails end: Limits::script_capacity (about 4 per stage per key) is too small for 30 notes every half beat.
            let limits = Limits {
                behavior_cells: plan.behavior_local_count().saturating_mul(limits.behaviors),
                ..limits
            };
            if std::env::var_os("PERF_DEBUG").is_some() {
                eprintln!(
                    "limits: behaviors {} behavior_cells {} voices {} (stages {}, locals {})",
                    limits.behaviors,
                    limits.behavior_cells,
                    limits.voices,
                    plan.stage_count(),
                    plan.behavior_local_count()
                );
            }
            let horizon = (report.head_frames.max(sampler_core::PAGE_FRAMES) + 512) as u32;
            eprintln!("stream policy: head_frames {} pool_bytes {} latency_p50_us {} latency_p95_us {} latency_p99_us {} horizon_frames {horizon}", report.head_frames, report.pool_bytes, report.latency_p50.as_micros(), report.latency_p95.as_micros(), report.latency_p99.as_micros());
            let mut rt = Runtime::new(plan, limits)
                .map_err(fail)?
                .with_stream_cache(cache);
            rt.set_stream_horizon(horizon).map_err(fail)?;
            rt.set_cold_starts(true);
            if let Some(fuel) = std::env::var("PERF_BLOCK_FUEL")
                .ok()
                .and_then(|v| v.parse().ok())
            {
                rt.set_behavior_block_fuel(fuel);
            }
            rt.set_voice_stealing(Some(Stealing::for_limits(RATE, limits.voices)))
                .map_err(fail)?;
            let mut groups = [None; 16];
            groups[0] = Some(Version::Midi1);
            Ok(Loaded {
                player: Player::Midi {
                    rt,
                    ingress: Ingress::new(0, groups),
                    horizon: Some(horizon),
                },
                stream_bytes: report.head_bytes as u64,
                impulses,
                _keep: Some(Box::new((streamer, assets))),
            })
        }
        Source::Uvi(bank, program) => {
            let bank =
                sampler_uvi::Bank::open(&root(UVI, "/mnt/MAIN_STORAGE/Libraries/UVI").join(bank))
                    .map_err(fail)?;
            let options = sampler_kontakt::Options {
                rate: RATE,
                ..Default::default()
            };
            let program = sampler_uvi::load_program_scripted_with_options(&bank, program, &options)
                .map_err(fail)?;
            let impulses = program.instrument.impulses.len();
            let limits = Limits {
                notes: 64,
                channels: 16,
                performances: 1,
                expressions: 64,
                families: 256,
                decisions: 256,
                voices: 512,
                commands: 256,
                behaviors: 16,
                behavior_fuel: 1 << 20,
                behavior_cells: 0,
                note_cells: 0,
            };
            let player = sampler_uvi::scripted::Player::new(program, limits, RATE).map_err(fail)?;
            Ok(Loaded {
                player: Player::Uvi(Box::new(player)),
                stream_bytes: 0,
                impulses,
                _keep: None,
            })
        }
    }
}

/// Everything one cell reports besides the block times.
#[derive(Default)]
struct Cell_ {
    times: Vec<u64>,
    /// Per block: thread CPU nanoseconds inside the same timed call (`times` is wall time).
    cpu: Vec<u64>,
    /// Per block: nanoseconds `service_streaming` took (inside `times`).
    service: Vec<u64>,
    allocations: u64,
    peak: f32,
    /// Seconds of the first and last block with output above -80 dB.
    audible: Option<(f64, f64)>,
    /// Per output frame (render clock): whether either channel exceeds -80 dB.
    loud: Vec<bool>,
    /// Per block counters, read outside the timed region (see [`Rec`]).
    recs: Vec<Rec>,
    /// FNV-1a over every output sample's bits: equal hashes mean bit-identical audio.
    hash: u64,
    /// MXCSR of the rendering thread before the first block (FTZ = bit 15, DAZ = bit 6).
    mxcsr: u32,
}

/// Deterministic per-block work counters. Wall time is noisy on a shared host;
/// these say what a block did, so the expensive blocks can be explained.
#[derive(Clone, Copy, Default)]
struct Rec {
    /// Heap calls (alloc, realloc, free) on this thread during the render.
    allocs: u64,
    /// Minor page faults of this thread during the render (first touch).
    minflt: u64,
    /// Note events in the block.
    events: u32,
    /// Callbacks flushed (script behaviors run) and ended during the block.
    behaviors: u32,
    ended: u32,
    /// Runtime counter deltas across the block.
    cold: u64,
    refused: u64,
    growths: u64,
    underruns: u64,
    replacements: u64,
    nonfinite: u64,
    /// Active voices after the block.
    voices: u32,
    /// Where the render time went (see `PHASE_NS`).
    phase: [u64; 3],
}

thread_local! {
    static BEHAVIORS_RUN: Cell<u32> = const { Cell::new(0) };
    static ENDED_RUN: Cell<u32> = const { Cell::new(0) };
    /// Nanoseconds of the last render, split: [service+due, ingress (events and
    /// audio), effects+callback flush].
    static PHASE_NS: Cell<[u64; 3]> = const { Cell::new([0; 3]) };
}

/// Minor faults of the calling thread so far.
#[allow(unsafe_code, reason = "getrusage is a plain syscall wrapper")]
fn thread_minflt() -> u64 {
    // SAFETY: zeroed rusage is a valid out-parameter for getrusage.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: RUSAGE_THREAD with a valid pointer to a rusage.
    unsafe { libc::getrusage(libc::RUSAGE_THREAD, &mut usage) };
    usage.ru_minflt as u64
}

#[cfg(target_arch = "x86_64")]
#[allow(unsafe_code, deprecated, reason = "MXCSR read and set is the only way to query FTZ/DAZ")]
fn mxcsr() -> u32 {
    // SAFETY: reads the thread's MXCSR, which is always accessible in user mode.
    unsafe { std::arch::x86_64::_mm_getcsr() }
}

#[cfg(not(target_arch = "x86_64"))]
fn mxcsr() -> u32 {
    0
}

/// Sets FTZ and DAZ on the calling thread (bench experiment via `PERF_FTZ=1`).
#[cfg(target_arch = "x86_64")]
#[allow(unsafe_code, deprecated, reason = "MXCSR set is the only way to enable FTZ/DAZ")]
fn set_ftz_daz() {
    // SAFETY: sets two rounding-control bits of the thread's own MXCSR.
    unsafe { std::arch::x86_64::_mm_setcsr(std::arch::x86_64::_mm_getcsr() | 0x8040) };
}

#[cfg(not(target_arch = "x86_64"))]
fn set_ftz_daz() {}

fn fnv(hash: u64, samples: &[Frame]) -> u64 {
    samples.iter().flatten().fold(hash, |h, x| {
        (h ^ u64::from(x.to_bits())).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn play(p: &mut Player, messages: &[Message], block: usize, frames: usize) -> Cell_ {
    let words: Vec<[u32; 1]> = messages
        .iter()
        .map(|&(_, [s, a, b])| {
            [0x2000_0000 | u32::from(s) << 16 | u32::from(a) << 8 | u32::from(b)]
        })
        .collect();
    let packets: Vec<(usize, TimedPacket<'_>)> = messages
        .iter()
        .zip(&words)
        .map(|(&(at, _), w)| {
            let packet = Packets::new(w).next().unwrap().unwrap();
            (
                (at * f64::from(RATE)).round() as usize,
                TimedPacket { offset: 0, packet },
            )
        })
        .collect();
    let mut cell = Cell_ {
        cpu: Vec::new(),
        times: Vec::with_capacity(frames / block + 1),
        service: Vec::with_capacity(frames / block + 1),
        allocations: 0,
        peak: 0.0,
        audible: None,
        loud: Vec::with_capacity(frames),
        recs: Vec::with_capacity(frames / block + 1),
        hash: 0xcbf2_9ce4_8422_2325,
        mxcsr: 0,
    };
    if std::env::var_os("PERF_FTZ").is_some() {
        set_ftz_daz();
    }
    cell.mxcsr = mxcsr();
    let (mut buffer, mut next) = (vec![[0.0f32; 2]; block], 0);
    let mut batch: Vec<TimedPacket<'_>> = Vec::with_capacity(64);
    let start = Instant::now();
    let mut last_underruns = 0;
    for begin in (0..frames).step_by(block) {
        batch.clear();
        while next < packets.len() && packets[next].0 < begin + block {
            batch.push(TimedPacket {
                offset: packets[next].0 - begin,
                ..packets[next].1
            });
            next += 1;
        }
        let before = CALLS.get();
        let (s0, f0) = (stats(p), thread_minflt());
        let (b0, e0) = (BEHAVIORS_RUN.get(), ENDED_RUN.get());
        COUNTING.set(true);
        let t = Instant::now();
        let c0 = cpu_ns(false);
        let service = render(p, &mut buffer, &batch, messages, begin, &mut next);
        hash_output(&buffer);
        cell.service.push(service);
        let took = t.elapsed();
        let cpu = cpu_ns(false).saturating_sub(c0);
        COUNTING.set(false);
        let (s1, f1) = (stats(p), thread_minflt());
        cell.recs.push(Rec {
            allocs: CALLS.get() - before,
            minflt: f1 - f0,
            events: batch.len() as u32,
            behaviors: BEHAVIORS_RUN.get() - b0,
            ended: ENDED_RUN.get() - e0,
            cold: s1.cold_starts - s0.cold_starts,
            refused: s1.refused_starts - s0.refused_starts,
            growths: s1.voice_growths - s0.voice_growths,
            underruns: s1.stream_underruns - s0.stream_underruns,
            replacements: s1.waiting_voice_replacements - s0.waiting_voice_replacements,
            nonfinite: s1.nonfinite_frames - s0.nonfinite_frames,
            voices: s1.voices as u32,
            phase: PHASE_NS.get(),
        });
        cell.hash = fnv(cell.hash, &buffer);
        if std::env::var_os("PERF_DEBUG").is_some() {
            let now = stats(p).stream_underruns;
            if now != last_underruns {
                eprintln!(
                    "underruns {last_underruns} -> {now} at {:.3}s ({} events in block)",
                    begin as f64 / f64::from(RATE),
                    batch.len()
                );
                last_underruns = now;
            }
        }
        cell.allocations += CALLS.get() - before;
        if std::env::var_os("PERF_SLOW").is_some() && took.as_micros() > 3000 {
            eprintln!(
                "slow block at {:.3}s: {} us (service {} us, {} events)",
                begin as f64 / f64::from(RATE),
                took.as_micros(),
                service / 1000,
                batch.len()
            );
        }
        cell.times.push(took.as_nanos() as u64);
        cell.cpu.push(cpu);
        let top = buffer.iter().flatten().fold(0.0f32, |m, x| m.max(x.abs()));
        cell.peak = cell.peak.max(top);
        if top > 1e-4 {
            let at = begin as f64 / f64::from(RATE);
            cell.audible = Some(cell.audible.map_or((at, at), |(first, _)| (first, at)));
        }
        cell.loud.extend(buffer.iter().map(|f| f[0].abs().max(f[1].abs()) > SILENT_FRAME));
        // Real time: streamed pages arrive from reader threads, which an unpaced render starves.
        let due = start + Duration::from_secs_f64((begin + block) as f64 / f64::from(RATE));
        if let Some(wait) = due.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
    }
    cell
}

/// Frame-level audibility for onset timing (-80 dB), the same threshold as `audible`.
const SILENT_FRAME: f32 = 1e-4;

fn render(
    p: &mut Player,
    buffer: &mut [Frame],
    batch: &[TimedPacket<'_>],
    _: &[Message],
    _: usize,
    _: &mut usize,
) -> u64 {
    let mut service = 0;
    match p {
        Player::Midi {
            rt,
            ingress,
            horizon,
        } => {
            let t_start = Instant::now();
            if let Some(h) = horizon {
                // Production V2 advances due/bootstrap callbacks before asking
                // for pages, without advancing the sample clock or refilling fuel.
                if rt.render(&mut []).is_err() { EVENT_ERRORS.set(EVENT_ERRORS.get() + 1); }
                let t = Instant::now();
                let r = rt.service_streaming(*h);
                service = t.elapsed().as_nanos() as u64;
                if r.is_err() { STREAM_SERVICE_ERRORS.set(STREAM_SERVICE_ERRORS.get() + 1); }
                match r {
                    Err(e) if std::env::var_os("PERF_DEBUG").is_some() => {
                        eprintln!("service_streaming: {e:?} voices {}", rt.voice_count())
                    }
                    _ => {}
                }
            }
            if std::env::var_os("PERF_VOICES").is_some() {
                thread_local! { static LAST: Cell<usize> = const { Cell::new(0) }; static T: Cell<usize> = const { Cell::new(0) }; }
                T.set(T.get() + buffer.len());
                let n = rt.voice_count() / 5;
                if n != LAST.replace(n) {
                    eprintln!(
                        "t {:.3}s voices {} stolen {} events {}",
                        T.get() as f64 / 48000.,
                        rt.voice_count(),
                        rt.stolen_voices(),
                        batch.len()
                    );
                }
            }
            let t_ingress = Instant::now();
            let mut errors = 0;
            let result = ingress.render(rt, buffer, batch, batch.len().max(64), |i, r| {
                if let Err(e) = r {
                    errors += 1;
                    if std::env::var_os("PERF_DEBUG").is_some() && errors < 4 && batch.len() > 0 {
                        eprintln!("event {i} of block ({} events): {e:?}", batch.len());
                    }
                }
            });
            EVENT_ERRORS.set(EVENT_ERRORS.get() + errors + u64::from(result.is_err()));
            // Match sampler-native's headless service policy: retain external
            // async work and fail the cell, rather than pretending it completed.
            // UI presentation effects may be discarded by this headless audit.
            let t_post = Instant::now();
            PHASE_NS.set([
                t_ingress.duration_since(t_start).as_nanos() as u64,
                t_post.duration_since(t_ingress).as_nanos() as u64,
                0,
            ]);
            rt.drain_effects(|effect| {
                let bit = match effect.service {
                    sampler_core::MIDI_SERVICE => 1,
                    sampler_core::ARRAY_FILE_SERVICE => 2,
                    _ => 0,
                };
                if bit != 0 {
                    SERVICE_ERRORS.set(SERVICE_ERRORS.get() + 1);
                    SERVICE_MASK.set(SERVICE_MASK.get() | bit);
                    false
                } else { true }
            });
            rt.flush_behaviors(|_, _, outcome| {
                BEHAVIORS_RUN.set(BEHAVIORS_RUN.get() + 1);
                if matches!(
                    outcome,
                    sampler_core::Outcome::Fault(_) | sampler_core::Outcome::FuelExhausted
                ) {
                    BEHAVIOR_ERRORS.set(BEHAVIOR_ERRORS.get() + 1);
                }
                true
            });
            rt.flush_ended(|_| {
                ENDED_RUN.set(ENDED_RUN.get() + 1);
                true
            });
            PHASE_NS.set([PHASE_NS.get()[0], PHASE_NS.get()[1], t_post.elapsed().as_nanos() as u64]);
        }
        Player::Uvi(player) => {
            // Script-host events are applied at block starts: coarse, but the same for every run.
            for packet in batch {
                let _ = packet;
            }
            let _ = player.render(buffer);
        }
    }
    service
}

fn uvi_events(p: &mut Player, messages: &[Message], from: usize, to: usize) {
    if let Player::Uvi(player) = p {
        for &(at, [status, key, velocity]) in messages {
            let frame = (at * f64::from(RATE)).round() as usize;
            if (from..to).contains(&frame) {
                let _ = if status & 0xf0 == 0x90 {
                    player.note_on(key, f64::from(velocity) / 127.0)
                } else {
                    player.note_off(key)
                };
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
    // UVI cells record no service times, so their service percentiles are empty.
    if sorted.is_empty() {
        return 0;
    }
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

/// Counts hardware events of this process, while it runs `f`.
fn counted<T>(f: impl FnOnce() -> T) -> (T, Value) {
    let file = std::env::temp_dir().join(format!("sampler-perf-{}.csv", std::process::id()));
    let child = Command::new("perf")
        .args([
            "stat",
            "-x,",
            "-e",
            EVENTS,
            "-p",
            &std::process::id().to_string(),
            "-o",
        ])
        .arg(&file)
        .stderr(Stdio::null())
        .spawn()
        .ok();
    std::thread::sleep(Duration::from_millis(300));
    let out = f();
    let mut counters = serde_json::Map::new();
    if let Some(mut child) = child {
        let _ = Command::new("kill")
            .args(["-INT", &child.id().to_string()])
            .status();
        let _ = child.wait();
        for line in std::fs::read_to_string(&file).unwrap_or_default().lines() {
            let f: Vec<&str> = line.split(',').collect();
            if let (Some(value), Some(event)) =
                (f.first().and_then(|v| v.parse::<f64>().ok()), f.get(2))
            {
                counters.insert(event.trim_end_matches(":u").to_string(), json!(value));
            }
        }
        let _ = std::fs::remove_file(&file);
    }
    if let (Some(c), Some(i)) = (
        counters.get("cycles").and_then(Value::as_f64),
        counters.get("instructions").and_then(Value::as_f64),
    ) {
        counters.insert("ipc".into(), json!(i / c.max(1.0)));
    }
    (out, Value::Object(counters))
}

fn run_scenario(
    s: &Scenario,
    seconds: Option<f64>,
    cores: usize,
    only_cell: Option<(usize, usize)>,
) -> Result<(Vec<Value>, Value), String> {
    let messages = {
        let mut m = (s.notes)();
        m.sort_by(|a, b| {
            a.0.total_cmp(&b.0)
                .then((a.1[0] & 0xf0 == 0x90).cmp(&(b.1[0] & 0xf0 == 0x90)))
        });
        m
    };
    let started = Instant::now();
    let rss0 = proc_value("/proc/self/status", "VmRSS:");
    let mut loaded = load(s, &messages, BLOCKS[0])?;
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
            loaded = load(s, &messages, block)?;
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
            let (rss, io0, rchar0) = (
                proc_value("/proc/self/status", "VmRSS:"),
                proc_value("/proc/self/io", "read_bytes:"),
                proc_value("/proc/self/io", "rchar:"),
            );
            OUTPUT_HASH.store(0xcbf29ce484222325, std::sync::atomic::Ordering::Relaxed);
            let (cell, counters) = counted(|| {
                if matches!(loaded.player, Player::Uvi(_)) {
                    play_uvi(&mut loaded.player, &messages, block, frames)
                } else {
                    play(&mut loaded.player, &messages, block, frames)
                }
            });
            eprintln!("output-hash {} block {block} threads {t} {:016x}", s.name, OUTPUT_HASH.load(std::sync::atomic::Ordering::Relaxed));
            let mut times = cell.times.clone();
            times.sort_unstable();
            let mut service = cell.service.clone();
            service.sort_unstable();
            let deadline = (block as f64 / f64::from(RATE) * 1e9) as u64;
            let st = stats(&loaded.player);
            // A cell that is silent, refused events or underran measured nothing useful.
            let mut failures = Vec::new();
            if cell.peak < SILENT {
                failures.push(format!("silent (peak {:.5})", cell.peak));
            }
            // Heap calls on the audio thread, Luau's included: mlua's allocator is the
            // Rust global allocator, so the counter sees Lua growth. Kontakt cells are
            // asserted to be zero; UVI cells report theirs (mlua argument conversion
            // in script field access, about 10 per block) until that path is fixed.
            if cell.allocations > 0 && !matches!(s.source, Source::Uvi(..)) {
                failures.push(format!("{} audio-thread allocations", cell.allocations));
            }
            let event_errors = EVENT_ERRORS.replace(0);
            if event_errors > 0 {
                failures.push(format!("{event_errors} event errors"));
            }
            if st.stream_underruns > 0 {
                failures.push(format!("{} stream underruns", st.stream_underruns));
            }
            for f in &failures {
                eprintln!("FAIL {} block {block} threads {t}: {f}", s.name);
            }
            cells.push(json!({
                "failures": failures,
                "scenario": s.name, "block": block, "threads": t,
                "blocks": times.len(),
                "deadline_us": deadline as f64 / 1e3,
                "p50_us": percentile(&times, 0.5) as f64 / 1e3,
                "p99_us": percentile(&times, 0.99) as f64 / 1e3,
                "max_us": *times.last().unwrap() as f64 / 1e3,
                // Busy time over the whole run as a share of one core: the figure to compare with Kontakt's CPU%.
                "cpu_pct_mean": times.iter().sum::<u64>() as f64 / (frames as f64 / f64::from(RATE) * 1e9) * 100.0,
                "service_p99_us": percentile(&service, 0.99) as f64 / 1e3,
                "service_max_us": service.last().copied().unwrap_or(0) as f64 / 1e3,
                "misses": times.iter().filter(|&&t| t > deadline).count(),
                "audio_thread_allocations": cell.allocations, "event_errors": event_errors, "peak": cell.peak, "audible_seconds": cell.audible,
                "perf": counters,
                "rss_kb": proc_value("/proc/self/status", "VmRSS:"),
                "rss_kb_added": proc_value("/proc/self/status", "VmRSS:") as i64 - rss as i64,
                "disk_read_bytes": proc_value("/proc/self/io", "read_bytes:") - io0,
                "read_calls_bytes": proc_value("/proc/self/io", "rchar:") - rchar0,
                "stream_cache_bytes": st.stream_cache_bytes,
                "stream_underruns": st.stream_underruns,
                "core_render_peak_us": st.render_nanos_peak as f64 / 1e3,
                "voice_capacity": st.voice_capacity, "voice_drops": st.voice_drops,
                "render_hash": format!("{:016x}", cell.hash), "mxcsr": format!("{:#06x}", cell.mxcsr),
                "spikes": spike_report(&cell.times, &cell.recs),
            }));
            eprintln!(
                "{} block {block} threads {t}: core render peak {:.0} us; p50 {:.0} us p99 {:.0} us max {:.0} us",
                s.name,
                st.render_nanos_peak as f64 / 1e3,
                percentile(&times, 0.5) as f64 / 1e3,
                percentile(&times, 0.99) as f64 / 1e3,
                *times.last().unwrap() as f64 / 1e3
            );
        }
    }
    Ok((
        cells,
        json!({ "load": load_info, "idle_wakeups_per_second": idle }),
    ))
}

/// [`play`] for a script-hosted program: notes go to the player between blocks.
fn play_uvi(p: &mut Player, messages: &[Message], block: usize, frames: usize) -> Cell_ {
    let mut cell = Cell_ {
        cpu: Vec::new(),
        times: Vec::with_capacity(frames / block + 1),
        service: Vec::with_capacity(frames / block + 1),
        allocations: 0,
        peak: 0.0,
        audible: None,
        loud: Vec::new(),
        recs: Vec::new(),
        hash: 0xcbf2_9ce4_8422_2325,
        mxcsr: 0,
    };
    let mut buffer = vec![[0.0f32; 2]; block];
    let start = Instant::now();
    for begin in (0..frames).step_by(block) {
        uvi_events(p, messages, begin, begin + block);
        let before = CALLS.get();
        let f0 = thread_minflt();
        COUNTING.set(true);
        let t = Instant::now();
        let c0 = cpu_ns(false);
        render(p, &mut buffer, &[], messages, begin, &mut 0);
        hash_output(&buffer);
        let took = t.elapsed();
        let cpu = cpu_ns(false).saturating_sub(c0);
        COUNTING.set(false);
        cell.allocations += CALLS.get() - before;
        cell.times.push(took.as_nanos() as u64);
        cell.cpu.push(cpu);
        cell.recs.push(Rec {
            allocs: CALLS.get() - before,
            minflt: thread_minflt() - f0,
            voices: stats(p).voices as u32,
            ..Rec::default()
        });
        cell.hash = fnv(cell.hash, &buffer);
        // Outside the counted window: the loudness log grows its own Vec.
        cell.loud.extend(buffer.iter().map(|f| f[0].abs().max(f[1].abs()) > SILENT_FRAME));
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

/// One cold load of a scenario's instrument, timed as the host sees it. Prints one JSON line:
/// wall and process CPU seconds of `load`, peak RSS (VmHWM) and RSS after load, in kB.
/// Run it in a fresh process per repeat: the peak is process-wide.
fn loadbench(args: &[String]) -> Result<(), String> {
    let name = match args {
        [flag, name] if flag == "--only" => name,
        _ => return Err("usage: sampler-perf loadbench --only NAME".into()),
    };
    let s = SCENARIOS
        .iter()
        .find(|s| s.name == name)
        .ok_or(format!("unknown scenario {name}"))?;
    let messages = (s.notes)();
    let cpu = || {
        // SAFETY: zeroed rusage is a valid out-parameter for getrusage.
        #[allow(unsafe_code, reason = "getrusage is a plain syscall wrapper")]
        let usage = unsafe {
            let mut u: libc::rusage = std::mem::zeroed();
            libc::getrusage(libc::RUSAGE_SELF, &mut u);
            u
        };
        let t = |tv: libc::timeval| tv.tv_sec as f64 + tv.tv_usec as f64 * 1e-6;
        t(usage.ru_utime) + t(usage.ru_stime)
    };
    let (wall0, cpu0) = (Instant::now(), cpu());
    let loaded = load(s, &messages, BLOCKS[0])?;
    let (wall, cpu_s) = (wall0.elapsed().as_secs_f64(), cpu() - cpu0);
    println!(
        "{}",
        json!({
            "scenario": s.name,
            "wall_s": wall,
            "cpu_s": cpu_s,
            "peak_rss_kb": proc_value("/proc/self/status", "VmHWM:"),
            "rss_kb": proc_value("/proc/self/status", "VmRSS:"),
            "stream_head_bytes": loaded.stream_bytes,
        })
    );
    Ok(())
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
                cell = Some((
                    b.parse::<usize>().map_err(fail)?,
                    t.parse::<usize>().map_err(fail)?,
                ));
            }
            "--seconds" => seconds = Some(v.parse::<f64>().map_err(fail)?),
            _ => return Err(format!("unknown option {a}")),
        }
    }
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    let (mut cells, mut extra, mut skipped) = (Vec::new(), Vec::new(), Vec::new());
    for s in SCENARIOS
        .iter()
        .filter(|s| only.as_ref().is_none_or(|o| o.iter().any(|n| n == s.name)))
    {
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
        .and_then(|c| {
            c.lines().find_map(|l| {
                l.strip_prefix("model name")?
                    .split(':')
                    .nth(1)
                    .map(|s| s.trim().to_string())
            })
        })
        .unwrap_or_default();
    let result = json!({
        "format": 1, "rate": RATE, "cores": cores, "cpu": model,
        "commit": Command::new("git").args(["rev-parse", "--short", "HEAD"]).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()),
        "load_and_idle": extra, "skipped": skipped, "cells": cells,
    });
    let text = serde_json::to_string_pretty(&result).map_err(fail)?;
    match out {
        Some(path) => std::fs::write(path, text).map_err(fail)?,
        None => println!("{text}"),
    }
    let failed = result["cells"].as_array().map_or(0, |c| {
        c.iter()
            .filter(|c| c["failures"].as_array().is_some_and(|f| !f.is_empty()))
            .count()
    });
    if failed > 0 {
        return Err(format!(
            "{failed} cells failed (silent, refused events or underruns)"
        ));
    }
    Ok(())
}

/// Metrics where a rise is a regression, and whether noise needs the threshold.
const WATCHED: &[(&str, &str)] = &[("p50_us", "p50"), ("p99_us", "p99"), ("rss_kb", "resident")];

fn key(c: &Value) -> String {
    format!(
        "{} block {} threads {}",
        c["scenario"].as_str().unwrap_or("?"),
        c["block"],
        c["threads"]
    )
}

fn compare(args: &[String]) -> Result<bool, String> {
    let [base, new, rest @ ..] = args else {
        return Err("usage: compare BASE.json NEW.json [--threshold PERCENT]".into());
    };
    let threshold = match rest {
        [] => 10.0,
        [flag, v] if flag == "--threshold" => v.parse::<f64>().map_err(fail)?,
        _ => return Err("usage: compare BASE.json NEW.json [--threshold PERCENT]".into()),
    };
    let read = |p: &String| -> Result<Value, String> {
        serde_json::from_str(&std::fs::read_to_string(p).map_err(fail)?).map_err(fail)
    };
    let (base, new) = (read(base)?, read(new)?);
    let empty = Vec::new();
    let old_cells = base["cells"].as_array().unwrap_or(&empty);
    let mut regressions = 0;
    for c in new["cells"].as_array().unwrap_or(&empty) {
        // Hard checks on the new run alone: it must sound, refuse no event, never underrun and never miss a deadline.
        let mut hard = Vec::new();
        if c["peak"].as_f64().unwrap_or(0.0) < f64::from(SILENT) {
            hard.push(format!(
                "silent (peak {:.5})",
                c["peak"].as_f64().unwrap_or(0.0)
            ));
        }
        for (field, what) in [
            ("event_errors", "event errors"),
            ("stream_underruns", "stream underruns"),
            ("misses", "deadline misses"),
        ] {
            if c[field].as_f64().unwrap_or(0.0) > 0.0 {
                hard.push(format!("{} {what}", c[field]));
            }
        }
        if !hard.is_empty() {
            regressions += 1;
            println!(
                "FAILED {}: {} (max block {:.0} us, deadline {:.0} us)",
                key(c),
                hard.join("; "),
                c["max_us"].as_f64().unwrap_or(0.0),
                c["deadline_us"].as_f64().unwrap_or(0.0)
            );
        }
        let Some(o) = old_cells.iter().find(|o| key(o) == key(c)) else {
            println!("{}: new cell, no baseline", key(c));
            continue;
        };
        let mut notes = Vec::new();
        let mut check = |label: &str, old: f64, now: f64| {
            if old > 0.0 && (now - old) / old * 100.0 > threshold {
                notes.push(format!(
                    "{label} {old:.1} -> {now:.1} (+{:.0}%)",
                    (now - old) / old * 100.0
                ));
            }
        };
        for (field, label) in WATCHED {
            check(
                label,
                o[field].as_f64().unwrap_or(0.0),
                c[field].as_f64().unwrap_or(0.0),
            );
        }
        for event in ["cycles", "instructions", "cache-misses", "branch-misses"] {
            check(
                event,
                o["perf"][event].as_f64().unwrap_or(0.0),
                c["perf"][event].as_f64().unwrap_or(0.0),
            );
        }
        let more = |f: &str| c[f].as_f64().unwrap_or(0.0) > o[f].as_f64().unwrap_or(0.0);
        for (f, label) in [
            ("misses", "deadline misses"),
            ("audio_thread_allocations", "audio-thread allocations"),
            ("stream_underruns", "underruns"),
        ] {
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
    if let Some(every) = std::env::var("PERF_ALLOC_TRACE").ok().and_then(|v| v.parse().ok()) {
        TRACE_EVERY.store(every, Ordering::Relaxed);
    }
    let result = match args.first().map(String::as_str) {
        Some("sustain") => sustain::run(&args[1..]).map(|()| true),
        Some("stress") => stress::run(&args[1..]).map(|()| true),
        Some("capacity") => capacity::run(&args[1..]).map(|()| true),
        Some("spike") => capacity::spike(&args[1..]).map(|()| true),
        Some("run") => run(&args[1..]).map(|()| true),
        Some("compare") => compare(&args[1..]),
        Some("loadbench") => loadbench(&args[1..]).map(|()| true),
        _ => Err("usage: sampler-perf sustain [--only afflatus|conflux|conflux-multi] [--block 64|256] [--notes 1|8] [--seconds S] [--out FILE] | run [--out FILE] [--only NAME,..] [--cell BLOCK,THREADS] [--seconds S] | compare BASE.json NEW.json [--threshold PERCENT] | capacity [--caps 64,256,4096] [--only NAME] [--block 64] [--seconds S] [--repeats N]".into()),
    };
    report_alloc_traces();
    match result {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(e) => {
            eprintln!("sampler-perf: {e}");
            std::process::exit(2);
        }
    }
}

/// Explains the slow blocks with their counters: spike blocks (over 4x the median
/// render time) against the rest, plus the eight slowest blocks in full.
fn spike_report(took: &[u64], recs: &[Rec]) -> Value {
    if took.is_empty() || took.len() != recs.len() {
        return json!(null);
    }
    let mut sorted = took.to_vec();
    sorted.sort_unstable();
    let median = sorted[sorted.len() / 2];
    let spike = |i: usize| took[i] > median.saturating_mul(4);
    let fields: [(&str, fn(&Rec) -> f64); 12] = [
        ("allocs", |r| r.allocs as f64),
        ("minflt", |r| r.minflt as f64),
        ("events", |r| f64::from(r.events)),
        ("behaviors", |r| f64::from(r.behaviors)),
        ("ended", |r| f64::from(r.ended)),
        ("cold", |r| r.cold as f64),
        ("refused", |r| r.refused as f64),
        ("growths", |r| r.growths as f64),
        ("underruns", |r| r.underruns as f64),
        ("replacements", |r| r.replacements as f64),
        ("nonfinite", |r| r.nonfinite as f64),
        ("voices", |r| f64::from(r.voices)),
    ];
    let (mut in_spike, mut outside) = (Vec::new(), Vec::new());
    for i in 0..took.len() {
        if spike(i) { in_spike.push(i) } else { outside.push(i) }
    }
    let mean = |ix: &[usize], f: fn(&Rec) -> f64| -> f64 {
        if ix.is_empty() { 0.0 } else { ix.iter().map(|&i| f(&recs[i])).sum::<f64>() / ix.len() as f64 }
    };
    let frac = |ix: &[usize], f: fn(&Rec) -> f64| -> f64 {
        if ix.is_empty() { 0.0 } else { ix.iter().filter(|&&i| f(&recs[i]) > 0.0).count() as f64 / ix.len() as f64 }
    };
    let mut per_field = serde_json::Map::new();
    for (name, f) in &fields {
        per_field.insert(
            (*name).to_string(),
            json!({
                "spike_mean": mean(&in_spike, *f),
                "normal_mean": mean(&outside, *f),
                "spike_nonzero": frac(&in_spike, *f),
                "normal_nonzero": frac(&outside, *f),
            }),
        );
    }
    let mut slowest: Vec<usize> = (0..took.len()).collect();
    slowest.sort_by_key(|&i| std::cmp::Reverse(took[i]));
    let top: Vec<Value> = slowest
        .iter()
        .take(8)
        .map(|&i| {
            let r = &recs[i];
            json!({
                "block": i, "took_us": took[i] as f64 / 1e3, "allocs": r.allocs, "minflt": r.minflt,
                "events": r.events, "behaviors": r.behaviors, "ended": r.ended, "cold": r.cold,
                "refused": r.refused, "growths": r.growths, "underruns": r.underruns,
                "replacements": r.replacements, "nonfinite": r.nonfinite, "voices": r.voices,
                "phase_us": r.phase.map(|n| n as f64 / 1e3),
            })
        })
        .collect();
    json!({
        "median_us": median as f64 / 1e3,
        "spike_blocks": in_spike.len(),
        "fields": per_field,
        "slowest": top,
    })
}

/// Prints the most common call paths of the sampled audio-thread allocations: the
/// first few frames outside std and the allocator, so the calling code shows.
fn report_alloc_traces() {
    if TRACE_EVERY.load(Ordering::Relaxed) == 0 {
        return;
    }
    let traces = TRACES.lock().map(|t| t.clone()).unwrap_or_default();
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for text in &traces {
        let frames: Vec<&str> = text
            .lines()
            .filter_map(|l| {
                let l = l.trim_start();
                let (n, sym) = l.split_once(": ")?;
                n.parse::<u32>().ok()?;
                Some(sym.trim())
            })
            .filter(|f| {
                !["std::", "core::", "alloc::", "backtrace", "sampler_perf::", "<alloc", "__rust", "<unknown"]
                    .iter()
                    .any(|skip| f.starts_with(skip))
            })
            .take(3)
            .collect();
        *counts.entry(frames.join(" <- ")).or_default() += 1;
    }
    let mut ranked: Vec<_> = counts.into_iter().collect();
    ranked.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    eprintln!("alloc traces: {} sampled audio-thread heap calls", traces.len());
    for (path, n) in ranked.iter().take(12) {
        eprintln!("  {n:>5}  {path}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Luau allocates through mlua's allocator, which is the Rust global allocator, so
    /// the audio-thread counter must see it. Small Lua objects come from Luau's own
    /// pages, so only page-sized growth shows up one call at a time: test with a big string.
    #[test]
    fn counter_sees_luau_allocations() {
        let lua = mlua::Lua::new();
        COUNTING.set(true);
        let before = CALLS.get();
        lua.load("return #string.rep('x', 1048576)").exec().unwrap();
        let counted = CALLS.get() - before;
        COUNTING.set(false);
        assert!(counted >= 1, "Luau allocation not counted");
    }

    #[test]
    fn spike_report_flags_only_the_slow_block() {
        let took = [100, 100, 100, 100, 1000];
        let mut recs = vec![Rec::default(); took.len()];
        recs[4].allocs = 7;
        let report = spike_report(&took, &recs);
        assert_eq!(report["spike_blocks"], 1);
        assert_eq!(report["fields"]["allocs"]["spike_mean"], 7.0);
        assert_eq!(report["fields"]["allocs"]["normal_nonzero"], 0.0);
    }
}

/// FNV-1a over the rendered output bits of the measured cell: equal hashes mean bit-identical audio.
static OUTPUT_HASH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0xcbf29ce484222325);
fn hash_output(b: &[[f32; 2]]) {
    let mut h = OUTPUT_HASH.load(std::sync::atomic::Ordering::Relaxed);
    for f in b { for x in f { h = (h ^ u64::from(x.to_bits())).wrapping_mul(0x100000001b3); } }
    OUTPUT_HASH.store(h, std::sync::atomic::Ordering::Relaxed);
}
