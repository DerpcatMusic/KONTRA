use sampler_core::{Instruction as I, *};
mod support;

fn address() -> ChannelAddress {
    ChannelAddress {
        protocol: Protocol::Midi2,
        port: 2,
        group: 3,
        channel: 15,
    }
}
fn program(code: Vec<I>) -> Program {
    Program::new(code)
        .unwrap()
        .with_wait_lifetime(WaitLifetime::Callback)
}
fn plan(programs: Vec<Program>, stages: Vec<usize>, globals: usize) -> Prepared {
    Prepared::new(48000, vec![], vec![], 0)
        .unwrap()
        .with_script_instances(vec![vec![0; globals]])
        .unwrap()
        .with_programs(
            programs
                .into_iter()
                .map(|p| p.with_script_instance(ScriptInstanceId(0)))
                .collect(),
            None,
        )
        .unwrap()
        .with_controller_programs(stages)
        .unwrap()
}
fn limits(plan: &Prepared, behaviors: usize) -> Limits {
    Limits {
        notes: 1,
        performances: 2,
        channels: 0,
        families: 0,
        voices: 0,
        expressions: 1,
        decisions: 0,
        commands: behaviors,
        behaviors,
        behavior_cells: plan.behavior_local_count() * behaviors,
        behavior_fuel: 64,
        note_cells: 0,
    }
}

#[test]
fn consumption_and_generated_ccs_project_only_into_following_stages() {
    let audit = |base, target| {
        let mut code = vec![
            I::ReadControllerNumber { local: 0 },
            I::WriteScriptCell {
                cell: base,
                local: 0,
            },
            I::ReadControllerValue { local: 1 },
            I::WriteScriptCell {
                cell: base + 1,
                local: 1,
            },
        ];
        for cc in 1..=3 {
            code.extend([
                I::SetLocal {
                    local: 0,
                    value: cc,
                },
                I::ReadInputController {
                    controller: 0,
                    local: 2,
                },
                I::WriteScriptCell {
                    cell: base + 1 + cc as u32,
                    local: 2,
                },
            ]);
        }
        if let Some(target) = target {
            code.extend([
                I::SuppressController,
                I::SetLocal {
                    local: 0,
                    value: target,
                },
                I::AddLocal { local: 1, value: 1 },
                I::WriteController {
                    controller: 0,
                    value: 1,
                },
            ]);
        } else {
            code.push(I::ForwardController);
        }
        if base == 0 {
            code.extend([
                I::ReadScriptCell { local: 2, cell: 11 },
                I::WriteScriptCell { cell: 15, local: 2 },
            ]);
        }
        program(code)
    };
    let prepared = plan(
        vec![audit(0, Some(2)), audit(5, Some(3)), audit(10, None)],
        vec![0, 1, 2],
        16,
    );
    let budget = limits(&prepared, 3);
    let mut rt = Runtime::new(prepared, budget).unwrap();
    support::without_heap(|| {
        let domain = rt.performance(1).unwrap();
        rt.dispatch_controller(domain, address(), 0xe000, 1, 0x8000_0001)
            .unwrap();
        let expected = [
            1,
            0x8000_0001,
            0x8000_0001,
            0,
            0,
            2,
            0x8000_0002,
            0,
            0x8000_0002,
            0,
            3,
            0x8000_0003,
            0,
            0,
            0x8000_0003,
        ];
        for (cell, value) in expected.into_iter().enumerate() {
            assert_eq!(
                rt.script_cell(rt.active_plan(), ScriptInstanceId(0), cell as u32),
                Ok(value)
            );
        }
        // A generated write finishes its downstream callbacks before the
        // creating callback executes its next instruction.
        assert_eq!(
            rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 15),
            Ok(0x8000_0003)
        );
        assert_eq!(rt.input_controller(domain, 1), Ok(0x8000_0001));
        for cc in [2, 3] {
            assert_eq!(rt.input_controller(domain, cc), Ok(0));
        }
        assert_eq!(rt.controller(domain, 1), Ok(0));
        assert_eq!(rt.controller(domain, 2), Ok(0));
        assert_eq!(rt.controller(domain, 3), Ok(0x8000_0003));
        assert_eq!(rt.controller(rt.performance(0).unwrap(), 3), Ok(0));
        let mut completed = 0;
        rt.flush_behaviors(|_, _, outcome| {
            assert_eq!(outcome, Outcome::Finished);
            completed += 1;
            true
        });
        assert_eq!(completed, 3);
        // Every reservation returned; reuse the complete chain under the same budget.
        rt.dispatch_controller(domain, address(), 0xe000, 1, 17)
            .unwrap();
        assert_eq!(rt.controller(domain, 3), Ok(19));
    });
}

