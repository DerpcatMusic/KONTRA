//! Per-event cost of one note callback, interpreter vs Luau, plus wait-frame
//! invariance across render block sizes (32/64/512/4096 frames, 48 kHz).
//! Usage: cargo run -p sampler-ksp --release --example luau_bench
//! Interpreter event = `trigger` + one 64-frame render (callback runs inside it;
//! the voice is idle, no region). Luau event = `note_on` + `advance_to`.
use sampler_core::*;
use sampler_core::ScriptInstanceId;
use sampler_ksp::luau::{host::Host, lower};
use std::time::Instant;

const SCRIPT: &str = "on init declare $x := 0 declare %t[8] declare $i := 0 end on
on note
  $i := 0
  while ($i < 8)
    %t[$i] := %t[$i] + $i * 3
    inc($i)
  end while
  $x := $x + %t[3]
  wait(1000)
  $x := $x + 1
end on";

const EVENTS: usize = 20_000;

fn pct(sorted: &[u128], p: f64) -> u128 {
    sorted[((sorted.len() as f64 - 1.0) * p).round() as usize]
}

fn report(name: &str, mut ns: Vec<u128>) {
    ns.sort_unstable();
    println!(
        "{name:12} p50 {:6} ns  p99 {:6}  p99.9 {:6}  max {:8}  (n={})",
        pct(&ns, 0.5),
        pct(&ns, 0.99),
        pct(&ns, 0.999),
        ns.last().unwrap(),
        ns.len()
    );
}

fn interp_runtime() -> Runtime {
    let script = sampler_ksp::compile(
        SCRIPT,
        48_000,
        sampler_ksp::Limits { source_bytes: 4096, instructions: 4096, variables: 8, array_cells: 16 },
        &[],
    )
    .unwrap();
    let plan = script
        .bind(Prepared::new(48_000, vec![], vec![], 0).unwrap())
        .unwrap();
    let cells = plan.behavior_local_count() * 2;
    Runtime::new(
        plan,
        Limits {
            notes: 4,
            channels: 0,
            performances: 1,
            families: 4,
            expressions: 4,
            voices: 4,
            decisions: 0,
            commands: 8,
            behaviors: 2,
            behavior_fuel: Limits::DEFAULT_BEHAVIOR_FUEL,
            behavior_cells: cells,
            note_cells: 4,
        },
    )
    .unwrap()
}

fn input(id: i32) -> Input {
    Input { protocol: Protocol::Clap, port: 0, group: 0, channel: 0, key: 60, external_id: Some(id) }
}

fn main() {
    let mut block = [[0.0_f32; 2]; 64];
    // Interpreter: one note per block, each block timed.
    let mut rt = interp_runtime();
    let mut interp = Vec::with_capacity(EVENTS);
    for i in 0..EVENTS {
        let t = Instant::now();
        let _ = rt.trigger(input(i as i32), 60, 1.0);
        rt.render(&mut block).unwrap();
        interp.push(t.elapsed().as_nanos());
        let _ = rt.note_off(input(i as i32), None);
        rt.flush_ended(|_| true);
    }
    // Luau: one note per 64 frames, same cadence.
    let lowered = lower(SCRIPT).unwrap();
    assert!(lowered.compiled.is_complete(), "{:?}", lowered.compiled.unsupported);
    let mut host = Host::new(&lowered.compiled, lowered.cells, lowered.texts, lowered.note_cells, 48_000).unwrap();
    host.init().unwrap();
    let mut luau = Vec::with_capacity(EVENTS);
    for i in 0..EVENTS {
        let t = Instant::now();
        host.note_on(i as u32, 60, 1.0).unwrap();
        host.advance_to(host.now() + 64).unwrap();
        luau.push(t.elapsed().as_nanos());
        host.release(i as u32, 60, 1.0).unwrap();
        host.advance_to(host.now() + 64).unwrap();
    }
    report("interpreter", interp);
    report("luau", luau);

    // Wait invariance: frame at which the note's wait resumes, for each block size.
    println!("wait(1000) started at frame 0 (48 kHz: resumes at index 48, visible after 49 rendered frames):");
    for block_size in [32_u64, 64, 512, 4096] {
        let mut host = Host::new(&lowered.compiled, lowered.cells, lowered.texts, lowered.note_cells, 48_000).unwrap();
        host.init().unwrap();
        host.note_on(1, 60, 1.0).unwrap();
        let mut resumed_at = None;
        let mut end = 0;
        while end < 4096 {
            end = (end + block_size).min(4096);
            host.advance_to(end).unwrap();
            // $x is cell 0: 9 from the loop, 10 after the wait.
            if resumed_at.is_none() && host.cells(lowered.cells)[0] >= 10.0 {
                resumed_at = Some(end);
            }
        }
        println!("  block {block_size:5}: first visible after {} frames", resumed_at.unwrap_or(0));
    }

    println!("interpreter, same script, trigger at frame 0, render in blocks of B (visible after N frames):");
    for block_size in [32_usize, 64, 512, 4096] {
        let mut rt = interp_runtime();
        let plan = rt.active_plan();
        let _ = rt.trigger(input(1), 60, 1.0);
        let (mut rendered, mut visible) = (0_usize, 0_usize);
        let mut buf = vec![[0.0_f32; 2]; block_size];
        while rendered < 4096 && visible == 0 {
            rt.render(&mut buf).unwrap();
            rendered += block_size;
            if rt.script_cell(plan, ScriptInstanceId(0), 0).is_ok_and(|v| v >= 10) {
                visible = rendered;
            }
        }
        println!("  block {block_size:5}: first visible after {visible} frames");
    }
}
