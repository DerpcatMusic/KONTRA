//! Differential harness: every `.ksp` in a directory runs through the interpreter
//! (sampler-core `Runtime`) and the Luau path (`luau::host::Host`) on one synthetic
//! event trace, and the script state is compared after every event.
//!
//! Usage: cargo run -p sampler-ksp --example ksp_diff --release -- [dir] [--verbose]
//! (dir defaults to `$KSP_CORPUS`, then `/data/dev/workspace/scratch/ksp-luau-corpus`;
//! extract it with `sampler-kontakt`'s `ksp_corpus_extract`). Exit code 1 on any mismatch.
//!
//! Trace per script: note-on over a key x velocity grid, each followed by a 64-frame
//! render, then note-off and another render, so `wait` timers and the release
//! callback run on both paths at the same frames. Compared after every step: every
//! global script cell (reals bit for bit) and the `on init` result.
//! Not yet in the trace: controllers, UI control changes and engine-write traces during
//! events. The Luau front end does not run those callbacks yet (see the census
//! histogram), and the interpreter exposes no event-time engine-write trace.
use sampler_core::{ChannelAddress, Frame, Input, Prepared, Protocol, Runtime, ScriptInstanceId, Limits as CoreLimits};
use sampler_ksp::luau::{check_init, host::Host, lower};
use sampler_script::Jit;
use std::collections::BTreeMap;
use std::path::PathBuf;

const RATE: u32 = 48_000;
const BLOCK: usize = 64;
const KEYS: [u8; 3] = [36, 60, 96];
/// Velocities in KSP's 0..127 scale (the script sees `round(v * 127)`).
const VELOCITIES: [u8; 3] = [1, 64, 127];
/// Blocks rendered after each event step (64 frames each).
const SETTLE_BLOCKS: usize = 4;
/// Controller messages on host channel 0 after the notes: a CC, a mid-range CC, pitch bend.
/// Values are the runtime's raw 32-bit controller values (0 .. u32::MAX).
const CONTROLLERS: [(u8, u32); 6] = [(1, 0), (1, 1 << 31), (64, u32::MAX), (64, 1 << 30), (128, 0), (128, u32::MAX)];
/// Downstream channel mask for channel 0 (the runtime's `channels` argument).
const CHANNEL_MASK: u16 = 1;

