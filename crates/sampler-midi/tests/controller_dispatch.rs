use sampler_core::{Instruction as I, *};
use sampler_midi::{Applied, Ingress, Mpe, Packets, Version, Zone};
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;

fn runtime(remap: bool) -> Runtime {
    let mut code = vec![
        I::SuppressController,
        I::ReadControllerPort { local: 2 },
        I::ReadControllerGroup { local: 3 },
        I::ReadControllerChannel { local: 4 },
    ];
    if remap {
        code.extend([
            I::SetLocal {
                local: 0,
                value: 64,
            },
            I::ReadControllerValue { local: 1 },
            I::WriteController {
                controller: 0,
                value: 1,
            },
        ]);
    }
    let plan = Prepared::new(48000, vec![], vec![], 0)
        .unwrap()
        .with_programs(
            vec![
                Program::new(code)
                    .unwrap()
                    .with_wait_lifetime(WaitLifetime::Callback),
            ],
            None,
        )
        .unwrap()
        .with_controller_program(0)
        .unwrap();
    Runtime::new(
        plan,
        Limits {
            notes: 4,
            performances: 2,
            channels: 16,
            families: 0,
            voices: 0,
            expressions: 4,
            decisions: 0,
            commands: 0,
            behaviors: 2,
            behavior_cells: 10,
            behavior_fuel: 8,
            note_cells: 0,
        },
    )
    .unwrap()
}

fn assert_origin(rt: &mut Runtime, expected: [i64; 3]) {
    let mut callback = None;
    rt.flush_behaviors(|id, _, outcome| {
        assert_eq!(outcome, Outcome::Finished);
        callback = Some(id);
        false
    });
    let id = callback.unwrap();
    for (index, value) in expected.into_iter().enumerate() {
        assert_eq!(rt.behavior_local(id, index as u16 + 2), Ok(value));
    }
    rt.flush_behaviors(|flushed, _, outcome| {
        assert_eq!(flushed, id);
        assert_eq!(outcome, Outcome::Finished);
        true
    });
}

#[test]
fn midi1_and_midi2_dispatch_before_pedals_or_downstream_controller_projection() {
    for version in [Version::Midi1, Version::Midi2] {
        for remap in [false, true] {
            let mut rt = runtime(remap);
            let ingress = Ingress::new(2, [Some(version); 16]);
            support::without_heap(|| {
                let domain = rt.performance(1).unwrap();
                let len = if version == Version::Midi1 { 1 } else { 2 };
                let on = if version == Version::Midi1 {
                    [0x2293_3c7f, 0]
                } else {
                    [0x4293_3c00, 0xffff0000]
                };
                let Applied::Started(note) = ingress
                    .apply_in(
                        &mut rt,
                        domain,
                        Packets::new(&on[..len]).next().unwrap().unwrap(),
                    )
                    .unwrap()
                else {
                    panic!()
                };
                let number = if remap { 1 } else { 64 };
                let down = if version == Version::Midi1 {
                    [0x22b3_007f | (number << 8), 0]
                } else {
                    [0x42b3_0000 | (number << 8), 0x80000001]
                };
                ingress
                    .apply_in(
                        &mut rt,
                        domain,
                        Packets::new(&down[..len]).next().unwrap().unwrap(),
                    )
                    .unwrap();
                assert_origin(&mut rt, [2, 2, 3]);
                rt.key_up(note, None).unwrap();
                assert_eq!(rt.note(note).unwrap().2, remap);
                assert_eq!(rt.controller(domain, number as u8), Ok(0));
                let expected = if version == Version::Midi1 {
                    u32::MAX
                } else {
                    0x80000001
                };
                assert_eq!(rt.input_controller(domain, number as u8), Ok(expected));
                assert_eq!(
                    rt.controller(domain, 64),
                    Ok(if remap { expected } else { 0 })
                );
                let up = if version == Version::Midi1 {
                    [0x22b3_0000 | (number << 8), 0]
                } else {
                    [0x42b3_0000 | (number << 8), 0]
                };
                ingress
                    .apply_in(
                        &mut rt,
                        domain,
                        Packets::new(&up[..len]).next().unwrap().unwrap(),
                    )
                    .unwrap();
                assert!(!rt.note(note).unwrap().2);
                assert_origin(&mut rt, [2, 2, 3]);
                rt.flush_behaviors(|_, _, outcome| {
                    assert_eq!(outcome, Outcome::Finished);
                    true
                });
                rt.flush_ended(|_| true);
                assert_eq!(rt.note_count(), 0);
            });
        }
    }
}

#[test]
fn an_mpe_manager_remap_to_sustain_retains_the_entire_zone_scope() {
    for zone in [Zone::Lower, Zone::Upper] {
        let mut rt = runtime(true);
        let domain = rt.performance(1).unwrap();
        let mut mpe = Mpe::new_in(&rt, domain, 2, 3, zone, 2, 4).unwrap();
        support::without_heap(|| {
            let manager = if zone == Zone::Lower { 0 } else { 15 };
            let member = if zone == Zone::Lower { 2 } else { 13 };
            let on = [0x2390_3c7f | (member << 16)];
            let Applied::Started(note) = mpe
                .apply(&mut rt, Packets::new(&on).next().unwrap().unwrap())
                .unwrap()
            else {
                panic!()
            };
            let down = [0x23b0_017f | (manager << 16)];
            mpe.apply(&mut rt, Packets::new(&down).next().unwrap().unwrap())
                .unwrap();
            assert_origin(&mut rt, [2, 3, i64::from(manager)]);
            rt.key_up(note, None).unwrap();
            assert!(rt.note(note).unwrap().2);
            assert_eq!(rt.controller(domain, 1), Ok(0));
            assert_eq!(rt.controller(domain, 64), Ok(u32::MAX));
            let up = [0x23b0_0100 | (manager << 16)];
            mpe.apply(&mut rt, Packets::new(&up).next().unwrap().unwrap())
                .unwrap();
            assert!(!rt.note(note).unwrap().2);
            assert_origin(&mut rt, [2, 3, i64::from(manager)]);
            rt.flush_behaviors(|_, _, outcome| {
                assert_eq!(outcome, Outcome::Finished);
                true
            });
            rt.flush_ended(|_| true);
            assert_eq!(rt.note_count(), 0);
        });
    }
}
