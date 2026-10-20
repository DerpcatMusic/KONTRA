use sampler_core::*;
mod support;

fn program(code: Vec<Instruction>) -> Program {
    Program::new(code)
        .unwrap()
        .with_script_instance(ScriptInstanceId(0))
        .with_wait_lifetime(WaitLifetime::Callback)
}

fn runtime(programs: Vec<Program>, cells: Vec<i64>, callback: Option<usize>) -> Runtime {
    let mut plan = Prepared::new(48000, vec![], vec![], 0)
        .unwrap()
        .with_script_instances(vec![cells])
        .unwrap()
        .with_programs(programs, None)
        .unwrap();
    if let Some(program) = callback {
        plan = plan
            .with_signal_programs(vec![SignalProgram {
                signal: MIDI_ASYNC_SIGNAL,
                program,
                stage: 0,
            }])
            .unwrap();
    }
    let mut limits = Limits::for_plan(&plan, 4, 0);
    limits.behaviors = 8;
    limits.behavior_cells = 64;
    limits.commands = 8;
    Runtime::new(plan, limits).unwrap()
}

fn reset(local: u16) -> Instruction {
    Instruction::Op(Op::Midi {
        action: MidiAction::Reset,
        args: 0,
        local,
        text: None,
    })
}

fn take_effect(runtime: &mut Runtime) -> Effect {
    let mut effect = None;
    runtime.drain_effects(|item| {
        assert!(effect.is_none());
        effect = Some(*item);
        true
    });
    effect.expect("one admitted MIDI job")
}

#[test]
fn stopped_async_wait_cannot_interrupt_the_callbacks_next_timed_wait() {
    let mut rt = runtime(
        vec![
            program(vec![
                Instruction::ReadCallbackId { local: 0 },
                Instruction::WriteScriptCell { cell: 0, local: 0 },
                reset(1),
                Instruction::Op(Op::WaitMidi { local: 1 }),
                Instruction::ReadCallbackId { local: 2 },
                Instruction::Wait(10),
                Instruction::SetLocal { local: 3, value: 1 },
                Instruction::End,
            ]),
            program(vec![
                Instruction::ReadScriptCell { cell: 0, local: 0 },
                Instruction::SetLocal { local: 1, value: 0 },
                Instruction::StopWait {
                    callback: 0,
                    disable: 1,
                },
                Instruction::End,
            ]),
            program(vec![
                reset(0),
                Instruction::Op(Op::WaitMidi { local: 0 }),
                Instruction::SetLocal { local: 1, value: 1 },
                Instruction::End,
            ]),
        ],
        vec![0],
        None,
    );
    let plan = rt.active_plan();
    let first = rt.start_plan_behavior(plan, 0).unwrap();
    let first_effect = take_effect(&mut rt);
    let second = rt.start_plan_behavior(plan, 2).unwrap();
    let second_effect = take_effect(&mut rt);
    assert_ne!(first_effect.args[1], second_effect.args[1]);
    let mut first_completion =
        MidiCompletion::empty(first_effect.args[1] as i32, ScriptInstanceId(0));
    let mut second_completion =
        MidiCompletion::empty(second_effect.args[1] as i32, ScriptInstanceId(0));
    support::without_heap(|| {
        rt.start_plan_behavior(plan, 1).unwrap();
        assert_eq!(rt.behavior_local(first, 0), rt.behavior_local(first, 2));
        assert_eq!(rt.pending_commands(), 1);

        // Another job only releases its own waiter.
        rt.complete_midi(plan, &mut second_completion).unwrap();
        assert_eq!(rt.behavior_outcome(second), Ok(Some(Outcome::Finished)));
        assert_eq!(rt.behavior_outcome(first), Ok(None));

        // The cancelled job still completes, but no longer owns this wait.
        rt.complete_midi(plan, &mut first_completion).unwrap();
        assert_eq!(rt.behavior_outcome(first), Ok(None));
        assert_eq!(rt.behavior_local(first, 3), Ok(0));
        assert_eq!(rt.pending_commands(), 1);
        assert_eq!(
            rt.complete_midi(plan, &mut first_completion),
            Err(Error::StaleHandle)
        );

        rt.render(&mut [[0.; 2]; 9]).unwrap();
        assert_eq!(rt.behavior_local(first, 3), Ok(0));
        rt.render(&mut [[0.; 2]; 2]).unwrap();
        assert_eq!(rt.behavior_local(first, 3), Ok(1));
        assert_eq!(rt.behavior_outcome(first), Ok(Some(Outcome::Finished)));
        assert_eq!(rt.pending_commands(), 0);
        let mut first_finished = 0;
        rt.flush_behaviors(|id, owner, outcome| {
            if id == first {
                assert_eq!(owner, BehaviorOwner::Plan(plan));
                assert_eq!(outcome, Outcome::Finished);
                first_finished += 1;
            }
            true
        });
        assert_eq!(first_finished, 1);
        rt.render(&mut [[0.; 2]; 20]).unwrap();
        rt.flush_behaviors(|_, _, _| panic!("completed callback resumed twice"));
        assert_eq!(rt.take_fault(), None);
    });
}

#[test]
fn overlapping_async_callbacks_retain_their_own_id_and_status_across_waits() {
    let array = |offset| ScriptArray { offset, len: 3 };
    let mut rt = runtime(
        vec![
            program(vec![reset(0), Instruction::End]),
            program(vec![
                Instruction::Op(Op::ReadHost { local: 0, slot: 6 }),
                Instruction::Op(Op::ReadHost { local: 1, slot: 7 }),
                Instruction::WriteScriptArray {
                    array: array(0),
                    index: 0,
                    local: 1,
                },
                Instruction::Wait(10),
                Instruction::Op(Op::ReadHost { local: 2, slot: 6 }),
                Instruction::Op(Op::ReadHost { local: 3, slot: 7 }),
                Instruction::WriteScriptArray {
                    array: array(3),
                    index: 0,
                    local: 2,
                },
                Instruction::WriteScriptArray {
                    array: array(6),
                    index: 0,
                    local: 3,
                },
                Instruction::End,
            ]),
        ],
        vec![-1; 9],
        Some(1),
    );
    let plan = rt.active_plan();
    rt.start_plan_behavior(plan, 0).unwrap();
    let first = take_effect(&mut rt);
    rt.start_plan_behavior(plan, 0).unwrap();
    let second = take_effect(&mut rt);
    assert_eq!((first.args[1], second.args[1]), (1, 2));
    let mut first_completion = MidiCompletion::empty(1, ScriptInstanceId(0));
    let mut second_completion = MidiCompletion::empty(2, ScriptInstanceId(0));
    second_completion.success = false;
    support::without_heap(|| {
        rt.complete_midi(plan, &mut first_completion).unwrap();
        rt.render(&mut [[0.; 2]; 2]).unwrap();
        rt.complete_midi(plan, &mut second_completion).unwrap();
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 1), Ok(1));
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 2), Ok(0));
        rt.render(&mut [[0.; 2]; 9]).unwrap();
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 4), Ok(1));
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 7), Ok(1));
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 5), Ok(-1));
        rt.render(&mut [[0.; 2]; 2]).unwrap();
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 5), Ok(2));
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 8), Ok(0));
        assert_eq!(rt.pending_commands(), 0);
        assert_eq!(rt.take_fault(), None);
    });
}
