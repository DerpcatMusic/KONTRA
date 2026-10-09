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
        behavior_fuel: 64,
        behavior_cells: 32,
        note_cells: 0,
    }
}

fn set(local: u16, value: i64) -> Instruction {
    Instruction::SetLocal { local, value }
}
fn op(op: Op) -> Instruction {
    Instruction::Op(op)
}

#[test]
fn reals_text_calls_store_controls_and_effects_run_without_heap() {
    let knob = ControlId(7);
    let code = vec![
        // 0: r0 := 3 / 2 as reals, r1 := floor(r0 * 2) as int
        set(0, 3),
        op(Op::IntegerToReal { local: 0 }),
        set(1, real_bits(2.0)),
        op(Op::Real {
            lhs: 0,
            rhs: 1,
            operation: RealBinary::Divide,
        }),
        op(Op::Call { target: 20 }),
        // 5: text 0 := "v=" & r0 & "/" & r2
        op(Op::TextClear {
            text: TextRef::Cell(0),
        }),
        op(Op::TextAppend {
            text: TextRef::Cell(0),
            part: TextPart::Constant(0),
        }),
        op(Op::TextAppend {
            text: TextRef::Cell(0),
            part: TextPart::Real(0),
        }),
        op(Op::TextAppend {
            text: TextRef::Cell(0),
            part: TextPart::Integer(2),
        }),
        // 9: store[(1,2,0,0)] := 2; r5 := store; then dynamic control write/read
        set(3, 1),
        set(4, 2),
        op(Op::Store {
            key: 3,
            local: 2,
            write: true,
        }),
        set(2, 0),
        op(Op::Store {
            key: 3,
            local: 2,
            write: false,
        }),
        set(7, 0),
        set(8, 500),
        op(Op::ControlAt {
            index: 7,
            local: 8,
            write: true,
        }),
        op(Op::ControlAt {
            index: 7,
            local: 9,
            write: false,
        }),
        op(Op::Emit {
            service: 42,
            args: 8,
            count: 2,
            text: Some(TextRef::Cell(0)),
        }),
        Instruction::End,
        // 20: subroutine: r2 := int(r0 * 2) shifted left by 1
        op(Op::Call { target: 22 }),
        op(Op::Return),
        // 22: nested subroutine
        set(2, real_bits(2.0)),
        op(Op::Real {
            lhs: 2,
            rhs: 0,
            operation: RealBinary::Multiply,
        }),
        op(Op::RealToInteger { local: 2 }),
        set(9, 1),
        op(Op::Integer {
            lhs: 2,
            rhs: 9,
            operation: IntegerExtra::ShiftLeft,
        }),
        op(Op::Return),
    ];
    let program = Program::new(code)
        .unwrap()
        .with_texts(&["v="])
        .unwrap()
        .with_script_instance(ScriptInstanceId(0))
        .with_wait_lifetime(WaitLifetime::Callback);
    let plan = Prepared::new(48000, vec![], vec![], 0)
        .unwrap()
        .with_controls(vec![ControlDefinition {
            id: knob,
            domain: ControlDomain::Integer { min: 0, max: 100 },
            default: ControlValue::Integer(0),
        }])
        .unwrap()
        .with_script_instances(vec![vec![]])
        .unwrap()
        .with_script_resources(vec![ScriptResources {
            texts: vec![String::new()],
            text_properties: vec![],
            store: vec![],
            store_capacity: 4,
            controls: vec![Some(knob)],
        }])
        .unwrap()
        .with_programs(vec![program], None)
        .unwrap();
    let mut rt = Runtime::new(plan, limits()).unwrap();
    let plan = rt.active_plan();
    support::without_heap(|| {
        let id = rt.start_plan_behavior(plan, 0).unwrap();
        assert_eq!(rt.behavior_outcome(id), Ok(Some(Outcome::Finished)));
        assert_eq!(rt.behavior_local(id, 2), Ok(6));
        assert_eq!(rt.behavior_local(id, 9), Ok(100));
        assert_eq!(rt.control_value(plan, knob), Ok(ControlValue::Integer(100)));
        assert_eq!(
            rt.script_store(plan, ScriptInstanceId(0), [1, 2, 0, 0]),
            Ok(Some(6))
        );
        let mut seen = 0;
        rt.drain_effects(|e| {
            assert_eq!((e.service, e.args()), (42, &[500, 100][..]));
            assert_eq!(e.text.unwrap().as_str(), "v=1.56");
            seen += 1;
            true
        });
        assert_eq!(seen, 1);
    });
}

