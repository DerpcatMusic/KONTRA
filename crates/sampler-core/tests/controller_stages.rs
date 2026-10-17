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
fn delayed_poly_pressure_captures_mapped_physical_recipients_without_heap() {
    let prepared = Prepared::new(48000, vec![], vec![], 0).unwrap()
        .with_programs(vec![program(vec![I::Wait(4), I::ForwardController])], None).unwrap()
        .with_stages(vec![Stage { poly_at: Some(0), ..Stage::default() }]).unwrap();
    let budget = Limits { notes: 3, expressions: 3, behaviors: 2, commands: 2,
        behavior_cells: prepared.behavior_local_count() * 2, behavior_fuel: 64,
        ..Limits::for_plan(&prepared, 3, 0) };
    let mut rt = Runtime::new(prepared, budget).unwrap();
    support::without_heap(|| {
        let wire = Input { protocol: Protocol::Midi1, port: 0, group: 0, channel: 0, key: 60, external_id: Some(1) };
        let first = rt.note_on(wire, 60, 1.).unwrap();
        let other = rt.note_on(Input { external_id: Some(2), ..wire }, 60, 1.).unwrap();
        let physical = Input { protocol: Protocol::Clap, port: 2, channel: 9, external_id: Some(101), ..wire };
        rt.set_input_source(first, physical).unwrap();
        rt.set_input_source(other, Input { external_id: Some(102), ..physical }).unwrap();
        let selector = InputSelector { protocol: Some(Protocol::Clap), port: Some(2),
            group: Some(0), channel: Some(9), key: Some(60), external_id: Some(101) };
        let domain = rt.performance(0).unwrap();
        rt.dispatch_poly_pressure_selected(domain, wire.channel_address(), 60, 345, Some(selector)).unwrap();
        let pressure = |rt: &Runtime, note| rt.expression(rt.expression_id(note).unwrap()).unwrap().pressure;
        assert_eq!(pressure(&rt, first), 0);
        // A later input with the same mapped wire must not broaden the pending event.
        rt.dispatch_poly_pressure_selected(domain, wire.channel_address(), 60, 789,
            Some(InputSelector { external_id: Some(102), ..selector })).unwrap();
        rt.render(&mut [[0.; 2]; 5]).unwrap();
        assert_eq!(pressure(&rt, first), 345);
        assert_eq!(pressure(&rt, other), 789);
        rt.flush_behaviors(|_, _, outcome| { assert_eq!(outcome, Outcome::Finished); true });
    });
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
        assert_eq!(rt.channel_input_controller(domain, address(), 1), Ok(0x8000_0001));
        for cc in [2, 3] {
            assert_eq!(rt.channel_input_controller(domain, address(), cc), Ok(0));
        }
        assert_eq!(rt.channel_controller(domain, address(), 1), Ok(0));
        assert_eq!(rt.channel_controller(domain, address(), 2), Ok(0));
        assert_eq!(rt.channel_controller(domain, address(), 3), Ok(0x8000_0003));
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
        assert_eq!(rt.channel_controller(domain, address(), 3), Ok(19));
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
        assert_eq!(rt.channel_input_controller(domain, address(), 1), Ok(0x8000_0001));
        assert_eq!(rt.channel_input_controller(domain, address(), 2), Ok(0));
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
        assert_eq!(rt.channel_input_controller(domain, address(), 1), Ok(0x8000_0001));
        rt.render(&mut [[0.; 2]; 2]).unwrap();
        assert_eq!(rt.channel_controller(domain, address(), 1), Ok(0));
        assert_eq!(rt.channel_controller(domain, address(), 2), Ok(0x8000_0001));
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
        assert_eq!(rt.channel_input_controller(domain, address(), 1), Ok(42));
        assert_eq!(rt.channel_controller(domain, address(), 1), Ok(0));
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
        assert_eq!(rt.channel_input_controller(domain, address(), 1), Ok(99));
        assert_eq!(rt.channel_controller(domain, address(), 1), Ok(42));
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
            assert_eq!(rt.channel_controller(domain, address(), 1), Ok(0));
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
                assert_eq!(rt.channel_controller(domain, address(), 1), Ok(u32::MAX));
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


// Callback epochs are captured at callback creation, not immutable note admission.
fn reset_epoch_plan() -> Prepared {
    let write=|cc,value| vec![I::SetLocal{local:0,value:cc},I::SetLocal{local:1,value},I::WriteController{controller:0,value:1}];
    let mut attack=vec![I::StartProgram{program:1},I::Wait(4)];attack.extend(write(1,111));
    let mut old_fork=vec![I::Wait(4)];old_fork.extend(write(11,222));
    let mut release=write(1,333);release.push(I::StartProgram{program:3});
    let mut current_fork=vec![I::Wait(1)];current_fork.extend(write(11,444));
    plan(vec![program(attack),program(old_fork),program(release),program(current_fork),
        program(vec![I::ReadScriptCell{local:0,cell:0},I::AddLocal{local:0,value:1},I::WriteScriptCell{cell:0,local:0},I::ForwardController])],vec![],1)
        .with_stages(vec![Stage{note:Some(0),release:Some(2),..Stage::default()},Stage{controller:Some(4),..Stage::default()}]).unwrap()
}

#[test]
fn selective_reset_expires_old_wait_and_fork_but_fresh_release_and_fork_use_current_epochs() {
    let prepared=reset_epoch_plan();let budget=Limits{notes:4,expressions:4,..limits(&prepared,32)};
    let mut rt=Runtime::new(prepared,budget).unwrap();
    support::without_heap(|| {
        let own=address();let other=ChannelAddress{port:own.port+1,..own};
        let input=|address:ChannelAddress,key| Input{protocol:address.protocol,port:address.port,group:address.group,channel:address.channel,key,external_id:None};
        let note=rt.trigger(input(own,60),60,1.).unwrap();let sibling=rt.trigger(input(other,61),61,1.).unwrap();
        let performance=rt.performance(0).unwrap();
        let scope=ChannelScope{protocol:own.protocol,port:own.port,group:own.group,channels:1<<own.channel};
        rt.reset_channel_controllers(performance,scope).unwrap();
        assert_eq!(rt.script_cell(rt.active_plan(),ScriptInstanceId(0),0),Ok(0),"selective reset invents no six CC callbacks");
        rt.render(&mut [[0.;2];6]).unwrap();
        assert_eq!(rt.channel_controller(performance,own,1),Ok(0));
        assert_eq!(rt.channel_controller(performance,own,11),Ok(u32::MAX));
        assert_eq!(rt.channel_controller(performance,other,1),Ok(111));
        assert_eq!(rt.channel_controller(performance,other,11),Ok(222));
        assert_eq!(rt.script_cell(rt.active_plan(),ScriptInstanceId(0),0),Ok(2));
        rt.note_off_exact(note,None).unwrap();rt.render(&mut [[0.;2];2]).unwrap();
        assert_eq!(rt.channel_controller(performance,own,1),Ok(333),"fresh onRelease is not tied to stale note admission epoch");
        assert_eq!(rt.channel_controller(performance,own,11),Ok(444),"fresh note callback's plan fork carries current physical epoch");
        assert_eq!(rt.script_cell(rt.active_plan(),ScriptInstanceId(0),0),Ok(4));
        assert!(rt.input_held(sibling).unwrap());
        assert_eq!(rt.controller(performance,1),Ok(0),"captured callbacks never become global");
    });
}

#[test]
fn hard_sound_off_cancels_captured_plan_forks_without_touching_other_source_domains() {
    let prepared=reset_epoch_plan();let budget=Limits{notes:4,expressions:4,..limits(&prepared,32)};
    let mut rt=Runtime::new(prepared,budget).unwrap();
    support::without_heap(|| {
        let own=address();let other=ChannelAddress{group:own.group+1,..own};
        let input=|address:ChannelAddress,key|Input{protocol:address.protocol,port:address.port,group:address.group,channel:address.channel,key,external_id:None};
        rt.trigger(input(own,60),60,1.).unwrap();let sibling=rt.trigger(input(other,61),61,1.).unwrap();
        rt.all_sound_off(own).unwrap();rt.render(&mut [[0.;2];6]).unwrap();
        let performance=rt.performance(0).unwrap();
        assert_eq!(rt.channel_controller(performance,own,1),Ok(0));assert_eq!(rt.channel_controller(performance,own,11),Ok(u32::MAX));
        assert_eq!(rt.channel_controller(performance,other,1),Ok(111));assert_eq!(rt.channel_controller(performance,other,11),Ok(222));
        assert_eq!(rt.script_cell(rt.active_plan(),ScriptInstanceId(0),0),Ok(2));assert!(rt.input_held(sibling).unwrap());
    });
}



#[test]
fn hard_sound_off_cancels_note_parameter_fanout_but_preserves_sibling_and_uncaptured_plan_work() {
    let send=program(vec![I::SetLocal{local:0,value:42},I::SetLocal{local:1,value:7},
        I::SendParameter{kind:ParameterKind::Rpn,address:0,value:1}])
        .with_script_instance(ScriptInstanceId(0));
    let captured=program(vec![I::Wait(4),I::SetLocal{local:0,value:1},
        I::SetLocal{local:1,value:777},I::WriteController{controller:0,value:1}])
        .with_script_instance(ScriptInstanceId(1));
    // Ordinary startup Control has epochs=None even when its origin happens
    // to equal the hard-killed physical address. Its global write must survive.
    let startup=program(vec![I::Wait(4),I::SetLocal{local:0,value:2},
        I::SetLocal{local:1,value:888},I::WriteController{controller:0,value:1}])
        .with_script_instance(ScriptInstanceId(0));
    let bare=program(vec![I::Wait(4),I::SetLocal{local:0,value:999},
        I::WriteScriptCell{cell:0,local:0}])
        .with_script_instance(ScriptInstanceId(0));
    let prepared=Prepared::new(48000,vec![],vec![],0).unwrap()
        .with_script_instances(vec![vec![0],vec![]]).unwrap()
        .with_programs(vec![send,captured,startup,bare],None).unwrap()
        .with_stages(vec![Stage{note:Some(0),..Stage::default()},Stage::default()]).unwrap()
        .with_parameter_programs(vec![ParameterProgram{kind:ParameterKind::Rpn,program:1,stage:1}]).unwrap()
        .with_plan_programs(vec![PlanProgram{program:2,stage:0}]).unwrap();
    let budget=Limits{notes:4,expressions:4,..limits(&prepared,32)};
    let mut rt=Runtime::new(prepared,budget).unwrap();
    support::without_heap(|| {
        let own=ChannelAddress{protocol:Protocol::Native,port:0,group:0,channel:0};
        let other=ChannelAddress{protocol:Protocol::Midi2,..own};
        let input=|address:ChannelAddress|Input{protocol:address.protocol,port:address.port,
            group:address.group,channel:address.channel,key:60,external_id:None};
        rt.start_plan_behavior(rt.active_plan(),3).unwrap();
        rt.trigger(input(own),60,1.).unwrap();
        let sibling=rt.trigger(input(other),60,1.).unwrap();
        rt.render(&mut []).unwrap();
        assert_eq!(rt.now(),0);
        rt.all_sound_off(own).unwrap();
        rt.render(&mut [[0.;2];6]).unwrap();
        let performance=rt.performance(0).unwrap();
        assert_eq!(rt.channel_controller(performance,own,1),Ok(0),
            "captured parameter receiver must not resurrect a hard-killed address");
        assert_eq!(rt.channel_controller(performance,other,1),Ok(777),
            "same port/group/channel/key in a different protocol remains independent");
        assert_eq!(rt.controller(performance,1),Ok(0));
        assert_eq!(rt.controller(performance,2),Ok(888),
            "uncaptured startup Control must not be canceled by coincident origin");
        assert_eq!(rt.script_cell(rt.active_plan(),ScriptInstanceId(0),0),Ok(999),
            "genuine Bare plan work remains global and live");
        assert!(rt.input_held(sibling).unwrap());
        assert_eq!(rt.take_fault(),None);
    });
}

#[test]
fn selective_reset_preflights_primary_secondary_union_without_publishing_rows_or_reviving_waits() {
    let prepared=plan(vec![program(vec![I::Wait(4),I::ForwardController])],vec![0],0);
    let budget=Limits{performances:1,..limits(&prepared,64)};let mut rt=Runtime::new(prepared,budget).unwrap();
    support::without_heap(|| {
        let performance=rt.performance(0).unwrap();
        for port in 0..47u16 {let source=ChannelAddress{port,..address()};rt.dispatch_controller(performance,source,1<<source.channel,1,u32::from(port)+1).unwrap();}
        let first=ChannelAddress{port:100,..address()};let second=ChannelAddress{protocol:Protocol::Native,port:101,..address()};
        let scope=|source:ChannelAddress|ChannelScope{protocol:source.protocol,port:source.port,group:source.group,channels:1<<source.channel};
        assert_eq!(rt.reset_protocol_controller_targets(performance,scope(first),Some(scope(second))),Err(Error::Capacity));
        assert_eq!(rt.now(),0);assert_eq!(rt.channel_input_controller(performance,first,1),Ok(0));
        rt.render(&mut [[0.;2];5]).unwrap();
        for port in 0..47u16 {assert_eq!(rt.channel_controller(performance,ChannelAddress{port,..address()},1),Ok(u32::from(port)+1),"rejected reset cannot invalidate waiting epochs");}
        rt.flush_behaviors(|_,_,_|true);
        rt.dispatch_controller(performance,first,1<<first.channel,1,999).unwrap();
        assert_eq!(rt.dispatch_controller(performance,second,1<<second.channel,1,1000),Err(Error::Capacity),"exactly one row remained; failed union consumed none");
        rt.render(&mut [[0.;2];5]).unwrap();assert_eq!(rt.channel_controller(performance,first,1),Ok(999));
    });
}