#[derive(Default)]
struct Tally {
    total: usize,
    frontend: Vec<String>,
    outside: usize,
    unsupported: BTreeMap<String, usize>,
    bind_failed: Vec<String>,
    init_mismatch: Vec<String>,
    event_mismatch: Vec<String>,
    matched: usize,
    events: usize,
    matched_native: usize,
    native_mismatch: Vec<String>,
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let verbose = args.iter().any(|a| a == "--verbose");
    args.retain(|a| a != "--verbose");
    let dir = args
        .first()
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("KSP_CORPUS").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("/data/dev/workspace/scratch/ksp-luau-corpus"));
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("corpus dir {}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "ksp"))
        .collect();
    paths.sort();

    let mut tally = Tally { total: paths.len(), ..Tally::default() };
    for path in &paths {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let source = String::from_utf8_lossy(&std::fs::read(path).expect("read script")).into_owned();
        let lowered = match lower(&source) {
            Ok(l) => l,
            Err(e) => {
                tally.frontend.push(format!("{name}: {}", e.chars().take(90).collect::<String>()));
                continue;
            }
        };
        if !lowered.compiled.is_complete() {
            tally.outside += 1;
            for (construct, n) in &lowered.compiled.unsupported {
                *tally.unsupported.entry(construct.clone()).or_default() += n;
            }
            continue;
        }
        if let Err(e) = check_init(&lowered) {
            tally.init_mismatch.push(format!("{name}: {e}"));
            continue;
        }
        let mut interp = match interpreter(&source) {
            Ok(rt) => rt,
            Err(e) => {
                tally.bind_failed.push(format!("{name}: {e}"));
                continue;
            }
        };
        match trace(&lowered, &mut interp, Jit::Interpreter) {
            Ok(n) => {
                tally.matched += 1;
                tally.events += n;
            }
            Err(e) => {
                tally.event_mismatch.push(format!("{name}: {e}"));
                continue;
            }
        }
        // The native-code VM must agree with the interpreter too; it is a separate
        // codegen path, so it gets its own check.
        let mut interp = match interpreter(&source) {
            Ok(rt) => rt,
            Err(_) => continue,
        };
        match trace(&lowered, &mut interp, Jit::Native) {
            Ok(_) => tally.matched_native += 1,
            Err(e) => tally.native_mismatch.push(format!("{name}: {e}")),
        }
        if verbose {
            println!("ok  {name}");
        }
    }

    let in_subset = tally.matched + tally.init_mismatch.len() + tally.bind_failed.len() + tally.event_mismatch.len();
    println!("scripts: {}", tally.total);
    println!("front end errors: {}", tally.frontend.len());
    for f in tally.frontend.iter().take(8) {
        println!("  {f}");
    }
    println!("outside the Luau subset: {}", tally.outside);
    println!("in subset: {in_subset}");
    println!("  matched, interpreted VM: {} ({} events compared)", tally.matched, tally.events);
    println!("  matched, native VM:      {}", tally.matched_native);
    println!("  init mismatches:         {}", tally.init_mismatch.len());
    println!("  event-trace mismatches:  {}", tally.event_mismatch.len() + tally.native_mismatch.len());
    println!("  interpreter bind failed: {}", tally.bind_failed.len());
    for m in tally.init_mismatch.iter().chain(&tally.event_mismatch).chain(&tally.native_mismatch).take(20) {
        println!("  MISMATCH {m}");
    }
    for m in tally.bind_failed.iter().take(8) {
        println!("  BIND {m}");
    }
    let mut top: Vec<_> = tally.unsupported.into_iter().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1));
    println!("unsupported constructs (sites, scripts outside the subset):");
    for (construct, n) in top.iter().take(40) {
        println!("  {n:6}  {construct}");
    }
    if !tally.init_mismatch.is_empty() || !tally.event_mismatch.is_empty() || !tally.native_mismatch.is_empty() {
        std::process::exit(1);
    }
}

/// Interpreter runtime for one script, bound the way the bench binds it.
fn interpreter(source: &str) -> Result<Runtime, String> {
    let script = sampler_ksp::compile(
        source,
        RATE,
        sampler_ksp::Limits { source_bytes: 1 << 20, instructions: 1 << 20, variables: 4096, array_cells: 1 << 16 },
        &[],
    )
    .map_err(|e| format!("compile: {e:?}"))?;
    let prepared = Prepared::new(RATE, vec![], vec![], 0).map_err(|e| format!("prepare: {e:?}"))?;
    let plan = script.bind(prepared).map_err(|e| format!("bind: {e:?}"))?;
    let cells = plan.behavior_local_count() * 2;
    Runtime::new(
        plan,
        CoreLimits {
            notes: 4,
            channels: 2,
            performances: 1,
            families: 4,
            expressions: 4,
            voices: 4,
            decisions: 0,
            commands: 8,
            behaviors: 2,
            behavior_fuel: CoreLimits::DEFAULT_BEHAVIOR_FUEL,
            behavior_cells: cells,
            note_cells: 4,
        },
    )
        .map_err(|e| format!("runtime: {e:?}"))
}

fn input(id: i32) -> Input {
    Input { protocol: Protocol::Clap, port: 0, group: 0, channel: 0, key: 60, external_id: Some(id) }
}

