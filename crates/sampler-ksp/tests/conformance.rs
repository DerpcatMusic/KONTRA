//! KSP conformance against real Kontakt 8 (golden vectors from tools/ksp-conformance).
//!
//! Each unit is a small init-time script whose integer results Kontakt reports as 32-bit
//! groups (set_engine_par(ENGINE_PAR_VOLUME, 1000, group, -1, -1) per set bit). The
//! interpreter runs the same source and the test decodes the same bits from its engine
//! writes. Units listed in `KNOWN_MISMATCHES` are tracked debt (docs/re/KSP_CONFORMANCE.md);
//! any other mismatch fails the test.
use serde_json::Value;
use std::path::PathBuf;

/// Units whose interpreter result differs from Kontakt today. Add an entry only as tracked
/// debt, and remove it in the change that fixes it.
///
/// `min`/`max` with two arguments: Kontakt rejects every on-init form measured (`builtin_*`),
/// the interpreter accepts them. No corpus library uses them (survey: no symbol occurrences).
/// Open: measure them in `on note` and then reject or keep.
const KNOWN_MISMATCHES: &[&str] = &[
    "builtin_minmax__0",
    "builtin_minmax__1",
    "builtin_min_lit",
    "builtin_max_lit",
    "builtin_min_var",
    "builtin_max_neg",
    // String `=` in a condition: Kontakt rejects it in on init (measured); the interpreter
    // accepts it. Open: measure string comparison in on note before rejecting it.
    "str_lit_cond",
    "string_case_sensitive",
    // get_num_menu_items in on init: Kontakt rejects it (diag_menu_count, diag_menu_count_after_add);
    // the interpreter and about ten existing tests use it there. Open: move those tests to
    // on note / on ui_control, then reject the call in on init.
    "ctl_menu_items",
    "diag_menu_count",
    "diag_menu_count_after_add",
];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tools/ksp-conformance")
}

/// Marker groups written by tools/ksp-conformance/gen.py: START first in on init, END last.
const END: i32 = 65;

/// Compile one unit and decode its result bits from the volume engine writes.
/// `Err` means the interpreter rejects the script.
fn interpreter_run(source: &str, results: usize) -> Result<(bool, Vec<i32>), String> {
    let pars = sampler_ksp::init_engine_pars(
        source,
        sampler_ksp::Limits::LIBRARY,
        &sampler_ksp::Environment::default(),
    )
    .map_err(|e| format!("compile error: {e}"))?;
    let volume = |par: &&sampler_ksp::EnginePar| par.parameter.ends_with("VOLUME") && par.value == 1000;
    let completed = pars.iter().filter(volume).any(|p| p.group == END);
    let values = (0..results)
        .map(|k| {
            let mut bits: u32 = 0;
            for par in pars.iter().filter(volume) {
                let group = par.group - 32 * k as i32;
                if (0..32).contains(&group) {
                    bits |= 1 << group;
                }
            }
            bits as i32
        })
        .collect();
    Ok((completed, values))
}

#[test]
fn kontakt_golden_results_match_the_interpreter() {
    let golden: Value = serde_json::from_str(
        &std::fs::read_to_string(root().join("golden.json")).expect("golden.json"),
    )
    .expect("golden.json parses");
    let manifest: Value = serde_json::from_str(
        &std::fs::read_to_string(root().join("manifest.json")).expect("manifest.json"),
    )
    .expect("manifest.json parses");

    let mut failures = Vec::new();
    for unit in manifest.as_array().expect("manifest is an array") {
        let id = unit["id"].as_str().expect("unit id");
        let count = unit["results"].as_u64().expect("result count") as usize;
        let want = &golden[id];
        let compiled = want["compiled"].as_bool().expect("compiled");
        let source = std::fs::read_to_string(root().join("cases").join(format!("{id}.ksp")))
            .expect("case source");
        match (compiled, interpreter_run(&source, count)) {
            (false, Ok(_)) => failures.push(format!("{id}: kontakt rejects the script, interpreter compiles it")),
            (false, Err(_)) => {}
            (true, Err(error)) => failures.push(format!("{id}: kontakt compiles it, {error}")),
            (true, Ok((completed, actual))) => {
                let expected_done = want["completed"].as_bool().expect("completed");
                if completed != expected_done {
                    failures.push(format!("{id}: init completion kontakt {expected_done} interpreter {completed}"));
                } else if expected_done {
                    let expected: Vec<i32> = want["results"]
                        .as_array()
                        .expect("results")
                        .iter()
                        .map(|v| v.as_i64().expect("golden int") as i32)
                        .collect();
                    if actual != expected {
                        failures.push(format!("{id}: kontakt {expected:?} interpreter {actual:?}"));
                    }
                }
            }
        }
    }

    let unexpected: Vec<_> = failures
        .iter()
        .filter(|f| !KNOWN_MISMATCHES.iter().any(|k| f.starts_with(&format!("{k}:"))))
        .collect();
    assert!(
        unexpected.is_empty(),
        "KSP conformance regressions:\n{}",
        unexpected.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n")
    );
}
