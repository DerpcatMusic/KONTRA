//! Spike P3.1b, real scripts: runs each UVI program through the sampler's own script host
//! (`sampler_uvi::script::ScriptHost`, which runs the bank's scripts through its mlua state
//! and `onInit`) on this thread, inside an arena scope, and reports the heap and allocator
//! numbers.
//!
//! The host's Lua state is private, so this binary cannot step its collector. GC step cost on
//! real scripts is therefore not measured here; see docs/architecture-v3/spikes/luau-alloc-mlua.md.
//!
//! Usage: `uvi_host <bank.ufs> <program> [<bank.ufs> <program> ...]`

use std::path::{Path, PathBuf};
use std::time::Instant;

use luau_alloc_mlua::routed::{self, ArenaScope};
use sampler_uvi::Bank;
use sampler_uvi::script::{Config, ScriptHost};

/// Arena for one host. Reserved once and paged in on first touch. The peak is read after a
/// reset, so each unit reports its own.
const ARENA_BYTES: usize = 2 << 30;

/// Finds the largest `*memory_bytes` value (the host reports `lua_memory_bytes`) anywhere in the
/// host's initialization profile.
fn heap_bytes(profile: &serde_json::Value) -> Option<f64> {
    if let Some(obj) = profile.as_object() {
        let own = obj
            .iter()
            .filter(|(k, _)| k.ends_with("memory_bytes"))
            .filter_map(|(_, v)| v.as_f64())
            .fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v))));
        let nested = obj
            .values()
            .filter_map(heap_bytes)
            .fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v))));
        return match (own, nested) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
    }
    if let Some(arr) = profile.as_array() {
        return arr
            .iter()
            .filter_map(heap_bytes)
            .fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v))));
    }
    None
}

fn run_unit(bank_path: &Path, program: &str) -> Result<String, String> {
    let bank = Bank::open(bank_path).map_err(|e| format!("open bank: {e:?}"))?;
    let (xml, _) = bank
        .program(program)
        .map_err(|e| format!("program: {e:?}"))?;
    let scripts = bank.scripts();
    let lua_members = scripts.occurrences().len();
    let label = format!(
        "{} / {}",
        bank_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?"),
        program
    );

    routed::reset_peak();
    let before = routed::stats();
    let started = Instant::now();
    let host = {
        let _scope = ArenaScope::enter();
        ScriptHost::new_profiled(&xml, scripts, Config::realtime(), false)
    };
    let init_ms = started.elapsed().as_secs_f64() * 1000.0;
    let peak_kb = routed::arena_stats().peak_live as f64 / 1024.0;
    let delta = routed::stats().since(before);

    let (status, heap) = match &host {
        Ok(h) => {
            let profile = h.initialization_profile();
            if std::env::var_os("UVI_HOST_PROFILE").is_some() {
                eprintln!("{label}: {profile}");
            }
            ("ok".to_owned(), heap_bytes(&profile))
        }
        Err(e) => (
            e.replace(['\n', '|'], " ").chars().take(160).collect(),
            None,
        ),
    };
    drop(host);
    let (vsize, valign) = routed::last_violation();
    Ok(format!(
        "| {label} | {lua_members} | {status} | {init_ms:.1} | {} | {peak_kb:.0} | {} | {} | {}/{} |",
        heap.map_or_else(|| "-".to_owned(), |h| format!("{:.0}", h / 1024.0)),
        delta.arena,
        delta.system_in_scope,
        vsize,
        valign,
    ))
}

fn main() {
    routed::reserve(ARENA_BYTES).expect("reserve arena");
    let args: Vec<String> = std::env::args().skip(1).collect();
    assert!(
        !args.is_empty() && args.len().is_multiple_of(2),
        "usage: uvi_host <bank.ufs> <program> ..."
    );
    println!(
        "| script | lua members | host init | init ms | Lua heap after init KiB | arena peak KiB | arena allocs | system-in-scope allocs | last system-in-scope size/align |"
    );
    println!("|---|---:|---|---:|---:|---:|---:|---:|---:|");
    for pair in args.chunks(2) {
        match run_unit(&PathBuf::from(&pair[0]), &pair[1]) {
            Ok(row) => println!("{row}"),
            Err(e) => println!(
                "| {} / {} | - | {e} | - | - | - | - | - | - |",
                pair[0], pair[1]
            ),
        }
    }
    println!(
        "\narena {} MiB: reserved {} B, spills {}",
        ARENA_BYTES >> 20,
        routed::arena_stats().reserved,
        routed::arena_stats().spills
    );
}
