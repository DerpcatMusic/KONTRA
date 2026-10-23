//! Times UVI script initialization (ScriptHost::new, no sample files) per
//! program: best of N. Usage: script_init_bench BANK.ufs RUNS [PROGRAM_SUBSTR...]
//! Cache on/off is controlled by KONTRA_SCRIPT_CACHE (see sampler_script::cache_dir).
use std::{path::Path, time::Instant};

use sampler_uvi::script::{Config, ScriptHost};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let runs: usize = args[2].parse()?;
    let bank = sampler_uvi::Bank::open(Path::new(&args[1]))?;
    if std::env::var_os("MODULES").is_some() {
        module_report(&bank, 8, runs)?;
        return Ok(());
    }
    let filters = &args[3..];
    for name in bank.programs() {
        if !filters.is_empty() && !filters.iter().any(|f| name.contains(f.as_str())) {
            continue;
        }
        let (xml, _) = bank.program(&name)?;
        let mut best = f64::MAX;
        for _ in 0..runs {
            let start = Instant::now();
            let host = ScriptHost::new(&xml, (), Config::default())?;
            best = best.min(start.elapsed().as_secs_f64() * 1e3);
        }
        let scripts = xml.matches("<script").count();
        let script_bytes: usize = xml.split("<script>").skip(1).map(|t| t.find("</script>").unwrap_or(0)).sum();
        println!("{best:9.3} ms  script_tags={scripts} script_bytes={script_bytes} xml_bytes={} {name}", xml.len());
    }
    Ok(())
}

/// Bank-wide embedded Lua modules, largest first: compile vs bytecode load.
#[allow(dead_code)]
pub fn module_report(bank: &sampler_uvi::Bank, top: usize, runs: usize) -> mlua::Result<()> {
    let mut modules = bank.scripts().sources().as_ref().clone();
    modules.sort_by_key(|(_, src)| std::cmp::Reverse(src.len()));
    println!("modules={} total_bytes={}", modules.len(), modules.iter().map(|(_, s)| s.len()).sum::<usize>());
    for (path, src) in modules.iter().take(top) {
        let best_compile = (0..runs).map(|_| {
            let t = Instant::now();
            sampler_script::cached_bytecode(None, src.as_bytes()).map(|_| t.elapsed().as_secs_f64() * 1e3)
        }).collect::<mlua::Result<Vec<_>>>()?.into_iter().fold(f64::MAX, f64::min);
        let bytes = sampler_script::cached_bytecode(None, src.as_bytes())?;
        // Fresh VM per load: one VM per program is the real shape.
        let best_load = (0..runs).map(|_| {
            let lua = sampler_script::new_vm(64 << 20)?;
            if std::env::var_os("JIT").is_some() { lua.enable_jit(true); } else { lua.enable_jit(false); }
            let t = Instant::now();
            lua.load(&bytes[..]).into_function().map(|_| t.elapsed().as_secs_f64() * 1e3)
        }).collect::<mlua::Result<Vec<_>>>()?.into_iter().fold(f64::MAX, f64::min);
        println!("compile {best_compile:8.3} ms  bytecode-load {best_load:8.3} ms  src_bytes={} {}", src.len(), path.rsplit('/').next().unwrap_or(""));
    }
    Ok(())
}
