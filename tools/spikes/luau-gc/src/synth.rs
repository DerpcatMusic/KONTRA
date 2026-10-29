//! The P3.1b synthetic heap, re-measured to separate the first cycle from
//! steady-state cycles.

use std::time::Instant;

use mlua::{Function, Lua};

use crate::gc;
use crate::host;

/// Same chunk as P3.1/P3.1b, so the numbers compare with those spikes.
const SRC: &str = r#"
local keep = nil

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

const TABLES: f64 = 4_000_000.0;

/// Same live bytes, different object graph. One cycle each, one internal step
/// per call, so a single call is the smallest slice Luau can be asked for.
const SHAPES: [(&str, &str); 4] = [
    (
        "array of `{i}` tables (P3.1b)",
        "keep = {}
         for i = 1, n do keep[i] = { i } end",
    ),
    (
        "linked list of `{v, next}` tables",
        "local t = nil
         for i = 1, n do t = { v = i, next = t } end
         keep = t",
    ),
    (
        "array of numbers",
        "keep = {}
         for i = 1, n do keep[i] = i end",
    ),
    (
        "array of interned strings",
        "keep = {}
         for i = 1, n do keep[i] = tostring(i) end",
    ),
];

/// How the worst step scales with the slot count of one live table.
///
/// # Errors
/// Propagates script and collector errors.
pub fn slots() -> Result<(), String> {
    println!("# One table's slot count vs the worst single step");
    println!();
    println!(
        "`keep[i] = {{i}}` at several sizes, one full collect, then one incremental cycle with a \
         0 KiB budget."
    );
    println!();
    println!(
        "| slots | heap KiB | full collect ms | steps/cycle | p50 µs | max µs | ns/slot | slowest step |"
    );
    println!("|---:|---:|---:|---:|---:|---:|---:|---|");
    for slots in [
        125_000.0,
        250_000.0,
        500_000.0,
        1_000_000.0,
        2_000_000.0,
        4_000_000.0,
    ] {
        let lua = Lua::new();
        lua.load("function build(n) keep = {} for i = 1, n do keep[i] = { i } end return n end")
            .exec()
            .map_err(|e| format!("load: {e}"))?;
        let build: Function = lua.globals().get("build").map_err(|e| format!("{e}"))?;
        build.call::<()>(slots).map_err(|e| format!("build: {e}"))?;
        let state = lua.state();
        let heap = host::used_bytes(state);
        let started = Instant::now();
        lua.gc_collect().map_err(|e| format!("collect: {e}"))?;
        let collect_ms = started.elapsed().as_secs_f64() * 1000.0;
        let run = gc::step_until_cycles(state, 0, 1, 8_000_000);
        let (index, ns) = run.slowest(1).first().copied().unwrap_or((0, 0));
        println!(
            "| {slots:.0} | {} | {collect_ms:.1} | {} | {:.1} | {:.1} | {:.2} | #{index} |",
            heap / 1024,
            run.steps,
            run.p50() as f64 / 1000.0,
            ns as f64 / 1000.0,
            ns as f64 / slots,
        );
    }
    Ok(())
}

/// Builds each [`SHAPES`] heap, collects once, then steps one cycle and reports
/// the worst single call. A stall that survives every shape is in the
/// collector's fixed phases; one that follows a single shape is in that
/// object's traversal.
///
/// # Errors
/// Propagates script and collector errors.
pub fn shapes() -> Result<(), String> {
    println!("# Same live data, different object graph");
    println!();
    println!(
        "Each shape holds roughly the same payload. One full collect, then one incremental cycle \
         with a 0 KiB budget, i.e. exactly one internal step per call."
    );
    println!();
    println!(
        "| shape | heap KiB | full collect ms | steps/cycle | p50 µs | max µs | slowest steps µs |"
    );
    println!("|---|---:|---:|---:|---:|---:|---|");
    for (name, body) in SHAPES {
        let lua = Lua::new();
        let src = format!("function build(n)\n{body}\nreturn n\nend");
        lua.load(&src)
            .exec()
            .map_err(|e| format!("load {name}: {e}"))?;
        let build: Function = lua.globals().get("build").map_err(|e| format!("{e}"))?;
        build
            .call::<()>(TABLES)
            .map_err(|e| format!("build {name}: {e}"))?;
        let state = lua.state();
        let heap = host::used_bytes(state);
        let started = Instant::now();
        lua.gc_collect()
            .map_err(|e| format!("collect {name}: {e}"))?;
        let collect_ms = started.elapsed().as_secs_f64() * 1000.0;
        let run = gc::step_until_cycles(state, 0, 1, 4_000_000);
        let slow: Vec<String> = run
            .slowest(3)
            .iter()
            .map(|(index, ns)| format!("#{index}: {:.0}", *ns as f64 / 1000.0))
            .collect();
        println!(
            "| {name} | {} | {collect_ms:.1} | {} | {:.1} | {:.1} | {} |",
            heap / 1024,
            run.steps,
            run.p50() as f64 / 1000.0,
            run.max() as f64 / 1000.0,
            slow.join("; "),
        );
    }
    Ok(())
}

