use kontra_engine::{Action, Engine, Plan, Text};
use kontra_stream::{Store, StoreConfig};
use kontra_vm::{HostApi, HostCall, HostValue};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;

struct Counting;
thread_local! { static COUNT: Cell<Option<usize>> = const { Cell::new(None) }; }
#[global_allocator]
static ALLOCATOR: Counting = Counting;

// SAFETY: all allocation operations are forwarded to the system allocator with
// their original arguments. The thread-local counter never allocates.
#[expect(unsafe_code, reason = "the isolated test allocator forwards to System")]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        COUNT.with(|n| n.set(n.get().map(|v| v + 1)));
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        COUNT.with(|n| n.set(n.get().map(|v| v + 1)));
        unsafe { System.realloc(ptr, layout, size) }
    }
}

fn call(e: &mut Engine, action: Action, key: Text, ints: &[i64], value: Text) -> HostValue {
    let mut out = Text::EMPTY;
    e.call(HostCall {
        action,
        context: 0,
        ints,
        texts: &[key, value],
        cells: &mut [],
        text_cells: &mut [],
        text_out: &mut out,
    })
}

#[test]
fn prepared_runtime_creations_edits_duplicates_and_refusals_allocate_nothing() {
    let store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut e = Engine::new(Arc::new(Plan::new(vec![], 0)), store.client(), 48_000);
    let keys: Vec<_> = (0..65)
        .map(|i| Text::from_str_cut(&format!("key{i}")).0)
        .collect();
    let value = Text::from_str_cut("value").0;
    let budget = e.pgs_budget();
    assert_eq!(
        (
            budget.integer_keys,
            budget.string_keys,
            budget.integer_values
        ),
        (64, 64, 64 * 256)
    );
    COUNT.with(|n| n.set(Some(0)));
    for (i, &key) in keys.iter().enumerate() {
        call(&mut e, Action::PgsCreateKey, key, &[256], value);
        call(&mut e, Action::PgsCreateStrKey, key, &[], value);
        if i < 64 {
            call(
                &mut e,
                Action::PgsSetKeyVal,
                key,
                &[255, i as i64 + 1],
                value,
            );
            call(&mut e, Action::PgsSetStrKeyVal, key, &[], value);
            assert_eq!(
                call(&mut e, Action::PgsGetKeyVal, key, &[255], value),
                HostValue::Int(i as i32 + 1)
            );
        }
    }
    call(&mut e, Action::PgsCreateKey, keys[0], &[1], value);
    call(&mut e, Action::PgsCreateStrKey, keys[0], &[], value);
    let allocations = COUNT.with(|n| n.replace(None).unwrap());
    assert_eq!(allocations, 0);
    assert_eq!(
        e.unhandled_actions(),
        vec![(Action::PgsCreateKey, 1), (Action::PgsCreateStrKey, 1)]
    );
    assert_eq!(
        call(&mut e, Action::PgsKeyExists, keys[64], &[], value),
        HostValue::Int(0)
    );
    assert_eq!(
        call(&mut e, Action::PgsGetKeyVal, keys[0], &[255], value),
        HostValue::Int(1)
    );
}

#[test]
fn note_controller_and_ui_callbacks_do_not_allocate_for_pgs() {
    use kontra_engine::{
        Callbacks, Controls, EnginePars, Event, HostNote, Op, Program, SampleTime, ScriptPlan,
        Timed, ir,
    };
    use kontra_vm::Returns;
    let store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![], 1);
    let host = |action, ints, texts| Op::Host {
        action,
        ints,
        texts,
        returns: Returns::Nothing,
    };
    let mut texts = vec![
        Text::from_str_cut("key").0,
        Text::from_str_cut("text").0,
        Text::from_str_cut("value").0,
    ];
    let mut code = Vec::new();
    for i in 0..420 {
        let name = u32::try_from(texts.len()).expect("fixture text index fits u32");
        texts.push(Text::from_str_cut(&format!("seed{i}")).0);
        code.extend([
            Op::Text(name),
            Op::Int(i % 7 + 1),
            host(Action::PgsCreateKey, 1, 1),
            Op::Text(name),
            host(Action::PgsCreateStrKey, 0, 1),
        ]);
    }
    code.push(Op::Exit);
    let entry = u32::try_from(code.len()).expect("fixture callback address fits u32");
    code.extend([
        Op::Text(0),
        Op::Int(2),
        host(Action::PgsCreateKey, 1, 1),
        Op::Text(0),
        Op::Int(1),
        Op::Int(17),
        host(Action::PgsSetKeyVal, 2, 1),
        Op::Text(1),
        host(Action::PgsCreateStrKey, 0, 1),
        Op::Text(1),
        Op::Text(2),
        host(Action::PgsSetStrKeyVal, 0, 2),
        Op::Exit,
    ]);
    plan.scripts.push(ScriptPlan {
        program: Program {
            texts,
            code,
            cells: 1,
            max_stack: 8,
            ..Program::default()
        },
        callbacks: Callbacks {
            init: Some(0),
            note: Some(entry),
            controller: Some(entry),
            ui_control: vec![(1, Some(entry))],
            ..Callbacks::default()
        },
        coroutines: 4,
        data: vec![],
        text_data: vec![],
        source_folder: None,
        library_folder: None,
        factory_folder: None,
        restore: vec![],
        restore_texts: vec![],
        controls: Controls {
            first: 1,
            cells: vec![Some(0)],
            ..Controls::default()
        },
        engine_pars: EnginePars::default(),
        links: vec![],
        reverb_laws: std::array::from_fn(|_| ir::Law::Linear { lo: 0.0, hi: 1.0 }),
        reverb_networks: vec![],
        knobs: vec![],
        foreign: None,
        files: None,
        arrays: vec![],
        hears_own_releases: false,
        held_release_ends_children: false,
    });
    let mut e = Engine::new(Arc::new(plan), store.client(), 48_000);
    let budget = e.pgs_budget();
    assert_eq!(budget.integer_keys, 484);
    assert_eq!(budget.string_keys, 484);
    let note = HostNote {
        port: 0,
        channel: 0,
        key: 60,
        id: -1,
        clap: true,
    };
    let events = [
        Timed {
            at: SampleTime(0),
            body: Event::NoteOn {
                note,
                velocity: 1.0,
                tune: 0.0,
            },
        },
        Timed {
            at: SampleTime(0),
            body: Event::midi1(0xb0, 1, 64),
        },
    ];
    let mut out = [[0.0; 2]; 7];
    COUNT.with(|n| n.set(Some(0)));
    e.process(&mut out, &events);
    e.set_control(0, 1, 1);
    let allocations = COUNT.with(|n| n.replace(None).unwrap());
    assert_eq!(allocations, 0);
    assert_eq!(e.pgs_budget(), budget, "callbacks retain prepared backing");
    assert_eq!(
        call(
            &mut e,
            Action::PgsGetKeyVal,
            Text::from_str_cut("key").0,
            &[1],
            Text::EMPTY
        ),
        HostValue::Int(17)
    );
    assert!(e.unhandled_actions().is_empty());
}
