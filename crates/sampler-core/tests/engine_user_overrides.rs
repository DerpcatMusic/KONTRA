//! Direct v1 user-offset semantics over the shared v2 engine service.
use sampler_core::*;

fn address(group: i32) -> EngineParameterAddress {
    EngineParameterAddress {
        parameter: engine_parameter_id("ENGINE_PAR_CUTOFF").unwrap(),
        group,
        slot: 3,
        generic: -1,
    }
}
fn runtime() -> Runtime {
    let ids = [ControlId(10), ControlId(11)];
    let plan = Prepared::new(48000, vec![], vec![], 0)
        .unwrap()
        .with_controls(
            ids.into_iter()
                .map(|id| ControlDefinition {
                    id,
                    domain: ControlDomain::Real {
                        min: 10.,
                        max: 10000.,
                    },
                    default: ControlValue::Real(100.),
                })
                .collect(),
        )
        .unwrap()
        .with_engine_parameters(
            ids.into_iter()
                .enumerate()
                .map(|(group, control)| EngineParameterBinding {
                    address: address(group as i32),
                    control,
                    law: EngineParameterLaw::Exponential {
                        low: 10.,
                        high: 10000.,
                    },
                })
                .collect(),
            vec![],
        )
        .unwrap();
    let limits = Limits::for_plan(&plan, 128, 16);
    Runtime::new(plan, limits).unwrap()
}

#[test]
fn v1_offsets_ride_script_base_and_global_plus_one_group_offsets_add() {
    let mut rt = runtime();
    rt.set_engine_offsets(&[
        EngineParameterOffset {
            address: address(-1),
            offset: 0.1,
        },
        EngineParameterOffset {
            address: address(1),
            offset: 0.2,
        },
    ])
    .unwrap();
    let base = rt.engine_parameter(address(0)).unwrap();
    assert!((base - 333333).abs() <= 1);
    assert_eq!(rt.engine_parameter(address(1)).unwrap(), base);
    assert_eq!(
        rt.control_base_value(rt.active_plan(), ControlId(11))
            .unwrap(),
        ControlValue::Real(100.),
        "host persistence captures the unedited base"
    );
    let law = EngineParameterLaw::Exponential {
        low: 10.,
        high: 10000.,
    };
    for (group, offset) in [(0, 100000), (1, 300000)] {
        let playing = rt
            .control_value(rt.active_plan(), ControlId(10 + group))
            .unwrap();
        assert_eq!(playing, ControlValue::Real(law.decode(base + offset)));
    }
    rt.set_engine_parameter(address(1), 500000).unwrap();
    assert_eq!(
        rt.engine_parameter(address(1)).unwrap(),
        500000,
        "script read sees its base"
    );
    assert_eq!(
        rt.control_value(rt.active_plan(), ControlId(11)).unwrap(),
        ControlValue::Real(law.decode(800000))
    );
    rt.edit_controls(
        rt.active_plan(),
        None,
        &[ControlWrite {
            id: ControlId(11),
            value: ControlValue::Real(100.),
        }],
    )
    .unwrap();
    assert_eq!(
        rt.engine_parameter(address(1)).unwrap(),
        base,
        "direct native control writes also update the base"
    );
    rt.set_engine_offsets(&[]).unwrap();
    assert_eq!(
        rt.control_value(rt.active_plan(), ControlId(11)).unwrap(),
        ControlValue::Real(100.),
        "reset recovers the exact unrounded base"
    );
}

#[test]
fn v1_offset_validation_is_atomic_and_clamps_only_what_plays() {
    let mut rt = runtime();
    rt.set_engine_parameter(address(0), 950000).unwrap();
    rt.set_engine_offsets(&[EngineParameterOffset {
        address: address(0),
        offset: 0.2,
    }])
    .unwrap();
    assert_eq!(rt.engine_parameter(address(0)).unwrap(), 950000);
    assert_eq!(
        rt.control_value(rt.active_plan(), ControlId(10)).unwrap(),
        ControlValue::Real(10000.)
    );
    let before = rt.control_value(rt.active_plan(), ControlId(10)).unwrap();
    assert_eq!(
        rt.set_engine_offsets(&[EngineParameterOffset {
            address: address(0),
            offset: f32::NAN
        }]),
        Err(Error::InvalidInput)
    );
    assert_eq!(
        rt.control_value(rt.active_plan(), ControlId(10)).unwrap(),
        before
    );
}

#[test]
fn all_group_amplitude_offsets_follow_real_lanes_across_physical_slots() {
    let env = Envelope::new(480, 0, 9600, 0.5, 14400).unwrap();
    let plan = Prepared::new(48000, vec![], vec![], 0)
        .unwrap()
        .with_groups(2, vec![])
        .unwrap()
        .with_group_envelope_parameters(0, 13, 2, env)
        .unwrap()
        .with_group_envelope_parameters(1, 9, 7, env)
        .unwrap();
    let attack = engine_parameter_id("ENGINE_PAR_ATTACK").unwrap();
    let bindings: Vec<_> = plan
        .engine_parameter_bindings()
        .iter()
        .copied()
        .filter(|b| b.address.parameter == attack)
        .collect();
    let limits = Limits::for_plan(&plan, 128, 8);
    let mut rt = Runtime::new(plan, limits).unwrap();
    rt.set_engine_offsets(&[
        EngineParameterOffset {
            address: EngineParameterAddress {
                parameter: attack,
                group: -1,
                slot: -2,
                generic: -1,
            },
            offset: 0.1,
        },
        EngineParameterOffset {
            address: EngineParameterAddress {
                parameter: attack,
                group: 9,
                slot: -2,
                generic: -1,
            },
            offset: 0.2,
        },
    ])
    .unwrap();
    for b in bindings {
        let n = rt.engine_parameter(b.address).unwrap();
        let offset = if b.address.group == 9 { 300000 } else { 100000 };
        assert_eq!(
            rt.control_value(rt.active_plan(), b.control).unwrap(),
            ControlValue::Real(b.law.decode(n + offset))
        );
        assert_eq!(
            rt.control_base_value(rt.active_plan(), b.control).unwrap(),
            ControlValue::Real(480.)
        );
    }
}
