//! Kontakt 8 golden for the persistence load order: tools/ksp-conformance/persistence_golden.json
//! (probe persistence_roundtrip.lua; decoded by persistence_decode.py). Each case loads the saved
//! values Kontakt wrote into its own .nki, runs the script's on init, and must match the points
//! Kontakt read back. `on persistence_changed` is not compared: the Lua load path did not run it.
use sampler_ksp::{Environment, Limits, model::Value};
use std::collections::BTreeMap;

const GOLDEN: &str = include_str!("../../../tools/ksp-conformance/persistence_golden.json");

/// Saved values Kontakt wrote into each probe .nki (`show_script ... state`), by variable name.
fn saved(case: &str) -> (BTreeMap<String, Value>, BTreeMap<String, Vec<Value>>) {
    let mut scalars: BTreeMap<String, Value> = [("$s".to_owned(), Value::Int(1))].into();
    let mut arrays = BTreeMap::new();
    match case {
        "int_pers_readvar" | "int_pers_noread" => {
            scalars.insert("$p".into(), Value::Int(7));
        }
        "real_pers" => {
            scalars.insert("~r".into(), Value::Real(3.7));
        }
        "array_pers" => {
            arrays.insert("%a".to_owned(), vec![Value::Int(0), Value::Int(5), Value::Int(0)]);
        }
        "untaken_mp" => {
            scalars.insert("$z".into(), Value::Int(4));
        }
        "instr_pers" => {
            scalars.insert("$ii".into(), Value::Int(6));
        }
        _ => {}
    }
    (scalars, arrays)
}

/// The bit value of each encoded point (base group), from the init writes of the interpreter.
fn points(source: &str, case: &str) -> BTreeMap<&'static str, u32> {
    let (persisted, persisted_arrays) = saved(case);
    let env = Environment {
        persisted,
        persisted_arrays,
        ..Default::default()
    };
    let pars = sampler_ksp::init_engine_pars(source, Limits::LIBRARY, &env)
        .unwrap_or_else(|e| panic!("{case}: {e}"));
    let set = |group: i32| {
        pars.iter()
            .any(|p| p.parameter.ends_with("VOLUME") && p.value == 1000 && p.group == group)
    };
    [("A", 0), ("B", 8), ("C", 16), ("S", 32)]
        .into_iter()
        .map(|(name, base)| {
            let bits = (0..8).filter(|b| set(base + b)).fold(0u32, |acc, b| acc | 1 << b);
            (name, bits)
        })
        .collect()
}

#[test]
fn init_matches_kontakt_points_for_saved_persistent_values() {
    let golden: serde_json::Value = serde_json::from_str(GOLDEN).unwrap();
    let cases = golden.as_object().unwrap();
    assert_eq!(cases.len(), 7);
    for (case, row) in cases {
        assert!(row["compiled"].as_bool().unwrap(), "{case}: Kontakt rejected the script");
        assert!(row["completed"].as_bool().unwrap(), "{case}: Kontakt init did not finish");
        let source = row["source"].as_str().unwrap();
        let got = points(source, case);
        for name in ["A", "B", "C", "S"] {
            let want = row["points"][name].as_u64().unwrap() as u32;
            assert_eq!(got[name], want, "{case}: point {name} (Kontakt {want}, interpreter {})", got[name]);
        }
    }
}
