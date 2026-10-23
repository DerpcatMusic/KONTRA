//! DAW-criteria numbers for the script runtime: per-callback latency
//! percentiles (with the work-budget hook installed), GC slice pauses, full
//! collect pause, per-VM creation cost and footprint, and runaway-abort time.
//! Usage: cargo run --release -p sampler-script --example daw_bench [N]
use std::{rc::Rc, time::Instant};

use sampler_script::{Budget, Jit, bounded_gc_step, new_vm_with};

fn pct(sorted: &[f64], p: f64) -> f64 {
    sorted[((sorted.len() as f64 - 1.0) * p).round() as usize]
}

fn report(name: &str, mut ns: Vec<f64>) {
    ns.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!(
        "{name:<28} p50 {:8.0}  p99 {:8.0}  p99.9 {:8.0}  max {:9.0} ns",
        pct(&ns, 0.5),
        pct(&ns, 0.99),
        pct(&ns, 0.999),
        ns[ns.len() - 1]
    );
}

fn main() -> mlua::Result<()> {
    let n: usize = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(20_000);
    const NOTE: &str = "slots={} for i=1,16 do slots[i]=i end
        function onNote(note, vel)
          local acc = 0
          for k = 1, 8 do acc = acc + (note * k + vel) % 7 end
          slots[(note % 16) + 1] = acc
          return acc
        end";
    for (label, jit) in [("interp", Jit::Interpreter), ("native", Jit::Native)] {
        let t = Instant::now();
        let lua = new_vm_with(64 << 20, jit)?;
        let create_us = t.elapsed().as_secs_f64() * 1e6;
        let used_empty = lua.used_memory();
        lua.load(NOTE).exec()?;
        let used_loaded = lua.used_memory();
        println!("[{label}] VM create {create_us:.0} us, used {} B empty, {} B with callback chunk", used_empty, used_loaded);
        let budget = Rc::new(Budget::new(u64::MAX, None));
        let hook = Rc::clone(&budget);
        lua.set_interrupt(move |_| {
            hook.checkpoint()?;
            Ok(mlua::VmState::Continue)
        });
        let on_note: mlua::Function = lua.globals().get("onNote")?;
        let mut ns = Vec::with_capacity(n);
        for i in 0..n {
            budget.refill(u64::MAX, None);
            let t = Instant::now();
            let _: i64 = on_note.call((60 + (i % 24) as i64, 100i64))?;
            ns.push(t.elapsed().as_nanos() as f64);
        }
        report(&format!("[{label}] onNote (hooked)"), ns);
        // GC: one 8-step slice per 64-frame block, against a garbage-heavy callback.
        lua.set_interrupt(|_| Ok(mlua::VmState::Continue));
        lua.load("for i=1,20000 do local t={i} end").exec()?;
        let mut slices = Vec::new();
        for _ in 0..2000 {
            let t = Instant::now();
            bounded_gc_step(&lua, 8)?;
            slices.push(t.elapsed().as_nanos() as f64);
        }
        report(&format!("[{label}] gc slice (8 steps)"), slices);
        let heap_before = lua.used_memory();
        let t = Instant::now();
        lua.gc_collect()?;
        println!("[{label}] full gc_collect {:.2} ms, heap {} -> {} B", t.elapsed().as_secs_f64() * 1e3, heap_before, lua.used_memory());
    }
    // Runaway abort wall time with a 1e6-checkpoint budget.
    let lua = new_vm_with(64 << 20, Jit::Interpreter)?;
    let budget = Rc::new(Budget::new(1_000_000, None));
    let hook = Rc::clone(&budget);
    lua.set_interrupt(move |_| {
        hook.checkpoint()?;
        Ok(mlua::VmState::Continue)
    });
    let t = Instant::now();
    let _ = lua.load("while true do end").exec();
    println!("runaway loop aborted after {} checkpoints in {:.2} ms", budget.checkpoints.get(), t.elapsed().as_secs_f64() * 1e3);
    Ok(())
}
