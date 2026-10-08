//! Batch construction must retain native lanes without quadratic schema copies.
use sampler_core::*;
mod support;

fn plan(groups: u32) -> Prepared {
    Prepared::new(48000, vec![], vec![], 0)
        .unwrap()
        .with_groups(groups, vec![])
        .unwrap()
}

#[test]
fn batched_envelope_schema_has_linear_requested_bytes() {
    let groups = 128u32;
    let mut result = Some(plan(groups));
    let bytes = support::allocated_bytes(|| {
        result = Some(
            result
                .take()
                .unwrap()
                .with_group_envelope_parameters_batch(
                    (0..groups).map(|g| (g, g as i32, 5, Envelope::default())),
                )
                .unwrap(),
        );
    });
    eprintln!("envelope_groups={groups} allocated_bytes={bytes}");
    assert!(
        bytes < groups as usize * 4096,
        "requested {bytes} bytes for {groups} envelope schemas"
    );
    let result = result.unwrap();
    assert_eq!(result.controls().len(), groups as usize * 6);
    assert_eq!(
        result.engine_parameter_bindings().len(),
        groups as usize * 6
    );
}

#[test]
fn batching_preserves_serial_lanes_engine_writes_and_validation() {
    let authored = Envelope::new(3, 2, 5, 0.75, 7).unwrap();
    let serial = (0..3)
        .try_fold(plan(3), |p, g| {
            p.with_group_envelope_parameters(g, g as i32 + 8, 5, authored)
        })
        .unwrap();
    let batched = plan(3)
        .with_group_envelope_parameters_batch((0..3).map(|g| (g, g as i32 + 8, 5, authored)))
        .unwrap();
    assert_eq!(serial.controls(), batched.controls());
    assert_eq!(
        serial.engine_parameter_bindings(),
        batched.engine_parameter_bindings()
    );
    let addresses: Vec<_> = serial
        .engine_parameter_bindings()
        .iter()
        .map(|b| b.address)
        .collect();
    let make = |p: Prepared| {
        let limits = Limits::for_plan(&p, 4, 16);
        Runtime::new(p, limits).unwrap()
    };
    let mut serial = make(serial);
    let mut batched = make(batched);
    support::without_heap(|| {
        for &address in &addresses {
            assert_eq!(
                serial.engine_parameter(address),
                batched.engine_parameter(address)
            );
            serial.set_engine_parameter(address, 123456).unwrap();
            batched.set_engine_parameter(address, 123456).unwrap();
            assert_eq!(
                serial.engine_parameter(address),
                batched.engine_parameter(address)
            );
        }
    });
    for invalid in [
        (3, 8, 5, authored),
        (1, -1, 5, authored),
        (1, 8, -1, authored),
        (0, 9, 5, authored),
    ] {
        assert!(matches!(
            plan(3).with_group_envelope_parameters_batch([(0, 8, 5, authored), invalid]),
            Err(Error::InvalidInput)
        ));
    }
    let mut empty = Some(plan(3));
    support::without_heap(|| {
        empty = Some(
            empty
                .take()
                .unwrap()
                .with_group_envelope_parameters_batch([])
                .unwrap(),
        );
    });
}
