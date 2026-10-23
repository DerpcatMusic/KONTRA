//! A synchronous `set_engine_par` reports no pending async operation (-1), so a
//! `while (id > -1) wait` guard around it exits without waiting.
use sampler_ksp::Environment;

#[test]
fn synchronous_engine_write_ends_its_wait_guard_and_the_rest_of_the_callback_runs() {
    // The Conflux 'Dark Context' shape: a filter-type change guarded by its async id.
    // A reported id of 0 made the guard wait forever, so every write after it was cut.
    let source = "on init declare $x end on
        on persistence_changed
        $x := set_engine_par($ENGINE_PAR_VOLUME, 250000, 0, -1, -1)
        while ($x > -1) wait(1000) end while
        set_engine_par($ENGINE_PAR_PAN, 750000, 0, -1, -1)
        end on";
    let initialized = sampler_ksp::initialize(source, sampler_ksp::Limits::LIBRARY, &Environment::default())
        .expect("the synthetic callback compiles");
    let parameters: Vec<_> = initialized
        .engine_pars()
        .into_iter()
        .map(|w| (w.parameter.clone(), w.value))
        .collect();
    assert!(
        parameters.iter().any(|(p, v)| p.ends_with("ENGINE_PAR_VOLUME") && *v == 250_000),
        "the guarded write runs: {parameters:?}"
    );
    assert!(
        parameters.iter().any(|(p, v)| p.ends_with("ENGINE_PAR_PAN") && *v == 750_000),
        "the write after the guard runs: {parameters:?}"
    );
}
