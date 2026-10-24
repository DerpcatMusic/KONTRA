//! Spike P3.1c: can Luau's collector run inline on the audio thread for large
//! UVI hosts? Answer in `docs/architecture-v3/spikes/luau-gc.md`.
//!
//! ```text
//! luau-gc host <bank.ufs> <program>     # heap decomposition, step cost, notes
//! luau-gc synth                         # the P3.1b 4e6-table heap, re-measured
//! luau-gc floor                         # timing floor of this machine
//! ```
//!
//! Environment knobs: `LUAU_GC_CYCLES`, `LUAU_GC_MAX_STEPS`, `LUAU_GC_BLOCKS`,
//! `LUAU_GC_BLOCK_MS`, `LUAU_GC_NOTE_EVERY`, `LUAU_GC_SKIP` (comma list of
//! `heap,gc,notes`).

mod dump;
mod gc;
mod host;
mod synth;

use std::path::PathBuf;
use std::time::Instant;

use mlua::ffi;

use crate::host::{Collector, Freed, Loaded};

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        usage();
        return;
    };
    let rest: Vec<String> = args.collect();
    let result = match command.as_str() {
        "host" => host_command(&rest),
        "synth" => synth::run(),
        "shapes" => synth::shapes(),
        "slots" => synth::slots(),
        "floor" => {
            println!("timer floor: {} ns", gc::timer_floor_ns());
            Ok(())
        }
        other => {
            eprintln!("unknown command {other}");
            usage();
            std::process::exit(2);
        }
    };
    if let Err(error) = result {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn usage() {
    eprintln!("usage: luau-gc host <bank.ufs> <program> | synth | floor");
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_f64(name: &str, default: f64) -> f64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn skip(section: &str) -> bool {
    std::env::var("LUAU_GC_SKIP")
        .map(|v| v.split(',').any(|s| s.trim() == section))
        .unwrap_or(false)
}

fn host_command(args: &[String]) -> Result<(), String> {
    let [bank, program] = args else {
        return Err("usage: luau-gc host <bank.ufs> <program>".to_owned());
    };
    let bank = PathBuf::from(bank);
    let mut loaded = host::load(&bank, program)?;
    println!("# {}", loaded.label);
    println!();
    if std::env::var_os("LUAU_GC_CLEAR_TOUCHED").is_some() {
        let lua = loaded.host.spike_lua();
        let touched = lua
            .globals()
            .raw_get::<mlua::Table>("__touched")
            .map_err(|e| format!("read __touched: {e}"))?;
        let live = touched.raw_len();
        let fresh = lua.create_table().map_err(|e| format!("create table: {e}"))?;
        lua.globals()
            .raw_set("__touched", fresh)
            .map_err(|e| format!("clear __touched: {e}"))?;
        println!("- `__touched` had {live} entries and was replaced after prepare");
    }
    println!(
        "- script members: {}, program XML: {} KiB, host init: {:.1} ms",
        loaded.members,
        loaded.xml_bytes / 1024,
        loaded.init_ms
    );
    println!(
        "- timer floor: {} ns; audio block assumption: {:.2} ms",
        gc::timer_floor_ns(),
        env_f64("LUAU_GC_BLOCK_MS", 1.33)
    );
    if !skip("heap") {
        heap_section(&loaded)?;
    }
    if !skip("gc") {
        gc_section(&loaded)?;
    }
    if !skip("notes") {
        notes_section(&mut loaded)?;
    }
    if !skip("teardown") {
        teardown_section(&loaded)?;
    }
    Ok(())
}

/// Used memory, the cost of one full collection, the exact per-type heap split
/// and the teardown attribution.
fn heap_section(loaded: &Loaded) -> Result<(), String> {
    let lua = loaded.host.spike_lua();
    let state = lua.state();
    let after_init = host::used_bytes(state);
    println!();
    println!("## Heap");
    println!();
    let started = Instant::now();
    lua.gc_collect().map_err(|e| format!("collect: {e}"))?;
    let collect_ms = started.elapsed().as_secs_f64() * 1000.0;
    let after_collect = host::used_bytes(state);
    println!(
        "| measure | KiB | MiB |\n|---|---:|---:|\n| after init | {} | {:.1} |\n| after one full collect | {} | {:.1} |\n| garbage after init | {} | {:.1} |",
        after_init / 1024,
        after_init as f64 / (1024.0 * 1024.0),
        after_collect / 1024,
        after_collect as f64 / (1024.0 * 1024.0),
        (after_init as i64 - after_collect as i64) / 1024,
        (after_init as i64 - after_collect as i64) as f64 / (1024.0 * 1024.0),
    );
    println!();
    println!("One full collect after init: {collect_ms:.2} ms.");
    println!();
    let path = std::env::temp_dir().join("kontra-luau-gc-dump.json");
    let totals = dump::dump(lua, &path)?;
    println!("Exact Luau heap split (`lua_memorydump`, {:.1} MiB):", totals.bytes as f64 / (1024.0 * 1024.0));
    println!();
    println!("| type | objects | KiB | % of heap | bytes/object |");
    println!("|---|---:|---:|---:|---:|");
    for total in &totals.by_type {
        println!(
            "| {} | {} | {} | {:.1} | {} |",
            total.kind,
            total.objects,
            total.bytes / 1024,
            100.0 * total.bytes as f64 / totals.bytes.max(1) as f64,
            total.bytes / total.objects.max(1),
        );
    }
    println!("| **total** | {} | {} | 100.0 | |", totals.objects, totals.bytes / 1024);
    println!();
    println!("### Largest single objects");
    println!();
    println!(
        "`traversetable` walks a table's whole array and node capacity inside one `propagatemark`, \
         so a single table is an indivisible stall. The synthetic law is 2.7–4.5 ns per 16-byte \
         slot (0.17–0.28 ns/byte); the last column applies 0.22 ns/byte."
    );
    println!();
    println!("| # | type | KiB | % of heap | predicted walk |");
    println!("|---:|---|---:|---:|---:|");
    for (rank, object) in totals.largest.iter().enumerate() {
        println!(
            "| {} | {} | {} | {:.1} | {:.2} ms |",
            rank + 1,
            object.kind,
            object.bytes / 1024,
            100.0 * object.bytes as f64 / totals.bytes.max(1) as f64,
            object.bytes as f64 * 0.22 / 1_000_000.0,
        );
    }
    Ok(())
}

/// Which globals hold the heap. Runs last: it drops everything.
fn teardown_section(loaded: &Loaded) -> Result<(), String> {
    let lua = loaded.host.spike_lua();
    let state = lua.state();
    let globals = lua.globals();
    println!();
    println!("### Teardown: bytes freed by dropping each global (depth 2)");
    println!();
    println!("| dropped | KiB | MiB |");
    println!("|---|---:|---:|");
    let mut freed: Vec<Freed> = Vec::new();
    host::teardown(lua, &globals, "_G", 2, &mut freed)?;
    for row in &freed {
        if row.bytes.abs() < 64 * 1024 {
            continue;
        }
        println!("| `{}` | {} | {:.1} |", row.path, row.kib(), row.bytes as f64 / (1024.0 * 1024.0));
    }
    let biggest: Vec<&Freed> = freed.iter().filter(|r| r.bytes > 0).collect();
    println!(
        "\n{} globals dropped, {} with more than 64 KiB freed.",
        freed.len(),
        biggest.len()
    );
    println!(
        "heap after teardown: {} KiB",
        host::used_bytes(state) / 1024
    );
    Ok(())
}

/// Worst single collector call at several step budgets and tunings.
fn gc_section(loaded: &Loaded) -> Result<(), String> {
    let lua = loaded.host.spike_lua();
    let state = lua.state();
    let cycles = env_u64("LUAU_GC_CYCLES", 3);
    let max_steps = env_u64("LUAU_GC_MAX_STEPS", 2_000_000);
    println!();
    println!("## Collector cost");
    println!();
    println!(
        "Each row: `LUA_GCSTEP` with a `kb` budget, run for {cycles} cycles after one full collect. \
         `max` is the worst single call, i.e. the worst stall an inline host would add."
    );
    println!();
    println!("| goal % | stepmul % | stepsize KiB | step budget KiB | steps/cycle | p50 µs | p99 µs | max µs | first-cycle max µs | steady max µs | reclaimed KiB |");
    println!("|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|");
    let tunings = [
        (gc::DEFAULT_GOAL, gc::DEFAULT_STEPMUL, gc::DEFAULT_STEPSIZE_KB),
        (1000, gc::DEFAULT_STEPMUL, gc::DEFAULT_STEPSIZE_KB),
        (gc::DEFAULT_GOAL, 100, gc::DEFAULT_STEPSIZE_KB),
        (gc::DEFAULT_GOAL, 400, gc::DEFAULT_STEPSIZE_KB),
        (gc::DEFAULT_GOAL, 1000, gc::DEFAULT_STEPSIZE_KB),
        (gc::DEFAULT_GOAL, gc::DEFAULT_STEPMUL, 4),
        (gc::DEFAULT_GOAL, gc::DEFAULT_STEPMUL, 16),
    ];
    let budgets = [0, 1, 8, 64];
    for (index, (goal, stepmul, stepsize)) in tunings.into_iter().enumerate() {
        let budgets: &[i32] = if index == 0 { &budgets } else { &[0] };
        for &kb in budgets {
            gc::set_params(lua, goal, stepmul, stepsize);
            lua.gc_collect().map_err(|e| format!("collect: {e}"))?;
            let run = gc::step_until_cycles(state, kb, cycles, max_steps);
            let steady = if run.cycle_max_ns.len() > 1 {
                run.cycle_max_ns[1..].iter().copied().max().unwrap_or(0)
            } else {
                0
            };
            let first = run.cycle_max_ns.first().copied().unwrap_or(0);
            let per_cycle = if run.cycles == 0 {
                0
            } else {
                run.steps / run.cycles
            };
            println!(
                "| {goal} | {stepmul} | {stepsize} | {kb} | {per_cycle} | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} | {} |",
                run.p50() as f64 / 1000.0,
                run.p99() as f64 / 1000.0,
                run.max() as f64 / 1000.0,
                first as f64 / 1000.0,
                steady as f64 / 1000.0,
                run.reclaimed() / 1024,
            );
            if run.cycles > 0 {
                let slow: Vec<String> = run
                    .slowest(3)
                    .iter()
                    .map(|(index, ns)| format!("step {index}: {:.1} µs", *ns as f64 / 1000.0))
                    .collect();
                println!(
                    "| | | | | | | | | | | | {} |",
                    slow.join("; ")
                );
            }
        }
    }
    gc::set_params(
        lua,
        gc::DEFAULT_GOAL,
        gc::DEFAULT_STEPMUL,
        gc::DEFAULT_STEPSIZE_KB,
    );
    Ok(())
}

/// The note stream with the collector stopped and with one bounded step per block.
fn notes_section(loaded: &mut Loaded) -> Result<(), String> {
    let state = loaded.host.spike_lua().state();
    let blocks = env_u64("LUAU_GC_BLOCKS", 20_000);
    let block_ms = env_f64("LUAU_GC_BLOCK_MS", 1.33);
    let note_every = env_u64("LUAU_GC_NOTE_EVERY", 67);
    let handles = loaded.host.handles_notes();
    println!();
    println!("## Note stream");
    println!();
    println!(
        "`handles_notes` = {handles}; {blocks} blocks of {block_ms:.2} ms, one note every {note_every} blocks."
    );
    println!();
    println!("| collector | cycles | gc p99 µs | gc max µs | script p99 µs | script max µs | block p99 µs | block max µs | heap change KiB |");
    println!("|---|---:|---:|---:|---:|---:|---:|---:|---:|");
    for collector in [
        Collector::Stopped,
        Collector::Step(0),
        Collector::Step(8),
        Collector::Step(64),
    ] {
        // SAFETY: live state of the host under measurement. A full collect runs
        // finalizers, which for this state are Luau-internal only.
        unsafe { ffi::lua_gc(state, ffi::LUA_GCCOLLECT, 0) };
        if matches!(collector, Collector::Stopped) {
            // SAFETY: live state of the host under measurement; stopping the
            // collector only writes the threshold. It must follow the full
            // collect, which resets the threshold and restarts the collector.
            unsafe { ffi::lua_gc(state, ffi::LUA_GCSTOP, 0) };
        }
        let run = host::run_blocks(
            &mut loaded.host,
            state,
            collector,
            blocks,
            block_ms,
            note_every,
        );
        println!(
            "| {} | {} | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} | {} |",
            run.collector,
            run.cycles,
            run.gc_p99_ns as f64 / 1000.0,
            run.gc_max_ns as f64 / 1000.0,
            run.script_p99_ns as f64 / 1000.0,
            run.script_max_ns as f64 / 1000.0,
            run.block_p99_ns as f64 / 1000.0,
            run.block_max_ns as f64 / 1000.0,
            run.memory_delta_kib(),
        );
    }
    Ok(())
}