/// Builds the heap, then reports the first cycle, the full collect and the
/// steady-state cycles.
///
/// # Errors
/// Propagates script, collector and memory-read errors.
pub fn run() -> Result<(), String> {
    let lua = Lua::new();
    lua.load(SRC).exec().map_err(|e| format!("load: {e}"))?;
    let build: Function = lua
        .globals()
        .get("build")
        .map_err(|e| format!("build: {e}"))?;
    let started = Instant::now();
    build
        .call::<()>(TABLES)
        .map_err(|e| format!("build heap: {e}"))?;
    let build_ms = started.elapsed().as_secs_f64() * 1000.0;
    let state = lua.state();
    let after_build = host::used_bytes(state);
    println!("# Synthetic heap (P3.1b shape)");
    println!();
    println!(
        "| measure | value |\n|---|---:|\n| tables | {TABLES:.0} |\n| build | {build_ms:.0} ms |\n| heap after build | {} KiB |",
        after_build / 1024
    );
    println!();
    println!("## First cycle after the build (collector ran through the build)");
    println!();
    println!(
        "| step budget KiB | cycles | steps/cycle | p50 µs | p99 µs | max µs | reclaimed KiB |"
    );
    println!("|---:|---:|---:|---:|---:|---:|---:|");
    for kb in [0, 64] {
        let run = gc::step_until_cycles(state, kb, 1, 4_000_000);
        println!(
            "| {kb} | {} | {} | {:.1} | {:.1} | {:.1} | {} |",
            run.cycles,
            run.steps.checked_div(run.cycles).unwrap_or(0),
            run.p50() as f64 / 1000.0,
            run.p99() as f64 / 1000.0,
            run.max() as f64 / 1000.0,
            run.reclaimed() / 1024,
        );
        let slow: Vec<String> = run
            .slowest(3)
            .iter()
            .map(|(index, ns)| format!("step {index}: {:.1} ms", *ns as f64 / 1_000_000.0))
            .collect();
        println!("| | | | | | | {} |", slow.join("; "));
    }
    let started = Instant::now();
    lua.gc_collect().map_err(|e| format!("collect: {e}"))?;
    let collect_ms = started.elapsed().as_secs_f64() * 1000.0;
    let after_collect = host::used_bytes(state);
    println!();
    println!(
        "Full collect after the build: **{collect_ms:.1} ms**, heap {} -> {} KiB ({} KiB garbage).",
        after_build / 1024,
        after_collect / 1024,
        (after_build as i64 - after_collect as i64) / 1024
    );
    println!();
    println!("## Steady-state cycles after one full collect");
    println!();
    println!(
        "| step budget KiB | cycles | steps/cycle | p50 µs | p99 µs | max µs | max per cycle µs |"
    );
    println!("|---:|---:|---:|---:|---:|---:|---|");
    let cycles = crate::env_u64("LUAU_GC_CYCLES", 3);
    for kb in [0, 64] {
        lua.gc_collect().map_err(|e| format!("collect: {e}"))?;
        let run = gc::step_until_cycles(state, kb, cycles, 8_000_000);
        let per_cycle: Vec<String> = run
            .cycle_max_ns
            .iter()
            .map(|ns| format!("{:.1}", *ns as f64 / 1000.0))
            .collect();
        println!(
            "| {kb} | {} | {} | {:.1} | {:.1} | {:.1} | {} |",
            run.cycles,
            run.steps.checked_div(run.cycles).unwrap_or(0),
            run.p50() as f64 / 1000.0,
            run.p99() as f64 / 1000.0,
            run.max() as f64 / 1000.0,
            per_cycle.join(", "),
        );
    }
    Ok(())
}

/// The atomic phase clears the main thread's unused stack (`clearstack`), so its
/// cost follows the stack's allocated size, not the live data. This grows the
/// main stack with a wide call, then times one cycle (steps of 0 KiB) with a
/// small live heap.
///
/// # Errors
/// Propagates script and collector errors.
pub fn stack() -> Result<(), String> {
    println!("# Main-thread stack size vs the worst step (small heap)");
    println!();
    println!(
        "A call with `n` arguments grows the main thread's stack to about `n` slots. The heap is \
         a 1e5-node linked list, so no single table is large. Each row: one full collect, then \
         4 cycles at 0 KiB per step."
    );
    println!();
    println!(
        "| stack grown to (slots) | stack bytes (`lua_gc` heap delta) | cycle | steps | max µs | slowest step µs |"
    );
    println!("|---:|---:|---:|---:|---:|---|");
    for n in [0_u64, 50_000, 200_000, 400_000] {
        let lua = Lua::new();
        lua.load(
            "keep = nil for i = 1, 100000 do keep = { v = i, next = keep } end
             function spread(...) return select('#', ...) end
             function grow(n)
                 local t = table.create(n, 0)
                 for i = 1, n do t[i] = i end
                 return spread(table.unpack(t))
             end",
        )
        .exec()
        .map_err(|e| format!("load: {e}"))?;
        let state = lua.state();
        if n > 0 {
            let grow: Function = lua
                .globals()
                .get("grow")
                .map_err(|e| format!("grow: {e}"))?;
            grow.call::<i64>(n).map_err(|e| format!("grow {n}: {e}"))?;
        }
        lua.gc_collect().map_err(|e| format!("collect: {e}"))?;
        for cycle in 1..=4 {
            let run = gc::step_until_cycles(state, 0, 1, 4_000_000);
            let (index, ns) = run.slowest(1).first().copied().unwrap_or((0, 0));
            println!(
                "| {n} | {} | {cycle} | {} | {:.1} | #{index}: {:.1} |",
                host::used_bytes(state),
                run.steps,
                run.max() as f64 / 1000.0,
                ns as f64 / 1000.0,
            );
        }
    }
    Ok(())
}
