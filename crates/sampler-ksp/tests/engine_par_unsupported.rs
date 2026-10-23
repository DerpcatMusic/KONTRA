//! Engine-parameter calls this engine cannot honour are diagnosed at build time,
//! not dropped silently. Groups are declared so the group-index check runs.
use sampler_ksp::{Environment, Limits};

fn compile_result(source: &str) -> Result<sampler_ksp::Script, sampler_ksp::Error> {
    sampler_ksp::compile_with(
        source,
        48000,
        Limits::LIBRARY,
        &[],
        &Environment {
            groups: vec!["Group 0".into()],
            ..Environment::default()
        },
    )
}

fn warnings(source: &str) -> Vec<String> {
    sampler_ksp::compile_with(
        source,
        48000,
        Limits::LIBRARY,
        &[],
        &Environment {
            groups: vec!["Group 0".into()],
            ..Environment::default()
        },
    )
    .unwrap_or_else(|e| panic!("{source}: {e:?}"))
    .warnings()
    .iter()
    .map(|w| w.message.clone())
    .collect()
}

#[test]
fn catalog_engine_parameter_without_an_id_is_reported_unsupported() {
    // Kontakt has ENGINE_PAR_WT_MOD_AMOUNT; this engine has no id for it yet.
    let w = warnings("on note set_engine_par($ENGINE_PAR_WT_MOD_AMOUNT, 0, -1, -1, -1) end on");
    assert!(
        w.iter()
            .any(|m| m.contains("set_engine_par") && m.contains("not supported")),
        "{w:?}"
    );
    let r = warnings("on init declare @v end on on note @v := get_engine_par($ENGINE_PAR_WT_MOD_AMOUNT, -1, -1, -1) end on");
    assert!(
        r.iter()
            .any(|m| m.contains("get_engine_par") && m.contains("not supported")),
        "{r:?}"
    );
}

#[test]
fn constant_group_index_outside_the_declared_groups_is_reported() {
    let w = warnings("on note set_engine_par($ENGINE_PAR_VOLUME, 0, 3, -1, -1) end on");
    assert!(
        w.iter()
            .any(|m| m.contains("set_engine_par") && m.contains("invalid group index")),
        "{w:?}"
    );
    let below = warnings("on init declare @v end on on note @v := get_engine_par($ENGINE_PAR_VOLUME, -2, -1, -1) end on");
    assert!(
        below.iter().any(|m| m.contains("invalid group index")),
        "{below:?}"
    );
}

#[test]
fn supported_engine_parameter_in_range_is_not_reported() {
    let w = warnings("on note set_engine_par($ENGINE_PAR_VOLUME, 0, 0, -1, -1) end on");
    assert!(
        w.iter()
            .all(|m| !m.contains("not supported") && !m.contains("invalid group")),
        "{w:?}"
    );
}

#[test]
fn display_query_of_a_catalog_parameter_without_an_id_is_reported() {
    let w = warnings("on init declare @t end on on note @t := get_engine_par_disp($ENGINE_PAR_WT_MOD_AMOUNT, -1, -1, -1) end on");
    assert!(w.iter().any(|m| m.contains("get_engine_par_disp") && m.contains("not supported")), "{w:?}");
}

#[test]
fn multi_script_only_engine_parameter_variants_are_compile_errors() {
    for source in [
        "on note set_engine_par_m($ENGINE_PAR_VOLUME, 0, 0, -1, -1) end on",
        "on init declare @v end on on note @v := get_engine_par_m($ENGINE_PAR_VOLUME, 0, -1, -1) end on",
    ] {
        let err = compile_result(source).err().expect("_m is multi-script only");
        assert!(format!("{err}").contains("can only be used in a multi script"), "{err}");
    }
}

#[test]
fn on_init_engine_parameter_access_to_an_unaddressable_name_is_reported() {
    let w = warnings("on init declare @t @t := get_engine_par_disp($ENGINE_PAR_WT_MOD_AMOUNT, -1, -1, -1) end on");
    assert!(w.iter().any(|m| m.contains("get_engine_par_disp") && m.contains("not supported")), "{w:?}");
}

#[test]
fn slot_or_output_display_query_is_reported_as_not_modelled() {
    // Legal in a single script (no multi-script error), but its display path is not modelled.
    let w = warnings("on init declare @t end on on note @t := get_engine_par_disp_m($ENGINE_PAR_VOLUME, -1, -1, 0) end on");
    assert!(w.iter().any(|m| m.contains("get_engine_par_disp_m") && m.contains("not modelled")), "{w:?}");
}

#[test]
fn find_group_of_an_unknown_constant_name_warns_with_the_native_wording() {
    let w = warnings("on note message(find_group(\"Missing\")) end on");
    assert!(
        w.iter().any(|m| m
            == "find_group(): Group \"Missing\" was not found. Index will be invalid!"),
        "{w:?}"
    );
    let known = warnings("on note message(find_group(\"Group 0\")) end on");
    assert!(known.iter().all(|m| !m.contains("was not found")), "{known:?}");
}

#[test]
fn unsupported_set_engine_par_warning_names_the_parameter() {
    // The load report groups these by message, so the name must be in it.
    let lowered = warnings("on note set_engine_par($ENGINE_PAR_WT_MOD_AMOUNT, 0, -1, -1, -1) end on");
    assert!(
        lowered.iter().any(|m| m.contains("set_engine_par") && m.contains("ENGINE_PAR_WT_MOD_AMOUNT") && m.contains("not supported")),
        "{lowered:?}"
    );
    let on_init = warnings("on init set_engine_par($ENGINE_PAR_WT_MOD_AMOUNT, 0, -1, -1, -1) end on on note end on");
    assert!(
        on_init.iter().any(|m| m.contains("set_engine_par") && m.contains("ENGINE_PAR_WT_MOD_AMOUNT") && m.contains("not supported")),
        "{on_init:?}"
    );
}
