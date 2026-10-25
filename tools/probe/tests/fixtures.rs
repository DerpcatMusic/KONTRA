//! Playback contracts on the synthetic fixtures, with no vendor library: each
//! fixture is authored as KSP plus sample IR, written by the library creator,
//! then played.
//!
//! Contracts that pass today are asserted. Contracts that fail today are
//! `#[ignore]`d with the task that fixes them: run `cargo test -p kontra-probe
//! -- --ignored` to see each audited bug reproduce.

use kontra_probe::{Fixture, Pattern, Session, Verdict, check};

/// Sample rate the probe loads every fixture at.
const RATE: f64 = 48_000.0;

/// Runs contract `id` on `pattern` against `fixture`, expecting the voices the
/// fixture's script is written to start.
fn verdict(fixture: Fixture, id: &str, pattern: Pattern) -> Verdict {
    let path = fixture.build().expect("fixture builds");
    let mut session = Session::load(&path, RATE)
        .expect("fixture loads")
        .expect_voices(fixture.expected_voices())
        .expect_ui_writes(fixture.ui_writes_per_key());
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
fn cc_start_block_size_null_passes() {
    assert_passes(&verdict(Fixture::CcStart, "C4", Pattern::Live));
}

#[test]
fn cc_start_has_one_voice_per_mic_group() {
    assert_passes(&verdict(Fixture::CcStart, "C6", Pattern::Live));
}

#[test]
#[ignore = "fails until P1.2: a start offset from play_note lands one block late (C1 worst 128 frames)"]
fn cc_start_plays_every_note_on_time() {
    assert_passes(&verdict(Fixture::CcStart, "C1", Pattern::Live));
}

#[test]
#[ignore = "fails until P1.2: CC113 start offsets land one block late (C9 worst 128 frames)"]
fn cc_start_start_offsets_play_on_time() {
    assert_passes(&verdict(Fixture::CcStart, "C9", Pattern::Offsets));
}

#[test]
#[ignore = "fails until P1.2: 63 of 64 chords miss their first block; likely the same offset lateness as C1, not isolated"]
fn cc_start_chords_start_in_their_first_block() {
    assert_passes(&verdict(Fixture::CcStart, "C8", Pattern::Dense));
}

#[test]
#[ignore = "fails until P1.3: offline and real time differ when play_note offsets are used (null about -3 dB)"]
fn cc_start_offline_matches_real_time() {
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
fn key_position_chords_start_in_their_first_block() {
    assert_passes(&verdict(Fixture::KeyPosition, "C8", Pattern::Dense));
}

#[test]
fn key_position_block_size_null_passes() {
    assert_passes(&verdict(Fixture::KeyPosition, "C4", Pattern::Live));
}

#[test]
#[ignore = "fails until P1.3: offline and real time differ when play_note offsets are used (null -5.3 dB)"]
fn key_position_offline_matches_real_time() {
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
fn dense_has_one_voice_per_key_and_null_passes() {
    assert_passes(&verdict(Fixture::Dense, "C6", Pattern::Live));
    assert_passes(&verdict(Fixture::Dense, "C5", Pattern::Live));
}

/// Four keys per chord, one voice each: C6 counts voices per key, so the chord's
/// four voices pass. Live C6 alone never exercised a chord.
#[test]
fn dense_chords_have_one_voice_per_key() {
    assert_passes(&verdict(Fixture::Dense, "C6", Pattern::Dense));
}

/// Dense has no UI load to measure, so C7 is unsupported rather than a pass.
#[test]
fn dense_script_load_is_unsupported() {
    let verdict = verdict(Fixture::Dense, "C7", Pattern::Live);
    assert_eq!(verdict.pass, None, "{}", verdict.detail);
    assert!(
        verdict.detail.starts_with("unsupported"),
        "{}",
        verdict.detail
    );
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

/// Streamed has no script and no UI load: C7 is unsupported, not a fail.
#[test]
fn streamed_script_load_is_unsupported() {
    let verdict = verdict(Fixture::Streamed, "C7", Pattern::Live);
    assert_eq!(verdict.pass, None, "{}", verdict.detail);
    assert!(
        verdict.detail.starts_with("unsupported"),
        "{}",
        verdict.detail
    );
}

#[test]
#[ignore = "fails until P2.1: 300 UI writes per note silence every note (outbox full, callback aborted)"]
fn ui_load_plays_every_note_on_time() {
    assert_passes(&verdict(Fixture::UiLoad, "C1", Pattern::Live));
}

#[test]
#[ignore = "fails until P2.1: every note is killed under the UI load, so no voice starts (C6 0 of 1 per note)"]
fn ui_load_has_one_voice_per_note() {
    assert_passes(&verdict(Fixture::UiLoad, "C6", Pattern::Live));
}

#[test]
#[ignore = "fails until P2.1: four chords in one block overflow the UI outbox; 0 of 64 chords audible"]
fn ui_load_chords_start_in_their_first_block() {
    assert_passes(&verdict(Fixture::UiLoad, "C8", Pattern::Dense));
}

#[test]
#[ignore = "fails until P2.1: script load (C1 and C6 under 300 UI writes per note) drops every note"]
fn ui_load_script_load_keeps_every_note() {
    assert_passes(&verdict(Fixture::UiLoad, "C7", Pattern::Live));
}
