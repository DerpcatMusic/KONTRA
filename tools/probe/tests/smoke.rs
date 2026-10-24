//! Real-instrument smoke run: `KONTRA_PROBE_NKI=<path> cargo test -p kontra-probe -- --ignored`.

use std::path::Path;

use kontra_probe::{BLOCK, Mode, Pattern, Session, check};

#[test]
#[ignore = "needs KONTRA_PROBE_NKI pointing at an instrument"]
fn live_pattern_renders_every_step() {
    let Some(path) = std::env::var_os("KONTRA_PROBE_NKI") else {
        return;
    };
    let mut session = Session::load(Path::new(&path), 48_000.0).expect("instrument loads");
    let report = session
        .play(Pattern::Live, Mode::Offline, BLOCK)
        .expect("live pattern renders");
    assert_eq!(report.notes.len(), 64, "one entry per live step");
    let verdict = check(&mut session, "C6", Pattern::Live).expect("C6 runs");
    assert!(
        verdict.pass.is_some(),
        "C6 is measurable: {}",
        verdict.detail
    );
}