/// Run the event trace on one Luau VM and the interpreter; `Ok(events)` when every step agrees.
fn trace(lowered: &sampler_ksp::luau::Lowered, rt: &mut Runtime, jit: Jit) -> Result<usize, String> {
    let plan = rt.active_plan();
    let mut host = Host::new_with(&lowered.compiled, lowered.cells, lowered.texts, lowered.note_cells, RATE, jit)?;
    host.init()?;
    let mut block: Vec<Frame> = vec![[0.0; 2]; BLOCK];
    let mut id = 0_i32;
    let mut events = 0;
    for &key in &KEYS {
        for &vel in &VELOCITIES {
            id += 1;
            let velocity = f64::from(vel) / 127.0;
            // Interpreter: trigger, settle; Luau: note_on, settle at the same frame.
            rt.trigger(input(id), key, velocity).map_err(|e| format!("trigger: {e:?}"))?;
            host.note_on(id as u32, i64::from(key), f64::from(vel))?;
            settle(rt, &mut host, &mut block, lowered, plan, &format!("note {key}/{vel}"))?;
            rt.note_off(input(id), None).map_err(|e| format!("note_off: {e:?}"))?;
            host.release(id as u32, i64::from(key), f64::from(vel))?;
            settle(rt, &mut host, &mut block, lowered, plan, &format!("release {key}/{vel}"))?;
            rt.flush_ended(|_| true);
            events += 2;
        }
    }
    // Controllers: each message runs `on controller` on both paths. The downstream value on
    // the channel must be the forwarded value, or the value it had before when the body ignored it.
    let performance = rt.performance(0).map_err(|e| format!("performance: {e:?}"))?;
    let origin = ChannelAddress { protocol: Protocol::Clap, port: 0, group: 0, channel: 0 };
    for &(number, value) in &CONTROLLERS {
        let before = rt.channel_controller(performance, origin, number).map_err(|e| format!("read cc {number}: {e:?}"))?;
        let forwarded = host.controller(number, value)?;
        rt.dispatch_controller(performance, origin, CHANNEL_MASK, number, value).map_err(|e| format!("dispatch cc {number}: {e:?}"))?;
        settle(rt, &mut host, &mut block, lowered, plan, &format!("cc {number}={value}"))?;
        let downstream = rt.channel_controller(performance, origin, number).map_err(|e| format!("read cc {number}: {e:?}"))?;
        let expected = if forwarded { value } else { before };
        if downstream != expected {
            return Err(format!("cc {number}={value}: interpreter downstream {downstream}, expected {expected} (forwarded: {forwarded})"));
        }
        events += 1;
    }
    Ok(events)
}

fn settle(
    rt: &mut Runtime,
    host: &mut Host,
    block: &mut [Frame],
    lowered: &sampler_ksp::luau::Lowered,
    plan: sampler_core::PlanId,
    step: &str,
) -> Result<(), String> {
    for _ in 0..SETTLE_BLOCKS {
        rt.render(block).map_err(|e| format!("render: {e:?}"))?;
        host.advance_to(host.now() + BLOCK as u64)?;
        compare(rt, host, lowered, plan).map_err(|e| format!("after {step}: {e}"))?;
    }
    Ok(())
}

/// Global cells, interpreter vs Luau: reals bit for bit, integers as `i32`.
fn compare(rt: &Runtime, host: &Host, lowered: &sampler_ksp::luau::Lowered, plan: sampler_core::PlanId) -> Result<(), String> {
    let luau = host.cells(lowered.cells);
    for i in 0..lowered.cells as usize {
        let raw = rt
            .script_cell(plan, ScriptInstanceId(0), i as u32)
            .map_err(|e| format!("cell {i} unreadable: {e:?}"))?;
        let expected = if lowered.cell_is_real(i) { f64::from_bits(raw as u64) } else { f64::from(raw as i32) };
        let got = luau[i];
        let same = if expected.is_nan() { got.is_nan() } else { got == expected };
        if !same {
            return Err(format!("cell {i}: interpreter {expected:?}, luau {got:?}"));
        }
    }
    Ok(())
}
