//! Spike P3.1b, synthetic suite: the P3.1 benchmark cases on mlua's API, with option A's arena
//! scope entered around every script entry. Prints markdown tables. See
//! docs/architecture-v3/spikes/luau-alloc-mlua.md.
//!
//! Run: `build-slot cargo run --release --manifest-path tools/spikes/luau-alloc-mlua/Cargo.toml`

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::time::Instant;

use luau_alloc_mlua::routed::{self, ArenaScope, Routed, Stats};
use mlua::{Function, LuaOptions, StdLib};

const CALLS: u64 = 100_000;
const HEAP_TABLES: f64 = 100_000.0;
const GC_STEPS: u64 = 1_000;
/// Large heap: a 64 KiB step does about 64 KiB of mark/sweep work, so 10k steps cover a full
/// cycle of a 350 MB heap, and the cycle boundary shows up in the max.
const GC_STEPS_LARGE: u64 = 10_000;
/// Arena for the suite. The large-heap run below needs about 0.4 GB of Luau heap.
const ARENA_BYTES: usize = 1 << 30;
/// Tables in the large heap. About 4e6 tables is about 370 MB of Luau heap, the same order as
/// the Augmented Orchestra hosts in uvi_host (378 MB after init).
const LARGE_HEAP_TABLES: f64 = 4_000_000.0;
const MICRO_ITERS: u64 = 10_000_000;

// Same chunk as P3.1, so the rows compare directly.
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
struct Row {
    backend: &'static str,
    case: &'static str,
    ops: u64,
    stats: Stats,
    mean_ns: u64,
    p99_ns: Option<u64>,
    max_ns: Option<u64>,
    note: String,
}

fn summarize(
    backend: &'static str,
    case: &'static str,
    stats: Stats,
    samples: &mut [u64],
    note: String,
) -> Row {
    samples.sort_unstable();
    let len = samples.len();
    let sum: u64 = samples.iter().sum();
    Row {
        backend,
        case,
        ops: len as u64,
        stats,
        mean_ns: sum / len as u64,
        p99_ns: Some(samples[(len * 99 / 100).min(len - 1)]),
        max_ns: samples.last().copied(),
        note,
    }
}

/// Entry-point wrapper. `scoped` decides whether each script entry runs in an arena scope.
trait Backend {
    fn resolve(&mut self, name: &str) -> Func;
    fn call_num(&mut self, f: Func, arg: f64) -> Result<f64, String>;
    fn gc_step(&mut self, kb: i32) -> Result<bool, String>;
}

#[derive(Clone, Copy, Debug)]
struct Func(usize);

/// mlua state with the sampler's std-lib subset. `scoped` enters an arena scope per entry.
struct MluaBackend {
    lua: mlua::Lua,
    fns: Vec<Function>,
    scoped: bool,
}

impl MluaBackend {
    fn open(scoped: bool) -> Self {
        let libs = StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::COROUTINE | StdLib::BIT;
        let lua = mlua::Lua::new_with(libs, LuaOptions::new()).expect("create mlua state");
        // The chunk loads outside any scope, as script setup does at prepare.
        lua.load(SRC).exec().expect("load bench chunk");
        Self {
            lua,
            fns: Vec::new(),
            scoped,
        }
    }

    fn guard(&self) -> Option<ArenaScope> {
        self.scoped.then(ArenaScope::enter)
    }
}

impl Backend for MluaBackend {
    fn resolve(&mut self, name: &str) -> Func {
        let f: Function = self
            .lua
            .globals()
            .get(name)
            .unwrap_or_else(|e| panic!("global {name}: {e}"));
        self.fns.push(f);
        Func(self.fns.len() - 1)
    }

    fn call_num(&mut self, f: Func, arg: f64) -> Result<f64, String> {
        let _scope = self.guard();
        self.fns[f.0].call::<f64>(arg).map_err(|e| e.to_string())
    }

    fn gc_step(&mut self, kb: i32) -> Result<bool, String> {
        let _scope = self.guard();
        // SAFETY: the state is the live main state of `self.lua`. `LUA_GCSTEP` advances the
        // collector and does not run Lua code.
        Ok(unsafe { mlua::ffi::lua_gc(self.lua.state(), mlua::ffi::LUA_GCSTEP, kb) } != 0)
    }
}

/// Per-call latency and allocator counts for a Rust-driven call, repeated `CALLS` times.
fn timed_calls<B: Backend>(b: &mut B, backend: &'static str, case: &'static str, f: Func) -> Row {
    let mut samples = Vec::with_capacity(CALLS as usize);
    let mut acc = 0.0f64;
    let before = routed::stats();
    for i in 0..CALLS {
        let t = Instant::now();
        acc += b
            .call_num(f, i as f64)
            .unwrap_or_else(|e| panic!("{case}: {e}"));
        samples.push(t.elapsed().as_nanos() as u64);
    }
    let delta = routed::stats().since(before);
    black_box(acc);
    summarize(backend, case, delta, &mut samples, String::new())
}

