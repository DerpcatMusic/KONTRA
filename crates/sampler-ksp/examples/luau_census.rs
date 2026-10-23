//! KSP -> Luau census and `on init` equivalence over a directory of `.ksp` files.
//! Usage: cargo run -p sampler-ksp --example luau_census -- <dir>
//! For each script: front end (parse, resolve, interpreter `on init`), Luau subset
//! coverage (unsupported constructs counted by name), and cell/text/engine equality.
use sampler_ksp::luau::{check_init, lower};
use std::collections::BTreeMap;

/// Constructs owned by the UI/persistence lane (lane A); the rest are the language/builtin lane.
fn ui_lane(name: &str) -> bool {
    const PREFIXES: [&str; 14] = [
        "builtin set_control_par", "builtin get_control_par", "builtin make_persistent", "builtin read_persistent_var",
        "builtin get_ui_id", "builtin set_ui_", "builtin move_control", "builtin set_menu", "builtin add_menu",
        "builtin add_text_line", "builtin set_text", "builtin set_script_title", "builtin make_perfview", "builtin set_control_help",
    ];
    PREFIXES.iter().any(|p| name.starts_with(p))
        || ["ui control", "callback UiControl", "callback UiUpdate", "callback UiControls", "callback PersistenceChanged"]
            .iter()
            .any(|p| name.starts_with(p))
}

fn main() {
    let dir = std::env::args().nth(1).expect("usage: luau_census <dir>");
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .expect("corpus dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "ksp"))
        .collect();
    paths.sort();
    let (mut frontend_err, mut outside, mut compared, mut matched) = (0usize, 0usize, 0usize, 0usize);
    let mut front_reasons: BTreeMap<String, usize> = BTreeMap::new();
    let mut unsupported: BTreeMap<String, usize> = BTreeMap::new();
    let mut mismatches = Vec::new();
    // Scripts per construct, and scripts blocked by that construct alone.
    let mut scripts_with: BTreeMap<String, usize> = BTreeMap::new();
    let mut lane_b_blocked: BTreeMap<String, usize> = BTreeMap::new();
    let mut lane_b_only = 0usize;
    let mut sole_blocker: BTreeMap<String, usize> = BTreeMap::new();
    for path in &paths {
        let source = String::from_utf8_lossy(&std::fs::read(path).unwrap()).into_owned();
        let lowered = match lower(&source) {
            Ok(l) => l,
            Err(e) => {
                frontend_err += 1;
                let reason: String = e.split(',').next().unwrap_or("").chars().take(70).collect();
                *front_reasons.entry(reason).or_default() += 1;
                continue;
            }
        };
        for (name, n) in &lowered.compiled.unsupported {
            *unsupported.entry(name.clone()).or_default() += n;
            *scripts_with.entry(name.clone()).or_default() += 1;
        }
        let mut b_only = true;
        for name in lowered.compiled.unsupported.keys() {
            if ui_lane(name) {
                continue;
            }
            b_only = false;
            *lane_b_blocked.entry(name.clone()).or_default() += 1;
        }
        if b_only {
            lane_b_only += 1;
        }
        if lowered.compiled.unsupported.len() == 1 {
            let name = lowered.compiled.unsupported.keys().next().unwrap();
            *sole_blocker.entry(name.clone()).or_default() += 1;
        }
        if !lowered.compiled.is_complete() {
            outside += 1;
            continue;
        }
        compared += 1;
        match check_init(&lowered) {
            Ok(_) => matched += 1,
            Err(e) => mismatches.push(format!("{}: {e}", path.file_name().unwrap().to_string_lossy())),
        }
    }
    println!("scripts: {}", paths.len());
    println!("front end errors (interpreter on-init fault or parse/resolve): {frontend_err}");
    for (reason, n) in front_reasons.iter().rev().take(8) {
        println!("  {n:5}  {reason}");
    }
    println!("compiled, outside the Luau subset: {outside}");
    println!("in subset, init compared: {compared}; matched {matched}; mismatched {}", mismatches.len());
    for m in mismatches.iter().take(12) {
        println!("  MISMATCH {m}");
    }
    let mut top: Vec<_> = unsupported.into_iter().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1));
    println!("unsupported constructs (sites, all scripts):");
    for (name, n) in top.iter().take(40) {
        println!("  {n:6}  {name}");
    }
    println!("scripts blocked only by non-UI (lane B) constructs: {lane_b_only}");
    let mut lb: Vec<_> = lane_b_blocked.into_iter().collect();
    lb.sort_by(|a, b| b.1.cmp(&a.1));
    println!("non-UI blockers (scripts with that construct, any):");
    for (name, n) in lb.iter().take(60) {
        println!("  {n:5}  {name}");
    }
    let mut by_scripts: Vec<_> = scripts_with.into_iter().collect();
    by_scripts.sort_by(|a, b| b.1.cmp(&a.1));
    println!("scripts blocked by construct (any; sole blocker):");
    for (name, n) in by_scripts.iter().take(40) {
        let sole = sole_blocker.get(name).copied().unwrap_or(0);
        println!("  {n:5}  sole {sole:4}  {name}");
    }
}
