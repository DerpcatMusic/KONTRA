use sampler_ir::*;
#[test]
fn filtered_zones_keep_their_physical_source_rows() {
    let mut ir = Instrument {
        zones: (0..3).map(|_| Zone::new(AssetRef(0))).collect(),
        assets: vec![Asset {
            location: AssetLocation::Path("one.wav".into()),
            encoding: Encoding::Wav,
            root_key: None,
            loops: vec![],
        }],
        source_indices: SourceIndices {
            zones: vec![
                Some(ZoneRef(0)),
                None,
                Some(ZoneRef(1)),
                None,
                Some(ZoneRef(2)),
            ],
            ..Default::default()
        },
        ..Default::default()
    };
    ir.zones[1].keys = KeyRange { low: 12, high: 12 };
    ir.retain_zones(|zone| zone.keys.low != 12);
    assert_eq!(
        ir.source_indices.zones,
        [Some(ZoneRef(0)), None, None, None, Some(ZoneRef(1))]
    );
}