#[test]
fn empty_module_positions_preserve_projection_and_only_reserve_actual_callbacks() {
    let prepared = plan(
        vec![
            program(vec![
                I::SuppressController,
                I::SetLocal { local: 0, value: 2 },
                I::ReadControllerValue { local: 1 },
                I::WriteController {
                    controller: 0,
                    value: 1,
                },
            ]),
            program(vec![
                I::SetLocal { local: 0, value: 1 },
                I::ReadInputController {
                    controller: 0,
                    local: 1,
                },
                I::WriteScriptCell { cell: 0, local: 1 },
                I::ReadControllerValue { local: 1 },
                I::WriteScriptCell { cell: 1, local: 1 },
                I::Wait(1),
                I::ForwardController,
            ]),
        ],
        vec![],
        2,
    )
    .with_stages(vec![
        Stage::default(),
        Stage {
            controller: Some(0),
            ..Stage::default()
        },
        Stage::default(),
        Stage {
            controller: Some(1),
            ..Stage::default()
        },
        Stage::default(),
    ])
    .unwrap();
    let budget = limits(&prepared, 2);
    let mut rt = Runtime::new(prepared, budget).unwrap();
    support::without_heap(|| {
        let domain = rt.performance(1).unwrap();
        rt.dispatch_controller(domain, address(), 0xe000, 1, 0x8000_0001)
            .unwrap();
        assert_eq!(rt.input_controller(domain, 1), Ok(0x8000_0001));
        assert_eq!(rt.input_controller(domain, 2), Ok(0));
        assert_eq!(
            rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 0),
            Ok(0)
        );
        assert_eq!(
            rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 1),
            Ok(0x8000_0001)
        );
        assert_eq!(
            rt.dispatch_controller(domain, address(), 0xe000, 1, 99),
            Err(Error::Capacity)
        );
        assert_eq!(rt.input_controller(domain, 1), Ok(0x8000_0001));
        rt.render(&mut [[0.; 2]; 2]).unwrap();
        assert_eq!(rt.controller(domain, 1), Ok(0));
        assert_eq!(rt.controller(domain, 2), Ok(0x8000_0001));
        rt.flush_behaviors(|_, _, outcome| {
            assert_eq!(outcome, Outcome::Finished);
            true
        });
    });
}