#[test]
fn programs_reject_unbacked_text_and_runaway_calls_fault() {
    let text = op(Op::TextClear {
        text: TextRef::Cell(3),
    });
    let program = Program::new(vec![text])
        .unwrap()
        .with_script_instance(ScriptInstanceId(0));
    let plan = Prepared::new(48000, vec![], vec![], 0)
        .unwrap()
        .with_script_instances(vec![vec![]])
        .unwrap();
    assert!(plan.with_programs(vec![program], None).is_err());
    assert!(
        Program::new(vec![op(Op::TextAppend {
            text: TextRef::Cell(0),
            part: TextPart::Constant(0)
        })])
        .unwrap()
        .with_texts(&[])
        .is_err()
    );

    let program = Program::new(vec![op(Op::Call { target: 0 })])
        .unwrap()
        .with_wait_lifetime(WaitLifetime::Callback);
    let plan = Prepared::new(48000, vec![], vec![], 0)
        .unwrap()
        .with_programs(vec![program], None)
        .unwrap();
    let mut rt = Runtime::new(plan, limits()).unwrap();
    let id = rt.start_plan_behavior(rt.active_plan(), 0).unwrap();
    assert_eq!(
        rt.behavior_outcome(id),
        Ok(Some(Outcome::Fault(Error::Capacity)))
    );
}

#[test]
fn borrowed_constants_and_tables_keep_utf8_truncation_and_self_append_without_heap() {
    let constant = "é".repeat(TEXT_CAPACITY / 2);
    let expected_self = format!("x{}x", "é".repeat(TEXT_CAPACITY / 2 - 1));
    let program = Program::new(vec![
        op(Op::TextAppend {
            text: TextRef::Cell(0),
            part: TextPart::Constant(0),
        }),
        op(Op::TextAppend {
            text: TextRef::Cell(0),
            part: TextPart::Constant(1),
        }),
        set(0, -1),
        op(Op::TextAppend {
            text: TextRef::Cell(0),
            part: TextPart::Table {
                base: 0,
                count: 1,
                index: 0,
            },
        }),
        op(Op::TextAppend {
            text: TextRef::Cell(1),
            part: TextPart::Constant(1),
        }),
        set(0, 0),
        op(Op::TextAppend {
            text: TextRef::Cell(1),
            part: TextPart::Table {
                base: 0,
                count: 1,
                index: 0,
            },
        }),
        op(Op::TextAppend {
            text: TextRef::Cell(1),
            part: TextPart::Text(TextRef::Cell(1)),
        }),
        Instruction::End,
    ])
    .unwrap()
    .with_texts(&[&constant, "x"])
    .unwrap()
    .with_script_instance(ScriptInstanceId(0))
    .with_wait_lifetime(WaitLifetime::Callback);
    let plan = Prepared::new(48000, vec![], vec![], 0)
        .unwrap()
        .with_script_instances(vec![vec![]])
        .unwrap()
        .with_script_resources(vec![ScriptResources {
            texts: vec![String::new(), String::new()],
            text_properties: vec![],
            store: vec![],
            store_capacity: 0,
            controls: vec![],
        }])
        .unwrap()
        .with_programs(vec![program], None)
        .unwrap();
    let mut rt = Runtime::new(plan, limits()).unwrap();
    let plan = rt.active_plan();
    support::without_heap(|| {
        let id = rt.start_plan_behavior(plan, 0).unwrap();
        assert_eq!(rt.behavior_outcome(id), Ok(Some(Outcome::Finished)));
        assert_eq!(
            rt.script_text(plan, ScriptInstanceId(0), 0)
                .unwrap()
                .as_str(),
            constant
        );
        assert_eq!(
            rt.script_text(plan, ScriptInstanceId(0), 1)
                .unwrap()
                .as_str(),
            expected_self
        );
        assert_eq!(rt.truncated_texts(), 3);
    });
}

#[test]
fn nested_subroutine_frames_survive_each_fuel_boundary_without_heap() {
    let program = Program::new(vec![
        set(0, 0),
        op(Op::Call { target: 4 }),
        Instruction::End,
        Instruction::End,
        op(Op::Call { target: 7 }),
        op(Op::Return),
        Instruction::End,
        Instruction::AddLocal { local: 0, value: 1 },
        op(Op::Return),
    ])
    .unwrap()
    .with_wait_lifetime(WaitLifetime::Callback);
    let plan = Prepared::new(48000, vec![], vec![], 0)
        .unwrap()
        .with_programs(vec![program], None)
        .unwrap();
    let mut limits = limits();
    limits.behavior_fuel = 2;
    let mut rt = Runtime::new(plan, limits).unwrap();
    support::without_heap(|| {
        let id = rt.start_plan_behavior(rt.active_plan(), 0).unwrap();
        assert_eq!(rt.behavior_outcome(id), Ok(None));
        assert_eq!(rt.behavior_local(id, 0), Ok(0));
        rt.render(&mut [[0.; 2]; 1]).unwrap();
        assert_eq!(rt.behavior_outcome(id), Ok(None));
        assert_eq!(rt.behavior_local(id, 0), Ok(1));
        rt.render(&mut [[0.; 2]; 1]).unwrap();
        assert_eq!(rt.behavior_outcome(id), Ok(None));
        rt.render(&mut [[0.; 2]; 1]).unwrap();
        assert_eq!(rt.behavior_outcome(id), Ok(Some(Outcome::Finished)));
        assert_eq!(rt.behavior_local(id, 0), Ok(1));
        assert_eq!(rt.preemptions(), 3);
    });
}
