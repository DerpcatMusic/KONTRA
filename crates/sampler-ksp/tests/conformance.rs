//! KSP conformance against real Kontakt 8 (golden vectors from tools/ksp-conformance).
//!
//! Each unit is a small init-time script whose integer results Kontakt reports as 32-bit
//! groups (set_engine_par(ENGINE_PAR_VOLUME, 1000, group, -1, -1) per set bit). The
//! interpreter runs the same source and the test decodes the same bits from its engine
//! writes. Units listed in `KNOWN_MISMATCHES` are tracked debt (docs/re/KSP_CONFORMANCE.md);
//! any other mismatch fails the test.
use serde_json::Value;
use std::path::PathBuf;

/// Units whose interpreter result differs from Kontakt today. Remove an entry in the
/// same change that fixes it. Categories and the Kontakt rule behind each are in
/// docs/re/KSP_CONFORMANCE.md.
const KNOWN_MISMATCHES: &[&str] = &[
    // typing: Kontakt accepts a real assigned to an integer and yields 0
    "type_real_to_int_assign",
    // logic: and/or/not are condition-only in Kontakt; bare integers are false
    "logic_not",
    "logic_and_or",
    "logic_vars",
    "logic_vars_2",
    // function: `call` from on init does not change the caller's global (Kontakt reads 0)
    "function_updates_global",
    "function_nested_call",
];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tools/ksp-conformance")
}

/// Decode `results` 32-bit values from the volume engine writes of one unit.
fn interpreter_results(source: &str, results: usize) -> Result<Vec<i32>, String> {
    let pars = sampler_ksp::init_engine_pars(
        source,
        sampler_ksp::Limits::LIBRARY,
        &sampler_ksp::Environment::default(),
    )
    .map_err(|e| format!("compile error: {e}"))?;
    Ok((0..results)
        .map(|k| {
            let mut bits: u32 = 0;
            for par in &pars {
                if !par.parameter.ends_with("VOLUME") || par.value != 1000 {
                    continue;
                }
                let group = par.group - 32 * k as i32;
                if (0..32).contains(&group) {
                    bits |= 1 << group;
                }
            }
            bits as i32
        })
        .collect())
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
        let expected: Vec<i32> = golden[id]["results"]
            .as_array()
            .unwrap_or_else(|| panic!("{id} missing from golden.json"))
            .iter()
            .map(|v| v.as_i64().expect("golden int") as i32)
            .collect();
        let source = std::fs::read_to_string(root().join("cases").join(format!("{id}.ksp")))
            .expect("case source");
        match interpreter_results(&source, expected.len()) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("{id}: kontakt {expected:?} interpreter {actual:?}")),
            Err(error) => failures.push(format!("{id}: kontakt {expected:?} {error}")),
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
