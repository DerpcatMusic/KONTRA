use sampler_ir::*;

fn catalog() -> Instrument {
    let mut ir = Instrument {
        assets: vec![Asset {
            location: AssetLocation::Path("synthetic.wav".into()),
            residency: Default::default(),
            encoding: Encoding::Wav,
            root_key: None,
            loops: vec![],
        }],
        zones: (0..4).map(|_| Zone::new(AssetRef(0))).collect(),
        ..Default::default()
    };
    ir.zones[1].keys = KeyRange { low: 12, high: 12 };
    ir.source_indices.zones = vec![Some(ZoneRef(0)), Some(ZoneRef(1)), Some(ZoneRef(2))];
    ir.source_indices.zone_catalog = vec![
        SourceZone {
            id: Some(0),
            home: SourceZoneHome {
                group: 42,
                low_key: 60,
                high_key: 60,
            },
        },
        SourceZone {
            id: Some(7),
            home: SourceZoneHome {
                group: 11,
                low_key: 12,
                high_key: 12,
            },
        },
        SourceZone {
            id: Some(2018),
            home: SourceZoneHome {
                group: 92,
                low_key: 0,
                high_key: 127,
            },
        },
    ];
    ir.zone_routing_mode = Some(3);
    ir.zone_routing = vec![ZoneRouting {
        source_zone: 7,
        group: 11,
        low_key: 12,
        high_key: 12,
        variants: vec![
            ZoneRouteVariant {
                group: 11,
                zone: Some(ZoneRef(1)),
            },
            ZoneRouteVariant {
                group: 42,
                zone: Some(ZoneRef(3)),
            },
            ZoneRouteVariant {
                group: 92,
                zone: None,
            },
        ],
    }];
    ir
}

#[test]
fn filtering_preserves_physical_catalog_and_optional_recipe_unavailability() {
    let mut ir = catalog();
    ir.validate().unwrap();
    let rows = ir.source_indices.zone_catalog.clone();
    ir.retain_zones(|z| z.keys.low != 12);
    assert_eq!(ir.source_indices.zone_catalog, rows);
    assert_eq!(
        ir.source_indices.zones,
        vec![Some(ZoneRef(0)), None, Some(ZoneRef(1))]
    );
    assert_eq!(
        ir.zone_routing[0].variants,
        vec![
            ZoneRouteVariant {
                group: 11,
                zone: None
            },
            ZoneRouteVariant {
                group: 42,
                zone: Some(ZoneRef(2))
            },
            ZoneRouteVariant {
                group: 92,
                zone: None
            },
        ]
    );
    ir.validate().unwrap();
    // The bank contains 3 retained recipes, while physical order still has 3
    // rows and native ID7 remains known even without its admitted primary.
    assert_eq!(ir.source_indices.zone_catalog[1].id, Some(7));
}

#[test]
fn invalid_identity_mode_aliases_and_initial_homes_are_structural_errors() {
    let original = catalog();
    let mut bad = original.clone();
    bad.source_indices.zone_catalog.pop();
    assert!(matches!(
        bad.validate(),
        Err(ValidationError::ZoneRouting { .. })
    ));
    // A shared identity is valid: Kontakt keeps it on every zone and its
    // lookup addresses the first.
    let mut shared = original.clone();
    shared.source_indices.zone_catalog[2].id = Some(0);
    shared.validate().unwrap();
    let mut bad = original.clone();
    bad.source_indices.zone_catalog[1].id = None;
    assert!(matches!(
        bad.validate(),
        Err(ValidationError::ZoneRouting { .. })
    ));
    for mode in [None, Some(0), Some(1), Some(4)] {
        let mut bad = original.clone();
        bad.zone_routing_mode = mode;
        assert!(matches!(
            bad.validate(),
            Err(ValidationError::ZoneRouting { .. })
        ));
    }
    let mut bad = original.clone();
    bad.zone_routing[0].high_key = 13;
    assert!(matches!(
        bad.validate(),
        Err(ValidationError::ZoneRouting { .. })
    ));
    let mut bad = original.clone();
    bad.zone_routing[0].variants[1].zone = Some(ZoneRef(1));
    assert!(matches!(
        bad.validate(),
        Err(ValidationError::ZoneRouting { .. })
    ));
    let mut bad = original.clone();
    bad.zone_routing[0].variants[0].zone = None;
    assert!(matches!(
        bad.validate(),
        Err(ValidationError::ZoneRouting { .. })
    ));
    // Absence of old-cache metadata is valid; no native IDs are fabricated.
    let mut old = original;
    old.source_indices.zone_catalog.clear();
    old.zone_routing.clear();
    old.validate().unwrap();
}

#[cfg(feature = "cache")]
#[test]
fn old_cache_catalog_and_routing_fields_default_to_absent() {
    let ir = Instrument::default();
    let mut value = serde_json::to_value(&ir).unwrap();
    value.as_object_mut().unwrap().remove("zone_routing");
    value.as_object_mut().unwrap().remove("zone_routing_mode");
    value["source_indices"]
        .as_object_mut()
        .unwrap()
        .remove("zone_catalog");
    let restored: Instrument = serde_json::from_value(value).unwrap();
    assert!(restored.zone_routing.is_empty());
    assert!(restored.zone_routing_mode.is_none());
    assert!(restored.source_indices.zone_catalog.is_empty());
}
