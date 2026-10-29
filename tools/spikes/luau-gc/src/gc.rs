//! Collector driving and per-call timing.
//!
//! Every measurement here is one `lua_gc` call on the audio-thread state: the
//! call is the stall an inline script host would add to a block.

use std::time::Instant;

use mlua::{Lua, ffi};

use crate::host;

/// Timing floor of `Instant::now` on this machine, taken once at startup.
/// Values at or below it are noise, so the tables print it next to p50.
pub fn timer_floor_ns() -> u64 {
    let mut best = u64::MAX;
    for _ in 0..1000 {
        let start = Instant::now();
        best = best.min(start.elapsed().as_nanos() as u64);
    }
    best
}

/// Defaults from `luau/VM/src/lgc.h`: goal 200 %, stepmul 200 %, stepsize 1 KiB.
pub const DEFAULT_GOAL: i32 = 200;
pub const DEFAULT_STEPMUL: i32 = 200;
pub const DEFAULT_STEPSIZE_KB: i32 = 1;

/// Sets the Luau incremental collector parameters.
///
/// `goal` is the heap ceiling as a percentage of live data, `stepmul` the work
/// per unit of allocation, and `stepsize_kb` the granularity of one internal
/// step.
pub fn set_params(lua: &Lua, goal: i32, stepmul: i32, stepsize_kb: i32) {
    // SAFETY: `lua.state()` is the live state; these `lua_gc` options only write
    // integers into the collector's parameter block and run no Lua code.
    unsafe {
        ffi::lua_gc(lua.state(), ffi::LUA_GCSETGOAL, goal);
        ffi::lua_gc(lua.state(), ffi::LUA_GCSETSTEPMUL, stepmul);
        ffi::lua_gc(lua.state(), ffi::LUA_GCSETSTEPSIZE, stepsize_kb);
    }
}

/// One measured run of bounded collector steps.
#[derive(Debug)]
pub struct Run {
    pub steps: u64,
    pub cycles: u64,
    pub samples_ns: Vec<u64>,
    /// Largest single call in each completed cycle.
    pub cycle_max_ns: Vec<u64>,
    /// Calls taken by each completed cycle.
    pub memory_before: usize,
    pub memory_after: usize,
}

impl Run {
    #[must_use]
    pub fn p50(&self) -> u64 {
        self.quantile(50)
    }

    #[must_use]
    pub fn p99(&self) -> u64 {
        self.quantile(99)
    }

    #[must_use]
    pub fn max(&self) -> u64 {
        self.samples_ns.iter().copied().max().unwrap_or(0)
    }

    /// The `n` slowest calls as `(step index, ns)`, slowest first.
    #[must_use]
    pub fn slowest(&self, n: usize) -> Vec<(usize, u64)> {
        let mut ranked: Vec<(usize, u64)> = self.samples_ns.iter().copied().enumerate().collect();
        ranked.sort_unstable_by_key(|a| std::cmp::Reverse(a.1));
        ranked.truncate(n);
        ranked
    }

    /// Bytes reclaimed during the run.
    #[must_use]
    pub fn reclaimed(&self) -> i64 {
        self.memory_before as i64 - self.memory_after as i64
    }

    fn quantile(&self, per_mille: usize) -> u64 {
        if self.samples_ns.is_empty() {
            return 0;
        }
        let mut sorted = self.samples_ns.clone();
        sorted.sort_unstable();
        let index = (sorted.len() * per_mille / 1000).min(sorted.len() - 1);
        sorted[index]
    }
}

/// Steps the collector with a `kbytes` budget until `cycles` cycles finish.
///
/// `kbytes == 0` performs exactly one internal step per call, the smallest
/// slice Luau offers; a larger value lets one call run that many KiB of work.
/// Stops early at `max_steps` so a huge heap cannot hang the spike.
#[must_use]
pub fn step_until_cycles(
    state: *mut ffi::lua_State,
    kbytes: i32,
    cycles: u64,
    max_steps: u64,
) -> Run {
    let memory_before = host::used_bytes(state);
    let mut samples_ns = Vec::new();
    let mut cycle_max_ns = Vec::new();
    let mut steps = 0;
    let mut done = 0;
    let mut current_max = 0;
    while done < cycles && steps < max_steps {
        let begin = Instant::now();
        // SAFETY: `state` is the live state of the host under measurement.
        // `LUA_GCSTEP` advances the collector by a bounded amount; it does not
        // call into Lua or run userdata destructors.
        let finished = unsafe { ffi::lua_gc(state, ffi::LUA_GCSTEP, kbytes) } != 0;
        let elapsed = begin.elapsed().as_nanos() as u64;
        samples_ns.push(elapsed);
        current_max = current_max.max(elapsed);
        steps += 1;
        if finished {
            done += 1;
            cycle_max_ns.push(current_max);
            current_max = 0;
        }
    }
    Run {
        steps,
        cycles: done,
        samples_ns,
        cycle_max_ns,
        memory_before,
        memory_after: crate::host::used_bytes(state),
    }
}
