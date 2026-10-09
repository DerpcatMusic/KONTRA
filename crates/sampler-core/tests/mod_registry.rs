//! Failing-first contract for the approved shared parameter registry.
//! Source-only until W0 supplies the exact frozen integration baseline.
use sampler_core::{
    ControlId, EngineParameterAddress, ModRoute, ParameterAddress, ParameterDescriptor,
    ParameterLaw, ParameterRegistry, ParameterScope, ParameterUnit,
};

fn descriptor(scope: ParameterScope, node: u32, parameter: u32) -> ParameterDescriptor {
    ParameterDescriptor {
        address: ParameterAddress {
            scope,
            node,
            parameter,
        },
        control: ControlId(
            0xfeed_0000
                + u128::from(node) * 256
                + u128::from(parameter)
                + match scope {
                    ParameterScope::Voice => 0,
                    ParameterScope::Group(id) => (1u128 << 64) + u128::from(id),
                    ParameterScope::Bus(id) => (2u128 << 64) + u128::from(id),
                    ParameterScope::Plan => 3u128 << 64,
                },
        ),
        name: "custom level".into(),
        role: sampler_core::ParameterRole::Gain,
        unit: ParameterUnit::Linear,
        range: [0., 2.],
        default: 1.,
        law: ParameterLaw::Linear,
        display: Default::default(),
    }
}

#[test]
fn generic_parameters_resolve_without_a_native_name_table_entry() {
    let d = descriptor(ParameterScope::Voice, 7, 0xfe);
    let mut registry = ParameterRegistry::default();
    registry.register(d.clone()).unwrap();
    let prepared = registry.prepare().unwrap();
    let lane = prepared
        .resolve(d.address)
        .expect("arbitrary registered parameter must be routable");
    assert_eq!(prepared.descriptor(lane), Some(&d));
    assert!(
        prepared
            .resolve(ParameterAddress {
                parameter: 0xff,
                ..d.address
            })
            .is_none()
    );
}

#[test]
fn a_route_accepts_an_open_parameter_address_without_a_target_enum_case() {
    let address = descriptor(ParameterScope::Voice, 7, 0xfe).address;
    let route = ModRoute::new(0, address, 0.25);
    assert_eq!(route.target, address);
    assert_eq!(route.depth, 0.25);
}

#[test]
fn scopes_and_sparse_processor_ids_resolve_to_distinct_dense_lanes() {
    let mut registry = ParameterRegistry::default();
    let descriptors = [
        descriptor(ParameterScope::Voice, 901, 2),
        descriptor(ParameterScope::Group(208), 901, 2),
        descriptor(ParameterScope::Bus(4), 901, 2),
        descriptor(ParameterScope::Plan, 901, 2),
    ];
    for d in &descriptors {
        registry.register(d.clone()).unwrap();
    }
    let prepared = registry.prepare().unwrap();
    let mut lanes: Vec<_> = descriptors
        .iter()
        .map(|d| prepared.resolve(d.address).unwrap())
        .collect();
    lanes.sort();
    lanes.dedup();
    assert_eq!(
        lanes,
        vec![0, 1, 2, 3],
        "scope isolation must survive dense packing"
    );
}

#[test]
fn native_alias_and_editor_metadata_share_the_registered_owner() {
    let d = descriptor(ParameterScope::Voice, 208, 2);
    let native = EngineParameterAddress {
        parameter: sampler_core::engine_parameter_id("$ENGINE_PAR_VOLUME").unwrap(),
        group: 208,
        slot: 0,
        generic: -1,
    };
    let mut registry = ParameterRegistry::default();
    registry.register(d.clone()).unwrap();
    registry.alias_native(native, d.address).unwrap();
    let prepared = registry.prepare().unwrap();
    assert_eq!(prepared.resolve_native(native), prepared.resolve(d.address));
    let visible: Vec<_> = prepared.descriptors().collect();
    assert_eq!(
        visible,
        vec![&d],
        "aliases must not create duplicate UI or value owners"
    );
    assert_eq!(visible[0].control, d.control);
    assert_eq!(visible[0].name, "custom level");
    assert_eq!(visible[0].range, [0., 2.]);
}

#[test]
fn invalid_or_ambiguous_descriptors_fail_before_publication() {
    for range in [[f64::NAN, 2.], [0., f64::INFINITY], [2., 0.]] {
        let mut registry = ParameterRegistry::default();
        let mut d = descriptor(ParameterScope::Voice, 1, 2);
        d.range = range;
        assert!(registry.register(d).is_err());
    }
    for default in [-1., 3., f64::NAN, f64::INFINITY] {
        let mut registry = ParameterRegistry::default();
        let mut d = descriptor(ParameterScope::Voice, 1, 2);
        d.default = default;
        assert!(registry.register(d).is_err());
    }
    let mut registry = ParameterRegistry::default();
    let d = descriptor(ParameterScope::Voice, 1, 2);
    registry.register(d.clone()).unwrap();
    assert!(
        registry.register(d).is_err(),
        "ambiguous address is a preparation error"
    );
    let missing = ParameterAddress {
        scope: ParameterScope::Voice,
        node: 999,
        parameter: 2,
    };
    let native = EngineParameterAddress {
        parameter: 0,
        group: 1,
        slot: 2,
        generic: -1,
    };
    assert!(registry.alias_native(native, missing).is_err());
}

#[test]
fn conflicting_native_aliases_cannot_redirect_an_existing_owner() {
    let first = descriptor(ParameterScope::Group(2), 7, 0);
    let second = descriptor(ParameterScope::Group(3), 7, 0);
    let native = EngineParameterAddress {
        parameter: sampler_core::engine_parameter_id("$ENGINE_PAR_VOLUME").unwrap(),
        group: 2,
        slot: -1,
        generic: -1,
    };
    let mut registry = ParameterRegistry::default();
    registry.register(first.clone()).unwrap();
    registry.register(second.clone()).unwrap();
    registry.alias_native(native, first.address).unwrap();
    assert!(registry.alias_native(native, second.address).is_err());
    let prepared = registry.prepare().unwrap();
    assert_eq!(
        prepared.resolve_native(native),
        prepared.resolve(first.address)
    );
    assert_ne!(
        prepared.resolve_native(native),
        prepared.resolve(second.address)
    );
}

#[test]
fn preparation_freezes_descriptor_metadata_and_registration_order() {
    let mut registry = ParameterRegistry::default();
    let mut source = descriptor(ParameterScope::Plan, 0xffff, 0x1000);
    registry.register(source.clone()).unwrap();
    source.name = "caller changed its copy".into();
    source.range = [0., 100.];
    let second = descriptor(ParameterScope::Voice, 1, 0);
    registry.register(second.clone()).unwrap();
    let prepared = registry.prepare().unwrap();
    let first_lane = prepared.resolve(source.address).unwrap();
    assert_eq!(first_lane, 0);
    assert_eq!(prepared.resolve(second.address), Some(1));
    let published = prepared.descriptor(first_lane).unwrap();
    assert_eq!(published.name, "custom level");
    assert_eq!(published.range, [0., 2.]);
    assert_eq!(published.default, 1.);
    assert_eq!(published.control, source.control);
}
