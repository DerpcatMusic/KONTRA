//! Native enum identity is independent of authored menu order.
#[test]
fn wavetable_enums_keep_v1_numeric_identity_in_initializer_writes() {
    let source = r#"
on init
    set_engine_par($ENGINE_PAR_WT_FORM_MODE, $NI_WT_FORM_ASYM2MP, 0, -1, -1)
    set_engine_par($ENGINE_PAR_WT_FORM2_MODE, $NI_WT_FORM_LINEAR, 0, -1, -1)
    set_engine_par($ENGINE_PAR_WT_MOD_TYPE, $NI_WT_MOD_TYPE_PM1, 0, -1, -1)
end on
"#;
    let writes = sampler_ksp::init_engine_pars(source, sampler_ksp::Limits::LIBRARY,
        &sampler_ksp::Environment::default()).unwrap();
    for (parameter, expected) in [("$ENGINE_PAR_WT_FORM_MODE", 16), ("$ENGINE_PAR_WT_FORM2_MODE", 0),
        ("$ENGINE_PAR_WT_MOD_TYPE", 6)] {
        assert_eq!(writes.iter().find(|w| w.parameter == parameter).unwrap().value, expected,
            "{parameter}: a vendor enum must not become an opaque interned identity");
    }
}
