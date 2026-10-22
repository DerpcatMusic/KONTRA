//! `on persistence_changed` that waits is scheduled for the runtime, but its
//! `set_engine_par` writes are load-time engine state (Kontakt applies them at load).
#[test]
fn scheduled_persistence_engine_writes_reach_init_writes() {
    let source = r#"
on init
end on
on persistence_changed
    wait(1)
    set_engine_par($ENGINE_PAR_GAIN1, 500000, 7, 5, -1)
end on
"#;
    let writes = sampler_ksp::init_engine_pars(
        source,
        sampler_ksp::Limits::LIBRARY,
        &sampler_ksp::Environment::default(),
    )
    .unwrap();
    let write = writes
        .iter()
        .find(|w| w.parameter == "$ENGINE_PAR_GAIN1")
        .expect("the scheduled persistence write must be visible to the loader");
    assert_eq!(
        (write.value, write.group, write.slot, write.generic),
        (500_000, 7, 5, -1)
    );
}
