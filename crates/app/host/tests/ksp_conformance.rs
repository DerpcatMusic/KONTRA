//! The KSP conformance suite against real Kontakt 8.13.1 (docs/re/KSP_CONFORMANCE.md):
//! every unit in `tools/ksp-conformance/cases` runs its `on init` on a fresh engine with
//! 66 groups, and what it leaves is read back the way the oracle probe reads Kontakt
//! (`gen.py` `lua_batch`): group 64 written means it compiled, group 65 that init ran to
//! the end, and bit `b` of result `k` is group `32k + b` turned down by
//! `set_engine_par($ENGINE_PAR_VOLUME, 1000, ...)`. Each unit must equal `golden.json`.
#![cfg(test)]

use std::path::Path;
use std::sync::Arc;

use kontra_engine::{Engine, Plan, ir};
use kontra_stream::{Store, StoreConfig};

const DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../tools/ksp-conformance"
);
const START: u8 = 64;
const END: u8 = 65;

/// Units KONTRA does not match yet; a fixed one must leave this list.
const KNOWN_MISMATCHES: &[&str] = &[
    // Kontakt rejects `load_performance_view("none")` at compile time: the view
    // must be in the instrument's resource container, which the compiler is
    // not given.
    "place_load_performance_view_init",
    "place_load_performance_view_note",
];

#[derive(Debug, PartialEq)]
struct Outcome {
    compiled: bool,
    completed: bool,
    results: Vec<i64>,
}

fn run(source: &str, results: usize) -> Outcome {
    let Ok(script) = kontra_ksp::compile(source, &[]) else {
        return Outcome {
            compiled: false,
            completed: false,
            results: vec![0; results],
        };
    };
    let store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(Vec::new(), 16);
    plan.groups = vec![kontra_engine::Group::standard(); usize::from(END) + 1];
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    let mut e = Engine::new(Arc::new(plan), store.client(), 48_000);
    e.process(&mut vec![[0.0; 2]; 64], &[]);
    // The probe's test: a group below -100 dB was written (1000 reads -167 dB).
    let set = |g: u8| {
        e.get_param(ir::Scope::Group(g.into()), ir::ParamAddr::Volume)
            .is_some_and(|gain| gain < 1e-5)
    };
    let word = |k: usize| {
        let bits = (0..32u8).fold(0u32, |w, b| w | (u32::from(set(32 * k as u8 + b)) << b));
        i64::from(bits as i32)
    };
    Outcome {
        compiled: set(START),
        completed: set(END),
        results: (0..results).map(word).collect(),
    }
}

#[test]
fn every_unit_matches_kontakt() {
    let golden: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(Path::new(DIR).join("golden.json")).unwrap())
            .unwrap();
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(Path::new(DIR).join("manifest.json")).unwrap(),
    )
    .unwrap();
    let mut wrong = Vec::new();
    let mut checked = 0;
    for unit in manifest.as_array().unwrap() {
        let id = unit["id"].as_str().unwrap();
        let Some(want) = golden.get(id) else {
            continue; // not run on the oracle yet
        };
        let want = Outcome {
            compiled: want["compiled"].as_bool().unwrap(),
            completed: want["completed"].as_bool().unwrap(),
            results: want["results"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_i64().unwrap())
                .collect(),
        };
        let source =
            std::fs::read_to_string(Path::new(DIR).join(format!("cases/{id}.ksp"))).unwrap();
        let mut got = run(&source, want.results.len());
        if !want.compiled {
            // Kontakt drops a rejected script: none of its groups move.
            got.results = vec![0; want.results.len()];
        }
        checked += 1;
        let matches = if want.compiled {
            got == want
        } else {
            !got.compiled
        };
        if matches == KNOWN_MISMATCHES.contains(&id) {
            wrong.push(format!("{id}: Kontakt {want:?}, KONTRA {got:?}"));
        }
    }
    assert!(checked > 100, "only {checked} units have a golden");
    assert!(
        wrong.is_empty(),
        "{} units:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}
