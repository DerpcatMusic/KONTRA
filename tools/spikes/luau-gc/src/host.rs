//! Loading real UVI hosts, attributing their heap, and running the note stream.

use std::path::Path;
use std::time::Instant;

use mlua::{Lua, Table, Value, ffi};
use sampler_uvi::script::{Config, ScriptHost};
use sampler_uvi::Bank;

/// One real bank program running through the sampler's own script host.
pub struct Loaded {
    pub label: String,
    pub host: ScriptHost,
    pub init_ms: f64,
    pub members: usize,
    pub xml_bytes: usize,
}

/// Loads `program` from `bank` and runs the whole bank's script set through
/// `ScriptHost::new_profiled`, the same path P3.1b used.
///
/// # Errors
/// Fails when the bank, the program or the host cannot be opened.
pub fn load(bank: &Path, program: &str) -> Result<Loaded, String> {
    let bank_file = Bank::open(bank).map_err(|e| format!("open {}: {e:?}", bank.display()))?;
    let (xml, _) = bank_file
        .program(program)
        .map_err(|e| format!("program {program}: {e:?}"))?;
    let scripts = bank_file.scripts();
    let members = scripts.occurrences().len();
    let label = format!(
        "{} / {program}",
        bank.file_stem().and_then(|s| s.to_str()).unwrap_or("?")
    );
    let started = Instant::now();
    let host = ScriptHost::new_profiled(&xml, scripts, Config::realtime(), false)?;
    Ok(Loaded {
        label,
        host,
        init_ms: started.elapsed().as_secs_f64() * 1000.0,
        members,
        xml_bytes: xml.len(),
    })
}

/// Bytes the collector holds right now, straight from `LUA_GCCOUNT`.
#[must_use]
pub fn used_bytes(state: *mut ffi::lua_State) -> usize {
    // SAFETY: `state` is the live state of the host this spike is measuring.
    unsafe {
        let kbytes = ffi::lua_gc(state, ffi::LUA_GCCOUNT, 0) as usize;
        let rest = ffi::lua_gc(state, ffi::LUA_GCCOUNTB, 0) as usize;
        kbytes * 1024 + rest
    }
}

/// One teardown step: what became unreachable when `path` was dropped.
#[derive(Debug, Clone)]
pub struct Freed {
    pub path: String,
    pub bytes: i64,
}

impl Freed {
    /// KiB, rounded, for tables.
    #[must_use]
    pub fn kib(&self) -> i64 {
        self.bytes / 1024
    }
}

/// Drops every child of `table` one at a time and records the bytes each drop
/// actually frees, largest first is not possible up front, so the printed order
/// is the teardown order and each number is exact for that point in time.
///
/// The child is anchored in the registry before it is unlinked, so a lasting
/// handle stays valid while the collector runs and the value is only freed when
/// the anchor is removed. `depth` levels of tables are drilled into.
///
/// # Errors
/// Propagates mlua errors from table iteration or registry anchoring.
pub fn teardown(
    lua: &Lua,
    table: &Table,
    path: &str,
    depth: usize,
    out: &mut Vec<Freed>,
) -> Result<(), String> {
    let mut children: Vec<(Value, Value)> = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair.map_err(|e| format!("pairs {path}: {e}"))?;
        children.push((key, value));
    }
    for (key, value) in children {
        let name = render(&key);
        let anchor = lua
            .create_registry_value(value.clone())
            .map_err(|e| format!("anchor {path}.{name}: {e}"))?;
        table
            .raw_set(key.clone(), Value::Nil)
            .map_err(|e| format!("drop {path}.{name}: {e}"))?;
        let before = used_bytes(lua.state());
        lua.gc_collect().map_err(|e| format!("collect: {e}"))?;
        let after = used_bytes(lua.state());
        out.push(Freed {
            path: format!("{path}.{name}"),
            bytes: before as i64 - after as i64,
        });
        if depth > 0
            && let Value::Table(child) = &value
        {
            teardown(lua, child, &format!("{path}.{name}"), depth - 1, out)?;
        }
        lua.remove_registry_value(anchor)
            .map_err(|e| format!("release anchor {path}.{name}: {e}"))?;
        lua.gc_collect().map_err(|e| format!("collect: {e}"))?;
    }
    Ok(())
}

