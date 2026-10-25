//! Playback contracts on the synthetic fixtures, with no vendor library: each
//! fixture is built from generated WAVs by the library creator, then played.
//!
//! Only contracts that pass today are asserted. None of the fixtures reproduces
//! an audited bug yet: the creator cannot author the UI load, the CC/KeyPosition
//! sample-start routes or the play_note double attack (see `Fixture::gaps`).

use kontra_probe::{Fixture, Pattern, Session, Verdict, check};

/// Sample rate the probe loads every fixture at.
const RATE: f64 = 48_000.0;

/// Runs contract `id` on `pattern` against `fixture`.
fn verdict(fixture: Fixture, id: &str, pattern: Pattern) -> Verdict {
    let path = fixture.build().expect("fixture builds");
    let mut session = Session::load(&path, RATE).expect("fixture loads");
    check(&mut session, id, pattern).expect("contract runs")
}

fn assert_passes(verdict: &Verdict) {
    assert_eq!(
        verdict.pass,
        Some(true),
        "{} [{}]: {}",
        verdict.id,
        verdict.pattern,
        verdict.detail
    );
}

#[test]
fn every_fixture_builds_and_loads() {
    for fixture in Fixture::ALL {
        let path = fixture.build().expect("fixture builds");
        Session::load(&path, RATE).unwrap_or_else(|e| panic!("{fixture}: {e:#}"));
    }
}

#[test]
fn cc_start_plays_every_note_on_time() {
    assert_passes(&verdict(Fixture::CcStart, "C1", Pattern::Live));
}

#[test]
fn cc_start_chords_start_in_their_first_block() {
    assert_passes(&verdict(Fixture::CcStart, "C8", Pattern::Dense));
}

#[test]
fn cc_start_has_one_voice_per_mic_group() {
    assert_passes(&verdict(Fixture::CcStart, "C6", Pattern::Live));
}

#[test]
fn cc_start_null_tests_pass() {
    assert_passes(&verdict(Fixture::CcStart, "C4", Pattern::Live));
    assert_passes(&verdict(Fixture::CcStart, "C5", Pattern::Live));
}

#[test]
fn key_position_plays_every_note_on_time() {
    assert_passes(&verdict(Fixture::KeyPosition, "C1", Pattern::Live));
}

#[test]
fn key_position_has_one_voice_per_note() {
    assert_passes(&verdict(Fixture::KeyPosition, "C6", Pattern::Live));
}

#[test]
fn key_position_null_tests_pass() {
    assert_passes(&verdict(Fixture::KeyPosition, "C4", Pattern::Live));
    assert_passes(&verdict(Fixture::KeyPosition, "C5", Pattern::Live));
}

#[test]
fn dense_chords_start_in_their_first_block() {
    assert_passes(&verdict(Fixture::Dense, "C8", Pattern::Dense));
}

#[test]
fn dense_plays_every_note_on_time() {
    assert_passes(&verdict(Fixture::Dense, "C1", Pattern::Live));
    assert_passes(&verdict(Fixture::Dense, "C1", Pattern::Dense));
}

#[test]
fn streamed_plays_every_note_on_time() {
    assert_passes(&verdict(Fixture::Streamed, "C1", Pattern::Live));
}

#[test]
fn streamed_null_tests_pass() {
    assert_passes(&verdict(Fixture::Streamed, "C4", Pattern::Live));
    assert_passes(&verdict(Fixture::Streamed, "C5", Pattern::Live));
}