/// One Luau-side loop of `n` iterations, which is VM throughput.
fn looped<B: Backend>(
    b: &mut B,
    backend: &'static str,
    case: &'static str,
    f: Func,
    n: u64,
) -> Row {
    let before = routed::stats();
    let t = Instant::now();
    let got = b
        .call_num(f, n as f64)
        .unwrap_or_else(|e| panic!("{case}: {e}"));
    let elapsed = t.elapsed().as_nanos() as u64;
    let delta = routed::stats().since(before);
    black_box(got);
    Row {
        backend,
        case,
        ops: n,
        stats: delta,
        mean_ns: elapsed / n,
        p99_ns: None,
        max_ns: None,
        note: format!("result {got}"),
    }
}

/// Per-step latency of bounded `LUA_GCSTEP` calls over a built heap.
fn gc_steps<B: Backend>(
    b: &mut B,
    backend: &'static str,
    case: &'static str,
    steps: u64,
    kb: i32,
) -> Row {
    let mut samples = Vec::with_capacity(steps as usize);
    let mut cycles = 0u64;
    let before = routed::stats();
    for _ in 0..steps {
        let t = Instant::now();
        let done = b.gc_step(kb).unwrap_or_else(|e| panic!("{case}: {e}"));
        samples.push(t.elapsed().as_nanos() as u64);
        cycles += u64::from(done);
    }
    let delta = routed::stats().since(before);
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
    // Setup builds the retained heap, so it is outside every measured window.
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

/// Mean cost of one alloc+free pair of a 48-byte block over `MICRO_ITERS` pairs.
fn pair_ns<A: GlobalAlloc>(label: &str, a: &A, layout: Layout) -> String {
    let t = Instant::now();
    for _ in 0..MICRO_ITERS {
        // SAFETY: `layout` is nonzero, and the pointer is freed with the same layout below.
        let p = unsafe { a.alloc(layout) };
        black_box(p);
        // SAFETY: `p` came from `a.alloc(layout)` just above and is not used after this call.
        unsafe { a.dealloc(p, layout) };
    }
    format!(
        "| {label} | {:.2} |",
        t.elapsed().as_nanos() as f64 / MICRO_ITERS as f64
    )
}

fn micro_table() {
    let layout = Layout::from_size_align(48, 16).expect("valid layout");
    println!("| allocator path | ns per alloc+free (48 B) |");
    println!("|---|---:|");
    println!(
        "{}",
        pair_ns("`System` direct (no wrapper)", &System, layout)
    );
    println!(
        "{}",
        pair_ns(
            "`Routed`, no scope (flag check, TLS counters, address check)",
            &Routed,
            layout
        )
    );
    let _scope = ArenaScope::enter();
    println!(
        "{}",
        pair_ns(
            "`Routed`, in scope (arena block, lock taken)",
            &Routed,
            layout
        )
    );
}

fn opt(v: Option<u64>) -> String {
    v.map_or_else(|| "-".to_owned(), |x| x.to_string())
}

fn print(rows: &[Row]) {
    println!(
        "| backend | case | ops | allocs/op | arena/op | system/op | system-in-scope/op | mean ns | p99 ns | max ns | note |"
    );
    println!("|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---|");
    for r in rows {
        let per = |x: u64| x as f64 / r.ops as f64;
        println!(
            "| {} | {} | {} | {:.3} | {:.3} | {:.3} | {:.3} | {} | {} | {} | {} |",
            r.backend,
            r.case,
            r.ops,
            per(r.stats.allocs),
            per(r.stats.arena),
            per(r.stats.system),
            per(r.stats.system_in_scope),
            r.mean_ns,
            opt(r.p99_ns),
            opt(r.max_ns),
            r.note,
        );
    }
}

fn main() {
    routed::reserve(ARENA_BYTES).expect("reserve arena");

    let mut rows = Vec::new();
    // Off-scope baseline: the same mlua state, with no scope entered.
    let mut plain = MluaBackend::open(false);
    rows.extend(suite(&mut plain, "mlua, no scope"));
    // Option A: every entry runs in an arena scope.
    let mut scoped = MluaBackend::open(true);
    rows.extend(suite(&mut scoped, "mlua + arena scope (A)"));

    let stats = routed::arena_stats();
    println!(
        "arena {} MiB after suite: reserved {} B, peak live {} B, spills {}\n",
        ARENA_BYTES >> 20,
        stats.reserved,
        stats.peak_live,
        stats.spills
    );
    // Large heap: same GC cases on a 4e6-table heap, in the same arena.
    let mut big = MluaBackend::open(true);
    let build = big.resolve("build");
    let drop_heap = big.resolve("drop_heap");
    big.call_num(build, LARGE_HEAP_TABLES)
        .unwrap_or_else(|e| panic!("build large heap: {e}"));
    let peak = routed::arena_stats().peak_live;
    rows.push(gc_steps(
        &mut big,
        "mlua + arena scope (A), large heap",
        "LUA_GCSTEP data=0 x10000, 4e6-table heap",
        GC_STEPS_LARGE,
        0,
    ));
    rows.push(gc_steps(
        &mut big,
        "mlua + arena scope (A), large heap",
        "LUA_GCSTEP data=64 KiB x10000, 4e6-table heap",
        GC_STEPS_LARGE,
        64,
    ));
    big.call_num(drop_heap, 0.0)
        .unwrap_or_else(|e| panic!("drop large heap: {e}"));
    println!("large heap arena peak live: {} KiB\n", peak / 1024);

    print(&rows);

    println!("\n### Allocator overhead (micro)\n");
    micro_table();
}