#[test]
fn deferred_forwarding_owns_downstream_capacity_and_original_projection_generation() {
    let prepared = plan(
        vec![
            program(vec![I::Wait(4), I::ForwardController]),
            program(vec![
                I::ReadControllerNumber { local: 0 },
                I::ReadInputController {
                    controller: 0,
                    local: 1,
                },
                I::WriteScriptCell { cell: 0, local: 1 },
                I::ForwardController,
            ]),
        ],
        vec![0, 1],
        1,
    );
    let budget = limits(&prepared, 2);
    let (mut rt, mut worker) = Runtime::with_plan_updates(prepared, budget, 2, 1).unwrap();
    let old = rt.active_plan();
    support::without_heap(|| {
        let domain = rt.performance(1).unwrap();
        rt.dispatch_controller(domain, address(), 1 << 15, 1, 42)
            .unwrap();
        assert_eq!(
            rt.dispatch_controller(domain, address(), 1 << 15, 1, 43),
            Err(Error::Capacity)
        );
        assert_eq!(rt.input_controller(domain, 1), Ok(42));
        assert_eq!(rt.controller(domain, 1), Ok(0));
    });
    worker
        .submit(Box::new(Prepared::new(48000, vec![], vec![], 0).unwrap()))
        .unwrap();
    support::without_heap(|| {
        rt.poll_plan_update().unwrap();
        let domain = rt.performance(1).unwrap();
        rt.dispatch_controller(domain, address(), 1 << 15, 1, 99)
            .unwrap();
        rt.render(&mut [[0.; 2]; 5]).unwrap();
        assert_eq!(rt.script_cell(old, ScriptInstanceId(0), 0), Ok(42));
        assert_eq!(rt.input_controller(domain, 1), Ok(99));
        assert_eq!(rt.controller(domain, 1), Ok(42));
        assert_eq!(rt.collect_retired_plans(), 0);
        rt.flush_behaviors(|_, owner, outcome| {
            assert_eq!(owner, BehaviorOwner::Plan(old));
            assert_eq!(outcome, Outcome::Finished);
            true
        });
        assert_eq!(rt.collect_retired_plans(), 1);
    });
    assert!(worker.retired().is_some());
}

#[test]
fn unused_stage_reservations_return_on_consumption_completion_fault_and_cancellation() {
    for mode in 0..6 {
        let code = match mode {
            0 => vec![I::SuppressController],
            1 => vec![I::End],
            2 => vec![
                I::SetLocal {
                    local: 0,
                    value: 130,
                },
                I::SetLocal { local: 1, value: 1 },
                I::WriteController {
                    controller: 0,
                    value: 1,
                },
            ],
            _ => vec![I::Wait(8), I::ForwardController],
        };
        let prepared = plan(
            vec![
                program(code),
                program(vec![I::ForwardController]),
                program(vec![]),
            ],
            vec![0, 1],
            0,
        );
        let budget = limits(&prepared, 2);
        let mut rt = Runtime::new(prepared, budget).unwrap();
        support::without_heap(|| {
            let domain = rt.performance(0).unwrap();
            let id = rt
                .dispatch_controller(domain, address(), 1 << 15, 1, 17)
                .unwrap()
                .unwrap();
            match mode {
                3 => rt.cancel_behavior(id).unwrap(),
                4 => {
                    rt.all_sound_off(address()).unwrap();
                }
                5 => rt.panic(),
                _ => {}
            }
            let expected = match mode {
                0 | 1 => Outcome::Finished,
                2 => Outcome::Fault(Error::InvalidInput),
                _ => Outcome::Cancelled,
            };
            assert_eq!(rt.behavior_outcome(id), Ok(Some(expected)));
            assert_eq!(rt.pending_commands(), 0);
            // The first outcome is deliberately unacknowledged; its unused
            // downstream reservation must still be available for unrelated work.
            rt.start_plan_behavior(rt.active_plan(), 2).unwrap();
            rt.flush_behaviors(|_, _, _| true);
            assert_eq!(rt.controller(domain, 1), Ok(0));
        });
    }
}

#[test]
fn long_controller_chains_use_bounded_dispatch_storage_instead_of_the_thread_stack() {
    const STAGES: usize = 4096;
    let prepared = plan(
        vec![program(vec![I::ForwardController])],
        vec![0; STAGES],
        0,
    );
    let budget = limits(&prepared, STAGES);
    let mut rt = Runtime::new(prepared, budget).unwrap();
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(move || {
            support::without_heap(|| {
                let domain = rt.performance(0).unwrap();
                rt.dispatch_controller(domain, address(), 1 << 15, 1, u32::MAX)
                    .unwrap();
                assert_eq!(rt.controller(domain, 1), Ok(u32::MAX));
                let mut count = 0;
                rt.flush_behaviors(|_, _, outcome| {
                    assert_eq!(outcome, Outcome::Finished);
                    count += 1;
                    true
                });
                assert_eq!(count, STAGES);
            });
        })
        .unwrap()
        .join()
        .unwrap();
}
