//! Facts measured on real Kontakt 8.13.1 through the native oracle
//! (tools/kontakt-oracle). The fixture was written by Kontakt itself
//! (probes/volume_law.lua): a fresh instrument whose 14 groups were set to
//! known dB values through Kontakt's own Lua API, then saved.

use std::path::Path;

const SET_DB: [f64; 14] = [-60., -36., -30., -24., -16., -12., -6., -3., -1., 0., 1., 3., 6., 12.];

fn fixture() -> sampler_kontakt::Kontakt {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/kontakt8131-volume-law.nki");
    sampler_kontakt::read(&path).expect("an instrument saved by Kontakt 8.13.1 loads")
}

#[test]
fn kontakt_8131_saved_instrument_loads() {
    // Failed before: BParExternalMod 0x105 (Kontakt 8.13.1 FUN_140d09620) was rejected.
    assert_eq!(fixture().instrument.groups.len(), SET_DB.len());
}

#[test]
fn kontakt_volume_db_is_six_db_per_doubling() {
    let k = fixture();
    for (group, db) in k.instrument.groups.iter().zip(SET_DB) {
        let sampler_ir::Gain::Linear(stored) = group.gain else { panic!("{:?}", group.gain) };
        let expected = (db / 6.0).exp2();
        assert!((stored - expected).abs() <= expected * 1e-6, "{db} dB stored {stored}, 2^(dB/6) = {expected}");
    }
}

/// probes/start_options.lua: start conditions set through Kontakt's own API.
/// Kontakt stores the round-robin position exactly as given to
/// `set_group_start_options`; its getter reports stored + 1. Stored positions are
/// 0-based (native evaluator 0x140876bd0); StartTest positions are 1-based.
#[test]
fn group_start_conditions_match_what_kontakt_stored() {
    use sampler_ir::StartTest::*;
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/kontakt8131-start-options.nki");
    let k = sampler_kontakt::read(&path).expect("loads");
    let tests: Vec<Vec<sampler_ir::StartTest>> =
        k.instrument.groups.iter().map(|g| g.start.iter().map(|s| s.test).collect()).collect();
    assert_eq!(
        tests,
        vec![
            vec![RoundRobin(2)],
            vec![RoundRobin(3)],
            vec![RoundRobin(4)],
            vec![RoundRobin(6)],
            vec![Random],
            vec![Key { low: 36, high: 40 }, RoundRobin(3)],
            vec![Controller { controller: 11, low: 10, high: 90 }],
        ]
    );
}

/// Native playback modes (tools/kontakt-oracle/dump.lua, `get_group_playback_mode`, Kontakt
/// 8.13.1) for each group in order, paired with the stored source mode of that group.
/// Stored 0 is Sampler and stored 3 is DFD (`library.rs` SOURCE_MODE_DFD; decompile menu
/// `0140610000.c` lists DFD with UI id 4, runtime mode 4 at `+0xba8`).
#[test]
fn stored_source_modes_match_kontakt_playback_modes() {
    use ni_file::kontakt::objects::{GroupList, Program};
    let cases: [(&str, &[u32]); 2] = [
        ("tests/fixtures/kontakt8131-volume-law.nki", &[3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
        ("tests/fixtures/kontakt8131-start-options.nki", &[3, 0, 0, 0, 0, 0, 0]),
    ];
    for (file, stored) in cases {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(file);
        let chunks = sampler_kontakt::read_chunks(&path).expect("chunks");
        let program = Program::try_from(chunks.find_first(0x28).expect("program")).expect("program");
        let groups = GroupList::try_from(program.0.find_first(0x33).expect("groups")).expect("groups");
        let modes: Vec<u32> = groups
            .groups
            .iter()
            .map(|g| g.source_params().expect("source").0.mode)
            .collect();
        assert_eq!(modes, stored, "{file}");
    }
}
