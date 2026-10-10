//! Narrow Op admission and legacy Store requirements. Authored NOT_RUN.
use sampler_core::*;
use sampler_core::waveform::{Action, Property, attachment_key, initial, source_key, symbol_key};
mod support;
const UI: i32 = 32768;
const SYMBOL: i32 = 0x0200_0000;
fn set(local: u16, value: i64) -> Instruction { Instruction::SetLocal { local, value } }
fn resources() -> ScriptResources {
    let mut store = initial(UI).to_vec();
    store[0].1 = 27;
    store.extend([(source_key(27),1),(source_key(91),1),(symbol_key(SYMBOL),Property::Table as i64)]);
    ScriptResources { store_capacity:store.len(), store, ..Default::default() }
}
fn runtime(code: Vec<Instruction>, resources: ScriptResources) -> Runtime {
    let program = Program::new(code).unwrap().with_script_instance(ScriptInstanceId(0))
        .with_wait_lifetime(WaitLifetime::Callback);
    let plan = Prepared::new(48000,vec![],vec![],1).unwrap()
        .with_script_instances(vec![vec![]]).unwrap()
        .with_script_resources(vec![resources]).unwrap()
        .with_programs(vec![program],None).unwrap();
    let mut limits = Limits::for_plan(&plan,1,1);
    limits.behaviors = 2; limits.behavior_cells = plan.behavior_local_count()*2;
    Runtime::new(plan,limits).unwrap()
}
fn operation(action: Action, ui: i32, selector: i32, index: i32, value: i32) -> Vec<Instruction> {
    vec![set(0,i64::from(ui)),set(1,i64::from(selector)),set(2,i64::from(index)),set(3,i64::from(value)),
        Instruction::Op(Op::Waveform { action,args:0,local:4,services:[7,8] }),Instruction::End]
}

#[test]
fn waveform_checked_store_capacity_rejection_is_explicit_and_all_or_none_without_heap() {
    let mut rt = runtime(operation(Action::Set,UI,SYMBOL,3,77),resources()); let plan=rt.active_plan();
    support::without_heap(|| {
        let b = rt.start_plan_behavior(plan,0).unwrap();
        assert_eq!(rt.behavior_outcome(b),Ok(Some(Outcome::Fault(Error::Capacity))));
        assert_eq!(rt.take_fault().map(|(_,e)|e),Some(Error::Capacity));
        assert_eq!(rt.script_store(plan,ScriptInstanceId(0),attachment_key(UI)),Ok(Some(27)));
        assert_eq!(rt.script_store(plan,ScriptInstanceId(0),Property::Table.key(UI,3)),Ok(None));
        let mut effects=0; rt.drain_effects(|_| { effects+=1; true }); assert_eq!(effects,0);
    });
}

#[test]
fn waveform_reset_missing_preseed_rejects_without_clearing_existing_table() {
    let mut r=resources();
    r.store.retain(|(key,_)| *key != Property::Highlight.key(UI,0));
    r.store.push((Property::Table.key(UI,3),77)); r.store_capacity=r.store.len();
    let mut rt=runtime(operation(Action::Attach,UI,91,11,0),r); let plan=rt.active_plan();
    support::without_heap(|| {
        rt.start_plan_behavior(plan,0).unwrap();
        assert_eq!(rt.take_fault().map(|(_,e)|e),Some(Error::InvalidInput));
        assert_eq!(rt.script_store(plan,ScriptInstanceId(0),attachment_key(UI)),Ok(Some(27)));
        assert_eq!(rt.script_store(plan,ScriptInstanceId(0),Property::Table.key(UI,3)),Ok(Some(77)));
        let mut effects=0; rt.drain_effects(|_| { effects+=1; true }); assert_eq!(effects,0);
    });
}

#[test]
fn waveform_seam_does_not_change_legacy_store_missing_read_or_dropped_write_behavior() {
    let code=vec![set(0,1),set(1,2),set(2,3),set(3,4),set(4,77),
        Instruction::Op(Op::Store { key:0,local:4,write:true }),set(4,1234),
        Instruction::Op(Op::Store { key:0,local:4,write:false }),Instruction::End];
    let mut rt=runtime(code,ScriptResources { store_capacity:0,..Default::default() }); let plan=rt.active_plan();
    support::without_heap(|| {
        let b=rt.start_plan_behavior(plan,0).unwrap();
        assert_eq!(rt.behavior_outcome(b),Ok(Some(Outcome::Finished)));
        assert_eq!(rt.behavior_local(b,4),Ok(1234));
        assert_eq!(rt.script_store(plan,ScriptInstanceId(0),[1,2,3,4]),Ok(None));
        assert!(rt.take_fault().is_none());
    });
}

