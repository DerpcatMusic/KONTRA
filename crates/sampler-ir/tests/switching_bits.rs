use sampler_ir::{Driver, SwitchKeys, SwitchOwner, Switching};

#[test]
fn every_switching_round_trips_through_a_byte() {
    for owner in [SwitchOwner::Native, SwitchOwner::Behavior] {
        for driver in [
            Driver::Keys,
            Driver::Velocity,
            Driver::Channel,
            Driver::Controller,
            Driver::Program,
        ] {
            for keys in [SwitchKeys::Keep, SwitchKeys::Play, SwitchKeys::Swallow] {
                let s = Switching {
                    owner,
                    driver,
                    keys,
                };
                assert_eq!(Switching::from_bits(s.to_bits()), Some(s));
            }
        }
    }
    assert_eq!(Switching::from_bits(0xff), None);
}

#[test]
fn amplitude_controllers_lists_live_controller_to_amplitude_routes_by_zone_count() {
    use sampler_ir::*;
    let mut ir = Instrument::default();
    ir.assets.push(Asset {
        location: AssetLocation::Path("a".into()),
        encoding: Encoding::Wav,
        root_key: None,
        loops: Vec::new(),
    });
    for cc in [1u8, 11, 7] {
        ir.modulators.push(Modulator {
            scope: Scope::Voice,
            source: ModulationSource::Controller(cc),
        });
    }
    let amp =
        |m: usize, depth| Route::new(ModulatorRef(m), Target::Amplitude, Depth::Normalized(depth));
    ir.routes = vec![amp(0, 1.0), amp(1, 1.0), amp(2, 0.0)];
    let zone = |routes: &[usize]| Zone {
        routes: routes.iter().map(|&r| RouteRef(r)).collect(),
        ..Zone::new(AssetRef(0))
    };
    ir.zones = vec![zone(&[0, 1, 2]), zone(&[0]), zone(&[0])];
    // CC1 drives three zones, CC11 one, CC7 has depth 0.
    assert_eq!(ir.amplitude_controllers(), vec![1, 11]);
}
