//! Untouched installed Harp; no PCM or script persistence.
#[test]
#[ignore = "requires installed Vista Harp; use kontakto-heavy"]
fn shipping_harp_compacts_without_fixture_mutation() {
    let root = std::env::var_os("KONTRA_KONTAKT_LIBRARIES").expect("installed library root");
    let path = std::path::PathBuf::from(root)
        .join("Performance Samples Vista/Instruments/Bonus/Vista - Harp.nki");
    for scripts in [false, true] {
        let library = sampler_kontakt::read(&path).unwrap();
        let zones = library.instrument.zones.len();
        assert!(library.instrument.source_indices.zones.iter().flatten().all(|z| z.0 < zones));
        let loaded = sampler_kontakt::load_read(library, &sampler_kontakt::Options {
            keys: 60..=60, scripts, mpe: None, ..Default::default()
        }, |_| {}, || false).unwrap();
        println!("W15_HARP_SHIPPING_LOAD scripts={scripts} original_zones={zones} plan_built=true");
        drop(loaded);
    }
}
