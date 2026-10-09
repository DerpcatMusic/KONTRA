use sampler_core::*;
#[path = "support/mod.rs"]
mod support;

#[test]
fn persistence_revision_tracks_fast_and_general_writes_but_not_reads_without_heap() {
    let array = ScriptArray { offset: 0, len: 2 };
    let programs = [
        vec![
            Instruction::ReadScriptCell { cell: 0, local: 0 },
            Instruction::SetLocal { local: 1, value: 1 },
            Instruction::ReadScriptArray {
                array,
                index: 1,
                local: 0,
            },
            Instruction::Op(Op::TextIndex {
                text: TextRef::Cell(0),
                local: 0,
            }),
        ],
        vec![
            Instruction::SetLocal {
                local: 0,
                value: 42,
            },
            Instruction::WriteScriptCell { cell: 0, local: 0 },
            Instruction::SetLocal { local: 1, value: 1 },
            Instruction::WriteScriptArray {
                array,
                index: 1,
                local: 0,
            },
        ],
        vec![Instruction::ReadEventIds { array }],
        vec![
            Instruction::Op(Op::TextClear {
                text: TextRef::Cell(0),
            }),
            Instruction::Op(Op::TextAppend {
                text: TextRef::Cell(0),
                part: TextPart::Constant(0),
            }),
            Instruction::SetLocal { local: 0, value: 7 },
            Instruction::Op(Op::TextAppend {
                text: TextRef::Cell(0),
                part: TextPart::Integer(0),
            }),
        ],
    ]
    .into_iter()
    .map(|code| {
        Program::new(code)
            .unwrap()
            .with_texts(&["value="])
            .unwrap()
            .with_script_instance(ScriptInstanceId(0))
            .with_wait_lifetime(WaitLifetime::Callback)
    })
    .collect();
    let prepared = Prepared::new(48000, vec![], vec![], 0)
        .unwrap()
        .with_script_instances(vec![vec![17, 18]])
        .unwrap()
        .with_script_resources(vec![ScriptResources {
            texts: vec!["initial".into()],
            ..Default::default()
        }])
        .unwrap()
        .with_programs(programs, None)
        .unwrap();
    let mut runtime = Runtime::new(
        prepared,
        Limits {
            notes: 1,
            channels: 0,
            performances: 1,
            families: 0,
            expressions: 1,
            voices: 0,
            decisions: 0,
            commands: 1,
            behaviors: 1,
            behavior_fuel: 32,
            behavior_cells: 2,
            note_cells: 0,
        },
    )
    .unwrap();
    let plan = runtime.active_plan();
    support::without_heap(|| {
        let initial = runtime.script_state_revision(plan).unwrap();
        for program in 0..4 {
            let before = runtime.script_state_revision(plan).unwrap();
            let id = runtime.start_plan_behavior(plan, program).unwrap();
            assert_eq!(runtime.behavior_outcome(id), Ok(Some(Outcome::Finished)));
            let after = runtime.script_state_revision(plan).unwrap();
            assert_eq!(
                after.0, initial.0,
                "script writes do not change control revision"
            );
            if program == 0 {
                assert_eq!(after, before);
            } else {
                assert_ne!(after, before);
            }
            runtime.flush_behaviors(|_, _, _| true);
        }
        assert_eq!(runtime.script_cell(plan, ScriptInstanceId(0), 0), Ok(0));
        assert_eq!(runtime.script_cell(plan, ScriptInstanceId(0), 1), Ok(42));
        assert_eq!(
            runtime
                .script_text(plan, ScriptInstanceId(0), 0)
                .unwrap()
                .as_str(),
            "value=7"
        );
    });
}

#[test]
fn registered_snapshot_tracks_only_captured_cells_in_fast_and_general_paths() {
    let programs = vec![
        vec![Instruction::SetLocal { local: 0, value: 42 }, Instruction::WriteScriptCell { cell: 1, local: 0 }],
        vec![Instruction::SetLocal { local: 0, value: 42 }, Instruction::WriteScriptCell { cell: 70, local: 0 }],
        vec![Instruction::SetLocal { local: 0, value: 0 }, Instruction::SetLocal { local: 1, value: 42 },
             Instruction::WriteScriptArray { array: ScriptArray { offset: 0, len: 1 }, index: 0, local: 1 }],
        vec![Instruction::ReadEventIds { array: ScriptArray { offset: 1, len: 1 } }],
        vec![Instruction::ReadEventIds { array: ScriptArray { offset: 70, len: 1 } }],
    ].into_iter().map(|code| Program::new(code).unwrap()
        .with_script_instance(ScriptInstanceId(0)).with_wait_lifetime(WaitLifetime::Callback)).collect();
    let prepared = Prepared::new(48000, vec![], vec![], 0).unwrap()
        .with_script_instances(vec![vec![17; 72]]).unwrap().with_programs(programs, None).unwrap();
    let limits = Limits::for_plan(&prepared, 128, 8);
    let mut runtime = Runtime::new(prepared, limits).unwrap();
    let plan = runtime.active_plan();
    let watched = [0, 70].map(|index| ScriptStateEntry {
        address: ScriptStateAddress::Cell { instance: ScriptInstanceId(0), index },
        value: ScriptStateValue::Cell(0),
    });
    runtime.watch_script_state_addresses(plan, &watched.map(|entry| entry.address)).unwrap();
    let before = runtime.script_state_revision(plan).unwrap();
    assert_eq!(runtime.watch_script_state_values(plan, &[ScriptStateEntry {
        address: ScriptStateAddress::Cell { instance: ScriptInstanceId(0), index: 72 },
        value: ScriptStateValue::Cell(0),
    }]), Err(Error::InvalidInput));
    assert_eq!(runtime.script_state_revision(plan).unwrap(), before);
    support::without_heap(|| {
        for (program, changes) in [(0,false), (1,true), (1,false), (2,true), (2,false), (3,false), (4,true), (4,false)] {
            let before = runtime.script_state_revision(plan).unwrap();
            let id = runtime.start_plan_behavior(plan, program).unwrap();
            assert_eq!(runtime.behavior_outcome(id), Ok(Some(Outcome::Finished)));
            assert_eq!(runtime.script_state_revision(plan).unwrap() != before, changes);
            let mut addresses = [None; 2];
            let mut count = 0;
            assert!(runtime.visit_dirty_script_cells(plan, |address| {
                addresses[count] = Some(address); count += 1;
            }).unwrap());
            assert_eq!(count, usize::from(changes));
            if changes {
                let index = if program == 2 { 0 } else { 70 };
                assert_eq!(addresses[0], Some(ScriptStateAddress::Cell { instance: ScriptInstanceId(0), index }));
            }
            runtime.clear_dirty_script_cells(plan).unwrap();
            assert!(runtime.visit_dirty_script_cells(plan, |_| panic!("dirty cell was not cleared")).unwrap());
            runtime.flush_behaviors(|_, _, _| true);
        }
    });
}
