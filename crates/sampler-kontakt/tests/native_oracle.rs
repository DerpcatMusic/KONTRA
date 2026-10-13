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