#[test]
fn waveform_op_keeps_existing_instruction_size_budget() {
    assert!(std::mem::size_of::<Instruction>() <= 32);
}

// Enumerate every prepared Store entry, plus the only possible new write.
// Setup and the owned expected snapshot are outside the guarded audio scope.
fn assert_store(rt: &Runtime, plan: PlanId, before: &[([i32; 4], i64)]) {
    for &(key, value) in before {
        assert_eq!(rt.script_store(plan, ScriptInstanceId(0), key), Ok(Some(value)));
    }
    assert_eq!(rt.script_store(plan, ScriptInstanceId(0), Property::Table.key(UI, 9)), Ok(None));
}

#[test]
fn waveform_wide_nonpositive_and_unattached_sources_reject_get_set_with_full_store_unchanged() {
    for zone in [(1_i64 << 32) + 27, i64::MAX, 0, -27, -1] {
        for action in [Action::Get, Action::Set] {
            let mut r = resources();
            r.store[0].1 = zone;
            // Zero/negative markers must not make these identities admissible.
            r.store.extend([(source_key(0), 1), (source_key(-27), 1),
                (source_key(-1), 1), (Property::Table.key(UI, 3), 77)]);
            r.store_capacity = r.store.len();
            let before = r.store.clone();
            let mut code = operation(action, UI, SYMBOL, 9, 99);
            code.insert(4, set(4, 12345));
            let mut rt = runtime(code, r); let plan = rt.active_plan();
            support::without_heap(|| {
                let b = rt.start_plan_behavior(plan, 0).unwrap();
                assert_eq!(rt.behavior_outcome(b), Ok(Some(Outcome::Fault(Error::InvalidInput))));
                assert_eq!(rt.take_fault().map(|(_, e)| e), Some(Error::InvalidInput));
                assert_eq!(rt.behavior_local(b, 4), Ok(12345));
                assert_store(&rt, plan, &before);
                let mut effects = 0; rt.drain_effects(|_| { effects += 1; true });
                assert_eq!(effects, 0);
            });
        }
    }
}

#[test]
fn waveform_exact_positive_i32_source_boundary_and_unattached_atomic_reset_are_admitted() {
    for (initial_zone, action) in [(-1, Action::Attach),
        (i64::from(i32::MAX), Action::Get), (i64::from(i32::MAX), Action::Set)] {
        let mut r = resources();
        r.store[0].1 = initial_zone;
        r.store.extend([(source_key(i32::MAX), 1), (Property::Table.key(UI, 3), 77)]);
        r.store_capacity = r.store.len();
        let code = if initial_zone == -1 {
            operation(Action::Attach, UI, i32::MAX, 11, 0)
        } else {
            operation(action, UI, SYMBOL, 3, 99)
        };
        let mut rt = runtime(code, r); let plan = rt.active_plan();
        support::without_heap(|| {
            let b = rt.start_plan_behavior(plan, 0).unwrap();
            assert_eq!(rt.behavior_outcome(b), Ok(Some(Outcome::Finished)));
            assert!(rt.take_fault().is_none());
            assert_eq!(rt.script_store(plan, ScriptInstanceId(0), attachment_key(UI)),
                Ok(Some(i64::from(i32::MAX))));
            if initial_zone == -1 {
                for (key, default) in initial(UI) {
                    let expected = if key == attachment_key(UI) { i64::from(i32::MAX) }
                        else if key == Property::Flags.key(UI, 0) { 11 } else { default };
                    assert_eq!(rt.script_store(plan, ScriptInstanceId(0), key), Ok(Some(expected)));
                }
                assert_eq!(rt.script_store(plan, ScriptInstanceId(0), Property::Table.key(UI, 3)), Ok(None));
            } else if action == Action::Get {
                assert_eq!(rt.behavior_local(b, 4), Ok(77));
            } else {
                assert_eq!(rt.script_store(plan, ScriptInstanceId(0), Property::Table.key(UI, 3)), Ok(Some(99)));
            }
            let mut effects = 0; rt.drain_effects(|e| {
                if action == Action::Attach {
                    assert_eq!(e.args(), &[i64::from(UI), i64::from(i32::MAX), 11]);
                } else {
                    assert_eq!(e.args(), &[i64::from(UI), i64::from(SYMBOL), 3, 99]);
                }
                effects += 1; true
            });
            assert_eq!(effects, usize::from(action != Action::Get));
        });
    }
}

