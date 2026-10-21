//! Reproductions for docs/audit/lifecycle-and-core-20261021.md. Each test
//! states the contract the audit expects; `known bug` tests fail today.
use super::tests::{transaction_candidate, transaction_fixture};
use super::*;

/// F-03: a canceled or failed switch must not make a later request for the
/// still-active source reload it. `attempted` is the only dedup key, so the
/// active source is rebuilt (new generation, voices cut) after A -> B(canceled) -> A.
#[test]
#[ignore = "known bug F-03: reselecting the active source after a canceled switch reloads it"]
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
#[ignore = "known bug F-03: reselecting the active source after a failed switch reloads it"]
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
#[ignore = "known bug F-05: a part prepared for another sample rate keeps sounding after reset"]
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
