//! Spike P3.1: can Luau run on the audio thread without system malloc?
//!
//! Runs one benchmark suite on mlua's default allocator and on a raw Luau state backed by a
//! fixed arena. Prints a markdown table. See docs/architecture-v3/spikes/luau-alloc.md.

mod arena;
mod backend;
mod counting;

use std::hint::black_box;
use std::time::Instant;

use backend::{ArenaBackend, Backend, Func, MluaBackend};
use counting::Snapshot;

const CALLS: u64 = 100_000;
const HEAP_TABLES: f64 = 100_000.0;
const GC_STEPS: u64 = 1_000;
const ARENA_BYTES: usize = 64 << 20;
const PROBE_BYTES: usize = 4 << 20;

const SRC: &str = r#"
local function arith(i)
    return i * 2 + 1
end

local function table_step(i)
    local t = { i, i + 1 }
    return t[1] + t[2]
end

local keep = nil

function call_arith(i) return arith(i) end
function call_table(i) return table_step(i) end

function loop_table(n)
    local acc = 0
    for i = 1, n do acc = acc + table_step(i) end
    return acc
end

function loop_coro(n)
    local acc = 0
    for i = 1, n do
        local co = coroutine.create(arith)
        local _, v = coroutine.resume(co, i)
        acc = acc + v
    end
    return acc
end

function build(n)
    keep = {}
    for i = 1, n do keep[i] = { i } end
    return n
end

function drop_heap()
    keep = nil
    return 0
end
"#;

#[derive(Debug)]
pub struct Row {
    backend: &'static str,
    case: String,
    ops: u64,
    allocs: u64,
    bytes: u64,
    mean_ns: u64,
    p99_ns: Option<u64>,
    max_ns: Option<u64>,
    note: String,
}

fn summarize(
    backend: &'static str,
    case: &str,
    delta: Snapshot,
    samples: &mut [u64],
    note: String,
) -> Row {
    samples.sort_unstable();
    let len = samples.len();
    let sum: u64 = samples.iter().sum();
    Row {
        backend,
        case: case.to_owned(),
        ops: len as u64,
        allocs: delta.allocs,
        bytes: delta.bytes,
        mean_ns: sum / len as u64,
        p99_ns: Some(samples[(len * 99 / 100).min(len - 1)]),
        max_ns: samples.last().copied(),
        note,
    }
}

/// Per-call latency and allocation count for a Rust-driven call, repeated `CALLS` times.
fn timed_calls<B: Backend>(b: &mut B, backend: &'static str, case: &str, f: Func) -> Row {
    let mut samples = Vec::with_capacity(CALLS as usize);
    let mut acc = 0.0f64;
    let before = counting::snapshot();
    for i in 0..CALLS {
        let t = Instant::now();
        acc += b
            .call_num(f, i as f64)
            .unwrap_or_else(|e| panic!("{case}: {e}"));
        samples.push(t.elapsed().as_nanos() as u64);
    }
    let delta = counting::snapshot().since(before);
    black_box(acc);
    summarize(backend, case, delta, &mut samples, String::new())
}

/// One Luau-side loop of `n` iterations. The loop runs inside the VM, so this is VM throughput.
fn looped<B: Backend>(b: &mut B, backend: &'static str, case: &str, f: Func, n: u64) -> Row {
    let before = counting::snapshot();
    let t = Instant::now();
    let got = b
        .call_num(f, n as f64)
        .unwrap_or_else(|e| panic!("{case}: {e}"));
    let elapsed = t.elapsed().as_nanos() as u64;
    let delta = counting::snapshot().since(before);
    black_box(got);
    Row {
        backend,
        case: case.to_owned(),
        ops: n,
        allocs: delta.allocs,
        bytes: delta.bytes,
        mean_ns: elapsed / n,
        p99_ns: None,
        max_ns: None,
        note: format!("result {got}"),
    }
}