#[test]
fn waveform_full_store_existing_scalar_overwrite_is_admitted() {
    let mut r = resources();
    r.store.iter_mut().find(|(key, _)| *key == symbol_key(SYMBOL)).unwrap().1 = Property::Cursor as i64;
    let mut rt = runtime(operation(Action::Set, UI, SYMBOL, 0, 24000), r);
    let plan = rt.active_plan();
    support::without_heap(|| {
        let b = rt.start_plan_behavior(plan, 0).unwrap();
        assert_eq!(rt.behavior_outcome(b), Ok(Some(Outcome::Finished)));
        assert_eq!(rt.script_store(plan, ScriptInstanceId(0), Property::Cursor.key(UI, 0)), Ok(Some(24000)));
        assert!(rt.take_fault().is_none());
        let mut effects = 0; rt.drain_effects(|e| {
            assert_eq!(e.args(), &[i64::from(UI), i64::from(SYMBOL), 0, 24000]);
            effects += 1; true
        });
        assert_eq!(effects, 1);
    });
}

#[test]
fn waveform_all_actions_reject_overflowing_register_window_before_reads_or_mutation() {
    for (action, args) in [(Action::Set, 65533), (Action::Get, 65534), (Action::Attach, 65535)] {
        let r = resources(); let before = r.store.clone();
        // Program::locals prepares sufficient cells even beyond the u16 range.
        // The first read would fault ArithmeticOverflow if reached; expect
        // InvalidInput from the complete-window check before even that read.
        let code = vec![set(args, i64::MAX), set(4, 12345), Instruction::Op(Op::Waveform {
            action, args, local: 4, services: [7, 8],
        }), Instruction::End];
        let mut rt = runtime(code, r); let plan = rt.active_plan();
        support::without_heap(|| {
            let b = rt.start_plan_behavior(plan, 0).unwrap();
            assert_eq!(rt.behavior_outcome(b), Ok(Some(Outcome::Fault(Error::InvalidInput))));
            assert_eq!(rt.take_fault().map(|(_, e)| e), Some(Error::InvalidInput));
            assert_eq!(rt.behavior_local(b, 4), Ok(12345));
            assert_store(&rt, plan, &before);
            let mut effects = 0; rt.drain_effects(|_| { effects += 1; true });
            assert_eq!(effects, 0);
        });
    }
}

#[test]
fn waveform_all_actions_accept_last_complete_u16_register_window() {
    for (action, args) in [(Action::Set, 65532_u16), (Action::Get, 65533), (Action::Attach, 65533)] {
        let mut r = resources();
        r.store.push((Property::Table.key(UI, 3), 77)); r.store_capacity = r.store.len();
        let values = if action == Action::Attach { [UI, 91, 11, 0] } else { [UI, SYMBOL, 3, 99] };
        let count = if action == Action::Set { 4 } else { 3 };
        let mut code = Vec::new();
        for (i, value) in values.into_iter().take(count).enumerate() {
            code.push(set(args.checked_add(i as u16).unwrap(), i64::from(value)));
        }
        code.extend([Instruction::Op(Op::Waveform { action, args, local: 4, services: [7, 8] }), Instruction::End]);
        let mut rt = runtime(code, r); let plan = rt.active_plan();
        support::without_heap(|| {
            let b = rt.start_plan_behavior(plan, 0).unwrap();
            assert_eq!(rt.behavior_outcome(b), Ok(Some(Outcome::Finished)));
            assert!(rt.take_fault().is_none());
            match action {
                Action::Get => assert_eq!(rt.behavior_local(b, 4), Ok(77)),
                Action::Set => assert_eq!(rt.script_store(plan, ScriptInstanceId(0), Property::Table.key(UI, 3)), Ok(Some(99))),
                Action::Attach => {
                    assert_eq!(rt.script_store(plan, ScriptInstanceId(0), attachment_key(UI)), Ok(Some(91)));
                    assert_eq!(rt.script_store(plan, ScriptInstanceId(0), Property::Table.key(UI, 3)), Ok(None));
                }
            }
            let mut effects = 0; rt.drain_effects(|_| { effects += 1; true });
            assert_eq!(effects, usize::from(action != Action::Get));
        });
    }
}
