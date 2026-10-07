use sampler_core::*;
mod support;
fn limits() -> Limits {
    Limits {
        notes: 1,
        channels: 0,
        performances: 1,
        families: 0,
        expressions: 1,
        voices: 0,
        decisions: 0,
        commands: 2,
        behaviors: 2,
        behavior_fuel: 16,
        behavior_cells: 8,
        note_cells: 0,
    }
}
fn program(code: Vec<Instruction>, instance: u16) -> Program {
    Program::new(code)
        .unwrap()
        .with_script_instance(ScriptInstanceId(instance))
        .with_wait_lifetime(WaitLifetime::Callback)
}
fn prepared(banks: Vec<Vec<i64>>, programs: Vec<Program>) -> Prepared {
    Prepared::new(48000, vec![], vec![], 0)
        .unwrap()
        .with_script_instances(banks)
        .unwrap()
        .with_programs(programs, None)
        .unwrap()
}

#[test]
fn large_array_views_share_only_the_bound_instance_and_retained_generation() {
    let array = ScriptArray {
        offset: 65536,
        len: 4,
    };
    let make_plan = |seed: i64| {
        let mut a = vec![99; 65540];
        a[65536..].copy_from_slice(&[seed + 10, seed + 20, seed + 30, seed + 40]);
        let b = vec![seed + 200; 65540];
        prepared(
            vec![a, b],
            vec![
                program(
                    vec![
                        Instruction::SetLocal { local: 0, value: 1 },
                        Instruction::SetLocal { local: 1, value: 7 },
                        Instruction::WriteScriptArray {
                            array,
                            index: 0,
                            local: 1,
                        },
                        Instruction::ReadScriptArray {
                            array,
                            index: 0,
                            local: 0,
                        },
                        Instruction::Wait(2),
                        Instruction::SetLocal { local: 0, value: 2 },
                        Instruction::ReadScriptArray {
                            array,
                            index: 0,
                            local: 1,
                        },
                    ],
                    0,
                ),
                program(
                    vec![
                        Instruction::SetLocal { local: 0, value: 1 },
                        Instruction::ReadScriptArray {
                            array,
                            index: 0,
                            local: 0,
                        },
                    ],
                    1,
                ),
            ],
        )
    };
    let (mut rt, mut control) = Runtime::with_plan_updates(make_plan(0), limits(), 2, 1).unwrap();
    control.submit(Box::new(make_plan(100))).unwrap();
    support::without_heap(|| {
        let old = rt.active_plan();
        let a = rt.start_plan_behavior(old, 0).unwrap();
        let b = rt.start_plan_behavior(old, 1).unwrap();
        assert_eq!(
            rt.behavior_local(a, 0),
            Ok(7),
            "index/destination alias reads before overwriting"
        );
        assert_eq!(rt.behavior_local(b, 0), Ok(200));
        assert_eq!(rt.script_cell(old, ScriptInstanceId(0), 65537), Ok(7));
        assert_eq!(rt.script_cell(old, ScriptInstanceId(1), 65537), Ok(200));
        rt.poll_plan_update().unwrap();
        let new = rt.active_plan();
        assert_eq!(rt.script_cell(new, ScriptInstanceId(0), 65537), Ok(120));
        assert_eq!(rt.collect_retired_plans(), 0);
        rt.render(&mut [[0.; 2]; 3]).unwrap();
        assert_eq!(rt.behavior_local(a, 1), Ok(30));
        assert_eq!(rt.behavior_outcome(a), Ok(Some(Outcome::Finished)));
        rt.flush_behaviors(|_, _, _| false);
        assert_eq!(rt.collect_retired_plans(), 0);
        rt.flush_behaviors(|_, _, outcome| {
            assert_eq!(outcome, Outcome::Finished);
            true
        });
        assert_eq!(rt.collect_retired_plans(), 1);
        assert_eq!(
            rt.script_cell(old, ScriptInstanceId(0), 65537),
            Err(Error::StaleHandle)
        );
        assert_eq!(rt.script_cell(new, ScriptInstanceId(0), 65537), Ok(120));
    });
    drop(control.retired().unwrap());
}

#[test]
fn array_indices_cannot_escape_their_view_and_read_zero_or_drop_the_write() {
    let array = ScriptArray { offset: 1, len: 2 };
    for index in [-1, 2, i64::MAX, i64::from(u32::MAX) + 1] {
        for (reads, operation) in [
            (
                true,
                Instruction::ReadScriptArray {
                    array,
                    index: 0,
                    local: 1,
                },
            ),
            (
                false,
                Instruction::WriteScriptArray {
                    array,
                    index: 0,
                    local: 1,
                },
            ),
        ] {
            let mut rt = Runtime::new(
                prepared(
                    vec![vec![99, 10, 20, 88]],
                    vec![program(
                        vec![
                            Instruction::SetLocal {
                                local: 0,
                                value: index,
                            },
                            Instruction::SetLocal {
                                local: 1,
                                value: 123,
                            },
                            operation,
                        ],
                        0,
                    )],
                ),
                limits(),
            )
            .unwrap();
            support::without_heap(|| {
                let plan = rt.active_plan();
                let callback = rt.start_plan_behavior(plan, 0).unwrap();
                assert_eq!(rt.behavior_outcome(callback), Ok(Some(Outcome::Finished)));
                // Kontakt reads outside the array as 0 and drops the write.
                assert_eq!(
                    rt.behavior_local(callback, 1),
                    Ok(if reads { 0 } else { 123 })
                );
                for (cell, value) in [99, 10, 20, 88].into_iter().enumerate() {
                    assert_eq!(
                        rt.script_cell(plan, ScriptInstanceId(0), cell as u32),
                        Ok(value)
                    );
                }
                rt.flush_behaviors(|_, _, _| true);
                assert_eq!(rt.pending_commands(), 0);
            });
        }
    }
}

#[test]
fn array_bank_views_and_both_registers_are_validated_before_activation() {
    for array in [
        ScriptArray { offset: 0, len: 0 },
        ScriptArray {
            offset: u32::MAX,
            len: 1,
        },
    ] {
        assert!(
            Program::new(vec![
                Instruction::End,
                Instruction::ReadScriptArray {
                    array,
                    index: 0,
                    local: 0
                }
            ])
            .is_err()
        );
    }
    let code = program(
        vec![
            Instruction::End,
            Instruction::ReadScriptArray {
                array: ScriptArray { offset: 2, len: 4 },
                index: 0,
                local: 1,
            },
        ],
        0,
    );
    assert!(
        Prepared::new(48000, vec![], vec![], 0)
            .unwrap()
            .with_script_instances(vec![vec![0; 5]])
            .unwrap()
            .with_programs(vec![code], None)
            .is_err()
    );
    for (index, local) in [(u16::MAX, 0), (0, u16::MAX)] {
        let p = prepared(
            vec![vec![0]],
            vec![program(
                vec![
                    Instruction::End,
                    Instruction::WriteScriptArray {
                        array: ScriptArray { offset: 0, len: 1 },
                        index,
                        local,
                    },
                ],
                0,
            )],
        );
        assert_eq!(p.behavior_local_count(), 65536);
        assert!(matches!(Runtime::new(p, limits()), Err(Error::Capacity)));
    }
}
