//! One-program metadata probe. Never prints or saves library source.
use sampler_uvi::{
    Bank,
    script::{Config, ScriptHost},
};
use std::{collections::BTreeMap, path::Path};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).is_some_and(|a| a == "--ledger") {
        print!("{}", sampler_uvi::parity::ledger::markdown());
        return Ok(());
    }
    let bank = Bank::open(Path::new(args.get(1).ok_or("bank path required")?))?;
    let program = args.get(2).ok_or("program path required")?;
    let (xml, _) = bank.program(program)?;
    let files = bank.scripts();
    let reports = sampler_uvi::parity::parity_reports_with_files(&xml, &files);
    let mut statuses = BTreeMap::new();
    let mut apis = BTreeMap::new();
    let mut callbacks = std::collections::BTreeSet::new();
    for c in reports.iter().flat_map(|r| &r.constructs) {
        *statuses.entry(format!("{:?}", c.status)).or_insert(0usize) += 1;
        if c.kind == sampler_parity::ConstructKind::LuaApi {
            *apis.entry(c.name.clone()).or_insert(0usize) += 1;
        }
        if c.kind == sampler_parity::ConstructKind::Callback {
            callbacks.insert(c.name.clone());
        }
    }
    let mut host = ScriptHost::new_inspected(&xml, files, Config::default())
        .map_err(|_| "inspected host failed (source omitted)")?;
    host.note_on(1, 60, 100, 0);
    host.advance(100.0);
    host.note_off(1, 60, 64, 0);
    host.advance(500.0);
    let coverage = host.coverage();
    println!(
        "{}",
        serde_json::json!({
            "program":program,"scripts":reports.len(),
            "tokens":reports.iter().map(|r| r.tokens.len()).sum::<usize>(),
            "constructs":reports.iter().map(|r| r.constructs.len()).sum::<usize>(),
            "lowered":reports.iter().map(|r| r.lowered.len()).sum::<usize>(),
            "statuses":statuses,"api_uses":apis,"callbacks":callbacks,"widgets":host.interface().widgets.len(),
            "covered_scripts":coverage.len(),"hit_lines":coverage.iter().map(|c| c.hits.len()).sum::<usize>(),
            "faults":coverage.iter().map(|c| c.faults.len()).sum::<usize>()
        })
    );
    Ok(())
}
