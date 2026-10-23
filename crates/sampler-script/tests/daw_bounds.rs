//! DAW-criteria bounds for the script runtime: a runaway script must stop at a
//! deterministic checkpoint count, a memory-hungry one must fail at the cap,
//! and the VM must stay usable after a budget error (isolation).
use std::{rc::Rc, time::Instant};

use sampler_script::{Budget, Jit, bounded_gc_step, new_vm, new_vm_with};

fn hooked(lua: &mlua::Lua, work: u64) -> Rc<Budget> {
    let budget = Rc::new(Budget::new(work, None));
    let hook = Rc::clone(&budget);
    lua.set_interrupt(move |_| {
        hook.checkpoint()?;
        Ok(mlua::VmState::Continue)
    });
    budget
}

#[test]
fn runaway_loop_stops_at_exact_checkpoint_and_vm_survives() {
    let lua = new_vm(64 << 20).unwrap();
    let budget = hooked(&lua, 10_000);
    let started = Instant::now();
    let err = lua.load("while true do end").exec().unwrap_err().to_string();
    assert!(err.contains("work budget exceeded"), "{err}");
    assert_eq!(budget.checkpoints.get(), 10_001, "stop is deterministic");
    assert!(started.elapsed().as_millis() < 2_000, "abort must be prompt");
    // Isolation: the same VM runs the next callback after a refill.
    budget.refill(10_000, None);
    let v: i64 = lua.load("local s=0 for i=1,100 do s=s+i end return s").eval().unwrap();
    assert_eq!(v, 5050);
}

#[test]
fn memory_cap_fails_the_callback_not_the_process() {
    let lua = new_vm(1 << 20).unwrap();
    let err = lua.load("local t={} for i=1,1e7 do t[i]=i end").exec();
    assert!(err.is_err());
    // Freed garbage lets later small work proceed.
    lua.load("local t={} for i=1,100 do t[i]={} end").exec().unwrap();
}

#[test]
fn gc_steps_are_bounded_and_finish_the_cycle() {
    let lua = new_vm(256 << 20).unwrap();
    lua.load("garbage={} for i=1,50000 do garbage[i]={i} end garbage=nil").exec().unwrap();
    let mut calls = 0usize;
    while !bounded_gc_step(&lua, 8).unwrap() {
        calls += 1;
        assert!(calls < 1_000_000, "cycle must finish");
    }
    assert!(calls > 0, "a large heap needs more than one slice");
}

#[test]
fn jit_policy_is_applied_per_vm() {
    // Interpreter is the default; Native must still run the same chunk.
    let a = new_vm(16 << 20).unwrap();
    let b = new_vm_with(16 << 20, Jit::Native).unwrap();
    for lua in [&a, &b] {
        let v: i64 = lua.load("local s=0 for i=1,10 do s=s+i end return s").eval().unwrap();
        assert_eq!(v, 55);
    }
}
