//! Opt-in owned-bank check; no bank data or access material is written.
#[cfg(feature = "library-access")]
#[test]
fn installed_protected_program_loads_and_renders() {
    let Some(path) = std::env::var_os("KONTRA_UVI_TEST_BANK")
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_file())
    else {
        eprintln!("skipped: KONTRA_UVI_TEST_BANK is absent");
        return;
    };
    let bank = sampler_uvi::Bank::open(&path).unwrap();
    let program = std::env::var("KONTRA_UVI_TEST_PROGRAM")
        .ok()
        .or_else(|| bank.programs().into_iter().next())
        .unwrap();
    let options = sampler_kontakt::Options {
        keys: 60..=60,
        ..Default::default()
    };
    let loaded = sampler_uvi::load_program_with_options(&bank, &program, &options).unwrap();
    assert!(!loaded.instrument.zones.is_empty());
    assert!(
        loaded
            .instrument
            .zones
            .iter()
            .all(|z| z.keys.low <= 60 && z.keys.high >= 60)
    );
    let limits = sampler_core::Limits {
        notes: 4,
        channels: 16,
        performances: 1,
        expressions: 4,
        families: 4,
        decisions: 256,
        voices: 512,
        commands: 256,
        behaviors: 0,
        behavior_fuel: 0,
        behavior_cells: 0,
        note_cells: 0,
    };
    let mut runtime = sampler_core::Runtime::new(loaded.plan, limits).unwrap();
    runtime
        .trigger(
            sampler_core::Input {
                protocol: sampler_core::Protocol::Native,
                port: 0,
                group: 0,
                channel: 0,
                key: 60,
                external_id: None,
            },
            60,
            0.8,
        )
        .unwrap();
    let mut audio = vec![[0.0; 2]; 4800];
    runtime.render(&mut audio).unwrap();
    assert!(audio.iter().flatten().all(|v| v.is_finite()));
    assert!(audio.iter().flatten().any(|v| v.abs() > 0.0001));
}
