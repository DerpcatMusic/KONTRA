use sampler_ir::*;
#[test]
fn filtered_zones_keep_their_physical_source_rows() {
    let mut ir = Instrument {
        zones: (0..3).map(|_| Zone::new(AssetRef(0))).collect(),
        assets: vec![Asset {
            location: AssetLocation::Path("one.wav".into()),
            residency: Default::default(),
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

/// Validation finds duplicate and unbound source rows with sets (it rescanned
/// earlier rows per entry); the rules are unchanged.
#[test]
fn route_control_duplicates_and_unbound_aliases_are_rejected() {
    let control = |key: &str| Control {
        key: key.into(),
        label: key.into(),
        value: ControlValue::Continuous { min: 0., max: 1., default: 0., unit: ControlUnit::None },
        automation: Automation::None,
    };
    let field = |ir: &Instrument| match ir.validate() {
        Err(ValidationError::OutOfRange { field, .. }) => Some(field),
        Err(other) => panic!("{other:?}"),
        Ok(()) => None,
    };
    let mut ir = Instrument {
        modulators: vec![Modulator { scope: Scope::Voice, source: ModulationSource::Velocity }],
        routes: (0..2)
            .map(|_| Route::new(ModulatorRef(0), Target::Amplitude, Depth::Normalized(0.5)))
            .collect(),
        controls: vec![control("depth"), control("bypass"), control("free")],
        ..Default::default()
    };
    ir.source_indices.route_controls = vec![
        (RouteRef(0), ControlRef(0), ControlRef(1)),
        (RouteRef(1), ControlRef(0), ControlRef(1)),
    ];
    let alias = |control| SourceControlAlias {
        control: ControlRef(control),
        parameter: "ENGINE_PAR_VOLUME".into(),
        address: SlotAddress { group: 0, slot: 0, generic: -1 },
    };
    ir.source_indices.control_aliases = vec![alias(1)];
    assert_eq!(field(&ir), None);
    ir.source_indices.control_aliases.push(alias(2));
    assert_eq!(field(&ir), Some("unbound native control alias"));
    ir.source_indices.control_aliases.pop();
    ir.source_indices.route_controls[1].0 = RouteRef(0);
    assert_eq!(field(&ir), Some("duplicate source route controls"));
}
