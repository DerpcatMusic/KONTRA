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
