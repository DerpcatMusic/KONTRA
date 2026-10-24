//! `find_zone(name)`: the index of the zone whose sample name matches, or -1. Measured on
//! Kontakt 8 (zones "pv_tone" and "pv_other" at indices 0 and 1): "pv_tone" -> 0, "pv_other" -> 1,
//! a missing name and the empty name -> -1. Group names did not match a zone (unmeasured for
//! named groups, so they are not matched here).
use sampler_ksp::{Environment, Limits};

fn volume_after_init(name_expr: &str, names: &[&str]) -> f32 {
    let mut env = Environment::default();
    env.zone_names = names.iter().map(|n| (*n).to_owned()).collect();
    let source = format!(
        "on init\n set_engine_par($ENGINE_PAR_VOLUME, find_zone({name_expr}) + 10, -1, -1, -1)\nend on"
    );
    let initialized = sampler_ksp::initialize(&source, Limits::LIBRARY, &env).unwrap();
    initialized.engine_pars()[0].value as f32
}

#[test]
fn find_zone_returns_the_index_of_the_named_zone() {
    let names = ["pv_tone", "pv_other"];
    assert_eq!(volume_after_init("\"pv_tone\"", &names), 10.0);
    assert_eq!(volume_after_init("\"pv_other\"", &names), 11.0);
}

#[test]
fn find_zone_is_minus_one_for_a_missing_or_empty_name() {
    let names = ["pv_tone", ""];
    assert_eq!(volume_after_init("\"no such zone\"", &names), 9.0);
    assert_eq!(volume_after_init("\"\"", &names), 9.0);
}