/// Per-step latency of bounded `LUA_GCSTEP` calls, over a heap that is already built.
fn gc_steps<B: Backend>(b: &mut B, backend: &'static str, case: &str, steps: u64, kb: i32) -> Row {
    let mut samples = Vec::with_capacity(steps as usize);
    let mut cycles = 0u64;
    let before = counting::snapshot();
    for _ in 0..steps {
        let t = Instant::now();
        let done = b.gc_step(kb).unwrap_or_else(|e| panic!("{case}: {e}"));
        samples.push(t.elapsed().as_nanos() as u64);
        cycles += u64::from(done);
    }
    let delta = counting::snapshot().since(before);
    summarize(
        backend,
        case,
        delta,
        &mut samples,
        format!("full GC cycles finished: {cycles}"),
    )
}

fn suite<B: Backend>(b: &mut B, backend: &'static str) -> Vec<Row> {
    let arith = b.resolve("call_arith");
    let table = b.resolve("call_table");
    let loop_table = b.resolve("loop_table");
    let loop_coro = b.resolve("loop_coro");
    let build = b.resolve("build");
    let drop_heap = b.resolve("drop_heap");

    let mut rows = vec![
        timed_calls(b, backend, "Rust->Luau call, arithmetic", arith),
        timed_calls(b, backend, "Rust->Luau call, 2-slot table alloc", table),
        looped(b, backend, "Luau loop, 1e5 table allocs", loop_table, CALLS),
        looped(
            b,
            backend,
            "Luau loop, 1e5 coroutine create+resume",
            loop_coro,
            CALLS,
        ),
    ];
    // Setup: builds the retained heap, so it is outside every measured window.
    b.call_num(build, HEAP_TABLES)
        .unwrap_or_else(|e| panic!("build heap: {e}"));
    rows.push(gc_steps(
        b,
        backend,
        "LUA_GCSTEP data=0 x1000, 1e5-table heap",
        GC_STEPS,
        0,
    ));
    rows.push(gc_steps(
        b,
        backend,
        "LUA_GCSTEP data=64 KiB x1000, 1e5-table heap",
        GC_STEPS,
        64,
    ));
    b.call_num(drop_heap, 0.0)
        .unwrap_or_else(|e| panic!("drop heap: {e}"));
    rows
}

fn opt(v: Option<u64>) -> String {
    v.map_or_else(|| "-".to_owned(), |x| x.to_string())
}

fn print(rows: &[Row]) {
    println!("| backend | case | ops | allocs/op | bytes/op | mean ns | p99 ns | max ns | note |");
    println!("|---|---|---:|---:|---:|---:|---:|---:|---|");
    for r in rows {
        let per = |x: u64| x as f64 / r.ops as f64;
        println!(
            "| {} | {} | {} | {:.3} | {:.1} | {} | {} | {} | {} |",
            r.backend,
            r.case,
            r.ops,
            per(r.allocs),
            per(r.bytes),
            r.mean_ns,
            opt(r.p99_ns),
            opt(r.max_ns),
            r.note,
        );
    }
}

fn main() {
    let mut rows = Vec::new();

    let mut mlua = MluaBackend::open(SRC);
    rows.extend(suite(&mut mlua, "mlua default"));

    let chunk = mlua::chunk::Compiler::default()
        .compile(SRC)
        .expect("compile bench chunk");
    let mut arena =
        ArenaBackend::open(ARENA_BYTES, &chunk).unwrap_or_else(|e| panic!("arena state: {e}"));
    rows.extend(suite(&mut arena, "raw arena 64 MiB"));
    let stats = arena.vm().arena();
    println!(
        "arena 64 MiB after suite: reserved {} B, peak live {} B, failures {}\n",
        stats.reserved(),
        stats.peak_live,
        stats.failures
    );

    print(&rows);

    // Exhaustion probe: the arena is too small for the retained heap. Expect a Luau error,
    // not an abort.
    eprintln!("probe: opening {PROBE_BYTES} byte arena");
    let mut small =
        ArenaBackend::open(PROBE_BYTES, &chunk).unwrap_or_else(|e| panic!("probe state: {e}"));
    let build = small.resolve("build");
    match small.call_num(build, HEAP_TABLES) {
        Ok(n) => println!("\nprobe {PROBE_BYTES} B arena: build {n} tables succeeded"),
        Err(e) => {
            println!("\nprobe {PROBE_BYTES} B arena: build {HEAP_TABLES} tables -> Luau error: {e}")
        }
    }
}