/// A table key as it appears in a teardown path.
fn render(key: &Value) -> String {
    match key {
        Value::String(s) => s.to_string_lossy(),
        Value::Integer(i) => i.to_string(),
        Value::Number(n) => n.to_string(),
        other => format!("{other:?}"),
    }
}

/// How a block run drives the collector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Collector {
    /// No `lua_gc` call at all; `LUA_GCSTOP` is in force.
    Stopped,
    /// One bounded `lua_gc(LUA_GCSTEP)` call per block with this KiB budget.
    Step(i32),
}

impl Collector {
    fn label(self) -> String {
        match self {
            Self::Stopped => "stopped".to_owned(),
            Self::Step(kb) => format!("step({kb} KiB)"),
        }
    }
}

/// Result of one simulated block stream.
#[derive(Debug)]
pub struct Blocks {
    pub collector: String,
    pub blocks: u64,
    pub notes: u64,
    pub gc_max_ns: u64,
    pub gc_p99_ns: u64,
    pub script_max_ns: u64,
    pub script_p99_ns: u64,
    pub block_max_ns: u64,
    pub block_p99_ns: u64,
    pub cycles: u64,
    pub memory_start: usize,
    pub memory_end: usize,
}

impl Blocks {
    /// MiB the collector gained or lost over the run.
    #[must_use]
    pub fn memory_delta_kib(&self) -> i64 {
        (self.memory_end as i64 - self.memory_start as i64) / 1024
    }
}

/// Simulates `blocks` audio blocks: one note event every `note_every` blocks and
/// one collector action per block. Timings are per block, so `block_max_ns` is
/// the total stall a block would see.
///
/// # Panics
/// Panics when sorting an empty sample set; `blocks` must be positive.
#[must_use]
pub fn run_blocks(
    host: &mut ScriptHost,
    state: *mut ffi::lua_State,
    collector: Collector,
    blocks: u64,
    block_ms: f64,
    note_every: u64,
) -> Blocks {
    let memory_start = used_bytes(state);
    let mut gc_ns = Vec::new();
    let mut script_ns = Vec::new();
    let mut block_ns = Vec::new();
    let mut notes = 0;
    let mut cycles = 0;
    host.set_time(0.0);
    for block in 0..blocks {
        let now = block as f64 * block_ms;
        let block_begin = Instant::now();
        let mut script = 0;
        if note_every != 0 && block.is_multiple_of(note_every) {
            let key = 60 + (notes % 12) as u8;
            host.note_on(notes + 1, key, 100, 0);
            host.advance(now);
            let _ = host.take_commands();
            script = block_begin.elapsed().as_nanos() as u64;
            notes += 1;
        }
        let mut gc = 0;
        if let Collector::Step(kb) = collector {
            let gc_begin = Instant::now();
            // SAFETY: `state` is the host's live state and the host is not
            // executing Lua while this call runs.
            let finished = unsafe { ffi::lua_gc(state, ffi::LUA_GCSTEP, kb) } != 0;
            gc = gc_begin.elapsed().as_nanos() as u64;
            cycles += u64::from(finished);
        }
        gc_ns.push(gc);
        script_ns.push(script);
        block_ns.push(block_begin.elapsed().as_nanos() as u64);
    }
    Blocks {
        collector: collector.label(),
        blocks,
        notes,
        gc_max_ns: max(&gc_ns),
        gc_p99_ns: quantile(&gc_ns, 99),
        script_max_ns: max(&script_ns),
        script_p99_ns: quantile(&script_ns, 99),
        block_max_ns: max(&block_ns),
        block_p99_ns: quantile(&block_ns, 99),
        cycles,
        memory_start,
        memory_end: used_bytes(state),
    }
}

fn max(samples: &[u64]) -> u64 {
    samples.iter().copied().max().unwrap_or(0)
}

fn quantile(samples: &[u64], per_mille: usize) -> u64 {
    if samples.is_empty() {
        return 0;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    sorted[(sorted.len() * per_mille / 1000).min(sorted.len() - 1)]
}
