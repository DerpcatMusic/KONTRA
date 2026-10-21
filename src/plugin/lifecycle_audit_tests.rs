//! Reproductions for docs/audit/lifecycle-and-core-20261021.md. Each test
//! states the contract the audit expects; `known bug` tests fail today.
use super::tests::{transaction_candidate, transaction_fixture};
use super::*;

/// F-03: a canceled or failed switch must not make a later request for the
/// still-active source reload it. `attempted` is the only dedup key, so the
/// active source is rebuilt (new generation, voices cut) after A -> B(canceled) -> A.
#[test]
fn returning_to_the_active_source_after_a_canceled_switch_does_not_reload() {
    let (params, mut core) = transaction_fixture();
    let atoms = params.shared.part(0).unwrap();
    let epoch = atoms.generation.load(Ordering::Acquire);
    params.selection.write_unpoisoned().parts[0].path = "candidate.nki".into();
    assert!(!load_part_with(&params, 0, None, |_, _, canceled| {
        // The user clicks back to the preset that is still playing.
        params.selection.write_unpoisoned().parts[0].path = "active.nki".into();
        assert!(canceled());
        Ok(transaction_candidate("Superseded"))
    }));
    assert!(!load_part(&params, 0, Some(transaction_candidate("Reloaded"))));
    assert_eq!(atoms.generation.load(Ordering::Acquire), epoch);
    assert!(params.shared.ready.pop().is_none());
    assert!(core.render(64).live[0], "held audio survives");
}

/// F-03 (failure variant): after a failed switch, selecting the old preset again.
#[test]
fn returning_to_the_active_source_after_a_failed_switch_does_not_reload() {
    let (params, _core) = transaction_fixture();
    let atoms = params.shared.part(0).unwrap();
    let epoch = atoms.generation.load(Ordering::Acquire);
    params.selection.write_unpoisoned().parts[0].path = "/nonexistent/x.nki".into();
    assert!(load_part(&params, 0, None));
    params.selection.write_unpoisoned().parts[0].path = "active.nki".into();
    assert!(!load_part(&params, 0, Some(transaction_candidate("Reloaded"))));
    assert_eq!(atoms.generation.load(Ordering::Acquire), epoch);
}

/// Control: the same-source request is deduplicated while nothing failed.
#[test]
fn an_unchanged_source_is_not_reloaded() {
    let (params, _core) = transaction_fixture();
    assert!(!load_part(&params, 0, Some(transaction_candidate("Again"))));
}

/// F-05: after the host changes the sample rate, parts still hold the old
/// prepared rate until the loader replaces them (`V2Core::reset` keeps them).
/// They keep rendering at the wrong speed for the whole reload.
#[test]
fn a_part_prepared_for_another_rate_is_silent_until_it_is_reloaded() {
    let (_params, mut core) = transaction_fixture();
    core.reset(44100.);
    core.play(0, CoreEvent::midi1(0x90, 60, 100));
    let out = core.render(64);
    assert!(
        !out.buses[0][0][..64].iter().any(|x| x.abs() > 0.0),
        "48 kHz material is being played at 44.1 kHz"
    );
}

/// F-07: a malformed or oversized saved UVI blob loads defaults instead of failing the preset.
#[test]
fn a_bad_saved_state_blob_loads_with_defaults() {
    for blob in ["{not json".to_string(), "x".repeat(MAX_SAVED_STATE + 1)] {
        let (params, _core) = transaction_fixture();
        {
            let mut selection = params.selection.write_unpoisoned();
            let part = &mut selection.parts[0];
            part.path = "candidate.nki".into();
            part.uvi_state_source = serde_json::to_string(&part.source()).unwrap();
            part.uvi_state = blob;
        }
        assert!(load_part_with(&params, 0, None, |request, _, _| {
            assert!(request.uvi_state.is_none());
            Ok(transaction_candidate("Recovered"))
        }));
        let view = params.shared.view.lock_unpoisoned();
        assert!(!view.parts[0].status.starts_with("Load failed"), "{}", view.parts[0].status);
    }
}

fn next_part(params: &SamplerParams, name: &str) -> Box<CorePart> {
    params.selection.write_unpoisoned().parts[0].path = format!("{name}.nki");
    assert!(load_part(params, 0, Some(transaction_candidate(name))));
    params.shared.ready.pop().unwrap().2.unwrap()
}

/// F-08: the replaced part fades out instead of being cut, then goes back to be dropped.
#[test]
fn a_replaced_part_fades_out_and_is_then_handed_back() {
    let (params, mut core) = transaction_fixture();
    let next = next_part(&params, "next");
    assert!(core.install(0, Some(next)).0.is_none(), "kept as a tail, not dropped on the audio thread");
    let first = core.render(64);
    assert!(first.live[0]);
    let head = first.buses[0][0][..64].iter().fold(0f32, |m, x| m.max(x.abs()));
    assert!(head > 0., "the old note is still heard");
    assert!(core.take_expired().is_none());
    for _ in 0..(48000 / 30 / 64 + 2) {
        core.render(64);
    }
    assert!(core.take_expired().is_some());
    assert!(core.take_expired().is_none());
}

/// F-02: a mute or fader move made while a preset prepares reaches the audio
/// thread's mix queue on the wait tick, not after the load.
#[test]
fn a_mixer_edit_during_a_load_reaches_the_audio_queue_on_the_wait_tick() {
    let (params, _core) = transaction_fixture();
    while params.shared.controls.pop().is_some() {}
    params.selection.write_unpoisoned().parts[0].path = "candidate.nki".into();
    assert!(load_part_with(&params, 0, None, |_, _, _| {
        // The loader is "blocked" here; the user drags the fader and the wait loop ticks.
        control_tick(&params);
        let before = params.shared.controls.pop().expect("mix pushed during the wait").parts[0].gain;
        params.selection.write_unpoisoned().parts[0].gain = 0.25;
        control_tick(&params);
        let after = params.shared.controls.pop().expect("mix pushed during the wait").parts[0].gain;
        assert_ne!(before, after);
        Ok(transaction_candidate("Candidate"))
    }));
}

/// F-15: unloading a slot leaves no value, script or binding of the old instrument behind.
#[test]
fn unloading_a_slot_clears_every_per_part_cell() {
    let (params, _core) = transaction_fixture();
    let atoms = params.shared.part(0).unwrap();
    assert!(!atoms.controls.lock_unpoisoned().is_empty());
    params.selection.write_unpoisoned().parts[0].path.clear();
    assert!(load_part(&params, 0, None));
    assert!(atoms.controls.lock_unpoisoned().is_empty());
    assert!(atoms.control_values().is_empty());
    assert!(atoms.engine_bindings.lock_unpoisoned().is_empty());
    assert!(atoms.node_meters.lock_unpoisoned().is_empty());
    assert!(atoms.scripts.lock_unpoisoned().views.is_empty());
}

/// F-16: a panic during a load leaves the slot retryable, not stuck at "Loading…".
#[test]
fn a_panicking_load_leaves_the_slot_retryable() {
    let (params, _core) = transaction_fixture();
    params.selection.write_unpoisoned().parts[0].path = "candidate.nki".into();
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        load_part_with(&params, 0, None, |_, _, _| panic!("loader bug"))
    }));
    assert!(panicked.is_err());
    {
        let view = params.shared.view.lock_unpoisoned();
        assert!(!view.parts[0].loading);
        assert!(view.parts[0].attempted.is_none());
        assert!(view.parts[0].status.starts_with("Load failed"));
    }
    assert!(load_part(&params, 0, Some(transaction_candidate("Retry"))), "the same request can run again");
}
